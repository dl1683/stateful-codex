//! Persistent, opt-in runtime write exclusion before project conversion.

use sha2::Digest;
use sha2::Sha256;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteSynchronous;
use std::time::Duration;
use thiserror::Error;

use crate::storage::DATABASE_NAME;
use crate::storage::MIGRATOR;
use codex_state::SqliteConfig;

const REGISTRY: &str = "CREATE TABLE _stateful_runtime_quiescence (project_id TEXT PRIMARY KEY NOT NULL, format INTEGER NOT NULL CHECK(format=1), migration_digest TEXT NOT NULL)";
const TABLES: [&str; 14] = [
    "stateful_acceptance_criteria",
    "stateful_acceptance_evidence",
    "stateful_acceptance_ledgers",
    "stateful_acceptance_pending",
    "stateful_acceptance_request_parts",
    "stateful_acceptance_steering",
    "stateful_host_answers",
    "stateful_obligations",
    "stateful_run_continuations",
    "stateful_run_leases",
    "stateful_run_threads",
    "stateful_runs",
    "stateful_steering",
    "stateful_turn_measurements",
];

#[derive(Debug, Error)]
pub enum LegacyQuiescenceError {
    #[error("project ID must contain 1-256 UTF-8 bytes without controls or outer whitespace")]
    InvalidProject,
    #[error("legacy runtime schema, migration history or quiescence registry is incompatible")]
    Incompatible,
    #[error("unfinished installer operation; await close, then reopen and reconcile")]
    Unfinished,
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
}

/// A dedicated existing-file installer. Never shares a legacy service pool.
/// An interrupted operation makes this owner close-only; awaited close drains
/// queued SQLite work, including an unacknowledged commit, before reconciliation.
pub struct LegacyRuntimeQuiescence {
    connection: SqliteConnection,
    prepared: PreparedQuiescence,
    ready: bool,
}

