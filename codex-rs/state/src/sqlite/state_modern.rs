//! Known private-namespace qualification on the caller's coherent snapshot.
//! Opaque future schema and production startup are separate qualification steps.

use std::collections::BTreeSet;

use anyhow::ensure;
use sqlx::SqliteConnection;

use super::archive;
use super::archive::ArchivedFact;
use super::state_catalog;
use super::state_ledger::AppliedRecord;
use super::state_ledger::Ledger;
use super::state_ledger::{self};
use crate::migrations::checksum_is_line_ending_equivalent;
use crate::sqlite::state_schema;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PrivateHistory {
    pub canonical: Vec<AppliedRecord>,
    pub private: Vec<AppliedRecord>,
    pub archive: Option<Vec<ArchivedFact>>,
}

pub(super) async fn qualify(connection: &mut SqliteConnection) -> anyhow::Result<PrivateHistory> {
    let private = state_ledger::read(connection, Ledger::Stateful)
        .await?
        .ok_or_else(|| anyhow::anyhow!("private migration ledger missing"))?;
    let canonical = state_ledger::read(connection, Ledger::Canonical)
        .await?
        .ok_or_else(|| anyhow::anyhow!("private namespace without canonical migration ledger"))?;
    let mut versions = BTreeSet::new();
    for record in &canonical {
        let identity = state_catalog::canonical(record.version).ok_or_else(|| {
            anyhow::anyhow!("known namespace qualification requires canonical versions through 60")
        })?;
        ensure!(
            identity.checksum.as_ref() == record.checksum
                || checksum_is_line_ending_equivalent(&identity, &record.checksum),
            "unknown canonical identity or unsupported mixed legacy/private history"
        );
        versions.insert(record.version);
    }
    let last = versions.last().copied().unwrap_or_default();
    ensure!(
        (1..=last).all(|version| versions.contains(&version)),
        "incomplete canonical namespace prefix"
    );
    let mut fork = BTreeSet::new();
    for (offset, record) in private.iter().enumerate() {
        ensure!(
            record.version == i64::try_from(offset)? + 1 && record.version <= 2,
            "unsupported private migration prefix"
        );
        let version = record.version + 58;
        let identity = state_catalog::fork(version)
            .ok_or_else(|| anyhow::anyhow!("private migration identity missing"))?;
        ensure!(
            identity.checksum.as_ref() == record.checksum
                || checksum_is_line_ending_equivalent(&identity, &record.checksum),
            "unknown private migration identity"
        );
        fork.insert(version);
    }
    ensure!(
        private.is_empty() || last >= 55,
        "private owner fact without required canonical prefix"
    );
    ensure!(
        private.len() < 2 || last >= 58,
        "private binding fact without required canonical prefix"
    );
    let archive = archive::qualify(connection).await?;
    ensure!(
        archive.is_none() || !private.is_empty(),
        "archive with empty private ledger"
    );
    // The three metadata tables have all been independently qualified above.
    // Exclude only their table objects; application triggers/indexes stay visible.
    state_schema::validate(
        connection,
        &state_schema::AppliedSchema {
            canonical: versions,
            fork,
            opaque_suffix: false,
        },
    )
    .await?;
    Ok(PrivateHistory {
        canonical,
        private,
        archive,
    })
}

#[cfg(test)]
#[path = "state_modern_tests.rs"]
mod tests;
