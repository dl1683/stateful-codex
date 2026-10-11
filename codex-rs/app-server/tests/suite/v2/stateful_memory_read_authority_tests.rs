//! One admitted read policy for the user's own memory: a user-confirmed entry, an entry whose
//! User revision was later downgraded to Agent, and an Agent-provenance entry with only a
//! HumanDirect context are returned by every model read (exact, search, history, `since`,
//! evidence dependents, context-map coverage counts) like an ordinary Agent control, and the
//! trusted-client `blackboard/query` keeps listing them across an app-server restart.
use super::capture_boundaries_tests::call;
use super::capture_boundaries_tests::message;
use super::capture_boundaries_tests::mount;
use super::capture_boundaries_tests::turn;
use super::memory_reads_tests::output;
use super::memory_reads_tests::project_with_notes;
use super::*;
use codex_app_server_protocol::RequestId;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;

const CONFIRMED: &str = "Lighthouse deploys happen on Tuesday mornings.";
const DOWNGRADED: &str = "Lighthouse keeps its codename internal.";
const DIRECT: &str = "Lighthouse staging hosts reboot nightly.";
const CONTROL: &str = "Lighthouse caches warm at boot.";
/// Every entry's words.
const ALL: [&str; 4] = [CONFIRMED, DOWNGRADED, DIRECT, CONTROL];

async fn rpc(server: &mut TestAppServer, method: &str, params: Value) -> Result<Value> {
    let id = server.send_request(method, Some(params)).await?;
    Ok(server
        .read_stream_until_response_message(RequestId::Integer(id))
        .await?
        .result)
}

/// An Agent-provenance entry citing the indexed route, written through `blackboard/upsert`.
fn agent_entry(project: &str, route: &Value, id: &str, content: &str, revision: Value) -> Value {
    json!({
        "projectId": project,
        "entryId": id,
        "expectedRevision": revision,
        "nodeId": route["nodeId"],
        "kind": "fact",
        "content": content,
        "confidenceBasisPoints": 9000,
        "verification": "unverified",
        "importance": "high",
        "rootPromotion": "notPromoted",
        "evidence": [{
            "contextMapEntryId": route["entryId"],
            "sourceFingerprint": route["sourceFingerprint"],
            "lineRange": null
        }],
        "provenance": {"kind": "agent", "sourceId": "witness-agent"}
    })
}

/// What the trusted client lists, as entry IDs.
async fn client_listing(server: &mut TestAppServer, project: &str) -> Result<BTreeSet<String>> {
    let listed = rpc(
        server,
        "blackboard/query",
        json!({"projectId": project, "text": null, "limit": 50}),
    )
    .await?;
    Ok(listed["data"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|hit| hit["entry"]["id"].as_str().expect("entry id").to_string())
        .collect())
}

