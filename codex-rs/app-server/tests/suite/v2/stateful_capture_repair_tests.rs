//! Public model delivery witnesses for C2 repair findings 1–3.
use super::capture_sources_tests::setup;
use super::model_retirement_tests::snapshot;
use super::*;
use codex_project_intelligence as pi;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[path = "stateful_capture_repair2_tests.rs"]
mod repair2_tests;

#[path = "stateful_capture_input_repair3_tests.rs"]
mod input_repair3_tests;

#[path = "stateful_capture_exact_repair3_tests.rs"]
mod exact_repair3_tests;

#[tokio::test]
async fn c3r1_public_large_proposal_decode_error_is_bounded_and_store_unchanged() -> Result<()> {
    malformed_proposal(ProposalDelivery::Direct).await
}

#[tokio::test]
async fn c3r1_public_code_mode_session_large_proposal_decode_error_is_bounded() -> Result<()> {
    malformed_proposal(ProposalDelivery::CodeModeSession).await
}

enum ProposalDelivery {
    Direct,
    CodeModeSession,
}

async fn malformed_proposal(delivery: ProposalDelivery) -> Result<()> {
    let (home, mut server, project, mut thread, responses_server) = setup().await?;
    if matches!(delivery, ProposalDelivery::CodeModeSession) {
        assert!(server.shutdown_gracefully().await?.success());
        MockResponsesConfig::new(&responses_server.uri())
            .enable_feature(Feature::Sqlite)
            .enable_feature(Feature::CodeModeOnly)
            .write(home.path())?;
        server = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        thread = start_thread(&mut server, &project).await?;
    }
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let before = snapshot(&sqlite).await?;
    for key in [
        "!~@#$%^&*()-+=|:;<>?".repeat(1000),
        "\"\\\n".repeat(2000),
        "😀é".repeat(2500),
    ] {
        let mut arguments = json!({"type":"sourceProposal","records":[]});
        arguments[&key] = json!(1);
        assert!(arguments.to_string().len() < 32768);
        let error = model_output(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            arguments,
        )
        .await?;
        assert!(error.starts_with("invalid tool arguments"), "{error}");
        assert!(serde_json::to_string(&error)?.len() <= 9000);
        assert_eq!(snapshot(&sqlite).await?, before);
    }
    assert!(server.shutdown_gracefully().await?.success());
    let reopened = pi::BlackboardStore::open(&sqlite).await?;
    assert_eq!(snapshot(&sqlite).await?, before);
    assert!(
        reopened
            .query(pi::BlackboardQuery {
                project_id: project,
                text: None,
                within_node: None,
                root_promotion: None,
                entry_scope: pi::BlackboardEntryScope::Active,
                max_results: 10,
            })
            .await?
            .data
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn c3r2_public_automatic_proposal_recall_cut_and_storage_replay_cold() -> Result<()> {
    let (home, mut server, project, _, responses_server) = setup().await?;
    assert!(server.shutdown_gracefully().await?.success());
    let config = core_test_support::load_default_config_for_test(&home).await;
    let model = codex_core::test_support::construct_model_info_offline("mock-model", &config);
    let catalog = home.path().join("query-models.json");
    let models: Vec<_> = [("query-small", 300), ("query-large", 9000)]
        .into_iter()
        .map(|(slug, limit)| {
            let mut model = model.clone();
            model.slug = slug.into();
            model.truncation_policy =
                codex_protocol::openai_models::TruncationPolicyConfig::bytes(limit);
            model
        })
        .collect();
    std::fs::write(&catalog, serde_json::to_vec(&json!({"models":models}))?)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .with_model("query-large")
        .with_root_config(&format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog)?
        ))
        .write(home.path())?;
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let thread = start_thread(&mut server, &project).await?;
    let mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![responses::ev_completed("observe")]),
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "Mara bought the teal prototype; motor was not damaged.",
    )
    .await?;
    assert_eq!(mock.requests().len(), 1);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let metadata: String =
        sqlx::query_scalar("SELECT metadata FROM capture_sources WHERE project_id = ? LIMIT 1")
            .bind(&project)
            .fetch_one(&pool)
            .await?;
    let seal: pi::SourceSeal = serde_json::from_str(&metadata)?;
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let hierarchy = pi::HierarchyStore::open(&sqlite).await?;
    let node = hierarchy.project_node(&project).await?.unwrap().id;
    let admission = codex_state::ThreadProjectAdmission::acquire(
        &sqlite,
        codex_protocol::ThreadId::from_string(&thread)?,
        &project,
    )
    .await?
    .unwrap();
    let proposal: pi::SourceProposal = serde_json::from_value(json!({
        "sourceId":seal.exact_source_locator,"digest":seal.digest,"sourceRevision":1,"partIndex":0,
        "spans":[{"startByte":0,"endByte":seal.original_utf8_length,"role":"body"}],
        "category":"background","interpretation":"budget witness prototype purchase"
    }))?;
    let result = store
        .propose_sources(&admission, node, &seal.observation.turn_id, vec![proposal])
        .await?;
    drop(admission);
    let id = result[0].entry_id.as_ref().unwrap();
    let before = snapshot(&sqlite).await?;
    for round in 0..2 {
        if round == 1 {
            reopen(&mut server, &home, &thread).await?;
        }
        for (model, budget) in [("query-small", 360), ("query-large", 9000)] {
            let call_id = format!("query-{round}-{model}");
            let mock = responses::mount_sse_sequence(
                &responses_server,
                vec![
                    responses::sse(vec![
                        responses::ev_function_call(
                            &call_id,
                            "blackboard_query",
                            &json!({"text":"budget witness"}).to_string(),
                        ),
                        responses::ev_completed("query"),
                    ]),
                    responses::sse(vec![responses::ev_completed("done")]),
                ],
            )
            .await;
            server
                .start_turn_and_wait_for_completion(TurnStartParams {
                    thread_id: thread.clone(),
                    model: Some(model.into()),
                    input: vec![UserInput::Text {
                        text: "Query the retained proposal.".into(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                })
                .await?;
            let requests = mock.requests();
            assert_eq!(requests.len(), 2);
            let output = requests[1].function_call_output_text(&call_id).unwrap();
            assert!(output.len() <= budget);
            let output: Value = serde_json::from_str(&output)?;
            assert_eq!(
                (output["data"].clone(), output["truncated"].clone()),
                (json!([]), json!(false))
            );
            if model == "query-large" {
                let output = model_output(
                    &mut server,
                    &responses_server,
                    &thread,
                    "blackboard_query",
                    json!({"entryId":id}),
                )
                .await?;
                assert!(output.contains("entry not found"), "{output}");
                let output = model_call(
                    &mut server,
                    &responses_server,
                    &thread,
                    "memory_read",
                    json!({"question":"budget witness", "includeHistory":false}),
                )
                .await?;
                assert_eq!(output["entries"], json!([]));
            }
            assert_eq!(snapshot(&sqlite).await?, before);
        }
    }
    Ok(())
}

async fn model_call(
    server: &mut TestAppServer,
    responses_server: &wiremock::MockServer,
    thread: &str,
    tool: &str,
    arguments: Value,
) -> Result<Value> {
    Ok(serde_json::from_str(
        &model_output(server, responses_server, thread, tool, arguments).await?,
    )?)
}

async fn model_output(
    server: &mut TestAppServer,
    responses_server: &wiremock::MockServer,
    thread: &str,
    tool: &str,
    arguments: Value,
) -> Result<String> {
    let call_id = format!("repair-{}", uuid::Uuid::new_v4());
    model_output_with_id(server, responses_server, thread, tool, arguments, &call_id).await
}

async fn model_output_with_id(
    server: &mut TestAppServer,
    responses_server: &wiremock::MockServer,
    thread: &str,
    tool: &str,
    arguments: Value,
    call_id: &str,
) -> Result<String> {
    let log = responses::mount_sse_sequence(
        responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(call_id, tool, &arguments.to_string()),
                responses::ev_completed("repair-request"),
            ]),
            responses::sse(vec![responses::ev_completed("repair-done")]),
        ],
    )
    .await;
    run_turn(server, thread, "Inspect the retained project memory.").await?;
    let requests = log.requests();
    assert_eq!(requests.len(), 2);
    let body = requests[1].body_json();
    Ok(body["input"]
        .as_array()
        .expect("model input array")
        .iter()
        .rev()
        .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
        .expect("actual latest model-delivered result")["output"]
        .as_str()
        .expect("text tool output")
        .to_string())
}

