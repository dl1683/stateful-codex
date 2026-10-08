//! Cross-chunk retirement checks over bounded, checksum-verified original source bytes.
use super::BlackboardStoreError;
use super::source::digest;
use sqlx::SqliteConnection;

pub(super) async fn range_eligible(
    connection: &mut SqliteConnection,
    project_id: &str,
    locator: &str,
    start: u32,
    end: u32,
) -> Result<bool, BlackboardStoreError> {
    let carry: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(octet_length(retirement_words)), 0) FROM capture_identity_aliases WHERE project_id = ? AND retired = 1")
        .bind(project_id).fetch_one(&mut *connection).await?;
    if carry == 0 {
        return Ok(true);
    }
    let seal = super::source::seal_on(connection, project_id, locator)
        .await?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    if seal.original_utf8_length > 65536 || start >= end || end > seal.original_utf8_length {
        return Err(BlackboardStoreError::InvalidSource);
    }
    // A 64 KiB source has at most 18 chunks with the frozen 4096/256 geometry.
    // Fetch one extra to reject malformed stores without unbounded materialization.
    let chunks: Vec<(i64, i64, Option<String>, String)> = sqlx::query_as("SELECT start_byte, end_byte, CASE WHEN octet_length(exact_bytes) <= 4096 THEN exact_bytes END, chunk_digest FROM capture_source_chunks WHERE source_id = ? ORDER BY start_byte LIMIT 19")
        .bind(locator).fetch_all(&mut *connection).await?;
    if chunks.len() > 18 {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let mut text = String::with_capacity(seal.original_utf8_length as usize);
    for (chunk_start, chunk_end, bytes, checksum) in chunks {
        let bytes = bytes.ok_or(BlackboardStoreError::InvalidSource)?;
        if chunk_start < 0
            || chunk_start > text.len() as i64
            || chunk_end > i64::from(seal.original_utf8_length)
            || chunk_end <= text.len() as i64
            || bytes.len() as i64 != chunk_end - chunk_start
            || digest(&bytes) != checksum
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let offset = text.len() - chunk_start as usize;
        let overlap = bytes
            .get(..offset)
            .ok_or(BlackboardStoreError::InvalidSource)?;
        if text.get(chunk_start as usize..) != Some(overlap) {
            return Err(BlackboardStoreError::InvalidSource);
        }
        text.push_str(
            bytes
                .get(offset..)
                .ok_or(BlackboardStoreError::InvalidSource)?,
        );
    }
    if text.len() != seal.original_utf8_length as usize
        || digest(&text) != seal.digest
        || !text.is_char_boundary(start as usize)
        || !text.is_char_boundary(end as usize)
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    // Expand the requested range to whole original tokens before normalization.
    // Carry is measured in normalized bytes, so spacing/folding cannot defeat the bound.
    let token_char =
        |ch: char| ch.is_alphanumeric() || unicode_normalization::char::is_combining_mark(ch);
    let before = text[..start as usize]
        .char_indices()
        .rev()
        .find(|(_, ch)| !token_char(*ch))
        .map_or(/*default*/ 0, |(offset, ch)| offset + ch.len_utf8());
    let after = text[end as usize..]
        .find(|ch: char| !token_char(ch))
        .map_or(text.len(), |offset| end as usize + offset);
    let normalized = crate::retirement_capture_words(&text);
    let prefix = crate::retirement_capture_words(&text[..before]);
    let through = crate::retirement_capture_words(&text[..after]);
    if !normalized.starts_with(&prefix) || !normalized.starts_with(&through) {
        // An unsupported normalization boundary cannot establish safe separation.
        return Ok(false);
    }
    let carry = usize::try_from(carry).map_err(|_| BlackboardStoreError::InvalidSource)?;
    let mut lower = prefix.len().saturating_sub(carry.saturating_add(1));
    let mut upper = through
        .len()
        .saturating_add(carry.saturating_add(1))
        .min(normalized.len());
    while !normalized.is_char_boundary(lower) {
        lower -= 1;
    }
    while !normalized.is_char_boundary(upper) {
        upper += 1;
    }
    let window = format!(" {} ", &normalized[lower..upper]);
    let excluded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_identity_aliases WHERE project_id = ? AND retired = 1 AND retirement_words != '' AND instr(?, ' ' || retirement_words || ' ') > 0)")
        .bind(project_id).bind(window).fetch_one(connection).await?;
    Ok(!excluded)
}
