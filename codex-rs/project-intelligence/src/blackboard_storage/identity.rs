//! One alias/retirement boundary for every PI writer. Queries precede result limits.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::context_bounds::ContextFields;
use super::context_bounds::read_context;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::CAPTURE_NORMALIZER_VERSION;
use crate::KnowledgeContext;
use crate::NewBlackboardEntry;
use crate::canonical_capture_words;
use crate::retirement_capture_words;
use sqlx::SqliteConnection;

/// Current source exclusions and cross-writer retirement apply before root/query limits.
pub(super) const ENTRY_SOURCE_ELIGIBILITY: &str = "
    AND revision.state != 'tombstoned'
    AND NOT EXISTS (SELECT 1 FROM capture_entry_sources AS link
        JOIN capture_sources AS source ON source.source_id = link.source_id
        JOIN capture_source_exclusions AS excluded ON excluded.project_id = source.project_id
          AND excluded.digest = source.digest AND excluded.start_byte < link.end_byte
          AND excluded.end_byte > link.start_byte
        WHERE link.entry_id = entry.id)
    AND NOT EXISTS (SELECT 1 FROM capture_identity_coverage AS coverage
            WHERE coverage.project_id = entry.project_id)
    AND (revision.provenance_kind = 'user' OR (
        (SELECT COUNT(*) FROM capture_current_words AS proof
             WHERE proof.entry_id = entry.id AND proof.revision = entry.revision) = 3
        AND NOT EXISTS (
        SELECT 1 FROM capture_current_words AS current_alias
        CROSS JOIN capture_identity_aliases AS retired_alias
        WHERE current_alias.entry_id = entry.id AND current_alias.revision = entry.revision
          AND current_alias.primary_words != ''
          AND retired_alias.project_id = entry.project_id
          AND ((retired_alias.retirement_words != ''
                AND instr(' ' || current_alias.retirement_words || ' ', ' ' || retired_alias.retirement_words || ' ') > 0)
               OR (retired_alias.retirement_words = '' AND retired_alias.primary_words != ''
                   AND instr(current_alias.primary_words, retired_alias.primary_words) > 0))
          AND retired_alias.retired = 1
          AND (retired_alias.scope_id = '' OR current_alias.scope_id = '' OR retired_alias.scope_id = current_alias.scope_id
              OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = retired_alias.project_id AND scope.scope_id = retired_alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512)
              OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = entry.project_id AND scope.scope_id = current_alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512))
        )))";

/// Automatic proposal recall is unavailable until publication after hooks is fenced.
/// Durable group membership covers proposals even when their context is unsupported.
/// Bounded historical payload checks also cover linked copies of proposal context.
/// The single exception is a proposal the user explicitly applied: its current revision
/// carries the user's own quoted words under a bounded HumanDirect promotion context, which
/// is ordinary admitted knowledge (an Undo or Forget removes that current revision again).
pub(super) fn automatic_entry_eligibility() -> String {
    format!(
        "{ENTRY_SOURCE_ELIGIBILITY} AND (EXISTS (
        SELECT 1 FROM knowledge_context AS applied
        WHERE applied.entry_id = entry.id AND applied.revision = entry.revision
          AND applied.authority = 'human_direct'
          AND octet_length(applied.payload) <= 8192 AND json_valid(applied.payload)
          AND json_type(applied.payload, '$.promotion') = 'object'
          AND json_type(applied.payload, '$.proposal') IS NULL)
        OR (NOT EXISTS (
        SELECT 1 FROM capture_group_members AS member
        JOIN capture_groups AS capture
          ON capture.project_id = member.project_id AND capture.group_id = member.group_id
        WHERE member.entry_id = entry.id AND capture.kind = 'proposal-v1')
        AND NOT EXISTS (
        SELECT 1 FROM knowledge_context AS context
        WHERE context.entry_id = entry.id
          AND CASE
              WHEN octet_length(context.payload) <= 8192 THEN CASE
                  WHEN json_valid(context.payload)
                    THEN json_type(context.payload, '$.proposal') IS NOT NULL
                  ELSE 0 END
              ELSE 0 END)))"
    )
}

impl BlackboardStore {
    /// Used by exact-entry, history and relation expansion before delivering automatic evidence.
    pub async fn entry_source_eligible(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<bool, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let eligible = entry_source_eligible_on(&mut tx, project_id, id).await?;
        tx.commit().await?;
        Ok(eligible)
    }

