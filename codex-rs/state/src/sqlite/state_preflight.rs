//! Qualification prototype for state migration identities, isolated from startup.
//!
//! This is a qualification preflight, not authorization to transfer legacy facts.
//! Schema-backed restart qualification and atomic transfer must precede
//! production activation.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::bail;
use anyhow::ensure;
use sqlx::SqliteConnection;
use sqlx::migrate::Migration;

use super::state_catalog;
use super::state_ledger::AppliedRecord;
use super::state_ledger::Ledger;
use super::state_ledger::{self};
use crate::migrations::checksum_is_line_ending_equivalent;

use super::SqliteConfig;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Destination {
    CanonicalRecency,
    StatefulOwner,
    StatefulBinding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// An approved legacy fact with its original row, before any ledger relocation.
pub(super) struct LegacyTransfer {
    pub source: AppliedRecord,
    pub destination: Destination,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Retains all original facts and the explicit moves required for this legacy store.
pub(super) struct LegacyHistory {
    pub original: Vec<AppliedRecord>,
    pub transfers: Vec<LegacyTransfer>,
}

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
async fn validate(connection: &mut SqliteConnection) -> anyhow::Result<LegacyHistory> {
    let reserved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name COLLATE NOCASE IN ('_stateful_migrations', '_stateful_migration_archive'))")
        .fetch_one(&mut *connection).await?;
    ensure!(
        !reserved,
        "reconciled namespace requires its dedicated classifier"
    );
    let Some(rows) = state_ledger::read(connection, Ledger::Canonical).await? else {
        let populated: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*')",
        )
        .fetch_one(&mut *connection)
        .await?;
        ensure!(
            !populated,
            "state migration ledger missing for nonempty database"
        );
        return Ok(LegacyHistory::default());
    };
    let mut transfers = Vec::new();
    let mut canonical = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut owner = false;
    let mut binding = false;
    let mut legacy_recency = false;
    let mut opaque = false;
    for row in &rows {
        let version = row.version;
        seen.insert(version);
        let checksum = row.checksum.as_slice();
        let matching = |candidate: &Migration| {
            candidate.checksum.as_ref() == checksum
                || checksum_is_line_ending_equivalent(candidate, checksum)
        };
        let fork_owner = state_catalog::fork(/*version*/ 59);
        let fork_binding = state_catalog::fork(/*version*/ 60);
        if matches!(version, 56 | 59) && fork_owner.as_ref().is_some_and(matching) {
            ensure!(!owner, "duplicate fork owner migration fact");
            owner = true;
            transfers.push(LegacyTransfer {
                source: row.clone(),
                destination: Destination::StatefulOwner,
            });
        } else if version == 60 && fork_binding.as_ref().is_some_and(matching) {
            binding = true;
            transfers.push(LegacyTransfer {
                source: row.clone(),
                destination: Destination::StatefulBinding,
            });
        } else if version == 38
            && state_catalog::canonical(/*version*/ 39)
                .as_ref()
                .is_some_and(matching)
        {
            legacy_recency = true;
            transfers.push(LegacyTransfer {
                source: row.clone(),
                destination: Destination::CanonicalRecency,
            });
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
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE NOT (type = 'table' AND name = '_sqlx_migrations' COLLATE NOCASE) AND name NOT GLOB 'sqlite_*')",
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
    .await?;
    Ok(LegacyHistory {
        original: rows,
        transfers,
    })
}

#[cfg(test)]
#[path = "state_preflight_tests.rs"]
mod tests;

#[path = "state_transfer.rs"]
mod transfer;

#[path = "state_archive.rs"]
mod archive;

#[path = "state_modern.rs"]
mod modern;

#[path = "state_projection_tokens.rs"]
mod projection_tokens;
