//! Public safety guards for retained attributed, nonbinding notes.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

#[path = "stateful_authority_repair_tests.rs"]
mod repair_tests;

fn tool_call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(&format!("{call_id}-response")),
    ])
}

fn assistant(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("assistant-message", text),
        responses::ev_completed("assistant-response"),
    ])
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

/// A public correction clears the old speaker facet. Model tools still cannot revise,
/// promote, or supersede that corrected entry into a binding record.
#[tokio::test]
async fn corrected_attributed_notes_refuse_model_rewrite_promotion_and_succession() -> Result<()> {
    use codex_app_server_protocol::StatefulMemoryCorrectParams;
    use codex_app_server_protocol::StatefulMemoryCorrectResponse;
    use codex_app_server_protocol::StatefulMemoryReadParams;
    use codex_app_server_protocol::StatefulMemoryReadResponse;
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Attributed".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "attributed-model-guard".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    responses::mount_sse_once(
        &responses_server,
        assistant("Recorded as nonbinding context."),
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "My colleague wrote: \"Never push on Fridays.\"",
    )
    .await?;
    let read = || {
        let project = project.clone();
        let thread = thread.clone();
        move |request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                expected_project_id: project.project.id.clone(),
                thread_id: thread,
                cursor: None,
                limit: None,
                background_section: true,
            },
        }
    };
    let original: StatefulMemoryReadResponse = server.request(read()).await?;
    let note = original
        .data
        .into_iter()
        .find(|item| item.attributed_to.is_some())
        .expect("attributed note");
    let corrected: StatefulMemoryCorrectResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryCorrect {
            request_id,
            params: StatefulMemoryCorrectParams {
                expected_project_id: project.project.id.clone(),
                thread_id: thread.clone(),
                entry_id: note.entry_id,
                expected_revision: note.revision,
                content: "Corrected report: avoid Friday pushes during maintenance.".to_string(),
                background_section: true,
            },
        })
        .await?;
    assert_eq!(corrected.item.attributed_to, None);
    let id = corrected.item.entry_id.clone();
    let revision = corrected.item.revision;
    let log = responses::mount_sse_sequence(&responses_server, vec![
        tool_call("attributed-update", "blackboard_update_batch", json!({"mutations": [
            {"action":"revise", "entryId":id, "expectedRevision":revision, "content":"Model rewritten words."}
        ]})),
        tool_call("attributed-promote", "blackboard_update_batch", json!({"mutations": [
            {"action":"setRootPromotion", "entryId":id, "expectedRevision":revision, "rootPromotion":"promoted"}
        ]})),
        tool_call("attributed-successor", "blackboard_record_batch", json!({"records":[{
            "idempotencyKey":"attributed-successor", "kind":"fact", "content":"Model promoted successor.", "confidenceBasisPoints":9000,
            "verification":"unverified", "importance":"high", "rootPromotion":"promoted", "supersedes":[{"entryId":id, "revision":revision}]
        }]})),
        assistant("The attributed note remains unchanged."),
    ]).await;
    run_turn(&mut server, &thread, "Review the recorded attributed note.").await?;
    let requests = log.requests();
    let update: Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("attributed-update")
            .expect("update output"),
    )?;
    let promotion: Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("attributed-promote")
            .expect("promotion output"),
    )?;
    let successor: Value = serde_json::from_str(
        &requests[3]
            .function_call_output_text("attributed-successor")
            .expect("successor output"),
    )?;
    let after: StatefulMemoryReadResponse = server.request(read()).await?;
    assert_eq!(
        (
            update["results"][0]["updated"].clone(),
            promotion["results"][0]["updated"].clone(),
            successor["results"][0]["recorded"].clone(),
            after.data
        ),
        (
            json!(false),
            json!(false),
            json!(false),
            vec![corrected.item]
        )
    );
    assert!(
        update
            .to_string()
            .contains("never promoted or revised by the model")
    );
    assert!(
        promotion
            .to_string()
            .contains("never promoted or revised by the model")
    );
    assert!(successor.to_string().contains("attributed note"));
    Ok(())
}
