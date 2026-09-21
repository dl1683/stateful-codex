use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardError;
use crate::BlackboardEvidenceFreshness;
use crate::BlackboardHit;
use crate::BlackboardQuery;
use crate::BlackboardQueryResult;
use crate::RootBlackboardProjection;
use crate::RootBlackboardQuery;
use crate::search::literal_prefix_expression;
use crate::storage::load_node;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;
use super::relation::load_relations_for_entry;

impl BlackboardStore {
    pub async fn query(
        &self,
        query: BlackboardQuery,
    ) -> Result<BlackboardQueryResult, BlackboardStoreError> {
        query.validate()?;
        let mut connection = self.pool.acquire().await?;
        if let Some(node_id) = query.within_node.as_ref()
            && load_node(&mut connection, &query.project_id, node_id)
                .await?
                .is_none()
        {
            return Err(BlackboardStoreError::NodeNotFound(node_id.to_string()));
        }
        let limit = i64::from(query.max_results) + 1;
        let entry_ids = query_entry_ids(&mut connection, &query, limit).await?;
        let truncated = entry_ids.len() > query.max_results as usize;
        let entry_ids = entry_ids.into_iter().take(query.max_results as usize);
        let mut data = Vec::with_capacity(query.max_results as usize);
        for raw_id in entry_ids {
            data.push(load_hit(&mut connection, &query.project_id, raw_id).await?);
        }
        Ok(BlackboardQueryResult { data, truncated })
    }

    pub async fn root_projection(
        &self,
        query: RootBlackboardQuery,
    ) -> Result<RootBlackboardProjection, BlackboardStoreError> {
        query.validate()?;
        let mut connection = self.pool.acquire().await?;
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
               AND revision.root_promotion = 'promoted'",
        )
        .bind(&query.project_id)
        .fetch_one(&mut *connection)
        .await?;
        let entry_ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
               AND revision.root_promotion = 'promoted'
             ORDER BY CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id
             LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(i64::from(query.max_entries))
        .fetch_all(&mut *connection)
        .await?;
        let mut data = Vec::with_capacity(entry_ids.len());
        for raw_id in entry_ids {
            data.push(load_hit(&mut connection, &query.project_id, raw_id).await?);
        }
        let revision = sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
        )
        .bind(&query.project_id)
        .fetch_optional(&mut *connection)
        .await?
        .unwrap_or_default();
        Ok(RootBlackboardProjection {
            project_id: query.project_id,
            revision: u64::try_from(revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            data,
            omitted_entries: u64::try_from(total)
                .map_err(|_| BlackboardStoreError::CountOverflow)?
                .saturating_sub(u64::from(query.max_entries)),
        })
    }
}

