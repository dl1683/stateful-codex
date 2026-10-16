use sha2::Digest;
use sha2::Sha256;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::migrate::Migrator;

use crate::StoreIdentity;
use crate::StoreRegistryError;
use crate::registry::SCHEMA;
use crate::registry::{self};

const TRANSITION: &str = "UPDATE project_store_identity SET format_version = 2, min_reader_version = 2, min_writer_version = 2 WHERE singleton = 1";
type History = Vec<(Option<String>, i64, Option<Vec<u8>>)>;

/// A domain's trusted application-owned, compiled migration list. Never populate
/// this from foreign-agent input. SQL executes with the caller's full authority.
/// Migrations must be forward, transactional main-database SQL with no transaction
/// control, attachments, connection pragmas or external effects.
pub struct DomainManifest<'a> {
    pub domain: &'a str,
    pub migrations: &'a Migrator,
}

/// A prepared complete format-2 schema. Preparation uses isolated memory before
/// any destination lock. No placement, legacy import or native routing is implied.
pub struct PreparedDomains {
    migrations: Vec<(String, i64, sqlx::SqlStr)>,
    schema: Vec<String>,
    history: History,
}

impl PreparedDomains {
    /// Prepare at most eight domains / 128 migrations / 2 MiB of trusted SQL.
    /// Only forward transactional simple migrations are supported in this slice.
    #[expect(
        clippy::disallowed_methods,
        reason = "isolated project-store schema preparation"
    )]
    pub async fn prepare(domains: &[DomainManifest<'_>]) -> Result<Self, StoreRegistryError> {
        let mut result = Self {
            migrations: Vec::new(),
            schema: Vec::new(),
            history: Vec::new(),
        };
        let mut names = std::collections::HashSet::new();
        let mut bytes = 0_usize;
        if domains.is_empty() || domains.len() > 8 {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        for domain in domains {
            if domain.domain.is_empty()
                || domain.domain.len() > 64
                || domain.domain == "project-store"
                || !names.insert(domain.domain)
                || !domain
                    .domain
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c == b'-')
                || domain.migrations.no_tx
                || domain.migrations.iter().len() == 0
            {
                return Err(StoreRegistryError::IncompatibleHistory);
            }
            let mut previous = 0;
            for migration in domain.migrations.iter() {
                bytes = bytes.saturating_add(migration.sql.as_str().len());
                if migration.version <= previous
                    || migration.no_tx
                    || migration.migration_type != sqlx::migrate::MigrationType::Simple
                    || result.migrations.len() == 128
                    || bytes > 2 * 1024 * 1024
                {
                    return Err(StoreRegistryError::IncompatibleHistory);
                }
                previous = migration.version;
                result.migrations.push((
                    domain.domain.to_string(),
                    migration.version,
                    migration.sql.clone(),
                ));
            }
        }
        result
            .migrations
            .sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        let mut scratch = SqliteConnection::connect("sqlite::memory:").await?;
        let identity = crate::initialize_connection(&mut scratch).await?;
        let kernel = schema(&mut scratch).await?;
        result.apply(&mut scratch).await?;
        result.schema = schema(&mut scratch).await?;
        // Application migrations must not change or remove either kernel table.
        for object in kernel {
            if !result.schema.contains(&object) {
                return Err(StoreRegistryError::UnknownSchema);
            }
        }
        result.history = history(&mut scratch).await?;
        let mut expected = vec![
            (
                Some("project-store".to_string()),
                1,
                Some(Sha256::digest(SCHEMA).to_vec()),
            ),
            (
                Some("project-store".to_string()),
                2,
                Some(Sha256::digest(TRANSITION).to_vec()),
            ),
        ];
        expected.extend(result.migrations.iter().map(|(domain, version, sql)| {
            (
                Some(domain.clone()),
                *version,
                Some(Sha256::digest(sql.as_str()).to_vec()),
            )
        }));
        expected.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        if result.history != expected {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        registry::validate_header(&mut scratch).await?;
        if registry::validate_identity(&mut scratch, /*version*/ 2).await? != identity {
            return Err(StoreRegistryError::InvalidIdentity);
        }
        scratch.close().await?;
        Ok(result)
    }

    /// Explicitly install into an already initialized bootstrap connection.
    /// Caller must qualify placement and configure a bounded busy timeout.
    /// All domain DDL/records and version changes commit together; errors roll back.
    /// Existing format-2 stores require exact full-manifest/schema equality.
    pub async fn install(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<StoreIdentity, StoreRegistryError> {
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await?;
        if foreign_keys != 1 {
            return Err(StoreRegistryError::UnknownSchema);
        }
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let version: i64 = sqlx::query_scalar(
                "SELECT format_version FROM project_store_identity WHERE singleton = 1",
            )
            .fetch_one(&mut *transaction)
            .await?;
            if version == 1 {
                let identity = registry::validate(&mut transaction).await?;
                self.apply(&mut transaction).await?;
                if self.validate(&mut transaction).await? != identity {
                    return Err(StoreRegistryError::InvalidIdentity);
                }
                Ok(identity)
            } else {
                self.validate(&mut transaction).await
            }
        }
        .await;
        match result {
            Ok(identity) => {
                transaction.commit().await?;
                Ok(identity)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    /// Admit a complete registered store using one read-only-compatible snapshot.
    pub async fn read_identity(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<StoreIdentity, StoreRegistryError> {
        let mut transaction = connection.begin().await?;
        let identity = self.validate(&mut transaction).await?;
        transaction.commit().await?;
        Ok(identity)
    }

    async fn validate(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<StoreIdentity, StoreRegistryError> {
        registry::validate_header(connection).await?;
        if schema(connection).await? != self.schema {
            return Err(StoreRegistryError::UnknownSchema);
        }
        let identity = registry::validate_identity(connection, /*version*/ 2).await?;
        if history(connection).await? != self.history {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        Ok(identity)
    }

    async fn apply(&self, connection: &mut SqliteConnection) -> Result<(), StoreRegistryError> {
        sqlx::raw_sql(TRANSITION).execute(&mut *connection).await?;
        for (domain, version, sql) in
            std::iter::once(("project-store", 2, sqlx::SqlStr::from_static(TRANSITION))).chain(
                self.migrations
                    .iter()
                    .map(|(domain, version, sql)| (domain.as_str(), *version, sql.clone())),
            )
        {
            let digest = Sha256::digest(sql.as_str()).to_vec();
            if domain != "project-store" {
                sqlx::raw_sql(sql).execute(&mut *connection).await?;
            }
            sqlx::query("INSERT INTO project_store_migrations VALUES (?, ?, ?)")
                .bind(domain)
                .bind(version)
                .bind(digest)
                .execute(&mut *connection)
                .await?;
        }
        Ok(())
    }
}

async fn schema(connection: &mut SqliteConnection) -> Result<Vec<String>, StoreRegistryError> {
    let rows: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT CASE WHEN octet_length(type || name || tbl_name || COALESCE(sql, '')) <= 65536
         THEN type || char(0) || name || char(0) || tbl_name || char(0) || COALESCE(sql, '') END
         FROM sqlite_schema ORDER BY name LIMIT 513",
    )
    .fetch_all(connection)
    .await?;
    if rows.len() > 512 {
        return Err(StoreRegistryError::UnknownSchema);
    }
    rows.into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(StoreRegistryError::UnknownSchema)
}

async fn history(connection: &mut SqliteConnection) -> Result<History, StoreRegistryError> {
    let rows = sqlx::query_as(
        "SELECT CASE WHEN octet_length(domain) <= 64 THEN domain END, version,
                CASE WHEN octet_length(digest) = 32 THEN digest END
         FROM project_store_migrations ORDER BY domain, version LIMIT 131",
    )
    .fetch_all(connection)
    .await?;
    Ok(rows)
}

#[cfg(test)]
#[path = "domains_tests.rs"]
mod tests;
