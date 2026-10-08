use super::*;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn public_predecessor_fan_in_preview_is_bounded_after_restart() -> Result<()> {
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
                name: "Fan-in".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "fanin-project".into(),
            },
        })
        .await?;
    let project_id = project.project.id;
    let mut thread = start_thread(&mut server, &project_id).await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project_id.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Seed template.".into(),
                scope: None,
                reason: None,
                client_action_id: "fanin-template".into(),
                background_section: true,
            },
        })
        .await?;
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                expected_project_id: project_id.clone(),
                thread_id: thread.clone(),
                entry_id: added.item.entry_id.clone(),
                expected_revision: added.item.revision,
            },
        })
        .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let successor = BlackboardEntryId::parse("fanin-successor")?;
    let mut previous = 0;
    for count in [64, 4096] {
        {
            let store = BlackboardStore::open(&sqlite).await?;
            let mut value = store
                .get_entry(
                    &project_id,
                    &BlackboardEntryId::parse(added.item.entry_id.clone())?,
                )
                .await?
                .unwrap()
                .value;
            value.kind = codex_project_intelligence::BlackboardKind::Note;
            value.provenance.kind = codex_project_intelligence::BlackboardProvenanceKind::Agent;
            value.content = "Current short note.".into();
            store.create_entry(successor.clone(), value.clone()).await?;
            value.root_promotion = codex_project_intelligence::RootPromotion::NotPromoted;
            for i in previous..count {
                value.content = format!("Original short note {i}.");
                let id = BlackboardEntryId::parse(format!("fanin-predecessor-{i:08}"))?;
                let entry = store.create_entry(id.clone(), value.clone()).await?;
                let update = codex_project_intelligence::BlackboardEntryUpdate {
                    expected_revision: entry.revision,
                    kind: value.kind,
                    content: value.content.clone(),
                    structured_value: value.structured_value.clone(),
                    confidence: value.confidence,
                    verification: value.verification,
                    importance: value.importance,
                    root_promotion: value.root_promotion,
                    evidence: value.evidence.clone(),
                    premises: value.premises.clone(),
                    state: codex_project_intelligence::BlackboardEntryState::Superseded,
                    superseded_by: Some(successor.clone()),
                    provenance: value.provenance.clone(),
                };
                store
                    .update_entry_from_model(&project_id, &id, update)
                    .await?;
            }
        }
        previous = count;
        let before = snapshot(&sqlite).await?;
        for attempt in 0..2 {
            let read: StatefulMemoryReadResponse = server
                .request(|request_id| ClientRequest::StatefulMemoryRead {
                    request_id,
                    params: StatefulMemoryReadParams {
                        expected_project_id: project_id.clone(),
                        thread_id: thread.clone(),
                        cursor: None,
                        limit: Some(1),
                        background_section: true,
                    },
                })
                .await?;
            // A one-item page must still carry only the three selected predecessors.
            let item = read
                .data
                .iter()
                .find(|item| item.entry_id == successor.as_str())
                .unwrap();
            assert_eq!(
                item.replaces
                    .iter()
                    .map(|entry| (entry.entry_id.clone(), entry.content.clone()))
                    .collect::<Vec<_>>(),
                (0..3)
                    .map(|i| (
                        format!("fanin-predecessor-{i:08}"),
                        format!("Original short note {i}.")
                    ))
                    .collect::<Vec<_>>()
            );
            assert_eq!(snapshot(&sqlite).await?, before);
            if attempt == 0 {
                assert!(server.shutdown_gracefully().await?.success());
                server = TestAppServer::builder()
                    .with_codex_home(home.path())
                    .build_initialized()
                    .await?;
                thread = start_thread(&mut server, &project_id).await?;
            }
        }
    }
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

// Compare all PI table cells, including revision history, context, action outcomes and
// journal; this fixture contains only bounded text. Native turn history is separate.
async fn snapshot(sqlite: &SqliteConfig) -> Result<Vec<Vec<String>>> {
    let pool = sqlite
        .open_read_only_pool(
            &sqlite.home().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let mut transaction = pool.begin().await?;
    let tables = sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB 'sqlite_*' ORDER BY name")
        .fetch_all(&mut *transaction).await?;
    let mut result = Vec::new();
    for table in tables {
        // Original turn observations commit independently of refused model mutations.
        // C2 source tests compare these tables too when no new native turn is submitted.
        if [
            "capture_sources",
            "capture_source_chunks",
            "capture_source_terms",
            "capture_source_omissions",
            "knowledge_source_sequences",
        ]
        .contains(&table.as_str())
        {
            continue;
        }
        let columns =
            sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut *transaction)
                .await?;
        let fields = columns
            .iter()
            .map(|name| format!("quote(\"{name}\")"))
            .collect::<Vec<_>>()
            .join(",");
        let rows = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
            "SELECT json_array({fields}) AS cells FROM \"{table}\" ORDER BY cells"
        )))
        .fetch_all(&mut *transaction)
        .await?;
        result.push(rows);
    }
    transaction.commit().await?;
    pool.close().await;
    Ok(result)
}

