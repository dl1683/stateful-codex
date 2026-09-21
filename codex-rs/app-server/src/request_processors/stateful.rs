use std::sync::Arc;

mod api;

use api::api_obligation;
use api::api_run;
use api::api_steering;
use api::workflow_mode;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::StatefulRunCancelParams;
use codex_app_server_protocol::StatefulRunCancelResponse;
use codex_app_server_protocol::StatefulRunPauseParams;
use codex_app_server_protocol::StatefulRunPauseResponse;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunResumeParams;
use codex_app_server_protocol::StatefulRunResumeResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunUpdatedNotification;
use codex_app_server_protocol::SteeringListParams;
use codex_app_server_protocol::SteeringListResponse;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::SteeringUpdatedNotification;
use codex_protocol::ThreadId;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::NewSteeringInstruction;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringId;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use sha2::Digest;
use sha2::Sha256;
use tokio::sync::OnceCell;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;
use crate::outgoing_message::OutgoingMessageSender;

const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 101;

#[derive(Clone)]
pub(crate) struct StatefulRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    store: Arc<OnceCell<StatefulRunStore>>,
    outgoing: Arc<OutgoingMessageSender>,
}

impl StatefulRequestProcessor {
    pub(crate) fn new(
        thread_store: Arc<dyn ThreadStore>,
        sqlite: Option<SqliteConfig>,
        outgoing: Arc<OutgoingMessageSender>,
    ) -> Self {
        Self {
            thread_store,
            sqlite,
            store: Arc::new(OnceCell::new()),
            outgoing,
        }
    }

    pub(crate) async fn run_start(
        &self,
        params: StatefulRunStartParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        validate_idempotency_key(&params.idempotency_key)?;
        self.validate_thread_project(&params.thread_id, &params.project_id)
            .await?;
        let id = stable_id("run", &params.project_id, &params.idempotency_key)?;
        let store = self.store().await?;
        if let Some(active) = store
            .run_for_thread(&params.thread_id)
            .await
            .map_err(runtime_error)?
            && active.id != id
        {
            return Err(invalid_params(format!(
                "thread already has active Stateful run {}",
                active.id
            )));
        }
        let run = store
            .create_run(
                id,
                NewStatefulRun {
                    project_id: params.project_id,
                    thread_ids: vec![params.thread_id],
                    goal: params.goal,
                    mode: workflow_mode(params.mode),
                },
            )
            .await
            .map_err(runtime_error)?;
        self.notify_run(&run).await;
        Ok(Some(StatefulRunStartResponse { run: api_run(run) }.into()))
    }

    pub(crate) async fn run_read(
        &self,
        params: StatefulRunReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let store = self.store().await?;
        let run = match (params.run_id, params.thread_id) {
            (Some(run_id), None) => store
                .get_run(&parse_run_id(run_id)?)
                .await
                .map_err(runtime_error)?,
            (None, Some(thread_id)) => store
                .run_for_thread(&thread_id)
                .await
                .map_err(runtime_error)?,
            _ => return Err(invalid_params("provide exactly one of runId or threadId")),
        };
        Ok(Some(
            StatefulRunReadResponse {
                run: run.map(api_run),
            }
            .into(),
        ))
    }

    pub(crate) async fn run_pause(
        &self,
        params: StatefulRunPauseParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let run = self
            .update_status(
                params.run_id,
                params.expected_revision,
                StatefulRunStatus::Paused,
            )
            .await?;
        Ok(Some(StatefulRunPauseResponse { run: api_run(run) }.into()))
    }

    pub(crate) async fn run_resume(
        &self,
        params: StatefulRunResumeParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let run = self
            .update_status(
                params.run_id,
                params.expected_revision,
                StatefulRunStatus::Running,
            )
            .await?;
        Ok(Some(StatefulRunResumeResponse { run: api_run(run) }.into()))
    }

    pub(crate) async fn run_cancel(
        &self,
        params: StatefulRunCancelParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let run = self
            .update_status(
                params.run_id,
                params.expected_revision,
                StatefulRunStatus::Cancelled,
            )
            .await?;
        Ok(Some(StatefulRunCancelResponse { run: api_run(run) }.into()))
    }

