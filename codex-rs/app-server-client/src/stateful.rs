use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectListParams;
use codex_app_server_protocol::ProjectListResponse;
use codex_app_server_protocol::ProjectReadParams;
use codex_app_server_protocol::ProjectReadResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_utils_absolute_path::AbsolutePathBuf;
use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

use crate::AppServerRequestHandle;
use crate::TypedRequestError;

pub const DEFAULT_STATEFUL_MAX_CONTINUATIONS: u32 = 24;
pub const DEFAULT_STATEFUL_MAX_ELAPSED_SECONDS: u32 = 14_400;
static NEXT_STATEFUL_RUN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulStartup {
    mode: StatefulWorkflowMode,
    project_id: Option<String>,
    goal: String,
    idempotency_key: String,
}

impl StatefulStartup {
    pub fn new(
        mode: StatefulWorkflowMode,
        project_id: Option<String>,
        goal: impl Into<String>,
    ) -> Result<Self, StatefulStartupError> {
        let goal = goal.into().trim().to_string();
        if goal.is_empty() {
            return Err(StatefulStartupError::EmptyGoal);
        }
        Ok(Self {
            mode,
            project_id,
            goal,
            idempotency_key: new_run_idempotency_key(),
        })
    }
}

#[derive(Debug)]
pub struct PreparedStatefulStartup {
    project_id: String,
    mode: StatefulWorkflowMode,
    goal: String,
    idempotency_key: String,
}

pub async fn prepare_stateful_startup(
    request_handle: &AppServerRequestHandle,
    thread_params: &mut ThreadStartParams,
    startup: StatefulStartup,
) -> Result<PreparedStatefulStartup, StatefulStartupError> {
    let root = thread_params
        .cwd
        .as_deref()
        .ok_or(StatefulStartupError::MissingProjectDirectory)?;
    let root = AbsolutePathBuf::from_absolute_path(root)
        .map_err(|error| StatefulStartupError::InvalidProjectDirectory(error.to_string()))?;
    let project_id = resolve_project(request_handle, &root, startup.project_id).await?;
    thread_params.project_id = Some(project_id.clone());
    Ok(PreparedStatefulStartup {
        project_id,
        mode: startup.mode,
        goal: startup.goal,
        idempotency_key: startup.idempotency_key,
    })
}

pub async fn start_stateful_run(
    request_handle: &AppServerRequestHandle,
    startup: &PreparedStatefulStartup,
    thread_id: &str,
) -> Result<(), StatefulStartupError> {
    let _: StatefulRunStartResponse = request_handle
        .request_typed(ClientRequest::StatefulRunStart {
            request_id: RequestId::String(format!(
                "stateful-run-{thread_id}-{}",
                startup.idempotency_key
            )),
            params: StatefulRunStartParams {
                project_id: startup.project_id.clone(),
                thread_id: thread_id.to_string(),
                goal: startup.goal.clone(),
                mode: startup.mode,
                budget: StatefulRunBudget {
                    max_continuations: DEFAULT_STATEFUL_MAX_CONTINUATIONS,
                    max_elapsed_seconds: DEFAULT_STATEFUL_MAX_ELAPSED_SECONDS,
                },
                idempotency_key: startup.idempotency_key.clone(),
            },
        })
        .await
        .map_err(|source| StatefulStartupError::Request {
            operation: "start the Stateful run",
            source,
        })?;
    Ok(())
}

/// What a resumed thread's Stateful startup did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResumedStatefulRun {
    /// The thread had no open run, so a new run now owns the resumed prompt.
    Started,
    /// The thread's open run in the same project and mode was running when read; the host
    /// binds the resumed prompt's turn to whichever run is open when that turn starts.
    Continued { run_id: String },
}

/// Starts a run for a resumed thread, or continues the thread's open run when that run is
/// running in the same project and mode. Any other open run is refused with the control
/// that can change it.
pub async fn start_or_continue_stateful_run(
    request_handle: &AppServerRequestHandle,
    startup: &PreparedStatefulStartup,
    thread_id: &str,
) -> Result<ResumedStatefulRun, StatefulStartupError> {
    let read: StatefulRunReadResponse = request_handle
        .request_typed(ClientRequest::StatefulRunRead {
            request_id: RequestId::String(format!(
                "stateful-run-read-{thread_id}-{}",
                startup.idempotency_key
            )),
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread_id.to_string()),
            },
        })
        .await
        .map_err(|source| StatefulStartupError::Request {
            operation: "read the thread's Stateful run",
            source,
        })?;
    let Some(run) = read.run else {
        start_stateful_run(request_handle, startup, thread_id).await?;
        return Ok(ResumedStatefulRun::Started);
    };
    let guidance = if run.project_id != startup.project_id {
        "it belongs to another project; resume the thread without --stateful".to_string()
    } else if run.mode != startup.mode {
        format!(
            "it runs in {} mode; change its mode or cancel it in the web workspace, or pass --stateful {}",
            mode_name(run.mode),
            mode_name(run.mode)
        )
    } else {
        match run.status {
            StatefulRunStatus::Running => {
                return Ok(ResumedStatefulRun::Continued { run_id: run.id });
            }
            StatefulRunStatus::Pending => {
                "it is waiting to begin; begin it from the web workspace".to_string()
            }
            StatefulRunStatus::Paused | StatefulRunStatus::Blocked => {
                "resume or cancel it from the web workspace run controls".to_string()
            }
            StatefulRunStatus::Completed
            | StatefulRunStatus::Cancelled
            | StatefulRunStatus::Failed => {
                start_stateful_run(request_handle, startup, thread_id).await?;
                return Ok(ResumedStatefulRun::Started);
            }
        }
    };
    Err(StatefulStartupError::OpenRun {
        run_id: run.id,
        status: status_name(run.status),
        guidance,
    })
}

