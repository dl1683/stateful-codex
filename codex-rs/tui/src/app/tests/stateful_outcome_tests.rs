//! Stateful run outcomes through the live app-server event path: an embedded app server, a
//! mocked model, and the App's own notification routing.

use super::*;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;

const QUESTION: &str = "How many days are in a leap year?";
const ANSWER: &str = "A leap year has 366 days.\n\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

#[tokio::test]
async fn an_answered_run_shows_its_calm_label_live() -> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    let home = tempdir()?;
    let project = tempdir()?;
    app.config.codex_home = home.path().abs();
    app.config.cwd = project.path().abs();
    app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
    let model = core_test_support::responses::start_mock_server().await;
    let _model_log = core_test_support::responses::mount_sse_sequence(
        &model,
        vec![core_test_support::responses::sse(vec![
            core_test_support::responses::ev_response_created("answer"),
            core_test_support::responses::ev_assistant_message("answer-message", ANSWER),
            core_test_support::responses::ev_completed("answer"),
        ])],
    )
    .await;
    app_test_support::MockResponsesConfig::new(&model.uri())
        .enable_feature(codex_features::Feature::Sqlite)
        .write(home.path())?;
    app.config.model_provider_id = "mock_provider".to_string();
    app.config.model_provider = ModelProviderInfo {
        name: "Mock".to_string(),
        base_url: Some(format!("{}/v1", model.uri())),
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        ..Default::default()
    };
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let started = crate::app_server_session::start_thread_with_request_handle(
        server.request_handle(),
        &crate::local_settings::LocalSettings::from(&app.config),
        app.config.clone(),
        crate::app_server_session::ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        crate::dynamic_tools_mcp::ThreadToolTransport::Disabled,
        /*model_provider_override*/ None,
        crate::stateful_ui::StatefulStartup::from_cli(
            Some(crate::cli::StatefulModeCliArg::Autonomous),
            /*project_id*/ None,
            Some(QUESTION),
        )?,
    )
    .await?;
    let thread = started.session.thread_id;
    Box::pin(app.enqueue_primary_thread_session(started.session, started.turns)).await?;
    let _: codex_app_server_protocol::TurnStartResponse = server
        .request_handle()
        .request_typed(codex_app_server_protocol::ClientRequest::TurnStart {
            request_id: AppServerRequestId::String("answer-turn".to_string()),
            params: codex_app_server_protocol::TurnStartParams {
                thread_id: thread.to_string(),
                input: vec![codex_app_server_protocol::UserInput::Text {
                    text: QUESTION.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;

    // Route every server event through the App until the turn completes, then drain briefly:
    // the run's update may arrive on either side of the turn's completion.
    let mut drain_until = None;
    loop {
        let deadline =
            drain_until.unwrap_or(tokio::time::Instant::now() + Duration::from_secs(/*secs*/ 20));
        let Ok(Some(event)) = tokio::time::timeout_at(deadline, server.next_event()).await else {
            break;
        };
        let completed = matches!(
            &event,
            codex_app_server_client::AppServerEvent::ServerNotification(notification)
                if matches!(**notification, ServerNotification::TurnCompleted(_))
        );
        app.handle_app_server_event(&server, event).await;
        if completed && drain_until.is_none() {
            drain_until = Some(tokio::time::Instant::now() + Duration::from_secs(/*secs*/ 2));
        }
    }
    let mut stateful_cells = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let rendered = cell
                .display_lines(/*width*/ 60)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            if rendered.starts_with("Stateful run") {
                stateful_cells.push(rendered);
            }
        }
    }
    assert_eq!(stateful_cells.len(), 1, "{stateful_cells:#?}");
    insta::assert_snapshot!(stateful_cells[0], @r"
    Stateful run · answered · not verified
    Goal
      • How many days are in a leap year?
    ");
    Ok(())
}
