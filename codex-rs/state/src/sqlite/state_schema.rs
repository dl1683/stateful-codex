//! Qualify cumulative schema effects without writing to the source database.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::ensure;
use sqlx::Connection;
use sqlx::SqliteConnection;

use super::state_catalog;

#[path = "state_schema_tokens.rs"]
mod lexer;

pub(super) struct AppliedSchema {
    pub canonical: BTreeSet<i64>,
    pub fork: BTreeSet<i64>,
    pub opaque_suffix: bool,
}

pub(super) async fn validate(
    connection: &mut SqliteConnection,
    history: &AppliedSchema,
) -> anyhow::Result<()> {
    if history.opaque_suffix {
        let collisions: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM pragma_table_xinfo('backfill_state') WHERE name = 'owner_token' COLLATE NOCASE) + (SELECT count(*) FROM pragma_table_xinfo('threads') WHERE name = 'project_binding_generation' COLLATE NOCASE) + (SELECT count(*) FROM sqlite_schema WHERE name = 'threads_project_binding_generation' COLLATE NOCASE)",
        ).fetch_one(&mut *connection).await?;
        ensure!(
            collisions == 0,
            "unexplained fork-owned schema in newer canonical state database"
        );
        return Ok(());
    }
    let mut reference = SqliteConnection::connect("sqlite::memory:")
        .await
        .map_err(|error| anyhow::anyhow!("trusted state reference open failed: {error}"))?;
    let expected = async {
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&mut reference)
            .await?;
        // This ordering is qualified only for the two independent legacy fork
        // additions. It is not an ordering contract for future fork migrations.
        for version in &history.canonical {
            let migration = state_catalog::canonical(*version)
                .ok_or_else(|| anyhow::anyhow!("canonical catalog missing version {version}"))?;
            ensure!(!migration.no_tx, "nontransactional state schema source");
            sqlx::raw_sql(migration.sql).execute(&mut reference).await?;
        }
        for version in &history.fork {
            let migration = state_catalog::fork(*version)
                .ok_or_else(|| anyhow::anyhow!("fork catalog missing version {version}"))?;
            ensure!(!migration.no_tx, "nontransactional fork schema source");
            sqlx::raw_sql(migration.sql).execute(&mut reference).await?;
        }
        schema(&mut reference).await
    }
    .await;
    let closed = reference.close().await;
    // A failure in disposable reference reconstruction is not corruption of the
    // original store: retain text, not a misleading SQLx corruption source chain.
    let expected = expected.map_err(|error| {
        anyhow::anyhow!("trusted state reference reconstruction failed: {error:#}")
    })?;
    closed.map_err(|error| anyhow::anyhow!("trusted state reference close failed: {error}"))?;
    let actual = schema(connection).await?;
    for (key, definition) in &expected {
        ensure!(
            actual.get(key) == Some(definition),
            "state schema mismatch for {} {}",
            key.0,
            key.1
        );
    }
    ensure!(
        actual.len() == expected.len(),
        "unexplained schema objects in state database"
    );
    Ok(())
}

type Schema = BTreeMap<(String, String, String), Vec<String>>;

async fn schema(connection: &mut SqliteConnection) -> anyhow::Result<Schema> {
    // Bound this collector before fetching SQL strings or allocating tokens.
    // These are qualification limits, not limits on opaque future schemas.
    let (count, total, largest): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(length(CAST(sql AS BLOB))), 0), COALESCE(max(length(CAST(sql AS BLOB))), 0) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND NOT (type = 'table' AND name = '_sqlx_migrations' COLLATE NOCASE) AND sql IS NOT NULL",
    ).fetch_one(&mut *connection).await?;
    ensure!(
        count <= 128 && total <= 1_048_576 && largest <= 65_536,
        "state schema exceeds qualification collection bounds"
    );
    let objects: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND NOT (type = 'table' AND name = '_sqlx_migrations' COLLATE NOCASE) AND sql IS NOT NULL ORDER BY type, name",
    ).fetch_all(connection).await?;
    objects
        .into_iter()
        .map(|(kind, name, table, sql)| Ok(((kind, name, table), lexer::tokens(&sql)?)))
        .collect()
}
