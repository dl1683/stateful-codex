//! Native TUI entrypoint for an explicitly selected Stateful Codex workflow.
//!
//! The app-server remains authoritative for project identity and run state. This module only maps
//! the CLI's explicit directory, goal, and mode choices onto that public API before inference.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::cli::StatefulModeCliArg;
use crate::history_cell::HistoryCell;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ObligationUpdatedNotification;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectListParams;
use codex_app_server_protocol::ProjectListResponse;
use codex_app_server_protocol::ProjectReadParams;
use codex_app_server_protocol::ProjectReadResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::StatefulObligation;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::eyre::Context;
use color_eyre::eyre::ContextCompat;
use color_eyre::eyre::Result;
use ratatui::style::Stylize;
use ratatui::text::Line;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;

const DEFAULT_MAX_CONTINUATIONS: u32 = 24;
const DEFAULT_MAX_ELAPSED_SECONDS: u32 = 14_400;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatefulStartup {
    mode: StatefulWorkflowMode,
    project_id: Option<String>,
    goal: String,
}

impl StatefulStartup {
    pub(crate) fn from_cli(
        mode: Option<StatefulModeCliArg>,
        project_id: Option<String>,
        prompt: Option<&str>,
    ) -> Result<Option<Self>> {
        let Some(mode) = mode else {
            return Ok(None);
        };
        let goal = prompt
            .map(str::trim)
            .filter(|goal| !goal.is_empty())
            .context("--stateful requires a non-empty goal prompt")?;
        Ok(Some(Self {
            mode: mode.into(),
            project_id,
            goal: goal.to_string(),
        }))
    }
}

#[derive(Debug)]
pub(crate) struct PreparedStatefulStartup {
    project_id: String,
    mode: StatefulWorkflowMode,
    goal: String,
}

pub(crate) async fn prepare_startup(
    request_handle: &AppServerRequestHandle,
    thread_params: &mut ThreadStartParams,
    startup: StatefulStartup,
) -> Result<PreparedStatefulStartup> {
    let project_root = project_root(thread_params)?;
    let project_id = resolve_project(request_handle, &project_root, startup.project_id).await?;
    thread_params.project_id = Some(project_id.clone());
    Ok(PreparedStatefulStartup {
        project_id,
        mode: startup.mode,
        goal: startup.goal,
    })
}

async fn resolve_project(
    request_handle: &AppServerRequestHandle,
    root: &AbsolutePathBuf,
    selected_project_id: Option<String>,
) -> Result<String> {
    if let Some(project_id) = selected_project_id {
        let response: ProjectReadResponse = request_handle
            .request_typed(ClientRequest::ProjectRead {
                request_id: RequestId::String("stateful-project-read".to_string()),
                params: ProjectReadParams {
                    project_id: project_id.clone(),
                },
            })
            .await
            .context("failed to read the selected Stateful project")?;
        if !response.project.roots.iter().any(|item| item.path == *root) {
            color_eyre::eyre::bail!(
                "Stateful project {project_id} does not include the selected directory {}",
                root.as_path().display()
            );
        }
        return Ok(project_id);
    }

    let matching_projects = projects_for_root(request_handle, root).await?;
    match matching_projects.as_slice() {
        [] => create_project(request_handle, root).await,
        [project_id] => Ok(project_id.clone()),
        project_ids => color_eyre::eyre::bail!(
            "multiple Stateful projects use {}: {}. Select one with --stateful-project",
            root.as_path().display(),
            project_ids.join(", ")
        ),
    }
}

async fn projects_for_root(
    request_handle: &AppServerRequestHandle,
    root: &AbsolutePathBuf,
) -> Result<Vec<String>> {
    let mut cursor = None;
    let mut project_ids = Vec::new();
    let mut page = 0_u32;
    loop {
        let response: ProjectListResponse = request_handle
            .request_typed(ClientRequest::ProjectList {
                request_id: RequestId::String(format!("stateful-project-list-{page}")),
                params: ProjectListParams {
                    cursor,
                    limit: Some(100),
                    sort_key: None,
                    sort_direction: None,
                },
            })
            .await
            .context("failed to list Stateful projects")?;
        project_ids.extend(
            response
                .data
                .into_iter()
                .filter(|project| project.roots.iter().any(|item| item.path == *root))
                .map(|project| project.id),
        );
        let Some(next_cursor) = response.next_cursor else {
            return Ok(project_ids);
        };
        cursor = Some(next_cursor);
        page = page.saturating_add(1);
    }
}