fn record(key: &str, content: &str) -> Value {
    json!({"idempotencyKey":key,"kind":"note","content":content,
        "confidenceBasisPoints":9000,"verification":"unverified",
        "importance":"high","rootPromotion":"promoted",
        "evidence":[],"premises":[],"supersedes":[]})
}

async fn reopen(server: &mut TestAppServer, home: &TempDir, thread: &str) -> Result<()> {
    assert!(server.shutdown_gracefully().await?.success());
    *server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let _: codex_app_server_protocol::ThreadResumeResponse = server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: codex_app_server_protocol::ThreadResumeParams {
                thread_id: thread.to_string(),
                ..Default::default()
            },
        })
        .await?;
    Ok(())
}

async fn forget(
    server: &mut TestAppServer,
    project: &str,
    thread: &str,
    id: &str,
    revision: u64,
) -> Result<()> {
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                expected_project_id: project.to_string(),
                thread_id: thread.to_string(),
                entry_id: id.to_string(),
                expected_revision: revision,
            },
        })
        .await?;
    Ok(())
}

async fn retirement_payload_witness(words: &str, payload: Value) -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let copy = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[payload.clone()]}),
    )
    .await?;
    assert_eq!(copy["recorded"], json!(1));
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: words.to_string(),
                scope: None,
                reason: None,
                client_action_id: "retired-payload".to_string(),
                background_section: true,
            },
        })
        .await?;
    forget(
        &mut server,
        &project,
        &thread,
        &added.item.entry_id,
        added.item.revision,
    )
    .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let root_query = pi::RootBlackboardQuery {
        project_id: project.clone(),
        max_entries: 256,
    };
    for attempt in 0..2 {
        let before = snapshot(&sqlite).await?;
        let mut retry = payload.clone();
        retry["idempotencyKey"] = json!(format!("fresh-copy-{attempt}"));
        let result = model_call(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            json!({"records":[retry]}),
        )
        .await?;
        assert_eq!(
            (result["recorded"].clone(), result["failed"].clone()),
            (json!(0), json!(1))
        );
        assert!(result.to_string().contains("retired"));
        assert_eq!(snapshot(&sqlite).await?, before);
        for scope in ["active", "all", "historical"] {
            let output = model_call(
                &mut server,
                &responses_server,
                &thread,
                "blackboard_query",
                json!({"entryScope":scope,"detail":"full","limit":50}),
            )
            .await?;
            assert!(
                !output.to_string().contains(words),
                "delivered output: {output}"
            );
        }
        let root = store.root_projection(root_query.clone()).await?;
        assert!(root.data.iter().all(|hit| {
            !hit.entry.value.content.contains(words)
                && hit
                    .entry
                    .value
                    .structured_value
                    .as_ref()
                    .is_none_or(|value| {
                        !value.value.contains(words)
                            && value.unit.as_ref().is_none_or(|unit| !unit.contains(words))
                    })
        }));
        assert!(!store.source_text_eligible(&project, words).await?);
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    let mut independent = record("independent-payload", "Independent cedar kits.");
    independent["structuredValue"] = json!({"value":"18","unit":"kits"});
    let result = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[independent]}),
    )
    .await?;
    assert_eq!(result["recorded"], json!(1));
    let output = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_query",
        json!({"entryScope":"active","detail":"full","limit":1}),
    )
    .await?;
    assert!(output.to_string().contains("Independent cedar kits."));
    assert!(!output.to_string().contains(words));
    let restored: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: words.to_string(),
                scope: None,
                reason: None,
                client_action_id: "fresh-deliberate-restore".to_string(),
                background_section: true,
            },
        })
        .await?;
    assert_ne!(restored.item.entry_id, added.item.entry_id);
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

