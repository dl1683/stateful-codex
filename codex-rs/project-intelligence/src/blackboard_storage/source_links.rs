//! Bounded exact evidence discovery for consumers of current source-backed entries.
use super::BlackboardStore;
use super::BlackboardStoreError;
use crate::BlackboardEntryId;
use crate::SourceLink;
use crate::SourceLinkCursor;
use crate::SourceLinkPage;
use crate::SourceSpan;

impl BlackboardStore {
    /// Returns eight separately labelled ranges, with at most nine bounded lookahead rows.
    /// Native archival history remains independent from automatic source eligibility.
    pub async fn entry_source_links(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
        after: Option<&SourceLinkCursor>,
    ) -> Result<SourceLinkPage, BlackboardStoreError> {
        if project_id.len() > 512 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let mut tx = self.pool.begin().await?;
        if !super::identity::entry_source_eligible_on(&mut tx, project_id, id).await? {
            return Err(BlackboardStoreError::SourceExcluded);
        }
        let (revision, count): (i64, i64) = sqlx::query_as("SELECT revision, (SELECT COUNT(*) FROM capture_entry_sources WHERE entry_id = ?) FROM blackboard_entries WHERE project_id = ? AND id = ?")
            .bind(id.as_str()).bind(project_id).bind(id.as_str()).fetch_one(&mut *tx).await?;
        if let Some(cursor) = after
            && (cursor.project_id != project_id
                || cursor.entry_id != id.as_str()
                || cursor.entry_revision != revision as u64
                || cursor.link_count != count
                || cursor.last.locator.len() != 64)
        {
            return Err(BlackboardStoreError::SourceCursorDrift);
        }
        let last = after.map(|cursor| &cursor.last);
        let role = last
            .map(|last| serde_json::to_string(&last.span.role))
            .transpose()
            .map_err(|_| BlackboardStoreError::InvalidSource)?
            .unwrap_or_default();
        let rows: Vec<(String, String, i64, i64, i64, Option<String>)> = sqlx::query_as("SELECT link.source_id, source.digest, source.source_revision, link.start_byte, link.end_byte, CASE WHEN octet_length(link.role) <= 32 THEN link.role END FROM capture_entry_sources AS link JOIN capture_sources AS source ON source.source_id = link.source_id WHERE source.project_id = ? AND link.entry_id = ? AND (link.source_id, link.start_byte, link.end_byte, link.role) > (?, ?, ?, ?) ORDER BY link.source_id, link.start_byte, link.end_byte, link.role LIMIT 9")
            .bind(project_id).bind(id.as_str()).bind(last.map_or("", |last| last.locator.as_str())).bind(last.map_or(/*default*/ -1, |last| i64::from(last.span.start_byte))).bind(last.map_or(/*default*/ -1, |last| i64::from(last.span.end_byte))).bind(role).fetch_all(&mut *tx).await?;
        let complete = rows.len() <= 8;
        let mut links = Vec::new();
        for (locator, digest, source_revision, start, end, role) in
            rows.into_iter().take(/*n*/ 8)
        {
            let span = SourceSpan {
                start_byte: u32::try_from(start)
                    .map_err(|_| BlackboardStoreError::InvalidSource)?,
                end_byte: u32::try_from(end).map_err(|_| BlackboardStoreError::InvalidSource)?,
                role: serde_json::from_str(&role.ok_or(BlackboardStoreError::InvalidSource)?)
                    .map_err(|_| BlackboardStoreError::InvalidSource)?,
            };
            super::source::validate_span(&mut tx, project_id, &locator, &span).await?;
            links.push(SourceLink {
                locator,
                digest,
                source_revision: source_revision as u64,
                span,
            });
        }
        let after = if complete {
            None
        } else {
            Some(SourceLinkCursor {
                project_id: project_id.to_string(),
                entry_id: id.to_string(),
                entry_revision: revision as u64,
                link_count: count,
                last: links
                    .last()
                    .ok_or(BlackboardStoreError::InvalidSource)?
                    .clone(),
            })
        };
        let page = SourceLinkPage {
            links,
            after,
            complete,
        };
        if serde_json::to_string(&page)
            .map_err(|_| BlackboardStoreError::InvalidSource)?
            .len()
            > 9000
            || page.after.as_ref().is_some_and(|cursor| {
                serde_json::to_string(cursor)
                    .map_or(/*default*/ true, |text| text.len() > 1024)
            })
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        tx.commit().await?;
        Ok(page)
    }
}
