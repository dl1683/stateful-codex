use super::*;
use codex_app_server_protocol::RequestId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::SourceSeal;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn setup() -> Result<(TempDir, TestAppServer, String, String, wiremock::MockServer)> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "C2".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "c2-project".into(),
            },
        })
        .await?;
    // Initialize project hierarchy independently; fault boundaries below own capture state.
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = codex_project_intelligence::HierarchyStore::open(&sqlite).await?;
    let map = codex_project_intelligence::ContextMapStore::open(&sqlite).await?;
    codex_project_intelligence::ProjectIndexer::new(hierarchy, map)
        .ensure_project_node(&project.project.id)
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    Ok((home, server, project.project.id, thread, responses_server))
}

#[tokio::test]
async fn c2_public_original_parts_fork_cold_source_recovery_and_project_switch() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("source-ingress"),
            responses::ev_completed("source-ingress"),
        ]),
    )
    .await;
    let parts = [
        "Notes from 12 June 2024: Mara bought teal M-17, receipt QX-704.\r\n",
        "ÜBER e\u{301} 👩‍🔬. The motor was not damaged.\n",
    ];
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: parts
                .iter()
                .map(|text| UserInput::Text {
                    text: (*text).to_string(),
                    text_elements: Vec::new(),
                })
                .collect(),
            ..Default::default()
        })
        .await?;
    assert_eq!(mock.requests().len(), 1);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let metadata: Vec<String> = sqlx::query_scalar(
        "SELECT metadata FROM capture_sources WHERE project_id = ? ORDER BY part_index",
    )
    .bind(&project)
    .fetch_all(&pool)
    .await?;
    assert_eq!(metadata.len(), 2);
    let seals = metadata
        .iter()
        .map(|json| serde_json::from_str::<SourceSeal>(json))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let store = BlackboardStore::open(&sqlite).await?;
    for (index, seal) in seals.iter().enumerate() {
        assert_eq!(seal.observation.authoritative_thread_id, thread);
        assert_eq!(seal.observation.part_index, index as u32);
        assert_eq!(
            store
                .read_source_range(
                    &project,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    seal.original_utf8_length
                )
                .await?
                .exact_text,
            parts[index]
        );
    }
    let compact_mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_assistant_message(
                "compact-summary",
                "Summary intentionally omits the receipt, identifiers and Unicode details.",
            ),
            responses::ev_completed("compact-summary"),
        ]),
    )
    .await;
    let id = server
        .send_request("thread/compact/start", Some(json!({"threadId": thread})))
        .await?;
    let _: codex_app_server_protocol::ThreadCompactStartResponse = server.read_response(id).await?;
    let _: codex_app_server_protocol::TurnCompletedNotification =
        server.read_notification("turn/completed").await?;
    assert_eq!(compact_mock.requests().len(), 1);
    for (index, seal) in seals.iter().enumerate() {
        assert_eq!(
            store
                .read_source_range(
                    &project,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    seal.original_utf8_length
                )
                .await?
                .exact_text,
            parts[index]
        );
    }
    let fork_id = server
        .send_request("thread/fork", Some(json!({"threadId": thread})))
        .await?;
    let fork: codex_app_server_protocol::ThreadForkResponse = server.read_response(fork_id).await?;
    assert_ne!(fork.thread.id, thread);
    // Fork preserves original source identity; source recovery never substitutes summary bytes.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM capture_sources WHERE project_id = ?")
            .bind(&project)
            .fetch_one(&pool)
            .await?,
        2
    );
    let other: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Q".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "c2-other".into(),
            },
        })
        .await?;
    for binding in [&other.project.id, &project] {
        let id = server
            .send_request(
                "thread/metadata/update",
                Some(json!({"threadId": thread, "projectId": binding})),
            )
            .await?;
        let _: codex_app_server_protocol::ThreadMetadataUpdateResponse =
            server.read_response(id).await?;
    }
    let native_thread = codex_protocol::ThreadId::from_string(&thread)?;
    let admission = codex_state::ThreadProjectAdmission::acquire(&sqlite, native_thread, &project)
        .await?
        .unwrap();
    assert!(admission.binding_generation() > seals[0].observation.binding_generation);
    assert!(
        store
            .read_source_range(
                &other.project.id,
                &seals[0].exact_source_locator,
                &seals[0].digest,
                /*start*/ 0,
                /*end*/ 4
            )
            .await
            .is_err()
    );
    drop(admission);
    let id = server.send_request("statefulMemory/add", Some(json!({"threadId":thread,"expectedProjectId":project,"kind":"note","content":"Explicit switch guard fixture.","clientActionId":"switch-fixture"}))).await?;
    let added: StatefulMemoryAddResponse = server.read_response(id).await?;
    let before = snapshot(&pool).await?;
    let target = codex_project_intelligence::BlackboardEntryId::parse(added.item.entry_id)?;
    let admission = codex_state::ThreadProjectAdmission::acquire(&sqlite, native_thread, &project)
        .await?
        .unwrap();
    assert!(
        store
            .link_entry_source(
                &admission,
                &target,
                added.item.revision,
                &seals[0],
                &seals[0].observation.ordered_spans
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&pool).await?, before);
    drop(admission);
    assert!(server.shutdown_gracefully().await?.success());
    drop(server);
    pool.close().await;
    let mut restarted = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let resume = restarted
        .send_request("thread/resume", Some(json!({"threadId": thread})))
        .await?;
    let _: codex_app_server_protocol::ThreadResumeResponse =
        restarted.read_response(resume).await?;
    for (index, seal) in seals.iter().enumerate() {
        assert_eq!(
            store
                .read_source_range(
                    &project,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    seal.original_utf8_length
                )
                .await?
                .exact_text,
            parts[index]
        );
    }
    Ok(())
}

