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
            value.content = "Original short note.".into();
            for i in previous..count {
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
                    .map(|entry| (&entry.entry_id, entry.content.as_str()))
                    .collect::<Vec<_>>(),
                (0..3)
                    .map(|i| format!("fanin-predecessor-{i:08}"))
                    .collect::<Vec<_>>()
                    .iter()
                    .map(|id| (id, "Original short note."))
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
            target = store
                .create_entry(BlackboardEntryId::parse("additional-target")?, value)
                .await?;
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
            } else if case == "agent-human-direct" {
                store
                    .record_context(
                        &target,
                        &KnowledgeContext::new(
                            KnowledgeCategory::Note,
                            KnowledgeAuthority::HumanDirect,
                        ),
                        /*change*/ None,
                    )
                    .await?;
            }
        }
        let arguments = json!({"mutations":[{"action":"retire","entryId":target.id.to_string(),"expectedRevision":target.revision}]}).to_string();
        let log = responses::mount_sse_sequence(
            &responses_server,
            (0..2)
                .flat_map(|_| {
                    [
                        responses::sse(vec![
                            responses::ev_function_call(
                                "model-retire",
                                "blackboard_update_batch",
                                &arguments,
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
            run_turn(
                &mut server,
                &thread,
                "Assess whether this memory is obsolete.",
            )
            .await?;
            let requests = log.requests();
            let output: serde_json::Value = serde_json::from_str(
                &requests[attempt * 2 + 1]
                    .function_call_output_text("model-retire")
                    .expect("model retirement result"),
            )?;
            assert_eq!(
                (output["updated"].clone(), output["failed"].clone()),
                (json!(0), json!(1)),
                "case={case}"
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
        assert!(server.shutdown_gracefully().await?.success());
    }
    Ok(())
}
