use codex_extension_api::PreviousWorldStateSection;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewWindowEvent;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WindowEventKind;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::capsule_bytes;
use super::capsule_section;
use super::window_capsule;

async fn store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store")
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

fn command(command: &str, exit_code: i64, output: &str) -> Value {
    json!({
        "command": command,
        "status": if exit_code == 0 { "completed" } else { "failed" },
        "exitCode": exit_code,
        "outputBytes": output.len(),
        "outputTail": output,
    })
}

fn edit(path: &std::path::Path) -> Value {
    json!({"status": "applied", "paths": [{"path": path.display().to_string(), "change": "update"}], "morePaths": 0})
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
    append(&store, "e1", WindowEventKind::Edit, edit(&source)).await;
    append(
        &store,
        "c1",
        WindowEventKind::Command,
        command(
            "bash -lc pytest -q",
            1,
            "FAILED test_eggs\n1 failed, 4 passed",
        ),
    )
    .await;
    append(
        &store,
        "p1",
        WindowEventKind::Plan,
        json!({"steps": [
            {"step": "Reproduce the egg rounding failure", "status": "completed"},
            {"step": "Round eggs at output, not input", "status": "in_progress"},
            {"step": "Rerun the tests", "status": "pending"},
        ]}),
    )
    .await;
    append(&store, "e2", WindowEventKind::Edit, edit(&worklog)).await;

    let capsule = window_capsule(&store, "thread-1", "window-2", capsule_bytes(None))
        .await
        .expect("capsule");
    let body = &capsule.body;
    assert_eq!(capsule.through_seq, 4);
    assert!(
        body.contains("Next step (the agent's last update_plan, 1 observations before compaction): \"Round eggs at output, not input\"."),
        "{body}"
    );
    assert!(
        body.contains("Last test or check command: `bash -lc pytest -q` exit 1; files were changed after it, so it may no longer hold."),
        "{body}"
    );
    assert!(body.contains("1 failed, 4 passed"), "{body}");
    assert!(body.contains("scale.py: last patch applied;"), "{body}");
    assert!(body.contains("sha256"), "{body}");
    assert!(body.contains("the end of its lines 21-60 of 60"), "{body}");
    assert!(body.contains("entry 60"), "{body}");
    assert!(!body.contains("entry 20\n"), "{body}");
    assert!(body.len() + "<stateful_task_capsule></stateful_task_capsule>".len() <= 4_096);

    // The capsule is captured once per window.
    append(
        &store,
        "c2",
        WindowEventKind::Command,
        command("bash -lc pytest -q", 0, "5 passed"),
    )
    .await;
    assert_eq!(
        window_capsule(&store, "thread-1", "window-2", capsule_bytes(None)).await,
        Some(capsule.clone())
    );
}

#[tokio::test]
async fn without_a_plan_the_capsule_quotes_or_says_unknown_and_never_claims_a_pass() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    append(
        &store,
        "c1",
        WindowEventKind::Command,
        command("cargo test -p scaler && cargo fmt", 0, "test result: ok"),
    )
    .await;
    let unknown = window_capsule(&store, "thread-1", "window-1", capsule_bytes(None))
        .await
        .expect("capsule");
    assert!(
        unknown.body.contains("Next step: unknown."),
        "{}",
        unknown.body
    );
    assert!(
        unknown
            .body
            .contains("(exit 0, the process exit code, not a test count;"),
        "{}",
        unknown.body
    );
    assert!(!unknown.body.contains("passed"), "{}", unknown.body);

    append(
        &store,
        "m1",
        WindowEventKind::Message,
        json!({"phase": "commentary", "text": "I will add the crepes recipe next."}),
    )
    .await;
    append(
        &store,
        "u1",
        WindowEventKind::User,
        json!({"text": "Actually stop and ask first."}),
    )
    .await;
    let quoted = window_capsule(&store, "thread-1", "window-2", capsule_bytes(Some(60_000)))
        .await
        .expect("capsule");
    assert!(
        quoted.body.contains("Last announced intention (the agent's own words, 1 observations before compaction): \"I will add the crepes recipe next.\". A later user message may have changed it"),
        "{}",
        quoted.body
    );
    assert!(quoted.body.len() + "<stateful_task_capsule></stateful_task_capsule>".len() <= 2_048);
}

#[tokio::test]
async fn the_capsule_renders_once_and_later_edits_add_one_notice() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    append(
        &store,
        "c1",
        WindowEventKind::Command,
        command("go test ./...", 1, "FAIL"),
    )
    .await;
    let capsule = window_capsule(&store, "thread-1", "window-1", capsule_bytes(None))
        .await
        .expect("capsule");
    let fresh = capsule_section("window-1", &capsule, /*stale*/ false);
    let stale = capsule_section("window-1", &capsule, /*stale*/ true);
    let rendered = |section: &codex_extension_api::WorldStateSectionContribution,
                    previous: PreviousWorldStateSection<'_>| {
        section
            .render_diff(previous)
            .map(|fragment| fragment.body().to_string())
    };
    assert_eq!(
        rendered(&fresh, PreviousWorldStateSection::Absent),
        Some(capsule.body.clone())
    );
    assert_eq!(
        rendered(&fresh, PreviousWorldStateSection::Known(fresh.snapshot())),
        None
    );
    let notice =
        rendered(&stale, PreviousWorldStateSection::Known(fresh.snapshot())).expect("notice");
    assert!(notice.contains("now historical"), "{notice}");
    assert_eq!(
        rendered(&stale, PreviousWorldStateSection::Known(stale.snapshot())),
        None
    );
}

/// within1: the agent learned that pytest needs a project-local temp directory on this Windows
/// host (the default one fails with an ACL error), then compaction came. The capsule must carry
/// the working form verbatim, with its directory, and the failing form it replaced, even when a
/// later unrelated command failed and the budget is the small one.
#[tokio::test]
async fn the_capsule_keeps_the_last_working_validation_and_the_workaround_it_needed() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    let failing = "powershell -Command python -m pytest -q";
    let working =
        r"powershell -Command $env:TMP='C:\work\.tmp'; python -m pytest -q --basetemp=.pytest_tmp";
    let mut failing_payload = command(
        failing,
        1,
        r"PermissionError: [WinError 5] Access is denied: 'C:\Users\me\AppData\Local\Temp\pytest-of-me'",
    );
    failing_payload["cwd"] = json!(r"C:\work\recipes");
    let mut working_payload = command(working, 0, "12 passed in 0.40s");
    working_payload["cwd"] = json!(r"C:\work\recipes");
    append(&store, "c1", WindowEventKind::Command, failing_payload).await;
    append(&store, "c2", WindowEventKind::Command, working_payload).await;
    append(
        &store,
        "c3",
        WindowEventKind::Command,
        command("powershell -Command git push", 128, "fatal: no upstream"),
    )
    .await;
    let capsule = window_capsule(&store, "thread-1", "window-2", capsule_bytes(Some(60_000)))
        .await
        .expect("capsule");
    let body = &capsule.body;
    assert!(
        body.contains(&format!(
            r"Last working test or check command (exit 0, the process exit code, not a test count; reuse it verbatim, with its working directory and any environment settings it contains): `{working}` in C:\work\recipes. It replaced a form that failed: `{failing}` exit 1."
        )),
        "{body}"
    );
    assert!(
        body.contains(
            "Latest command without exit code 0: `powershell -Command git push` exit 128."
        ),
        "{body}"
    );
    assert!(body.len() + "<stateful_task_capsule></stateful_task_capsule>".len() <= 2_048);
}
