//! Rebuildable lexical projection; immutable seals and exclusions are its authority.
use super::BlackboardStore;
use super::BlackboardStoreError;
use crate::SourceSearchPage;
use sqlx::SqliteConnection;

pub(super) async fn index_chunk(
    connection: &mut SqliteConnection,
    project_id: &str,
    source_id: &str,
    start: u32,
    text: &str,
) -> Result<(), BlackboardStoreError> {
    let terms: std::collections::BTreeSet<String> = crate::retirement_capture_words(text)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    for term in terms {
        sqlx::query("INSERT OR IGNORE INTO capture_source_terms(project_id, term, source_id, start_byte) VALUES (?, ?, ?, ?)")
            .bind(project_id).bind(term).bind(source_id).bind(i64::from(start)).execute(&mut *connection).await?;
    }
    Ok(())
}

impl BlackboardStore {
    /// Exact lexical alternatives, bounded to 50 results and 256 examined chunks.
    /// A caller's continuation addresses the last examined original chunk, never a summary.
    pub async fn search_source_ranges(
        &self,
        project_id: &str,
        query: &str,
        after: Option<&crate::SourceSearchCursor>,
    ) -> Result<SourceSearchPage, BlackboardStoreError> {
        self.search_source_ranges_with_budget(project_id, query, after, /*byte_budget*/ 9000)
            .await
    }

