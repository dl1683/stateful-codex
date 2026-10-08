//! Bounded committed group recovery in one source/eligibility snapshot.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::knowledge::parse;
use super::knowledge::unsigned;
use crate::CaptureGroup;
use crate::CaptureGroupMember;
use sqlx::FromRow;
use sqlx::SqliteConnection;

impl BlackboardStore {
    /// A stored capture group with its members in order, so a receipt can be replayed.
    pub async fn capture_group(
        &self,
        project_id: &str,
        group_id: &str,
    ) -> Result<Option<CaptureGroup>, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let result = capture_group_on(&mut tx, project_id, group_id).await?;
        tx.commit().await?;
        Ok(result)
    }
}

pub(super) async fn capture_group_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    group_id: &str,
) -> Result<Option<CaptureGroup>, BlackboardStoreError> {
    if project_id.len() > 512 || group_id.len() > 512 {
        return Err(BlackboardStoreError::UnsupportedContext);
    }
    let unsupported: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_groups WHERE project_id = ? AND group_id = ? AND (octet_length(project_id) > 512 OR octet_length(group_id) > 512 OR octet_length(thread_id) > 512 OR octet_length(turn_id) > 512 OR octet_length(kind) > 32)) OR EXISTS(SELECT 1 FROM capture_group_members WHERE project_id = ? AND group_id = ? AND (octet_length(entry_id) > 512 OR octet_length(outcome) > 32 OR octet_length(preview) > 240 OR octet_length(reason) > 240))")
            .bind(project_id).bind(group_id).bind(project_id).bind(group_id).fetch_one(&mut *connection).await?;
    if unsupported {
        return Err(BlackboardStoreError::UnsupportedContext);
    }
    let Some(row) = sqlx::query_as::<_, StoredGroup>(
        "SELECT * FROM capture_groups WHERE project_id = ? AND group_id = ?",
    )
    .bind(project_id)
    .bind(group_id)
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let members = sqlx::query_as::<_, StoredMember>(
        "SELECT * FROM capture_group_members
             WHERE project_id = ? AND group_id = ? ORDER BY ordinal LIMIT 25",
    )
    .bind(project_id)
    .bind(group_id)
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(StoredMember::into_member)
    .collect::<Result<Vec<_>, _>>()?;
    if members.len() > 24 {
        return Err(BlackboardStoreError::UnsupportedContext);
    }
    let count =
        |value: i64| u32::try_from(value).map_err(|_| BlackboardStoreError::RevisionOverflow);
    Ok(Some(CaptureGroup {
        project_id: row.project_id,
        group_id: row.group_id,
        thread_id: row.thread_id,
        turn_id: row.turn_id,
        kind: row.kind,
        declared_count: row.declared_count.map(count).transpose()?,
        recognized: count(row.recognized)?,
        saved: count(row.saved)?,
        already_present: count(row.already_present)?,
        pending: count(row.pending)?,
        omitted: count(row.omitted)?,
        failed: count(row.failed)?,
        members,
    }))
}

#[derive(FromRow)]
struct StoredGroup {
    project_id: String,
    group_id: String,
    thread_id: Option<String>,
    turn_id: Option<String>,
    kind: String,
    declared_count: Option<i64>,
    recognized: i64,
    saved: i64,
    already_present: i64,
    pending: i64,
    omitted: i64,
    failed: i64,
}

#[derive(FromRow)]
struct StoredMember {
    ordinal: i64,
    entry_id: Option<String>,
    revision: Option<i64>,
    outcome: String,
    preview: String,
    reason: Option<String>,
}

impl StoredMember {
    fn into_member(self) -> Result<CaptureGroupMember, BlackboardStoreError> {
        Ok(CaptureGroupMember {
            ordinal: u32::try_from(self.ordinal)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            entry_id: self.entry_id,
            revision: self.revision.map(unsigned).transpose()?,
            outcome: parse(&self.outcome)?,
            preview: self.preview,
            reason: self.reason,
        })
    }
}