async fn query_entry_ids(
    connection: &mut SqliteConnection,
    query: &BlackboardQuery,
    limit: i64,
) -> Result<Vec<String>, BlackboardStoreError> {
    match (&query.text, &query.within_node) {
        (Some(text), Some(node_id)) => {
            let expression =
                literal_prefix_expression(text).ok_or(BlackboardError::NoSearchTerms)?;
            sqlx::query_scalar(
                "WITH RECURSIVE scoped_nodes(id) AS (
                    SELECT id FROM hierarchy_nodes WHERE project_id = ? AND id = ?
                    UNION ALL
                    SELECT child.id FROM hierarchy_nodes AS child
                    JOIN scoped_nodes AS parent ON child.parent_id = parent.id
                    WHERE child.project_id = ?
                 )
                 SELECT entry.id FROM blackboard_search
                 JOIN blackboard_entries AS entry ON entry.id = blackboard_search.entry_id
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE blackboard_search MATCH ? AND entry.project_id = ?
                   AND entry.node_id IN (SELECT id FROM scoped_nodes)
                   AND revision.state = 'active'
                 ORDER BY bm25(blackboard_search), entry.id LIMIT ?",
            )
            .bind(&query.project_id)
            .bind(node_id.as_str())
            .bind(&query.project_id)
            .bind(expression)
            .bind(&query.project_id)
            .bind(limit)
            .fetch_all(connection)
            .await
            .map_err(Into::into)
        }
        (Some(text), None) => {
            let expression =
                literal_prefix_expression(text).ok_or(BlackboardError::NoSearchTerms)?;
            sqlx::query_scalar(
                "SELECT entry.id FROM blackboard_search
                 JOIN blackboard_entries AS entry ON entry.id = blackboard_search.entry_id
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE blackboard_search MATCH ? AND entry.project_id = ?
                   AND revision.state = 'active'
                 ORDER BY bm25(blackboard_search), entry.id LIMIT ?",
            )
            .bind(expression)
            .bind(&query.project_id)
            .bind(limit)
            .fetch_all(connection)
            .await
            .map_err(Into::into)
        }
        (None, Some(node_id)) => sqlx::query_scalar(
            "WITH RECURSIVE scoped_nodes(id) AS (
                SELECT id FROM hierarchy_nodes WHERE project_id = ? AND id = ?
                UNION ALL
                SELECT child.id FROM hierarchy_nodes AS child
                JOIN scoped_nodes AS parent ON child.parent_id = parent.id
                WHERE child.project_id = ?
             )
             SELECT entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND entry.node_id IN (SELECT id FROM scoped_nodes)
               AND revision.state = 'active'
             ORDER BY CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(node_id.as_str())
        .bind(&query.project_id)
        .bind(&query.project_id)
        .bind(limit)
        .fetch_all(connection)
        .await
        .map_err(Into::into),
        (None, None) => sqlx::query_scalar(
            "SELECT entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
             ORDER BY CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(limit)
        .fetch_all(connection)
        .await
        .map_err(Into::into),
    }
}

async fn load_hit(
    connection: &mut SqliteConnection,
    project_id: &str,
    raw_id: String,
) -> Result<BlackboardHit, BlackboardStoreError> {
    let id = BlackboardEntryId::parse(&raw_id)
        .map_err(|_| BlackboardStoreError::CorruptEntry(raw_id.clone()))?;
    let entry = load_entry(connection, project_id, &id)
        .await?
        .ok_or_else(|| BlackboardStoreError::EntryNotFound(raw_id))?;
    let freshness = load_evidence_freshness(connection, &entry).await?;
    let relations = load_relations_for_entry(connection, project_id, &entry.id, 256).await?;
    Ok(BlackboardHit::new(entry, freshness).with_relations(relations))
}

#[derive(FromRow)]
struct EvidenceCounts {
    total: i64,
    stale: i64,
    unavailable: i64,
}

async fn load_evidence_freshness(
    connection: &mut SqliteConnection,
    entry: &crate::BlackboardEntry,
) -> Result<BlackboardEvidenceFreshness, BlackboardStoreError> {
    let counts = sqlx::query_as::<_, EvidenceCounts>(
        "SELECT COUNT(*) AS total,
                COALESCE(SUM(CASE WHEN source.lifecycle = 'missing' THEN 1 ELSE 0 END), 0)
                    AS unavailable,
                COALESCE(SUM(CASE WHEN source.lifecycle = 'replaced'
                    OR evidence.source_fingerprint <> link.source_fingerprint
                    OR source.source_fingerprint <> link.source_fingerprint
                    THEN 1 ELSE 0 END), 0) AS stale
         FROM blackboard_evidence_links AS link
         JOIN context_map_entries AS evidence ON evidence.id = link.context_map_entry_id
         JOIN hierarchy_nodes AS source ON source.id = evidence.node_id
         WHERE link.entry_id = ? AND link.revision = ?",
    )
    .bind(entry.id.as_str())
    .bind(i64::try_from(entry.revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?)
    .fetch_one(connection)
    .await?;
    Ok(if counts.total == 0 {
        BlackboardEvidenceFreshness::NotApplicable
    } else if counts.unavailable > 0 {
        BlackboardEvidenceFreshness::SourceUnavailable
    } else if counts.stale > 0 {
        BlackboardEvidenceFreshness::Stale
    } else {
        BlackboardEvidenceFreshness::Current
    })
}