fn mode_name(mode: StatefulWorkflowMode) -> &'static str {
    match mode {
        StatefulWorkflowMode::Autonomous => "autonomous",
        StatefulWorkflowMode::Collaborative => "collaborative",
        StatefulWorkflowMode::Socratic => "socratic",
    }
}

fn status_name(status: StatefulRunStatus) -> &'static str {
    match status {
        StatefulRunStatus::Pending => "pending",
        StatefulRunStatus::Running => "running",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
    }
}

fn new_run_idempotency_key() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_STATEFUL_RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "stateful-client-run-v2-{}-{timestamp}-{sequence}",
        std::process::id()
    )
}

async fn resolve_project(
    request_handle: &AppServerRequestHandle,
    root: &AbsolutePathBuf,
    selected_project_id: Option<String>,
) -> Result<String, StatefulStartupError> {
    if let Some(project_id) = selected_project_id {
        let response: ProjectReadResponse = request_handle
            .request_typed(ClientRequest::ProjectRead {
                request_id: RequestId::String("stateful-project-read".to_string()),
                params: ProjectReadParams {
                    project_id: project_id.clone(),
                },
            })
            .await
            .map_err(|source| StatefulStartupError::Request {
                operation: "read the selected Stateful project",
                source,
            })?;
        if !response.project.roots.iter().any(|item| item.path == *root) {
            return Err(StatefulStartupError::SelectedProjectMismatch {
                project_id,
                root: root.as_path().display().to_string(),
            });
        }
        return Ok(response.project.id);
    }

    let matching_projects = projects_for_root(request_handle, root).await?;
    match matching_projects.as_slice() {
        [] => create_project(request_handle, root).await,
        [project_id] => Ok(project_id.clone()),
        project_ids => Err(StatefulStartupError::MultipleProjects {
            root: root.as_path().display().to_string(),
            project_ids: project_ids.join(", "),
        }),
    }
}

async fn projects_for_root(
    request_handle: &AppServerRequestHandle,
    root: &AbsolutePathBuf,
) -> Result<Vec<String>, StatefulStartupError> {
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
            .map_err(|source| StatefulStartupError::Request {
                operation: "list Stateful projects",
                source,
            })?;
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
) -> Result<String, StatefulStartupError> {
    let root_text = root.as_path().to_string_lossy();
    let digest = Sha256::digest(root_text.as_bytes());
    let response: ProjectCreateResponse = request_handle
        .request_typed(ClientRequest::ProjectCreate {
            request_id: RequestId::String(format!("stateful-project-{digest:x}")),
            params: ProjectCreateParams {
                name: project_name(root.as_path()),
                roots: vec![ProjectRoot { path: root.clone() }],
                metadata: None,
                idempotency_key: format!("stateful-client-project-v1-{digest:x}"),
            },
        })
        .await
        .map_err(|source| StatefulStartupError::Request {
            operation: "create the Stateful project",
            source,
        })?;
    Ok(response.project.id)
}

fn project_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| root.display().to_string())
}

#[derive(Debug, Error)]
pub enum StatefulStartupError {
    #[error("--stateful requires a non-empty goal prompt")]
    EmptyGoal,
    #[error("--stateful requires an explicit project directory")]
    MissingProjectDirectory,
    #[error("--stateful requires an absolute project directory: {0}")]
    InvalidProjectDirectory(String),
    #[error("Stateful project {project_id} does not include the selected directory {root}")]
    SelectedProjectMismatch { project_id: String, root: String },
    #[error(
        "multiple Stateful projects use {root}: {project_ids}. Select one with --stateful-project"
    )]
    MultipleProjects { root: String, project_ids: String },
    #[error("this thread already has an open Stateful run {run_id} ({status}): {guidance}")]
    OpenRun {
        run_id: String,
        status: &'static str,
        guidance: String,
    },
    #[error("failed to {operation}: {source}")]
    Request {
        operation: &'static str,
        #[source]
        source: TypedRequestError,
    },
}
