use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulTurnMeasurement;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::StatefulTokenUsage;
use codex_stateful_runtime::StatefulTurnStatus;
use codex_stateful_runtime::StatefulTurnTerminalMeasurement;
use codex_stateful_runtime::TurnTrajectory;
use tokio::sync::OnceCell;
use tokio::sync::Semaphore;
use tokio::sync::oneshot;
use tokio_util::task::TaskTracker;

const MAX_PENDING_MEASUREMENTS: usize = 128;
const MEASUREMENT_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const MEASUREMENT_MERGE_TIMEOUT: Duration = Duration::from_secs(10);

/// Lazily shares the Stateful runtime store and ordered measurement writes.
#[derive(Clone)]
pub(crate) struct StatefulStoreHandle {
    sqlite: Option<SqliteConfig>,
    store: Arc<OnceCell<StatefulRunStore>>,
    expected_measurements: Arc<StdMutex<HashSet<MeasurementKey>>>,
    measurement_merge: Arc<StdMutex<MeasurementMergeState>>,
    measurement_gate: Arc<Semaphore>,
    measurement_tasks: TaskTracker,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct MeasurementKey {
    thread_id: String,
    turn_id: String,
}

struct PendingTrajectory {
    terminal: StatefulTurnTerminalMeasurement,
    completions: Vec<oneshot::Sender<()>>,
}

#[derive(Default)]
struct MeasurementMergeState {
    pending_trajectories: HashMap<MeasurementKey, PendingTrajectory>,
}

impl StatefulStoreHandle {
    pub(crate) fn new(sqlite: Option<SqliteConfig>) -> Self {
        Self {
            sqlite,
            store: Arc::new(OnceCell::new()),
            expected_measurements: Arc::new(StdMutex::new(HashSet::new())),
            measurement_merge: Arc::new(StdMutex::new(MeasurementMergeState::default())),
            measurement_gate: Arc::new(Semaphore::new(/*permits*/ 1)),
            measurement_tasks: TaskTracker::new(),
        }
    }

    pub(crate) async fn get(&self) -> Result<Option<&StatefulRunStore>, StatefulRunStoreError> {
        let Some(sqlite) = &self.sqlite else {
            return Ok(None);
        };
        self.store
            .get_or_try_init(|| StatefulRunStore::open(sqlite))
            .await
            .map(Some)
    }

    pub(crate) fn schedule_attribution(&self, measurement: NewStatefulTurnMeasurement) {
        if self.sqlite.is_none() {
            return;
        }
        let key = MeasurementKey {
            thread_id: measurement.thread_id.clone(),
            turn_id: measurement.turn_id.clone(),
        };
        {
            let mut expected = self
                .expected_measurements
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !expected.contains(&key) && expected.len() >= MAX_PENDING_MEASUREMENTS {
                tracing::warn!(
                    thread_id = %key.thread_id,
                    turn_id = %key.turn_id,
                    max_pending = MAX_PENDING_MEASUREMENTS,
                    "Stateful measurement queue is full; attribution was not scheduled"
                );
                return;
            }
            expected.insert(key.clone());
        }

        let handle = self.clone();
        drop(self.measurement_tasks.spawn(async move {
            if let Err(error) = handle.persist_attribution(&key, measurement).await {
                handle.forget_measurement(&key);
                tracing::warn!(
                    thread_id = %key.thread_id,
                    turn_id = %key.turn_id,
                    %error,
                    "failed to persist Stateful turn attribution"
                );
            }
        }));
    }

    pub(crate) async fn record_terminal_measurement(
        &self,
        thread_id: &str,
        turn_id: &str,
        status: StatefulTurnStatus,
        completed_at_ms: Option<i64>,
        value: &codex_app_server_protocol::TurnTrajectory,
        token_usage: Option<StatefulTokenUsage>,
    ) -> Result<(), StatefulRunStoreError> {
        let key = MeasurementKey {
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
        };
        if !self
            .expected_measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&key)
        {
            return Ok(());
        }
        let terminal = StatefulTurnTerminalMeasurement {
            status,
            completed_at_ms,
            trajectory: TurnTrajectory {
                completed_model_responses: value.completed_model_responses,
                compactions: value.compactions,
                model_tool_calls: value.model_tool_calls,
                model_shell_tool_calls: value.model_shell_tool_calls,
                model_function_tool_calls: value.model_function_tool_calls,
                model_custom_tool_calls: value.model_custom_tool_calls,
                model_tool_search_calls: value.model_tool_search_calls,
                model_web_search_calls: value.model_web_search_calls,
                model_image_generation_calls: value.model_image_generation_calls,
                tool_output_bytes: value.tool_output_bytes,
            },
            token_usage,
        };
        self.persist_terminal(&key, terminal).await
    }

