use crate::metadata;
use crate::state_db::StateDbHandle;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use tracing::warn;

#[cfg(not(test))]
const BACKFILL_RETRY_INTERVAL: Duration = Duration::from_secs(10);
#[cfg(test)]
const BACKFILL_RETRY_INTERVAL: Duration = Duration::from_millis(10);

pub(super) async fn start(
    runtime: StateDbHandle,
    codex_home: PathBuf,
    default_model_provider_id: String,
    backfill_lease_seconds: Option<i64>,
) -> anyhow::Result<()> {
    let state = runtime.get_backfill_state().await.map_err(|err| {
        anyhow::anyhow!(
            "failed to read backfill state at {}: {err}",
            codex_home.display()
        )
    })?;
    if state.status == codex_state::BackfillStatus::Complete {
        return Ok(());
    }

    crate::state_db::emit_startup_warning(&format!(
        "state db historical rollout backfill is {} at {}; continuing in the background and using filesystem fallback until it completes",
        state.status.as_str(),
        codex_home.display()
    ));
    tokio::spawn(run(
        runtime,
        codex_home,
        default_model_provider_id,
        backfill_lease_seconds,
    ));
    Ok(())
}

async fn run(
    runtime: StateDbHandle,
    codex_home: PathBuf,
    default_model_provider_id: String,
    backfill_lease_seconds: Option<i64>,
) {
    loop {
        if let Some(backfill_lease_seconds) = backfill_lease_seconds {
            metadata::backfill_sessions_with_lease(
                runtime.as_ref(),
                codex_home.as_path(),
                default_model_provider_id.as_str(),
                backfill_lease_seconds,
            )
            .await;
        } else {
            metadata::backfill_sessions(
                runtime.as_ref(),
                codex_home.as_path(),
                default_model_provider_id.as_str(),
            )
            .await;
        }

        match runtime.get_backfill_state().await {
            Ok(state) if state.status == codex_state::BackfillStatus::Complete => {
                info!(
                    "state db historical rollout backfill completed at {}",
                    codex_home.display()
                );
                return;
            }
            Ok(state) => {
                info!(
                    "state db historical rollout backfill is {} at {}; retrying after {:?}",
                    state.status.as_str(),
                    codex_home.display(),
                    BACKFILL_RETRY_INTERVAL
                );
            }
            Err(err) => {
                warn!(
                    "state db historical rollout backfill stopped after its state became unreadable at {}: {err}",
                    codex_home.display()
                );
                return;
            }
        }
        tokio::time::sleep(BACKFILL_RETRY_INTERVAL).await;
    }
}

pub(super) async fn ready<'a>(
    context: Option<&'a codex_state::StateRuntime>,
    stage: &'static str,
) -> Option<&'a codex_state::StateRuntime> {
    let runtime = context?;
    match runtime.get_backfill_state().await {
        Ok(state) if state.status == codex_state::BackfillStatus::Complete => Some(runtime),
        Ok(_) => {
            codex_state::record_fallback(
                stage,
                "backfill_incomplete",
                /*telemetry_override*/ None,
            );
            None
        }
        Err(err) => {
            warn!("state db backfill readiness check failed during {stage}: {err}");
            codex_state::record_fallback(stage, "db_error", /*telemetry_override*/ None);
            None
        }
    }
}
