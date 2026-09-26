use sqlx::FromRow;
use sqlx::QueryBuilder;
use sqlx::Sqlite;
use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardEntryScope;
use crate::BlackboardError;
use crate::BlackboardEvidenceDependentsQuery;
use crate::BlackboardEvidenceDependentsResult;
use crate::BlackboardEvidenceFreshness;
use crate::BlackboardHit;
use crate::BlackboardPremiseFreshness;
use crate::BlackboardQuery;
use crate::BlackboardQueryResult;
use crate::BlackboardRouteKnowledge;
use crate::BlackboardRouteKnowledgeQuery;
use crate::ContextMapEntryId;
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

#[derive(FromRow)]
struct StoredRouteKnowledge {
    context_map_entry_id: String,
    active_entries: i64,
    root_entries: i64,
}

impl BlackboardStore {
    pub async fn evidence_dependents(
        &self,
        query: BlackboardEvidenceDependentsQuery,
    ) -> Result<BlackboardEvidenceDependentsResult, BlackboardStoreError> {
        query.validate()?;
        let mut transaction = self.pool.begin().await?;
        let project_revision = sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
        )
        .bind(&query.project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or_default();
        let project_revision =
            u64::try_from(project_revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        if let Some(expected) = query.expected_project_revision
            && expected != project_revision
        {
            return Err(BlackboardStoreError::ProjectRevisionConflict {
                expected,
                actual: project_revision,
            });
        }
        let mut route_builder = QueryBuilder::<Sqlite>::new(
            "SELECT seed.id
             FROM context_map_entries AS seed
             JOIN hierarchy_nodes AS source
               ON source.id = seed.node_id AND source.project_id = seed.project_id
             WHERE seed.project_id = ",
        );
        route_builder.push_bind(&query.project_id);
        route_builder.push(" AND source.kind IN ('file', 'region') AND seed.id IN (");
        let mut separated = route_builder.separated(", ");
        for entry_id in &query.context_map_entry_ids {
            separated.push_bind(entry_id.as_str());
        }
        separated.push_unseparated(")");
        let resolved_routes = route_builder
            .build_query_scalar::<String>()
            .fetch_all(&mut *transaction)
            .await?
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        if let Some(missing) = query
            .context_map_entry_ids
            .iter()
            .find(|entry_id| !resolved_routes.contains(entry_id.as_str()))
        {
            return Err(BlackboardStoreError::EvidenceNotFound(missing.to_string()));
        }
        let limit = i64::from(query.max_results) + 1;
        let mut builder = QueryBuilder::<Sqlite>::new(
            "WITH RECURSIVE changed_sources AS (
               SELECT DISTINCT source.project_root, source.relative_path
               FROM context_map_entries AS seed
               JOIN hierarchy_nodes AS source
                 ON source.id = seed.node_id AND source.project_id = seed.project_id
               WHERE seed.project_id = ",
        );
        builder.push_bind(&query.project_id);
        builder.push(" AND seed.id IN (");
        let mut separated = builder.separated(", ");
        for entry_id in &query.context_map_entry_ids {
            separated.push_bind(entry_id.as_str());
        }
        separated.push_unseparated(
            ")
             ),
             affected_revisions(entry_id, revision) AS (
               SELECT DISTINCT link.entry_id, link.revision
               FROM blackboard_evidence_links AS link
               JOIN context_map_entries AS evidence ON evidence.id = link.context_map_entry_id
               JOIN hierarchy_nodes AS evidence_source
                 ON evidence_source.id = evidence.node_id
                AND evidence_source.project_id = evidence.project_id
               JOIN changed_sources AS changed
                 ON changed.project_root = evidence_source.project_root
                AND changed.relative_path = evidence_source.relative_path
               WHERE evidence.project_id = ",
        );
        builder.push_bind(&query.project_id);
        builder.push(
            " UNION
               SELECT owner.entry_id, owner.revision
               FROM blackboard_premise_links AS owner
               JOIN affected_revisions AS affected
                 ON affected.entry_id = owner.premise_entry_id
                AND affected.revision = owner.premise_revision
             )
             SELECT DISTINCT entry.id
             FROM affected_revisions AS affected
             JOIN blackboard_entries AS entry ON entry.id = affected.entry_id
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id
              AND revision.revision = entry.revision
              AND revision.revision = affected.revision
             WHERE entry.project_id = ",
        );
        builder.push_bind(&query.project_id);
        builder.push(" AND CASE ");
        builder.push_bind(entry_scope_name(query.entry_scope));
        builder.push(
            " WHEN 'active' THEN revision.state = 'active'
              WHEN 'historical' THEN revision.state <> 'active'
              ELSE 1 END",
        );
        if let Some(after_entry_id) = &query.after_entry_id {
            builder.push(" AND entry.id > ");
            builder.push_bind(after_entry_id.as_str());
        }
        builder.push(" ORDER BY entry.id LIMIT ");
        builder.push_bind(limit);
        let entry_ids = builder
            .build_query_scalar::<String>()
            .fetch_all(&mut *transaction)
            .await?;
        let truncated = entry_ids.len() > query.max_results as usize;
        let entry_ids = entry_ids.into_iter().take(query.max_results as usize);
        let mut data = Vec::with_capacity(query.max_results as usize);
        for raw_id in entry_ids {
            let id = BlackboardEntryId::parse(&raw_id)
                .map_err(|_| BlackboardStoreError::CorruptEntry(raw_id.clone()))?;
            let entry = load_entry(&mut transaction, &query.project_id, &id)
                .await?
                .ok_or_else(|| BlackboardStoreError::EntryNotFound(raw_id))?;
            let freshness = load_evidence_freshness(&mut transaction, &entry).await?;
            let premise_freshness = load_premise_freshness(&mut transaction, &entry).await?;
            data.push(
                BlackboardHit::new(entry, freshness).with_premise_freshness(premise_freshness),
            );
        }
        transaction.commit().await?;
        Ok(BlackboardEvidenceDependentsResult {
            project_revision,
            data,
            truncated,
        })
    }

