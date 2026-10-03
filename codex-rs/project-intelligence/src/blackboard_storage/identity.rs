//! The canonical identity index of captured knowledge, and its backfill for entries written
//! without one. The key itself is computed by the caller (it owns word normalization); this
//! module stores keys, finds the entries under them with their current lifecycle, and
//! tracks how far older entries have been indexed.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::parse_kind;
use super::parse_provenance;
use super::parse_state;

/// One entry the identity backfill reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyEntry {
    pub rowid: i64,
    pub id: BlackboardEntryId,
    pub kind: BlackboardKind,
    pub provenance_kind: BlackboardProvenanceKind,
    pub content: String,
    /// The scope recorded with the entry, if any.
    pub scope_id: Option<String>,
}

/// An entry found under an identity key, with its current lifecycle.
pub(super) struct IdentityMatch {
    pub(super) id: BlackboardEntryId,
    pub(super) revision: u64,
    pub(super) state: BlackboardEntryState,
}

impl BlackboardStore {
    /// Whether every entry of the project has its identity computed.
    pub async fn identities_complete(
        &self,
        project_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        identities_cover(&mut connection, project_id).await
    }

    /// The entries recorded under any of `keys`, oldest first, with their current revision
    /// and whether each is still active.
    pub async fn identity_entries(
        &self,
        project_id: &str,
        keys: &[String],
    ) -> Result<Vec<(BlackboardEntryId, u64, bool)>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        Ok(identity_matches(&mut connection, project_id, keys)
            .await?
            .into_iter()
            .map(|entry| {
                (
                    entry.id,
                    entry.revision,
                    entry.state == BlackboardEntryState::Active,
                )
            })
            .collect())
    }

    /// The entries after the covered rowid in rowid order, at most `limit`, for the
    /// identity backfill, with the covered rowid and the newest rowid of the project.
    pub async fn identity_backfill_batch(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<(Vec<LegacyEntry>, i64, i64), BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        let covered = covered_rowid(&mut connection, project_id).await?;
        let newest = newest_rowid(&mut connection, project_id).await?;
        let rows = sqlx::query_as::<_, StoredLegacy>(
            "SELECT entry.rowid AS rowid, entry.id, revision.kind, revision.provenance_kind,
                    revision.content,
                    (SELECT context.scope_id FROM knowledge_context AS context
                     WHERE context.entry_id = entry.id
                     ORDER BY context.revision DESC LIMIT 1) AS scope_id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND entry.rowid > ?
             ORDER BY entry.rowid LIMIT ?",
        )
        .bind(project_id)
        .bind(covered)
        .bind(i64::from(limit))
        .fetch_all(&mut *connection)
        .await?;
        let entries = rows
            .into_iter()
            .map(StoredLegacy::into_entry)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, covered, newest))
    }

    /// Records identities computed for a backfill batch and advances the covered rowid to
    /// `through_rowid`, in one transaction. A batch computed from an older watermark than
    /// the stored one changes nothing.
    pub async fn record_identity_backfill(
        &self,
        project_id: &str,
        from_rowid: i64,
        through_rowid: i64,
        identities: &[(String, BlackboardEntryId)],
    ) -> Result<(), BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if covered_rowid(&mut transaction, project_id).await? != from_rowid {
            transaction.commit().await?;
            return Ok(());
        }
        for (key, id) in identities {
            record_identity(&mut transaction, project_id, key, id).await?;
        }
        sqlx::query(
            "INSERT INTO knowledge_identity_coverage (project_id, covered_rowid) VALUES (?, ?)
             ON CONFLICT(project_id) DO UPDATE SET covered_rowid = excluded.covered_rowid",
        )
        .bind(project_id)
        .bind(through_rowid)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// Whether every entry of the project up to the newest has its identity computed.
pub(super) async fn identities_cover(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<bool, BlackboardStoreError> {
    Ok(covered_rowid(&mut *connection, project_id).await?
        >= newest_rowid(&mut *connection, project_id).await?)
}

/// Every entry recorded under any of `keys`, with its current lifecycle.
pub(super) async fn identity_matches(
    connection: &mut SqliteConnection,
    project_id: &str,
    keys: &[String],
) -> Result<Vec<IdentityMatch>, BlackboardStoreError> {
    let mut found = Vec::new();
    for key in keys {
        let rows = sqlx::query_as::<_, StoredMatch>(
            "SELECT entry.id, entry.revision, revision.state
             FROM knowledge_identities AS identity
             JOIN blackboard_entries AS entry ON entry.id = identity.entry_id
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE identity.project_id = ? AND identity.identity_key = ?
             ORDER BY entry.rowid",
        )
        .bind(project_id)
        .bind(key)
        .fetch_all(&mut *connection)
        .await?;
        for row in rows {
            found.push(IdentityMatch {
                id: BlackboardEntryId::parse(&row.id)
                    .map_err(|_| BlackboardStoreError::CorruptEntry(row.id.clone()))?,
                revision: u64::try_from(row.revision)
                    .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
                state: parse_state(&row.state)?,
            });
        }
    }
    Ok(found)
}

/// Records `id` under `key`.
pub(super) async fn record_identity(
    connection: &mut SqliteConnection,
    project_id: &str,
    key: &str,
    id: &BlackboardEntryId,
) -> Result<(), BlackboardStoreError> {
    sqlx::query(
        "INSERT OR IGNORE INTO knowledge_identities (project_id, identity_key, entry_id)
         VALUES (?, ?, ?)",
    )
    .bind(project_id)
    .bind(key)
    .bind(id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn covered_rowid(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<i64, BlackboardStoreError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT covered_rowid FROM knowledge_identity_coverage WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .unwrap_or(0))
}

async fn newest_rowid(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<i64, BlackboardStoreError> {
    Ok(sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(rowid) FROM blackboard_entries WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_one(&mut *connection)
    .await?
    .unwrap_or(0))
}

#[derive(FromRow)]
struct StoredMatch {
    id: String,
    revision: i64,
    state: String,
}

#[derive(FromRow)]
struct StoredLegacy {
    rowid: i64,
    id: String,
    kind: String,
    provenance_kind: String,
    content: String,
    scope_id: Option<String>,
}

impl StoredLegacy {
    fn into_entry(self) -> Result<LegacyEntry, BlackboardStoreError> {
        Ok(LegacyEntry {
            rowid: self.rowid,
            id: BlackboardEntryId::parse(&self.id)
                .map_err(|_| BlackboardStoreError::CorruptEntry(self.id.clone()))?,
            kind: parse_kind(&self.kind)?,
            provenance_kind: parse_provenance(&self.provenance_kind)?,
            content: self.content,
            scope_id: self.scope_id,
        })
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
