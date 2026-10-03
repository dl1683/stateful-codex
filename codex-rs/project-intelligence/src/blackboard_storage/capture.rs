//! One capture committed whole: its new entries with their context and journal rows, the
//! group's counts, where it was read from, and its ordered members, in one transaction. Also
//! the bounded reads recall uses to find knowledge by category before any ranking.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntry;
use crate::BlackboardEntryState;
use crate::CaptureGroup;
use crate::CaptureMember;
use crate::CaptureSource;
use crate::CaptureUnit;
use crate::CommittedCapture;
use crate::ExistingMatch;
use crate::MemberOutcome;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::identity::identities_cover;
use super::identity::identity_matches;
use super::identity::record_identity;
use super::insert_new_entry;
use super::knowledge::append_change;
use super::knowledge::context_of;
use super::knowledge::parse;
use super::knowledge::write_context;
use super::load_entry;
use super::load_entry_by_id;

impl BlackboardStore {
    /// Commits one capture. Units are judged in order: a new identity is saved with its
    /// context and journal row, an identity already active is listed as already present, an
    /// identity retired earlier is not restored, and invalid or overlength units are listed
    /// as omitted. The group row carries the resulting counts. Committing the same group from
    /// the same source again changes nothing and returns the first result.
    pub async fn commit_capture(
        &self,
        group: &CaptureGroup,
        source: &CaptureSource,
        units: Vec<CaptureUnit>,
    ) -> Result<(CommittedCapture, Vec<BlackboardEntry>), BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = load_capture(&mut transaction, &group.project_id, &group.group_id).await?;
        // A group records one source; another source under its ID is refused rather than
        // rewriting what the first capture committed.
        if let Some(existing) = &existing
            && existing.source.as_ref() != Some(source)
        {
            return Err(BlackboardStoreError::EntryIdentityConflict(
                group.group_id.clone(),
            ));
        }
        if let Some(existing) = existing {
            let entries = member_entries(&mut transaction, &group.project_id, &existing).await?;
            transaction.commit().await?;
            return Ok((
                CommittedCapture {
                    newly_committed: false,
                    ..existing
                },
                entries,
            ));
        }
        let now = unix_timestamp_millis()?;
        // Identity lookups are complete only when every entry has an identity; until then
        // nothing new is created, so forgotten words cannot slip through an unindexed entry.
        let covered = identities_cover(&mut transaction, &group.project_id).await?;
        let mut members = Vec::with_capacity(units.len());
        for (ordinal, unit) in units.into_iter().enumerate() {
            let ordinal =
                u32::try_from(ordinal).map_err(|_| BlackboardStoreError::CountOverflow)?;
            let (entry_id, outcome, note) = match unit {
                CaptureUnit::Entry {
                    id,
                    identity_keys,
                    existing,
                    value,
                    context,
                    change,
                } => {
                    commit_entry(
                        &mut transaction,
                        &group.project_id,
                        covered,
                        EntryUnit {
                            id,
                            identity_keys,
                            existing,
                            value: *value,
                            context: *context,
                            change: *change,
                        },
                        now,
                    )
                    .await?
                }
                CaptureUnit::Omitted { note } => (None, MemberOutcome::Omitted, Some(note)),
            };
            members.push(CaptureMember {
                ordinal,
                entry_id,
                outcome,
                note,
            });
        }
        let count = |outcomes: &[MemberOutcome]| {
            u32::try_from(
                members
                    .iter()
                    .filter(|member| outcomes.contains(&member.outcome))
                    .count(),
            )
            .unwrap_or(u32::MAX)
        };
        let group = CaptureGroup {
            recognized: u32::try_from(members.len()).unwrap_or(u32::MAX),
            saved: count(&[MemberOutcome::Saved]),
            already_present: count(&[MemberOutcome::AlreadyPresent]),
            pending: 0,
            omitted: count(&[MemberOutcome::NotRestored, MemberOutcome::Omitted]),
            failed: 0,
            ..group.clone()
        };
        write_group(&mut transaction, &group, source, &members, now).await?;
        let committed = CommittedCapture {
            group,
            source: Some(source.clone()),
            members,
            newly_committed: true,
        };
        let entries =
            member_entries(&mut transaction, &committed.group.project_id, &committed).await?;
        transaction.commit().await?;
        Ok((committed, entries))
    }

    /// A stored capture group with its members in order.
    pub async fn capture(
        &self,
        project_id: &str,
        group_id: &str,
    ) -> Result<Option<CommittedCapture>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_capture(&mut connection, project_id, group_id).await
    }
}

