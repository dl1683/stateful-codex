//! Closed metadata-table grammar and index ownership for namespace qualification.
//! This validates structure only; callers must separately prove row identities.

use anyhow::ensure;
use sqlx::SqliteConnection;

use super::state_ledger::Ledger;
use super::state_schema::lexer;

#[derive(Clone, Copy)]
pub(super) enum Metadata {
    Ledger(Ledger),
    Archive,
}

impl Metadata {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Ledger(ledger) => ledger.name(),
            Self::Archive => "_stateful_migration_archive",
        }
    }

    fn definition(self) -> String {
        let name = self.name();
        match self {
            Self::Ledger(_) => format!(
                "CREATE TABLE {name} (version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)"
            ),
            Self::Archive => format!(
                "CREATE TABLE {name} (source_version BIGINT PRIMARY KEY, destination_ledger TEXT NOT NULL, destination_version BIGINT NOT NULL, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)"
            ),
        }
    }
}

/// Accept only the trusted grammar, allowing ASCII case and table-name quoting.
/// There are no string literals in either trusted definition. Keeping quoted
/// identifiers distinct from unquoted keywords prevents quoted constraint words
/// from acquiring a different meaning during comparison.
fn signature(sql: &str) -> anyhow::Result<Vec<String>> {
    let mut tokens = lexer::tokens(sql)?;
    if tokens.last().is_some_and(|token| token == ";") {
        tokens.pop();
    }
    Ok(tokens
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect())
}

pub(super) async fn validate(
    connection: &mut SqliteConnection,
    metadata: Metadata,
) -> anyhow::Result<bool> {
    let name = metadata.name();
    // Bound SQL before fetching/tokenizing it. Reserved names share one policy
    // across tables, views, triggers and indexes, including case variants.
    let objects: Vec<(String, Option<i64>)> = sqlx::query_as(
        "SELECT type, length(CAST(sql AS BLOB)) FROM sqlite_schema WHERE name = ? COLLATE NOCASE ORDER BY type",
    ).bind(name).fetch_all(&mut *connection).await?;
    if objects.is_empty() {
        return Ok(false);
    }
    ensure!(
        objects.len() == 1 && objects[0].0 == "table",
        "state migration ledger {name} identity is ambiguous or not a table"
    );
    ensure!(
        objects[0].1.is_some_and(|length| length <= 4096),
        "unsupported migration metadata definition size: {name}"
    );
    let definition: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ? COLLATE NOCASE",
    )
    .bind(name)
    .fetch_one(&mut *connection)
    .await?;
    let mut actual = signature(&definition)?;
    let expected = signature(&metadata.definition())?;
    // SQLite quotes table names after ALTER TABLE RENAME. Only the table-name
    // token has a closed, independently checked identifier substitution.
    if let Some(table) = actual.get_mut(2) {
        let identifier = table
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                table
                    .strip_prefix('`')
                    .and_then(|value| value.strip_suffix('`'))
            })
            .or_else(|| {
                table
                    .strip_prefix('[')
                    .and_then(|value| value.strip_suffix(']'))
            })
            .unwrap_or(table);
        ensure!(
            identifier == name,
            "unexpected migration metadata table name"
        );
        *table = name.to_owned();
    }
    ensure!(
        actual == expected,
        "unsupported migration metadata SQL: {name}"
    );
    let triggers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ? COLLATE NOCASE",
    )
    .bind(name)
    .fetch_one(&mut *connection)
    .await?;
    ensure!(triggers == 0, "unexpected migration ledger trigger: {name}");
    let indexes: Vec<(String, i64, String, i64)> = sqlx::query_as(
        "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?) ORDER BY seq",
    )
    .bind(name)
    .fetch_all(&mut *connection)
    .await?;
    ensure!(
        indexes.len() == 1 && indexes[0].1 == 1 && indexes[0].2 == "pk" && indexes[0].3 == 0,
        "unexpected migration metadata indexes: {name}"
    );
    let key = match metadata {
        Metadata::Ledger(_) => "version",
        Metadata::Archive => "source_version",
    };
    let columns: Vec<(i64, i64, Option<String>, i64, String, i64)> = sqlx::query_as(
        "SELECT seqno, cid, name, desc, coll, key FROM pragma_index_xinfo(?) ORDER BY seqno",
    )
    .bind(&indexes[0].0)
    .fetch_all(&mut *connection)
    .await?;
    ensure!(
        columns
            == [
                (0, 0, Some(key.to_owned()), 0, "BINARY".to_owned(), 1),
                (1, -1, None, 0, "BINARY".to_owned(), 0)
            ],
        "unexpected migration metadata primary key: {name}"
    );
    Ok(true)
}

#[cfg(test)]
#[path = "state_metadata_tests.rs"]
mod tests;
