use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_extension_api::CodexErrorDetails;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadReadyInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_protocol::items::TurnItem;
use codex_stateful_runtime::AutonomousClaimOutcome;
use codex_stateful_runtime::AutonomousClaimRequest;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::StatefulRunUpdate;

use crate::SelectedProject;
use crate::SelectedThread;
use crate::StatefulAttributionStatus;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::StatefulExtension;
use crate::attribution::begin_turn_attribution;
use crate::attribution::fail_turn_attribution;
use crate::attribution::finish_turn_attribution;

/// One host request to continue a claimed Autonomous run on its thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutonomousContinuationRequest {
    pub thread_id: String,
    pub run_id: String,
    pub previous_turn_id: String,
}

/// Host disposition after attempting to submit one claimed continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutonomousContinuationOutcome {
    Started,
    YieldedToNewerTurn,
}

pub type AutonomousContinuationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AutonomousContinuationOutcome, String>> + Send + 'a>>;

/// Host boundary used by the Stateful supervisor to start a model turn.
///
/// Implementations must submit only if `previous_turn_id` is still the most
/// recently started turn. A superseding user turn should be treated as a safe
/// no-op rather than queued behind current work.
pub trait AutonomousContinuationSink: Send + Sync {
    fn continue_run<'a>(
        &'a self,
        request: AutonomousContinuationRequest,
    ) -> AutonomousContinuationFuture<'a>;
}

/// Process-wide fence between Autonomous continuation admission and run cancellation.
///
/// Continuation holds it from claiming a run through starting its turn; cancellation holds
/// it from reading the run's active turns through the cancelled status write and their
/// interruption. Either cancellation sees the started turn, or the claim sees the
/// cancelled status, so no continuation starts after a cancel in this process.
#[derive(Clone, Default)]
pub struct RunAdmissionFence(Arc<tokio::sync::Mutex<()>>);

impl RunAdmissionFence {
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.0.lock().await
    }
}

/// Process-scoped ownership and host submission service for Autonomous runs.
#[derive(Clone)]
pub struct AutonomousContinuation {
    pub(crate) owner_id: String,
    pub(crate) sink: Arc<dyn AutonomousContinuationSink>,
    pub(crate) admission: RunAdmissionFence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveRunTurn {
    run_id: StatefulRunId,
    turn_id: String,
}

/// Returns the turn on this thread that is bound to `run_id`, if any.
///
/// The binding is recorded when the turn starts, so a turn that belongs to another
/// run on the same thread is never reported.
pub fn bound_run_turn(thread_store: &ExtensionData, run_id: &str) -> Option<String> {
    let bound = thread_store.get::<ActiveRunTurn>()?;
    (bound.run_id.as_str() == run_id).then(|| bound.turn_id.clone())
}

impl AutonomousContinuation {
    pub fn new(
        owner_id: String,
        sink: Arc<dyn AutonomousContinuationSink>,
        admission: RunAdmissionFence,
    ) -> Self {
        Self {
            owner_id,
            sink,
            admission,
        }
    }
}

impl<C: Sync> ThreadLifecycleContributor<C> for StatefulExtension {
    fn on_thread_ready<'a>(&'a self, input: ThreadReadyInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(SelectedThread::new(input.thread_id.to_string()));
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.cause != ThreadIdleCause::Completed {
                return;
            }
            let (Some(previous_turn_id), Some(selected), Some(services), Some(autonomous)) = (
                input.previous_turn_id,
                input.thread_store.get::<SelectedProject>(),
                self.services.as_ref(),
                self.autonomous.as_ref(),
            ) else {
                return;
            };
            let store = match services.runtime().await {
                Ok(store) => store,
                Err(error) => {
                    tracing::warn!(%error, "failed to open Autonomous run store");
                    return;
                }
            };
            let run = match store.run_for_thread(&input.thread_id.to_string()).await {
                Ok(Some(run)) if run.value.project_id == selected.project_id() => run,
                Ok(_) => return,
                Err(error) => {
                    tracing::warn!(thread_id = %input.thread_id, %error, "failed to load Autonomous run");
                    return;
                }
            };
            let Some(active_turn) = input.thread_store.get::<ActiveRunTurn>() else {
                return;
            };
            if active_turn.run_id != run.id || active_turn.turn_id != previous_turn_id {
                return;
            }
            let request = PendingContinuation {
                thread_id: input.thread_id.to_string(),
                run_id: run.id,
                previous_turn_id: previous_turn_id.to_string(),
            };
            if let Some(lease_expires_at_ms) =
                attempt_continuation(services, autonomous, self.event_sink.as_deref(), &request)
                    .await
            {
                let services = services.clone();
                let autonomous = autonomous.clone();
                let event_sink = self.event_sink.clone();
                tokio::spawn(async move {
                    let mut retry_at_ms = Some(lease_expires_at_ms);
                    while let Some(lease_expires_at_ms) = retry_at_ms {
                        tokio::time::sleep(recovery_delay(lease_expires_at_ms)).await;
                        retry_at_ms = attempt_continuation(
                            &services,
                            &autonomous,
                            event_sink.as_deref(),
                            &request,
                        )
                        .await;
                    }
                });
            }
        })
    }
}

