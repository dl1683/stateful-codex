use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_extension_api::ExtensionFuture;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadReadyInput;
use codex_stateful_runtime::AutonomousClaimOutcome;
use codex_stateful_runtime::AutonomousClaimRequest;
use codex_stateful_runtime::StatefulRunId;

use crate::SelectedProject;
use crate::SelectedThread;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::StatefulExtension;

/// One host request to continue a claimed Autonomous run on its thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutonomousContinuationRequest {
    pub thread_id: String,
    pub run_id: String,
    pub previous_turn_id: String,
}

pub type AutonomousContinuationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

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

/// Process-scoped ownership and host submission service for Autonomous runs.
#[derive(Clone)]
pub struct AutonomousContinuation {
    pub(crate) owner_id: String,
    pub(crate) sink: Arc<dyn AutonomousContinuationSink>,
}

impl AutonomousContinuation {
    pub fn new(owner_id: String, sink: Arc<dyn AutonomousContinuationSink>) -> Self {
        Self { owner_id, sink }
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
                    tokio::time::sleep(recovery_delay(lease_expires_at_ms)).await;
                    let _ = attempt_continuation(
                        &services,
                        &autonomous,
                        event_sink.as_deref(),
                        &request,
                    )
                    .await;
                });
            }
        })
    }
}

struct PendingContinuation {
    thread_id: String,
    run_id: StatefulRunId,
    previous_turn_id: String,
}

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
            if let Err(error) = autonomous
                .sink
                .continue_run(AutonomousContinuationRequest {
                    thread_id: request.thread_id.clone(),
                    run_id: run.id.to_string(),
                    previous_turn_id: request.previous_turn_id.clone(),
                })
                .await
            {
                tracing::warn!(run_id = %run.id, %error, "failed to submit Autonomous continuation");
                return Some(lease_expires_at_ms);
            }
            None
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
