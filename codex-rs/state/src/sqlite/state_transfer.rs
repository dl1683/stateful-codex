//! Stages legacy facts within the caller's immediate migration transaction.
//! This component neither commits nor authorizes opening a reconciled store.

use std::borrow::Cow;

use anyhow::ensure;
use sqlx::AssertSqlSafe;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::Transaction;
use sqlx::migrate::Migrate;
use sqlx::migrate::Migration;
use sqlx::migrate::Migrator;

use super::Destination;
use super::LegacyHistory;
use super::state_catalog;
use super::state_ledger::Ledger;
use super::state_ledger::{self};
use crate::migrations::checksum_is_line_ending_equivalent;

pub(super) static FORK_MIGRATOR: Migrator = sqlx_macros::migrate!("./stateful_migrations");

struct Target {
    ledger: Ledger,
    version: i64,
    identity: Migration,
}

fn target(destination: &Destination) -> anyhow::Result<Target> {
    let (ledger, version, identity) = match destination {
        Destination::CanonicalRecency => (
            Ledger::Canonical,
            39,
            state_catalog::canonical(/*version*/ 39),
        ),
        Destination::StatefulOwner => (Ledger::Stateful, 1, state_catalog::fork(/*version*/ 59)),
        Destination::StatefulBinding => (Ledger::Stateful, 2, state_catalog::fork(/*version*/ 60)),
    };
    Ok(Target {
        ledger,
        version,
        identity: identity.ok_or_else(|| anyhow::anyhow!("missing pinned transfer identity"))?,
    })
}

/// Caller must classify under this same BEGIN IMMEDIATE and finish both runners
/// before committing. Errors must roll back the caller's complete transaction.
pub(super) async fn stage(
    transaction: &mut Transaction<'_, Sqlite>,
    history: &LegacyHistory,
) -> anyhow::Result<()> {
    if history.transfers.is_empty() {
        return Ok(());
    }
    let connection = &mut **transaction;
    let current = state_ledger::read(connection, Ledger::Canonical)
        .await?
        .unwrap_or_default();
    ensure!(
        current == history.original,
        "migration facts changed after classification"
    );
    let reserved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name COLLATE NOCASE IN ('_stateful_migrations', '_stateful_migration_archive'))")
        .fetch_one(&mut *connection).await?;
    ensure!(
        !reserved,
        "legacy staging cannot adopt an existing private namespace"
    );
    let mut destinations = Vec::new();
    for transfer in &history.transfers {
        let target = target(&transfer.destination)?;
        let source = &transfer.source;
        let allowed = match transfer.destination {
            Destination::CanonicalRecency => source.version == 38,
            Destination::StatefulOwner => matches!(source.version, 56 | 59),
            Destination::StatefulBinding => source.version == 60,
        };
        ensure!(
            allowed
                && current.contains(source)
                && (target.identity.checksum.as_ref() == source.checksum
                    || checksum_is_line_ending_equivalent(&target.identity, &source.checksum)),
            "unsupported legacy transfer fact"
        );
        ensure!(
            !destinations.contains(&(target.ledger, target.version)),
            "duplicate transfer destination"
        );
        destinations.push((target.ledger, target.version));
    }
    connection
        .ensure_migrations_table(Ledger::Stateful.name())
        .await?;
    sqlx::query("CREATE TABLE _stateful_migration_archive (source_version BIGINT PRIMARY KEY, destination_ledger TEXT NOT NULL, destination_version BIGINT NOT NULL, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)")
        .execute(&mut *connection).await?;
    for transfer in &history.transfers {
        let target = target(&transfer.destination)?;
        let source = &transfer.source;
        let ledger = target.ledger.name();
        sqlx::query("INSERT INTO _stateful_migration_archive VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(source.version)
            .bind(ledger)
            .bind(target.version)
            .bind(&source.description)
            .bind(&source.installed_on)
            .bind(source.success)
            .bind(&source.checksum)
            .bind(source.execution_time)
            .execute(&mut *connection)
            .await?;
        sqlx::query(AssertSqlSafe(format!("INSERT INTO {ledger} (version, description, installed_on, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?, ?)")))
            .bind(target.version).bind(&source.description).bind(&source.installed_on).bind(source.success)
            .bind(&source.checksum).bind(source.execution_time).execute(&mut *connection).await?;
        let deleted =
            sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ? AND checksum = ?")
                .bind(source.version)
                .bind(&source.checksum)
                .execute(&mut *connection)
                .await?;
        ensure!(
            deleted.rows_affected() == 1,
            "legacy source fact disappeared during transfer"
        );
    }
    Ok(())
}

/// Run a pinned application range on the held connection, adapting only proven
/// LF/CRLF checksums in memory. SQLx nests its per-migration savepoints here.
pub(super) async fn run(
    connection: &mut SqliteConnection,
    base: &Migrator,
    ledger: Ledger,
) -> anyhow::Result<()> {
    ensure!(
        !base.no_tx && base.iter().all(|migration| !migration.no_tx),
        "nontransactional migrations cannot join namespace conversion"
    );
    for migration in base.iter() {
        let identity = match (ledger, migration.version) {
            (Ledger::Canonical, version) => state_catalog::canonical(version),
            (Ledger::Stateful, 1) => state_catalog::fork(/*version*/ 59),
            (Ledger::Stateful, 2) => state_catalog::fork(/*version*/ 60),
            (Ledger::Stateful, _) => None,
        };
        ensure!(
            identity
                .as_ref()
                .is_some_and(|identity| identity.sql.as_str().replace("\r\n", "\n")
                    == migration.sql.as_str().replace("\r\n", "\n")
                    && (identity.checksum == migration.checksum
                        || checksum_is_line_ending_equivalent(
                            identity,
                            migration.checksum.as_ref()
                        ))),
            "application range contains an unpinned migration"
        );
    }
    let applied = state_ledger::read(connection, ledger)
        .await?
        .unwrap_or_default();
    let mut migrations = base.migrations.to_vec();
    for migration in &mut migrations {
        if let Some(record) = applied
            .iter()
            .find(|record| record.version == migration.version)
            && migration.checksum.as_ref() != record.checksum
            && checksum_is_line_ending_equivalent(migration, &record.checksum)
        {
            migration.checksum = Cow::Owned(record.checksum.clone());
        }
    }
    let compatible = Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: match ledger {
            Ledger::Canonical => base.ignore_missing,
            Ledger::Stateful => false,
        },
        locking: base.locking,
        no_tx: base.no_tx,
        table_name: ledger.name().into(),
        create_schemas: base.create_schemas.clone(),
    };
    compatible.run(&mut *connection).await?;
    Ok(())
}

#[cfg(test)]
#[path = "state_transfer_tests.rs"]
mod tests;
