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
    AND (revision.provenance_kind = 'user' OR NOT EXISTS (
        SELECT 1 FROM capture_identity_aliases AS current_alias
        JOIN capture_identity_aliases AS retired_alias
          ON retired_alias.project_id = current_alias.project_id
          AND retired_alias.retirement_words != ''
          AND instr(' ' || current_alias.retirement_words || ' ', ' ' || retired_alias.retirement_words || ' ') > 0
          AND retired_alias.retired = 1
          AND (retired_alias.scope_id = '' OR current_alias.scope_id = '' OR retired_alias.scope_id = current_alias.scope_id
              OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = retired_alias.project_id AND scope.scope_id = retired_alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512)
              OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = current_alias.project_id AND scope.scope_id = current_alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512))
        WHERE current_alias.entry_id = entry.id AND current_alias.retired = 0))";

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
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id AND revision.revision = entry.revision WHERE entry.project_id = ? AND entry.id = ? {ENTRY_SOURCE_ELIGIBILITY})")))
        .bind(project_id).bind(id.as_str()).fetch_one(connection).await?)
}

pub(super) async fn check_activation(
    connection: &mut SqliteConnection,
    value: &NewBlackboardEntry,
    context: Option<&KnowledgeContext>,
) -> Result<(), BlackboardStoreError> {
    let incomplete: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_identity_coverage WHERE project_id = ? AND (after_rowid < watermark OR blocked_reason IS NOT NULL))")
        .bind(&value.project_id).fetch_one(&mut *connection).await?;
    if incomplete {
        return Err(BlackboardStoreError::IdentityCoverageIncomplete);
    }
    let requested_scope = context
        .and_then(|context| context.scope_id.as_deref())
        .unwrap_or("");
    // Different strings prove disjoint scope only when both have bounded host scope rows.
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_scopes WHERE project_id = ? AND scope_id = ? AND octet_length(scope_id) <= 512 AND octet_length(title) <= 512 AND (end_condition IS NULL OR octet_length(end_condition) <= 2000) AND octet_length(opened_source) <= 512)")
        .bind(&value.project_id).bind(requested_scope).fetch_one(&mut *connection).await?;
    let scope = if known { requested_scope } else { "" };
    let retired: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_identity_aliases AS alias WHERE project_id = ? AND retirement_words != '' AND instr(?, ' ' || retirement_words || ' ') > 0 AND retired = 1 AND (scope_id = '' OR ? = '' OR scope_id = ? OR NOT EXISTS(SELECT 1 FROM knowledge_scopes AS scope WHERE scope.project_id = alias.project_id AND scope.scope_id = alias.scope_id AND octet_length(scope.scope_id) <= 512 AND octet_length(scope.title) <= 512 AND (scope.end_condition IS NULL OR octet_length(scope.end_condition) <= 2000) AND octet_length(scope.opened_source) <= 512)))")
        .bind(&value.project_id).bind(format!(" {} ", retirement_capture_words(&value.content)))
        .bind(scope).bind(scope).fetch_one(connection).await?;
    if retired {
        return Err(BlackboardStoreError::RetiredIdentity);
    }
    Ok(())
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
    .await
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
    sqlx::query("UPDATE capture_identity_aliases SET retired = 1 WHERE entry_id = ? AND (primary_words != ? OR ?)")
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

type IdentityRevision = (i64, Option<String>, String, Option<String>, String);

impl BlackboardStore {
    /// Upgrade coverage, at most 64 revision rows per page and 256 per operation.
    /// Progress commits independently of later refused semantic writes. Unknown source stays unknown.
    pub async fn maintain_capture_identities(
        &self,
        project_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        let start = std::time::Instant::now();
        let mut examined = 0;
        while examined < 256
            && start.elapsed() < std::time::Duration::from_millis(/*millis*/ 1500)
        {
            let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let coverage: Option<(i64, i64, Option<String>)> = sqlx::query_as("SELECT after_rowid, watermark, blocked_reason FROM capture_identity_coverage WHERE project_id = ?")
                .bind(project_id).fetch_optional(&mut *tx).await?;
            let Some((after, watermark, blocked)) = coverage else {
                return Ok(true);
            };
            if blocked.is_some() {
                return Ok(false);
            }
            if after >= watermark {
                return Ok(true);
            }
            let rows: Vec<IdentityRevision> = sqlx::query_as("SELECT revision.rowid, CASE WHEN octet_length(entry.id) <= 512 THEN entry.id END, revision.kind, CASE WHEN octet_length(revision.content) <= 4096 AND octet_length(entry.id) <= 512 THEN revision.content END, revision.state FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id WHERE entry.project_id = ? AND revision.rowid > ? AND revision.rowid <= ? ORDER BY revision.rowid LIMIT 64")
                .bind(project_id).bind(after).bind(watermark).fetch_all(&mut *tx).await?;
            let mut last = after;
            for (rowid, id, kind, content, historical_state) in &rows {
                let (Some(id), Some(content)) = (id, content) else {
                    sqlx::query("UPDATE capture_identity_coverage SET blocked_reason = 'unsupported legacy identity' WHERE project_id = ?").bind(project_id).execute(&mut *tx).await?;
                    tx.commit().await?;
                    return Ok(false);
                };
                let id = BlackboardEntryId::parse(id)?;
                let context = match read_context(
                    &mut tx,
                    project_id,
                    id.as_str(),
                    ContextFields::Identity,
                )
                .await
                {
                    Ok(context) => context,
                    Err(BlackboardStoreError::UnsupportedContext) => {
                        sqlx::query("UPDATE capture_identity_coverage SET blocked_reason = 'unsupported legacy context' WHERE project_id = ?").bind(project_id).execute(&mut *tx).await?;
                        tx.commit().await?;
                        return Ok(false);
                    }
                    Err(error) => return Err(error),
                };
                let current_state: String = sqlx::query_scalar("SELECT revision.state FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id AND revision.revision = entry.revision WHERE entry.project_id = ? AND entry.id = ?").bind(project_id).bind(id.as_str()).fetch_one(&mut *tx).await?;
                // Replay actual lifecycle and wording transitions in durable row order.
                // register_alias retires changed wording and never clears a real fence;
                // a metadata-only Active revision is not itself a retirement.
                let state = if historical_state != "active" || current_state != "active" {
                    BlackboardEntryState::Tombstoned
                } else {
                    BlackboardEntryState::Active
                };
                register_alias(
                    &mut tx,
                    project_id,
                    &id,
                    super::parse_kind(kind)?,
                    content,
                    state,
                    context.as_ref(),
                )
                .await?;
                last = *rowid;
            }
            if rows.len() < 64 {
                last = watermark;
            }
            sqlx::query(
                "UPDATE capture_identity_coverage SET after_rowid = ? WHERE project_id = ?",
            )
            .bind(last)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            examined += rows.len();
            if last >= watermark {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
