//! The bounded reads recall and capture use to find knowledge by category, before any
//! ranking: one query per read, without loading evidence or relations.

use sqlx::FromRow;

use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;
use crate::CommittedCapture;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::capture::StoredGroup;
use super::kind_name;
use super::knowledge::parse;
use super::parse_kind;
use super::parse_provenance;
use super::parse_state;

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

/// One category read: entries whose current context is in `categories`, plus entries
/// without any context whose kind is in `legacy_kinds`.
#[derive(Clone, Copy, Debug)]
pub struct CategoryQuery<'a> {
    pub project_id: &'a str,
    pub categories: &'a [KnowledgeCategory],
    pub legacy_kinds: &'a [BlackboardKind],
    pub lifecycle: CandidateLifecycle,
    /// Lowercase topic words; when any is given, only entries whose text or recorded
    /// context mentions one are read, so older entries on the topic are never cut off by
    /// newer unrelated ones.
    pub topic: &'a [String],
    /// Only entries changed at or after this time (Unix milliseconds).
    pub changed_since_ms: Option<i64>,
    pub limit: u32,
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
    pub state: BlackboardEntryState,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub context: Option<KnowledgeContext>,
}

const CANDIDATE_COLUMNS: &str = "entry.id, entry.revision, entry.created_at_ms,
    entry.updated_at_ms, revision.kind, revision.content, revision.provenance_kind,
    revision.state, context.category, context.authority, context.scope_id,
    context.end_condition, context.source_sequence, context.unit_ordinal, context.group_id,
    context.validity, context.payload";

impl BlackboardStore {
    /// Entries matching `query`, newest first and a group's members in source order. At
    /// most `limit` (capped at `MAX_CATEGORIZED_ENTRIES`) are returned; the flag says
    /// whether more exist.
    pub async fn categorized_entries(
        &self,
        query: CategoryQuery<'_>,
    ) -> Result<(Vec<CategorizedEntry>, bool), BlackboardStoreError> {
        let limit = query.limit.clamp(1, MAX_CATEGORIZED_ENTRIES);
        let current = query.lifecycle == CandidateLifecycle::Current;
        let sql = format!(
            "SELECT {CANDIDATE_COLUMNS}
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
               AND (? IS NULL OR entry.updated_at_ms >= ?)
               AND (json_array_length(?) = 0 OR EXISTS (
                    SELECT 1 FROM json_each(?) AS term
                    WHERE instr(lower(revision.content), term.value) > 0
                       OR instr(lower(COALESCE(context.payload, '')), term.value) > 0))
             ORDER BY entry.created_at_ms DESC, context.group_id,
                 COALESCE(context.unit_ordinal, 0), entry.id
             LIMIT ?"
        );
        let topic = json_array(query.topic.iter().map(String::as_str));
        let rows = sqlx::query_as::<_, StoredCandidate>(sqlx::AssertSqlSafe(sql))
            .bind(query.project_id)
            .bind(current)
            .bind(current)
            .bind(json_array(
                query.categories.iter().map(|category| category.as_str()),
            ))
            .bind(current)
            .bind(json_array(
                query.legacy_kinds.iter().map(|kind| kind_name(*kind)),
            ))
            .bind(query.changed_since_ms)
            .bind(query.changed_since_ms)
            .bind(&topic)
            .bind(&topic)
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

    /// Every entry, current or retired, already holding a unit's words: entries of
    /// `category` whose context records `words_digest`, and entries without context of
    /// `legacy_kind` whose content, with runs of whitespace collapsed, is `spaced_content`
    /// (the unit's words separated by single spaces; callers compare words exactly). Unbounded by age, so a word-for-word
    /// match is found however much was written since.
    pub async fn entries_with_words(
        &self,
        project_id: &str,
        category: KnowledgeCategory,
        legacy_kind: BlackboardKind,
        spaced_content: &str,
        words_digest: &str,
    ) -> Result<Vec<CategorizedEntry>, BlackboardStoreError> {
        let sql = format!(
            "SELECT {CANDIDATE_COLUMNS}
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             LEFT JOIN knowledge_context AS context
               ON context.entry_id = entry.id AND context.revision = (
                   SELECT MAX(latest.revision) FROM knowledge_context AS latest
                   WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
             WHERE entry.project_id = ?
               AND ((context.category = ?
                     AND json_extract(context.payload, '$.wordsDigest') = ?)
                    OR (context.entry_id IS NULL AND revision.kind = ?
                        AND trim(replace(replace(replace(replace(replace(replace(
                            revision.content, char(13), ' '), char(10), ' '), char(9), ' '),
                            '  ', ' '), '  ', ' '), '  ', ' ')) = ?))
             ORDER BY entry.created_at_ms, entry.id
             LIMIT 64"
        );
        sqlx::query_as::<_, StoredCandidate>(sqlx::AssertSqlSafe(sql))
            .bind(project_id)
            .bind(category.as_str())
            .bind(words_digest)
            .bind(kind_name(legacy_kind))
            .bind(spaced_content)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(StoredCandidate::into_entry)
            .collect()
    }

    /// Capture groups of `kind` recorded at or after `since_ms` (all, without it) that
    /// recognized units they did not keep: the newest `limit`, and how many exist.
    pub async fn incomplete_captures(
        &self,
        project_id: &str,
        kind: &str,
        since_ms: Option<i64>,
        limit: u32,
    ) -> Result<(Vec<CommittedCapture>, u64), BlackboardStoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM capture_groups
             WHERE project_id = ? AND kind = ? AND (omitted > 0 OR failed > 0)
               AND (? IS NULL OR recorded_at_ms >= ?)",
        )
        .bind(project_id)
        .bind(kind)
        .bind(since_ms)
        .bind(since_ms)
        .fetch_one(&self.pool)
        .await?;
        let groups = sqlx::query_as::<_, StoredGroup>(
            "SELECT * FROM capture_groups
             WHERE project_id = ? AND kind = ? AND (omitted > 0 OR failed > 0)
               AND (? IS NULL OR recorded_at_ms >= ?)
             ORDER BY recorded_at_ms DESC, group_id LIMIT ?",
        )
        .bind(project_id)
        .bind(kind)
        .bind(since_ms)
        .bind(since_ms)
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

/// A JSON array of strings, escaped.
fn json_array<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let items = values
        .map(|value| {
            let escaped = value
                .chars()
                .map(|character| match character {
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    character if character.is_control() => {
                        format!("\\u{:04x}", u32::from(character))
                    }
                    character => character.to_string(),
                })
                .collect::<String>();
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

#[derive(FromRow)]
struct StoredCandidate {
    id: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    kind: String,
    content: String,
    provenance_kind: String,
    state: String,
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
            state: parse_state(&self.state)?,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            context,
        })
    }
}
