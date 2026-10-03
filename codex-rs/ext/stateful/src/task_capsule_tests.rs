use std::path::PathBuf;
use std::time::Duration;

use codex_extension_api::PreviousWorldStateSection;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::ExecCommandSource;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewWindowEvent;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WindowEventKind;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::capsule_caps;
use super::capsule_section;
use super::estimated_tokens;
use super::window_capsule;
use crate::window_capture::journal_item;

const SMALL: Option<i64> = Some(50_000);

async fn store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store")
}

/// Journals a real command item, as the host does when it completes.
async fn run(
    store: &StatefulRunStore,
    turn: &str,
    id: &str,
    argv: &[&str],
    exit_code: i32,
    output: &str,
) {
    let cwd = TempDir::new().expect("cwd");
    journal_item(
        store,
        "project-1",
        "thread-1",
        turn,
        &TurnItem::CommandExecution(CommandExecutionItem {
            model_context: None,
            sandbox_type: None,
            id: id.to_string(),
            plugin_id: None,
            script_path: None,
            process_id: None,
            command: argv.iter().map(|word| word.to_string()).collect(),
            cwd: PathUri::from_abs_path(&cwd.path().abs()),
            parsed_cmd: Vec::new(),
            source: ExecCommandSource::Agent,
            interaction_input: None,
            status: if exit_code == 0 {
                CommandExecutionStatus::Completed
            } else {
                CommandExecutionStatus::Failed
            },
            stdout: Some(output.to_string()),
            stderr: Some(String::new()),
            aggregated_output: Some(output.to_string()),
            exit_code: Some(exit_code),
            duration: Some(Duration::from_millis(5)),
            formatted_output: None,
        }),
    )
    .await;
}

async fn say(store: &StatefulRunStore, turn: &str, id: &str, phase: MessagePhase, text: &str) {
    journal_item(
        store,
        "project-1",
        "thread-1",
        turn,
        &TurnItem::AgentMessage(AgentMessageItem {
            id: id.to_string(),
            content: vec![AgentMessageContent::Text {
                text: text.to_string(),
            }],
            phase: Some(phase),
            memory_citation: None,
            delivery: None,
            questions: None,
        }),
    )
    .await;
}

async fn append(store: &StatefulRunStore, key: &str, kind: WindowEventKind, payload: Value) {
    store
        .append_window_event(&NewWindowEvent {
            thread_id: "thread-1".to_string(),
            event_key: key.to_string(),
            project_id: "project-1".to_string(),
            turn_id: "turn-1".to_string(),
            kind,
            payload,
        })
        .await
        .expect("append");
}

fn edit(path: &std::path::Path, status: &str) -> Value {
    json!({"status": status, "paths": [{"path": path.display().to_string(), "change": "update"}], "morePaths": 0})
}

async fn capsule(
    store: &StatefulRunStore,
    window: &str,
    roots: Vec<PathBuf>,
    limit: Option<i64>,
) -> String {
    window_capsule(
        store,
        ("thread-1", "project-1", window),
        roots,
        capsule_caps(limit),
    )
    .await
    .expect("capsule")
    .body
}

fn within(body: &str, caps_bytes: usize, caps_tokens: usize) {
    let wrapped = format!("<stateful_task_capsule>{body}</stateful_task_capsule>");
    assert!(
        wrapped.len() <= caps_bytes,
        "{} bytes: {body}",
        wrapped.len()
    );
    assert!(estimated_tokens(&wrapped) <= caps_tokens, "{body}");
}