#[tokio::test]
async fn public_user_memory_survives_model_retirement_and_cold_retry() -> Result<()> {
    for case in [
        "public-add",
        "corrected-agent",
        "legacy-user",
        "agent-human-direct",
        "public-import",
        "old-import-agent",
        "legacy-unknown",
    ] {
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
                    name: "Protected memory".into(),
                    roots: Vec::new(),
                    metadata: None,
                    idempotency_key: "protected-project".into(),
                },
            })
            .await?;
        let project_id = project.project.id;
        let thread = start_thread(&mut server, &project_id).await?;
        let added: StatefulMemoryAddResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryAdd {
                request_id,
                params: StatefulMemoryAddParams {
                    expected_project_id: project_id.clone(),
                    thread_id: thread.clone(),
                    kind: StatefulMemoryAddKind::Rule,
                    content: "Never push.".into(),
                    scope: None,
                    reason: None,
                    client_action_id: "protected-add".into(),
                    background_section: true,
                },
            })
            .await?;
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let store = BlackboardStore::open(&sqlite).await?;
        let mut target = store
            .get_entry(
                &project_id,
                &BlackboardEntryId::parse(added.item.entry_id.clone())?,
            )
            .await?
            .unwrap();
        if case != "public-add" {
            let mut value = target.value.clone();
            value.provenance.kind = if case == "legacy-user" {
                codex_project_intelligence::BlackboardProvenanceKind::User
            } else {
                codex_project_intelligence::BlackboardProvenanceKind::Agent
            };
            value.kind = codex_project_intelligence::BlackboardKind::Note;
            value.content = "Assistant-origin note before correction.".into();
            if case == "public-import" || case == "old-import-agent" {
                let imported: codex_app_server_protocol::BlackboardUpsertResponse = server
                    .request(|request_id| ClientRequest::BlackboardUpsert {
                        request_id,
                        params: codex_app_server_protocol::BlackboardUpsertParams {
                            project_id: project_id.clone(),
                            entry_id: "import-note".into(),
                            expected_revision: None,
                            node_id: Some(value.node_id.to_string()),
                            kind: BlackboardKind::Note,
                            content: "Use local persistence.".into(),
                            structured_value: None,
                            confidence_basis_points: 9000,
                            verification:
                                codex_app_server_protocol::BlackboardVerification::Unverified,
                            importance: codex_app_server_protocol::BlackboardImportance::High,
                            root_promotion:
                                codex_app_server_protocol::BlackboardRootPromotion::Promoted,
                            evidence: Vec::new(),
                            premises: None,
                            provenance: codex_app_server_protocol::BlackboardProvenance {
                                kind: BlackboardProvenanceKind::Import,
                                source_id: "public-import".into(),
                            },
                            state: None,
                            superseded_by: None,
                        },
                    })
                    .await?;
                target = store
                    .get_entry(&project_id, &BlackboardEntryId::parse(imported.entry.id)?)
                    .await?
                    .unwrap();
                assert_eq!(store.knowledge_policy(&project_id, &target.id).await?, None);
                if case == "old-import-agent" {
                    // Persist the base binary's Import rev1 -> Agent rev2 transition
                    // directly through its host writer, keeping the imported text intact.
                    target = store
                        .update_entry(
                            &project_id,
                            &target.id,
                            codex_project_intelligence::BlackboardEntryUpdate {
                                expected_revision: target.revision,
                                kind: codex_project_intelligence::BlackboardKind::Fact,
                                content: target.value.content.clone(),
                                structured_value: target.value.structured_value.clone(),
                                confidence: target.value.confidence,
                                verification: target.value.verification,
                                importance: target.value.importance,
                                root_promotion: target.value.root_promotion,
                                evidence: target.value.evidence.clone(),
                                premises: target.value.premises.clone(),
                                state: codex_project_intelligence::BlackboardEntryState::Active,
                                superseded_by: None,
                                provenance: codex_project_intelligence::BlackboardProvenance {
                                    kind:
                                        codex_project_intelligence::BlackboardProvenanceKind::Agent,
                                    source_id: "old-model-call".into(),
                                },
                            },
                        )
                        .await?;
                    assert_eq!(target.revision, 2);
                    assert_eq!(target.value.content, "Use local persistence.");
                }
            } else {
                target = store
                    .create_entry(BlackboardEntryId::parse("additional-target")?, value)
                    .await?;
            }
            if case == "corrected-agent" {
                let corrected: StatefulMemoryCorrectResponse = server
                    .request(|request_id| ClientRequest::StatefulMemoryCorrect {
                        request_id,
                        params: StatefulMemoryCorrectParams {
                            expected_project_id: project_id.clone(),
                            thread_id: thread.clone(),
                            entry_id: target.id.to_string(),
                            expected_revision: target.revision,
                            content: "The user's corrected note.".into(),
                            background_section: true,
                        },
                    })
                    .await?;
                target = store
                    .get_entry(
                        &project_id,
                        &BlackboardEntryId::parse(corrected.item.entry_id)?,
                    )
                    .await?
                    .unwrap();
            } else if case == "agent-human-direct" || case == "legacy-unknown" {
                store
                    .record_context(
                        &target,
                        &KnowledgeContext::new(
                            KnowledgeCategory::Note,
                            if case == "legacy-unknown" {
                                KnowledgeAuthority::LegacyUnknown
                            } else {
                                KnowledgeAuthority::HumanDirect
                            },
                        ),
                        /*change*/ None,
                    )
                    .await?;
            }
        }
        // Separate registered calls, in the reviewer's exact order, against the same revision.
        let mut actions = vec![
            json!({"mutations":[{"action":"revise","entryId":target.id.to_string(),"expectedRevision":target.revision,"kind":"fact"}]}).to_string(),
            json!({"mutations":[{"action":"retire","entryId":target.id.to_string(),"expectedRevision":target.revision}]}).to_string(),
        ];
        if case == "old-import-agent" {
            let successor = store
                .create_entry(
                    BlackboardEntryId::parse("model-supersede-target")?,
                    target.value.clone(),
                )
                .await?;
            actions.extend([
                json!({"mutations":[{"action":"setRootPromotion","entryId":target.id.to_string(),"expectedRevision":target.revision,"rootPromotion":"candidate"}]}).to_string(),
                json!({"mutations":[{"action":"supersede","entryId":target.id.to_string(),"expectedRevision":target.revision,"successorEntryId":successor.id.to_string()}]}).to_string(),
            ]);
        }
        let log = responses::mount_sse_sequence(
            &responses_server,
            actions
                .iter()
                .cycle()
                .take(actions.len() * 2)
                .flat_map(|arguments| {
                    [
                        responses::sse(vec![
                            responses::ev_function_call(
                                "model-mutate",
                                "blackboard_update_batch",
                                arguments,
                            ),
                            responses::ev_completed("retire-request"),
                        ]),
                        responses::sse(vec![responses::ev_completed("retire-done")]),
                    ]
                })
                .collect(),
        )
        .await;
        let before = snapshot(&sqlite).await?;
        let query = codex_project_intelligence::RootBlackboardQuery {
            project_id: project_id.clone(),
            max_entries: 256,
        };
        let root = store.root_projection(query.clone()).await?;
        for attempt in 0..2 {
            for action in 0..actions.len() {
                run_turn(
                    &mut server,
                    &thread,
                    "Assess the classification and lifecycle of this memory.",
                )
                .await?;
                let requests = log.requests();
                let output: serde_json::Value = serde_json::from_str(
                    &requests[(attempt * actions.len() + action) * 2 + 1]
                        .function_call_output_text("model-mutate")
                        .expect("model retirement result"),
                )?;
                assert_eq!(
                    (output["updated"].clone(), output["failed"].clone()),
                    (json!(0), json!(1)),
                    "case={case}"
                );
                assert!(
                    output["results"][0]["error"]
                        .as_str()
                        .unwrap()
                        .contains("memory")
                );
                assert_eq!(
                    store.get_entry(&project_id, &target.id).await?,
                    Some(target.clone())
                );
                assert_eq!(store.root_projection(query.clone()).await?, root);
                assert_eq!(
                    snapshot(&sqlite).await?,
                    before,
                    "case={case}, attempt={attempt}"
                );
            }
            if attempt == 0 {
                assert!(server.shutdown_gracefully().await?.success());
                server = TestAppServer::builder()
                    .with_codex_home(home.path())
                    .build_initialized()
                    .await?;
                let _: codex_app_server_protocol::ThreadResumeResponse = server
                    .request(|request_id| ClientRequest::ThreadResume {
                        request_id,
                        params: codex_app_server_protocol::ThreadResumeParams {
                            thread_id: thread.clone(),
                            ..Default::default()
                        },
                    })
                    .await?;
            }
        }
        // Explicit user Forget is still available through the actual public control.
        let forgotten: StatefulMemoryForgetResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryForget {
                request_id,
                params: StatefulMemoryForgetParams {
                    expected_project_id: project_id.clone(),
                    thread_id: thread.clone(),
                    entry_id: target.id.to_string(),
                    expected_revision: target.revision,
                },
            })
            .await?;
        assert_eq!(forgotten.revision, target.revision + 1);
        let forgotten_entry = store.get_entry(&project_id, &target.id).await?.unwrap();
        assert_eq!(
            (forgotten_entry.state, forgotten_entry.value),
            (
                codex_project_intelligence::BlackboardEntryState::Tombstoned,
                target.value
            ),
        );
        assert!(server.shutdown_gracefully().await?.success());
    }
    Ok(())
}