impl StatefulExtension {
    /// The selected project's configured roots, or none when it cannot be read.
    async fn project_roots(&self, project_id: &str) -> Vec<String> {
        match self.projects.read_project(project_id.to_string()).await {
            Ok(Some(project)) => project.roots.into_iter().map(|root| root.path).collect(),
            Ok(None) => Vec::new(),
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to read project roots");
                Vec::new()
            }
        }
    }

    /// Records the checkout as a turn left it, so the next turn reports only later changes.
    async fn observe_checkout_at_turn_end(&self, thread_store: &ExtensionData) {
        let (Some(selected), Some(services)) = (
            thread_store.get::<SelectedProject>(),
            self.services.as_ref(),
        ) else {
            return;
        };
        let roots = self.project_roots(selected.project_id()).await;
        crate::checkout::observe_turn_end(services, selected.project_id(), &roots).await;
    }
}

impl TurnLifecycleContributor for StatefulExtension {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            begin_turn_attribution(self, input.turn_id, input.thread_store);
            if let Some(selected) = input.thread_store.get::<SelectedProject>() {
                crate::request_scope::RequestScope::record_turn_start(
                    input.turn_store,
                    input.user_input,
                );
                if let Some(thread) = input.thread_store.get::<SelectedThread>() {
                    self.user_messages.record(
                        &thread.thread_id,
                        selected.project_id(),
                        input.turn_id,
                        input.user_input,
                    );
                }
                if let (Some(thread), Some(services)) = (
                    input.thread_store.get::<SelectedThread>(),
                    self.services.as_ref(),
                ) {
                    let text = input
                        .user_input
                        .iter()
                        .filter_map(|item| match item {
                            codex_protocol::user_input::UserInput::Text { text, .. } => {
                                Some(text.as_str())
                            }
                            // `UserInput` is non-exhaustive; rules are read from text only.
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    crate::rule_capture::capture_background(
                        services,
                        self.event_sink.as_deref(),
                        selected.project_id(),
                        &thread.thread_id,
                        input.turn_id,
                        &text,
                    )
                    .await;
                    crate::rule_group::capture_marked_rules(
                        services,
                        self.event_sink.as_deref(),
                        selected.project_id(),
                        &thread.thread_id,
                        input.turn_id,
                        &text,
                    )
                    .await;
                    if let Ok(store) = services.blackboard().await {
                        crate::rule_scope::observe_turn_start(
                            store,
                            selected.project_id(),
                            &thread.thread_id,
                            input.turn_id,
                            &text,
                            crate::request_scope::RequestScope::of_turn(input.turn_store),
                        )
                        .await;
                    }
                    let roots = self.project_roots(selected.project_id()).await;
                    if let Some(report) = crate::checkout::observe_turn_start(
                        services,
                        self.event_sink.as_deref(),
                        selected.project_id(),
                        &roots,
                        crate::checkout::TurnRef {
                            thread_id: &thread.thread_id,
                            turn_id: input.turn_id,
                        },
                    )
                    .await
                    {
                        input.turn_store.insert(report);
                    }
                }
            }
            let (Some(selected), Some(thread), Some(services)) = (
                input.thread_store.get::<SelectedProject>(),
                input.thread_store.get::<SelectedThread>(),
                self.services.as_ref(),
            ) else {
                input.thread_store.remove::<ActiveRunTurn>();
                return;
            };
            let store = match services.runtime().await {
                Ok(store) => store,
                Err(error) => {
                    input.thread_store.remove::<ActiveRunTurn>();
                    tracing::warn!(%error, "failed to open Stateful run store at turn start");
                    return;
                }
            };
            match store.run_for_thread(&thread.thread_id).await {
                Ok(Some(run)) if run.value.project_id == selected.project_id() => {
                    self.attribution.bind_run(input.turn_id, run.id.clone());
                    input.thread_store.insert(ActiveRunTurn {
                        run_id: run.id,
                        turn_id: input.turn_id.to_string(),
                    });
                }
                Ok(_) => {
                    input.thread_store.remove::<ActiveRunTurn>();
                }
                Err(error) => {
                    input.thread_store.remove::<ActiveRunTurn>();
                    tracing::warn!(
                        thread_id = %thread.thread_id,
                        %error,
                        "failed to bind a Stateful run to its starting turn"
                    );
                }
            }
        })
    }

    fn on_item_completed<'a>(
        &'a self,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        if let TurnItem::UserMessage(message) = item
            && thread_store.get::<SelectedProject>().is_some()
        {
            crate::request_scope::RequestScope::observe_user_message(turn_store, &message.content);
        }
        Box::pin(std::future::ready(()))
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            finish_turn_attribution(self, input.turn_store, StatefulAttributionStatus::Completed);
            self.observe_checkout_at_turn_end(input.thread_store).await;
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            finish_turn_attribution(self, input.turn_store, StatefulAttributionStatus::Aborted);
            self.observe_checkout_at_turn_end(input.thread_store).await;
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            fail_turn_attribution(self, input.turn_id);
            if !matches!(input.error_details, CodexErrorDetails::TaskPanicked(_)) {
                return;
            }
            let (Some(active_turn), Some(services)) = (
                input.thread_store.get::<ActiveRunTurn>(),
                self.services.as_ref(),
            ) else {
                return;
            };
            if active_turn.turn_id == input.turn_id {
                fail_run_after_task_panic(
                    services,
                    self.event_sink.as_deref(),
                    &active_turn.run_id,
                )
                .await;
            }
        })
    }
}

