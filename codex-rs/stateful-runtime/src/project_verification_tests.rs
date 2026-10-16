use std::collections::BTreeMap;

use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::ProjectWriteError;
use codex_project_store::StoreIdentity;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::Row;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

use super::*;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Durable = BTreeMap<String, Vec<Vec<String>>>;

async fn domains() -> Result<PreparedDomains> {
    Ok(PreparedDomains::prepare(&[DomainManifest { domain: "stateful-runtime", migrations: &crate::storage::MIGRATOR }]).await?)
}

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    observer: SqliteConnection,
    run: StatefulRunId,
}

impl Fixture {
    #[expect(clippy::disallowed_methods, reason = "disposable project lease fixture")]
    async fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let mut observer = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(location.database_path().as_path()).foreign_keys(/*on*/ true)).await?;
        domains().await?.install(&mut observer).await?;
        sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms,max_continuations,max_elapsed_seconds,continuations_used) VALUES ('run',?,'Do work.','autonomous','running',0,1,100,100,4,600,0)")
            .bind(identity.project_uuid.to_string()).execute(&mut observer).await?;
        sqlx::query("INSERT INTO stateful_run_threads VALUES ('run',0,'thread')").execute(&mut observer).await?;
        Ok(Self { _directory: directory, location, identity, observer, run: StatefulRunId::parse("run")? })
    }

    async fn writer(&self) -> Result<ProjectStoreWriter> {
        Ok(ProjectStoreWriter::open(&self.location, domains().await?, &self.identity).await?)
    }

    async fn start(&self, owner: &str, lease_ms: u32) -> Result<u64> {
        let mut writer = self.writer().await?;
        let result = commit_project_verification_start(writer.begin_write().await?, &self.run, owner, lease_ms).await;
        writer.close().await?;
        Ok(result?)
    }

    async fn end(&self, owner: &str, attempt: u64) -> Result {
        let mut writer = self.writer().await?;
        let result = commit_project_verification_end(writer.begin_write().await?, &self.run, owner, attempt).await;
        writer.close().await?;
        Ok(result?)
    }

    async fn lease(&mut self) -> Result<(i64, Option<String>, Option<i64>)> {
        Ok(sqlx::query_as("SELECT verification_attempt,verification_owner,verification_lease_expires_at_ms-updated_at_ms FROM stateful_acceptance_ledgers WHERE run_id='run'").fetch_one(&mut self.observer).await?)
    }

    async fn durable(&mut self) -> Result<Durable> {
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut self.observer).await?;
        let mut tables = BTreeMap::new();
        let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
        for name in names {
            let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid").bind(&name).fetch_all(&mut self.observer).await?;
            let fields = columns.iter().map(|name| format!("quote({})", quoted(name))).collect::<Vec<_>>().join(",");
            let mut rows = Vec::new();
            for row in sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {fields} FROM {}", quoted(&name)))).fetch_all(&mut self.observer).await? {
                rows.push((0..columns.len()).map(|index| row.try_get::<String, _>(index)).collect::<std::result::Result<Vec<_>, _>>()?);
            }
            rows.sort();
            tables.insert(name, rows);
        }
        Ok(tables)
    }
}

#[tokio::test]
async fn same_owner_takeover_and_exact_release_preserve_other_attempts() -> Result {
    let mut f = Fixture::new().await?;
    assert_eq!(f.start("owner", /*lease_ms*/ 60_000).await?, 1);
    assert_eq!(f.start("owner", /*lease_ms*/ 60_000).await?, 2);
    assert_eq!(f.lease().await?, (2, Some("owner".into()), Some(60_000)));
    let before = f.durable().await?;
    f.end("owner", /*attempt*/ 1).await?;
    f.end("other", /*attempt*/ 2).await?;
    assert_eq!(f.durable().await?, before);
    f.end("owner", /*attempt*/ 2).await?;
    assert_eq!(f.lease().await?, (2, None, None));
    assert_eq!(f.start("other", /*lease_ms*/ 0).await?, 3);
    assert_eq!(f.start("next", /*lease_ms*/ 60_000).await?, 4);
    assert_eq!(f.lease().await?, (4, Some("next".into()), Some(60_000)));
    Ok(())
}

#[tokio::test]
async fn competing_lease_refusal_rolls_back_prior_writes_and_closes_owner() -> Result {
    let mut f = Fixture::new().await?;
    f.start("first", /*lease_ms*/ 60_000).await?;
    let before = f.durable().await?;
    let mut writer = f.writer().await?;
    let mut transaction = writer.begin_write().await?;
    sqlx::query("UPDATE stateful_runs SET updated_at_ms=999").execute(transaction.connection()).await?;
    let error = commit_project_verification_start(transaction, &f.run, "other", /*lease_ms*/ 60_000).await.unwrap_err();
    assert!(matches!(error, ProjectVerificationError::Runtime(StatefulRunStoreError::Acceptance(crate::AcceptanceError::VerificationLeased(_)))));
    assert!(matches!(writer.begin_write().await, Err(ProjectWriteError::UnfinishedOperation)));
    writer.close().await?;
    assert_eq!(f.durable().await?, before);
    Ok(())
}

#[derive(Clone, Copy)]
enum Refusal { Foreign, Missing, Terminal, InvalidOwner }

#[tokio::test]
async fn ownership_state_and_input_refusals_leave_all_project_rows_unchanged() -> Result {
    for case in [Refusal::Foreign, Refusal::Missing, Refusal::Terminal, Refusal::InvalidOwner] {
        let mut f = Fixture::new().await?;
        match case {
            Refusal::Foreign => { sqlx::query("UPDATE stateful_runs SET project_id='foreign'").execute(&mut f.observer).await?; }
            Refusal::Missing => { sqlx::query("DELETE FROM stateful_runs").execute(&mut f.observer).await?; }
            Refusal::Terminal => { sqlx::query("UPDATE stateful_runs SET status='cancelled'").execute(&mut f.observer).await?; }
            Refusal::InvalidOwner => {}
        }
        let before = f.durable().await?;
        let mut writer = f.writer().await?;
        let mut transaction = writer.begin_write().await?;
        sqlx::query("UPDATE stateful_runs SET updated_at_ms=999").execute(transaction.connection()).await?;
        let owner = if matches!(case, Refusal::InvalidOwner) { "" } else { "owner" };
        let error = commit_project_verification_start(transaction, &f.run, owner, /*lease_ms*/ 60_000).await.unwrap_err();
        assert!(match case {
            Refusal::Foreign => matches!(error, ProjectVerificationError::ProjectMismatch),
            Refusal::Missing => matches!(error, ProjectVerificationError::Runtime(StatefulRunStoreError::RunNotFound(_))),
            Refusal::Terminal => matches!(error, ProjectVerificationError::Runtime(StatefulRunStoreError::AcceptanceRunState(_))),
            Refusal::InvalidOwner => matches!(error, ProjectVerificationError::Runtime(StatefulRunStoreError::InvalidRun(_))),
        });
        writer.close().await?;
        assert_eq!(f.durable().await?, before);
    }
    Ok(())
}
