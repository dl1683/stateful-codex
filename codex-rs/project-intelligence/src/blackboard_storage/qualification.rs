//! Qualifying remembered assertions against committed source changes. Assertions are never
//! deleted or rewritten: they are marked as needing a check. Jobs persist per project root so
//! an unfinished scan resumes where it stopped, and a new target replaces it explicitly.

use sqlx::FromRow;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardVerification;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::KnowledgeValidity;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;

/// What an invalidation did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidationOutcome {
    /// A new revision records the entry as no longer plainly current.
    Invalidated,
    /// The entry already had this validity (or was already obsolete); nothing changed.
    Unchanged,
}

/// Where a qualification job stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualificationState {
    /// Changed paths are known; entries up to the watermark are being examined.
    Scanning,
    /// Every eligible entry up to the watermark was examined against the target.
    Complete,
    /// The changes could not be listed (too many, timed out, discontinuous history); nothing
    /// recorded before the target can be treated as checked against it.
    Blocked,
}

/// The qualification of one project root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualificationJob {
    pub project_id: String,
    pub project_root: String,
    /// The last commit every eligible entry was qualified against.
    pub qualified_head: String,
    pub target_head: String,
    pub generation: u64,
    pub state: QualificationState,
    /// The changed paths, encoded by the caller.
    pub manifest: Option<String>,
    pub reason: Option<String>,
    /// Entries inserted at or before this sequence predate the target.
    pub watermark: i64,
    /// Entries at or before this sequence were examined.
    pub cursor: i64,
}

/// A source location an entry depends on: its own node and the nodes of its evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDependency {
    pub project_root: String,
    pub relative_path: String,
    /// A directory: every path below it is a dependency.
    pub directory: bool,
}

/// One entry of a scan page, with its insertion sequence and source dependencies.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanItem {
    pub sequence: i64,
    pub entry: BlackboardEntry,
    pub dependencies: Vec<SourceDependency>,
}

