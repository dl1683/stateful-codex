//! One capture committed whole: its new entries with their context and journal rows, the
//! group's counts, where it was read from, and its ordered members, in one transaction. Also
//! the bounded reads recall uses to find knowledge by category before any ranking.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;
use crate::CaptureGroup;
use crate::CaptureMember;
use crate::CaptureSource;
use crate::CaptureUnit;
use crate::CommittedCapture;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::MemberOutcome;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::insert_new_entry;
use super::kind_name;
use super::knowledge::append_change;
use super::knowledge::context_of;
use super::knowledge::parse;
use super::knowledge::write_context;
use super::load_entry;
use super::load_entry_by_id;
use super::parse_kind;
use super::parse_provenance;

/// Most entries one category read returns.
pub const MAX_CATEGORIZED_ENTRIES: u32 = 5_000;

/// Which lifecycle a category read selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateLifecycle {
    /// Active entries that are current or need a check.
    Current,
    /// Entries that were forgotten or replaced.
    Retired,
}

/// One entry of a category read: what recall and capture need, without evidence or
/// relations, and the context of its current revision (`None` for entries written before
/// contexts were recorded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorizedEntry {
    pub id: BlackboardEntryId,
    pub revision: u64,
    pub kind: BlackboardKind,
    pub content: String,
    pub provenance_kind: BlackboardProvenanceKind,
    pub created_at_ms: i64,
    pub context: Option<KnowledgeContext>,
}

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
        let mut members = Vec::with_capacity(units.len());
        for (ordinal, unit) in units.into_iter().enumerate() {
            let ordinal =
                u32::try_from(ordinal).map_err(|_| BlackboardStoreError::CountOverflow)?;
            let (entry_id, outcome, note) = match unit {
                CaptureUnit::Entry {
                    id,
                    retired_identities,
                    value,
                    context,
                    change,
                } => match load_entry_by_id(&mut transaction, &id).await? {
                    Some(existing)
                        if existing.state == BlackboardEntryState::Active
                            && same_meaning(&existing, &value) =>
                    {
                        (Some(id.to_string()), MemberOutcome::AlreadyPresent, None)
                    }
                    Some(existing) if existing.state == BlackboardEntryState::Active => (
                        None,
                        MemberOutcome::Omitted,
                        Some(format!("changed since it was saved: {}", change.preview)),
                    ),
                    Some(_) => (None, MemberOutcome::NotRestored, Some(change.preview)),
                    None if retired(&mut transaction, &retired_identities).await? => {
                        (None, MemberOutcome::NotRestored, Some(change.preview))
                    }
                    None => match value.validate() {
                        Ok(()) => {
                            insert_new_entry(&mut transaction, &id, &value, now).await?;
                            write_context(&mut transaction, &value.project_id, &id, 1, &context)
                                .await?;
                            append_change(
                                &mut transaction,
                                &value.project_id,
                                Some((&id, 1)),
                                &change,
                                now,
                            )
                            .await?;
                            (Some(id.to_string()), MemberOutcome::Saved, None)
                        }
                        Err(error) => (
                            None,
                            MemberOutcome::Omitted,
                            Some(format!("{error}: {}", change.preview)),
                        ),
                    },
                },
                CaptureUnit::Existing {
                    id,
                    revision,
                    context,
                } => match load_entry(&mut transaction, &group.project_id, &id).await? {
                    Some(existing)
                        if existing.state == BlackboardEntryState::Active
                            && existing.revision == revision =>
                    {
                        if context_of(&mut transaction, &group.project_id, id.as_str())
                            .await?
                            .is_none()
                        {
                            let revision = i64::try_from(existing.revision)
                                .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
                            write_context(
                                &mut transaction,
                                &group.project_id,
                                &id,
                                revision,
                                &context,
                            )
                            .await?;
                        }
                        (Some(id.to_string()), MemberOutcome::AlreadyPresent, None)
                    }
                    Some(existing) if existing.state == BlackboardEntryState::Active => (
                        None,
                        MemberOutcome::Omitted,
                        Some(format!("{id} changed while this was being saved")),
                    ),
                    _ => (None, MemberOutcome::NotRestored, Some(id.to_string())),
                },
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

    /// Entries whose current context is in `categories`, plus entries without any context
    /// whose kind is in `legacy_kinds`: current ones (active, and current or in need of a
    /// check) or retired ones (forgotten or replaced). Newest first, a group's members in
    /// source order, read in one query without loading whole entries. At most `limit`
    /// (capped at `MAX_CATEGORIZED_ENTRIES`) are returned; the flag says whether more exist.
    pub async fn categorized_entries(
        &self,
        project_id: &str,
        categories: &[KnowledgeCategory],
        legacy_kinds: &[BlackboardKind],
        lifecycle: CandidateLifecycle,
        limit: u32,
    ) -> Result<(Vec<CategorizedEntry>, bool), BlackboardStoreError> {
        let limit = limit.clamp(1, MAX_CATEGORIZED_ENTRIES);
        // The values are fixed identifiers, so a JSON array of them needs no escaping.
        let json_array = |names: Vec<&str>| {
            format!(
                "[{}]",
                names
                    .iter()
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let categories = json_array(
            categories
                .iter()
                .map(|category| category.as_str())
                .collect(),
        );
        let legacy_kinds = json_array(legacy_kinds.iter().map(|kind| kind_name(*kind)).collect());
        let current = lifecycle == CandidateLifecycle::Current;
        let rows = sqlx::query_as::<_, StoredCandidate>(
            "SELECT entry.id, entry.revision, entry.created_at_ms, revision.kind, revision.content,
                    revision.provenance_kind, context.category, context.authority,
                    context.scope_id, context.end_condition, context.source_sequence,
                    context.unit_ordinal, context.group_id, context.validity, context.payload
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             LEFT JOIN knowledge_context AS context
               ON context.entry_id = entry.id AND context.revision = (
                   SELECT MAX(latest.revision) FROM knowledge_context AS latest
                   WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
             WHERE entry.project_id = ? AND ((? AND revision.state = 'active')
                    OR (NOT ? AND revision.state <> 'active'))
               AND ((context.category IN (SELECT value FROM json_each(?))
                     AND (NOT ? OR context.validity IN ('current', 'needs_check')))
                    OR (context.entry_id IS NULL
                        AND revision.kind IN (SELECT value FROM json_each(?))))
             ORDER BY entry.created_at_ms DESC, context.group_id,
                 COALESCE(context.unit_ordinal, 0), entry.id
             LIMIT ?",
        )
        .bind(project_id)
        .bind(current)
        .bind(current)
        .bind(categories)
        .bind(current)
        .bind(legacy_kinds)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await?;
        let more = rows.len() > limit as usize;
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(StoredCandidate::into_entry)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, more))
    }

    /// Capture groups of `kind` that recognized units they did not keep, newest first, at
    /// most `limit`, and how many such groups exist.
    pub async fn incomplete_captures(
        &self,
        project_id: &str,
        kind: &str,
        limit: u32,
    ) -> Result<(Vec<CommittedCapture>, u64), BlackboardStoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM capture_groups
             WHERE project_id = ? AND kind = ? AND (omitted > 0 OR failed > 0)",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_one(&self.pool)
        .await?;
        let groups = sqlx::query_as::<_, StoredGroup>(
            "SELECT * FROM capture_groups
             WHERE project_id = ? AND kind = ? AND (omitted > 0 OR failed > 0)
             ORDER BY recorded_at_ms DESC, group_id LIMIT ?",
        )
        .bind(project_id)
        .bind(kind)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|group| group.into_capture(Vec::new()))
        .collect::<Result<Vec<_>, _>>()?;
        Ok((
            groups,
            u64::try_from(total).map_err(|_| BlackboardStoreError::CountOverflow)?,
        ))
    }
}

