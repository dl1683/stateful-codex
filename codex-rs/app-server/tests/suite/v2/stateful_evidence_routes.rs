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
/// current ranges by path (choosing a root where the path is ambiguous), and gets
/// truthful diagnostics for a wrapped route item, a mistyped fingerprint, an unrelated
/// unknown field, conflicting selectors, an ambiguous path, and a foreign root.
#[tokio::test]
async fn code_mode_reads_selected_routes_and_reports_route_mistakes_truthfully() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let second_root = TempDir::new()?;
    std::fs::write(project_root.path().join("shared.txt"), "first\n")?;
    std::fs::write(second_root.path().join("shared.txt"), "second\n")?;
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
                roots: [&project_root, &second_root]
                    .into_iter()
                    .map(|root| ProjectRoot {
                        path: AbsolutePathBuf::try_from(root.path().to_path_buf())
                            .expect("temporary project root should be absolute"),
                    })
                    .collect(),
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
const second = refreshed.routes.find(r => r.source.relativePath === "shared.txt" && r.source.projectRoot !== item.source.projectRoot);
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
  routedIdentity: [routed.contextMapEntryId === item.evidenceRoute.contextMapEntryId, routed.sourceFingerprint === fingerprint],
  routedReceipt: routed.blackboardEvidence.readReceiptId.startsWith("stateful-read-"),
  ranged: ranged.content,
  explicitRoot: (await tools.evidence_read({ relativePath: "shared.txt", projectRoot: second.source.projectRoot })).content,
  bothSelectors: await attempt({ evidenceRoute: item.evidenceRoute, relativePath: "textkit/cli.py" }),
  ambiguous: await attempt({ relativePath: "shared.txt" }),
  foreignRoot: await attempt({ relativePath: "shared.txt", projectRoot: item.source.projectRoot + "-outside" }),
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
    let diagnosed =
        |key: &str, text: &str| printed[key].as_str().unwrap_or_default().contains(text);
    let wrapper_diagnostic = "rather than the whole route item or {name, evidenceRoute} wrapper";
    let observed = json!({
        "routed": printed["routed"],
        "routedIdentity": printed["routedIdentity"],
        "routedReceipt": printed["routedReceipt"],
        "ranged": printed["ranged"],
        "explicitRoot": printed["explicitRoot"],
        "bothSelectors": diagnosed("bothSelectors", "provide either evidenceRoute"),
        "ambiguous": diagnosed("ambiguous", "multiple project roots"),
        "foreignRoot": diagnosed("foreignRoot", "outside the selected project"),
        "wrapper": diagnosed("wrapper", wrapper_diagnostic),
        "wholeItem": diagnosed("wholeItem", wrapper_diagnostic),
        "mistypedMismatch": diagnosed("mistyped", "does not match this indexed route"),
        "mistypedClaimsNotCurrent": diagnosed("mistyped", "not current"),
        "unknown": diagnosed("unknown", "unknown field `extra`"),
    });
    assert_eq!(
        observed,
        json!({
            "routed": "L1: import core\nL2: \nL3: def main():\nL4:     core.run()\n",
            "routedIdentity": [true, true],
            "routedReceipt": true,
            "ranged": "L3: def main():\nL4:     core.run()\n",
            "explicitRoot": "L1: second\n",
            "bothSelectors": true,
            "ambiguous": true,
            "foreignRoot": true,
            "wrapper": true,
            "wholeItem": true,
            "mistypedMismatch": true,
            "mistypedClaimsNotCurrent": false,
            "unknown": true,
        }),
        "{printed}"
    );
    Ok(())
}