impl LegacyRuntimeQuiescence {
    /// Prepare trusted DDL, then open an existing native runtime database without
    /// creation or migration. Five seconds bounds each SQLite lock wait, not total
    /// admission latency. There is no shared-pool acquisition or automatic retry.
    #[expect(
        clippy::disallowed_methods,
        reason = "dedicated legacy cutover installer"
    )]
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, LegacyQuiescenceError> {
        let prepared = prepare().await?;
        let connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(sqlite.home().join(DATABASE_NAME))
                .create_if_missing(/*create*/ false)
                .foreign_keys(/*on*/ true)
                .synchronous(SqliteSynchronous::Full)
                .busy_timeout(Duration::from_secs(/*secs*/ 5)),
        )
        .await?;
        Ok(Self {
            connection,
            prepared,
            ready: true,
        })
    }

    /// Drain the dedicated connection before reconciling on a newly opened owner.
    pub async fn close(self) -> Result<(), LegacyQuiescenceError> {
        self.connection.close().await?;
        Ok(())
    }

    /// Permanently refuse this project's legacy runtime writes, including from
    /// already-open clients. Installation is atomic and idempotent; reads remain
    /// available. No import, native attachment, knowledge/metadata barrier or
    /// multi-database cutover is implied. No automatic caller or unfreeze exists.
    ///
    /// Operates only on the exact compiled native schema. Other projects remain
    /// writable, except statements colliding with protected unique keys/rowids.
    /// SQLite's unspecified BEFORE INSERT rowid (-1) can conservatively refuse
    /// an insert when a protected row already has rowid -1. Arbitrary same-user
    /// DDL that alters the guards or registry is outside this contract. Registry
    /// DML cannot unfreeze or replace existing markers. At most 128 markers.
    /// Read-only replays remain permitted. This does not stop active commands or
    /// models; operational quiescence must precede any actual cutover.
    /// On cancellation/error, await close and reopen before repeating this method.
    /// Cancellation is neither proof of rollback nor an installed receipt.
    pub async fn quiesce_project(&mut self, project_id: &str) -> Result<(), LegacyQuiescenceError> {
        if !valid_project(project_id) {
            return Err(LegacyQuiescenceError::InvalidProject);
        }
        if !self.ready {
            return Err(LegacyQuiescenceError::Unfinished);
        }
        self.ready = false;
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let history: Vec<(i64, String, bool, Vec<u8>)> = sqlx::query_as(
            "SELECT version, description, success=1, checksum FROM _sqlx_migrations ORDER BY version LIMIT 129",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let expected: Vec<_> = MIGRATOR
            .iter()
            .map(|m| {
                (
                    m.version,
                    m.description.to_string(),
                    true,
                    m.checksum.to_vec(),
                )
            })
            .collect();
        if history != expected {
            return Err(LegacyQuiescenceError::Incompatible);
        }
        let actual = schema(&mut transaction).await?;
        if actual == self.prepared.base {
            sqlx::raw_sql(sqlx::AssertSqlSafe(self.prepared.ddl.as_str()))
                .execute(&mut *transaction)
                .await?;
        } else if actual != self.prepared.guarded {
            return Err(LegacyQuiescenceError::Incompatible);
        }
        let markers: Vec<(String, bool, String)> = sqlx::query_as(
            "SELECT project_id,typeof(format)='integer' AND format=1 AND typeof(project_id)='text' AND typeof(migration_digest)='text',migration_digest FROM _stateful_runtime_quiescence LIMIT 129",
        )
        .fetch_all(&mut *transaction)
        .await?;
        if markers.len() > 128
            || markers.iter().any(|(project, valid, digest)| {
                !valid || !valid_project(project) || digest != &self.prepared.digest
            })
        {
            return Err(LegacyQuiescenceError::Incompatible);
        }
        if !markers.iter().any(|(project, _, _)| project == project_id) {
            if markers.len() == 128 {
                return Err(LegacyQuiescenceError::Incompatible);
            }
            sqlx::query("INSERT INTO _stateful_runtime_quiescence VALUES (?,1,?)")
                .bind(project_id)
                .bind(&self.prepared.digest)
                .execute(&mut *transaction)
                .await?;
        }
        if schema(&mut transaction).await? != self.prepared.guarded {
            return Err(LegacyQuiescenceError::Incompatible);
        }
        transaction.commit().await?;
        self.ready = true;
        Ok(())
    }
}

type Schema = Vec<(String, String, String, Option<String>)>;

struct PreparedQuiescence {
    base: Schema,
    guarded: Schema,
    ddl: String,
    digest: String,
}

fn valid_project(project: &str) -> bool {
    !project.is_empty()
        && project.len() <= 256
        && project.trim() == project
        && !project.chars().any(char::is_control)
}

async fn schema(connection: &mut SqliteConnection) -> Result<Schema, sqlx::Error> {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name<>'sqlite_sequence' ORDER BY type,name LIMIT 129")
        .fetch_all(connection)
        .await
}