async fn create_project(
    request_handle: &AppServerRequestHandle,
    root: &AbsolutePathBuf,
) -> Result<String> {
    let root_text = root.as_path().to_string_lossy();
    let digest = Sha256::digest(root_text.as_bytes());
    let response: ProjectCreateResponse = request_handle
        .request_typed(ClientRequest::ProjectCreate {
            request_id: RequestId::String(format!("stateful-project-{digest:x}")),
            params: ProjectCreateParams {
                name: project_name(root.as_path()),
                roots: vec![ProjectRoot { path: root.clone() }],
                metadata: None,
                idempotency_key: format!("stateful-tui-project-v1-{digest:x}"),
            },
        })
        .await
        .context("failed to create the Stateful project")?;
    Ok(response.project.id)
}

pub(crate) async fn start_run(
    request_handle: &AppServerRequestHandle,
    startup: &PreparedStatefulStartup,
    thread_id: ThreadId,
) -> Result<()> {
    let thread_id = thread_id.to_string();
    let _: StatefulRunStartResponse = request_handle
        .request_typed(ClientRequest::StatefulRunStart {
            request_id: RequestId::String(format!("stateful-run-{thread_id}")),
            params: StatefulRunStartParams {
                project_id: startup.project_id.clone(),
                thread_id: thread_id.clone(),
                goal: startup.goal.clone(),
                mode: startup.mode,
                budget: StatefulRunBudget {
                    max_continuations: DEFAULT_MAX_CONTINUATIONS,
                    max_elapsed_seconds: DEFAULT_MAX_ELAPSED_SECONDS,
                },
                idempotency_key: format!("stateful-tui-run-v1-{thread_id}"),
            },
        })
        .await
        .context("failed to start the Stateful run")?;
    Ok(())
}

fn project_root(thread_params: &ThreadStartParams) -> Result<AbsolutePathBuf> {
    let root = thread_params
        .cwd
        .as_deref()
        .context("--stateful requires an explicit project directory")?;
    AbsolutePathBuf::from_absolute_path(root)
        .context("--stateful requires an absolute project directory")
}

fn project_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| root.display().to_string())
}

impl From<StatefulModeCliArg> for StatefulWorkflowMode {
    fn from(value: StatefulModeCliArg) -> Self {
        match value {
            StatefulModeCliArg::Autonomous => Self::Autonomous,
            StatefulModeCliArg::Collaborative => Self::Collaborative,
            StatefulModeCliArg::Socratic => Self::Socratic,
        }
    }
}

pub(crate) async fn handle_app_scoped_notification(
    request_handle: AppServerRequestHandle,
    primary_thread_id: Option<ThreadId>,
    app_event_tx: AppEventSender,
    notification: ServerNotification,
) -> bool {
    match notification {
        ServerNotification::ObligationUpdated(notification) => {
            if let Err(error) = publish_obligation_update(
                &request_handle,
                primary_thread_id,
                &app_event_tx,
                notification,
            )
            .await
            {
                tracing::warn!(%error, "failed to render Stateful obligation update");
            }
            true
        }
        ServerNotification::StatefulRunUpdated(notification) => {
            let result = read_run(&request_handle, &notification.run_id).await;
            match result {
                Ok(Some(run))
                    if run_is_for_thread(&run, primary_thread_id)
                        && is_terminal_status(run.status) =>
                {
                    app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                        StatefulSemanticHistoryCell::for_run(run),
                    )));
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "failed to render Stateful run update");
                }
            }
            true
        }
        ServerNotification::SteeringUpdated(_)
        | ServerNotification::BlackboardUpdated(_)
        | ServerNotification::ProjectChanged(_) => true,
        _ => false,
    }
}

async fn publish_obligation_update(
    request_handle: &AppServerRequestHandle,
    primary_thread_id: Option<ThreadId>,
    app_event_tx: &AppEventSender,
    notification: ObligationUpdatedNotification,
) -> Result<()> {
    let Some(run) = read_run(request_handle, &notification.run_id).await? else {
        return Ok(());
    };
    if !run_is_for_thread(&run, primary_thread_id) {
        return Ok(());
    }
    let Some(obligation) = read_obligation(
        request_handle,
        &notification.run_id,
        &notification.obligation_id,
    )
    .await?
    else {
        return Ok(());
    };
    app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
        StatefulSemanticHistoryCell::for_obligation(obligation),
    )));
    Ok(())
}

