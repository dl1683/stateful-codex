//! Typed temporal reads preserve storage clocks and explicit unknowns.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::context_bounds::context_of;
use super::load_entry;
use crate::BlackboardEntryId;
impl BlackboardStore {
    /// Explicit unknown source/event time for historical entries with no typed payload.
    /// Recording time retains its original storage meaning; it never becomes event time.
    pub async fn temporal_context(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<crate::TemporalContext, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let entry = load_entry(&mut tx, project_id, id)
            .await?
            .ok_or(BlackboardStoreError::InvalidSource)?;
        let context = context_of(&mut tx, project_id, id.as_str()).await?;
        let mut temporal = context
            .and_then(|context| context.payload)
            .map(|payload| {
                serde_json::from_str::<serde_json::Value>(&payload)
                    .map_err(|_| BlackboardStoreError::UnsupportedContext)
            })
            .transpose()?
            .and_then(|payload| payload.get("temporal").cloned())
            .map(|value| {
                serde_json::from_value::<crate::TemporalContext>(value)
                    .map_err(|_| BlackboardStoreError::UnsupportedContext)
            })
            .transpose()?
            .unwrap_or(crate::TemporalContext {
                version: 1,
                recorded_at_ms: entry.created_at_ms,
                source_time: crate::SourceTime::Unknown,
                event_time: crate::EventTime::Unknown,
                event_status: crate::EventStatus::Unknown,
            });
        temporal.recorded_at_ms = entry.created_at_ms;
        temporal.validate()?;
        tx.commit().await?;
        Ok(temporal)
    }
}
