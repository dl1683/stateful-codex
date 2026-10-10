use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
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
                name: "Repair".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "repair-project".to_string(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    Ok((home, server, project.project.id, thread, responses_server))
}

async fn read(
    server: &mut TestAppServer,
    thread: &str,
    project: &str,
) -> Result<StatefulMemoryReadResponse> {
    server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.to_string(),
                expected_project_id: project.to_string(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await
}

#[tokio::test]
async fn unpersisted_binding_refuses_without_flushing_stale_metadata() -> Result<()> {
    let (home, mut server, project, durable_thread, _responses_server) = setup().await?;
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let request = server.send_request("statefulMemory/add", Some(json!({"threadId":fresh,"expectedProjectId":project,"kind":"note","content":"Do not materialize a staged binding.","clientActionId":"staged-binding"}))).await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request))
        .await?;
    assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
    // Another metadata writer establishes a different durable binding while this host
    // still has its original staged patch. Memory must neither flush nor overwrite it.
    let other: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Independent writer".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "independent-binding".to_string(),
            },
        })
        .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    // Clone valid metadata into this fresh ID through an independent database writer.
    // This leaves the original host's pending patch intact and exercises admission alone.
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('threads')")
        .fetch_all(&pool)
        .await?;
    let selected = columns
        .iter()
        .map(|column| match column.as_str() {
            "id" | "project_id" => "?".to_string(),
            _ => format!("\"{column}\""),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let named = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    // Audited identifiers come exclusively from this test database's migrated schema;
    // both replacement values and the source ID remain bound parameters.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO threads ({named}) SELECT {selected} FROM threads WHERE id = ?"
    )))
    .bind(&fresh)
    .bind(&other.project.id)
    .bind(&durable_thread)
    .execute(&pool)
    .await?;
    let request = server
        .send_request(
            "statefulMemory/read",
            Some(json!({"threadId":fresh,"expectedProjectId":project})),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request))
        .await?;
    assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
    let binding: String = sqlx::query_scalar("SELECT project_id FROM threads WHERE id = ?")
        .bind(&fresh)
        .fetch_one(&pool)
        .await?;
    assert_eq!(binding, other.project.id);
    assert_eq!(
        read(&mut server, &durable_thread, &project).await?.data,
        Vec::new()
    );
    Ok(())
}

#[tokio::test]
async fn submitted_project_controls_refuse_after_same_thread_rebind_or_unlink() -> Result<()> {
    let (home, mut server, project, thread, _responses_server) = setup().await?;
    let _submitted_binding = read(&mut server, &thread, &project).await?;
    let other: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Other".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "other-project".to_string(),
            },
        })
        .await?;
    for binding in [other.project.id, String::new()] {
        let update = server
            .send_request(
                "thread/metadata/update",
                Some(json!({"threadId":thread, "projectId":binding})),
            )
            .await?;
        let _: codex_app_server_protocol::ThreadMetadataUpdateResponse =
            server.read_response(update).await?;
        for (method, fields) in [
            (
                "add",
                json!({"kind":"rule", "content":"Never push.", "clientActionId":"submitted-on-original"}),
            ),
            ("forget", json!({"entryId":"absent", "expectedRevision":1})),
            (
                "correct",
                json!({"entryId":"absent", "expectedRevision":1, "content":"Replacement"}),
            ),
            ("scope", json!({"action":"leave"})),
            ("read", json!({})),
        ] {
            let mut params = fields;
            params["threadId"] = json!(thread);
            params["expectedProjectId"] = json!(project);
            let request = server
                .send_request(&format!("statefulMemory/{method}"), Some(params))
                .await?;
            let error = server
                .read_stream_until_error_message(RequestId::Integer(request))
                .await?;
            assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
        }
    }
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM blackboard_entries), (SELECT COUNT(*) FROM memory_changes)",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(counts, (0, 0));
    Ok(())
}

#[tokio::test]
async fn shortened_model_entry_has_revision_pinned_exact_text_recovery() -> Result<()> {
    let (_home, mut server, project, thread, responses_server) = setup().await?;
    let words = format!(
        "Decision qualification: {} final reason.",
        "αβ ".repeat(600)
    );
    let log = responses::mount_sse_sequence(&responses_server, vec![
        responses::sse(vec![responses::ev_function_call("record-long", "blackboard_record_batch", &json!({"records":[{
            "idempotencyKey":"long-note", "kind":"note", "content":words, "confidenceBasisPoints":10000,
            "verification":"unverified", "importance":"normal", "rootPromotion":"notPromoted"
        }]}).to_string()), responses::ev_completed("recorded")]),
        responses::sse(vec![responses::ev_completed("done")]),
    ]).await;
    run_turn(
        &mut server,
        &thread,
        "Keep this reported note without applying it.",
    )
    .await?;
    let page = read(&mut server, &thread, &project).await?;
    assert_eq!(page.data.len(), 1);
    let item = &page.data[0];
    assert!(item.content_truncated);
    assert!(words.starts_with(&item.content));
    assert!(item.content.len() <= 2000);
    let exact_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "read-exact",
                    "blackboard_query",
                    &json!({"entryId":item.entry_id,"expectedEntryRevision":item.revision})
                        .to_string(),
                ),
                responses::ev_completed("read"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Read the exact retained words.").await?;
    let exact: serde_json::Value = serde_json::from_str(
        &exact_log.requests()[1]
            .function_call_output_text("read-exact")
            .expect("exact output"),
    )?;
    assert_eq!(
        (
            exact["content"].clone(),
            exact["complete"].clone(),
            exact["revision"].clone()
        ),
        (json!(words), json!(true), json!(item.revision))
    );
    assert_eq!(log.requests().len(), 2);
    Ok(())
}