    /// Exact automatic recovery checks exclusion and loads entry in one read snapshot.
    /// Explicit archival history continues to use `get_entry`.
    pub async fn get_source_eligible_entry(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<Option<crate::BlackboardEntry>, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let entry = if entry_source_eligible_on(&mut tx, project_id, id).await? {
            super::load_entry(&mut tx, project_id, id).await?
        } else {
            None
        };
        tx.commit().await?;
        Ok(entry)
    }
}

pub(super) async fn entry_source_eligible_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardEntryId,
) -> Result<bool, BlackboardStoreError> {
    let eligibility = automatic_entry_eligibility();
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id AND revision.revision = entry.revision WHERE entry.project_id = ? AND entry.id = ? {eligibility})")))
        .bind(project_id).bind(id.as_str()).fetch_one(connection).await?)
}

/// Storage replay retains the common retirement policy without enabling automatic recall.
pub(super) async fn entry_storage_eligible_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardEntryId,
) -> Result<bool, BlackboardStoreError> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id AND revision.revision = entry.revision WHERE entry.project_id = ? AND entry.id = ? {ENTRY_SOURCE_ELIGIBILITY})")))
        .bind(project_id).bind(id.as_str()).fetch_one(connection).await?)
}

pub(super) async fn check_activation(
    connection: &mut SqliteConnection,
    value: &NewBlackboardEntry,
    context: Option<&KnowledgeContext>,
) -> Result<(), BlackboardStoreError> {
    if !coverage_available(connection, &value.project_id).await? {
        return Err(BlackboardStoreError::IdentityCoverageIncomplete);
    }
    let requested_scope = context
        .and_then(|context| context.scope_id.as_deref())
        .unwrap_or("");
    // Different strings prove disjoint scope only when both have bounded host scope rows.
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_scopes WHERE project_id = ? AND scope_id = ? AND octet_length(scope_id) <= 512 AND octet_length(title) <= 512 AND (end_condition IS NULL OR octet_length(end_condition) <= 2000) AND octet_length(opened_source) <= 512)")
        .bind(&value.project_id).bind(requested_scope).fetch_one(&mut *connection).await?;
    let scope = if known { requested_scope } else { "" };
    for text in [
        Some(value.content.as_str()),
        value
            .structured_value
            .as_ref()
            .map(|value| value.value.as_str()),
        value
            .structured_value
            .as_ref()
            .and_then(|value| value.unit.as_deref()),
    ] {
        let Some(text) = text else {
            continue;
        };
        if !text_eligible(connection, &value.project_id, scope, text).await? {
            return Err(BlackboardStoreError::RetiredIdentity);
        }
    }
    Ok(())
}

/// The same retired-word match guards activation and native/relational text fallbacks.
pub(super) async fn text_eligible(
    connection: &mut SqliteConnection,
    project_id: &str,
    scope: &str,
    text: &str,
) -> Result<bool, BlackboardStoreError> {
    if !coverage_available(connection, project_id).await? {
        return Ok(false);
    }
    let retired: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_identity_aliases AS alias WHERE project_id = ? AND retired = 1 AND ((retirement_words != '' AND instr(?, ' ' || retirement_words || ' ') > 0) OR (retirement_words = '' AND primary_words != '' AND instr(?, primary_words) > 0)) AND (scope_id = '' OR ? = '' OR scope_id = ? OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = alias.project_id AND scope.scope_id = alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512)))")
        .bind(project_id).bind(format!(" {} ", retirement_capture_words(text)))
        .bind(canonical_capture_words(text)).bind(scope).bind(scope)
        .fetch_one(connection).await?;
    Ok(!retired)
}

pub(super) async fn register(
    connection: &mut SqliteConnection,
    id: &BlackboardEntryId,
    value: &NewBlackboardEntry,
    state: BlackboardEntryState,
    context: Option<&KnowledgeContext>,
) -> Result<(), BlackboardStoreError> {
    register_alias(
        connection,
        &value.project_id,
        id,
        value.kind,
        &value.content,
        state,
        context,
    )
    .await?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM blackboard_entries WHERE id = ?")
        .bind(id.as_str())
        .fetch_one(&mut *connection)
        .await?;
    super::current_words::register(connection, id.as_str(), revision, context).await
}

