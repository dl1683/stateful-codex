//! A continuation window (opened by compaction in the same thread) through the public API:
//! it carries one task capsule built from what the host observed, knowledge it does not show
//! stays selectable for completion, and selecting another project in the thread starts that
//! project's full packet instead of a continuation.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadMetadataUpdateParams;
use codex_app_server_protocol::ThreadMetadataUpdateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::responses::ResponsesRequest;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::stateful_project_context::promote_root_fact;
use super::stateful_project_context::seed_root_blackboard;

const SEEDED_FACT: &str = "A decisive project fact survives every thread view.";
const OTHER_PROJECT_FACT: &str = "The other project ships on Fridays only.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn continuation_keeps_hidden_findings_selectable_and_a_new_project_starts_full() -> Result<()>
{
    let server = responses::start_mock_server().await;
    let reply = |id: &str| {
        responses::sse(vec![
            responses::ev_assistant_message(&format!("{id}-message"), "Done"),
            responses::ev_completed(id),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            reply("first-turn"),
            reply("manual-summary"),
            responses::sse(vec![
                responses::ev_function_call(
                    "query-hidden",
                    "blackboard_query",
                    &json!({"text": "decisive project fact"}).to_string(),
                ),
                responses::ev_completed("query-response"),
            ]),
            reply("after-query"),
            reply("other-project"),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .with_provider_config("supports_websockets = false")
        .write(codex_home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let first = create_project(&mut app, "Continuation project", "continuation-a").await?;
    let other = create_project(&mut app, "Other project", "continuation-b").await?;
    seed_root_blackboard(codex_home.path(), &first).await?;
    seed_root_blackboard(codex_home.path(), &other).await?;
    promote_root_fact(
        codex_home.path(),
        &other,
        &format!("other-fact-{other}"),
        OTHER_PROJECT_FACT,
    )
    .await?;
    let thread_id = app
        .start_thread(ThreadStartParams {
            project_id: Some(first.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;

    run_turn(&mut app, &thread_id).await?;
    compact(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;
    let _: ThreadMetadataUpdateResponse = app
        .request(|request_id| ClientRequest::ThreadMetadataUpdate {
            request_id,
            params: ThreadMetadataUpdateParams {
                thread_id: thread_id.clone(),
                project_id: Some(other.clone()),
                daybreak_enabled: None,
                git_info: None,
            },
        })
        .await?;
    run_turn(&mut app, &thread_id).await?;

    let requests = mock.requests();
    let [_, _, continued, after_query, switched] = requests.as_slice() else {
        panic!("expected five model requests, got {}", requests.len());
    };

    // The continuation carrier does not repeat the promoted fact.
    let carriers = project_carriers(continued);
    assert_eq!(carriers.len(), 1, "{carriers:?}");
    assert!(carriers[0].contains("continues the same thread after context compaction"));
    assert!(!carriers[0].contains(SEEDED_FACT));
    let packet_revision = number_after(&carriers[0], "Project intelligence revision: ");

    // The query names it with the alias and revision completion resolves.
    let output: Value = serde_json::from_str(
        &after_query
            .function_call_output_text("query-hidden")
            .expect("query output"),
    )?;
    let hit = output["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["content"] == SEEDED_FACT)
        .expect("the seeded fact is found");
    assert_eq!(hit["rootAlias"], json!("E1"));
    assert_eq!(hit["rootRevision"].as_u64(), packet_revision);

    // The newly selected project gets its full packet, with its own knowledge.
    let switched_carriers = project_carriers(switched)
        .into_iter()
        .filter(|carrier| carrier.contains(&format!("Project ID: {other}")))
        .collect::<Vec<_>>();
    assert_eq!(switched_carriers.len(), 1, "{switched_carriers:?}");
    assert!(switched_carriers[0].contains(OTHER_PROJECT_FACT));
    assert!(!switched_carriers[0].contains("continues the same thread after context compaction"));
    Ok(())
}

/// A command that fails, then mid-turn compaction: the next request carries exactly one task
/// capsule naming the failure from the host's own receipt, and an unchanged step adds none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mid_turn_compaction_installs_one_capsule_from_host_receipts() -> Result<()> {
    let server = responses::start_mock_server().await;
    let reply = |id: &str| {
        responses::sse(vec![
            responses::ev_assistant_message(&format!("{id}-message"), "Done"),
            responses::ev_completed_with_tokens(id, /*total_tokens*/ 120),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "id": "intent",
                        "phase": "commentary",
                        "content": [{"type": "output_text", "text": "Next I will fix the failing build step."}]
                    }
                }),
                responses::ev_function_call(
                    "failing-command",
                    "exec_command",
                    &json!({"cmd": "exit 3", "yield_time_ms": 10_000}).to_string(),
                ),
                responses::ev_completed_with_tokens("over-limit", /*total_tokens*/ 330_000),
            ]),
            reply("summary"),
            reply("continued"),
            reply("unchanged"),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::Sqlite)
        .enable_feature(Feature::UnifiedExec)
        .with_sandbox_mode("danger-full-access")
        .with_root_config(
            "compact_prompt = \"Summarize.\"\nmodel_auto_compact_token_limit = 200000",
        )
        .with_provider_config("supports_websockets = false")
        .write(codex_home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project = create_project(&mut app, "Capsule project", "capsule-project").await?;
    seed_root_blackboard(codex_home.path(), &project).await?;
    let thread_id = app
        .start_thread(ThreadStartParams {
            project_id: Some(project),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut app, &thread_id).await?;
    run_turn(&mut app, &thread_id).await?;

    let requests = mock.requests();
    let [first, _, continued, unchanged] = requests.as_slice() else {
        panic!("expected four model requests, got {}", requests.len());
    };
    assert_eq!(capsules(first).len(), 0);
    let installed = capsules(continued);
    assert_eq!(installed.len(), 1, "{installed:?}");
    let capsule = &installed[0];
    assert!(
        capsule.contains("Latest command without exit code 0:"),
        "{capsule}"
    );
    assert!(capsule.contains("exit 3"), "{capsule}");
    assert!(
        capsule.contains("Last announced intention (the agent's own words"),
        "{capsule}"
    );
    assert!(
        capsule.contains("Next I will fix the failing build step."),
        "{capsule}"
    );
    // The capsule stays in history; an unchanged step does not repeat it.
    assert_eq!(capsules(unchanged), installed);
    Ok(())
}

/// `<stateful_task_capsule>` fragments in one request.
fn capsules(request: &ResponsesRequest) -> Vec<String> {
    request.body_json()["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["role"] == "developer")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .map(str::trim)
        .filter(|text| text.starts_with("<stateful_task_capsule>"))
        .map(str::to_string)
        .collect()
}

async fn create_project(app: &mut TestAppServer, name: &str, key: &str) -> Result<String> {
    let created: ProjectCreateResponse = app
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: name.to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: key.to_string(),
            },
        })
        .await?;
    Ok(created.project.id)
}

async fn compact(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let request = app
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: thread_id.to_string(),
        })
        .await?;
    let _: ThreadCompactStartResponse = app.read_response(request).await?;
    let _: TurnCompletedNotification = app.read_notification("turn/completed").await?;
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

/// Developer-role `<stateful_project>` fragments in one request.
fn project_carriers(request: &ResponsesRequest) -> Vec<String> {
    request.body_json()["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["role"] == "developer")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .map(str::trim)
        .filter(|text| text.starts_with("<stateful_project>"))
        .map(str::to_string)
        .collect()
}

fn number_after(text: &str, label: &str) -> Option<u64> {
    let (_, rest) = text.split_once(label)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
