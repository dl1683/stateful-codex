//! Model reads of the user's own memory through the public app-server, under the next-turn
//! Forget contract: declared rules and applied decisions come back with attribution, and once
//! a Forget commits, no read of a later turn returns them.
use super::capture_boundaries_tests::DECISION;
use super::capture_boundaries_tests::apply_request;
use super::capture_boundaries_tests::call;
use super::capture_boundaries_tests::kept;
use super::capture_boundaries_tests::message;
use super::capture_boundaries_tests::mount;
use super::capture_boundaries_tests::proposal;
use super::capture_boundaries_tests::root_contents;
use super::capture_boundaries_tests::turn;
use super::*;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryAddParams;
use codex_app_server_protocol::StatefulMemoryAddResponse;
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryApplyResponse;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

const RULE: &str = "Ground rules for this project:\n- Never push.";
const FINDING: &str = "The release checklist bumps the version first.";
const NOTES: &str = "Release checklist: bump the version.\n";
/// Words of the user's rule and decision; no read after their Forget may return them.
const USER_WORDS: [&str; 4] = ["Never push", "offline on a laptop", "SQLite", "Postgres"];

/// A project with one indexed source file and a durably bound thread.
pub(super) async fn project_with_notes(
    home: &TempDir,
    root: &TempDir,
    responses_server: &wiremock::MockServer,
) -> Result<(TestAppServer, String, String)> {
    std::fs::write(root.path().join("notes.md"), NOTES)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let path = AbsolutePathBuf::try_from(root.path().to_path_buf())?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Cut".into(),
                roots: vec![ProjectRoot { path }],
                metadata: None,
                idempotency_key: "cut-project".into(),
            },
        })
        .await?;
    let project = project.project.id;
    let _: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project.clone(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project).await?;
    Ok((server, project, thread))
}

/// Session 1: an applied decision, a declared rule and an agent finding. Returns the
/// decision's id and applied revision and the rule's id.
async fn remember(
    server: &mut TestAppServer,
    project: &str,
    thread: &str,
) -> Result<(String, u64, String)> {
    turn(server, thread, &[DECISION]).await?;
    let decision = kept(server).await?.members[0]
        .entry_id
        .clone()
        .expect("decision proposal");
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            project,
            thread,
            &decision,
            StatefulMemoryApplyCategory::Decision,
            "apply-cut",
        ))
        .await?;
    let revision = applied.receipt.members[0]
        .revision
        .expect("applied revision");
    turn(server, thread, &[RULE]).await?;
    let rule = kept(server).await?.members[0]
        .entry_id
        .clone()
        .expect("admitted rule");
    turn(server, thread, &["Record what the release checklist says."]).await?;
    Ok((decision, revision, rule))
}

/// The model's script: session 1 proposes the decision and records the finding; later
/// requests make the calls `reads` lists, in order (an empty tool name ends a turn).
fn script(
    targets: Arc<Mutex<Vec<(String, String, Value)>>>,
) -> super::capture_boundaries_tests::Script {
    Box::new(move |sequence, body| match sequence {
        0 => proposal(body, "decision", "SQLite for offline laptops.", None, None),
        3 => call(
            "finding",
            "blackboard_record_batch",
            json!({"records":[{
                "idempotencyKey":"release-finding","kind":"fact","content":FINDING,
                "confidenceBasisPoints":9000,"verification":"unverified","importance":"high",
                "rootPromotion":"notPromoted"
            }]}),
        ),
        _ => {
            let reads = targets.lock().expect("targets lock");
            match sequence.checked_sub(5).and_then(|index| reads.get(index)) {
                Some((id, tool, args)) if !tool.is_empty() => call(id, tool, args.clone()),
                _ => message("Done."),
            }
        }
    })
}

pub(super) fn output(body: &Value, call_id: &str) -> String {
    body["input"]
        .as_array()
        .expect("input items")
        .iter()
        .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
        .unwrap_or_else(|| panic!("{call_id} output delivered"))["output"]
        .as_str()
        .expect("text output")
        .to_string()
}

