//! Stateful project context across compaction boundaries.
//!
//! The full project root must be present exactly once in every model request: installed inside
//! the replacement history by mid-turn compaction, not repeated on an unchanged step, updated by a
//! delta when the root changes, reinjected after a checkpoint that dropped it, and preserved by a
//! cold resume.

use std::collections::BTreeMap;

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
use tempfile::TempDir;

use super::stateful_project_context::promote_root_fact;
use super::stateful_project_context::seed_root_blackboard;

const FULL_ROOT: &str = "<stateful_project>";
const ROOT_UPDATE: &str = "<stateful_project_update>";
const SEEDED_FACT: &str = "A decisive project fact survives every thread view.";
const LATER_FACT: &str = "A later project decision changes the root.";

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
            reply("after-checkpoint", /*total_tokens*/ 120),
            reply("after-resume", /*total_tokens*/ 120),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .with_root_config("model_auto_compact_token_limit = 200000")
        .with_provider_config("supports_websockets = false")
        .write(codex_home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
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
    let thread_id = app
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;

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
    run_turn(&mut app, &thread_id).await?;

    drop(app);
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let resumed: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            },
        })
        .await?;
    run_turn(&mut app, &resumed.thread.id).await?;

    let requests = mock.requests();
    let [
        over_limit,
        _mid_turn_summary,
        mid_turn_continuation,
        unchanged_step,
        revised_step,
        _manual_summary,
        after_checkpoint,
        after_resume,
    ] = requests.as_slice()
    else {
        panic!("expected eight model requests, got {}", requests.len());
    };
    assert_eq!(
        [
            over_limit,
            mid_turn_continuation,
            unchanged_step,
            revised_step,
            after_checkpoint,
            after_resume,
        ]
        .map(|request| RootView::of(request, &project_id)),
        [
            RootView::seeded(/*updates*/ 0),
            RootView::seeded(/*updates*/ 0),
            RootView::seeded(/*updates*/ 0),
            RootView::revised(/*updates*/ 1),
            RootView::revised(/*updates*/ 0),
            RootView::revised(/*updates*/ 0),
        ]
    );
    Ok(())
}

/// What one model request shows of the project root.
#[derive(Debug, PartialEq, Eq)]
struct RootView {
    full_roots: usize,
    updates: usize,
    has_project_id: bool,
    has_seeded_fact: bool,
    has_later_fact: bool,
}

impl RootView {
    fn of(request: &ResponsesRequest, project_id: &str) -> Self {
        let body = request.body_json();
        // Count fragments by their opening marker; update prose may mention the root marker.
        let fragments_opening_with = |marker: &str| {
            body["input"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["content"].as_array())
                .flatten()
                .filter_map(|content| content["text"].as_str())
                .filter(|text| text.trim_start().starts_with(marker))
                .count()
        };
        let body_text = body.to_string();
        Self {
            full_roots: fragments_opening_with(FULL_ROOT),
            updates: fragments_opening_with(ROOT_UPDATE),
            has_project_id: body_text.contains(&format!("Project ID: {project_id}")),
            has_seeded_fact: body_text.contains(SEEDED_FACT),
            has_later_fact: body_text.contains(LATER_FACT),
        }
    }

    fn seeded(updates: usize) -> Self {
        Self {
            full_roots: 1,
            updates,
            has_project_id: true,
            has_seeded_fact: true,
            has_later_fact: false,
        }
    }

    fn revised(updates: usize) -> Self {
        Self {
            has_later_fact: true,
            ..Self::seeded(updates)
        }
    }
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
