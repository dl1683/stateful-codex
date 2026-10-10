use std::sync::Arc;

mod api;

use api::api_measurement_summary;
use api::api_obligation;
use api::api_run;
use api::api_steering;
use api::api_turn_measurement;
use api::workflow_mode;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::StatefulHostAnswer;
use codex_app_server_protocol::StatefulMeasurementListParams;
use codex_app_server_protocol::StatefulMeasurementListResponse;
use codex_app_server_protocol::StatefulMeasurementSummaryParams;
use codex_app_server_protocol::StatefulMeasurementSummaryResponse;
use codex_app_server_protocol::StatefulRunCancelParams;
use codex_app_server_protocol::StatefulRunCancelResponse;
use codex_app_server_protocol::StatefulRunPauseParams;
use codex_app_server_protocol::StatefulRunPauseResponse;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunRecovery;
use codex_app_server_protocol::StatefulRunResumeParams;
use codex_app_server_protocol::StatefulRunResumeResponse;
use codex_app_server_protocol::StatefulRunSetModeParams;
use codex_app_server_protocol::StatefulRunSetModeResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunUpdatedNotification;
use codex_app_server_protocol::SteeringListParams;
use codex_app_server_protocol::SteeringListResponse;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::SteeringUpdatedNotification;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_stateful_extension::RunAdmissionFence;
use codex_stateful_extension::bound_run_turn;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::NewSteeringInstruction;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunModeUpdate;
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

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;
use crate::outgoing_message::OutgoingMessageSender;
use crate::stateful_store::StatefulStoreHandle;

const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 101;

#[derive(Clone)]
pub(crate) struct StatefulRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    store: StatefulStoreHandle,
    outgoing: Arc<OutgoingMessageSender>,
    thread_manager: Arc<ThreadManager>,
    run_admission: RunAdmissionFence,
}

