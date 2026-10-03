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
    /// Topic words; when any is given, only entries whose text or recorded answer opening
    /// contains one are read, so older entries on the topic are never cut off by newer
    /// unrelated ones. Matching here is a superset; callers judge relevance.
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
                       OR instr(revision.content, term.value) > 0
                       OR instr(lower(COALESCE(
                              json_extract(context.payload, '$.answerOpening'), '')),
                              term.value) > 0
                       OR instr(COALESCE(
                              json_extract(context.payload, '$.answerOpening'), ''),
                              term.value) > 0))
             ORDER BY entry.created_at_ms DESC, context.group_id,
                 COALESCE(context.unit_ordinal, 0), entry.id
             LIMIT ?"
        );
        // SQLite lowercases ASCII only, so each word is also matched as written in lower,
        // upper and title case; the caller's matching decides relevance afterwards.
        let variants = query
            .topic
            .iter()
            .flat_map(|term| {
                let mut title = term.chars();
                let title = title
                    .next()
                    .map(|first| first.to_uppercase().chain(title).collect::<String>())
                    .unwrap_or_default();
                [term.to_lowercase(), term.to_uppercase(), title]
            })
            .collect::<Vec<_>>();
        let topic = json_array(variants.iter().map(String::as_str));
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

    /// Every entry, current or retired, that may hold a unit's words where the unit applies:
    /// assistant-reported entries of `category` (project-wide or in `scope_id`) whose context
    /// records `words_digest`, and agent entries without context of `legacy_kind` whose
    /// lowercased content without ASCII whitespace is `compact_content`. Eligibility is
    /// decided before the bound; callers compare normalized words exactly. Unbounded by age, so a word-for-word
    /// match is found however much was written since.
    pub async fn entries_with_words(
        &self,
        project_id: &str,
        category: KnowledgeCategory,
        legacy_kind: BlackboardKind,
        scope_id: Option<&str>,
        compact_content: &str,
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
                     AND context.authority = 'assistant_reported'
                     AND (context.scope_id IS NULL OR context.scope_id = ?)
                     AND json_extract(context.payload, '$.wordsDigest') = ?)
                    OR (context.entry_id IS NULL AND revision.kind = ?
                        AND revision.provenance_kind = 'agent'
                        AND lower(replace(replace(replace(replace(
                            revision.content, ' ', ''), char(9), ''), char(10), ''),
                            char(13), '')) = ?))
             ORDER BY entry.created_at_ms, entry.id
             LIMIT 256"
        );
        sqlx::query_as::<_, StoredCandidate>(sqlx::AssertSqlSafe(sql))
            .bind(project_id)
            .bind(category.as_str())
            .bind(scope_id)
            .bind(words_digest)
            .bind(kind_name(legacy_kind))
            .bind(compact_content)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(StoredCandidate::into_entry)
            .collect()
    }

    /// The current entries among `ids` (active, and current or in need of a check when they
    /// carry a context): the members of a selected group that a bounded read left out.
    pub async fn current_entries(
        &self,
        project_id: &str,
        ids: &[String],
    ) -> Result<Vec<CategorizedEntry>, BlackboardStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {CANDIDATE_COLUMNS}
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             LEFT JOIN knowledge_context AS context
               ON context.entry_id = entry.id AND context.revision = (
                   SELECT MAX(latest.revision) FROM knowledge_context AS latest
                   WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
             WHERE entry.project_id = ? AND revision.state = 'active'
               AND entry.id IN (SELECT value FROM json_each(?))
               AND (context.entry_id IS NULL
                    OR context.validity IN ('current', 'needs_check'))"
        );
        sqlx::query_as::<_, StoredCandidate>(sqlx::AssertSqlSafe(sql))
            .bind(project_id)
            .bind(json_array(ids.iter().map(String::as_str)))
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