/// One entry unit of a capture.
struct EntryUnit {
    id: crate::BlackboardEntryId,
    identity_keys: Vec<String>,
    existing: ExistingMatch,
    value: crate::NewBlackboardEntry,
    context: crate::KnowledgeContext,
    change: crate::ChangeRecord,
}

/// Judges and commits one entry unit inside the capture transaction: words forgotten under
/// any of its identities stay forgotten, words already active are the same unit, anything
/// else is created with its context, journal row and identity.
async fn commit_entry(
    connection: &mut SqliteConnection,
    project_id: &str,
    covered: bool,
    unit: EntryUnit,
    now: i64,
) -> Result<(Option<String>, MemberOutcome, Option<String>), BlackboardStoreError> {
    let EntryUnit {
        id,
        identity_keys,
        existing,
        value,
        context,
        change,
    } = unit;
    if !covered {
        return Ok((
            None,
            MemberOutcome::Omitted,
            Some(format!(
                "not saved while older entries are being indexed (the answer keeps it): {}",
                change.preview
            )),
        ));
    }
    let matches = identity_matches(&mut *connection, project_id, &identity_keys).await?;
    let own = load_entry_by_id(&mut *connection, &id).await?;
    let retired = matches
        .iter()
        .any(|entry| entry.state != BlackboardEntryState::Active)
        || own
            .as_ref()
            .is_some_and(|entry| entry.state != BlackboardEntryState::Active);
    if retired {
        return Ok((None, MemberOutcome::NotRestored, Some(change.preview)));
    }
    if let Some(own) = own {
        return Ok((
            Some(own.id.to_string()),
            MemberOutcome::AlreadyPresent,
            None,
        ));
    }
    if existing == ExistingMatch::Reuse
        && let Some(active) = matches.first()
    {
        if context_of(&mut *connection, project_id, active.id.as_str())
            .await?
            .is_none()
        {
            let revision = i64::try_from(active.revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
            write_context(&mut *connection, project_id, &active.id, revision, &context).await?;
        }
        return Ok((
            Some(active.id.to_string()),
            MemberOutcome::AlreadyPresent,
            None,
        ));
    }
    if let Err(error) = value.validate() {
        return Ok((
            None,
            MemberOutcome::Omitted,
            Some(format!("{error}: {}", change.preview)),
        ));
    }
    insert_new_entry(&mut *connection, &id, &value, now).await?;
    write_context(&mut *connection, &value.project_id, &id, 1, &context).await?;
    append_change(
        &mut *connection,
        &value.project_id,
        Some((&id, 1)),
        &change,
        now,
    )
    .await?;
    if let Some(key) = identity_keys.first() {
        record_identity(&mut *connection, project_id, key, &id).await?;
    }
    Ok((Some(id.to_string()), MemberOutcome::Saved, None))
}

async fn write_group(
    connection: &mut SqliteConnection,
    group: &CaptureGroup,
    source: &CaptureSource,
    members: &[CaptureMember],
    now: i64,
) -> Result<(), BlackboardStoreError> {
    sqlx::query(
        "INSERT OR REPLACE INTO capture_groups (
            project_id, group_id, thread_id, turn_id, kind, declared_count, recognized,
            saved, already_present, pending, omitted, failed, recorded_at_ms,
            source_locator, source_digest
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&group.project_id)
    .bind(&group.group_id)
    .bind(&group.thread_id)
    .bind(&group.turn_id)
    .bind(&group.kind)
    .bind(group.declared_count.map(i64::from))
    .bind(i64::from(group.recognized))
    .bind(i64::from(group.saved))
    .bind(i64::from(group.already_present))
    .bind(i64::from(group.pending))
    .bind(i64::from(group.omitted))
    .bind(i64::from(group.failed))
    .bind(now)
    .bind(&source.locator)
    .bind(&source.digest)
    .execute(&mut *connection)
    .await?;
    sqlx::query("DELETE FROM capture_group_members WHERE project_id = ? AND group_id = ?")
        .bind(&group.project_id)
        .bind(&group.group_id)
        .execute(&mut *connection)
        .await?;
    for member in members {
        sqlx::query(
            "INSERT INTO capture_group_members (
                project_id, group_id, ordinal, entry_id, outcome, note
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&group.project_id)
        .bind(&group.group_id)
        .bind(i64::from(member.ordinal))
        .bind(&member.entry_id)
        .bind(member.outcome.as_str())
        .bind(&member.note)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(super) async fn load_capture(
    connection: &mut SqliteConnection,
    project_id: &str,
    group_id: &str,
) -> Result<Option<CommittedCapture>, BlackboardStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredGroup>(
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
        "SELECT ordinal, entry_id, outcome, note FROM capture_group_members
         WHERE project_id = ? AND group_id = ? ORDER BY ordinal",
    )
    .bind(project_id)
    .bind(group_id)
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(StoredMember::into_member)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(stored.into_capture(members)?))
}

/// The current entries of a capture's saved and already-present members, in member order.
async fn member_entries(
    connection: &mut SqliteConnection,
    project_id: &str,
    capture: &CommittedCapture,
) -> Result<Vec<BlackboardEntry>, BlackboardStoreError> {
    let mut entries = Vec::new();
    for member in &capture.members {
        if let Some(raw_id) = &member.entry_id {
            let id = crate::BlackboardEntryId::parse(raw_id)
                .map_err(|_| BlackboardStoreError::CorruptEntry(raw_id.clone()))?;
            if let Some(entry) = load_entry(&mut *connection, project_id, &id).await? {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

#[derive(FromRow)]
pub(super) struct StoredGroup {
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
    source_locator: Option<String>,
    source_digest: Option<String>,
}

impl StoredGroup {
    pub(super) fn into_capture(
        self,
        members: Vec<CaptureMember>,
    ) -> Result<CommittedCapture, BlackboardStoreError> {
        let count =
            |value: i64| u32::try_from(value).map_err(|_| BlackboardStoreError::CountOverflow);
        Ok(CommittedCapture {
            group: CaptureGroup {
                project_id: self.project_id,
                group_id: self.group_id,
                thread_id: self.thread_id,
                turn_id: self.turn_id,
                kind: self.kind,
                declared_count: self.declared_count.map(count).transpose()?,
                recognized: count(self.recognized)?,
                saved: count(self.saved)?,
                already_present: count(self.already_present)?,
                pending: count(self.pending)?,
                omitted: count(self.omitted)?,
                failed: count(self.failed)?,
            },
            source: self
                .source_locator
                .zip(self.source_digest)
                .map(|(locator, digest)| CaptureSource { locator, digest }),
            members,
            newly_committed: true,
        })
    }
}

#[derive(FromRow)]
struct StoredMember {
    ordinal: i64,
    entry_id: Option<String>,
    outcome: String,
    note: Option<String>,
}

impl StoredMember {
    fn into_member(self) -> Result<CaptureMember, BlackboardStoreError> {
        Ok(CaptureMember {
            ordinal: u32::try_from(self.ordinal)
                .map_err(|_| BlackboardStoreError::CountOverflow)?,
            entry_id: self.entry_id,
            outcome: parse(&self.outcome)?,
            note: self.note,
        })
    }
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
