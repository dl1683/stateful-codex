use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_extension_api::ExtensionFuture;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_stateful_runtime::AutonomousClaimOutcome;
use codex_stateful_runtime::AutonomousClaimRequest;

use crate::SelectedProject;
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
            let outcome = store
                .claim_autonomous_continuation(
                    &run.id,
                    AutonomousClaimRequest {
                        owner_id: autonomous.owner_id.clone(),
                        previous_turn_id: previous_turn_id.to_string(),
                        lease_duration_ms: 120_000,
                    },
                )
                .await;
            match outcome {
                Ok(AutonomousClaimOutcome::Claimed { run, .. }) => {
                    emit_run_updated(self.event_sink.as_deref(), &run);
                    if let Err(error) = autonomous
                        .sink
                        .continue_run(AutonomousContinuationRequest {
                            thread_id: input.thread_id.to_string(),
                            run_id: run.id.to_string(),
                            previous_turn_id: previous_turn_id.to_string(),
                        })
                        .await
                    {
                        tracing::warn!(run_id = %run.id, %error, "failed to submit Autonomous continuation");
                    }
                }
                Ok(AutonomousClaimOutcome::BudgetExhausted(run)) => {
                    emit_run_updated(self.event_sink.as_deref(), &run);
                }
                Ok(
                    AutonomousClaimOutcome::AlreadyClaimed
                    | AutonomousClaimOutcome::Leased
                    | AutonomousClaimOutcome::NotEligible,
                ) => {}
                Err(error) => {
                    tracing::warn!(run_id = %run.id, %error, "failed to claim Autonomous continuation");
                }
            }
        })
    }
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
