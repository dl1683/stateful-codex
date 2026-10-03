//! Stateful project context across compaction boundaries.
//!
//! Every model request carries exactly one project packet. The thread's first window holds the
//! full root; a window opened by compaction in the same thread holds the continuation packet
//! (rules and a memory pointer) beside the native summary: installed in the replacement history
//! by mid-turn compaction, not repeated on an unchanged step, amended by a one-line receipt when
//! only the root revision changes, left out of a manual checkpoint and injected on the next turn,
//! and kept, not replaced by the full root, when the thread is cold-resumed.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use core_test_support::responses::ResponsesRequest;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::stateful_project_context::promote_root_fact;
use super::stateful_project_context::seed_root_blackboard;

const ROOT: (&str, &str) = ("<stateful_project>", "</stateful_project>");
const CONTINUITY: (&str, &str) = ("<stateful_continuity>", "</stateful_continuity>");
const UPDATE: (&str, &str) = ("<stateful_project_update>", "</stateful_project_update>");
const SEEDED_FACT: &str = "A decisive project fact survives every thread view.";
const LATER_FACT: &str = "A later project decision changes the root.";
const COMPACT_PROMPT: &str = "Summarize the Stateful conversation.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_root_appears_once_across_compaction_revision_and_resume() -> Result<()> {
    let server = responses::start_mock_server().await;
    let reply = |id: &str, total_tokens| {
        responses::sse(vec![
            responses::ev_assistant_message(&format!("{id}-message"), "Done"),
            responses::ev_completed_with_tokens(id, total_tokens),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            // A tool call whose usage crosses the limit forces compaction inside the turn.
            responses::sse(vec![
                responses::ev_function_call("call-1", "unsupported_tool", "{}"),
                responses::ev_completed_with_tokens("over-limit", /*total_tokens*/ 330_000),
            ]),
            reply("mid-turn-summary", /*total_tokens*/ 200),
            reply("mid-turn-continuation", /*total_tokens*/ 120),
            reply("unchanged-step", /*total_tokens*/ 120),
            reply("revised-step", /*total_tokens*/ 120),
            reply("manual-summary", /*total_tokens*/ 200),
            reply("resumed-from-checkpoint", /*total_tokens*/ 120),
            reply("unchanged-after-resume", /*total_tokens*/ 120),
            reply("resumed-from-repaired", /*total_tokens*/ 120),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .with_root_config(&format!(
            "compact_prompt = \"{COMPACT_PROMPT}\"\nmodel_auto_compact_token_limit = 200000"
        ))
        .with_provider_config("supports_websockets = false")
        .write(codex_home.path())?;
    let mut app = start_app(codex_home.path()).await?;
    let created: ProjectCreateResponse = app
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Compaction Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-compaction-project".to_string(),
            },
        })
        .await?;
    let project_id = created.project.id;
    seed_root_blackboard(codex_home.path(), &project_id).await?;
    let thread = app
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread;
    let thread_id = thread.id;
    let rollout = thread.path.expect("thread should be persisted");

    run_turn(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;
    promote_root_fact(
        codex_home.path(),
        &project_id,
        &format!("later-fact-{project_id}"),
        LATER_FACT,
    )
    .await?;
    run_turn(&mut app, &thread_id).await?;
    let compact_request = app
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: thread_id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse = app.read_response(compact_request).await?;
    let _: TurnCompletedNotification = app.read_notification("turn/completed").await?;

    // Restart straight from the manual checkpoint, whose history holds no root.
    drop(app);
    let mut app = start_app(codex_home.path()).await?;
    resume(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;
    // Restart again from history that already carries the reinjected root.
    drop(app);
    let mut app = start_app(codex_home.path()).await?;
    resume(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;

    let requests = mock.requests();
    let [
        over_limit,
        mid_turn_summary,
        mid_turn_continuation,
        unchanged_step,
        revised_step,
        manual_summary,
        resumed_from_checkpoint,
        unchanged_after_resume,
        resumed_from_repaired,
    ] = requests.as_slice()
    else {
        panic!("expected nine model requests, got {}", requests.len());
    };

    assert_eq!(
        [mid_turn_summary, manual_summary].map(compaction_metadata),
        [
            json!({
                "request_kind": "compaction",
                "trigger": "auto",
                "phase": "mid_turn",
                "implementation": "responses",
                "prompt": true,
            }),
            json!({
                "request_kind": "compaction",
                "trigger": "manual",
                "phase": "standalone_turn",
                "implementation": "responses",
                "prompt": true,
            }),
        ]
    );

    let seeded = RootView::of(over_limit, &project_id);
    let revised = RootView::of(revised_step, &project_id);
    assert_eq!(
        [
            over_limit,
            mid_turn_summary,
            mid_turn_continuation,
            unchanged_step,
            revised_step,
            manual_summary,
            resumed_from_checkpoint,
            unchanged_after_resume,
            resumed_from_repaired,
        ]
        .map(|request| RootView::of(request, &project_id)),
        [
            RootView::seeded(seeded.root_revision),
            RootView::seeded(seeded.root_revision),
            RootView::continuation(seeded.root_revision),
            RootView::continuation(seeded.root_revision),
            RootView::receipt(seeded.root_revision, revised.update_revision),
            RootView::receipt(seeded.root_revision, revised.update_revision),
            RootView::continuation(revised.update_revision),
            RootView::continuation(revised.update_revision),
            RootView::continuation(revised.update_revision),
        ]
    );
    assert!(seeded.root_revision.is_some() && revised.update_revision > seeded.root_revision);

    // The mid-turn checkpoint installs the packet; the manual checkpoint leaves it to the next turn.
    assert_eq!(checkpoint_root_counts(&rollout)?, vec![1, 0]);

    // The thread's own conversation record belongs to its first window only: after compaction
    // the summary and retained messages carry it, and this project has no other thread.
    assert_eq!(
        requests
            .iter()
            .map(|request| continuity_records(request).len())
            .collect::<Vec<_>>(),
        vec![1, 1, 0, 0, 0, 0, 0, 0, 0]
    );
    Ok(())
}

/// Repeated mid-turn compactions and a compaction at a turn boundary never reinstall the
/// thread's own conversation record beside the native summary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn own_continuity_record_stays_in_the_first_window_across_compactions() -> Result<()> {
    let server = responses::start_mock_server().await;
    let over_limit_call = |id: &str| {
        responses::sse(vec![
            responses::ev_function_call(&format!("{id}-call"), "unsupported_tool", "{}"),
            responses::ev_completed_with_tokens(id, /*total_tokens*/ 330_000),
        ])
    };
    let reply = |id: &str, total_tokens| {
        responses::sse(vec![
            responses::ev_assistant_message(&format!("{id}-message"), "Done"),
            responses::ev_completed_with_tokens(id, total_tokens),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            over_limit_call("first-over-limit"),
            reply("first-summary", /*total_tokens*/ 200),
            over_limit_call("second-over-limit"),
            reply("second-summary", /*total_tokens*/ 200),
            reply("over-limit-answer", /*total_tokens*/ 330_000),
            reply("boundary-summary", /*total_tokens*/ 200),
            reply("next-turn", /*total_tokens*/ 120),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .with_root_config(&format!(
            "compact_prompt = \"{COMPACT_PROMPT}\"
model_auto_compact_token_limit = 200000"
        ))
        .with_provider_config("supports_websockets = false")
        .write(codex_home.path())?;
    let mut app = start_app(codex_home.path()).await?;
    let created: ProjectCreateResponse = app
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Repeated compaction".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "repeated-compaction-project".to_string(),
            },
        })
        .await?;
    let thread_id = app
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;

    let requests = mock.requests();
    let phases = requests
        .iter()
        .map(|request| {
            let metadata: Value = serde_json::from_str(
                &request
                    .header("x-codex-turn-metadata")
                    .unwrap_or_else(|| "{}".to_string()),
            )
            .unwrap_or_default();
            metadata["compaction"]["phase"].as_str().map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        vec![
            None,
            Some("mid_turn".to_string()),
            None,
            Some("mid_turn".to_string()),
            None,
            Some("pre_turn".to_string()),
            None,
        ]
    );
    let records = requests
        .iter()
        .zip(&phases)
        .filter(|(_, phase)| phase.is_none())
        .map(|(request, _)| continuity_records(request))
        .collect::<Vec<_>>();
    assert_eq!(
        records.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![1, 0, 0, 0]
    );
    Ok(())
}

/// Well-formed continuity records in one model request.
fn continuity_records(request: &ResponsesRequest) -> Vec<String> {
    request.body_json()["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["role"] == "developer")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .map(str::trim)
        .filter(|text| {
            text.starts_with(CONTINUITY.0)
                && text.ends_with(CONTINUITY.1)
                && text.matches(CONTINUITY.0).count() == 1
        })
        .map(str::to_string)
        .collect()
}

/// What one model request shows of the project root, read only from recognized fragments.
#[derive(Debug, PartialEq, Eq)]
struct RootView {
    /// Developer-role fragments opened and closed by the full-root markers.
    full_roots: usize,
    /// Developer-role fragments opened and closed by the update markers.
    updates: usize,
    /// Fragments opened by either marker with another role, without their closing marker, or
    /// holding more than one pair of their markers.
    malformed: usize,
    root_has_project_id: bool,
    root_has_seeded_fact: bool,
    root_has_later_fact: bool,
    update_has_later_fact: bool,
    root_revision: Option<u64>,
    update_revision: Option<u64>,
}

impl RootView {
    fn of(request: &ResponsesRequest, project_id: &str) -> Self {
        let body = request.body_json();
        let messages = body["input"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|item| {
                let role = item["role"].as_str().unwrap_or_default().to_string();
                item["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|content| content["text"].as_str())
                    .map(move |text| (role.clone(), text.trim().to_string()))
            })
            .collect::<Vec<_>>();
        let mut malformed = 0;
        let mut fragments = |(open, close): (&str, &str)| {
            let (valid, invalid): (Vec<_>, Vec<_>) = messages
                .iter()
                .filter(|(_, text)| text.starts_with(open))
                .partition(|(role, text)| {
                    role == "developer"
                        && text.ends_with(close)
                        && text.matches(open).count() == 1
                        && text.matches(close).count() == 1
                });
            malformed += invalid.len();
            valid
                .into_iter()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>()
        };
        let roots = fragments(ROOT);
        let updates = fragments(UPDATE);
        let root_text = roots.join("\n");
        let update_text = updates.join("\n");
        Self {
            full_roots: roots.len(),
            updates: updates.len(),
            malformed,
            root_has_project_id: root_text.contains(&format!("Project ID: {project_id}")),
            root_has_seeded_fact: root_text.contains(SEEDED_FACT),
            root_has_later_fact: root_text.contains(LATER_FACT),
            update_has_later_fact: update_text.contains(LATER_FACT),
            root_revision: number_after(&root_text, "Project intelligence revision: "),
            update_revision: number_after(&update_text, "rootRevision "),
        }
    }

    fn seeded(root_revision: Option<u64>) -> Self {
        Self {
            full_roots: 1,
            updates: 0,
            malformed: 0,
            root_has_project_id: true,
            root_has_seeded_fact: true,
            root_has_later_fact: false,
            update_has_later_fact: false,
            root_revision,
            update_revision: None,
        }
    }

    fn amended(root_revision: Option<u64>, update_revision: Option<u64>) -> Self {
        Self {
            updates: 1,
            update_has_later_fact: true,
            update_revision,
            ..Self::seeded(root_revision)
        }
    }

    /// The continuation packet: project identity and rules, not the promoted knowledge.
    fn continuation(root_revision: Option<u64>) -> Self {
        Self {
            root_has_seeded_fact: false,
            ..Self::seeded(root_revision)
        }
    }

    /// A continuation packet amended by a revision-only receipt that repeats no knowledge.
    fn receipt(root_revision: Option<u64>, update_revision: Option<u64>) -> Self {
        Self {
            updates: 1,
            update_revision,
            ..Self::continuation(root_revision)
        }
    }
}

fn number_after(text: &str, label: &str) -> Option<u64> {
    let (_, rest) = text.split_once(label)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn compaction_metadata(request: &ResponsesRequest) -> Value {
    let metadata: Value = serde_json::from_str(
        &request
            .header("x-codex-turn-metadata")
            .expect("compaction request should carry turn metadata"),
    )
    .expect("turn metadata should be JSON");
    json!({
        "request_kind": metadata["request_kind"],
        "trigger": metadata["compaction"]["trigger"],
        "phase": metadata["compaction"]["phase"],
        "implementation": metadata["compaction"]["implementation"],
        "prompt": request.body_json().to_string().contains(COMPACT_PROMPT),
    })
}

/// Counts full-root fragments in each persisted compaction checkpoint, oldest first.
fn checkpoint_root_counts(rollout: &Path) -> Result<Vec<usize>> {
    let mut counts = Vec::new();
    for line in std::fs::read_to_string(rollout)?.lines() {
        let item: Value = serde_json::from_str(line)?;
        if item["type"] != "compacted" {
            continue;
        }
        // Count root markers inside root fragments, so two roots in one text count twice.
        let roots = item["payload"]["replacement_history"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|content| content["text"].as_str())
            .filter(|text| text.trim_start().starts_with(ROOT.0))
            .map(|text| text.matches(ROOT.0).count())
            .sum();
        counts.push(roots);
    }
    Ok(counts)
}

async fn start_app(codex_home: &Path) -> Result<TestAppServer> {
    TestAppServer::builder()
        .with_codex_home(codex_home)
        .build_initialized()
        .await
}

async fn resume(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let _: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.to_string(),
                ..Default::default()
            },
        })
        .await?;
    Ok(())
}

async fn run_turn(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    app.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread_id.to_string(),
        input: vec![UserInput::Text {
            text: "Continue the project work.".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    })
    .await?;
    Ok(())
}
