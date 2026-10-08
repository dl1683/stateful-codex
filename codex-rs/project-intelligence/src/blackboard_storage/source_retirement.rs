//! Short-source retirement checks; long unproven enclosures are excluded automatically.
use super::BlackboardStoreError;
use super::source::SourceBudget;
use super::source::digest;
use sqlx::SqliteConnection;

pub(super) async fn range_eligible(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
    start: u32,
    end: u32,
    budget: &mut SourceBudget,
) -> Result<bool, BlackboardStoreError> {
    if !super::identity::coverage_available(connection, project_id).await? {
        return Ok(false);
    }
    let retired: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_identity_aliases WHERE project_id = ? AND retired = 1)")
        .bind(project_id).fetch_one(&mut *connection).await?;
    if !retired {
        return Ok(true);
    }
    let seal = super::source::seal_on(connection, project_id, locator)
        .await?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    if start >= end || end > seal.original_utf8_length {
        return Err(BlackboardStoreError::InvalidSource);
    }
    // The whole-part reconstruction path is physically removed. Without persisted
    // retirement coverage, any long enclosure could conceal a cross-chunk alias.
    if seal.original_utf8_length > 4096 {
        return Ok(false);
    }
    budget.charge(/*units*/ 1)?;
    let chunk: Option<(i64, Option<String>, String)> = sqlx::query_as("SELECT end_byte, CASE WHEN octet_length(exact_bytes) <= 4096 THEN exact_bytes END, chunk_digest FROM capture_source_chunks WHERE source_id = ? AND start_byte = 0 LIMIT 1")
        .bind(locator).fetch_optional(&mut *connection).await?;
    let (length, text, checksum) = chunk.ok_or(BlackboardStoreError::InvalidSource)?;
    let text = text.ok_or(BlackboardStoreError::InvalidSource)?;
    if length != i64::from(seal.original_utf8_length)
        || text.len() as i64 != length
        || digest(&text) != checksum
        || checksum != seal.digest
        || !text.is_char_boundary(start as usize)
        || !text.is_char_boundary(end as usize)
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    super::identity::text_eligible(connection, project_id, "", &text).await
}
