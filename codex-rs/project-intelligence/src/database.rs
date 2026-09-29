use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use codex_state::SqliteConfig;
use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::migrate::Migration;
use sqlx::migrate::Migrator;
use thiserror::Error;

use crate::ProjectKnowledgeAccess;
use crate::ProjectKnowledgeOperation;
use crate::ProjectKnowledgeReadOnlyError;
use crate::blackboard_storage::BlackboardStore;
use crate::context_map_storage::ContextMapStore;
use crate::indexer::ProjectIndexer;
use crate::storage::HierarchyStore;

pub(crate) const DATABASE_NAME: &str = "project_intelligence_1.sqlite";
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Error)]
pub enum ProjectKnowledgeDatabaseError {
    #[error("project intelligence database is missing: {path}")]
    MissingDatabase { path: PathBuf },
    #[error("project intelligence database is missing migration {missing_version}: {path}")]
    UnmigratedDatabase { path: PathBuf, missing_version: i64 },
    #[error("project intelligence migration {version} failed: {path}")]
    FailedMigration { path: PathBuf, version: i64 },
    #[error("project intelligence migration {version} checksum does not match: {path}")]
    MigrationChecksumMismatch { path: PathBuf, version: i64 },
    #[error("project intelligence database contains unknown migration {version}: {path}")]
    UnknownMigration { path: PathBuf, version: i64 },
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct ProjectKnowledgeDatabase {
    pub(crate) pool: SqlitePool,
    path: PathBuf,
    access: ProjectKnowledgeAccess,
}

impl ProjectKnowledgeDatabase {
    pub async fn open(
        sqlite: &SqliteConfig,
        access: ProjectKnowledgeAccess,
    ) -> Result<Self, ProjectKnowledgeDatabaseError> {
        let path = sqlite.home().join(DATABASE_NAME);
        let pool = match access {
            ProjectKnowledgeAccess::ReadWrite => {
                tokio::fs::create_dir_all(sqlite.home()).await?;
                let pool = sqlite.open_read_write_pool(&path).await?;
                if let Err(error) = sqlite.run_migrations(&pool, &MIGRATOR).await {
                    pool.close().await;
                    return Err(error.into());
                }
                pool
            }
            ProjectKnowledgeAccess::ReadOnly => {
                if !tokio::fs::try_exists(&path).await? {
                    return Err(ProjectKnowledgeDatabaseError::MissingDatabase { path });
                }
                let pool = sqlite
                    .open_read_only_pool(&path, Some(Duration::from_secs(5)))
                    .await?;
                if let Err(error) = validate_migrations(&pool, &path).await {
                    pool.close().await;
                    return Err(error);
                }
                pool
            }
        };
        Ok(Self { pool, path, access })
    }

    pub fn access(&self) -> ProjectKnowledgeAccess {
        self.access
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn hierarchy_store(&self) -> HierarchyStore {
        HierarchyStore::from_database(self.clone())
    }

    pub fn context_map_store(&self) -> ContextMapStore {
        ContextMapStore::from_database(self.clone())
    }

    pub fn blackboard_store(&self) -> BlackboardStore {
        BlackboardStore::from_database(self.clone())
    }

    pub fn indexer(&self) -> ProjectIndexer {
        ProjectIndexer::new(self.hierarchy_store(), self.context_map_store())
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub(crate) fn require_write(
        &self,
        operation: ProjectKnowledgeOperation,
        project_id: &str,
    ) -> Result<(), ProjectKnowledgeReadOnlyError> {
        if self.access == ProjectKnowledgeAccess::ReadOnly {
            return Err(ProjectKnowledgeReadOnlyError {
                operation,
                project_id: project_id.to_string(),
            });
        }
        Ok(())
    }
}

async fn validate_migrations(
    pool: &SqlitePool,
    path: &Path,
) -> Result<(), ProjectKnowledgeDatabaseError> {
    let table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !table_exists {
        return Err(ProjectKnowledgeDatabaseError::UnmigratedDatabase {
            path: path.to_path_buf(),
            missing_version: MIGRATOR
                .migrations
                .first()
                .map_or(1, |migration| migration.version),
        });
    }

    let applied = sqlx::query_as::<_, (i64, i64, Vec<u8>)>(
        "SELECT version, success, checksum FROM _sqlx_migrations",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(version, success, checksum)| (version, (success, checksum)))
    .collect::<HashMap<_, _>>();

    for migration in MIGRATOR.migrations.iter() {
        let Some((success, checksum)) = applied.get(&migration.version) else {
            return Err(ProjectKnowledgeDatabaseError::UnmigratedDatabase {
                path: path.to_path_buf(),
                missing_version: migration.version,
            });
        };
        if *success == 0 {
            return Err(ProjectKnowledgeDatabaseError::FailedMigration {
                path: path.to_path_buf(),
                version: migration.version,
            });
        }
        if migration.checksum.as_ref() != checksum
            && !checksum_is_line_ending_equivalent(migration, checksum)
        {
            return Err(ProjectKnowledgeDatabaseError::MigrationChecksumMismatch {
                path: path.to_path_buf(),
                version: migration.version,
            });
        }
    }

    for version in applied.keys() {
        if !MIGRATOR
            .migrations
            .iter()
            .any(|migration| migration.version == *version)
        {
            return Err(ProjectKnowledgeDatabaseError::UnknownMigration {
                path: path.to_path_buf(),
                version: *version,
            });
        }
    }
    Ok(())
}

fn checksum_is_line_ending_equivalent(migration: &Migration, applied_checksum: &[u8]) -> bool {
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

#[cfg(test)]
#[path = "database_tests.rs"]
mod tests;