    pub(crate) async fn obligation_list(
        &self,
        params: ObligationListParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let run_id = parse_run_id(params.run_id)?;
        let limit = list_limit(params.limit)?;
        let cursor = params
            .cursor
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| invalid_params("invalid obligation cursor"))
            })
            .transpose()?;
        let mut data = self
            .store()
            .await?
            .list_obligations(&run_id, cursor, limit.saturating_add(1))
            .await
            .map_err(runtime_error)?;
        let next_cursor =
            (data.len() > limit as usize).then(|| data[limit as usize - 1].sequence.to_string());
        data.truncate(limit as usize);
        Ok(Some(
            ObligationListResponse {
                data: data.into_iter().map(api_obligation).collect(),
                next_cursor,
            }
            .into(),
        ))
    }

    pub(crate) async fn steering_submit(
        &self,
        params: SteeringSubmitParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        validate_idempotency_key(&params.idempotency_key)?;
        let run_id = parse_run_id(params.run_id)?;
        let store = self.store().await?;
        let run = store
            .get_run(&run_id)
            .await
            .map_err(runtime_error)?
            .ok_or_else(|| invalid_params(format!("run not found: {run_id}")))?;
        let id = stable_id("steering", run_id.as_str(), &params.idempotency_key)?;
        let steering = store
            .submit_steering(
                SteeringId::parse(id.to_string())
                    .map_err(|error| invalid_params(error.to_string()))?,
                NewSteeringInstruction {
                    project_id: run.value.project_id,
                    run_id,
                    input: params.input,
                    affected_obligation_ids: params.affected_obligation_ids,
                },
            )
            .await
            .map_err(runtime_error)?;
        self.notify_steering(&steering).await;
        Ok(Some(
            SteeringSubmitResponse {
                steering: api_steering(steering),
            }
            .into(),
        ))
    }

    pub(crate) async fn steering_list(
        &self,
        params: SteeringListParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let run_id = parse_run_id(params.run_id)?;
        let limit = list_limit(params.limit)?;
        let cursor = params
            .cursor
            .map(SteeringId::parse)
            .transpose()
            .map_err(|error| invalid_params(error.to_string()))?;
        let mut data = self
            .store()
            .await?
            .list_steering(&run_id, cursor.as_ref(), limit.saturating_add(1))
            .await
            .map_err(runtime_error)?;
        let next_cursor =
            (data.len() > limit as usize).then(|| data[limit as usize - 1].id.to_string());
        data.truncate(limit as usize);
        Ok(Some(
            SteeringListResponse {
                data: data.into_iter().map(api_steering).collect(),
                next_cursor,
            }
            .into(),
        ))
    }

    async fn update_status(
        &self,
        raw_id: String,
        expected_revision: u64,
        status: StatefulRunStatus,
    ) -> Result<StatefulRun, JSONRPCErrorError> {
        let id = parse_run_id(raw_id)?;
        let store = self.store().await?;
        let current = store
            .get_run(&id)
            .await
            .map_err(runtime_error)?
            .ok_or_else(|| invalid_params(format!("run not found: {id}")))?;
        let run = store
            .update_run(
                &id,
                StatefulRunUpdate {
                    expected_revision,
                    status,
                    strategy: current.strategy,
                    result: current.result,
                },
            )
            .await
            .map_err(runtime_error)?;
        self.notify_run(&run).await;
        Ok(run)
    }

    async fn validate_thread_project(
        &self,
        raw_thread_id: &str,
        project_id: &str,
    ) -> Result<(), JSONRPCErrorError> {
        self.thread_store
            .read_project(project_id.to_string())
            .await
            .map_err(thread_store_error)?
            .ok_or_else(|| invalid_params(format!("project not found: {project_id}")))?;
        let thread_id = ThreadId::from_string(raw_thread_id)
            .map_err(|_| invalid_params("threadId must be a valid thread ID"))?;
        if self
            .thread_store
            .read_pending_thread_metadata(thread_id)
            .await
            .map_err(thread_store_error)?
            .and_then(|metadata| metadata.project_id.flatten())
            .as_deref()
            == Some(project_id)
        {
            return Ok(());
        }
        let thread = self
            .thread_store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await
            .map_err(thread_store_error)?;
        if thread.project_id.as_deref() != Some(project_id) {
            return Err(invalid_params(
                "thread does not belong to the selected project",
            ));
        }
        Ok(())
    }

    async fn store(&self) -> Result<&StatefulRunStore, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("Stateful runs are unavailable without sqlite state")
        })?;
        self.store
            .get_or_try_init(|| StatefulRunStore::open(sqlite))
            .await
            .map_err(runtime_error)
    }

    async fn notify_run(&self, run: &StatefulRun) {
        self.outgoing
            .send_server_notification(ServerNotification::StatefulRunUpdated(
                StatefulRunUpdatedNotification {
                    project_id: run.value.project_id.clone(),
                    run_id: run.id.to_string(),
                    revision: run.revision,
                    cursor: format!("run:{}:{}", run.id, run.revision),
                },
            ))
            .await;
    }

    async fn notify_steering(&self, steering: &StatefulSteering) {
        self.outgoing
            .send_server_notification(ServerNotification::SteeringUpdated(
                SteeringUpdatedNotification {
                    project_id: steering.value.project_id.clone(),
                    run_id: steering.value.run_id.to_string(),
                    steering_id: steering.id.to_string(),
                    revision: steering.revision,
                    cursor: format!("steering:{}:{}", steering.id, steering.revision),
                },
            ))
            .await;
    }
}

