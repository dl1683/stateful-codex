//! Original-fact decoding and destination proof for completed namespace archives.
//! This is not a complete private/canonical history or application-schema check.

use anyhow::bail;
use anyhow::ensure;
use sqlx::Row;
use sqlx::SqliteConnection;
use sqlx::migrate::Migration;

use super::Destination;
use super::state_catalog;
use super::state_ledger::AppliedRecord;
use super::state_ledger::Ledger;
use super::state_ledger::{self};
use crate::migrations::checksum_is_line_ending_equivalent;
use crate::sqlite::state_metadata;
use crate::sqlite::state_metadata::Metadata;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ArchivedFact {
    pub source: AppliedRecord,
    pub destination: Destination,
}

fn matches(identity: &Migration, checksum: &[u8]) -> bool {
    identity.checksum.as_ref() == checksum || checksum_is_line_ending_equivalent(identity, checksum)
}

/// Read on the caller's coherent snapshot. This proves archive provenance only;
/// the caller must still validate both complete histories and application schema.
pub(super) async fn qualify(
    connection: &mut SqliteConnection,
) -> anyhow::Result<Option<Vec<ArchivedFact>>> {
    if !state_metadata::validate(connection, Metadata::Archive).await? {
        return Ok(None);
    }
    let private = state_ledger::read(connection, Ledger::Stateful)
        .await?
        .ok_or_else(|| anyhow::anyhow!("orphan migration archive without private ledger"))?;
    let canonical = state_ledger::read(connection, Ledger::Canonical)
        .await?
        .ok_or_else(|| anyhow::anyhow!("migration archive without canonical ledger"))?;
    // The completed transfer protocol emits only one or two original facts.
    // This is a lineage restriction, not a bound on future canonical histories.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _stateful_migration_archive")
        .fetch_one(&mut *connection)
        .await?;
    ensure!((1..=2).contains(&count), "unsupported archive history size");
    let rows = sqlx::query("SELECT source_version, destination_ledger, destination_version, description, installed_on, success, checksum, execution_time, typeof(source_version) AS vt, typeof(destination_ledger) AS lt, typeof(destination_version) AS dv, typeof(description) AS dt, typeof(installed_on) AS it, typeof(success) AS st, typeof(checksum) AS ct, typeof(execution_time) AS et FROM _stateful_migration_archive ORDER BY source_version")
        .fetch_all(&mut *connection).await?;
    let mut result = Vec::new();
    let mut sources = Vec::new();
    let mut destinations = Vec::new();
    for row in rows {
        for (field, expected) in [
            ("vt", "integer"),
            ("lt", "text"),
            ("dv", "integer"),
            ("dt", "text"),
            ("it", "text"),
            ("st", "integer"),
            ("ct", "blob"),
            ("et", "integer"),
        ] {
            ensure!(
                row.try_get::<String, _>(field)? == expected,
                "unsupported archive storage class: {field}"
            );
        }
        let source = AppliedRecord {
            version: row.try_get("source_version")?,
            description: row.try_get("description")?,
            installed_on: row.try_get("installed_on")?,
            success: row.try_get("success")?,
            checksum: row.try_get("checksum")?,
            execution_time: row.try_get("execution_time")?,
        };
        ensure!(
            source.success == 1 && source.version > 0 && !sources.contains(&source.version),
            "invalid or duplicate archive source fact"
        );
        let ledger: String = row.try_get("destination_ledger")?;
        let version: i64 = row.try_get("destination_version")?;
        let (destination, identity, records) = match (source.version, ledger.as_str(), version) {
            (38, "_sqlx_migrations", 39) => (
                Destination::CanonicalRecency,
                state_catalog::canonical(/*version*/ 39),
                &canonical,
            ),
            (56 | 59, "_stateful_migrations", 1) => (
                Destination::StatefulOwner,
                state_catalog::fork(/*version*/ 59),
                &private,
            ),
            (60, "_stateful_migrations", 2) => (
                Destination::StatefulBinding,
                state_catalog::fork(/*version*/ 60),
                &private,
            ),
            _ => bail!("unsupported archive destination"),
        };
        ensure!(
            !destinations.contains(&(ledger.clone(), version)),
            "duplicate archive destination"
        );
        let identity =
            identity.ok_or_else(|| anyhow::anyhow!("missing archive source identity"))?;
        ensure!(
            matches(&identity, &source.checksum),
            "unknown archived migration checksum"
        );
        if destination == Destination::StatefulOwner {
            ensure!(
                !canonical
                    .iter()
                    .any(|record| matches!(record.version, 56 | 59)
                        && matches(&identity, &record.checksum)),
                "archived owner fact also remains at a canonical legacy alias"
            );
        }
        let mut expected = source.clone();
        expected.version = version;
        ensure!(
            records.iter().find(|record| record.version == version) == Some(&expected),
            "archive destination metadata differs from original fact"
        );
        if let Some(replacement) = canonical
            .iter()
            .find(|record| record.version == source.version)
        {
            let identity = state_catalog::canonical(source.version)
                .ok_or_else(|| anyhow::anyhow!("unknown archive source replacement"))?;
            ensure!(
                matches(&identity, &replacement.checksum),
                "legacy archive source remains in canonical ledger or replacement is invalid"
            );
        }
        sources.push(source.version);
        destinations.push((ledger, version));
        result.push(ArchivedFact {
            source,
            destination,
        });
    }
    ensure!(
        matches!(sources.as_slice(), [38] | [56] | [59] | [59, 60]),
        "unsupported combined archive history"
    );
    Ok(Some(result))
}
