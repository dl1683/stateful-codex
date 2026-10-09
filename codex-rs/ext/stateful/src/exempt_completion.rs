//! Ledger-free completion is admitted when the completing turn ends, not when its completion
//! call runs.
//!
//! The completion call of an action-free run validates and leaves the run Running with an
//! exempt completion pending for its turn, holding the exact update it would commit. The rest
//! of that turn stays under the host's fences: every later call of the turn (local,
//! bookkeeping, a read, or a hosted call once observed) is recorded against the still-open
//! run. When the turn ends normally the pending completion commits Completed with the
//! exemption only if the run is still action-free; otherwise the run stays Running and the
//! next completion owes acceptance criteria (its attempt is no longer the sole one). An
//! interrupted, aborted or failed turn discards it, and a pending completion never survives
//! its process.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use codex_stateful_runtime::AcceptanceCommit;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::VerificationClaim;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

const FENCE_TIMEOUT: Duration = Duration::from_secs(5);
/// Lease of the commit's verification attempt.
const VERIFICATION_LEASE_MS: u32 = 120_000;

/// The commit a lone completion of an action-free run would have made.
pub(crate) struct PendingExemptCompletion {
    pub(crate) run_id: StatefulRunId,
    pub(crate) project_id: String,
    /// When the run was created; knowledge written since then needs durable learning.
    pub(crate) run_created_at_ms: i64,
    pub(crate) verification_owner: String,
    /// The terminal update, its result carrying the acceptance basis.
    pub(crate) update: StatefulRunUpdate,
    pub(crate) validated_obligation_sequence: Option<u64>,
}

/// Pending exempt completions by the turn that made them.
static PENDING: LazyLock<Mutex<HashMap<String, PendingExemptCompletion>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn pending() -> std::sync::MutexGuard<'static, HashMap<String, PendingExemptCompletion>> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Leaves `completion` pending until `turn_id` ends.
pub(crate) fn defer(turn_id: String, completion: PendingExemptCompletion) {
    pending().insert(turn_id, completion);
}

/// Whether `run_id` has an exempt completion pending.
pub(crate) fn is_pending(run_id: &StatefulRunId) -> bool {
    pending()
        .values()
        .any(|completion| &completion.run_id == run_id)
}

/// Drops the completion `turn_id` left pending: the turn did not end normally.
pub(crate) fn discard(turn_id: &str) {
    if pending().remove(turn_id).is_some() {
        tracing::info!(
            turn_id,
            "a pending no-tool completion was discarded because its turn did not end normally"
        );
    }
}

/// Commits the completion `turn_id` left pending, now that the turn ended normally, if the
/// run is still action-free. Otherwise the run stays Running.
pub(crate) async fn finish_turn(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    turn_id: &str,
) {
    let Some(completion) = pending().remove(turn_id) else {
        return;
    };
    let run_id = completion.run_id.clone();
    if let Err(reason) = commit(services, event_sink, completion).await {
        tracing::info!(%run_id, turn_id, reason, "a pending no-tool completion was not committed; the run stays running");
    }
}

async fn commit(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    completion: PendingExemptCompletion,
) -> Result<(), String> {
    // Hooks that could have run in the turn, or a lost record of an observed call.
    if codex_extension_api::host_hooks_configured()
        || crate::host_actions::unrecorded_actions_possible()
    {
        return Err("hooks were configured or an action record was lost".to_string());
    }
    let runtime = services
        .runtime()
        .await
        .map_err(|error| error.to_string())?;
    // The same project-knowledge fence the completion call held, so no knowledge write lands
    // between this check and the terminal commit.
    let mut fence = services
        .blackboard()
        .await
        .map_err(|error| error.to_string())?
        .acquire_completion_fence(FENCE_TIMEOUT)
        .await
        .map_err(|error| error.to_string())?;
    let result = async {
        if fence
            .agent_knowledge_changed_since(&completion.project_id, completion.run_created_at_ms)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("project knowledge changed during the run".to_string());
        }
        let ledger = runtime
            .acceptance_ledger(&completion.run_id)
            .await
            .map_err(|error| error.to_string())?;
        if !codex_stateful_runtime::no_tool_exempt(&ledger) {
            return Err("the run is no longer action-free".to_string());
        }
        let attempt = runtime
            .begin_verification(
                &completion.run_id,
                &completion.verification_owner,
                VERIFICATION_LEASE_MS,
            )
            .await
            .map_err(|error| error.to_string())?;
        let commit = AcceptanceCommit {
            ledger_revision: ledger.revision,
            workspace_generation: ledger.workspace_generation,
            artifacts: Default::default(),
            checkers: Default::default(),
            verification: VerificationClaim {
                owner: completion.verification_owner.clone(),
                attempt,
            },
            validated_obligation_sequence: completion.validated_obligation_sequence,
        };
        match runtime
            .complete_run_with_acceptance(
                &completion.run_id,
                completion.update,
                &commit,
                /*obligation*/ None,
            )
            .await
        {
            Ok((run, _)) => Ok(run),
            Err(error) => {
                if let Err(end_error) = runtime
                    .end_verification(&completion.run_id, &completion.verification_owner, attempt)
                    .await
                {
                    tracing::warn!(%end_error, "failed to end a no-tool completion's verification");
                }
                Err(error.to_string())
            }
        }
    }
    .await;
    if let Err(error) = fence.release().await {
        tracing::warn!("failed to release the Stateful completion fence: {error}");
    }
    let run = result?;
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::RunUpdated {
            project_id: run.value.project_id.clone(),
            run_id: run.id.to_string(),
            revision: run.revision,
        });
    }
    Ok(())
}