/// Whether an identity's stored entry still holds the same knowledge: same kind and the
/// same words apart from spacing.
fn same_meaning(existing: &BlackboardEntry, value: &crate::NewBlackboardEntry) -> bool {
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    existing.value.kind == value.kind && words(&existing.value.content) == words(&value.content)
}

/// Whether any of `ids` exists and is no longer active.
async fn retired(
    connection: &mut SqliteConnection,
    ids: &[crate::BlackboardEntryId],
) -> Result<bool, BlackboardStoreError> {
    for id in ids {
        if load_entry_by_id(&mut *connection, id)
            .await?
            .is_some_and(|entry| entry.state != BlackboardEntryState::Active)
        {
            return Ok(true);
        }
    }
    Ok(false)
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

async fn load_capture(
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
    source_locator: Option<String>,
    source_digest: Option<String>,
}

impl StoredGroup {
    fn into_capture(
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
struct StoredCandidate {
    id: String,
    revision: i64,
    created_at_ms: i64,
    kind: String,
    content: String,
    provenance_kind: String,
    category: Option<String>,
    authority: Option<String>,
    scope_id: Option<String>,
    end_condition: Option<String>,
    source_sequence: Option<i64>,
    unit_ordinal: Option<i64>,
    group_id: Option<String>,
    validity: Option<String>,
    payload: Option<String>,
}

impl StoredCandidate {
    fn into_entry(self) -> Result<CategorizedEntry, BlackboardStoreError> {
        let context = match (self.category, self.authority, self.validity) {
            (Some(category), Some(authority), Some(validity)) => Some(KnowledgeContext {
                category: parse(&category)?,
                authority: parse(&authority)?,
                scope_id: self.scope_id,
                end_condition: self.end_condition,
                source_sequence: self
                    .source_sequence
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
                unit_ordinal: self
                    .unit_ordinal
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
                group_id: self.group_id,
                validity: parse(&validity)?,
                payload: self.payload,
            }),
            _ => None,
        };
        Ok(CategorizedEntry {
            id: BlackboardEntryId::parse(&self.id)
                .map_err(|_| BlackboardStoreError::CorruptEntry(self.id.clone()))?,
            revision: u64::try_from(self.revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            kind: parse_kind(&self.kind)?,
            content: self.content,
            provenance_kind: parse_provenance(&self.provenance_kind)?,
            created_at_ms: self.created_at_ms,
            context,
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
