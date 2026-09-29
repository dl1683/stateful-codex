//! Shared SQLite connection configuration.

#![expect(
    clippy::disallowed_methods,
    reason = "this is the centralized SQLite connection shim"
)]

use crate::DbTelemetry;
use crate::migrations::checksum_is_line_ending_equivalent;
use crate::migrations::repair_legacy_recency_migration_version;
use crate::runtime::RuntimeDbInitError;
use crate::telemetry;
use crate::telemetry::DbKind;
use codex_utils_absolute_path::AbsolutePathBuf;
use log::LevelFilter;
use sqlx::ConnectOptions;
use sqlx::Error;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteAutoVacuum;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::sqlite::SqliteSynchronous;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

const LOGS_DB_FILENAME: &str = "logs_2.sqlite";
const GOALS_DB_FILENAME: &str = "goals_1.sqlite";
const MEMORIES_DB_FILENAME: &str = "memories_1.sqlite";
const QUEUE_DB_FILENAME: &str = "queue_1.sqlite";
const STATE_DB_FILENAME: &str = "state_5.sqlite";
const THREAD_HISTORY_DB_FILENAME: &str = "thread_history_1.sqlite";

#[derive(Clone, Copy)]
struct RuntimeDbSpec {
    label: &'static str,
    filename: &'static str,
    kind: DbKind,
    open_phase: &'static str,
    migrate_phase: &'static str,
}

impl RuntimeDbSpec {
    fn path(self, codex_home: &Path) -> PathBuf {
        codex_home.join(self.filename)
    }
}

const STATE_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "state DB",
    filename: STATE_DB_FILENAME,
    kind: DbKind::State,
    open_phase: "open_state",
    migrate_phase: "migrate_state",
};

const LOGS_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "log DB",
    filename: LOGS_DB_FILENAME,
    kind: DbKind::Logs,
    open_phase: "open_logs",
    migrate_phase: "migrate_logs",
};

const GOALS_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "goals DB",
    filename: GOALS_DB_FILENAME,
    kind: DbKind::Goals,
    open_phase: "open_goals",
    migrate_phase: "migrate_goals",
};

const MEMORIES_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "memories DB",
    filename: MEMORIES_DB_FILENAME,
    kind: DbKind::Memories,
    open_phase: "open_memories",
    migrate_phase: "migrate_memories",
};

const MEMORIES_V2_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "memories v2 DB",
    filename: "memories_v2_1.sqlite",
    ..MEMORIES_DB
};

const QUEUE_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "queue DB",
    filename: QUEUE_DB_FILENAME,
    kind: DbKind::Queue,
    open_phase: "open_queue",
    migrate_phase: "migrate_queue",
};

const THREAD_HISTORY_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "thread history DB",
    filename: THREAD_HISTORY_DB_FILENAME,
    kind: DbKind::ThreadHistory,
    open_phase: "open_thread_history",
    migrate_phase: "migrate_thread_history",
};

const RUNTIME_DBS: [RuntimeDbSpec; 7] = [
    STATE_DB,
    LOGS_DB,
    GOALS_DB,
    MEMORIES_DB,
    MEMORIES_V2_DB,
    QUEUE_DB,
    THREAD_HISTORY_DB,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDbPath {
    pub label: &'static str,
    pub path: PathBuf,
}

/// Resolved configuration shared by all Codex SQLite connections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteConfig {
    sqlite_home: AbsolutePathBuf,
}

impl SqliteConfig {
    pub fn from_sqlite_home(sqlite_home: AbsolutePathBuf) -> Self {
        Self { sqlite_home }
    }

    pub fn new_for_testing(sqlite_home: AbsolutePathBuf) -> Self {
        Self::from_sqlite_home(sqlite_home)
    }

    pub fn home(&self) -> &Path {
        self.sqlite_home.as_path()
    }

    /// Run embedded migrations while treating CRLF and LF source checkouts as
    /// the same migration content.
    ///
    /// SQLx hashes migration source bytes, so Git line-ending conversion can
    /// otherwise make the same SQL look modified across Windows and Unix. This
    /// compatibility path adapts only the in-memory expected checksum and still
    /// rejects every substantive source change.
    pub async fn run_migrations(
        &self,
        pool: &SqlitePool,
        migrator: &Migrator,
    ) -> Result<(), MigrateError> {
        let compatible_migrator = line_ending_compatible_migrator(pool, migrator).await?;
        let result = match compatible_migrator.as_ref() {
            Some(compatible_migrator) => compatible_migrator.run(pool).await,
            None => migrator.run(pool).await,
        };
        if !matches!(&result, Err(MigrateError::VersionMismatch(_))) {
            return result;
        }

        // Another process can apply a migration between the compatibility read
        // above and SQLx acquiring its migration lock. Refresh once, and retry
        // only when the stored checksum is proven to differ by line endings.
        let Some(refreshed_migrator) = line_ending_compatible_migrator(pool, migrator).await?
        else {
            return result;
        };
        refreshed_migrator.run(pool).await
    }

    /// Return the path to the primary state database.
    pub fn state_db_path(&self) -> PathBuf {
        STATE_DB.path(self.home())
    }

    /// Return the path to the logs database.
    pub fn logs_db_path(&self) -> PathBuf {
        LOGS_DB.path(self.home())
    }

