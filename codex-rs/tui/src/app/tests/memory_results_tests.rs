use super::*;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;

async fn result(events: &mut mpsc::UnboundedReceiver<AppEvent>) -> AppEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(/*secs*/ 10), events.recv())
            .await
            .expect("memory result timeout")
            .expect("event");
        if matches!(event, AppEvent::StatefulMemoryResult { .. }) {
            return event;
        }
    }
}

async fn turn(server: &mut AppServerSession, thread: ThreadId, text: &str) -> Result<()> {
    let _: codex_app_server_protocol::TurnStartResponse = server
        .request_handle()
        .request_typed(codex_app_server_protocol::ClientRequest::TurnStart {
            request_id: AppServerRequestId::String(uuid::Uuid::new_v4().to_string()),
            params: codex_app_server_protocol::TurnStartParams {
                thread_id: thread.to_string(),
                input: vec![codex_app_server_protocol::UserInput::Text {
                    text: text.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    loop {
        if let Some(codex_app_server_client::AppServerEvent::ServerNotification(notification)) =
            tokio::time::timeout(Duration::from_secs(/*secs*/ 10), server.next_event()).await?
            && matches!(*notification, ServerNotification::TurnCompleted(_))
        {
            return Ok(());
        }
    }
}

#[tokio::test]
async fn memory_application_fences_generations_binding_and_client_and_refuses_bad_scopes()
-> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    let home = tempdir()?;
    app.config.codex_home = home.path().abs();
    app.config.sqlite = codex_state::SqliteConfig::new_for_testing(home.path().abs());
    let model = core_test_support::responses::start_mock_server().await;
    let model_log = core_test_support::responses::mount_sse_sequence(
        &model,
        vec![
            core_test_support::responses::sse(vec![core_test_support::responses::ev_completed(
                "preparation",
            )]),
            core_test_support::responses::sse(vec![core_test_support::responses::ev_completed(
                "opening",
            )]),
        ],
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
    let started = server.start_thread(&app.config).await?;
    let thread = started.session.thread_id;
    Box::pin(app.enqueue_primary_thread_session(started.session, started.turns)).await?;
    turn(
        &mut server,
        thread,
        "Persist this conversation for control testing.",
    )
    .await?;
    let handle = server.request_handle();
    let project: ProjectCreateResponse = handle
        .request_typed(codex_app_server_protocol::ClientRequest::ProjectCreate {
            request_id: AppServerRequestId::String("memory-project".to_string()),
            params: ProjectCreateParams {
                name: "Memory application".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "memory-application".to_string(),
            },
        })
        .await?;
    let binding =
        |project_id: String| codex_app_server_protocol::ClientRequest::ThreadMetadataUpdate {
            request_id: AppServerRequestId::String(uuid::Uuid::new_v4().to_string()),
            params: codex_app_server_protocol::ThreadMetadataUpdateParams {
                thread_id: thread.to_string(),
                project_id: Some(project_id),
                git_info: None,
                daybreak_enabled: None,
            },
        };
    let _: codex_app_server_protocol::ThreadMetadataUpdateResponse = handle
        .request_typed(binding(project.project.id.clone()))
        .await?;
    turn(
        &mut server,
        thread,
        "Some ground rules for this whole investigation:\n- Never commit.",
    )
    .await?;
    let initial: StatefulMemoryReadResponse = handle
        .request_typed(
            codex_app_server_protocol::ClientRequest::StatefulMemoryRead {
                request_id: AppServerRequestId::String("initial".to_string()),
                params: StatefulMemoryReadParams {
                    thread_id: thread.to_string(),
                    expected_project_id: project.project.id.clone(),
                    cursor: None,
                    limit: None,
                    background_section: true,
                },
            },
        )
        .await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let submit = |args: &str| AppEvent::StatefulMemory {
        thread_id: Some(thread),
        args: args.to_string(),
    };
    app.handle_event(&mut tui, &mut server, submit("")).await?;
    let first = result(&mut events).await;
    app.handle_event(&mut tui, &mut server, submit("")).await?;
    let second = result(&mut events).await;
    app.handle_event(&mut tui, &mut server, second).await?;
    let cells = app.transcript_cells.len();
    app.handle_event(&mut tui, &mut server, first).await?;
    assert_eq!(app.transcript_cells.len(), cells);
    // Both the cache and on-screen generation still belong to the second listing.
    app.handle_event(&mut tui, &mut server, submit("next"))
        .await?;
    let next = result(&mut events).await;
    if let AppEvent::StatefulMemoryResult { cell, .. } = &next {
        assert!(
            cell.display_lines(/*width*/ 100)
                .iter()
                .any(|line| line.to_string().contains("whole list"))
        );
    }
    app.handle_event(&mut tui, &mut server, next).await?;
    // Invalid scope-looking commands traverse dispatch but never write globally.
    for args in [
        "add rule for this investigation",
        "add rule for :Do not push",
        "add rule for this investigation:",
        "add rule for:Never push",
        "add rule for:this this investigation:Never push",
    ] {
        app.handle_event(&mut tui, &mut server, submit(args))
            .await?;
        let refusal = result(&mut events).await;
        if let AppEvent::StatefulMemoryResult { cell, .. } = &refusal {
            assert!(
                cell.display_lines(/*width*/ 200)
                    .iter()
                    .any(|line| line.to_string().contains("Usage:"))
            );
        }
        app.handle_event(&mut tui, &mut server, refusal).await?;
    }
    let empty: StatefulMemoryReadResponse = handle
        .request_typed(
            codex_app_server_protocol::ClientRequest::StatefulMemoryRead {
                request_id: AppServerRequestId::String("after-refusal".to_string()),
                params: StatefulMemoryReadParams {
                    thread_id: thread.to_string(),
                    expected_project_id: project.project.id.clone(),
                    cursor: None,
                    limit: None,
                    background_section: true,
                },
            },
        )
        .await?;
    assert_eq!(empty, initial);
    for args in [
        "add rule for this investigation:Never push",
        "add rule for   this investigation : Never push",
    ] {
        app.handle_event(&mut tui, &mut server, submit(args))
            .await?;
        let scoped = result(&mut events).await;
        if let AppEvent::StatefulMemoryResult { cell, .. } = &scoped {
            assert!(
                cell.display_lines(/*width*/ 200)
                    .iter()
                    .any(|line| line.to_string().contains("retained rule"))
            );
        }
        app.handle_event(&mut tui, &mut server, scoped).await?;
    }
    let page: StatefulMemoryReadResponse = handle
        .request_typed(
            codex_app_server_protocol::ClientRequest::StatefulMemoryRead {
                request_id: AppServerRequestId::String("after-scoped-add".to_string()),
                params: StatefulMemoryReadParams {
                    thread_id: thread.to_string(),
                    expected_project_id: project.project.id.clone(),
                    cursor: None,
                    limit: None,
                    background_section: true,
                },
            },
        )
        .await?;
    assert_eq!(page.data.len(), 2);
    assert!(page.data[0].scope_title.is_some());
    // Render an actual public retry refusal, including the client's RPC error envelope.
    let addition = codex_app_server_protocol::StatefulMemoryAddParams {
        thread_id: thread.to_string(),
        expected_project_id: project.project.id.clone(),
        kind: codex_app_server_protocol::StatefulMemoryAddKind::Note,
        content: "Temporary note.".to_string(),
        scope: None,
        reason: None,
        client_action_id: "retired-rendering".to_string(),
        background_section: true,
    };
    let request = || codex_app_server_protocol::ClientRequest::StatefulMemoryAdd {
        request_id: AppServerRequestId::String(uuid::Uuid::new_v4().to_string()),
        params: addition.clone(),
    };
    let added: codex_app_server_protocol::StatefulMemoryAddResponse =
        handle.request_typed(request()).await?;
    let _: codex_app_server_protocol::StatefulMemoryForgetResponse = handle
        .request_typed(
            codex_app_server_protocol::ClientRequest::StatefulMemoryForget {
                request_id: AppServerRequestId::String("retire-rendering".to_string()),
                params: codex_app_server_protocol::StatefulMemoryForgetParams {
                    thread_id: thread.to_string(),
                    expected_project_id: project.project.id.clone(),
                    entry_id: added.item.entry_id,
                    expected_revision: added.item.revision,
                },
            },
        )
        .await?;
    let replay = handle
        .request_typed::<codex_app_server_protocol::StatefulMemoryAddResponse>(request())
        .await
        .expect_err("retired action cannot return a current item");
    let cell = crate::history_cell::new_error_event(format!("Nothing was added: {replay}"));
    insta::assert_snapshot!(
        "memory_retired_action",
        cell.display_lines(/*width*/ 200)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    // A queued old-project listing, then an error receipt, cannot become current after unlink.
    app.handle_event(&mut tui, &mut server, submit("")).await?;
    let old_project = result(&mut events).await;
    let _: codex_app_server_protocol::ThreadMetadataUpdateResponse =
        handle.request_typed(binding(String::new())).await?;
    let cells = app.transcript_cells.len();
    app.handle_event(&mut tui, &mut server, old_project).await?;
    assert_eq!(app.transcript_cells.len(), cells);
    let _: codex_app_server_protocol::ThreadMetadataUpdateResponse = handle
        .request_typed(binding(project.project.id.clone()))
        .await?;
    app.handle_event(&mut tui, &mut server, submit("forget missing@1"))
        .await?;
    let old_client = result(&mut events).await;
    server.client_id = uuid::Uuid::new_v4();
    app.handle_event(&mut tui, &mut server, old_client).await?;
    assert_eq!(app.transcript_cells.len(), cells);
    assert_eq!(model_log.requests().len(), 2);
    Ok(())
}
