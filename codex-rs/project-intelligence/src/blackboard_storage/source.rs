//! Durable first observation and bounded revision-pinned exact recovery.
use super::BlackboardStore;
use super::BlackboardStoreError;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::SourceRangeRead;
use crate::SourceSeal;
use crate::SourceSpan;
use sha2::Digest;
use sha2::Sha256;
use sqlx::SqliteConnection;

pub(super) fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;

impl BlackboardStore {
    /// A model/automatic exact read always rechecks current range exclusions in its snapshot.
    /// Native archival access is a separate user surface, not a mode of this reader.
    pub async fn read_source_range(
        &self,
        project_id: &str,
        locator: &str,
        expected_digest: &str,
        start: u32,
        end: u32,
    ) -> Result<SourceRangeRead, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let result = read_on(&mut tx, project_id, locator, expected_digest, start, end).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Links bounded complete evidence to the current entry under the common writer lock.
    /// No new authority or admission transition is created by this host operation.
    pub async fn link_entry_source(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        entry_id: &BlackboardEntryId,
        expected_revision: u64,
        seal: &SourceSeal,
        spans: &[SourceSpan],
    ) -> Result<(), BlackboardStoreError> {
        let project_id = admission.project_id();
        if spans.is_empty()
            || spans.len() > 8
            || project_id != seal.observation.project_id
            || admission.thread_id() != seal.observation.authoritative_thread_id
            || admission.binding_generation() != seal.observation.binding_generation
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let entry = super::load_entry(&mut tx, project_id, entry_id)
            .await?
            .ok_or(BlackboardStoreError::InvalidSource)?;
        if entry.revision != expected_revision || entry.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let stored = seal_on(&mut tx, project_id, &seal.exact_source_locator)
            .await?
            .ok_or(BlackboardStoreError::InvalidSource)?;
        if &stored != seal {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM capture_entry_sources WHERE entry_id = ?")
                .bind(entry_id.as_str())
                .fetch_one(&mut *tx)
                .await?;
        if count + spans.len() as i64 > 192 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let mut previous = 0;
        let mut bytes = 0;
        for span in spans {
            if span.start_byte < previous || span.start_byte >= span.end_byte {
                return Err(BlackboardStoreError::InvalidSource);
            }
            previous = span.end_byte;
            bytes += span.end_byte - span.start_byte;
            if bytes > 16384 {
                return Err(BlackboardStoreError::InvalidSource);
            }
            validate_span(&mut tx, project_id, &seal.exact_source_locator, span).await?;
            sqlx::query("INSERT OR IGNORE INTO capture_entry_sources(entry_id, source_id, start_byte, end_byte, role) VALUES (?, ?, ?, ?, ?)")
                .bind(entry_id.as_str()).bind(&seal.exact_source_locator).bind(i64::from(span.start_byte)).bind(i64::from(span.end_byte))
                .bind(serde_json::to_string(&span.role).map_err(|_| BlackboardStoreError::InvalidSource)?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// A native summary can join text parts; a forgotten part excludes that turn fallback.
    /// Eligible independent original ranges remain separately searchable/readable.
    pub async fn source_turn_eligible(
        &self,
        project_id: &str,
        turn_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        if [project_id, turn_id].iter().any(|id| id.len() > 512) {
            return Ok(false);
        }
        let excluded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_sources AS source JOIN capture_source_exclusions AS excluded ON excluded.project_id = source.project_id AND excluded.digest = source.digest WHERE source.project_id = ? AND source.turn_id = ?)")
            .bind(project_id).bind(turn_id).fetch_one(&self.pool).await?;
        Ok(!excluded)
    }

    /// Guards retained conversation fallbacks. Unknown or oversized original parts fail closed.
    pub async fn source_text_eligible(
        &self,
        project_id: &str,
        text: &str,
    ) -> Result<bool, BlackboardStoreError> {
        if text.len() > 65536 {
            return Ok(false);
        }
        if text.is_empty() {
            return Ok(true);
        }
        let normalized = format!(" {} ", crate::retirement_capture_words(text));
        let excluded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_source_exclusions WHERE project_id = ? AND digest = ?) OR EXISTS(SELECT 1 FROM capture_identity_aliases WHERE project_id = ? AND retired = 1 AND retirement_words != '' AND instr(?, ' ' || retirement_words || ' ') > 0) OR EXISTS(SELECT 1 FROM capture_source_exclusions AS excluded JOIN capture_sources AS source ON source.project_id = excluded.project_id AND source.digest = excluded.digest JOIN capture_source_chunks AS chunk ON chunk.source_id = source.source_id AND chunk.start_byte < excluded.end_byte AND chunk.end_byte > excluded.start_byte WHERE excluded.project_id = ? AND (instr(CAST(? AS BLOB), substr(CAST(chunk.exact_bytes AS BLOB), MAX(excluded.start_byte, chunk.start_byte) - chunk.start_byte + 1, MIN(excluded.end_byte, chunk.end_byte) - MAX(excluded.start_byte, chunk.start_byte))) > 0 OR instr(substr(CAST(chunk.exact_bytes AS BLOB), MAX(excluded.start_byte, chunk.start_byte) - chunk.start_byte + 1, MIN(excluded.end_byte, chunk.end_byte) - MAX(excluded.start_byte, chunk.start_byte)), CAST(? AS BLOB)) > 0))")
            .bind(project_id).bind(digest(text)).bind(project_id).bind(normalized).bind(project_id).bind(text).bind(text).fetch_one(&self.pool).await?;
        Ok(!excluded)
    }
}

pub(super) async fn seal_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
) -> Result<Option<SourceSeal>, BlackboardStoreError> {
    if project_id.len() > 512 || locator.len() != 64 {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let metadata: Option<Option<String>> = sqlx::query_scalar("SELECT CASE WHEN octet_length(metadata) <= 8192 THEN metadata END FROM capture_sources WHERE project_id = ? AND source_id = ?")
        .bind(project_id).bind(locator).fetch_optional(connection).await?;
    metadata
        .map(|metadata| {
            serde_json::from_str(&metadata.ok_or(BlackboardStoreError::InvalidSource)?)
                .map_err(|_| BlackboardStoreError::InvalidSource)
        })
        .transpose()
}

pub(super) async fn eligible(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
    start: u32,
    end: u32,
) -> Result<bool, BlackboardStoreError> {
    let excluded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_sources AS source JOIN capture_source_exclusions AS exclusion ON exclusion.project_id = source.project_id AND exclusion.digest = source.digest WHERE source.project_id = ? AND source.source_id = ? AND exclusion.start_byte < ? AND exclusion.end_byte > ?) OR EXISTS(SELECT 1 FROM capture_source_chunks AS chunk JOIN capture_identity_aliases AS alias ON alias.project_id = ? AND alias.retired = 1 AND alias.retirement_words != '' WHERE chunk.source_id = ? AND chunk.start_byte < ? AND chunk.end_byte > ? AND instr(' ' || chunk.search_text || ' ', ' ' || alias.retirement_words || ' ') > 0)")
        .bind(project_id).bind(locator).bind(i64::from(end)).bind(i64::from(start))
        .bind(project_id).bind(locator).bind(i64::from(end)).bind(i64::from(start)).fetch_one(connection).await?;
    Ok(!excluded)
}

pub(super) async fn read_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
    expected_digest: &str,
    start: u32,
    end: u32,
) -> Result<SourceRangeRead, BlackboardStoreError> {
    let seal = seal_on(connection, project_id, locator)
        .await?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    if seal.digest != expected_digest
        || start >= end
        || end > seal.original_utf8_length
        || end - start > 4096
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    if !eligible(connection, project_id, locator, start, end).await? {
        return Err(BlackboardStoreError::SourceExcluded);
    }
    let chunks: Vec<(i64, i64, Option<String>, String)> = sqlx::query_as("SELECT start_byte, end_byte, CASE WHEN octet_length(exact_bytes) <= 4096 THEN exact_bytes END, chunk_digest FROM capture_source_chunks WHERE source_id = ? AND start_byte < ? AND end_byte > ? ORDER BY start_byte LIMIT 3")
        .bind(locator).bind(i64::from(end)).bind(i64::from(start)).fetch_all(connection).await?;
    let mut text = String::new();
    let mut offset = start;
    for (chunk_start, chunk_end, bytes, checksum) in chunks {
        let bytes = bytes.ok_or(BlackboardStoreError::InvalidSource)?;
        if digest(&bytes) != checksum
            || bytes.len() as i64 != chunk_end - chunk_start
            || chunk_start > i64::from(offset)
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let stop = end.min(chunk_end as u32);
        if stop > offset {
            let value = bytes
                .get(
                    (offset as i64 - chunk_start) as usize
                        ..(i64::from(stop) - chunk_start) as usize,
                )
                .ok_or(BlackboardStoreError::InvalidSource)?;
            text.push_str(value);
            offset = stop;
        }
    }
    if offset != end {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let mut result = SourceRangeRead {
        seal,
        start_byte: start,
        end_byte: end,
        exact_text: text,
        next_offset: None,
    };
    while serde_json::to_string(&result)
        .map_err(|_| BlackboardStoreError::InvalidSource)?
        .len()
        > 9000
    {
        let Some((position, _)) = result.exact_text.char_indices().next_back() else {
            return Err(BlackboardStoreError::InvalidSource);
        };
        result.exact_text.truncate(position);
        result.end_byte = start + position as u32;
        result.next_offset = Some(result.end_byte);
    }
    if result.exact_text.is_empty() {
        return Err(BlackboardStoreError::InvalidSource);
    }
    Ok(result)
}

pub(super) async fn retire_entry_sources(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &BlackboardEntryId,
) -> Result<(), BlackboardStoreError> {
    sqlx::query("INSERT OR IGNORE INTO capture_source_exclusions(project_id, digest, start_byte, end_byte, entry_id) SELECT source.project_id, source.digest, link.start_byte, link.end_byte, link.entry_id FROM capture_entry_sources AS link JOIN capture_sources AS source ON source.source_id = link.source_id WHERE source.project_id = ? AND link.entry_id = ?")
        .bind(project_id).bind(entry_id.as_str()).execute(connection).await?;
    Ok(())
}

pub(super) async fn validate_span(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
    span: &SourceSpan,
) -> Result<(), BlackboardStoreError> {
    if !eligible(
        connection,
        project_id,
        locator,
        span.start_byte,
        span.end_byte,
    )
    .await?
    {
        return Err(BlackboardStoreError::SourceExcluded);
    }
    for offset in [span.start_byte, span.end_byte] {
        let row: Option<(i64, Option<String>, String)> = sqlx::query_as("SELECT start_byte, CASE WHEN octet_length(exact_bytes) <= 4096 THEN exact_bytes END, chunk_digest FROM capture_source_chunks WHERE source_id = ? AND start_byte <= ? AND end_byte >= ? ORDER BY start_byte DESC LIMIT 1")
            .bind(locator).bind(i64::from(offset)).bind(i64::from(offset)).fetch_optional(&mut *connection).await?;
        let (start, text, checksum) = row.ok_or(BlackboardStoreError::InvalidSource)?;
        let text = text.ok_or(BlackboardStoreError::InvalidSource)?;
        if digest(&text) != checksum || !text.is_char_boundary((i64::from(offset) - start) as usize)
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
    }
    Ok(())
}