fn parse_run_id(value: String) -> Result<StatefulRunId, JSONRPCErrorError> {
    StatefulRunId::parse(value).map_err(|error| invalid_params(error.to_string()))
}

fn validate_idempotency_key(value: &str) -> Result<(), JSONRPCErrorError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(invalid_params(
            "idempotencyKey must be non-empty, at most 512 bytes, and contain no controls",
        ));
    }
    Ok(())
}

fn stable_id(
    kind: &str,
    scope: &str,
    idempotency_key: &str,
) -> Result<StatefulRunId, JSONRPCErrorError> {
    StatefulRunId::parse(format!(
        "{kind}-{:x}",
        Sha256::digest(format!("{scope}\0{idempotency_key}").as_bytes())
    ))
    .map_err(|error| invalid_params(error.to_string()))
}

fn list_limit(value: Option<u32>) -> Result<u32, JSONRPCErrorError> {
    let value = value.unwrap_or(DEFAULT_LIST_LIMIT);
    if value == 0 || value >= MAX_LIST_LIMIT {
        return Err(invalid_params("limit must be between 1 and 100"));
    }
    Ok(value)
}

fn runtime_error(error: StatefulRunStoreError) -> JSONRPCErrorError {
    match error {
        StatefulRunStoreError::InvalidRun(_)
        | StatefulRunStoreError::InvalidSteering(_)
        | StatefulRunStoreError::RunNotFound(_)
        | StatefulRunStoreError::RunIdentityConflict(_)
        | StatefulRunStoreError::RevisionConflict { .. }
        | StatefulRunStoreError::InvalidTransition { .. }
        | StatefulRunStoreError::ObligationNotFound(_)
        | StatefulRunStoreError::ProjectMismatch
        | StatefulRunStoreError::SteeringIdentityConflict(_)
        | StatefulRunStoreError::SteeringNotFound(_)
        | StatefulRunStoreError::InvalidSteeringTransition { .. }
        | StatefulRunStoreError::StrategyRevisionMismatch
        | StatefulRunStoreError::InvalidListLimit
        | StatefulRunStoreError::InvalidListCursor => invalid_params(error.to_string()),
        error => internal_error(format!("Stateful runtime failed: {error}")),
    }
}

fn thread_store_error(error: ThreadStoreError) -> JSONRPCErrorError {
    match error {
        ThreadStoreError::ThreadNotFound { .. }
        | ThreadStoreError::InvalidRequest { .. }
        | ThreadStoreError::Conflict { .. } => invalid_params(error.to_string()),
        ThreadStoreError::Unsupported { .. } => {
            method_not_found("Stateful runs are unavailable without sqlite state")
        }
        ThreadStoreError::Internal { .. } => {
            internal_error(format!("failed to validate Stateful run scope: {error}"))
        }
    }
}
