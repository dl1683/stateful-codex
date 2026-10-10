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
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

/// The model selects a refreshed route and reads it, reads current ranges by path (choosing
/// a root where the path is ambiguous), and gets truthful diagnostics for a wrapped route
/// item, a mistyped fingerprint, an unrelated unknown field, conflicting selectors, an
/// ambiguous path, and a foreign root. Stateful tools stay direct model tools in a
/// code-mode-only session: nested cells cannot reach them.
#[tokio::test]
async fn direct_calls_read_selected_routes_and_report_route_mistakes_truthfully() -> Result<()> {
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
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let log = requests.clone();
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("JSON body");
            let mut log = log.lock().expect("request log");
            let response = match log.len() {
                0 => responses::sse(vec![
                    responses::ev_response_created("refresh-response"),
                    responses::ev_function_call("refresh", "context_map_refresh", "{}"),
                    responses::ev_completed("refresh-response"),
                ]),
                1 => route_reads(&body),
                _ => responses::sse(vec![
                    responses::ev_assistant_message("done-message", "Done"),
                    responses::ev_completed("done-response"),
                ]),
            };
            log.push(body);
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(response)
        })
        .mount(&responses_server)
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

    let requests = requests.lock().expect("request log").clone();
    assert_eq!(requests.len(), 3);
    let output = |call: &str| call_output(&requests[2], call);
    let read = |call: &str| -> Value { serde_json::from_str(&output(call)).unwrap_or_default() };
    let item = cli_route(&refresh_routes(&requests[1]));
    let routed = read("routed");
    let diagnosed = |call: &str, text: &str| output(call).contains(text);
    let wrapper_diagnostic = "rather than the whole route item or {name, evidenceRoute} wrapper";
    let observed = json!({
        "routed": routed["content"],
        "routedIdentity": [
            routed["contextMapEntryId"] == item["evidenceRoute"]["contextMapEntryId"],
            routed["sourceFingerprint"] == item["evidenceRoute"]["sourceFingerprint"],
        ],
        "routedReceipt": routed["blackboardEvidence"]["readReceiptId"]
            .as_str()
            .is_some_and(|id| id.starts_with("stateful-read-")),
        "ranged": read("ranged")["content"],
        "explicitRoot": read("explicitRoot")["content"],
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
        "{requests:?}"
    );
    Ok(())
}

/// The text delivered for `call_id` in a request body.
fn call_output(body: &Value, call_id: &str) -> String {
    let item = body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
        .unwrap_or_else(|| panic!("no output for {call_id}: {body}"));
    match &item["output"] {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The routes `context_map_refresh` returned, as the model received them.
fn refresh_routes(body: &Value) -> Value {
    let refreshed: Value =
        serde_json::from_str(&call_output(body, "refresh")).expect("refresh output is JSON");
    refreshed["routes"].clone()
}

fn cli_route(routes: &Value) -> Value {
    routes
        .as_array()
        .into_iter()
        .flatten()
        .find(|route| route["source"]["relativePath"] == "textkit/cli.py")
        .cloned()
        .expect("the CLI route")
}

/// One response of direct reads: the selected routes and every mistaken form.
fn route_reads(body: &Value) -> String {
    let routes = refresh_routes(body);
    let item = cli_route(&routes);
    let second = routes
        .as_array()
        .into_iter()
        .flatten()
        .find(|route| {
            route["source"]["relativePath"] == "shared.txt"
                && route["source"]["projectRoot"] != item["source"]["projectRoot"]
        })
        .cloned()
        .expect("the second root's shared.txt");
    let route = item["evidenceRoute"].clone();
    let fingerprint = route["sourceFingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_string();
    let mut mistyped = route.clone();
    mistyped["sourceFingerprint"] = json!(format!(
        "{}{}",
        &fingerprint[..fingerprint.len() - 1],
        if fingerprint.ends_with('0') { "1" } else { "0" }
    ));
    let outside = format!(
        "{}-outside",
        item["source"]["projectRoot"].as_str().expect("root")
    );
    let calls = [
        ("routed", json!({"evidenceRoute": route})),
        (
            "ranged",
            json!({"relativePath": "textkit/cli.py", "lineRange": {"start": 3, "end": 4}}),
        ),
        (
            "explicitRoot",
            json!({"relativePath": "shared.txt", "projectRoot": second["source"]["projectRoot"]}),
        ),
        (
            "bothSelectors",
            json!({"evidenceRoute": route, "relativePath": "textkit/cli.py"}),
        ),
        ("ambiguous", json!({"relativePath": "shared.txt"})),
        (
            "foreignRoot",
            json!({"relativePath": "shared.txt", "projectRoot": outside}),
        ),
        ("wrapper", json!({"name": "cli", "evidenceRoute": route})),
        ("wholeItem", json!({"evidenceRoute": item})),
        ("mistyped", json!({"evidenceRoute": mistyped})),
        ("unknown", json!({"evidenceRoute": route, "extra": 1})),
    ];
    let mut events = vec![responses::ev_response_created("reads-response")];
    events.extend(calls.iter().map(|(call, arguments)| {
        responses::ev_function_call(call, "evidence_read", &arguments.to_string())
    }));
    events.push(responses::ev_completed("reads-response"));
    responses::sse(events)
}