#[tokio::test]
async fn reads_return_user_authority_entries_under_one_policy() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, _) = project_with_notes(&home, &root, &responses_server).await?;
    let routes = rpc(
        &mut server,
        "contextMap/query",
        json!({"projectId": project, "text": "Release checklist", "limit": 1}),
    )
    .await?;
    let route = routes["data"][0].clone();

    // A user-confirmed entry; an entry confirmed and then downgraded to Agent; an ordinary
    // Agent control. All cite the same file route.
    for (id, content) in [
        ("confirmed-1", CONFIRMED),
        ("downgraded-1", DOWNGRADED),
        ("control-1", CONTROL),
    ] {
        rpc(
            &mut server,
            "blackboard/upsert",
            agent_entry(&project, &route, id, content, Value::Null),
        )
        .await?;
    }
    for id in ["confirmed-1", "downgraded-1"] {
        rpc(
            &mut server,
            "blackboard/confirm",
            json!({"projectId": project, "entryId": id, "expectedRevision": 1}),
        )
        .await?;
    }
    // The downgrade keeps the entry where it is (no nodeId).
    let mut downgrade = agent_entry(&project, &route, "downgraded-1", DOWNGRADED, json!(2));
    downgrade["nodeId"] = Value::Null;
    let downgraded = rpc(&mut server, "blackboard/upsert", downgrade).await?;
    assert_eq!(
        (
            downgraded["entry"]["provenance"]["kind"].clone(),
            downgraded["entry"]["revision"].clone()
        ),
        (json!("agent"), json!(3))
    );
    // An Agent-provenance entry whose only user signal is its HumanDirect context.
    let store = BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
        AbsolutePathBuf::try_from(home.path().to_path_buf())?,
    ))
    .await?;
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("direct-1")?,
            NewBlackboardEntry {
                project_id: project.clone(),
                node_id: HierarchyNodeId::parse(route["nodeId"].as_str().expect("node"))?,
                kind: codex_project_intelligence::BlackboardKind::Fact,
                content: DIRECT.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9000)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::NotPromoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: ContextMapEntryId::parse(
                        route["entryId"].as_str().expect("route"),
                    )?,
                    source_fingerprint: SourceFingerprint::parse(
                        route["sourceFingerprint"].as_str().expect("fingerprint"),
                    )?,
                    line_range: None,
                }],
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
                    source_id: "witness-agent".to_string(),
                },
            },
            KnowledgeContext::new(KnowledgeCategory::Note, KnowledgeAuthority::HumanDirect),
            ChangeRecord {
                operation: ChangeOperation::Saved,
                origin: ChangeOrigin::DirectControl,
                category: KnowledgeCategory::Note,
                action_id: None,
                thread_id: None,
                turn_id: None,
                group_id: None,
                preview: DIRECT.to_string(),
            },
        )
        .await?;

    // The trusted client keeps every entry, before and after an app-server restart.
    let everything = BTreeSet::from(
        ["confirmed-1", "control-1", "direct-1", "downgraded-1"].map(str::to_string),
    );
    assert_eq!(client_listing(&mut server, &project).await?, everything);
    drop(server);
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    assert_eq!(client_listing(&mut server, &project).await?, everything);

    let route_id = route["entryId"].clone();
    let reads = [
        (
            "exact-confirmed",
            "blackboard_query",
            json!({"entryId": "confirmed-1"}),
        ),
        (
            "exact-downgraded",
            "blackboard_query",
            json!({"entryId": "downgraded-1"}),
        ),
        (
            "exact-direct",
            "blackboard_query",
            json!({"entryId": "direct-1"}),
        ),
        (
            "exact-control",
            "blackboard_query",
            json!({"entryId": "control-1"}),
        ),
        ("search", "blackboard_query", json!({"text": "Lighthouse"})),
        (
            "history",
            "blackboard_query",
            json!({"text": "Lighthouse", "entryScope": "all"}),
        ),
        (
            "recall-history",
            "memory_read",
            json!({"question": "Lighthouse", "includeHistory": true}),
        ),
        ("since", "memory_read", json!({"since": "2000-01-01"})),
        (
            "dependents",
            "blackboard_query",
            json!({"evidenceContextMapEntryIds": [route_id]}),
        ),
        (
            "map-query",
            "context_map_query",
            json!({"text": "Release checklist"}),
        ),
        ("map-refresh", "context_map_refresh", json!({})),
    ];
    let script_reads = reads.clone();
    let calls = mount(
        &responses_server,
        Box::new(move |sequence, _| match script_reads.get(sequence) {
            Some((id, tool, args)) => call(id, tool, args.clone()),
            None => message("Done."),
        }),
    )
    .await;
    let thread = start_thread(&mut server, &project).await?;
    turn(&mut server, &thread, &["What do we know about Lighthouse?"]).await?;
    let bodies = calls.lock().expect("request log lock").clone();
    let outputs = reads
        .iter()
        .enumerate()
        .map(|(index, (id, _, _))| (*id, output(&bodies[index + 1], id)))
        .collect::<Vec<_>>();
    for (id, content) in [
        ("exact-confirmed", CONFIRMED),
        ("exact-downgraded", DOWNGRADED),
        ("exact-direct", DIRECT),
        ("exact-control", CONTROL),
    ] {
        let output = &outputs.iter().find(|(found, _)| found == &id).expect(id).1;
        assert!(output.contains(content), "{id}: {output}");
    }
    for (id, output) in outputs.iter().filter(|(id, _)| {
        ["search", "history", "recall-history", "since", "dependents"].contains(id)
    }) {
        for content in ALL {
            assert!(output.contains(content), "{id} lacks {content:?}: {output}");
        }
    }
    // Coverage counts on the shared route count all four entries.
    for (id, output) in outputs.iter().filter(|(id, _)| id.starts_with("map-")) {
        let parsed: Value = serde_json::from_str(output)?;
        let routes = parsed
            .get("data")
            .or_else(|| parsed.get("routes"))
            .and_then(Value::as_array)
            .expect("routes");
        let known = routes
            .iter()
            .filter_map(|route| route.get("knownKnowledge").cloned())
            .collect::<Vec<_>>();
        assert!(
            !known.is_empty(),
            "{id} lost the entries' coverage: {output}"
        );
        assert!(
            known
                .iter()
                .all(|known| *known == json!({"rootEntries": 0, "deeperEntries": 4})),
            "{id}: {output}"
        );
    }
    Ok(())
}
