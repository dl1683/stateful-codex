//! C4-C6 repair 3: every Stateful tool result is checked where the host records it into model
//! history, under the store's writer lock, so a Forget committed while the result waits (for
//! the rest of the response stream, an earlier result, or a PostToolUse hook) withholds it
//! together with every hook copy of it. Other writers on the store are never blocked while a
//! result waits.

use super::*;
use codex_app_server_protocol::StatefulMemoryCapturedNotification;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_state::SqliteConfig;
use codex_stateful_extension::MemoryActor;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

/// Start of the text a withheld read is replaced by.
const WITHHELD_READ: &str = "memory changed while this result was pending";
/// Start of the text a withheld successful write is replaced by.
#[cfg(not(target_os = "windows"))]
const WITHHELD_COMMITTED: &str = "this call's change was committed, but its output was withheld";

/// A PostToolUse command hook. A call whose ID starts with `free` passes at once; any other
/// waits for `release_<call>`. Each writes `started_<call>` once its tool result is ready.
/// `ctx*` calls echo the tool response as additional context, `block*` calls as blocking
/// feedback.
const HOOK: &str = r#"import json, os, sys, time
d = r'LATCH_DIR'
p = json.load(sys.stdin)
call = p.get('tool_use_id', '')
open(os.path.join(d, 'started_' + call), 'w').close()
if not call.startswith('free'):
    while not os.path.exists(os.path.join(d, 'release_' + call)):
        time.sleep(0.05)
echo = json.dumps(p.get('tool_response'))
if call.startswith('block'):
    print(json.dumps({'decision': 'block', 'reason': echo}))
elif call.startswith('ctx'):
    print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PostToolUse', 'additionalContext': echo}}))
"#;

/// Installs the hook for tools matching `matcher`; returns the latch directory.
fn install_hook(home: &Path, matcher: &str) -> Result<PathBuf> {
    let latch = home.join("latch");
    std::fs::create_dir_all(&latch)?;
    let hook = home.join("latch_hook.py");
    std::fs::write(
        &hook,
        HOOK.replace("LATCH_DIR", &latch.display().to_string()),
    )?;
    std::fs::write(
        home.join("requirements.toml"),
        format!(
            "[hooks]\n\n[[hooks.PostToolUse]]\nmatcher = '{matcher}'\n\n[[hooks.PostToolUse.hooks]]\ntype = 'command'\ncommand = '{python} {}'\n",
            hook.display(),
            python = if cfg!(windows) { "python" } else { "python3" }
        ),
    )?;
    Ok(latch)
}

async fn started(latch: &Path, call: &str) -> Result<()> {
    let marker = latch.join(format!("started_{call}"));
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() {
        anyhow::ensure!(Instant::now() < deadline, "the hook for {call} never ran");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

fn release(latch: &Path, call: &str) -> Result<()> {
    std::fs::write(latch.join(format!("release_{call}")), "")?;
    Ok(())
}

async fn project(
    server: &mut TestAppServer,
    home: &Path,
    name: &str,
    root: Option<&Path>,
) -> Result<String> {
    let roots = root
        .map(|root| codex_app_server_protocol::ProjectRoot {
            path: codex_utils_absolute_path::AbsolutePathBuf::try_from(root.to_path_buf())
                .expect("absolute root"),
        })
        .into_iter()
        .collect();
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: name.into(),
                roots,
                metadata: None,
                idempotency_key: format!("{name}-project"),
            },
        })
        .await?;
    let project = project.project.id;
    let sqlite = SqliteConfig::new_for_testing(home.abs());
    codex_project_intelligence::ProjectIndexer::new(
        codex_project_intelligence::HierarchyStore::open(&sqlite).await?,
        codex_project_intelligence::ContextMapStore::open(&sqlite).await?,
    )
    .ensure_project_node(&project)
    .await?;
    Ok(project)
}