impl StatefulRequestProcessor {
    pub(crate) fn new(
        thread_store: Arc<dyn ThreadStore>,
        store: StatefulStoreHandle,
        outgoing: Arc<OutgoingMessageSender>,
        thread_manager: Arc<ThreadManager>,
        run_admission: RunAdmissionFence,
    ) -> Self {
        Self {
            thread_store,
            store,
            outgoing,
            thread_manager,
            run_admission,
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
        let newly_created = store.get_run(&id).await.map_err(runtime_error)?.is_none();
        let thread_id = params.thread_id.clone();
        let run = store
            .create_run(
                id,
                NewStatefulRun {
                    project_id: params.project_id,
                    thread_ids: vec![params.thread_id],
                    goal: params.goal,
                    mode: workflow_mode(params.mode),
                    budget: RunBudget {
                        max_continuations: params.budget.max_continuations,
                        max_elapsed_seconds: params.budget.max_elapsed_seconds,
                    },
                },
            )
            .await
            .map_err(runtime_error)?;
        // A new Autonomous run on an idle thread may end with its first answering task's
        // answer; an idempotent replay, a busy thread or any other mode never may.
        if newly_created
            && run.value.mode == codex_stateful_runtime::WorkflowMode::Autonomous
            && self.thread_is_idle(&thread_id).await
        {
            self.run_admission
                .host_answer_candidates()
                .grant(run.id.as_str(), &thread_id);
        }
        self.notify_run(&run).await;
        Ok(Some(StatefulRunStartResponse { run: api_run(run) }.into()))
    }

    /// Whether the thread is loaded here and has no running turn.
    async fn thread_is_idle(&self, thread_id: &str) -> bool {
        let Ok(thread_id) = ThreadId::from_string(thread_id) else {
            return false;
        };
        match self.thread_manager.get_thread(thread_id).await {
            Ok(thread) => thread.active_turn_id().await.is_none(),
            Err(_) => false,
        }
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
        let recovery = match run.as_ref() {
            Some(run) => Some(
                store
                    .autonomous_recovery_state(&run.id)
                    .await
                    .map_err(runtime_error)?,
            ),
            None => None,
        };
        let host_answer = match run.as_ref() {
            Some(run) => store
                .host_answer(&run.id)
                .await
                .map_err(runtime_error)?
                .map(|record| StatefulHostAnswer {
                    turn_id: record.turn_id,
                    answer: record.answer,
                    basis: record.basis,
                    committed_at: record.committed_at_ms.div_euclid(/*rhs*/ 1000),
                }),
            None => None,
        };
        Ok(Some(
            StatefulRunReadResponse {
                run: run.map(api_run),
                recovery: recovery.map(|state| StatefulRunRecovery {
                    lease_expires_at: state.lease_expires_at_ms.map(|value| value / 1_000),
                    previous_turn_id: state.previous_turn_id,
                    last_continuation_claimed_at: state
                        .last_claimed_at_ms
                        .map(|value| value / 1_000),
                }),
                host_answer,
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

    /// Cancels the run and interrupts its turns that are active in this process.
    ///
    /// The admission fence keeps an Autonomous continuation from starting between
    /// reading the active turns and the cancelled status write. Only turns read before
    /// that write are interrupted, so a newer user turn is never touched.
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "the admission fence must span the active-turn read through the status write and interruption"
    )]
    pub(crate) async fn run_cancel(
        &self,
        params: StatefulRunCancelParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let _admission = self.run_admission.lock().await;
        let id = parse_run_id(params.run_id.clone())?;
        let current = self
            .store()
            .await?
            .get_run(&id)
            .await
            .map_err(runtime_error)?
            .ok_or_else(|| invalid_params(format!("run not found: {id}")))?;
        let mut active_turns = Vec::new();
        for raw_thread_id in &current.value.thread_ids {
            let Ok(thread_id) = ThreadId::from_string(raw_thread_id) else {
                continue;
            };
            let Ok(thread) = self.thread_manager.get_thread(thread_id).await else {
                continue;
            };
            // Only the active turn that started bound to this run is its own; another
            // run's turn on the same thread is never interrupted.
            let bound_turn = bound_run_turn(thread.thread_extension_data(), id.as_str());
            if let Some(turn_id) = thread.active_turn_id().await
                && bound_turn.as_deref() == Some(turn_id.as_str())
            {
                active_turns.push((thread, turn_id));
            }
        }
        let run = self
            .update_status(
                params.run_id,
                params.expected_revision,
                StatefulRunStatus::Cancelled,
            )
            .await?;
        let mut interrupted_turn_ids = Vec::new();
        for (thread, turn_id) in active_turns {
            if thread.interrupt_turn(&turn_id).await {
                interrupted_turn_ids.push(turn_id);
            }
        }
        Ok(Some(
            StatefulRunCancelResponse {
                run: api_run(run),
                interrupted_turn_ids,
            }
            .into(),
        ))
    }

    pub(crate) async fn run_set_mode(
        &self,
        params: StatefulRunSetModeParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let id = parse_run_id(params.run_id)?;
        let run = self
            .store()
            .await?
            .update_mode(
                &id,
                StatefulRunModeUpdate {
                    expected_revision: params.expected_revision,
                    mode: workflow_mode(params.mode),
                },
            )
            .await
            .map_err(runtime_error)?;
        self.notify_run(&run).await;
        Ok(Some(
            StatefulRunSetModeResponse { run: api_run(run) }.into(),
        ))
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

    pub(crate) async fn measurement_list(
        &self,
        params: StatefulMeasurementListParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let limit = list_limit(params.limit)?;
        let page = self
            .store()
            .await?
            .list_project_measurements(&params.project_id, params.cursor.as_deref(), limit)
            .await
            .map_err(runtime_error)?;
        Ok(Some(
            StatefulMeasurementListResponse {
                data: page.data.into_iter().map(api_turn_measurement).collect(),
                next_cursor: page.next_cursor,
            }
            .into(),
        ))
    }

    pub(crate) async fn measurement_summary(
        &self,
        params: StatefulMeasurementSummaryParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let limit = list_limit(params.limit)?;
        let summary = self
            .store()
            .await?
            .summarize_project_measurements(&params.project_id, limit)
            .await
            .map_err(runtime_error)?;
        Ok(Some(
            StatefulMeasurementSummaryResponse {
                summary: api_measurement_summary(summary),
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
        self.store
            .get()
            .await
            .map_err(runtime_error)
            .and_then(|store| {
                store.ok_or_else(|| {
                    method_not_found("Stateful runs are unavailable without sqlite state")
                })
            })
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
        | StatefulRunStoreError::InvalidModeTransition
        | StatefulRunStoreError::ObligationNotFound(_)
        | StatefulRunStoreError::ProjectMismatch
        | StatefulRunStoreError::SteeringIdentityConflict(_)
        | StatefulRunStoreError::SteeringNotFound(_)
        | StatefulRunStoreError::InvalidSteeringTransition { .. }
        | StatefulRunStoreError::StrategyRevisionMismatch
        | StatefulRunStoreError::InvalidListLimit
        | StatefulRunStoreError::InvalidListCursor
        | StatefulRunStoreError::MeasurementThreadMismatch
        | StatefulRunStoreError::MeasurementIdentityConflict
        | StatefulRunStoreError::MeasurementNotFound
        | StatefulRunStoreError::InvalidMeasurementTimestamp
        | StatefulRunStoreError::MeasurementPendingCapacity
        | StatefulRunStoreError::MeasurementMergeTimeout
        | StatefulRunStoreError::MeasurementWriterClosed => invalid_params(error.to_string()),
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