    pub async fn route_knowledge(
        &self,
        query: BlackboardRouteKnowledgeQuery,
    ) -> Result<Vec<BlackboardRouteKnowledge>, BlackboardStoreError> {
        query.validate()?;
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT link.context_map_entry_id,
                    COUNT(DISTINCT entry.id) AS active_entries,
                    COUNT(DISTINCT CASE WHEN revision.root_promotion = 'promoted'
                        THEN entry.id END) AS root_entries
             FROM blackboard_evidence_links AS link
             JOIN blackboard_entries AS entry ON entry.id = link.entry_id
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id
              AND revision.revision = entry.revision
              AND revision.revision = link.revision
             WHERE entry.project_id = ",
        );
        builder.push_bind(&query.project_id);
        builder.push(" AND revision.state = 'active' AND link.context_map_entry_id IN (");
        let mut separated = builder.separated(", ");
        for entry_id in &query.context_map_entry_ids {
            separated.push_bind(entry_id.as_str());
        }
        separated.push_unseparated(") GROUP BY link.context_map_entry_id");
        let rows = builder
            .build_query_as::<StoredRouteKnowledge>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(BlackboardRouteKnowledge {
                    context_map_entry_id: ContextMapEntryId::parse(
                        row.context_map_entry_id.clone(),
                    )
                    .map_err(|_| {
                        BlackboardStoreError::CorruptEntry(row.context_map_entry_id.clone())
                    })?,
                    active_entries: u32::try_from(row.active_entries)
                        .map_err(|_| BlackboardStoreError::CountOverflow)?,
                    root_entries: u32::try_from(row.root_entries)
                        .map_err(|_| BlackboardStoreError::CountOverflow)?,
                })
            })
            .collect()
    }

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
        let mut transaction = self.pool.begin().await?;
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
        .fetch_one(&mut *transaction)
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
        .fetch_all(&mut *transaction)
        .await?;
        let mut data = Vec::with_capacity(entry_ids.len());
        for raw_id in entry_ids {
            data.push(load_hit(&mut transaction, &query.project_id, raw_id).await?);
        }
        let revision = sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
        )
        .bind(&query.project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or_default();
        transaction.commit().await?;
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
    let premise_freshness = load_premise_freshness(connection, &entry).await?;
    let relations = load_relations_for_entry(connection, project_id, &entry.id, 256).await?;
    Ok(BlackboardHit::new(entry, freshness)
        .with_premise_freshness(premise_freshness)
        .with_relations(relations))
}