/// Marks a running run failed after its bound turn's task panicked.
///
/// Retries revision conflicts from unrelated updates; never overrides a run that a user or the
/// model already paused, cancelled, blocked, or completed.
async fn fail_run_after_task_panic(
    services: &crate::services::ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    run_id: &StatefulRunId,
) {
    const ATTEMPTS: usize = 3;
    let store = match services.runtime().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%error, "failed to open Stateful run store after a task panic");
            return;
        }
    };
    for _ in 0..ATTEMPTS {
        let run = match store.get_run(run_id).await {
            Ok(Some(run)) if run.status == StatefulRunStatus::Running => run,
            Ok(_) => return,
            Err(error) => {
                tracing::warn!(%run_id, %error, "failed to load the run of a panicked turn");
                return;
            }
        };
        match store
            .update_run(
                run_id,
                StatefulRunUpdate {
                    expected_revision: run.revision,
                    status: StatefulRunStatus::Failed,
                    strategy: run.strategy,
                    result: run.result,
                },
            )
            .await
        {
            Ok(run) => {
                emit_run_updated(event_sink, &run);
                return;
            }
            Err(StatefulRunStoreError::RevisionConflict { .. }) => {}
            Err(error) => {
                tracing::warn!(%run_id, %error, "failed to mark the run of a panicked turn failed");
                return;
            }
        }
    }
    tracing::warn!(%run_id, "run kept changing; it was not marked failed after a task panic");
}