/// The reads one turn makes; `phase` keeps call IDs unique across turns.
fn reads(phase: &str, decision: &str, rule: &str) -> Vec<(String, String, Value)> {
    [
        (
            "exact-decision",
            "blackboard_query",
            json!({"entryId": decision}),
        ),
        ("exact-rule", "blackboard_query", json!({"entryId": rule})),
        (
            "search-user",
            "blackboard_query",
            json!({"text": "SQLite", "detail": "full"}),
        ),
        (
            "search-push",
            "blackboard_query",
            json!({"text": "push", "detail": "full"}),
        ),
        (
            "search-agent",
            "blackboard_query",
            json!({"text": "release"}),
        ),
        (
            "recall",
            "memory_read",
            json!({"question": "SQLite Postgres offline laptop push release checklist"}),
        ),
    ]
    .into_iter()
    .map(|(id, tool, args)| (format!("{phase}-{id}"), tool.to_string(), args))
    .chain([(format!("{phase}-end"), String::new(), Value::Null)])
    .collect()
}

/// Every read's output in the logged requests, by call ID.
fn outputs(bodies: &[Value], reads: &[(String, String, Value)]) -> Vec<(String, String)> {
    reads
        .iter()
        .filter(|(_, tool, _)| !tool.is_empty())
        .map(|(id, _, _)| {
            let body = bodies
                .iter()
                .find(|body| body.to_string().contains(&format!("\"call_id\":\"{id}\"")))
                .unwrap_or_else(|| panic!("{id} output delivered"));
            (id.clone(), output(body, id))
        })
        .collect()
}

#[tokio::test]
async fn reads_return_user_memory_with_attribution_until_forget() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, thread) = project_with_notes(&home, &root, &responses_server).await?;
    let script_reads = Arc::new(Mutex::new(Vec::new()));
    let calls = mount(&responses_server, script(script_reads.clone())).await;
    let (decision, revision, rule) = remember(&mut server, &project, &thread).await?;
    let mut root_words = root_contents(&home, &project).await?;
    root_words.sort();
    assert_eq!(
        root_words,
        vec!["Never push.".to_string(), DECISION.to_string()]
    );

    let before = reads("before", &decision, &rule);
    let after = reads("after", &decision, &rule);
    *script_reads.lock().expect("targets lock") = [before.clone(), after.clone()].concat();
    let second = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &second,
        &["What do we know about this project?"],
    )
    .await?;
    let found = outputs(&calls.lock().expect("request log lock"), &before);
    let by_id = |wanted: &str| -> Value {
        let text = &found.iter().find(|(id, _)| id == wanted).expect(wanted).1;
        serde_json::from_str(text).expect("json output")
    };
    // Exact reads carry the whole words and who stated them; the applied decision also says
    // when the user adopted it.
    let decision_read = by_id("before-exact-decision");
    assert_eq!(
        (
            decision_read["content"].clone(),
            decision_read["attribution"]["by"].clone(),
            decision_read["attribution"]["adopted"].is_string(),
            decision_read["revision"].clone(),
        ),
        (
            json!(DECISION),
            json!("user (applied)"),
            true,
            json!(revision)
        )
    );
    let rule_read = by_id("before-exact-rule");
    assert_eq!(
        (
            rule_read["content"].clone(),
            rule_read["attribution"]["by"].clone(),
            rule_read["attribution"]["said"].is_string(),
        ),
        (json!("Never push."), json!("user"), true)
    );
    for (id, word) in [
        ("before-search-user", "offline on a laptop"),
        ("before-search-push", "Never push"),
        ("before-search-agent", FINDING),
        ("before-recall", "Never push"),
        ("before-recall", "offline on a laptop"),
        ("before-recall", FINDING),
    ] {
        assert!(by_id(id).to_string().contains(word), "{id} lacks {word:?}");
    }

    // The user forgets both; every read of the next turn excludes them, the root drops them,
    // and the agent finding stays readable.
    for (entry_id, expected_revision) in [(rule, 1), (decision, revision)] {
        let _: StatefulMemoryForgetResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryForget {
                request_id,
                params: StatefulMemoryForgetParams {
                    thread_id: second.clone(),
                    expected_project_id: project.clone(),
                    entry_id,
                    expected_revision,
                },
            })
            .await?;
    }
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    let third = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &third,
        &["What do we know about this project?"],
    )
    .await?;
    let found = outputs(&calls.lock().expect("request log lock"), &after);
    for (id, output) in &found {
        for word in USER_WORDS {
            assert!(!output.contains(word), "{id} carried {word:?}: {output}");
        }
    }
    for id in ["after-search-agent", "after-recall"] {
        let output = &found.iter().find(|(found, _)| found == id).expect(id).1;
        assert!(output.contains(FINDING), "{id}: {output}");
    }
    Ok(())
}