#[derive(FromRow)]
struct EvidenceCounts {
    total: i64,
    stale: i64,
    unavailable: i64,
}

pub(super) async fn load_evidence_freshness(
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

#[derive(FromRow)]
struct PremiseCounts {
    total: i64,
    stale: i64,
    unavailable: i64,
}

pub(super) async fn load_premise_freshness(
    connection: &mut SqliteConnection,
    entry: &crate::BlackboardEntry,
) -> Result<BlackboardPremiseFreshness, BlackboardStoreError> {
    let counts = sqlx::query_as::<_, PremiseCounts>(
        "WITH RECURSIVE premise_tree(entry_id, revision) AS (
           SELECT premise_entry_id, premise_revision
           FROM blackboard_premise_links
           WHERE entry_id = ? AND revision = ?
           UNION
           SELECT link.premise_entry_id, link.premise_revision
           FROM blackboard_premise_links AS link
           JOIN premise_tree AS owner
             ON owner.entry_id = link.entry_id AND owner.revision = link.revision
         )
         SELECT COUNT(*) AS total,
                COALESCE(SUM(CASE WHEN current.id IS NULL
                    OR current.revision <> premise_tree.revision
                    OR revision.state <> 'active'
                    OR revision.verification NOT IN ('source_verified', 'user_confirmed')
                    OR (revision.verification = 'source_verified' AND EXISTS (
                        SELECT 1
                        FROM blackboard_evidence_links AS evidence_link
                        JOIN context_map_entries AS evidence
                          ON evidence.id = evidence_link.context_map_entry_id
                        JOIN hierarchy_nodes AS source ON source.id = evidence.node_id
                        WHERE evidence_link.entry_id = premise_tree.entry_id
                          AND evidence_link.revision = premise_tree.revision
                          AND (source.lifecycle = 'replaced'
                            OR evidence.source_fingerprint <> evidence_link.source_fingerprint
                            OR source.source_fingerprint <> evidence_link.source_fingerprint)
                    )) THEN 1 ELSE 0 END), 0) AS stale,
                COALESCE(SUM(CASE WHEN revision.verification = 'source_verified' AND EXISTS (
                    SELECT 1
                    FROM blackboard_evidence_links AS evidence_link
                    JOIN context_map_entries AS evidence
                      ON evidence.id = evidence_link.context_map_entry_id
                    JOIN hierarchy_nodes AS source ON source.id = evidence.node_id
                    WHERE evidence_link.entry_id = premise_tree.entry_id
                      AND evidence_link.revision = premise_tree.revision
                      AND source.lifecycle = 'missing'
                ) THEN 1 ELSE 0 END), 0) AS unavailable
         FROM premise_tree
         LEFT JOIN blackboard_entries AS current ON current.id = premise_tree.entry_id
         LEFT JOIN blackboard_entry_revisions AS revision
           ON revision.entry_id = premise_tree.entry_id
          AND revision.revision = premise_tree.revision",
    )
    .bind(entry.id.as_str())
    .bind(i64::try_from(entry.revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?)
    .fetch_one(connection)
    .await?;
    Ok(if counts.total == 0 {
        BlackboardPremiseFreshness::NotApplicable
    } else if counts.unavailable > 0 {
        BlackboardPremiseFreshness::SourceUnavailable
    } else if counts.stale > 0 {
        BlackboardPremiseFreshness::Stale
    } else {
        BlackboardPremiseFreshness::Current
    })
}