/// A second pool on the server's database: another process's writer.
async fn other_writer(home: &Path) -> Result<BlackboardStore> {
    Ok(BlackboardStore::open(&SqliteConfig::new_for_testing(home.abs())).await?)
}

/// Forgets `id` through `store` and returns how long the write took.
async fn forget(store: &BlackboardStore, project: &str, id: &str) -> Result<Duration> {
    let started = Instant::now();
    codex_stateful_extension::forget_entry(
        store,
        &MemoryActor::default(),
        project,
        &BlackboardEntryId::parse(id)?,
        /*expected_revision*/ 1,
    )
    .await
    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    Ok(started.elapsed())
}

/// The entry IDs a declaration turn saved, from its receipt.
async fn saved(server: &mut TestAppServer) -> Result<Vec<String>> {
    let notification = server
        .read_stream_until_notification_message("statefulMemory/captured")
        .await?;
    let captured: StatefulMemoryCapturedNotification =
        serde_json::from_value(notification.params.expect("receipt params"))?;
    Ok(captured
        .receipt
        .members
        .into_iter()
        .filter_map(|member| member.entry_id)
        .collect())
}

async fn turn(server: &mut TestAppServer, thread: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.to_string(),
            input: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// Starts a turn without waiting for it to finish.
async fn begin_turn(server: &mut TestAppServer, thread: &str, text: &str) -> Result<()> {
    let id = server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.to_string(),
            input: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    server
        .read_stream_until_response_message(codex_app_server_protocol::RequestId::Integer(id))
        .await?;
    Ok(())
}

fn items(body: &Value) -> &Vec<Value> {
    body["input"].as_array().expect("input items")
}

/// The text output delivered for `call_id`.
fn output(body: &Value, call_id: &str) -> String {
    let item = items(body)
        .iter()
        .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
        .unwrap_or_else(|| panic!("no output for {call_id}"));
    match &item["output"] {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Items of `later` that `earlier` did not already carry (by identity): what a request newly
/// delivers. Context delivered before a Forget is not erased from history.
fn new_items<'a>(earlier: &Value, later: &'a Value) -> Vec<&'a Value> {
    let earlier = items(earlier);
    items(later)
        .iter()
        .filter(|item| !earlier.contains(item))
        .collect()
}

fn assert_never_delivered(items: &[&Value], words: &[&str]) {
    for item in items {
        let text = item.to_string();
        for word in words {
            assert!(!text.contains(word), "{word:?} newly delivered in {text}");
        }
    }
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

fn chunk(gate: Option<tokio::sync::oneshot::Receiver<()>>, body: String) -> StreamingSseChunk {
    StreamingSseChunk { gate, body }
}

/// Finding 1: results ready but not yet recorded (the stream's completion is held back) are
/// checked where they are recorded; another pool's Forget during that wait commits at once.
#[tokio::test]
async fn c456r3_results_waiting_for_the_stream_are_checked_where_they_are_recorded() -> Result<()> {
    let read = json!({"question": "ground rules"}).to_string();
    let (gate, held) = tokio::sync::oneshot::channel();
    let (sse, _completions) = start_streaming_sse_server(vec![
        vec![chunk(None, message("r0", "Done."))],
        vec![chunk(
            None,
            responses::sse(vec![
                responses::ev_response_created("r1"),
                responses::ev_function_call("free-control", "memory_read", &read),
                responses::ev_completed("r1"),
            ]),
        )],
        vec![chunk(None, message("r2", "Done."))],
        vec![
            chunk(
                None,
                responses::sse(vec![
                    responses::ev_response_created("r3"),
                    responses::ev_function_call("free-held", "memory_read", &read),
                    responses::ev_function_call("free-later", "memory_read", &read),
                ]),
            ),
            chunk(
                Some(held),
                responses::sse(vec![responses::ev_completed("r3")]),
            ),
        ],
        vec![chunk(None, message("r4", "Done."))],
    ])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(sse.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let latch = install_hook(home.path(), "^memory_read$")?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project = project(&mut server, home.path(), "Stream", /*root*/ None).await?;
    let first = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &first,
        "Ground rules for this project:\n- Never push.",
    )
    .await?;
    let rule = saved(&mut server).await?.remove(0);
    let second = start_thread(&mut server, &project).await?;
    turn(&mut server, &second, "What are the ground rules?").await?;

    begin_turn(&mut server, &second, "Check them again.").await?;
    // Both reads have finished (their hooks ran); the stream has not completed, so neither
    // result is in history yet.
    started(&latch, "free-held").await?;
    started(&latch, "free-later").await?;
    let elapsed = forget(&other_writer(home.path()).await?, &project, &rule).await?;
    assert!(
        elapsed < Duration::from_secs(3),
        "a waiting result blocked another writer for {elapsed:?}"
    );
    gate.send(()).expect("stream still open");
    server
        .read_stream_until_notification_message("turn/completed")
        .await?;

    let bodies = sse
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<Value>(body))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(bodies.len(), 5);
    // Positive control: the same read, recorded with nothing retired, is published.
    let control = output(&bodies[2], "free-control");
    assert!(
        control.contains("Never push") && !control.contains(WITHHELD_READ),
        "{control}"
    );
    for call in ["free-held", "free-later"] {
        let delivered = output(&bodies[4], call);
        assert!(delivered.starts_with(WITHHELD_READ), "{call}: {delivered}");
    }
    assert_never_delivered(&new_items(&bodies[3], &bodies[4]), &["Never push"]);
    Ok(())
}

/// Finding 3: hook context and blocking feedback that echo a result are recorded only when
/// that result's own check passes.
#[tokio::test]
async fn c456r3_hook_echoes_of_a_withheld_result_are_never_recorded() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let latch = install_hook(home.path(), "^memory_read$")?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project = project(&mut server, home.path(), "Echo", /*root*/ None).await?;
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let log = calls.clone();
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("JSON body");
            let mut log = log.lock().expect("request log");
            let read = |id: &str| {
                responses::sse(vec![
                    responses::ev_response_created(id),
                    responses::ev_function_call(
                        id,
                        "memory_read",
                        &json!({"question": "ground rules"}).to_string(),
                    ),
                    responses::ev_completed(id),
                ])
            };
            let response = match log.len() {
                1 => read("ctx-held"),
                2 => read("block-held"),
                3 => read("ctx-fresh"),
                _ => message("done", "Done."),
            };
            log.push(body);
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(response)
        })
        .mount(&responses_server)
        .await;
    let first = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &first,
        "Ground rules for this project:\n- Never push.\n- Always test first.",
    )
    .await?;
    let rules = saved(&mut server).await?;
    let writer = other_writer(home.path()).await?;
    let second = start_thread(&mut server, &project).await?;
    release(&latch, "ctx-fresh")?;
    begin_turn(&mut server, &second, "What are the ground rules?").await?;
    for (call, rule) in [("ctx-held", &rules[0]), ("block-held", &rules[1])] {
        started(&latch, call).await?;
        forget(&writer, &project, rule).await?;
        release(&latch, call)?;
    }
    server
        .read_stream_until_notification_message("turn/completed")
        .await?;

    let bodies = calls.lock().expect("request log").clone();
    assert_eq!(bodies.len(), 5);
    assert!(output(&bodies[2], "ctx-held").starts_with(WITHHELD_READ));
    assert!(output(&bodies[3], "block-held").starts_with(WITHHELD_READ));
    for (earlier, later) in [(1, 2), (2, 3), (3, 4)] {
        assert_never_delivered(
            &new_items(&bodies[earlier], &bodies[later]),
            &["Never push", "Always test first"],
        );
    }
    // Positive control: a current result's echo is still recorded as hook context.
    let fresh = new_items(&bodies[3], &bodies[4]);
    assert!(
        fresh
            .iter()
            .any(|item| item["type"] != "function_call_output"
                && item["type"] != "function_call"
                && item.to_string().contains("coverage")),
        "the fresh read's hook context was not recorded: {fresh:?}"
    );
    Ok(())
}