#[tokio::test]
async fn the_capsule_reports_observations_and_the_agents_own_next_step() {
    let home = TempDir::new().expect("home");
    let work = TempDir::new().expect("work");
    let source = work.path().join("scale.py");
    let worklog = work.path().join("WORKLOG.md");
    std::fs::write(&source, "def scale(eggs):\n    return round(eggs)\n").expect("source");
    std::fs::write(
        &worklog,
        (1..=60)
            .map(|line| format!("entry {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("worklog");
    let store = store(&home).await;
    append(
        &store,
        "e1",
        WindowEventKind::Edit,
        edit(&source, "applied"),
    )
    .await;
    run(
        &store,
        "turn-1",
        "c1",
        &["bash", "-lc", "pytest -q"],
        1,
        "FAILED test_eggs\n1 failed, 4 passed",
    )
    .await;
    append(
        &store,
        "p1",
        WindowEventKind::Plan,
        json!({"steps": [
            {"step": "Reproduce the egg rounding failure", "status": "completed"},
            {"step": "Round eggs at output, not input", "status": "in_progress"},
        ]}),
    )
    .await;
    append(
        &store,
        "e2",
        WindowEventKind::Edit,
        edit(&worklog, "applied"),
    )
    .await;
    let body = capsule(&store, "window-2", vec![work.path().to_path_buf()], None).await;
    assert!(
        body.contains("Next step (the agent's last update_plan, 1 observations before compaction): \"Round eggs at output, not input\"."),
        "{body}"
    );
    assert!(
        body.contains("Last test or check command (exit 1): `pytest -q`; files were patched after it, so it may no longer hold."),
        "{body}"
    );
    assert!(body.contains("1 failed, 4 passed"), "{body}");
    assert!(
        body.contains("scale.py: in a patch that applied;"),
        "{body}"
    );
    assert!(body.contains("the end of its lines 21-60 of 60"), "{body}");
    within(&body, 4_096, 1_024);
    // Captured once per window.
    run(
        &store,
        "turn-1",
        "c2",
        &["bash", "-lc", "pytest -q"],
        0,
        "5 passed",
    )
    .await;
    assert_eq!(capsule(&store, "window-2", Vec::new(), None).await, body);
}

#[tokio::test]
async fn files_outside_the_project_roots_are_not_read() {
    let home = TempDir::new().expect("home");
    let elsewhere = TempDir::new().expect("elsewhere");
    let notes = elsewhere.path().join("PROGRESS.md");
    std::fs::write(&notes, "secret host-local progress").expect("notes");
    let store = store(&home).await;
    append(&store, "e1", WindowEventKind::Edit, edit(&notes, "applied")).await;
    let body = capsule(&store, "window-2", Vec::new(), None).await;
    assert!(
        body.contains("not observed (outside the project roots on this host)"),
        "{body}"
    );
    assert!(!body.contains("secret host-local progress"), "{body}");
}

#[tokio::test]
async fn without_an_open_plan_the_capsule_quotes_or_says_unknown_and_never_claims_a_pass() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    run(
        &store,
        "turn-1",
        "c1",
        &["bash", "-lc", "cargo test -p scaler && cargo fmt"],
        0,
        "test result: ok",
    )
    .await;
    // The newest plan has every step done: an older open step is not resurrected.
    append(
        &store,
        "p0",
        WindowEventKind::Plan,
        json!({"steps": [{"step": "Implement loader", "status": "in_progress"}]}),
    )
    .await;
    append(
        &store,
        "p1",
        WindowEventKind::Plan,
        json!({"steps": [{"step": "Implement loader", "status": "completed"}]}),
    )
    .await;
    let body = capsule(&store, "window-1", Vec::new(), None).await;
    assert!(body.contains("marked every step done"), "{body}");
    assert!(!body.contains("\"Implement loader\""), "{body}");
    assert!(
        body.contains("(exit 0, a process exit code, not a test count)"),
        "{body}"
    );
    assert!(!body.contains("passed"), "{body}");

    say(
        &store,
        "turn-2",
        "m1",
        MessagePhase::Commentary,
        "I will add the crepes recipe next.",
    )
    .await;
    append(
        &store,
        "u1",
        WindowEventKind::User,
        json!({"text": "Actually stop and ask first."}),
    )
    .await;
    let body = capsule(&store, "window-2", Vec::new(), SMALL).await;
    assert!(
        body.contains("Last announced intention (the agent's own words, 1 observations before compaction): \"I will add the crepes recipe next.\". A later user message may have changed it"),
        "{body}"
    );
    within(&body, 2_048, 512);
}

#[tokio::test]
async fn the_capsule_renders_once_and_later_work_adds_one_notice() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    run(
        &store,
        "turn-1",
        "c1",
        &["bash", "-lc", "go test ./..."],
        1,
        "FAIL",
    )
    .await;
    let stored = window_capsule(
        &store,
        ("thread-1", "project-1", "window-1"),
        Vec::new(),
        capsule_caps(None),
    )
    .await
    .expect("capsule");
    let fresh = capsule_section("window-1", &stored, /*stale*/ false);
    let stale = capsule_section("window-1", &stored, /*stale*/ true);
    let rendered = |section: &codex_extension_api::WorldStateSectionContribution,
                    previous: PreviousWorldStateSection<'_>| {
        section
            .render_diff(previous)
            .map(|fragment| fragment.body().to_string())
    };
    assert_eq!(
        rendered(&fresh, PreviousWorldStateSection::Absent),
        Some(stored.body.clone())
    );
    assert_eq!(
        rendered(&fresh, PreviousWorldStateSection::Known(fresh.snapshot())),
        None
    );
    let notice =
        rendered(&stale, PreviousWorldStateSection::Known(fresh.snapshot())).expect("notice");
    assert!(notice.contains("may be historical"), "{notice}");
    assert_eq!(
        rendered(&stale, PreviousWorldStateSection::Known(stale.snapshot())),
        None
    );
}

