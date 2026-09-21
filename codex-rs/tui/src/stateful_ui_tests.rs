use super::*;
use crate::app_server_session::ThreadParamsMode;
use crate::dynamic_tools_mcp::ThreadToolTransport;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::local_settings::LocalSettings;
use codex_app_server_protocol::ProjectReadParams;
use codex_app_server_protocol::ProjectReadResponse;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[test]
fn cli_selection_requires_a_goal_and_preserves_the_selected_mode() {
    assert_eq!(StatefulStartup::from_cli(None, None, None).unwrap(), None);
    assert!(StatefulStartup::from_cli(Some(StatefulModeCliArg::Autonomous), None, None).is_err());
    assert_eq!(
        StatefulStartup::from_cli(
            Some(StatefulModeCliArg::Collaborative),
            Some("project-1".to_string()),
            Some("  investigate the evidence  "),
        )
        .unwrap(),
        Some(StatefulStartup {
            mode: StatefulWorkflowMode::Collaborative,
            project_id: Some("project-1".to_string()),
            goal: "investigate the evidence".to_string(),
        })
    );
}

#[test]
fn project_name_uses_the_selected_directory() {
    let path = if cfg!(windows) {
        Path::new(r"C:\work\alpha")
    } else {
        Path::new("/work/alpha")
    };
    assert_eq!(project_name(path), "alpha");
}

#[test]
fn semantic_obligation_cell_renders_a_compact_structured_update() {
    let cell = StatefulSemanticHistoryCell::for_obligation(StatefulObligation {
        id: "obligation-1".to_string(),
        project_id: "project-1".to_string(),
        run_id: "run-1".to_string(),
        packet: codex_app_server_protocol::StatefulObligationPacket {
            examined: vec!["The project index and all current source fingerprints.".to_string()],
            learning: vec![
                "Eight files are present: two HTML files, five JavaScript modules, and one stylesheet."
                    .to_string(),
            ],
            implication: vec![
                "Future work can detect corpus drift without rescanning unchanged sources."
                    .to_string(),
            ],
            next: vec!["Verify changed fingerprints before consequential claims.".to_string()],
            uncertainty: vec!["No content-level claim was tested in this pass.".to_string()],
            ..Default::default()
        },
        provenance_source_id: "source-1".to_string(),
        sequence: 3,
        revision: 1,
        created_at: 1,
    });
    let rendered = cell
        .display_lines(/*width*/ 56)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r"
    Stateful update · 3
    Examined
      • The project index and all current source
        fingerprints.
    Learned
      • Eight files are present: two HTML files, five
        JavaScript modules, and one stylesheet.
    Implications
      • Future work can detect corpus drift without
        rescanning unchanged sources.
    Next
      • Verify changed fingerprints before consequential
        claims.
    Uncertainty
      • No content-level claim was tested in this pass.
    ");
}

#[test]
fn completed_run_cell_renders_the_durable_result() {
    let cell = StatefulSemanticHistoryCell::for_run(StatefulRun {
        id: "run-1".to_string(),
        project_id: "project-1".to_string(),
        thread_ids: vec!["thread-1".to_string()],
        goal: "Verify the public file count".to_string(),
        mode: StatefulWorkflowMode::Collaborative,
        status: StatefulRunStatus::Completed,
        budget: StatefulRunBudget {
            max_continuations: DEFAULT_MAX_CONTINUATIONS,
            max_elapsed_seconds: DEFAULT_MAX_ELAPSED_SECONDS,
        },
        continuations_used: 0,
        strategy: Some("Reuse accumulated evidence and verify only changed sources.".to_string()),
        result: Some("Verified eight files without changing source files.".to_string()),
        strategy_revision: 1,
        revision: 2,
        created_at: 1,
        updated_at: 2,
    });
    let rendered = cell
        .display_lines(/*width*/ 56)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r"
    Stateful run · completed
    Goal
      • Verify the public file count
    Result
      • Verified eight files without changing source files.
    Final strategy
      • Reuse accumulated evidence and verify only changed
        sources.
    ");
}

#[tokio::test]
async fn native_startup_attaches_the_selected_project_and_starts_the_run() -> Result<()> {
    let codex_home = tempfile::tempdir()?;
    let project = tempfile::tempdir()?;
    let mut config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(project.path().to_path_buf()),
            ..ConfigOverrides::default()
        })
        .build()
        .await?;
    config.sqlite =
        SqliteConfig::new_for_testing(AbsolutePathBuf::from_absolute_path(codex_home.path())?);
    let local_settings = LocalSettings::from(&config);
    let app_server = crate::start_embedded_app_server_for_picker(&config).await?;

    let started = crate::app_server_session::start_thread_with_request_handle(
        app_server.request_handle(),
        &local_settings,
        config.clone(),
        ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        ThreadToolTransport::Disabled,
        StatefulStartup::from_cli(
            Some(StatefulModeCliArg::Collaborative),
            None,
            Some("Investigate the project"),
        )?,
    )
    .await?;
    let thread_id = started.session.thread_id.to_string();
    let handle = app_server.request_handle();
    let run: StatefulRunReadResponse = handle
        .request_typed(ClientRequest::StatefulRunRead {
            request_id: RequestId::String("read-stateful-run".to_string()),
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread_id),
            },
        })
        .await?;
    let run = run.run.context("Stateful run should exist")?;
    let project_response: ProjectReadResponse = handle
        .request_typed(ClientRequest::ProjectRead {
            request_id: RequestId::String("read-stateful-project".to_string()),
            params: ProjectReadParams {
                project_id: run.project_id.clone(),
            },
        })
        .await?;

    assert_eq!(project_response.project.id, run.project_id);
    assert_eq!(
        project_response.project.roots,
        vec![ProjectRoot {
            path: AbsolutePathBuf::from_absolute_path(project.path())?,
        }]
    );
    assert_eq!(run.goal, "Investigate the project");
    assert_eq!(run.mode, StatefulWorkflowMode::Collaborative);
    assert_eq!(
        run.budget,
        StatefulRunBudget {
            max_continuations: DEFAULT_MAX_CONTINUATIONS,
            max_elapsed_seconds: DEFAULT_MAX_ELAPSED_SECONDS,
        }
    );

    let second = crate::app_server_session::start_thread_with_request_handle(
        app_server.request_handle(),
        &local_settings,
        config,
        ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        ThreadToolTransport::Disabled,
        StatefulStartup::from_cli(
            Some(StatefulModeCliArg::Socratic),
            None,
            Some("Challenge the current strategy"),
        )?,
    )
    .await?;
    let second_run: StatefulRunReadResponse = handle
        .request_typed(ClientRequest::StatefulRunRead {
            request_id: RequestId::String("read-second-stateful-run".to_string()),
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(second.session.thread_id.to_string()),
            },
        })
        .await?;
    let second_run = second_run.run.context("second Stateful run should exist")?;
    assert_eq!(second_run.project_id, run.project_id);
    assert_ne!(second_run.id, run.id);
    assert_eq!(second_run.mode, StatefulWorkflowMode::Socratic);
    app_server.shutdown().await?;
    Ok(())
}