async fn read_run(
    request_handle: &AppServerRequestHandle,
    run_id: &str,
) -> Result<Option<StatefulRun>> {
    let response: StatefulRunReadResponse = request_handle
        .request_typed(ClientRequest::StatefulRunRead {
            request_id: RequestId::String(format!("stateful-tui-run-read-{run_id}")),
            params: StatefulRunReadParams {
                run_id: Some(run_id.to_string()),
                thread_id: None,
            },
        })
        .await
        .context("failed to read the Stateful run")?;
    Ok(response.run)
}

async fn read_obligation(
    request_handle: &AppServerRequestHandle,
    run_id: &str,
    obligation_id: &str,
) -> Result<Option<StatefulObligation>> {
    let mut cursor = None;
    let mut page = 0_u32;
    loop {
        let response: ObligationListResponse = request_handle
            .request_typed(ClientRequest::ObligationList {
                request_id: RequestId::String(format!(
                    "stateful-tui-obligation-list-{run_id}-{page}"
                )),
                params: ObligationListParams {
                    run_id: run_id.to_string(),
                    cursor,
                    limit: Some(50),
                },
            })
            .await
            .context("failed to read Stateful obligations")?;
        if let Some(obligation) = response
            .data
            .into_iter()
            .find(|obligation| obligation.id == obligation_id)
        {
            return Ok(Some(obligation));
        }
        let Some(next_cursor) = response.next_cursor else {
            return Ok(None);
        };
        cursor = Some(next_cursor);
        page = page.saturating_add(1);
    }
}

fn run_is_for_thread(run: &StatefulRun, primary_thread_id: Option<ThreadId>) -> bool {
    primary_thread_id.is_some_and(|thread_id| {
        let thread_id = thread_id.to_string();
        run.thread_ids
            .iter()
            .any(|candidate| candidate == &thread_id)
    })
}

fn is_terminal_status(status: StatefulRunStatus) -> bool {
    matches!(
        status,
        StatefulRunStatus::Completed
            | StatefulRunStatus::Cancelled
            | StatefulRunStatus::Blocked
            | StatefulRunStatus::Failed
    )
}

#[derive(Debug)]
struct StatefulSemanticHistoryCell {
    title: String,
    sections: Vec<(&'static str, Vec<String>)>,
}

impl StatefulSemanticHistoryCell {
    fn for_obligation(obligation: StatefulObligation) -> Self {
        let packet = obligation.packet;
        Self {
            title: format!("Stateful update · {}", obligation.sequence),
            sections: vec![
                ("Examined", packet.examined),
                ("Why it matters", packet.rationale),
                ("Learned", packet.learning),
                ("Implications", packet.implication),
                ("Strategy", packet.strategy),
                ("Changed", packet.changed),
                ("Next", packet.next),
                ("Uncertainty", packet.uncertainty),
                ("Blockers", packet.blockers),
                ("Your judgment", packet.requested_judgment),
            ],
        }
    }

    fn for_run(run: StatefulRun) -> Self {
        let status = match run.status {
            StatefulRunStatus::Pending => "pending",
            StatefulRunStatus::Running => "running",
            StatefulRunStatus::Paused => "paused",
            StatefulRunStatus::Completed => "completed",
            StatefulRunStatus::Cancelled => "cancelled",
            StatefulRunStatus::Blocked => "blocked",
            StatefulRunStatus::Failed => "failed",
        };
        Self {
            title: format!("Stateful run · {status}"),
            sections: vec![
                ("Goal", vec![run.goal]),
                ("Result", run.result.into_iter().collect()),
                ("Final strategy", run.strategy.into_iter().collect()),
            ],
        }
    }

    fn raw(&self) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(self.title.clone())];
        for (label, values) in &self.sections {
            if values.is_empty() {
                continue;
            }
            lines.push(Line::from((*label).to_string()));
            lines.extend(values.iter().map(|value| Line::from(format!("- {value}"))));
        }
        lines
    }
}

impl HistoryCell for StatefulSemanticHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(self.title.clone().bold())];
        let width = usize::from(width.max(1));
        for (label, values) in &self.sections {
            if values.is_empty() {
                continue;
            }
            lines.push(Line::from((*label).bold()));
            for value in values {
                lines.extend(word_wrap_lines(
                    [Line::from(value.clone())],
                    RtOptions::new(width)
                        .initial_indent("  • ".dim().into())
                        .subsequent_indent("    ".into()),
                ));
            }
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.raw()
    }
}

#[cfg(test)]
#[path = "stateful_ui_tests.rs"]
mod tests;