#[tokio::test]
async fn c2r2_public_symbol_identity_refuses_repeat_query_root_and_cold_retry() -> Result<()> {
    retirement_payload_witness("🛑", record("old-symbol-copy", "🛑")).await
}

#[tokio::test]
async fn c2r2_public_structured_copy_refuses_repeat_full_query_root_and_cold_retry() -> Result<()> {
    let mut payload = record("old-structured-copy", "Purchase receipt identifier.");
    payload["structuredValue"] = json!({"value":"QX704","unit":null});
    retirement_payload_witness("QX704", payload).await
}

#[tokio::test]
async fn c2r3_public_incomplete_upgrade_withholds_legacy_word_cycle_after_restart() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let seed = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("legacy-A", "Old meridian fact.")]}),
    )
    .await?;
    assert_eq!(seed["recorded"], json!(1));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let root_query = pi::RootBlackboardQuery {
        project_id: project.clone(),
        max_entries: 256,
    };
    let id = store.root_projection(root_query.clone()).await?.data[0]
        .entry
        .id
        .clone();
    // Faithful pre-C2 revision seeding: old binaries allowed Active A -> B -> A.
    // Only this isolated fixture is seeded; maintenance must not rewrite its revisions.
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join("project_intelligence_1.sqlite"))
        .await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for (revision, content) in [(2_i64, "Current cedar fact."), (3, "Old meridian fact.")] {
        sqlx::query("UPDATE blackboard_entries SET revision = ? WHERE id = ?")
            .bind(revision)
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO blackboard_entry_revisions SELECT entry_id, ?, kind, ?, structured_value, structured_unit, confidence_basis_points, verification, importance, root_promotion, state, superseded_by, provenance_kind, provenance_source_id, recorded_at_ms, agent_run_id FROM blackboard_entry_revisions WHERE entry_id = ? AND revision = 1")
            .bind(revision).bind(content).bind(id.as_str()).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM capture_identity_aliases WHERE entry_id = ?")
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM capture_current_words WHERE entry_id = ?")
        .bind(id.as_str())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO capture_identity_coverage(project_id, watermark) SELECT ?, MAX(revision.rowid) FROM blackboard_entry_revisions AS revision JOIN blackboard_entries AS entry ON entry.id = revision.entry_id WHERE entry.project_id = ? ON CONFLICT(project_id) DO UPDATE SET after_rowid = 0, watermark = excluded.watermark")
        .bind(&project).bind(&project).execute(&mut *tx).await?;
    tx.commit().await?;
    pool.close().await;
    let legacy = store.get_entry(&project, &id).await?;
    assert!(!store.maintain_capture_identities(&project).await?);
    assert_eq!(store.get_entry(&project, &id).await?, legacy);
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: "Old meridian fact.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "legacy-strengthening-add".to_string(),
                background_section: true,
            },
        })
        .await?;
    forget(
        &mut server,
        &project,
        &thread,
        &added.item.entry_id,
        added.item.revision,
    )
    .await?;
    for attempt in 0..2 {
        let before = snapshot(&sqlite).await?;
        let output = model_output(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_query",
            json!({"entryScope":"all","detail":"full","limit":50}),
        )
        .await?;
        assert!(!output.contains("Old meridian fact."));
        assert!(output.contains("coverage"), "{output}");
        let exact = model_output(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_query",
            json!({"entryId":id.to_string(),"detail":"full"}),
        )
        .await?;
        assert!(!exact.contains("Old meridian fact."));
        let result = model_call(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            json!({"records":[record(&format!("legacy-repeat-{attempt}"), "Old meridian fact.")]}),
        )
        .await?;
        assert_eq!(
            (result["recorded"].clone(), result["failed"].clone()),
            (json!(0), json!(1))
        );
        assert_eq!(snapshot(&sqlite).await?, before);
        assert_eq!(store.get_entry(&project, &id).await?, legacy);
        assert!(matches!(
            store.root_projection(root_query.clone()).await,
            Err(pi::BlackboardStoreError::IdentityCoverageIncomplete)
        ));
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

#[tokio::test]
async fn c2r1_public_heading_container_refuses_after_forget_and_cold_retry() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Never push.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "retired-rule".to_string(),
                background_section: true,
            },
        })
        .await?;
    forget(
        &mut server,
        &project,
        &thread,
        &added.item.entry_id,
        added.item.revision,
    )
    .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let query = pi::RootBlackboardQuery {
        project_id: project.clone(),
        max_entries: 256,
    };
    let root = store.root_projection(query.clone()).await?;
    let before = snapshot(&sqlite).await?;
    for attempt in 0..2 {
        let result = model_call(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            json!({"records":[record("heading-note", "Rule: Never push.")]}),
        )
        .await?;
        assert_eq!(
            (result["recorded"].clone(), result["failed"].clone()),
            (json!(0), json!(1))
        );
        assert!(result.to_string().contains("retired"));
        assert_eq!(snapshot(&sqlite).await?, before);
        assert_eq!(store.root_projection(query.clone()).await?, root);
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    let result = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("independent-note", "Cedar kit remains independent.")]}),
    )
    .await?;
    assert_eq!(
        (result["recorded"].clone(), result["failed"].clone()),
        (json!(1), json!(0))
    );
    let restored: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Never push.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "fresh-deliberate-add".to_string(),
                background_section: true,
            },
        })
        .await?;
    assert_ne!(restored.item.entry_id, added.item.entry_id);
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

