use std::path::Path;

use sqlx::Error;
use sqlx::migrate::Migrator;

use crate::migrations::checksum_is_line_ending_equivalent;

use super::SqliteConfig;

impl SqliteConfig {
    /// Refuse unsupported applied history before a caller opens a writable pool.
    ///
    /// Reads the existing store without changing journal mode or SQLx metadata. Only
    /// the candidate migration bytes (including the existing LF/CRLF equivalence)
    /// are supported; unknown versions, substantive edits and failed rows refuse.
    pub async fn check_migration_history(
        &self,
        path: &Path,
        migrator: &Migrator,
    ) -> Result<(), Error> {
        match tokio::fs::metadata(path).await {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        let pool = self
            .open_read_only_pool(path, /*busy_timeout*/ None)
            .await?;
        let result = async {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations')",
            )
            .fetch_one(&pool)
            .await?;
            let applied = if exists { sqlx::query_as::<_, (i64, String, Vec<u8>, bool)>(
                "SELECT version, description, checksum, success FROM _sqlx_migrations ORDER BY version",
            )
            .fetch_all(&pool)
            .await?
            } else {
                Vec::new()
            };
            if applied.is_empty() {
                let has_schema: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE NOT (type = 'table' AND name = '_sqlx_migrations') AND name NOT GLOB 'sqlite_*')",
                ).fetch_one(&pool).await?;
                if has_schema {
                    return Err(Error::Protocol(format!(
                        "unsupported migration history: database={}, version=unknown, description=\"missing or empty migration ledger for a nonempty database\", checksum=unavailable; original store untouched; automatic conversion is unavailable",
                        path.display()
                    )));
                }
            }
            for (version, description, checksum, success) in applied {
                let supported = success && migrator.iter().any(|migration| {
                    migration.version == version
                        && (migration.checksum.as_ref() == checksum
                            || checksum_is_line_ending_equivalent(migration, &checksum))
                });
                if !supported {
                    let prefix: String = checksum.iter().take(/*n*/ 12).map(|byte| format!("{byte:02x}")).collect();
                    return Err(Error::Protocol(format!(
                        "unsupported migration history: database={}, version={version:04}, description={description:?}, checksum={prefix}, success={success}; original store untouched; automatic conversion is unavailable",
                        path.display()
                    )));
                }
            }
            Ok(())
        }
        .await;
        pool.close().await;
        result
    }
}