    /// Sizes evidence and its continuation inside the source/eligibility snapshot.
    pub async fn search_source_ranges_with_budget(
        &self,
        project_id: &str,
        query: &str,
        after: Option<&crate::SourceSearchCursor>,
        byte_budget: usize,
    ) -> Result<SourceSearchPage, BlackboardStoreError> {
        if query.is_empty() || query.len() > 1024 || project_id.len() > 512 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let terms: std::collections::BTreeSet<String> = crate::retirement_capture_words(query)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if terms.is_empty() || terms.len() > 24 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let mut tx = self.pool.begin().await?;
        let rebuilding: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_projection_coverage WHERE project_id = ? AND complete = 0)")
            .bind(project_id).fetch_one(&mut *tx).await?;
        if rebuilding {
            return Err(BlackboardStoreError::SourceIndexIncomplete);
        }
        let (source_watermark, eligibility_revision, index_generation): (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COALESCE(MAX(observed_sequence), 0) FROM capture_sources WHERE project_id = ?), (SELECT COALESCE(MAX(revision.rowid), 0) FROM blackboard_entry_revisions AS revision JOIN blackboard_entries AS entry ON entry.id = revision.entry_id WHERE entry.project_id = ?), (SELECT COALESCE(MAX(generation), 0) FROM capture_projection_coverage WHERE project_id = ?)")
            .bind(project_id).bind(project_id).bind(project_id).fetch_one(&mut *tx).await?;
        let query_digest = super::source::digest(query);
        if let Some(cursor) = after
            && (cursor.project_id != project_id
                || cursor.query_digest != query_digest
                || cursor.source_watermark != source_watermark
                || cursor.eligibility_revision != eligibility_revision
                || cursor.index_generation != index_generation
                || cursor.after_source.len() != 64)
        {
            return Err(BlackboardStoreError::SourceCursorDrift);
        }
        let (after_source, after_byte) = after.map_or(("", -1), |cursor| {
            (cursor.after_source.as_str(), i64::from(cursor.after_byte))
        });
        let mut sql = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT DISTINCT term.source_id, term.start_byte, chunk.end_byte, source.digest FROM capture_source_terms AS term JOIN capture_source_chunks AS chunk ON chunk.source_id = term.source_id AND chunk.start_byte = term.start_byte JOIN capture_sources AS source ON source.source_id = term.source_id WHERE term.project_id = ",
        );
        sql.push_bind(project_id).push(" AND term.term IN (");
        let mut separated = sql.separated(", ");
        for term in &terms {
            separated.push_bind(term);
        }
        sql.push(") AND (term.source_id, term.start_byte) > (")
            .push_bind(after_source)
            .push(", ")
            .push_bind(after_byte)
            // Reserve at most five units per candidate: metadata, retirement check,
            // and up to three overlapping exact chunks. One budget owns all reads.
            .push(") ORDER BY term.source_id, term.start_byte LIMIT 51");
        let rows: Vec<(String, i64, i64, String)> =
            sql.build_query_as().fetch_all(&mut *tx).await?;
        let mut budget = super::source::SourceBudget::default();
        budget.charge(rows.len() as u32)?;
        let mut page = SourceSearchPage {
            ranges: Vec::new(),
            after: None,
            examined: 0,
            complete: rows.len() < 51,
        };
        let total_rows = rows.len();
        for (index, (locator, start, end, digest)) in rows.into_iter().enumerate() {
            let start = start as u32;
            let result = super::source::read_with_budget(
                &mut tx,
                project_id,
                &locator,
                &digest,
                start,
                end as u32,
                &mut budget,
            )
            .await;
            page.examined = budget.examined;
            let range = match result {
                Ok(range) => Some(range),
                Err(BlackboardStoreError::SourceExcluded) => None,
                Err(error) => return Err(error),
            };
            if let Some(mut range) = range {
                // Enclosure is identified by the immutable seal; excerpt is explicitly a range.
                let mut offset = 0;
                let matched = range
                    .exact_text
                    .split_inclusive(char::is_whitespace)
                    .find_map(|piece| {
                        let start = offset;
                        offset += piece.len();
                        crate::retirement_capture_words(piece)
                            .split_whitespace()
                            .any(|word| terms.contains(word))
                            .then_some(start)
                    })
                    .unwrap_or(0);
                let mut excerpt_start = matched.saturating_sub(/*rhs*/ 80);
                while !range.exact_text.is_char_boundary(excerpt_start) {
                    excerpt_start -= 1;
                }
                let mut excerpt_end = (excerpt_start + 240).min(range.exact_text.len());
                while !range.exact_text.is_char_boundary(excerpt_end) {
                    excerpt_end -= 1;
                }
                range.exact_text = range.exact_text[excerpt_start..excerpt_end].to_string();
                range.start_byte = start + excerpt_start as u32;
                range.end_byte = start + excerpt_end as u32;
                range.next_offset =
                    (range.end_byte < range.seal.original_utf8_length).then_some(range.end_byte);
                let previous = page.after.clone();
                page.after = Some(crate::SourceSearchCursor {
                    project_id: project_id.to_string(),
                    query_digest: query_digest.clone(),
                    source_watermark,
                    eligibility_revision,
                    index_generation,
                    after_source: locator.clone(),
                    after_byte: start,
                });
                page.ranges.push(range);
                if page.ranges.len() > 50
                    || serde_json::to_string(&page)
                        .map_err(|_| BlackboardStoreError::InvalidSource)?
                        .len()
                        > byte_budget.min(9000)
                {
                    page.ranges.pop();
                    page.after = previous;
                    if page.ranges.is_empty() {
                        return Err(BlackboardStoreError::SourceBudgetInsufficient);
                    }
                    page.complete = false;
                    break;
                }
            }
            page.after = Some(crate::SourceSearchCursor {
                project_id: project_id.to_string(),
                query_digest: query_digest.clone(),
                source_watermark,
                eligibility_revision,
                index_generation,
                after_source: locator,
                after_byte: start,
            });
            if index + 1 == total_rows && total_rows < 51 {
                page.complete = true;
            }
        }
        if page.complete {
            page.after = None;
        }
        if serde_json::to_string(&page)
            .map_err(|_| BlackboardStoreError::InvalidSource)?
            .len()
            > byte_budget.min(9000)
        {
            return Err(BlackboardStoreError::SourceBudgetInsufficient);
        }
        tx.commit().await?;
        Ok(page)
    }

    /// Starts a projection rebuild without deleting original bytes or range exclusions.
    pub async fn begin_source_index_rebuild(
        &self,
        project_id: &str,
    ) -> Result<(), BlackboardStoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM capture_source_terms WHERE project_id = ?")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO capture_projection_coverage(project_id) VALUES (?) ON CONFLICT(project_id) DO UPDATE SET after_source = '', after_byte = -1, complete = 0, generation = generation + 1")
            .bind(project_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Rebuilds at most 256 chunks in four 64-row pages, with a durable resume cursor.
    pub async fn maintain_source_index(
        &self,
        project_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        let started = std::time::Instant::now();
        for _ in 0..4 {
            if started.elapsed() >= std::time::Duration::from_millis(/*millis*/ 1500) {
                return Ok(false);
            }
            let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let coverage: Option<(String, i64, bool)> = sqlx::query_as("SELECT after_source, after_byte, complete FROM capture_projection_coverage WHERE project_id = ?")
                .bind(project_id).fetch_optional(&mut *tx).await?;
            let Some((source, byte, complete)) = coverage else {
                return Ok(true);
            };
            if complete {
                return Ok(true);
            }
            let rows: Vec<(String, i64, Option<String>, String)> = sqlx::query_as("SELECT chunk.source_id, chunk.start_byte, CASE WHEN octet_length(chunk.exact_bytes) <= 4096 THEN chunk.exact_bytes END, chunk.chunk_digest FROM capture_source_chunks AS chunk JOIN capture_sources AS source ON source.source_id = chunk.source_id WHERE source.project_id = ? AND (chunk.source_id, chunk.start_byte) > (?, ?) ORDER BY chunk.source_id, chunk.start_byte LIMIT 64")
                .bind(project_id).bind(&source).bind(byte).fetch_all(&mut *tx).await?;
            for (locator, start, text, checksum) in &rows {
                let text = text.as_ref().ok_or(BlackboardStoreError::InvalidSource)?;
                if super::source::digest(text) != *checksum {
                    return Err(BlackboardStoreError::InvalidSource);
                }
                sqlx::query("UPDATE capture_source_chunks SET search_text = ? WHERE source_id = ? AND start_byte = ?").bind(crate::retirement_capture_words(text)).bind(locator).bind(start).execute(&mut *tx).await?;
                index_chunk(&mut tx, project_id, locator, *start as u32, text).await?;
            }
            let (source, byte) = rows
                .last()
                .map_or((source.as_str(), byte), |(source, byte, _, _)| {
                    (source.as_str(), *byte)
                });
            let complete = rows.len() < 64;
            sqlx::query("UPDATE capture_projection_coverage SET after_source = ?, after_byte = ?, complete = ? WHERE project_id = ?")
                .bind(source).bind(byte).bind(complete).bind(project_id).execute(&mut *tx).await?;
            tx.commit().await?;
            if complete {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