impl BlackboardStore {
    /// Records `context` (validity `NeedsCheck` or `Obsolete`) for a new revision of the active
    /// entry `id` at `expected_revision`, with stale verification, journaling `change` in the
    /// same transaction. Content, links and promotion are kept. Repeating it, or asking for a
    /// check of an entry that already needs one or is obsolete, changes nothing.
    pub async fn invalidate_entry(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
        expected_revision: u64,
        context: KnowledgeContext,
        change: &ChangeRecord,
    ) -> Result<InvalidationOutcome, BlackboardStoreError> {
        if !matches!(
            context.validity,
            KnowledgeValidity::NeedsCheck | KnowledgeValidity::Obsolete
        ) {
            return Err(BlackboardStoreError::InvalidStoredKnowledge(format!(
                "an invalidation records needs_check or obsolete, not {}",
                context.validity.as_str()
            )));
        }
        let current = self
            .get_entry(project_id, id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        if current.revision != expected_revision {
            return Err(BlackboardStoreError::RevisionConflict {
                expected: expected_revision,
                actual: current.revision,
            });
        }
        if current.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::EntryNotActive(id.to_string()));
        }
        let validity = self
            .knowledge_context_at(project_id, id, current.revision)
            .await?
            .map(|context| context.validity);
        if validity == Some(KnowledgeValidity::Obsolete) || validity == Some(context.validity) {
            return Ok(InvalidationOutcome::Unchanged);
        }
        let value = current.value;
        let update = BlackboardEntryUpdate {
            expected_revision,
            kind: value.kind,
            content: value.content,
            structured_value: value.structured_value,
            confidence: value.confidence,
            verification: BlackboardVerification::Stale,
            importance: value.importance,
            root_promotion: value.root_promotion,
            evidence: value.evidence,
            premises: value.premises,
            state: BlackboardEntryState::Active,
            superseded_by: None,
            provenance: value.provenance,
        };
        self.update_entry_with_context(project_id, id, update, Some(change), Some(&context))
            .await
            .map(|_| InvalidationOutcome::Invalidated)
    }

    /// The qualification jobs of a project.
    pub async fn qualification_jobs(
        &self,
        project_id: &str,
    ) -> Result<Vec<QualificationJob>, BlackboardStoreError> {
        sqlx::query_as::<_, StoredJob>("SELECT * FROM qualification_jobs WHERE project_id = ?")
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(StoredJob::into_job)
            .collect()
    }

    /// Records `head` as the qualified head of a root that has no job yet (its first
    /// observation, when nothing older is known). Returns the root's job.
    pub async fn establish_qualified_head(
        &self,
        project_id: &str,
        project_root: &str,
        head: &str,
    ) -> Result<QualificationJob, BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO qualification_jobs (
                project_id, project_root, qualified_head, target_head, generation, state,
                manifest, reason, watermark, cursor, updated_at_ms
             ) VALUES (?, ?, ?, ?, 1, 'complete', NULL, NULL, 0, 0, ?)
             ON CONFLICT(project_id, project_root) DO NOTHING",
        )
        .bind(project_id)
        .bind(project_root)
        .bind(head)
        .bind(head)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.job(project_id, project_root).await
    }

    /// Starts qualifying the root against `target_head`, replacing any unfinished job (whose
    /// marks stay) with a new generation. The qualified head is kept; `qualified_head` is used
    /// only when the root has no job. Entries inserted so far are below the new watermark.
    pub async fn start_qualification(
        &self,
        project_id: &str,
        project_root: &str,
        qualified_head: &str,
        target_head: &str,
        outcome: Result<String, String>,
    ) -> Result<QualificationJob, BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        let (state, manifest, reason) = match outcome {
            Ok(manifest) => ("scanning", Some(manifest), None),
            Err(reason) => ("blocked", None, Some(reason)),
        };
        sqlx::query(
            "INSERT INTO qualification_jobs (
                project_id, project_root, qualified_head, target_head, generation, state,
                manifest, reason, watermark, cursor, updated_at_ms
             ) VALUES (?, ?, ?, ?, 1, ?, ?, ?,
                (SELECT COALESCE(MAX(rowid), 0) FROM blackboard_entries), 0, ?)
             ON CONFLICT(project_id, project_root) DO UPDATE SET
                target_head = excluded.target_head, generation = generation + 1,
                state = excluded.state, manifest = excluded.manifest, reason = excluded.reason,
                watermark = excluded.watermark, cursor = 0,
                updated_at_ms = excluded.updated_at_ms",
        )
        .bind(project_id)
        .bind(project_root)
        .bind(qualified_head)
        .bind(target_head)
        .bind(state)
        .bind(manifest)
        .bind(reason)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.job(project_id, project_root).await
    }

    /// Moves a scanning job's cursor forward; false when the job is no longer that
    /// generation (a newer target replaced it).
    pub async fn record_qualification_progress(
        &self,
        job: &QualificationJob,
        cursor: i64,
    ) -> Result<bool, BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        let updated = sqlx::query(
            "UPDATE qualification_jobs
             SET cursor = ?, updated_at_ms = ?
             WHERE project_id = ? AND project_root = ? AND generation = ?
               AND state = 'scanning' AND cursor <= ?",
        )
        .bind(cursor)
        .bind(now)
        .bind(&job.project_id)
        .bind(&job.project_root)
        .bind(count(job.generation)?)
        .bind(cursor)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(updated == 1)
    }

    /// Completes a scanning job whose cursor reached its watermark: the target becomes the
    /// qualified head. False when the job changed meanwhile.
    pub async fn complete_qualification(
        &self,
        job: &QualificationJob,
    ) -> Result<bool, BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        let updated = sqlx::query(
            "UPDATE qualification_jobs
             SET state = 'complete', qualified_head = target_head, manifest = NULL,
                 updated_at_ms = ?
             WHERE project_id = ? AND project_root = ? AND generation = ?
               AND state = 'scanning' AND cursor >= watermark",
        )
        .bind(now)
        .bind(&job.project_id)
        .bind(&job.project_root)
        .bind(count(job.generation)?)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(updated == 1)
    }

    /// The next active agent-recorded entries after `cursor`, up to `watermark`, in insertion
    /// order, at most `limit`, each with its source dependencies.
    pub async fn qualification_page(
        &self,
        project_id: &str,
        cursor: i64,
        watermark: i64,
        limit: u32,
    ) -> Result<Vec<ScanItem>, BlackboardStoreError> {
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query_as::<_, (i64, String)>(
            "SELECT entry.rowid, entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND entry.rowid > ? AND entry.rowid <= ?
               AND revision.state = 'active' AND revision.provenance_kind = 'agent'
             ORDER BY entry.rowid LIMIT ?",
        )
        .bind(project_id)
        .bind(cursor)
        .bind(watermark)
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await?;
        let mut items = Vec::with_capacity(rows.len());
        for (sequence, raw_id) in rows {
            let id = BlackboardEntryId::parse(raw_id)?;
            let Some(entry) = load_entry(&mut transaction, project_id, &id).await? else {
                continue;
            };
            let revision = i64::try_from(entry.revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
            let dependencies = sqlx::query_as::<_, (String, String, String)>(
                "SELECT DISTINCT node.project_root, node.relative_path, node.kind
                 FROM hierarchy_nodes AS node
                 WHERE node.project_root IS NOT NULL
                   AND node.kind IN ('directory', 'file', 'region')
                   AND (node.id = (SELECT node_id FROM blackboard_entries WHERE id = ?)
                     OR node.id IN (
                       SELECT evidence.node_id FROM blackboard_evidence_links AS link
                       JOIN context_map_entries AS evidence
                         ON evidence.id = link.context_map_entry_id
                       WHERE link.entry_id = ? AND link.revision = ?))
                 ORDER BY node.project_root, node.relative_path",
            )
            .bind(id.as_str())
            .bind(id.as_str())
            .bind(revision)
            .fetch_all(&mut *transaction)
            .await?
            .into_iter()
            .map(|(project_root, relative_path, kind)| SourceDependency {
                project_root,
                relative_path,
                directory: kind == "directory",
            })
            .collect();
            items.push(ScanItem {
                sequence,
                entry,
                dependencies,
            });
        }
        transaction.commit().await?;
        Ok(items)
    }

    async fn job(
        &self,
        project_id: &str,
        project_root: &str,
    ) -> Result<QualificationJob, BlackboardStoreError> {
        sqlx::query_as::<_, StoredJob>(
            "SELECT * FROM qualification_jobs WHERE project_id = ? AND project_root = ?",
        )
        .bind(project_id)
        .bind(project_root)
        .fetch_one(&self.pool)
        .await?
        .into_job()
    }
}

