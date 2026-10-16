use std::time::Duration;

use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

use crate::PreparedDomains;
use crate::ProjectStoreLocation;
use crate::ProjectStoreOpenError;
use crate::StoreIdentity;
use crate::opener;

/// A format-admitted, project-bound read connection for trusted domain code.
/// No pool/reconnect, engine home, native attachment or knowledge eligibility
/// policy is supplied. Close and reselect after moving a project.
pub struct ProjectStoreReader {
    pub(crate) identity: StoreIdentity,
    pub(crate) connection: SqliteConnection,
    pub(crate) domains: PreparedDomains,
}

impl ProjectStoreReader {
    /// Open an existing, host-qualified local unsynchronized project store.
    /// Unknown, bootstrap-only or incomplete stores refuse without repair.
    #[expect(
        clippy::disallowed_methods,
        reason = "project-store owns qualified read-only connections"
    )]
    pub async fn open(
        location: &ProjectStoreLocation,
        domains: PreparedDomains,
    ) -> Result<Self, ProjectStoreOpenError> {
        opener::qualify_store(location, &opener::qualify_for_open)?;
        let options = SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .read_only(true)
            .busy_timeout(Duration::from_secs(/*secs*/ 2));
        let mut connection = SqliteConnection::connect_with(&options).await?;
        match domains.read_identity(&mut connection).await {
            Ok(identity) => Ok(Self {
                identity,
                connection,
                domains,
            }),
            Err(error) => {
                connection.close().await?;
                Err(error.into())
            }
        }
    }

    /// Identity validated at admission, not a knowledge/freshness certificate.
    pub fn identity(&self) -> &StoreIdentity {
        &self.identity
    }

    /// Await release of the SQLite file handle before moving a project.
    pub async fn close(self) -> Result<(), sqlx::Error> {
        self.connection.close().await
    }
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