/// The legacy public collection endpoint must refuse rather than return an empty complete list.
#[tokio::test]
async fn scope_collection_returns_an_explicit_unsupported_error() -> Result<()> {
    let (_home, mut server, project, thread, _responses_server) = setup().await?;
    let id = server
        .send_request(
            "statefulMemory/scope",
            Some(json!({
                "threadId": thread, "expectedProjectId": project, "action": "list"
            })),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        error.error,
        JSONRPCErrorError {
            code: -32602,
            message: "investigations and scope collection controls are unsupported; stored scoped entries remain history and are held back from application".to_string(),
            data: None,
        }
    );
    Ok(())
}

/// Large historical collections never enter the retained root/review/exact-read routes.
/// A formerly bound open scope and ended scoped words remain history, held back everywhere.
#[tokio::test]
async fn large_scope_history_is_quarantined_and_retained_reads_stay_bounded() -> Result<()> {
    use codex_project_intelligence::BlackboardEntryId;
    use codex_project_intelligence::BlackboardStore;
    use codex_project_intelligence::ChangeOperation;
    use codex_project_intelligence::ChangeOrigin;
    use codex_project_intelligence::ChangeRecord;
    use codex_project_intelligence::KnowledgeAuthority;
    use codex_project_intelligence::KnowledgeCategory;
    use codex_project_intelligence::KnowledgeContext;
    use codex_project_intelligence::RootBlackboardQuery;
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Always preserve exact reasons.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "unscoped-history-control".to_string(),
                background_section: true,
            },
        })
        .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = BlackboardStore::open(&sqlite).await?;
    let unscoped = store
        .get_entry(
            &project,
            &BlackboardEntryId::parse(added.item.entry_id.clone())?,
        )
        .await?
        .expect("direct entry");
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    // Seed historical rows only. Production SQL and migrations are unchanged.
    sqlx::query("WITH RECURSIVE ord(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM ord WHERE n<4095) INSERT INTO knowledge_scopes (project_id,scope_id,kind,title,state,opened_source,ended_source,created_at_ms,updated_at_ms) SELECT ?, 'history-' || n, 'investigation', 'Historical investigation ' || n, CASE WHEN n=0 THEN 'open' ELSE 'ended' END, 'historical-source', CASE WHEN n=0 THEN NULL ELSE 'historical-end' END, n, n FROM ord")
        .bind(&project).execute(&pool).await?;
    sqlx::query("INSERT INTO knowledge_scope_bindings (project_id,thread_id,scope_id,bound_at_ms) VALUES (?,?,'history-0',1)")
        .bind(&project).bind(&thread).execute(&pool).await?;
    for ordinal in 0..300 {
        let mut value = unscoped.value.clone();
        value.content =
            format!("Scoped history marker {ordinal}: preserve these exact historical words.");
        store
            .create_entry_with_context(
                BlackboardEntryId::parse(format!("historical-rule-{ordinal}"))?,
                value.clone(),
                KnowledgeContext {
                    scope_id: Some(format!("history-{ordinal}")),
                    ..KnowledgeContext::new(
                        KnowledgeCategory::Rule,
                        KnowledgeAuthority::HumanDirect,
                    )
                },
                ChangeRecord {
                    operation: ChangeOperation::Saved,
                    origin: ChangeOrigin::HostCapture,
                    category: KnowledgeCategory::Rule,
                    action_id: None,
                    thread_id: Some(thread.clone()),
                    turn_id: None,
                    group_id: None,
                    preview: value.content,
                },
            )
            .await?;
    }
    let (root, quarantine) = store
        .root_projection_for_thread(
            RootBlackboardQuery {
                project_id: project.clone(),
                max_entries: 1,
            },
            &thread,
        )
        .await?;
    assert_eq!(
        (
            root.data
                .into_iter()
                .map(|hit| hit.entry.id.to_string())
                .collect::<Vec<_>>(),
            root.omitted_entries,
            quarantine.scoped_held_back,
            quarantine.legacy_held_back
        ),
        (vec![added.item.entry_id.clone()], 0, 300, 0)
    );
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: project.clone(),
            max_entries: 1,
        })
        .await?;
    assert_eq!(
        root.data
            .iter()
            .map(|hit| hit.entry.id.to_string())
            .collect::<Vec<_>>(),
        vec![added.item.entry_id]
    );
    let page = read(&mut server, &thread, &project).await?;
    assert_eq!(page.data.len(), 50);
    assert!(page.next_cursor.is_some());
    assert!(
        page.data
            .iter()
            .filter(|item| item.entry_id.starts_with("historical-rule-"))
            .all(|item| item.scope_state
                == Some(codex_app_server_protocol::StatefulMemoryScopeState::Unsupported))
    );
    let exact_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "historical-exact",
                    "blackboard_query",
                    &json!({"entryId":"historical-rule-299","expectedEntryRevision":1}).to_string(),
                ),
                responses::ev_completed("history-read"),
            ]),
            responses::sse(vec![responses::ev_completed("history-done")]),
        ],
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "Read the exact historical entry, without applying it.",
    )
    .await?;
    let requests = exact_log.requests();
    let packet = requests[0].body_json().to_string();
    assert!(packet.contains("300 historical scoped entries are held back"));
    assert!(!packet.contains("Scoped history marker"));
    // The user's own rules stay readable through /memory (above) but never through a model
    // tool result.
    let exact = requests[1]
        .function_call_output_text("historical-exact")
        .expect("exact output");
    assert!(!exact.contains("Scoped history marker"), "{exact}");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_scopes WHERE project_id=?")
        .bind(&project)
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 4096);
    Ok(())
}

#[path = "stateful_memory_cut_tests.rs"]
mod cut_tests;

#[path = "stateful_scope_output_cut_tests.rs"]
mod scope_output_cut_tests;
