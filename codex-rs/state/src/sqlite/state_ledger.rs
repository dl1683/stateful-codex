//! Strict typed migration-ledger reads on the caller's SQLite snapshot.
//! Identity, prefix, and schema decisions remain the caller's responsibility.

use anyhow::ensure;
use sqlx::AssertSqlSafe;
use sqlx::Row;
use sqlx::SqliteConnection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Ledger {
    Canonical,
    Stateful,
}

impl Ledger {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Canonical => "_sqlx_migrations",
            Self::Stateful => "_stateful_migrations",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AppliedRecord {
    pub version: i64,
    pub description: String,
    pub installed_on: String,
    pub success: i64,
    pub checksum: Vec<u8>,
    pub execution_time: i64,
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    ledger: Ledger,
) -> anyhow::Result<Option<Vec<AppliedRecord>>> {
    let name = ledger.name();
    if !super::state_metadata::validate(connection, super::state_metadata::Metadata::Ledger(ledger))
        .await?
    {
        return Ok(None);
    }
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT type FROM sqlite_schema WHERE name = ? COLLATE NOCASE ORDER BY type",
    )
    .bind(name)
    .fetch_all(&mut *connection)
    .await?;
    if kinds.is_empty() {
        return Ok(None);
    }
    ensure!(
        kinds == ["table"],
        "state migration ledger {name} identity is ambiguous or not a table"
    );
    let columns: Vec<(String, String, i64, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?) ORDER BY cid",
    ).bind(name).fetch_all(&mut *connection).await?;
    let expected = [
        ("version", "BIGINT", 0, None, 1, 0),
        ("description", "TEXT", 1, None, 0, 0),
        (
            "installed_on",
            "TIMESTAMP",
            1,
            Some("CURRENT_TIMESTAMP"),
            0,
            0,
        ),
        ("success", "BOOLEAN", 1, None, 0, 0),
        ("checksum", "BLOB", 1, None, 0, 0),
        ("execution_time", "BIGINT", 1, None, 0, 0),
    ]
    .map(|(name, ty, required, default, pk, hidden)| {
        (
            name.to_owned(),
            ty.to_owned(),
            required,
            default.map(str::to_owned),
            pk,
            hidden,
        )
    });
    ensure!(
        columns == expected,
        "unsupported migration ledger layout: {name}"
    );
    let triggered: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ? COLLATE NOCASE)")
        .bind(name).fetch_one(&mut *connection).await?;
    ensure!(!triggered, "unexpected migration ledger trigger: {name}");
    // The interpolated identifier comes only from the closed Ledger enum.
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT version, description, installed_on, success, checksum, execution_time, typeof(version) AS vt, typeof(description) AS dt, typeof(installed_on) AS it, typeof(success) AS st, typeof(checksum) AS ct, typeof(execution_time) AS et FROM {name} ORDER BY version"
    ))).fetch_all(&mut *connection).await?;
    let mut applied = Vec::with_capacity(rows.len());
    let mut previous = 0;
    for row in rows {
        for (field, expected) in [
            ("vt", "integer"),
            ("dt", "text"),
            ("it", "text"),
            ("st", "integer"),
            ("ct", "blob"),
            ("et", "integer"),
        ] {
            ensure!(
                row.try_get::<String, _>(field)? == expected,
                "unsupported migration storage class in {name}: {field}"
            );
        }
        let version: i64 = row.try_get("version")?;
        ensure!(
            version > previous,
            "invalid or duplicate migration version in {name}: {version}"
        );
        let success: i64 = row.try_get("success")?;
        ensure!(
            success == 1,
            "unsuccessful state migration {version} in {name}"
        );
        applied.push(AppliedRecord {
            version,
            description: row.try_get("description")?,
            installed_on: row.try_get("installed_on")?,
            success,
            checksum: row.try_get("checksum")?,
            execution_time: row.try_get("execution_time")?,
        });
        previous = version;
    }
    Ok(Some(applied))
}

#[cfg(test)]
#[path = "state_ledger_tests.rs"]
mod tests;