#[tokio::test]
async fn c2r1_public_query_relations_exclude_retired_endpoint_and_copied_note() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let result = model_call(&mut server, &responses_server, &thread, "blackboard_record_batch", json!({"records":[
        record("owner", "Anchor meridian entry."), record("forgotten", "Private receipt QX704."), record("independent", "Independent cedar entry.")
    ]})).await?;
    assert_eq!(result["recorded"], json!(3));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let entries = store
        .query(pi::BlackboardQuery {
            project_id: project.clone(),
            text: None,
            within_node: None,
            root_promotion: None,
            entry_scope: pi::BlackboardEntryScope::Active,
            max_results: 20,
        })
        .await?
        .data;
    let find = |words: &str| {
        entries
            .iter()
            .find(|hit| hit.entry.value.content == words)
            .unwrap()
            .entry
            .clone()
    };
    let owner = find("Anchor meridian entry.");
    let retired = find("Private receipt QX704.");
    let independent = find("Independent cedar entry.");
    for (key, counterpart, note) in [
        ("retired-endpoint", &retired.id, "Private receipt QX704."),
        (
            "copied-note",
            &independent.id,
            "Heading: Private receipt QX704.",
        ),
        (
            "eligible-relation",
            &independent.id,
            "Cedar remains independent.",
        ),
    ] {
        let output = model_call(&mut server, &responses_server, &thread, "blackboard_relate", json!({"idempotencyKey":key,"fromEntryId":owner.id.to_string(),"toEntryId":counterpart.to_string(),"kind":"relatedTo","note":note,"confidenceBasisPoints":9000})).await?;
        assert!(output["relationId"].is_string(), "{output}");
    }
    forget(
        &mut server,
        &project,
        &thread,
        retired.id.as_str(),
        retired.revision,
    )
    .await?;
    for attempt in 0..2 {
        let before = snapshot(&sqlite).await?;
        let output = model_call(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_query",
            json!({"text":"Anchor meridian","detail":"full"}),
        )
        .await?;
        assert_eq!(output["data"].as_array().unwrap().len(), 1);
        let relations = output["data"][0]["relations"].as_array().unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0]["note"], json!("Cedar remains independent."));
        assert!(!output.to_string().contains("QX704"));
        let memory = model_call(
            &mut server,
            &responses_server,
            &thread,
            "memory_read",
            json!({"question":"Anchor meridian"}),
        )
        .await?;
        assert!(!memory.to_string().contains("QX704"));
        assert_eq!(
            store
                .list_relations(&project, &owner.id, /*max_results*/ 256)
                .await?
                .len(),
            3
        );
        assert_eq!(snapshot(&sqlite).await?, before);
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