fn count(value: u64) -> Result<i64, BlackboardStoreError> {
    i64::try_from(value).map_err(|_| BlackboardStoreError::CountOverflow)
}

#[derive(FromRow)]
struct StoredJob {
    project_id: String,
    project_root: String,
    qualified_head: String,
    target_head: String,
    generation: i64,
    state: String,
    manifest: Option<String>,
    reason: Option<String>,
    watermark: i64,
    cursor: i64,
}

impl StoredJob {
    fn into_job(self) -> Result<QualificationJob, BlackboardStoreError> {
        let unsigned =
            |value: i64| u64::try_from(value).map_err(|_| BlackboardStoreError::CountOverflow);
        Ok(QualificationJob {
            project_id: self.project_id,
            project_root: self.project_root,
            qualified_head: self.qualified_head,
            target_head: self.target_head,
            generation: unsigned(self.generation)?,
            state: match self.state.as_str() {
                "scanning" => QualificationState::Scanning,
                "complete" => QualificationState::Complete,
                "blocked" => QualificationState::Blocked,
                other => return Err(BlackboardStoreError::InvalidStoredKnowledge(other.into())),
            },
            manifest: self.manifest,
            reason: self.reason,
            watermark: self.watermark,
            cursor: self.cursor,
        })
    }
}

#[cfg(test)]
#[path = "qualification_tests.rs"]
mod tests;