    pub(crate) async fn drain_measurements(&self) {
        self.measurement_tasks.close();
        if tokio::time::timeout(MEASUREMENT_DRAIN_TIMEOUT, self.measurement_tasks.wait())
            .await
            .is_err()
        {
            tracing::warn!(
                timeout_seconds = MEASUREMENT_DRAIN_TIMEOUT.as_secs(),
                "timed out waiting for Stateful measurements to persist"
            );
        }
    }

    async fn persist_attribution(
        &self,
        key: &MeasurementKey,
        measurement: NewStatefulTurnMeasurement,
    ) -> Result<(), StatefulRunStoreError> {
        let Some(store) = self.get().await? else {
            return Ok(());
        };
        let _permit = Arc::clone(&self.measurement_gate)
            .acquire_owned()
            .await
            .map_err(|_| StatefulRunStoreError::MeasurementWriterClosed)?;
        let stored = store.record_turn_attribution(measurement).await?;
        if stored.trajectory.is_some() {
            self.mark_measurement_complete(key);
            return Ok(());
        }
        let pending = self
            .measurement_merge
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending_trajectories
            .remove(key);
        if let Some(pending) = pending {
            let result = store
                .record_turn_terminal_for_thread(&key.thread_id, &key.turn_id, pending.terminal)
                .await;
            for completion in pending.completions {
                let _ = completion.send(());
            }
            result?;
            self.mark_measurement_complete(key);
        }
        Ok(())
    }

    async fn persist_terminal(
        &self,
        key: &MeasurementKey,
        terminal: StatefulTurnTerminalMeasurement,
    ) -> Result<(), StatefulRunStoreError> {
        let Some(store) = self.get().await? else {
            return Ok(());
        };
        let permit = Arc::clone(&self.measurement_gate)
            .acquire_owned()
            .await
            .map_err(|_| StatefulRunStoreError::MeasurementWriterClosed)?;
        match store
            .record_turn_terminal_for_thread(&key.thread_id, &key.turn_id, terminal.clone())
            .await
        {
            Ok(_) => {
                self.mark_measurement_complete(key);
                Ok(())
            }
            Err(StatefulRunStoreError::MeasurementNotFound) => {
                let completion_rx = {
                    let mut merge = self
                        .measurement_merge
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(pending) = merge.pending_trajectories.get_mut(key) {
                        if pending.terminal != terminal {
                            Err(StatefulRunStoreError::MeasurementIdentityConflict)
                        } else {
                            let (completion_tx, completion_rx) = oneshot::channel();
                            pending.completions.push(completion_tx);
                            Ok(completion_rx)
                        }
                    } else if merge.pending_trajectories.len() >= MAX_PENDING_MEASUREMENTS {
                        Err(StatefulRunStoreError::MeasurementPendingCapacity)
                    } else {
                        let (completion_tx, completion_rx) = oneshot::channel();
                        merge.pending_trajectories.insert(
                            key.clone(),
                            PendingTrajectory {
                                terminal: terminal.clone(),
                                completions: vec![completion_tx],
                            },
                        );
                        Ok(completion_rx)
                    }
                }?;
                drop(permit);
                self.await_trajectory_merge(completion_rx).await?;
                store
                    .record_turn_terminal_for_thread(&key.thread_id, &key.turn_id, terminal)
                    .await?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn forget_measurement(&self, key: &MeasurementKey) {
        if let Some(pending) = self
            .measurement_merge
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending_trajectories
            .remove(key)
        {
            for completion in pending.completions {
                let _ = completion.send(());
            }
        }
        self.mark_measurement_complete(key);
    }

    async fn await_trajectory_merge(
        &self,
        completion_rx: oneshot::Receiver<()>,
    ) -> Result<(), StatefulRunStoreError> {
        tokio::time::timeout(MEASUREMENT_MERGE_TIMEOUT, completion_rx)
            .await
            .map_err(|_| StatefulRunStoreError::MeasurementMergeTimeout)?
            .map_err(|_| StatefulRunStoreError::MeasurementNotFound)
    }

    fn mark_measurement_complete(&self, key: &MeasurementKey) {
        self.expected_measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
    }
}

#[cfg(test)]
#[path = "stateful_store_tests.rs"]
mod tests;