#[tokio::test]
async fn c2r1_public_evidence_query_excludes_forgotten_before_limit_and_cold_reopen() -> Result<()>
{
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let seed = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("template", "Template agent note.")]}),
    )
    .await?;
    assert_eq!(seed["recorded"], json!(1));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let template = store
        .query(pi::BlackboardQuery {
            project_id: project.clone(),
            text: None,
            within_node: None,
            root_promotion: None,
            entry_scope: pi::BlackboardEntryScope::Active,
            max_results: 20,
        })
        .await?
        .data
        .remove(0)
        .entry
        .value;
    let hierarchy = pi::HierarchyStore::open(&sqlite).await?;
    let root_node = pi::HierarchyNodeId::parse("evidence-root")?;
    hierarchy
        .create_node(
            root_node.clone(),
            pi::NewHierarchyNode {
                project_id: project.clone(),
                parent_id: Some(template.node_id.clone()),
                kind: pi::NodeKind::Directory,
                project_root: Some(home.path().to_string_lossy().into_owned()),
                relative_path: pi::ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let source_node = pi::HierarchyNodeId::parse("evidence-file")?;
    let fingerprint = pi::SourceFingerprint::parse("sha256:fixture")?;
    hierarchy
        .create_node(
            source_node.clone(),
            pi::NewHierarchyNode {
                project_id: project.clone(),
                parent_id: Some(root_node),
                kind: pi::NodeKind::File,
                project_root: Some(home.path().to_string_lossy().into_owned()),
                relative_path: pi::ProjectRelativePath::parse("evidence.txt")?,
                region_anchor: None,
                source_fingerprint: Some(fingerprint.clone()),
            },
        )
        .await?;
    let evidence_id = pi::ContextMapEntryId::parse("evidence-context")?;
    pi::ContextMapStore::open(&sqlite)
        .await?
        .create_entry(
            evidence_id.clone(),
            pi::NewContextMapEntry {
                project_id: project.clone(),
                node_id: source_node,
                source_fingerprint: fingerprint.clone(),
                description: "Fixture evidence".to_string(),
                routing_terms: vec!["fixture".to_string()],
                coverage: pi::ContextMapCoverage::Complete,
            },
        )
        .await?;
    for (id, words) in [
        ("a-forgotten", "Private evidence receipt QX704."),
        ("z-independent", "Independent evidence cedar."),
    ] {
        let mut value = template.clone();
        value.content = words.to_string();
        value.evidence = vec![pi::BlackboardEvidenceLink {
            context_map_entry_id: evidence_id.clone(),
            source_fingerprint: fingerprint.clone(),
            line_range: None,
        }];
        let entry = store
            .create_entry(pi::BlackboardEntryId::parse(id)?, value)
            .await?;
        if id == "a-forgotten" {
            forget(&mut server, &project, &thread, id, entry.revision).await?;
        }
    }
    for attempt in 0..2 {
        let before = snapshot(&sqlite).await?;
        for scope in ["historical", "all"] {
            let output = model_call(&mut server, &responses_server, &thread, "blackboard_query", json!({"evidenceContextMapEntryIds":[evidence_id.to_string()],"entryScope":scope,"detail":"full","limit":1})).await?;
            let expected = if scope == "historical" {
                Vec::new()
            } else {
                vec![json!("z-independent")]
            };
            assert_eq!(
                output["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item["entryId"].clone())
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(output["truncated"], json!(false));
            assert!(!output.to_string().contains("QX704"));
        }
        assert_eq!(
            store
                .get_entry(&project, &pi::BlackboardEntryId::parse("a-forgotten")?)
                .await?
                .unwrap()
                .value
                .evidence
                .len(),
            1
        );
        assert_eq!(snapshot(&sqlite).await?, before);
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}

#[path = "stateful_capture_repair3_tests.rs"]
mod repair3_tests;

#[path = "stateful_capture_route_repair3_tests.rs"]
mod route_repair3_tests;

// The completion witness runs an `sh` acceptance check.
#[cfg(not(target_os = "windows"))]
#[path = "stateful_capture_cut_tests.rs"]
mod cut_tests;