#[expect(
    clippy::disallowed_methods,
    reason = "isolated compiled schema preparation"
)]
async fn prepare() -> Result<PreparedQuiescence, LegacyQuiescenceError> {
    let mut scratch = SqliteConnection::connect("sqlite::memory:").await?;
    MIGRATOR.run(&mut scratch).await?;
    let base = schema(&mut scratch).await?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT IN ('sqlite_sequence','_sqlx_migrations') ORDER BY name",
    )
    .fetch_all(&mut scratch)
    .await?;
    if tables != TABLES {
        return Err(LegacyQuiescenceError::Incompatible);
    }
    let mut hash = Sha256::new();
    for migration in MIGRATOR.iter() {
        hash.update(migration.version.to_be_bytes());
        hash.update(&migration.checksum);
    }
    let digest = format!("{:x}", hash.finalize());
    let mut ddl = format!("{REGISTRY};");
    for operation in ["UPDATE", "DELETE"] {
        ddl.push_str(&format!("CREATE TRIGGER _stateful_quiescence_registry_{operation} BEFORE {operation} ON _stateful_runtime_quiescence BEGIN SELECT RAISE(ABORT,'runtime quiescence markers are immutable'); END;"));
    }
    ddl.push_str(&format!("CREATE TRIGGER _stateful_quiescence_registry_INSERT BEFORE INSERT ON _stateful_runtime_quiescence WHEN typeof(NEW.project_id)<>'text' OR length(CAST(NEW.project_id AS BLOB)) NOT BETWEEN 1 AND 256 OR typeof(NEW.format)<>'integer' OR NEW.format<>1 OR typeof(NEW.migration_digest)<>'text' OR NEW.migration_digest<>'{digest}' OR EXISTS(SELECT 1 FROM _stateful_runtime_quiescence existing WHERE existing.project_id=NEW.project_id OR existing.rowid=NEW.rowid) BEGIN SELECT RAISE(ABORT,'runtime quiescence marker is invalid or already exists'); END;"));
    for table in TABLES {
        let mut keys = vec!["existing.rowid=NEW.rowid".to_string()];
        let indexes: Vec<(String, i64)> = sqlx::query_as(
            "SELECT name,partial FROM pragma_index_list(?) WHERE \"unique\"=1 ORDER BY name",
        )
        .bind(table)
        .fetch_all(&mut scratch)
        .await?;
        for (index, partial) in indexes {
            let columns: Vec<(Option<String>, String, i64)> = sqlx::query_as(
                "SELECT name,coll,cid FROM pragma_index_xinfo(?) WHERE key=1 ORDER BY seqno",
            )
            .bind(index)
            .fetch_all(&mut scratch)
            .await?;
            if partial != 0
                || columns.is_empty()
                || columns.iter().any(|c| c.2 < 0 || c.0.is_none())
            {
                return Err(LegacyQuiescenceError::Incompatible);
            }
            keys.push(
                columns
                    .into_iter()
                    .map(|(column, collation, _)| {
                        let column = quote(column.as_deref().unwrap_or_default());
                        format!(
                            "existing.{column}=NEW.{column} COLLATE {}",
                            quote(&collation)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" AND "),
            );
        }
        let collision = format!(
            "EXISTS(SELECT 1 FROM {} existing WHERE ({}) AND ({}))",
            quote(table),
            keys.join(" OR "),
            protected(table, "existing"),
        );
        for (operation, predicate) in [
            (
                "INSERT",
                format!("({}) OR {collision}", protected(table, "NEW")),
            ),
            (
                "UPDATE",
                format!(
                    "({}) OR ({}) OR {collision}",
                    protected(table, "OLD"),
                    protected(table, "NEW")
                ),
            ),
            ("DELETE", protected(table, "OLD")),
        ] {
            ddl.push_str(&format!(
                "CREATE TRIGGER {} BEFORE {operation} ON {} WHEN {predicate} BEGIN SELECT RAISE(ABORT,'stateful project runtime writes are quiesced'); END;",
                quote(&format!("_stateful_quiescence_{table}_{operation}")), quote(table),
            ));
        }
    }
    // Only compiled table names, quoted scratch-schema identifiers and a hex
    // compiled-migration digest enter this DDL. Project IDs remain bound values.
    sqlx::raw_sql(sqlx::AssertSqlSafe(ddl.as_str()))
        .execute(&mut scratch)
        .await?;
    let guarded = schema(&mut scratch).await?;
    scratch.close().await?;
    Ok(PreparedQuiescence {
        base,
        guarded,
        ddl,
        digest,
    })
}

fn protected(table: &str, alias: &str) -> String {
    let direct = format!(
        "EXISTS(SELECT 1 FROM _stateful_runtime_quiescence b WHERE b.project_id={alias}.project_id)"
    );
    let parent = format!(
        "EXISTS(SELECT 1 FROM stateful_runs r JOIN _stateful_runtime_quiescence b ON b.project_id=r.project_id WHERE r.id={alias}.run_id)"
    );
    match table {
        "stateful_runs" => direct,
        "stateful_obligations" | "stateful_steering" | "stateful_turn_measurements" => {
            format!("{direct} OR {parent}")
        }
        _ => parent,
    }
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
#[path = "legacy_quiescence_tests.rs"]
mod tests;