#[tokio::test]
async fn c2_public_add_correct_forget_faults_preserve_whole_store_and_retry() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("persist"),
            responses::ev_completed("persist"),
        ]),
    )
    .await;
    run_turn(&mut server, &thread, "Native conversation for cold retry.").await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    for (ordinal, table) in [
        "blackboard_entry_revisions",
        "knowledge_context",
        "capture_identity_aliases",
        "capture_action_outcomes",
        "memory_changes",
    ]
    .into_iter()
    .enumerate()
    {
        let action = format!("c2-fault-{ordinal}");
        let params = json!({"threadId":thread,"expectedProjectId":project,"kind":"rule","content":format!("Never push fixture {ordinal}."),"clientActionId":action});
        let before = snapshot(&pool).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER public_fault BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'public fault'); END"))).execute(&pool).await?;
        let id = server
            .send_request("statefulMemory/add", Some(params.clone()))
            .await?;
        let _error = server
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        sqlx::query("DROP TRIGGER public_fault")
            .execute(&pool)
            .await?;
        assert_eq!(snapshot(&pool).await?, before, "{table}");
        let id = server
            .send_request("statefulMemory/add", Some(params.clone()))
            .await?;
        let added: StatefulMemoryAddResponse = server.read_response(id).await?;
        let saved = snapshot(&pool).await?;
        let id = server
            .send_request("statefulMemory/add", Some(params))
            .await?;
        let retry: StatefulMemoryAddResponse = server.read_response(id).await?;
        assert_eq!(retry.outcome, StatefulMemoryAddOutcome::AlreadyDone);
        assert_eq!(snapshot(&pool).await?, saved);
        let mut entry = added.item;
        for method in ["correct", "forget"] {
            let params = if method == "correct" {
                json!({"threadId":thread,"expectedProjectId":project,"entryId":entry.entry_id,"expectedRevision":entry.revision,"content":format!("Never commit fixture {ordinal}."),"clientActionId":format!("correct-{ordinal}")})
            } else {
                json!({"threadId":thread,"expectedProjectId":project,"entryId":entry.entry_id,"expectedRevision":entry.revision,"clientActionId":format!("forget-{ordinal}")})
            };
            let before = snapshot(&pool).await?;
            let (phase_table, operation, predicate) = if method == "correct" {
                match ordinal {
                    0 => ("blackboard_entry_revisions", "INSERT", ""),
                    1 => ("knowledge_context", "INSERT", ""),
                    2 => ("capture_identity_aliases", "INSERT", ""),
                    3 => (
                        "blackboard_entry_revisions",
                        "INSERT",
                        "WHEN NEW.state = 'superseded'",
                    ),
                    _ => ("memory_changes", "INSERT", ""),
                }
            } else {
                match ordinal {
                    0 => (
                        "blackboard_entry_revisions",
                        "INSERT",
                        "WHEN NEW.state = 'tombstoned'",
                    ),
                    1 => ("capture_identity_aliases", "UPDATE", ""),
                    2 => ("capture_identity_aliases", "INSERT", ""),
                    3 => ("blackboard_entries", "UPDATE", ""),
                    _ => ("memory_changes", "INSERT", ""),
                }
            };
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER public_fault BEFORE {operation} ON {phase_table} {predicate} BEGIN SELECT RAISE(ABORT, 'public control fault'); END"))).execute(&pool).await?;
            let id = server
                .send_request(&format!("statefulMemory/{method}"), Some(params.clone()))
                .await?;
            let _error = server
                .read_stream_until_error_message(RequestId::Integer(id))
                .await?;
            sqlx::query("DROP TRIGGER public_fault")
                .execute(&pool)
                .await?;
            assert_eq!(snapshot(&pool).await?, before, "{method}");
            // Retry after rollback commits the whole control, then restart after success
            // and use the original revision to check honest stale-request recovery.
            let id = server
                .send_request(&format!("statefulMemory/{method}"), Some(params.clone()))
                .await?;
            if method == "correct" {
                let corrected: StatefulMemoryCorrectResponse = server.read_response(id).await?;
                entry = corrected.item;
            } else {
                let _: StatefulMemoryForgetResponse = server.read_response(id).await?;
            }
            let committed = snapshot(&pool).await?;
            assert!(server.shutdown_gracefully().await?.success());
            server = TestAppServer::builder()
                .with_codex_home(home.path())
                .build_initialized()
                .await?;
            let id = server
                .send_request("thread/resume", Some(json!({"threadId":thread})))
                .await?;
            let _: codex_app_server_protocol::ThreadResumeResponse =
                server.read_response(id).await?;
            let id = server
                .send_request(&format!("statefulMemory/{method}"), Some(params))
                .await?;
            if method == "correct" {
                let recovered: StatefulMemoryCorrectResponse = server.read_response(id).await?;
                assert_eq!(recovered.item, entry);
            } else {
                let _stale = server
                    .read_stream_until_error_message(RequestId::Integer(id))
                    .await?;
            }
            assert_eq!(snapshot(&pool).await?, committed);
        }
    }
    Ok(())
}

