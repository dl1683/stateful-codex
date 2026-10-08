//! Revision-bound identities for every emitted semantic field, including legacy copies.
use super::BlackboardStoreError;
use crate::KnowledgeContext;
use crate::canonical_capture_words;
use crate::retirement_capture_words;
use sqlx::SqliteConnection;

pub(super) async fn register(
    connection: &mut SqliteConnection,
    id: &str,
    revision: i64,
    context: Option<&KnowledgeContext>,
) -> Result<(), BlackboardStoreError> {
    // Scalar guards prevent materializing unsupported legacy payloads.
    let fields: Option<(String, Option<String>, Option<String>)> = sqlx::query_as("SELECT content, structured_value, structured_unit FROM blackboard_entry_revisions WHERE entry_id = ? AND revision = ? AND octet_length(content) <= 4096 AND (structured_value IS NULL OR octet_length(structured_value) <= 2048) AND (structured_unit IS NULL OR octet_length(structured_unit) <= 128)")
        .bind(id).bind(revision).fetch_optional(&mut *connection).await?;
    let Some((content, value, unit)) = fields else {
        return Err(BlackboardStoreError::IdentityCoverageIncomplete);
    };
    let scope = context
        .and_then(|context| context.scope_id.as_deref())
        .unwrap_or("");
    let (original, retired): (String, bool) = sqlx::query_as("SELECT (SELECT provenance_kind FROM blackboard_entry_revisions WHERE entry_id = ? ORDER BY revision LIMIT 1), historical.state != 'active' OR current.state != 'active' FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS current ON current.entry_id = entry.id AND current.revision = entry.revision JOIN blackboard_entry_revisions AS historical ON historical.entry_id = entry.id AND historical.revision = ? WHERE entry.id = ?")
        .bind(id).bind(revision).bind(id).fetch_one(&mut *connection).await?;
    let authority = context.map_or(original.as_str(), |context| context.authority.as_str());
    for (field, words) in [("content", Some(content)), ("value", value), ("unit", unit)] {
        // Missing optional fields still have a revision proof; old values cannot survive
        // removal. A later writer cannot be overwritten by an older maintenance page.
        let words = words.unwrap_or_default();
        let primary = canonical_capture_words(&words);
        let retirement = retirement_capture_words(&words);
        if field != "content" {
            let category = format!("structured_{field}");
            sqlx::query("UPDATE capture_identity_aliases SET retired = 1 WHERE entry_id = ? AND category = ? AND (primary_words != ? OR ?)")
                .bind(id).bind(&category).bind(&primary).bind(retired).execute(&mut *connection).await?;
            if !words.is_empty() {
                sqlx::query("INSERT INTO capture_identity_aliases(project_id, entry_id, normalizer_version, category, authority, scope_id, primary_words, retirement_words, retired) SELECT project_id, id, ?, ?, ?, ?, ?, ?, ? FROM blackboard_entries WHERE id = ? ON CONFLICT(entry_id, primary_words, category, authority, scope_id) DO UPDATE SET retired = MAX(retired, excluded.retired)")
                    .bind(crate::CAPTURE_NORMALIZER_VERSION).bind(category).bind(authority).bind(scope)
                    .bind(&primary).bind(&retirement).bind(retired).bind(id).execute(&mut *connection).await?;
            }
        }
        sqlx::query("INSERT INTO capture_current_words(entry_id, field, revision, scope_id, primary_words, retirement_words) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(entry_id, field) DO UPDATE SET revision = excluded.revision, scope_id = excluded.scope_id, primary_words = excluded.primary_words, retirement_words = excluded.retirement_words WHERE capture_current_words.revision <= excluded.revision")
            .bind(id).bind(field).bind(revision).bind(scope)
            .bind(primary).bind(retirement)
            .execute(&mut *connection).await?;
    }
    Ok(())
}
