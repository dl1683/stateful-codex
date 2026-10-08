//! Independent immutable first observation and honest over-budget omissions.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::source::digest;
use super::source::seal_on;
use crate::SourceObservation;
use crate::SourceSeal;
use crate::capture_source::SourceOrigin;

impl BlackboardStore {
    /// Commits the original part independently before any semantic writer runs.
    /// Redelivery uses the original event/revision/part, never a transport request ID.
    pub async fn observe_source(
        &self,
        observation: SourceObservation,
        exact_text: &str,
    ) -> Result<SourceSeal, BlackboardStoreError> {
        observation.validate(exact_text)?;
        let source_id = digest(
            &serde_json::to_string(&(
                &observation.project_id,
                &observation.original_event_id,
                observation.source_revision,
                observation.part_index,
            ))
            .map_err(|_| BlackboardStoreError::InvalidSource)?,
        );
        let source_digest = digest(exact_text);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let omitted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_source_omissions WHERE project_id = ? AND original_event_id = ? AND source_revision = ?)").bind(&observation.project_id).bind(&observation.original_event_id).bind(observation.source_revision as i64).fetch_one(&mut *tx).await?;
        if omitted {
            return Err(BlackboardStoreError::InvalidSource);
        }
        if let Some(existing) = seal_on(&mut tx, &observation.project_id, &source_id).await? {
            if existing.observation != observation || existing.digest != source_digest {
                return Err(BlackboardStoreError::InvalidSource);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        let sequence = super::source_order::allocate_on(&mut tx, &observation.project_id).await?;
        let seal = SourceSeal {
            origin: SourceOrigin::LiveUserTurnText,
            original_utf8_length: exact_text.len() as u32,
            digest: source_digest.clone(),
            immutable_first_observation_sequence: sequence,
            recorded_at_ms: super::unix_timestamp_millis()?,
            capture_contract_version: "capture-c1-v3".to_string(),
            exact_source_locator: source_id.clone(),
            observation,
        };
        let metadata =
            serde_json::to_string(&seal).map_err(|_| BlackboardStoreError::InvalidSource)?;
        if metadata.len() > 8192 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        sqlx::query("INSERT INTO capture_sources(source_id, project_id, original_event_id, authoritative_thread_id, turn_id, binding_generation, source_revision, part_index, digest, metadata, observed_sequence, recorded_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&source_id).bind(&seal.observation.project_id).bind(&seal.observation.original_event_id)
            .bind(&seal.observation.authoritative_thread_id).bind(&seal.observation.turn_id).bind(seal.observation.binding_generation as i64)
            .bind(seal.observation.source_revision as i64).bind(i64::from(seal.observation.part_index)).bind(&source_digest)
            .bind(metadata).bind(sequence as i64).bind(seal.recorded_at_ms).execute(&mut *tx).await?;
        let mut start = 0;
        while start < exact_text.len() {
            let mut end = (start + 4096).min(exact_text.len());
            while !exact_text.is_char_boundary(end) {
                end -= 1;
            }
            let text = &exact_text[start..end];
            sqlx::query("INSERT INTO capture_source_chunks(source_id, start_byte, end_byte, exact_bytes, chunk_digest, search_text) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(&source_id).bind(start as i64).bind(end as i64).bind(text).bind(digest(text))
                .bind(crate::retirement_capture_words(text)).execute(&mut *tx).await?;
            super::source_projection::index_chunk(
                &mut tx,
                &seal.observation.project_id,
                &source_id,
                start as u32,
                text,
            )
            .await?;
            if end == exact_text.len() {
                break;
            }
            // Overlap is part of the original source, not a synthetic concatenation.
            start = end.saturating_sub(/*rhs*/ 256);
            while !exact_text.is_char_boundary(start) {
                start += 1;
            }
        }
        tx.commit().await?;
        Ok(seal)
    }

    /// Retains a native locator and honest omission reason when inspection is over budget.
    /// It cannot be used as an exact seal or as admission evidence.
    pub async fn record_source_omission(
        &self,
        observation: SourceObservation,
    ) -> Result<(), BlackboardStoreError> {
        observation.validate("omitted")?;
        if observation.complete_envelope || !observation.ordered_spans.is_empty() {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let metadata =
            serde_json::to_string(&observation).map_err(|_| BlackboardStoreError::InvalidSource)?;
        if metadata.len() > 8192 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let sealed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_sources WHERE project_id = ? AND original_event_id = ? AND source_revision = ?)").bind(&observation.project_id).bind(&observation.original_event_id).bind(observation.source_revision as i64).fetch_one(&mut *tx).await?;
        if sealed {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let existing: Option<Option<String>> = sqlx::query_scalar("SELECT CASE WHEN octet_length(metadata) <= 8192 THEN metadata END FROM capture_source_omissions WHERE project_id = ? AND original_event_id = ? AND source_revision = ?")
            .bind(&observation.project_id).bind(&observation.original_event_id).bind(observation.source_revision as i64).fetch_optional(&mut *tx).await?;
        if let Some(existing) = existing {
            if existing.as_deref() != Some(&metadata) {
                return Err(BlackboardStoreError::InvalidSource);
            }
        } else {
            let sequence =
                super::source_order::allocate_on(&mut tx, &observation.project_id).await?;
            sqlx::query("INSERT INTO capture_source_omissions(project_id, original_event_id, source_revision, metadata, observed_sequence, recorded_at_ms) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(&observation.project_id).bind(&observation.original_event_id).bind(observation.source_revision as i64).bind(metadata).bind(sequence as i64).bind(super::unix_timestamp_millis()?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