/// within1 N5 and N7 under pressure: a working pytest route found early (after a failing form),
/// then many failing runs, a search that only mentions pytest, a commentary-heavy detour and
/// long commands, before compaction at the 50k caps. The route stays verbatim, a mention does
/// not replace it, and the next step stated at the end of t05 survives.
#[tokio::test]
async fn the_next_step_before_a_detour_and_the_working_route_survive_pressure() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    let failing = "python -m pytest tests/test_config.py -q";
    let working = r"$env:TMP='C:\tmpdir'; python -m pytest tests/test_config.py -q --basetemp C:\tmpdir\click-config-tests";
    run(
        &store,
        "t02",
        "t02-c1",
        &["pwsh", "-Command", failing],
        1,
        "PermissionError: [WinError 5]",
    )
    .await;
    run(
        &store,
        "t02",
        "t02-c2",
        &["pwsh", "-Command", working],
        0,
        "12 passed",
    )
    .await;
    for index in 0..20 {
        run(
            &store,
            "t03",
            &format!("t03-f{index}"),
            &["pwsh", "-Command", "python -m pytest -q"],
            1,
            "WinError 5",
        )
        .await;
    }
    run(
        &store,
        "t04",
        "t04-rg",
        &["pwsh", "-Command", "rg pytest README.md"],
        0,
        "README.md: pytest",
    )
    .await;
    let closing = format!(
        "{}Next step: add the INI and JSON loaders behind the same loader interface, then the layering tests.",
        "Implemented TOML loading and precedence. ".repeat(40)
    );
    say(
        &store,
        "t05",
        "t05-final",
        MessagePhase::FinalAnswer,
        &closing,
    )
    .await;
    append(
        &store,
        "t06-u1",
        WindowEventKind::User,
        json!({"text": "Quick detour: profile the help output."}),
    )
    .await;
    for index in 0..40 {
        say(
            &store,
            "t06",
            &format!("t06-m{index}"),
            MessagePhase::Commentary,
            &format!("Profiling step {index}."),
        )
        .await;
        run(
            &store,
            "t06",
            &format!("t06-c{index}"),
            &[
                "pwsh",
                "-Command",
                &format!("python bench.py --case {index} {}", "x".repeat(400)),
            ],
            0,
            "ok",
        )
        .await;
    }
    say(
        &store,
        "t06",
        "t06-final",
        MessagePhase::FinalAnswer,
        "Profiled: help rendering is 4 ms.",
    )
    .await;

    let body = capsule(&store, "window-9", Vec::new(), SMALL).await;
    assert!(body.contains(&format!("`{working}`")), "{body}");
    assert!(body.contains("run by pwsh"), "{body}");
    assert!(
        body.contains("Next step: add the INI and JSON loaders behind the same loader interface, then the layering tests."),
        "{body}"
    );
    assert!(body.contains("Profiled: help rendering is 4 ms."), "{body}");
    within(&body, 2_048, 512);
}