async fn snapshot(pool: &sqlx::SqlitePool) -> Result<Vec<String>> {
    let tables = sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB 'sqlite_*' ORDER BY name").fetch_all(pool).await?;
    let mut result = Vec::new();
    for table in tables {
        let columns =
            sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(pool)
                .await?;
        let fields = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(",");
        result.push(table.clone());
        result.extend(
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
                "SELECT json_array({fields}) AS cells FROM \"{table}\" ORDER BY cells"
            )))
            .fetch_all(pool)
            .await?,
        );
    }
    Ok(result)
}

#[tokio::test]
async fn c2_public_forget_excludes_exact_source_and_linked_copy_after_cold_rebuild() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let text = "Notes from 12 June 2024: Mara bought teal M-17 from Sol, receipt QX-704. No telephone number was supplied.";
    let mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("purchase"),
            responses::ev_completed("purchase"),
        ]),
    )
    .await;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: [text, "Independent original second part."]
                .iter()
                .map(|text| UserInput::Text {
                    text: (*text).to_string(),
                    text_elements: Vec::new(),
                })
                .collect(),
            ..Default::default()
        })
        .await?;
    assert_eq!(mock.requests().len(), 1);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    let metadata: String = sqlx::query_scalar(
        "SELECT metadata FROM capture_sources WHERE project_id = ? AND part_index = 0",
    )
    .bind(&project)
    .fetch_one(&pool)
    .await?;
    let seal: SourceSeal = serde_json::from_str(&metadata)?;
    let params = json!({"threadId": thread, "expectedProjectId": project, "kind": "background", "content": "Purchase unit supported by original receipt evidence.", "clientActionId": "purchase-control"});
    let id = server
        .send_request("statefulMemory/add", Some(params))
        .await?;
    let added: StatefulMemoryAddResponse = server.read_response(id).await?;
    let store = BlackboardStore::open(&sqlite).await?;
    let entry_id =
        codex_project_intelligence::BlackboardEntryId::parse(added.item.entry_id.clone())?;
    let span = codex_project_intelligence::SourceSpan {
        start_byte: 0,
        end_byte: text.len() as u32,
        role: codex_project_intelligence::SourceSpanRole::Body,
    };
    // Fixture linkage uses the host foundation; C2 adds no factual admission producer.
    let admission = codex_state::ThreadProjectAdmission::acquire(
        &sqlite,
        codex_protocol::ThreadId::from_string(&thread)?,
        &project,
    )
    .await?
    .unwrap();
    store
        .link_entry_source(
            &admission,
            &entry_id,
            added.item.revision,
            &seal,
            std::slice::from_ref(&span),
        )
        .await?;
    let links = store
        .entry_source_links(&project, &entry_id, /*after*/ None)
        .await?;
    assert!(links.complete);
    assert_eq!(
        links.links,
        vec![codex_project_intelligence::SourceLink {
            locator: seal.exact_source_locator.clone(),
            digest: seal.digest.clone(),
            source_revision: seal.observation.source_revision,
            span: span.clone()
        }]
    );
    let entry = store.get_entry(&project, &entry_id).await?.unwrap();
    let copy_id = codex_project_intelligence::BlackboardEntryId::parse("linked-reported-copy")?;
    let mut copy = entry.value.clone();
    copy.kind = codex_project_intelligence::BlackboardKind::Note;
    copy.content = "Reported purchase of a teal prototype.".to_string();
    copy.provenance.kind = codex_project_intelligence::BlackboardProvenanceKind::Agent;
    let copy = store.create_entry(copy_id.clone(), copy).await?;
    store
        .link_entry_source(
            &admission,
            &copy_id,
            copy.revision,
            &seal,
            std::slice::from_ref(&span),
        )
        .await?;
    drop(admission);
    assert!(
        !store
            .search_source_ranges(&project, "Mara Sol receipt", /*after*/ None)
            .await?
            .ranges
            .is_empty()
    );
    let id = server.send_request("statefulMemory/forget", Some(json!({"threadId":thread,"expectedProjectId":project,"entryId":added.item.entry_id,"expectedRevision":added.item.revision}))).await?;
    let forgotten: StatefulMemoryForgetResponse = server.read_response(id).await?;
    assert_eq!(forgotten.revision, added.item.revision + 1);
    assert!(
        store
            .get_source_eligible_entry(&project, &copy_id)
            .await?
            .is_none()
    );
    assert!(
        store
            .entry_source_links(&project, &entry_id, /*after*/ None)
            .await
            .is_err()
    );
    let before = snapshot(&pool).await?;
    let mutation = codex_project_intelligence::BlackboardEntryUpdate {
        expected_revision: copy.revision,
        kind: copy.value.kind,
        content: "New interpretation evading the forgotten source.".into(),
        structured_value: None,
        confidence: copy.value.confidence,
        verification: copy.value.verification,
        importance: copy.value.importance,
        root_promotion: copy.value.root_promotion,
        evidence: Vec::new(),
        premises: Vec::new(),
        state: codex_project_intelligence::BlackboardEntryState::Active,
        superseded_by: None,
        provenance: copy.value.provenance,
    };
    assert!(
        store
            .update_entry_from_model(&project, &copy_id, mutation)
            .await
            .is_err()
    );
    assert_eq!(snapshot(&pool).await?, before);
    let recall = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "retired-recall",
                    "memory_read",
                    "{\"query\":\"Mara\"}",
                ),
                responses::ev_completed("recall"),
            ]),
            responses::sse(vec![responses::ev_completed("recall-done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Find older details.").await?;
    let requests = recall.requests();
    assert_eq!(requests.len(), 2);
    let delivered = requests[1]
        .function_call_output_text("retired-recall")
        .unwrap();
    assert!(!delivered.contains("QX-704"));
    assert!(!delivered.contains("M-17"));
    assert!(server.shutdown_gracefully().await?.success());
    drop(server);
    pool.close().await;
    drop(store);
    let cold = BlackboardStore::open(&sqlite).await?;
    cold.begin_source_index_rebuild(&project).await?;
    while !cold.maintain_source_index(&project).await? {}
    for query in ["Mara", "Sol", "receipt", "teal purchase"] {
        assert!(
            cold.search_source_ranges(&project, query, /*after*/ None)
                .await?
                .ranges
                .is_empty()
        );
    }
    assert!(
        cold.read_source_range(
            &project,
            &seal.exact_source_locator,
            &seal.digest,
            /*start*/ 0,
            text.len() as u32
        )
        .await
        .is_err()
    );
    assert!(!cold.source_text_eligible(&project, text).await?);
    assert!(
        !cold
            .source_text_eligible(
                &project,
                &format!("{text}\nIndependent original second part.")
            )
            .await?
    );
    assert!(
        !cold
            .source_turn_eligible(&project, &seal.observation.turn_id)
            .await?
    );
    assert!(
        !cold
            .search_source_ranges(&project, "Independent", /*after*/ None)
            .await?
            .ranges
            .is_empty()
    );
    assert!(cold.get_entry(&project, &entry_id).await?.is_some());
    assert!(
        cold.get_source_eligible_entry(&project, &copy_id)
            .await?
            .is_none()
    );
    let cold_pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let before = snapshot(&cold_pool).await?;
    assert_eq!(
        cold.observe_source(seal.observation.clone(), text).await?,
        seal
    );
    assert_eq!(snapshot(&cold_pool).await?, before);
    Ok(())
}

#[tokio::test]
async fn c2_public_oversized_original_parts_record_native_omission_without_seal() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("omitted"),
            responses::ev_completed("omitted"),
        ]),
    )
    .await;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: (0..9)
                .map(|index| UserInput::Text {
                    text: format!("Original part {index}."),
                    text_elements: Vec::new(),
                })
                .collect(),
            ..Default::default()
        })
        .await?;
    assert_eq!(mock.requests().len(), 1);
    let pool = SqliteConfig::new_for_testing(home.path().abs())
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let metadata: String =
        sqlx::query_scalar("SELECT metadata FROM capture_source_omissions WHERE project_id = ?")
            .bind(&project)
            .fetch_one(&pool)
            .await?;
    let omitted: codex_project_intelligence::SourceObservation = serde_json::from_str(&metadata)?;
    assert_eq!(omitted.authoritative_thread_id, thread);
    assert!(!omitted.complete_envelope);
    assert!(omitted.incomplete_reason.unwrap().contains("eight parts"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM capture_sources WHERE project_id = ?")
            .bind(&project)
            .fetch_one(&pool)
            .await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn c2_public_steering_text_is_an_original_part_with_current_turn_and_own_event() -> Result<()>
{
    use core_test_support::streaming_sse::StreamingSseChunk;
    use core_test_support::streaming_sse::start_streaming_sse_server;
    let (release, gate) = tokio::sync::oneshot::channel();
    let (responses_server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![responses::ev_response_created("first")]),
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: responses::sse(vec![responses::ev_completed("first")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("steered"),
                responses::ev_completed("steered"),
            ]),
        }],
    ])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Steering".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "steering-project".into(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    let first = "Ordinary original text.";
    let steered = "Additional original: ÜBER e\u{301} 👩‍🔬.\r\nNo damage was observed.";
    let id = server
        .send_request(
            "turn/start",
            Some(json!({"threadId":thread,"input":[{"type":"text","text":first}]})),
        )
        .await?;
    let started: codex_app_server_protocol::TurnStartResponse = server.read_response(id).await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 10),
        responses_server.wait_for_request_count(/*count*/ 1),
    )
    .await?;
    let id = server.send_request("turn/steer", Some(json!({"threadId":thread,"expectedTurnId":started.turn.id,"input":[{"type":"text","text":steered}]}))).await?;
    let _: codex_app_server_protocol::TurnSteerResponse = server.read_response(id).await?;
    release.send(()).unwrap();
    let _: codex_app_server_protocol::TurnCompletedNotification =
        server.read_notification("turn/completed").await?;
    assert_eq!(responses_server.requests().await.len(), 2);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let metadata: Vec<String> = sqlx::query_scalar(
        "SELECT metadata FROM capture_sources WHERE project_id = ? ORDER BY observed_sequence",
    )
    .bind(&project.project.id)
    .fetch_all(&pool)
    .await?;
    let seals = metadata
        .iter()
        .map(|json| serde_json::from_str::<SourceSeal>(json))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(seals.len(), 2);
    assert_ne!(
        seals[0].observation.original_event_id,
        seals[1].observation.original_event_id
    );
    let store = BlackboardStore::open(&sqlite).await?;
    for (seal, text) in seals.iter().zip([first, steered]) {
        assert_eq!(seal.observation.turn_id, started.turn.id);
        assert_eq!(seal.observation.authoritative_thread_id, thread);
        assert_eq!(
            store
                .read_source_range(
                    &project.project.id,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    text.len() as u32
                )
                .await?
                .exact_text,
            text
        );
    }
    Ok(())
}