async fn register_alias(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardEntryId,
    kind: BlackboardKind,
    content: &str,
    state: BlackboardEntryState,
    context: Option<&KnowledgeContext>,
) -> Result<(), BlackboardStoreError> {
    let words = canonical_capture_words(content);
    // Earlier wording remains a retirement fence after an in-place revision too.
    sqlx::query("UPDATE capture_identity_aliases SET retired = 1 WHERE entry_id = ? AND category NOT IN ('structured_value', 'structured_unit') AND (primary_words != ? OR ?)")
        .bind(id.as_str()).bind(&words).bind(state != BlackboardEntryState::Active).execute(&mut *connection).await?;
    let original: String = sqlx::query_scalar("SELECT provenance_kind FROM blackboard_entry_revisions WHERE entry_id = ? ORDER BY revision LIMIT 1")
        .bind(id.as_str()).fetch_one(&mut *connection).await?;
    let authority = context.map_or(original.as_str(), |context| context.authority.as_str());
    let category = context.map_or(
        match kind {
            BlackboardKind::Instruction => "rule",
            BlackboardKind::Decision => "decision",
            BlackboardKind::RejectedApproach => "ruled_out",
            BlackboardKind::Question => "open_check",
            BlackboardKind::Fact
            | BlackboardKind::Claim
            | BlackboardKind::Number
            | BlackboardKind::Strategy
            | BlackboardKind::Contradiction
            | BlackboardKind::Failure
            | BlackboardKind::Signal
            | BlackboardKind::Note => "note",
        },
        |context| context.category.as_str(),
    );
    let scope = context
        .and_then(|context| context.scope_id.as_deref())
        .unwrap_or("");
    sqlx::query("INSERT INTO capture_identity_aliases(project_id, entry_id, normalizer_version, category, authority, scope_id, primary_words, retirement_words, retired) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(entry_id, primary_words, category, authority, scope_id) DO UPDATE SET retired = MAX(retired, excluded.retired)")
        .bind(project_id).bind(id.as_str()).bind(CAPTURE_NORMALIZER_VERSION)
        .bind(category).bind(authority).bind(scope).bind(words).bind(retirement_capture_words(content))
        .bind(state != BlackboardEntryState::Active).execute(connection).await?;
    Ok(())
}

pub(super) async fn direct_match(
    connection: &mut SqliteConnection,
    value: &NewBlackboardEntry,
    context: &KnowledgeContext,
) -> Result<Option<BlackboardEntryId>, BlackboardStoreError> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT entry_id FROM capture_identity_aliases WHERE project_id = ? AND primary_words = ? AND category = ? AND authority = ? AND scope_id = ? AND retired = 0 ORDER BY entry_id LIMIT 2")
        .bind(&value.project_id).bind(canonical_capture_words(&value.content))
        .bind(context.category.as_str()).bind(context.authority.as_str()).bind(context.scope_id.as_deref().unwrap_or(""))
        .fetch_all(&mut *connection).await?;
    if ids.len() > 1 {
        return Err(BlackboardStoreError::AmbiguousIdentity);
    }
    let Some(id) = ids.first() else {
        return Ok(None);
    };
    // Validate legacy metadata before reuse, rather than trusting an old alias projection.
    read_context(connection, &value.project_id, id, ContextFields::Identity).await?;
    Ok(Some(BlackboardEntryId::parse(id)?))
}

impl BlackboardStore {
    /// Reports whether automatic identity coverage is available. Historical alias replay is
    /// excluded: it can manufacture retirement of live wording on populated upgraded stores.
    /// Upgraded stores, including previously certified ones, retain their archival bytes
    /// and return false without mutation. Fresh stores have no upgrade coverage row.
    pub async fn maintain_capture_identities(
        &self,
        project_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let complete = coverage_available(&mut tx, project_id).await?;
        tx.commit().await?;
        Ok(complete)
    }
}

pub(super) async fn coverage_available(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<bool, BlackboardStoreError> {
    let upgraded: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM capture_identity_coverage WHERE project_id = ?)",
    )
    .bind(project_id)
    .fetch_one(connection)
    .await?;
    Ok(!upgraded)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "identity_repair2_tests.rs"]
mod repair2_tests;
