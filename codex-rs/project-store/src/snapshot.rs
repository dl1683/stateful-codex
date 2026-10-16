use sqlx::Connection;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::Transaction;
use thiserror::Error;

use crate::DomainManifest;
use crate::ProjectStoreReader;
use crate::StoreIdentity;
use crate::StoreRegistryError;

/// One admitted read-only connection whose operations revalidate the store.
/// No pool, reconnect, home route or knowledge eligibility is supplied.
pub struct ProjectDomainReader {
    reader: ProjectStoreReader,
    ready: bool,
}

/// An admitted transaction held through trusted domain reads. Borrowers must not
/// issue transaction control or retain the connection beyond this guard.
/// Dropping without finishing makes the owner close-only, including cancellation.
pub struct ProjectReadSnapshot<'a> {
    transaction: Transaction<'a, Sqlite>,
    ready: &'a mut bool,
    identity: &'a StoreIdentity,
}

/// Refusal to start or finish an admitted domain snapshot.
#[derive(Debug, Error)]
pub enum ProjectReadError {
    #[error("unfinished read operation; close this reader before reopening")]
    UnfinishedOperation,
    #[error(transparent)]
    Registry(#[from] StoreRegistryError),
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
}

impl ProjectStoreReader {
    /// Retain the complete compiled admission contract for each domain snapshot.
    pub async fn into_domain_reader(
        self,
        manifest: &DomainManifest<'_>,
    ) -> Result<ProjectDomainReader, StoreRegistryError> {
        Ok(ProjectDomainReader {
            reader: self.admit_domain(manifest).await?,
            ready: true,
        })
    }
}

impl ProjectDomainReader {
    /// Validate schema/history/header and both UUIDs in the transaction returned
    /// to the caller. No external work or inference should occur while held.
    /// Cancellation or refusal leaves the owner close-only until explicitly closed.
    pub async fn begin_snapshot(&mut self) -> Result<ProjectReadSnapshot<'_>, ProjectReadError> {
        if !self.ready {
            return Err(ProjectReadError::UnfinishedOperation);
        }
        self.ready = false;
        let mut transaction = self.reader.connection.begin().await?;
        let identity = match self.reader.domains.validate(&mut transaction).await {
            Ok(identity) => identity,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error.into());
            }
        };
        if identity != self.reader.identity {
            transaction.rollback().await?;
            return Err(StoreRegistryError::InvalidIdentity.into());
        }
        Ok(ProjectReadSnapshot {
            transaction,
            ready: &mut self.ready,
            identity: &self.reader.identity,
        })
    }

    /// Await release, including rollback queued by an unfinished guard, before moving.
    pub async fn close(self) -> Result<(), sqlx::Error> {
        self.reader.close().await
    }
}

impl ProjectReadSnapshot<'_> {
    /// Both UUIDs validated in this snapshot, not a knowledge/freshness certificate.
    pub fn identity(&self) -> &StoreIdentity {
        self.identity
    }

    /// Trusted domain reads only; no transaction control, pragmas or attachments.
    pub fn connection(&mut self) -> &mut SqliteConnection {
        &mut self.transaction
    }

    /// Await completion before making the owner reusable. Failure/cancellation
    /// keeps it close-only; dropping SQLx's transaction alone is not completion.
    pub async fn finish(self) -> Result<(), ProjectReadError> {
        self.transaction.commit().await?;
        *self.ready = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
