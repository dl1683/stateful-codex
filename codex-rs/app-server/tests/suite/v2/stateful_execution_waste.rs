//! Execution waste through the public API: explicit source reads in a project that was
//! never indexed (debug2's false "not indexed" errors), and repeated knowledge writes.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

async fn project_thread(server: &mut TestAppServer, root: &TempDir, name: &str) -> Result<String> {
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: name.to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: format!("{name}-project"),
            },
        })
        .await?;
    Ok(server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?
        .thread
        .id)
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

fn assistant(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("assistant-message", text),
        responses::ev_completed("assistant-response"),
    ])
}

/// Parallel first reads of existing files in a never-indexed project, including a
/// git-ignored vendored file, all return their content.
#[tokio::test]
async fn parallel_first_reads_in_an_unindexed_project_return_every_file() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let root = TempDir::new()?;
    std::fs::create_dir_all(root.path().join(".git"))?;
    std::fs::write(root.path().join(".gitignore"), "vendor/\n")?;
    std::fs::create_dir_all(root.path().join("shipit"))?;
    std::fs::create_dir_all(root.path().join("vendor/click"))?;
    std::fs::write(root.path().join("shipit/cli.py"), "import click\n")?;
    std::fs::write(root.path().join("README.md"), "# shipit\n")?;
    std::fs::write(
        root.path().join("vendor/click/core.py"),
        "def main(): ...\n",
    )?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let thread = project_thread(&mut server, &root, "unindexed-reads").await?;
    let reads = [
        ("read-cli", "shipit/cli.py"),
        ("read-readme", "README.md"),
        ("read-vendor", "vendor/click/core.py"),
    ];
    let mut events = reads
        .iter()
        .map(|(call_id, path)| {
            responses::ev_function_call(
                call_id,
                "evidence_read",
                &json!({"relativePath": path}).to_string(),
            )
        })
        .collect::<Vec<_>>();
    events.push(responses::ev_completed("reads-response"));
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![responses::sse(events), assistant("Read them.")],
    )
    .await;
    run_turn(&mut server, &thread, "Why does the deploy fail?").await?;

    let request = &log.requests()[1];
    let contents = reads
        .iter()
        .map(|(call_id, _)| {
            let text = request
                .function_call_output_text(call_id)
                .expect("read output");
            serde_json::from_str::<Value>(&text)
                .map(|output| output["content"].clone())
                .unwrap_or(Value::String(text))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            json!("L1: import click\n"),
            json!("L1: # shipit\n"),
            json!("L1: def main(): ...\n"),
        ]
    );
    Ok(())
}

/// Recording the same decision again, in a later call or as a replay, saves nothing new.
#[tokio::test]
async fn a_repeated_decision_is_already_present() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let root = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let thread = project_thread(&mut server, &root, "repeated-decision").await?;
    let decision = |key: &str| {
        json!({"records": [{
            "idempotencyKey": key,
            "kind": "decision",
            "content": "Fix the vendored Click ordering, because both consumers read the source.",
            "confidenceBasisPoints": 9000,
            "verification": "unverified",
            "importance": "high",
            "rootPromotion": "promoted"
        }]})
    };
    let call = |call_id: &str, key: &str| {
        responses::sse(vec![
            responses::ev_function_call(
                call_id,
                "blackboard_record_batch",
                &decision(key).to_string(),
            ),
            responses::ev_completed(&format!("{call_id}-response")),
        ])
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            call("first", "decision-1"),
            call("again", "decision-2"),
            call("replay", "decision-1"),
            assistant("Recorded."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "We agreed: fix vendored Click.").await?;

    let requests = log.requests();
    let counts = [(1, "first"), (2, "again"), (3, "replay")].map(
        |(index, call_id)| -> Result<(Value, Value)> {
            let output: Value = serde_json::from_str(
                &requests[index]
                    .function_call_output_text(call_id)
                    .expect("record output"),
            )?;
            Ok((output["recorded"].clone(), output["alreadyPresent"].clone()))
        },
    );
    let [first, again, replay] = counts;
    assert_eq!(
        [first?, again?, replay?],
        [
            (json!(1), json!(0)),
            (json!(0), json!(1)),
            (json!(0), json!(1)),
        ]
    );
    Ok(())
}
