//! Rebuildable lexical projection; immutable seals and exclusions are its authority.
use super::BlackboardStoreError;
use sqlx::SqliteConnection;

pub(super) async fn index_chunk(
    connection: &mut SqliteConnection,
    project_id: &str,
    source_id: &str,
    start: u32,
    text: &str,
) -> Result<(), BlackboardStoreError> {
    let terms: std::collections::BTreeSet<String> = crate::retirement_capture_words(text)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    for term in terms {
        sqlx::query("INSERT OR IGNORE INTO capture_source_terms(project_id, term, source_id, start_byte) VALUES (?, ?, ?, ?)")
            .bind(project_id).bind(term).bind(source_id).bind(i64::from(start)).execute(&mut *connection).await?;
    }
    Ok(())
}