struct PendingContinuation {
    thread_id: String,
    run_id: StatefulRunId,
    previous_turn_id: String,
}

#[expect(
    clippy::await_holding_invalid_type,
    reason = "the admission fence must span the claim through the continuation's turn start"
)]
async fn attempt_continuation(
    services: &crate::services::ProjectIntelligenceServices,
    autonomous: &AutonomousContinuation,
    event_sink: Option<&dyn StatefulEventSink>,
    request: &PendingContinuation,
) -> Option<i64> {
    let store = match services.runtime().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%error, "failed to open Autonomous run store");
            return None;
        }
    };
    // Held from the claim's running check through the turn start; see RunAdmissionFence.
    let _admission = autonomous.admission.lock().await;
    match store
        .claim_autonomous_continuation(
            &request.run_id,
            AutonomousClaimRequest {
                owner_id: autonomous.owner_id.clone(),
                previous_turn_id: request.previous_turn_id.clone(),
                lease_duration_ms: 120_000,
            },
        )
        .await
    {
        Ok(AutonomousClaimOutcome::Claimed {
            run,
            lease_expires_at_ms,
        }) => {
            emit_run_updated(event_sink, &run);
            match autonomous
                .sink
                .continue_run(AutonomousContinuationRequest {
                    thread_id: request.thread_id.clone(),
                    run_id: run.id.to_string(),
                    previous_turn_id: request.previous_turn_id.clone(),
                })
                .await
            {
                Ok(AutonomousContinuationOutcome::Started) => None,
                Ok(AutonomousContinuationOutcome::YieldedToNewerTurn) => {
                    match store
                        .abandon_autonomous_continuation(
                            &run.id,
                            &autonomous.owner_id,
                            &request.previous_turn_id,
                        )
                        .await
                    {
                        Ok(Some(run)) => emit_run_updated(event_sink, &run),
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(run_id = %run.id, %error, "failed to release superseded Autonomous continuation");
                            return Some(lease_expires_at_ms);
                        }
                    }
                    None
                }
                Err(error) => {
                    tracing::warn!(run_id = %run.id, %error, "failed to submit Autonomous continuation");
                    Some(lease_expires_at_ms)
                }
            }
        }
        Ok(AutonomousClaimOutcome::BudgetExhausted(run)) => {
            emit_run_updated(event_sink, &run);
            None
        }
        Ok(AutonomousClaimOutcome::Leased {
            lease_expires_at_ms,
        }) => Some(lease_expires_at_ms),
        Ok(AutonomousClaimOutcome::AlreadyClaimed | AutonomousClaimOutcome::NotEligible) => None,
        Err(error) => {
            tracing::warn!(run_id = %request.run_id, %error, "failed to claim Autonomous continuation");
            None
        }
    }
}

fn recovery_delay(lease_expires_at_ms: i64) -> std::time::Duration {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let remaining = u128::try_from(lease_expires_at_ms)
        .unwrap_or_default()
        .saturating_sub(now_ms)
        .saturating_add(50)
        .min(u128::from(u64::MAX));
    std::time::Duration::from_millis(remaining as u64)
}

fn emit_run_updated(
    sink: Option<&dyn StatefulEventSink>,
    run: &codex_stateful_runtime::StatefulRun,
) {
    if let Some(sink) = sink {
        sink.emit(StatefulEvent::RunUpdated {
            project_id: run.value.project_id.clone(),
            run_id: run.id.to_string(),
            revision: run.revision,
        });
    }
}

#[cfg(test)]
#[path = "autonomy_tests.rs"]
mod tests;
