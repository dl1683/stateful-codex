use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardEntryScope;
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
use super::promotion_name;
use super::relation::load_relations_for_entry;

#[derive(FromRow)]
struct RootEntryCounts {
    promoted: i64,
    candidates: i64,
}

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
        let counts = sqlx::query_as::<_, RootEntryCounts>(
            "SELECT
                COALESCE(SUM(CASE WHEN revision.root_promotion = 'promoted' THEN 1 ELSE 0 END), 0)
                    AS promoted,
                COALESCE(SUM(CASE WHEN revision.root_promotion = 'candidate' THEN 1 ELSE 0 END), 0)
                    AS candidates
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'",
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
            omitted_entries: u64::try_from(counts.promoted)
                .map_err(|_| BlackboardStoreError::CountOverflow)?
                .saturating_sub(u64::from(query.max_entries)),
            candidate_entries: u64::try_from(counts.candidates)
                .map_err(|_| BlackboardStoreError::CountOverflow)?,
        })
    }
}

async fn query_entry_ids(
    connection: &mut SqliteConnection,
    query: &BlackboardQuery,
    limit: i64,
) -> Result<Vec<String>, BlackboardStoreError> {
    let promotion_filter = query.root_promotion.map(promotion_name);
    let entry_scope = entry_scope_name(query.entry_scope);
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
                   AND CASE ? WHEN 'active' THEN revision.state = 'active'
                              WHEN 'historical' THEN revision.state <> 'active'
                              ELSE 1 END
                   AND (? IS NULL OR revision.root_promotion = ?)
                 ORDER BY bm25(blackboard_search), entry.id LIMIT ?",
            )
            .bind(&query.project_id)
            .bind(node_id.as_str())
            .bind(&query.project_id)
            .bind(expression)
            .bind(&query.project_id)
            .bind(entry_scope)
            .bind(promotion_filter)
            .bind(promotion_filter)
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
                   AND CASE ? WHEN 'active' THEN revision.state = 'active'
                              WHEN 'historical' THEN revision.state <> 'active'
                              ELSE 1 END
                   AND (? IS NULL OR revision.root_promotion = ?)
                 ORDER BY bm25(blackboard_search), entry.id LIMIT ?",
            )
            .bind(expression)
            .bind(&query.project_id)
            .bind(entry_scope)
            .bind(promotion_filter)
            .bind(promotion_filter)
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
               AND CASE ? WHEN 'active' THEN revision.state = 'active'
                          WHEN 'historical' THEN revision.state <> 'active'
                          ELSE 1 END
               AND (? IS NULL OR revision.root_promotion = ?)
             ORDER BY CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(node_id.as_str())
        .bind(&query.project_id)
        .bind(&query.project_id)
        .bind(entry_scope)
        .bind(promotion_filter)
        .bind(promotion_filter)
        .bind(limit)
        .fetch_all(connection)
        .await
        .map_err(Into::into),
        (None, None) => sqlx::query_scalar(
            "SELECT entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ?
               AND CASE ? WHEN 'active' THEN revision.state = 'active'
                          WHEN 'historical' THEN revision.state <> 'active'
                          ELSE 1 END
               AND (? IS NULL OR revision.root_promotion = ?)
             ORDER BY CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(entry_scope)
        .bind(promotion_filter)
        .bind(promotion_filter)
        .bind(limit)
        .fetch_all(connection)
        .await
        .map_err(Into::into),
    }
}

fn entry_scope_name(scope: BlackboardEntryScope) -> &'static str {
    match scope {
        BlackboardEntryScope::Active => "active",
        BlackboardEntryScope::Historical => "historical",
        BlackboardEntryScope::All => "all",
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
