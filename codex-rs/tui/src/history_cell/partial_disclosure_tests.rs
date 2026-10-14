//! Actual Partial finals disclose through the public warning transport.

use super::*;

/// S1: an actual Partial commit reaches the warning cell through the public transport.
#[tokio::test]
async fn partial_disclosure_cell_snapshot() -> anyhow::Result<()> {
    use app_test_support::MockResponsesConfig;
    use app_test_support::TestAppServer;
    use codex_app_server_protocol::ClientRequest;
    use codex_app_server_protocol::JSONRPCMessage;
    use codex_app_server_protocol::ProjectCreateParams;
    use codex_app_server_protocol::ProjectCreateResponse;
    use codex_app_server_protocol::ProjectRoot;
    use codex_app_server_protocol::StatefulRunBudget;
    use codex_app_server_protocol::StatefulRunStartParams;
    use codex_app_server_protocol::StatefulRunStartResponse;
    use codex_app_server_protocol::StatefulWorkflowMode;
    use codex_app_server_protocol::ThreadStartParams;
    use codex_app_server_protocol::TurnStartParams;
    use codex_app_server_protocol::TurnStartResponse;
    use codex_app_server_protocol::UserInput;
    use codex_app_server_protocol::WarningNotification;
    use core_test_support::responses;

    let model = responses::start_mock_server().await;
    let home = tempfile::tempdir()?;
    let root = tempfile::tempdir()?;
    MockResponsesConfig::new(&model.uri())
        .enable_feature(codex_features::Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Partial disclosure".into(),
                roots: vec![ProjectRoot {
                    path: codex_utils_absolute_path::AbsolutePathBuf::try_from(
                        root.path().to_path_buf(),
                    )
                    .expect("absolute root"),
                }],
                metadata: None,
                idempotency_key: "disclosure-project".into(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            cwd: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await?;
    let _: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: "What is 2 + 2?".into(),
                mode: StatefulWorkflowMode::Autonomous,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "disclosure-run".into(),
            },
        })
        .await?;
    let reply = responses::mount_sse_once(&model, responses::sse(vec![
        responses::ev_response_created("partial"),
        responses::ev_assistant_message("answer", "4\n[stateful-outcome]\ndisposition: answer\nopen-issues:\n- tests remain\n[/stateful-outcome]"),
        responses::ev_completed("partial"),
    ])).await;
    let turn = server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "What is 2 + 2?".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = server.read_response(turn).await?;
    let lines = tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 30), async {
        let mut lines = Vec::new();
        loop {
            if let JSONRPCMessage::Notification(notification) = server.read_next_message().await? {
                if notification.method == "turn/completed" {
                    break;
                }
                if notification.method == "warning" {
                    let warning: WarningNotification =
                        serde_json::from_value(notification.params.expect("warning params"))?;
                    if warning
                        .message
                        .starts_with("Stateful: this run ended partial")
                    {
                        lines.extend(
                            new_warning_event(warning.message)
                                .transcript_lines(/*width*/ 80)
                                .iter()
                                .map(ToString::to_string),
                        );
                    }
                }
            }
        }
        Ok::<_, anyhow::Error>(lines)
    })
    .await??;
    insta::assert_snapshot!(lines.join("\n"));
    reply.single_request();
    Ok(())
}