/// An in-place `blackboard/upsert` of an existing entry as the Agent.
async fn agent_upsert(
    server: &mut TestAppServer,
    project: &str,
    entry_id: &str,
    revision: u64,
    content: &str,
) -> Result<()> {
    let request = server
        .send_request(
            "blackboard/upsert",
            Some(json!({
                "projectId": project, "entryId": entry_id, "expectedRevision": revision,
                "nodeId": null, "kind": "instruction", "content": content,
                "confidenceBasisPoints": 9000, "verification": "unverified", "importance": "high",
                "rootPromotion": "notPromoted", "evidence": [],
                "provenance": {"kind": "agent", "sourceId": "agent-edit"}
            })),
        )
        .await?;
    server
        .read_stream_until_response_message(codex_app_server_protocol::RequestId::Integer(request))
        .await?;
    Ok(())
}

/// A corrected rule's superseded words are never read again, by ID, search, history, recall or
/// `since`; a declared rule downgraded and then edited by the Agent is returned with the Agent's
/// words and no user statement.
#[tokio::test]
async fn reads_return_only_current_user_words_and_attribute_the_returned_revision() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, thread) = project_with_notes(&home, &root, &responses_server).await?;
    let script_reads = Arc::new(Mutex::new(vec![(
        "declare-end".to_string(),
        String::new(),
        Value::Null,
    )]));
    let scripted = script_reads.clone();
    let calls = mount(
        &responses_server,
        Box::new(
            move |sequence, _| match scripted.lock().expect("targets lock").get(sequence) {
                Some((id, tool, args)) if !tool.is_empty() => call(id, tool, args.clone()),
                _ => message("Done."),
            },
        ),
    )
    .await;
    turn(&mut server, &thread, &[RULE]).await?;
    let rule = kept(&mut server).await?.members[0]
        .entry_id
        .clone()
        .expect("admitted rule");
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Always cite paths with line numbers.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "superseded-add".to_string(),
                background_section: false,
            },
        })
        .await?;
    let _: StatefulMemoryCorrectResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryCorrect {
            request_id,
            params: StatefulMemoryCorrectParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                entry_id: added.item.entry_id.clone(),
                expected_revision: added.item.revision,
                content: "Always cite paths with commit hashes.".to_string(),
                background_section: false,
            },
        })
        .await?;
    agent_upsert(&mut server, &project, &rule, 1, "Never push.").await?;
    agent_upsert(&mut server, &project, &rule, 2, "Push only from CI.").await?;

    let reads = [
        (
            "old-exact",
            "blackboard_query",
            json!({"entryId": added.item.entry_id}),
        ),
        (
            "search",
            "blackboard_query",
            json!({"text": "cite", "detail": "full"}),
        ),
        (
            "history",
            "blackboard_query",
            json!({"text": "cite", "entryScope": "all", "detail": "full"}),
        ),
        ("recall", "memory_read", json!({"question": "cite paths"})),
        ("since", "memory_read", json!({"since": "2000-01-01"})),
        ("edited-exact", "blackboard_query", json!({"entryId": rule})),
    ]
    .map(|(id, tool, args)| (id.to_string(), tool.to_string(), args));
    script_reads
        .lock()
        .expect("targets lock")
        .extend(reads.iter().cloned());
    let second = start_thread(&mut server, &project).await?;
    turn(&mut server, &second, &["How do we cite paths?"]).await?;
    let found = outputs(&calls.lock().expect("request log lock"), &reads);
    for (id, output) in &found {
        assert!(
            !output.contains("line numbers"),
            "{id} carried superseded words: {output}"
        );
    }
    for (id, output) in found
        .iter()
        .filter(|(id, _)| ["search", "history", "recall", "since"].contains(&id.as_str()))
    {
        assert!(
            output.contains("commit hashes"),
            "{id} lacks the successor: {output}"
        );
    }
    let edited: Value = serde_json::from_str(
        &found
            .iter()
            .find(|(id, _)| id == "edited-exact")
            .expect("edited")
            .1,
    )?;
    assert_eq!(
        (
            edited["content"].clone(),
            edited["source"]["kind"].clone(),
            edited.get("attribution").cloned(),
        ),
        (json!("Push only from CI."), json!("agent"), None)
    );
    Ok(())
}