/// Finding 2: coverage reports and a completion receipt are checked too; a withheld
/// completion says its change was committed, and the run stays completed. (The acceptance
/// check it completes on runs `sh`, as in the other completion tests.)
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn c456r3_held_coverage_and_completion_outputs_are_withheld_truthfully() -> Result<()> {
    use super::super::stateful_acceptance_support::run_check;
    use super::super::stateful_acceptance_support::seed_admitted_plan;
    use super::super::stateful_acceptance_support::write_acceptance_files;
    use codex_app_server_protocol::StatefulRunReadParams;
    use codex_app_server_protocol::StatefulRunReadResponse;
    use codex_app_server_protocol::StatefulRunStartParams;
    use codex_app_server_protocol::StatefulRunStartResponse;
    use codex_app_server_protocol::StatefulRunStatus;
    use codex_app_server_protocol::StatefulWorkflowMode;
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    let root = TempDir::new()?;
    write_acceptance_files(root.path())?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let latch = install_hook(home.path(), "^(context_map_query|stateful_run_update)$")?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project = project(&mut server, home.path(), "Coverage", Some(root.path())).await?;
    responses::mount_sse_once(&responses_server, message("declared", "Done.")).await;
    let first = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &first,
        "Ground rules for this project:\n- Never push.\n- Always test first.",
    )
    .await?;
    let rules = saved(&mut server).await?;
    let thread = start_thread(&mut server, &project).await?;
    let started_run: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.clone(),
                thread_id: thread.clone(),
                goal: "Answer a lookup.".into(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: codex_app_server_protocol::StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "coverage-run".into(),
            },
        })
        .await?;
    seed_admitted_plan(home.path(), &started_run.run.id, &[]).await?;
    let coverage = json!({"text": "ground rules"}).to_string();
    let completion = json!({
        "expectedRevision": started_run.run.revision,
        "status": "completed",
        "openIssues": [],
        "completionDisposition": "noReusableLearning",
        "result": "Nothing to change."
    })
    .to_string();
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            run_check("acceptance-check", root.path()),
            responses::sse(vec![
                responses::ev_response_created("map"),
                responses::ev_function_call("map-held", "context_map_query", &coverage),
                responses::ev_completed("map"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("complete"),
                responses::ev_function_call("complete-held", "stateful_run_update", &completion),
                responses::ev_completed("complete"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("fresh"),
                responses::ev_function_call("free-map", "context_map_query", &coverage),
                responses::ev_completed("fresh"),
            ]),
            message("done", "Nothing to change."),
        ],
    )
    .await;
    let writer = other_writer(home.path()).await?;
    begin_turn(&mut server, &thread, "Is there anything to change?").await?;
    for (call, rule) in [("map-held", &rules[0]), ("complete-held", &rules[1])] {
        started(&latch, call).await?;
        forget(&writer, &project, rule).await?;
        release(&latch, call)?;
    }
    server
        .read_stream_until_notification_message("turn/completed")
        .await?;

    let map = log
        .function_call_output_text("map-held")
        .expect("map output");
    assert!(map.starts_with(WITHHELD_READ), "{map}");
    let completed = log
        .function_call_output_text("complete-held")
        .expect("completion output");
    assert!(completed.starts_with(WITHHELD_COMMITTED), "{completed}");
    // Positive control: the same coverage read after both Forgets is published.
    let fresh = log
        .function_call_output_text("free-map")
        .expect("fresh map output");
    assert!(!fresh.contains(WITHHELD_READ), "{fresh}");
    // The completion was committed: the run is completed, so the model must not repeat it.
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started_run.run.id.clone()),
                thread_id: None,
            },
        })
        .await?;
    assert_eq!(read.run.expect("run").status, StatefulRunStatus::Completed);
    Ok(())
}
