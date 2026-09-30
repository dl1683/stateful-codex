use std::borrow::Cow;

use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::Migrator;
use sqlx_sqlite::SqlitePool;

pub(crate) static STATE_MIGRATOR: Migrator = sqlx_macros::migrate!("./migrations");
pub(crate) static LOGS_MIGRATOR: Migrator = sqlx_macros::migrate!("./logs_migrations");
pub(crate) static GOALS_MIGRATOR: Migrator = sqlx_macros::migrate!("./goals_migrations");
pub(crate) static MEMORIES_MIGRATOR: Migrator = sqlx_macros::migrate!("./memory_migrations");
pub(crate) static QUEUE_MIGRATOR: Migrator = sqlx_macros::migrate!("./queue_migrations");
pub(crate) static THREAD_HISTORY_MIGRATOR: Migrator =
    sqlx_macros::migrate!("./thread_history_migrations");

/// Allow an older Codex binary to open a database that has already been
/// migrated by a newer binary running in parallel.
///
/// We intentionally ignore applied migration versions that are newer than the
/// embedded migration set. Known migration versions are still validated by
/// checksum, so this only relaxes the "database is ahead of me" case.
///
/// This does not make migration compatibility bidirectional. The legacy fork
/// version-56 owner-token row is repaired to version 59 before migration; an
/// older binary will therefore reject the upgraded database because it still
/// expects that row at version 56. The supported coexistence window is an old
/// binary opening an old database before the upgrade and the new binary
/// completing that upgrade. Rolling back to, or concurrently using, the old
/// binary after repair is unsupported.
fn runtime_migrator(base: &'static Migrator) -> Migrator {
    Migrator {
        migrations: Cow::Borrowed(base.migrations.as_ref()),
        ignore_missing: true,
        locking: base.locking,
        no_tx: base.no_tx,
        table_name: base.table_name.clone(),
        create_schemas: base.create_schemas.clone(),
    }
}

pub(crate) fn runtime_state_migrator() -> Migrator {
    runtime_migrator(&STATE_MIGRATOR)
}

pub(crate) fn runtime_logs_migrator() -> Migrator {
    runtime_migrator(&LOGS_MIGRATOR)
}

pub(crate) fn runtime_goals_migrator() -> Migrator {
    runtime_migrator(&GOALS_MIGRATOR)
}

pub(crate) fn runtime_memories_migrator() -> Migrator {
    runtime_migrator(&MEMORIES_MIGRATOR)
}

pub(crate) fn runtime_queue_migrator() -> Migrator {
    runtime_migrator(&QUEUE_MIGRATOR)
}

// The paginated history projector will call this when it takes ownership of opening the database.
#[allow(dead_code)]
pub(crate) fn runtime_thread_history_migrator() -> Migrator {
    runtime_migrator(&THREAD_HISTORY_MIGRATOR)
}

pub(crate) fn checksum_is_line_ending_equivalent(
    migration: &Migration,
    applied_checksum: &[u8],
) -> bool {
    let lf_sql = migration.sql.as_str().replace("\r\n", "\n");
    let crlf_sql = lf_sql.replace('\n', "\r\n");
    [lf_sql, crlf_sql].into_iter().any(|sql| {
        Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            AssertSqlSafe(sql).into_sql_str(),
            migration.no_tx,
        )
        .checksum
        .as_ref()
            == applied_checksum
    })
}

pub(crate) async fn repair_legacy_recency_migration_version(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    let Some(recency_migration) = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == 39)
    else {
        return Ok(());
    };
    let migrations_table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !migrations_table_exists {
        return Ok(());
    }

    let legacy_checksum = sqlx::query_scalar::<_, Vec<u8>>(
        r#"
SELECT checksum
FROM _sqlx_migrations
WHERE version = ?
  AND NOT EXISTS (
      SELECT 1 FROM _sqlx_migrations WHERE version = ?
  )
        "#,
    )
    .bind(38_i64)
    .bind(recency_migration.version)
    .fetch_optional(pool)
    .await?;
    let Some(legacy_checksum) = legacy_checksum else {
        return Ok(());
    };
    if !checksum_is_line_ending_equivalent(recency_migration, &legacy_checksum) {
        return Ok(());
    }

    sqlx::query(
        r#"
UPDATE _sqlx_migrations
SET version = ?, description = ?
WHERE version = ?
  AND checksum = ?
  AND NOT EXISTS (
      SELECT 1 FROM _sqlx_migrations WHERE version = ?
  )
        "#,
    )
    .bind(recency_migration.version)
    .bind(recency_migration.description.as_ref())
    .bind(38_i64)
    .bind(legacy_checksum)
    .bind(recency_migration.version)
    .execute(pool)
    .await?;
    Ok(())
}

/// Reconcile the fork's unreleased version-56 owner-token migration with the
/// upstream version-56 creator-identity migration.
///
/// The fork migration is preserved semantically as version 59. A database
/// that already applied the fork migration has the owner-token column and a
/// version-56 row but no creator columns; move that row to 59 after replaying
/// its reset so upstream 56-58 can run normally. This is an explicit
/// compatibility repair, not a silent rewrite of a released migration.
pub(crate) async fn repair_legacy_backfill_owner_migration_version(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    let Some(owner_migration) = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == 59)
    else {
        return Ok(());
    };
    let migration_table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !migration_table_exists {
        return Ok(());
    }
    let has_owner_token = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM pragma_table_info('backfill_state') WHERE name = 'owner_token'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    let has_creator_identity = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM pragma_table_info('threads') WHERE name = 'creator_user_id'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !has_owner_token || has_creator_identity {
        return Ok(());
    }
    let legacy_checksum = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT checksum FROM _sqlx_migrations WHERE version = 56 AND success = 1",
    )
    .fetch_optional(pool)
    .await?;
    let Some(legacy_checksum) = legacy_checksum else {
        return Ok(());
    };
    if !checksum_is_line_ending_equivalent(owner_migration, &legacy_checksum) {
        return Ok(());
    }

    sqlx::query(
        "UPDATE backfill_state SET status = 'pending', last_watermark = NULL, last_success_at = NULL, owner_token = NULL, updated_at = CAST(strftime('%s', 'now') AS INTEGER) WHERE id = 1",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE _sqlx_migrations SET version = ?, description = ?, checksum = ? WHERE version = 56 AND success = 1 AND checksum = ? AND NOT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = ?)",
    )
    .bind(owner_migration.version)
    .bind(owner_migration.description.as_ref())
    .bind(owner_migration.checksum.to_vec())
    .bind(legacy_checksum)
    .bind(owner_migration.version)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "migrations_tests.rs"]
mod tests;
