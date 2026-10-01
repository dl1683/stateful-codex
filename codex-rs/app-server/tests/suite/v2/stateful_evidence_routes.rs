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

/// A code-mode script selects a refreshed route programmatically and reads it, reads
/// a current range by path, and gets truthful diagnostics for a wrapped route item, a
/// mistyped fingerprint, and an unrelated unknown field.
#[tokio::test]
async fn code_mode_reads_selected_routes_and_reports_route_mistakes_truthfully() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::create_dir(project_root.path().join("textkit"))?;
    std::fs::write(
        project_root.path().join("textkit").join("cli.py"),
        "import core\n\ndef main():\n    core.run()\n",
    )?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .enable_feature(Feature::CodeModeOnly)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Evidence routes".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: "evidence-routes-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("routes-response"),
                responses::ev_custom_tool_call(
                    "routes-exec",
                    "exec",
                    r#"
const refreshed = await tools.context_map_refresh({});
const item = refreshed.routes.find(r => r.source.relativePath === "textkit/cli.py");
const routed = await tools.evidence_read({ evidenceRoute: item.evidenceRoute });
const ranged = await tools.evidence_read({ relativePath: "textkit/cli.py", lineRange: { start: 3, end: 4 } });
const attempt = async (args) => {
  try { await tools.evidence_read(args); return "accepted"; }
  catch (error) { return String((error && error.message) || error); }
};
const fingerprint = item.evidenceRoute.sourceFingerprint;
const mistyped = fingerprint.slice(0, -1) + (fingerprint.endsWith("0") ? "1" : "0");
text(JSON.stringify({
  routed: routed.content,
  routedReceipt: routed.blackboardEvidence !== null,
  ranged: ranged.content,
  wrapper: await attempt({ name: "cli", evidenceRoute: item.evidenceRoute }),
  wholeItem: await attempt({ evidenceRoute: item }),
  mistyped: await attempt({ evidenceRoute: { ...item.evidenceRoute, sourceFingerprint: mistyped } }),
  unknown: await attempt({ evidenceRoute: item.evidenceRoute, extra: 1 }),
}));
"#,
                ),
                responses::ev_completed("routes-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Read the CLI entry point.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    let output = requests[1].custom_tool_call_output("routes-exec");
    let printed = output["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
        .unwrap_or_else(|| panic!("the script should print its results: {output}"));
    let message = |key: &str| printed[key].as_str().unwrap_or_default().to_string();
    let wrapper_diagnostic = "rather than the whole route item or {name, evidenceRoute} wrapper";
    assert_eq!(
        (
            &printed["routed"],
            &printed["routedReceipt"],
            &printed["ranged"],
            message("wrapper").contains(wrapper_diagnostic),
            message("wholeItem").contains(wrapper_diagnostic),
            message("mistyped").contains("does not match this indexed route"),
            message("mistyped").contains("not current"),
            message("unknown").contains("unknown field `extra`"),
        ),
        (
            &json!("L1: import core\nL2: \nL3: def main():\nL4:     core.run()\n"),
            &json!(true),
            &json!("L3: def main():\nL4:     core.run()\n"),
            true,
            true,
            true,
            false,
            true,
        ),
        "{printed}"
    );
    Ok(())
}
