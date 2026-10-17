use std::collections::BTreeSet;
use std::time::Duration;

use sha2::Digest;
use sha2::Sha256;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

use super::PreparedDomains;
use crate::ProjectStoreLocation;
use crate::ProjectWriteError;
use crate::StoreIdentity;
use crate::StoreRegistryError;
use crate::opener;

/// Explicit trusted append-only domain transition. Both complete manifests must
/// already have prepared successfully; ordinary readers never upgrade implicitly.
/// Prefix equality does not establish preservation for arbitrary migration SQL.
pub struct PreparedDomainUpgrade {
    predecessor: PreparedDomains,
    successor: PreparedDomains,
    suffix: Vec<(String, i64, sqlx::SqlStr)>,
}

/// Exact durable cohort observed in one qualified read-only snapshot. This is
/// neither an exclusive reservation nor permission to bypass writer admission.
#[derive(Debug, Eq, PartialEq)]
pub enum DomainUpgradeState {
    Predecessor,
    Successor,
}

impl PreparedDomainUpgrade {
    /// Require the same domains and unchanged per-domain migration prefixes,
    /// with at least one new suffix. No replacement, downgrade or domain addition.
    /// These are compiled application manifests, never foreign-supplied SQL.
    pub fn new(
        predecessor: PreparedDomains,
        successor: PreparedDomains,
    ) -> Result<Self, StoreRegistryError> {
        let names = |domains: &PreparedDomains| {
            domains.migrations.iter().map(|(name, _, _)| name.clone()).collect::<BTreeSet<_>>()
        };
        let old_names = names(&predecessor);
        if old_names != names(&successor) {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        let mut suffix = Vec::new();
        for name in old_names {
            let old: Vec<_> = predecessor.migrations.iter().filter(|(domain, _, _)| *domain == name)
                .map(|(_, version, sql)| (*version, sql.as_str())).collect();
            let new: Vec<_> = successor.migrations.iter().filter(|(domain, _, _)| *domain == name)
                .map(|(_, version, sql)| (*version, sql.as_str())).collect();
            if !new.starts_with(&old) {
                return Err(StoreRegistryError::IncompatibleHistory);
            }
            suffix.extend(successor.migrations.iter().filter(|(domain, _, _)| *domain == name).skip(old.len()).cloned());
        }
        if suffix.is_empty() {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        Ok(Self { predecessor, successor, suffix })
    }

    // Called only within the existing owner's immediate transaction. Its own
    // manifest is validated first, so the predecessor must match that same store.
    pub(crate) async fn apply(
        &self,
        connection: &mut SqliteConnection,
        expected: &StoreIdentity,
    ) -> Result<PreparedDomains, ProjectWriteError> {
        if self.predecessor.validate(connection).await? != *expected {
            return Err(ProjectWriteError::IdentityMismatch);
        }
        for (domain, version, sql) in &self.suffix {
            sqlx::raw_sql(sql.clone()).execute(&mut *connection).await?;
            sqlx::query("INSERT INTO project_store_migrations VALUES (?, ?, ?)")
                .bind(domain).bind(version).bind(Sha256::digest(sql.as_str()).to_vec())
                .execute(&mut *connection).await?;
        }
        if self.successor.validate(connection).await? != *expected {
            return Err(ProjectWriteError::IdentityMismatch);
        }
        Ok(PreparedDomains {
            migrations: self.successor.migrations.clone(),
            schema: self.successor.schema.clone(),
            history: self.successor.history.clone(),
        })
    }

    /// After awaited owner close, observe either complete cohort with the same
    /// expected UUIDs in ONE read-only snapshot. Unknown/partial states and I/O,
    /// busy or close errors refuse. Never infer commit from an acknowledgement
    /// loss or old in-memory state. A new attempt must revalidate under its lock.
    /// SQLite can create operational sidecars for an existing WAL database.
    #[expect(clippy::disallowed_methods, reason = "qualified read-only upgrade reconciliation")]
    pub async fn reconcile(
        &self,
        location: &ProjectStoreLocation,
        expected: &StoreIdentity,
    ) -> Result<DomainUpgradeState, ProjectWriteError> {
        opener::qualify_store(location, &opener::qualify_for_open)?;
        let options = SqliteConnectOptions::new()
            .filename(location.database_path().as_path()).read_only(/*read_only*/ true)
            .busy_timeout(Duration::from_secs(/*secs*/ 2));
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let result = async {
            let mut transaction = connection.begin().await?;
            let (identity, state) = match self.predecessor.validate(&mut transaction).await {
                Ok(identity) => (identity, DomainUpgradeState::Predecessor),
                Err(StoreRegistryError::UnknownSchema | StoreRegistryError::IncompatibleHistory) =>
                    (self.successor.validate(&mut transaction).await?, DomainUpgradeState::Successor),
                Err(error @ (StoreRegistryError::InvalidIdentity | StoreRegistryError::UnsupportedFormat | StoreRegistryError::Sqlite(_))) => return Err(error.into()),
            };
            if identity != *expected {
                return Err(ProjectWriteError::IdentityMismatch);
            }
            transaction.commit().await?;
            Ok(state)
        }.await;
        connection.close().await?;
        result
    }
}