    /// Return the path to the goals database.
    pub fn goals_db_path(&self) -> PathBuf {
        GOALS_DB.path(self.home())
    }

    /// Return the path to the memories database.
    pub fn memories_db_path(&self) -> PathBuf {
        MEMORIES_DB.path(self.home())
    }

    pub(crate) fn memories_v2_db_path(&self) -> PathBuf {
        MEMORIES_V2_DB.path(self.home())
    }

    pub(crate) async fn open_memories_v2_db(&self) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(
            MEMORIES_V2_DB,
            &crate::migrations::runtime_memories_migrator(),
            /*telemetry_override*/ None,
        )
        .await
    }

    /// Return the path to the durable user-message queue database.
    pub fn queue_db_path(&self) -> PathBuf {
        QUEUE_DB.path(self.home())
    }

    /// Return the path to the paginated thread-history database.
    pub fn thread_history_db_path(&self) -> PathBuf {
        THREAD_HISTORY_DB.path(self.home())
    }

    /// Return the paths to every database managed by the state runtime.
    pub fn runtime_db_paths(&self) -> Vec<RuntimeDbPath> {
        RUNTIME_DBS
            .iter()
            .map(|spec| RuntimeDbPath {
                label: spec.label,
                path: spec.path(self.home()),
            })
            .collect()
    }

    pub(super) async fn open_state_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        // New state DBs should use incremental auto-vacuum, but retrofitting an
        // existing DB requires a full VACUUM. Do not attempt that during process
        // startup: it is maintenance work that can contend with foreground writers.
        self.open_runtime_db(STATE_DB, migrator, telemetry_override)
            .await
    }

    pub(super) async fn open_logs_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(LOGS_DB, migrator, telemetry_override)
            .await
    }

    pub(super) async fn open_goals_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(GOALS_DB, migrator, telemetry_override)
            .await
    }

    pub(super) async fn open_memories_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(MEMORIES_DB, migrator, telemetry_override)
            .await
    }

    pub(super) async fn open_queue_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(QUEUE_DB, migrator, telemetry_override)
            .await
    }

    pub(super) async fn open_thread_history_db(
        &self,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        self.open_runtime_db(THREAD_HISTORY_DB, migrator, telemetry_override)
            .await
    }

    async fn open_runtime_db(
        &self,
        spec: RuntimeDbSpec,
        migrator: &Migrator,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<SqlitePool> {
        let path = spec.path(self.home());
        let started = Instant::now();
        let pool_result = self
            .open_read_write_pool(&path)
            .await
            .map_err(anyhow::Error::from);
        telemetry::record_init_result(
            telemetry_override,
            spec.kind,
            spec.open_phase,
            started.elapsed(),
            &pool_result,
        );
        let pool = pool_result.map_err(|source| {
            RuntimeDbInitError::new(spec.label, "open", path.as_path(), source)
        })?;
        let started = Instant::now();
        let migrate_result = async {
            if matches!(spec.kind, DbKind::State) {
                repair_legacy_recency_migration_version(&pool, migrator).await?;
            }
            self.run_migrations(&pool, migrator)
                .await
                .map_err(anyhow::Error::from)
        }
        .await;
        telemetry::record_init_result(
            telemetry_override,
            spec.kind,
            spec.migrate_phase,
            started.elapsed(),
            &migrate_result,
        );
        if let Err(source) = migrate_result {
            pool.close().await;
            return Err(
                RuntimeDbInitError::new(spec.label, "migrate", path.as_path(), source).into(),
            );
        }
        Ok(pool)
    }

    /// Open a writable Codex SQLite database, creating it if necessary.
    pub async fn open_read_write_pool(&self, path: &Path) -> Result<SqlitePool, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .auto_vacuum(SqliteAutoVacuum::Incremental)
            .busy_timeout(Duration::from_secs(5))
            .log_statements(LevelFilter::Off);
        SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
    }

    /// Open an existing Codex SQLite database without creating or modifying it.
    pub async fn open_read_only_pool(
        &self,
        path: &Path,
        busy_timeout: Option<Duration>,
    ) -> Result<SqlitePool, Error> {
        let mut options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true)
            .immutable(true)
            .log_statements(LevelFilter::Off);
        if let Some(busy_timeout) = busy_timeout {
            options = options.busy_timeout(busy_timeout);
        }
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
    }
}

async fn line_ending_compatible_migrator(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> Result<Option<Migrator>, Error> {
    if migrator.table_name.as_ref() != "_sqlx_migrations" {
        return Ok(None);
    }
    let migrations_table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !migrations_table_exists {
        return Ok(None);
    }

    let applied = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT version, checksum FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect::<HashMap<_, _>>();
    let mut migrations = migrator.migrations.to_vec();
    let mut adapted = false;
    for migration in &mut migrations {
        let Some(applied_checksum) = applied.get(&migration.version) else {
            continue;
        };
        if migration.checksum.as_ref() == applied_checksum
            || !checksum_is_line_ending_equivalent(migration, applied_checksum)
        {
            continue;
        }
        migration.checksum = Cow::Owned(applied_checksum.clone());
        adapted = true;
    }
    if !adapted {
        return Ok(None);
    }

    Ok(Some(Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: migrator.ignore_missing,
        locking: migrator.locking,
        no_tx: migrator.no_tx,
        table_name: migrator.table_name.clone(),
        create_schemas: migrator.create_schemas.clone(),
    }))
}
