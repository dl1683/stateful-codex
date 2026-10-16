use std::time::Duration;

use sqlx::Connection;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::Transaction;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteSynchronous;
use thiserror::Error;

use crate::DomainManifest;
use crate::PreparedDomains;
use crate::ProjectStoreLocation;
use crate::ProjectStoreOpenError;
use crate::ProjectStoreReader;
use crate::StoreIdentity;
use crate::StoreRegistryError;
use crate::opener;

/// One project-local writer for trusted domain code. The caller must own this
/// writable location; a copied database does not authorize a writable fork.
/// No creation, installation, legacy/home route, reconnect or native cutover.
pub struct ProjectStoreWriter {
    connection: SqliteConnection,
    identity: StoreIdentity,
    domains: PreparedDomains,
    ready: bool,
}

/// An admitted immediate transaction spanning domain writes in one database.
/// Keep work bounded: never hold over inference, scans, commands or another DB.
/// Dropping/cancelling without an awaited finish makes the owner close-only.
pub struct ProjectWriteTransaction<'a> {
    transaction: Transaction<'a, Sqlite>,
    identity: &'a StoreIdentity,
    domains: &'a PreparedDomains,
    ready: &'a mut bool,
}

#[derive(Debug, Error)]
pub enum ProjectWriteError {
    #[error(transparent)]
    Open(#[from] ProjectStoreOpenError),
    #[error(transparent)]
    Registry(#[from] StoreRegistryError),
    #[error(transparent)]
    Sqlite(sqlx::Error),
    #[error("project writer is busy; close and reopen before retrying")]
    Busy,
    #[error("expected project/store identity does not match")]
    IdentityMismatch,
    #[error("unfinished write operation; close this writer before reopening")]
    UnfinishedOperation,
    #[error("the project store did not enter WAL mode")]
    WalUnavailable,
}

impl From<sqlx::Error> for ProjectWriteError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &error
            && database
                .code()
                .and_then(|code| code.parse::<u32>().ok())
                .is_some_and(|code| matches!(code & 0xff, 5 | 6))
        {
            return Self::Busy;
        }
        Self::Sqlite(error)
    }
}

impl ProjectStoreWriter {
    /// Admit an existing complete store read-only before any writable hooks.
    /// Revalidate location and schema on writable open, then enable WAL. A fixed
    /// two-second SQLite busy timeout bounds each lock wait, not total operation
    /// time. No silent retry. Same-user path replacement is outside this boundary.
    #[expect(
        clippy::disallowed_methods,
        reason = "project-store owns qualified local SQLite writers"
    )]
    pub async fn open(
        location: &ProjectStoreLocation,
        domains: PreparedDomains,
        expected: &StoreIdentity,
    ) -> Result<Self, ProjectWriteError> {
        let reader = ProjectStoreReader::open(location, domains).await?;
        let ProjectStoreReader {
            identity,
            connection,
            domains,
        } = reader;
        connection.close().await?;
        if identity != *expected {
            return Err(ProjectWriteError::IdentityMismatch);
        }
        opener::qualify_store(location, &opener::qualify_for_open)?;
        let options = SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .create_if_missing(/*create*/ false)
            .foreign_keys(/*on*/ true)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(/*secs*/ 2));
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let result = async {
            if domains.read_identity(&mut connection).await? != identity {
                return Err(ProjectWriteError::IdentityMismatch);
            }
            let mode: String = sqlx::query_scalar("PRAGMA journal_mode = WAL")
                .fetch_one(&mut connection)
                .await?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(ProjectWriteError::WalUnavailable);
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            connection.close().await?;
            return Err(error);
        }
        Ok(Self {
            connection,
            identity,
            domains,
            ready: true,
        })
    }

    /// Acquire the writer reservation before validating the entire compiled
    /// schema/history and both UUIDs in the returned transaction. Refusal or
    /// cancellation is close-only, even when SQLx queues a rollback itself.
    pub async fn begin_write(&mut self) -> Result<ProjectWriteTransaction<'_>, ProjectWriteError> {
        if !self.ready {
            return Err(ProjectWriteError::UnfinishedOperation);
        }
        self.ready = false;
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let identity = match self.domains.validate(&mut transaction).await {
            Ok(identity) => identity,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error.into());
            }
        };
        if identity != self.identity {
            transaction.rollback().await?;
            return Err(ProjectWriteError::IdentityMismatch);
        }
        Ok(ProjectWriteTransaction {
            transaction,
            identity: &self.identity,
            domains: &self.domains,
            ready: &mut self.ready,
        })
    }

    /// Await rollback/file release before reopening, retrying or moving a store.
    pub async fn close(self) -> Result<(), sqlx::Error> {
        self.connection.close().await
    }
}

impl ProjectWriteTransaction<'_> {
    /// Require a domain owner's actual compiled migration list before its SQL.
    /// This is in-process admission, not authentication of a foreign actor.
    pub fn require_domain(&self, manifest: &DomainManifest<'_>) -> Result<(), ProjectWriteError> {
        if !self.domains.matches_domain(manifest) {
            return Err(StoreRegistryError::IncompatibleHistory.into());
        }
        Ok(())
    }

    /// Identity checked under this writer reservation, not domain authorization.
    pub fn identity(&self) -> &StoreIdentity {
        self.identity
    }

    /// Trusted bounded domain SQL only: no transaction control, pragmas,
    /// attachments or external effects. Domain owners enforce row-level policy.
    pub fn connection(&mut self) -> &mut SqliteConnection {
        &mut self.transaction
    }

    /// Revalidate the kernel/schema before committing all domain writes together.
    /// A failed or cancelled commit is not a saved receipt; close and reconcile
    /// durable operation identities before deciding whether to retry.
    pub async fn commit(mut self) -> Result<(), ProjectWriteError> {
        let result = self.domains.validate(&mut self.transaction).await;
        match result {
            Ok(identity) if identity == *self.identity => {}
            result => {
                self.transaction.rollback().await?;
                return Err(match result {
                    Ok(_) => ProjectWriteError::IdentityMismatch,
                    Err(error) => error.into(),
                });
            }
        }
        self.transaction.commit().await?;
        *self.ready = true;
        Ok(())
    }

    /// Await full rollback before reusing this owner. Failure/cancellation keeps
    /// it close-only; no partially applied cross-domain result is published.
    pub async fn rollback(self) -> Result<(), ProjectWriteError> {
        self.transaction.rollback().await?;
        *self.ready = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
