//! The root projection: the bounded set of promoted entries a packet shows, optionally as it
//! applies in one thread. Applicability is decided in SQL before the bound, in one read
//! transaction with the thread's binding and the project's investigations, so a projection,
//! its counts and the investigations that explain it always come from one snapshot.

use sqlx::FromRow;

use crate::RootBlackboardProjection;
use crate::RootBlackboardQuery;
use crate::ThreadScopes;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::query::load_hit;
use super::scopes::IN_OTHER_OPEN_SCOPE;
use super::scopes::LEGACY_LIMITED_RULE;
use super::scopes::OUTSIDE_THREAD_SCOPE;
use super::scopes::thread_scopes_on;

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
        Ok(self.root_projection_in(query, /*thread_id*/ None).await?.0)
    }

    /// The root projection as it applies in `thread_id`: an entry limited to an investigation
    /// is left out unless the thread continues that investigation while it is open. The
    /// projection, the thread's binding and the project's investigations come from one
    /// snapshot, so the aliases a packet assigns never mix two states.
    pub async fn root_projection_for_thread(
        &self,
        query: RootBlackboardQuery,
        thread_id: &str,
    ) -> Result<(RootBlackboardProjection, ThreadScopes), BlackboardStoreError> {
        let (projection, scopes) = self.root_projection_in(query, Some(thread_id)).await?;
        Ok((projection, scopes.unwrap_or_default()))
    }

    async fn root_projection_in(
        &self,
        query: RootBlackboardQuery,
        thread_id: Option<&str>,
    ) -> Result<(RootBlackboardProjection, Option<ThreadScopes>), BlackboardStoreError> {
        query.validate()?;
        let mut transaction = self.pool.begin().await?;
        let outside = if thread_id.is_some() {
            format!("{OUTSIDE_THREAD_SCOPE} AND NOT ({LEGACY_LIMITED_RULE})")
        } else {
            String::new()
        };
        let mut counts = sqlx::query_as::<_, RootEntryCounts>(
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
        let mut scopes = None;
        if let Some(thread_id) = thread_id {
            let applicable = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*)
                 FROM blackboard_entries AS entry
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE entry.project_id = ? AND revision.state = 'active'
                   AND revision.root_promotion = 'promoted'{outside}"
            )))
            .bind(&query.project_id)
            .bind(thread_id)
            .fetch_one(&mut *transaction)
            .await?;
            let elsewhere = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*)
                 FROM blackboard_entries AS entry
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE entry.project_id = ? AND revision.state = 'active'
                   AND revision.root_promotion = 'promoted'{IN_OTHER_OPEN_SCOPE}"
            )))
            .bind(&query.project_id)
            .bind(thread_id)
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
            let (bound, all) =
                thread_scopes_on(&mut transaction, &query.project_id, thread_id).await?;
            let count =
                |value: i64| u64::try_from(value).map_err(|_| BlackboardStoreError::CountOverflow);
            scopes = Some(ThreadScopes {
                bound,
                scopes: all,
                scoped_elsewhere: count(elsewhere)?,
                legacy_held_back: count(legacy)?,
            });
            counts.promoted = applicable;
        }
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
                           WHERE context.entry_id = entry.id
                           ORDER BY context.revision DESC LIMIT 1) END,
                 CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                     THEN (SELECT context.unit_ordinal FROM knowledge_context AS context
                           WHERE context.entry_id = entry.id
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
        let mut entry_ids =
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(list)).bind(&query.project_id);
        if let Some(thread_id) = thread_id {
            entry_ids = entry_ids.bind(thread_id);
        }
        let entry_ids = entry_ids
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
        let projection = RootBlackboardProjection {
            project_id: query.project_id,
            revision: u64::try_from(revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            data,
            omitted_entries: u64::try_from(counts.promoted)
                .map_err(|_| BlackboardStoreError::CountOverflow)?
                .saturating_sub(u64::from(query.max_entries)),
            candidate_entries: u64::try_from(counts.candidates)
                .map_err(|_| BlackboardStoreError::CountOverflow)?,
        };
        Ok((projection, scopes))
    }
}
