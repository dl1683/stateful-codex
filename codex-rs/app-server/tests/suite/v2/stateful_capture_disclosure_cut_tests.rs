//! The C4-C6 cut through the public app-server: memory the user can forget (declared rules,
//! applied decisions) never enters a model tool result, so a result held after a Forget
//! cannot carry it. It reaches the model only through the thread-start root snapshot.
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
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryApplyResponse;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

const RULE: &str = "Ground rules for this project:\n- Never push.";
const FINDING: &str = "The release checklist bumps the version first.";
const NOTES: &str = "Release checklist: bump the version.\n";
/// Words of the user's rule and decision; none may appear in any tool result.
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
/// requests make the calls `reads` lists, in order.
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
                Some((id, tool, args)) => call(id, tool, args.clone()),
                None => message("Done."),
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

#[tokio::test]
async fn c456cut_tool_results_never_carry_the_users_words() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, thread) = project_with_notes(&home, &root, &responses_server).await?;
    let reads = Arc::new(Mutex::new(Vec::new()));
    let calls = mount(&responses_server, script(reads.clone())).await;
    let (decision, _, rule) = remember(&mut server, &project, &thread).await?;
    // Root delivery keeps the user's memory.
    let mut root_words = root_contents(&home, &project).await?;
    root_words.sort();
    assert_eq!(
        root_words,
        vec!["Never push.".to_string(), DECISION.to_string()]
    );

    *reads.lock().expect("targets lock") = vec![
        (
            "exact-decision".into(),
            "blackboard_query".into(),
            json!({"entryId": decision}),
        ),
        (
            "exact-rule".into(),
            "blackboard_query".into(),
            json!({"entryId": rule}),
        ),
        (
            "search-user".into(),
            "blackboard_query".into(),
            json!({"text": "SQLite"}),
        ),
        (
            "search-push".into(),
            "blackboard_query".into(),
            json!({"text": "push"}),
        ),
        (
            "search-agent".into(),
            "blackboard_query".into(),
            json!({"text": "release"}),
        ),
        (
            "recall".into(),
            "memory_read".into(),
            json!({"question": "SQLite Postgres offline laptop push release checklist"}),
        ),
        (
            "evidence".into(),
            "evidence_read".into(),
            json!({"relativePath": "notes.md"}),
        ),
    ];
    let second = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &second,
        &["What do we know about this project?"],
    )
    .await?;
    let bodies = calls.lock().expect("request log lock").clone();
    let read_ids = reads
        .lock()
        .expect("targets lock")
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<Vec<_>>();
    let outputs = read_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), output(&bodies[6 + index], id)))
        .collect::<Vec<_>>();
    for (id, output) in &outputs {
        for word in USER_WORDS {
            assert!(!output.contains(word), "{id} carried {word:?}: {output}");
        }
    }
    // Positive controls: the agent finding and the source file are still returned.
    let by_id = |wanted: &str| {
        outputs
            .iter()
            .find(|(id, _)| id == wanted)
            .map(|(_, output)| output.clone())
            .unwrap_or_default()
    };
    assert!(
        by_id("search-agent").contains(FINDING),
        "{}",
        by_id("search-agent")
    );
    assert!(by_id("recall").contains(FINDING), "{}", by_id("recall"));
    assert!(
        by_id("evidence").contains("Release checklist: bump the version."),
        "{}",
        by_id("evidence")
    );
    Ok(())
}
