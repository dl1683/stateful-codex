//! Bounded root projection and historical quarantine counts from one read snapshot.
//! Existing SQL eligibility runs before LIMIT; historical bindings never admit scoped words.

use sqlx::FromRow;

use crate::RootBlackboardProjection;
use crate::RootBlackboardQuery;
use crate::ThreadScopes;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::query::load_hit;
use super::scopes::LEGACY_LIMITED_RULE;

// Apply authority/category eligibility before either counts or LIMIT, using the latest
// meaning visible at this entry revision. Legacy user instructions remain source-backed.
const ROOT_ELIGIBILITY: &str = "
    AND NOT EXISTS (SELECT 1 FROM knowledge_context AS meaning
        WHERE meaning.entry_id = entry.id AND meaning.revision = (
            SELECT MAX(latest.revision) FROM knowledge_context AS latest
            WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
        AND CASE WHEN octet_length(meaning.category) > 32 THEN 1
                 ELSE meaning.category = 'attributed_context' END)
    AND (revision.kind != 'instruction' OR (revision.provenance_kind = 'user'
        AND NOT EXISTS (SELECT 1 FROM knowledge_context AS meaning
            WHERE meaning.entry_id = entry.id AND meaning.revision = (
                SELECT MAX(latest.revision) FROM knowledge_context AS latest
                WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
            AND CASE WHEN octet_length(meaning.category) > 32
                       OR octet_length(meaning.authority) > 32 THEN 1
                     ELSE meaning.category != 'rule' OR meaning.authority != 'human_direct' END)))";

// Root never applies historical scoped entries. Check presence without loading their IDs
// or consulting bindings that cannot grant application here.
const UNSCOPED_CONTEXT: &str = "
    AND NOT EXISTS (SELECT 1 FROM knowledge_context AS scoped
        WHERE scoped.entry_id = entry.id AND scoped.revision = (
            SELECT MAX(latest.revision) FROM knowledge_context AS latest
            WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
        AND scoped.scope_id IS NOT NULL)";

#[derive(FromRow)]
struct RootEntryCounts {
    promoted: i64,
    candidates: i64,
}

impl BlackboardStore {
    pub async fn root_projection(
        &self,
        query: RootBlackboardQuery,
    ) -> Result<RootBlackboardProjection, BlackboardStoreError> {
        Ok(self.root_projection_in(query).await?.0)
    }

    /// The bounded root with historical scope quarantine counts. Historical bindings never
    /// grant application; every scoped entry is held back before LIMIT.
    pub async fn root_projection_for_thread(
        &self,
        query: RootBlackboardQuery,
        _thread_id: &str,
    ) -> Result<(RootBlackboardProjection, ThreadScopes), BlackboardStoreError> {
        let (projection, scopes) = self.root_projection_in(query).await?;
        Ok((projection, scopes))
    }

    async fn root_projection_in(
        &self,
        query: RootBlackboardQuery,
    ) -> Result<(RootBlackboardProjection, ThreadScopes), BlackboardStoreError> {
        query.validate()?;
        let mut transaction = self.pool.begin().await?;
        if !super::identity::coverage_available(&mut transaction, &query.project_id).await? {
            return Err(BlackboardStoreError::IdentityCoverageIncomplete);
        }
        let eligibility = super::identity::ENTRY_SOURCE_ELIGIBILITY;
        let scoped = format!("{UNSCOPED_CONTEXT} AND NOT ({LEGACY_LIMITED_RULE})");
        let outside = format!("{ROOT_ELIGIBILITY}{scoped}{eligibility}");
        let mut counts = sqlx::query_as::<_, RootEntryCounts>(
            sqlx::AssertSqlSafe(format!("SELECT
                COALESCE(SUM(CASE WHEN revision.root_promotion = 'promoted' THEN 1 ELSE 0 END), 0)
                    AS promoted,
                COALESCE(SUM(CASE WHEN revision.root_promotion = 'candidate' THEN 1 ELSE 0 END), 0)
                    AS candidates
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'{ROOT_ELIGIBILITY}{eligibility} AND NOT ({LEGACY_LIMITED_RULE})")),
        )
        .bind(&query.project_id)
        .fetch_one(&mut *transaction)
        .await?;
        let scopes = {
            let applicable = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*)
                 FROM blackboard_entries AS entry
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE entry.project_id = ? AND revision.state = 'active'
                   AND revision.root_promotion = 'promoted'{outside}"
            )))
            .bind(&query.project_id)
            .fetch_one(&mut *transaction)
            .await?;
            let legacy = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*)
                 FROM blackboard_entries AS entry
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE entry.project_id = ? AND revision.state = 'active'
                   AND revision.root_promotion = 'promoted' AND {LEGACY_LIMITED_RULE}"
            )))
            .bind(&query.project_id)
            .fetch_one(&mut *transaction)
            .await?;
            let count =
                |value: i64| u64::try_from(value).map_err(|_| BlackboardStoreError::CountOverflow);
            let scopes = ThreadScopes {
                scoped_held_back: count(counts.promoted.saturating_sub(applicable))?,
                legacy_held_back: count(legacy)?,
            };
            counts.promoted = applicable;
            scopes
        };
        let list = format!(
            "SELECT entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
               AND revision.root_promotion = 'promoted'{outside}
             ORDER BY CASE WHEN revision.kind = 'instruction'
                     AND revision.provenance_kind = 'user' THEN 0
                 WHEN revision.provenance_kind = 'user' THEN 1 ELSE 2 END,
                 CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                     THEN (SELECT context.source_sequence FROM knowledge_context AS context
                           WHERE context.entry_id = entry.id AND context.revision <= entry.revision
                           ORDER BY context.revision DESC LIMIT 1) END,
                 CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                     THEN (SELECT context.unit_ordinal FROM knowledge_context AS context
                           WHERE context.entry_id = entry.id AND context.revision <= entry.revision
                           ORDER BY context.revision DESC LIMIT 1) END,
                 CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                     THEN entry.created_at_ms END,
                 CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                     THEN entry.rowid END,
                 CASE revision.importance
                 WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                 WHEN 'normal' THEN 2 ELSE 3 END, entry.id
             LIMIT ?"
        );
        let entry_ids =
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(list)).bind(&query.project_id);
        let entry_ids = entry_ids
            .bind(i64::from(query.max_entries))
            .fetch_all(&mut *transaction)
            .await?;
        let mut data = Vec::with_capacity(entry_ids.len());
        let mut contexts = std::collections::HashMap::new();
        for raw_id in entry_ids {
            if let Some(context) =
                super::knowledge::context_of(&mut transaction, &query.project_id, &raw_id).await?
            {
                contexts.insert(raw_id.clone(), context);
            }
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
        let projection = RootBlackboardProjection {
            project_id: query.project_id,
            revision: u64::try_from(revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            data,
            contexts,
            omitted_entries: u64::try_from(counts.promoted)
                .map_err(|_| BlackboardStoreError::CountOverflow)?
                .saturating_sub(u64::from(query.max_entries)),
            candidate_entries: u64::try_from(counts.candidates)
                .map_err(|_| BlackboardStoreError::CountOverflow)?,
        };
        Ok((projection, scopes))
    }
}
