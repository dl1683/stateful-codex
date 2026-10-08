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
    let log = responses::mount_sse_sequence(
        responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(&call_id, tool, &arguments.to_string()),
                responses::ev_completed("repair-request"),
            ]),
            responses::sse(vec![responses::ev_completed("repair-done")]),
        ],
    )
    .await;
    run_turn(server, thread, "Inspect the retained project memory.").await?;
    let requests = log.requests();
    assert_eq!(requests.len(), 2);
    Ok(requests[1]
        .function_call_output_text(&call_id)
        .expect("actual model-delivered result"))
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

#[tokio::test]
async fn c2r1_public_conversation_fallbacks_exclude_forgotten_bytes_after_restart() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let text = "Private meridian receipt QX704.";
    let log = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![responses::ev_completed("original")]),
    )
    .await;
    let original = server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    assert_eq!(log.requests().len(), 1);
    let _: codex_app_server_protocol::ThreadSetNameResponse = server
        .request(|request_id| ClientRequest::ThreadSetName {
            request_id,
            params: codex_app_server_protocol::ThreadSetNameParams {
                thread_id: thread.clone(),
                name: text.to_string(),
            },
        })
        .await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: text.to_string(),
                scope: None,
                reason: None,
                client_action_id: "forget-conversation-words".to_string(),
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
        let threads = model_call(
            &mut server,
            &responses_server,
            &thread,
            "conversation_read",
            json!({}),
        )
        .await?;
        assert!(!threads.to_string().contains("QX704"));
        let turns = model_call(
            &mut server,
            &responses_server,
            &thread,
            "conversation_read",
            json!({"threadId":thread}),
        )
        .await?;
        assert!(!turns.to_string().contains("QX704"));
        let exact = model_output(
            &mut server,
            &responses_server,
            &thread,
            "conversation_read",
            json!({"threadId":thread,"turnId":original.turn.id,"part":"user"}),
        )
        .await?;
        assert!(!exact.contains("QX704"));
        assert!(exact.contains("retirement"), "{exact}");
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}
