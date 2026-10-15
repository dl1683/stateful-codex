//! Qualification prototype for state migration identities, isolated from startup.
//!
//! This is a qualification preflight, not authorization to transfer legacy facts.
//! Schema-backed restart qualification and atomic transfer must precede
//! production activation.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::bail;
use anyhow::ensure;
use sqlx::Row;
use sqlx::SqliteConnection;
use sqlx::migrate::Migration;

use super::state_catalog;
use crate::migrations::checksum_is_line_ending_equivalent;

use super::SqliteConfig;

pub(super) async fn check(config: &SqliteConfig, path: &Path) -> anyhow::Result<()> {
    match tokio::fs::metadata(path).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let pool = config
        .open_read_only_pool(path, /*busy_timeout*/ None)
        .await?;
    let result = async {
        let mut transaction = pool.begin().await?;
        validate(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

/// Check the ledger within the caller's coherent SQLite snapshot.
async fn validate(connection: &mut SqliteConnection) -> anyhow::Result<()> {
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT type FROM sqlite_schema WHERE name = '_sqlx_migrations' COLLATE NOCASE ORDER BY type")
            .fetch_all(&mut *connection)
            .await?;
    if kinds.is_empty() {
        let populated: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*')",
        )
        .fetch_one(&mut *connection)
        .await?;
        ensure!(
            !populated,
            "state migration ledger missing for nonempty database"
        );
        return Ok(());
    }
    ensure!(
        kinds == ["table"],
        "state migration ledger identity is ambiguous or not a table"
    );
    let columns: Vec<(String, String, i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_xinfo('_sqlx_migrations') ORDER BY cid",
    )
    .fetch_all(&mut *connection)
    .await?;
    let expected = [
        ("version", "BIGINT", 0, None, 1),
        ("description", "TEXT", 1, None, 0),
        ("installed_on", "TIMESTAMP", 1, Some("CURRENT_TIMESTAMP"), 0),
        ("success", "BOOLEAN", 1, None, 0),
        ("checksum", "BLOB", 1, None, 0),
        ("execution_time", "BIGINT", 1, None, 0),
    ]
    .map(|(name, ty, required, default, pk)| {
        (
            name.to_owned(),
            ty.to_owned(),
            required,
            default.map(str::to_owned),
            pk,
        )
    });
    ensure!(
        columns == expected,
        "unsupported state migration ledger layout"
    );
    let hidden: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo('_sqlx_migrations') WHERE hidden != 0)",
    )
    .fetch_one(&mut *connection)
    .await?;
    ensure!(
        !hidden,
        "unsupported generated state migration ledger columns"
    );
    let triggers: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = '_sqlx_migrations')",
    )
    .fetch_one(&mut *connection)
    .await?;
    ensure!(!triggers, "unexpected state migration ledger trigger");

    let rows = sqlx::query(
        "SELECT version, checksum, typeof(version) AS vt, typeof(description) AS dt, typeof(installed_on) AS it, typeof(success) AS st, success, typeof(checksum) AS ct, typeof(execution_time) AS et FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut canonical = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut owner = false;
    let mut binding = false;
    let mut legacy_recency = false;
    let mut opaque = false;
    for row in rows {
        for (field, expected_type) in [
            ("vt", "integer"),
            ("dt", "text"),
            ("it", "text"),
            ("st", "integer"),
            ("ct", "blob"),
            ("et", "integer"),
        ] {
            ensure!(
                row.try_get::<String, _>(field)? == expected_type,
                "invalid state migration row storage class: {field}"
            );
        }
        let version: i64 = row.try_get("version")?;
        ensure!(
            version > 0 && seen.insert(version),
            "invalid or duplicate state migration version {version}"
        );
        ensure!(
            row.try_get::<i64, _>("success")? == 1,
            "unsuccessful state migration {version}"
        );
        let checksum: Vec<u8> = row.try_get("checksum")?;
        let matching = |candidate: &Migration| {
            candidate.checksum.as_ref() == checksum
                || checksum_is_line_ending_equivalent(candidate, &checksum)
        };
        let fork_owner = state_catalog::fork(/*version*/ 59);
        let fork_binding = state_catalog::fork(/*version*/ 60);
        if matches!(version, 56 | 59) && fork_owner.as_ref().is_some_and(matching) {
            ensure!(!owner, "duplicate fork owner migration fact");
            owner = true;
        } else if version == 60 && fork_binding.as_ref().is_some_and(matching) {
            binding = true;
        } else if version == 38
            && state_catalog::canonical(/*version*/ 39)
                .as_ref()
                .is_some_and(matching)
        {
            legacy_recency = true;
            canonical.insert(39);
        } else if version > 60 {
            opaque = true;
        } else {
            let candidate = state_catalog::canonical(version);
            if !candidate.as_ref().is_some_and(matching) {
                return Err(sqlx::migrate::MigrateError::VersionMismatch(version).into());
            }
            ensure!(
                canonical.insert(version),
                "duplicate canonical state migration fact {version}"
            );
        }
    }
    ensure!(
        !binding || owner,
        "fork binding migration without owner fact"
    );
    ensure!(
        !legacy_recency || !seen.contains(&39),
        "duplicate recency migration fact"
    );
    let last = canonical.last().copied().unwrap_or_default();
    for version in 1..=last {
        ensure!(
            canonical.contains(&version) || (version == 38 && canonical.contains(&39)),
            "state migration prefix missing version {version}"
        );
    }
    if owner {
        ensure!(
            last >= 55,
            "fork owner fact without required canonical prefix"
        );
    }
    ensure!(
        !binding || (1..=58).all(|version| canonical.contains(&version)),
        "fork binding fact without complete canonical prefix through 58"
    );
    if opaque && (canonical.len() != 60 || owner || binding || legacy_recency) {
        bail!(
            "opaque future state migrations require a complete canonical prefix without legacy transfer"
        );
    }
    if seen.is_empty() {
        let populated: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name != '_sqlx_migrations' AND name NOT GLOB 'sqlite_*')",
        ).fetch_one(&mut *connection).await?;
        ensure!(
            !populated,
            "empty state migration ledger for nonempty database"
        );
    }
    let fork = [(59, owner), (60, binding)]
        .into_iter()
        .filter_map(|(version, applied)| applied.then_some(version))
        .collect();
    super::state_schema::validate(
        connection,
        &super::state_schema::AppliedSchema {
            canonical,
            fork,
            opaque_suffix: opaque,
        },
    )
    .await
}

#[cfg(test)]
#[path = "state_preflight_tests.rs"]
mod tests;
