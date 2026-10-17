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

async fn domains(domain: &str) -> Result<PreparedDomains> {
    Ok(PreparedDomains::prepare(&[DomainManifest {
        domain,
        migrations: &crate::storage::MIGRATOR,
    }])
    .await?)
}

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    domain: &'static str,
    observer: SqliteConnection,
    run: StatefulRunId,
}

impl Fixture {
    #[expect(
        clippy::disallowed_methods,
        reason = "disposable project lease fixture"
    )]
    async fn new(domain: &'static str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let mut observer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(location.database_path().as_path())
                .foreign_keys(/*on*/ true),
        )
        .await?;
        domains(domain).await?.install(&mut observer).await?;
        sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms,max_continuations,max_elapsed_seconds,continuations_used) VALUES ('run',?,'Do work.','autonomous','running',0,1,100,100,4,600,0)")
            .bind(identity.project_uuid.to_string()).execute(&mut observer).await?;
        sqlx::query("INSERT INTO stateful_run_threads VALUES ('run',0,'thread')")
            .execute(&mut observer)
            .await?;
        Ok(Self {
            _directory: directory,
            location,
            identity,
            domain,
            observer,
            run: StatefulRunId::parse("run")?,
        })
    }

    async fn writer(&self) -> Result<ProjectStoreWriter> {
        Ok(
            ProjectStoreWriter::open(&self.location, domains(self.domain).await?, &self.identity)
                .await?,
        )
    }

    async fn start(&self, owner: &str, lease_ms: u32) -> Result<u64> {
        let mut writer = self.writer().await?;
        let result = commit_project_verification_start(
            writer.begin_write().await?,
            &self.run,
            owner,
            lease_ms,
        )
        .await;
        writer.close().await?;
        Ok(result?)
    }

    async fn end(&self, owner: &str, attempt: u64) -> Result {
        let mut writer = self.writer().await?;
        let result =
            commit_project_verification_end(writer.begin_write().await?, &self.run, owner, attempt)
                .await;
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
            let columns: Vec<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                    .bind(&name)
                    .fetch_all(&mut self.observer)
                    .await?;
            // SQL quote() truncates TEXT at embedded NUL. Preserve storage class
            // and full TEXT/BLOB bytes; compare REAL values by their exact bits.
            let fields = columns.iter().map(|name| {
                let name = quoted(name);
                format!("typeof({name}),CASE WHEN typeof({name}) IN ('text','blob') THEN hex(CAST({name} AS BLOB)) ELSE {name} END")
            }).collect::<Vec<_>>().join(",");
            let mut rows = Vec::new();
            for row in sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {fields} FROM {}",
                quoted(&name)
            )))
            .fetch_all(&mut self.observer)
            .await?
            {
                let mut cells = Vec::new();
                for index in 0..columns.len() {
                    let storage: String = row.try_get(index * 2)?;
                    let value = match storage.as_str() {
                        "null" => String::new(),
                        "integer" => row.try_get::<i64, _>(index * 2 + 1)?.to_string(),
                        "real" => {
                            format!("{:016x}", row.try_get::<f64, _>(index * 2 + 1)?.to_bits())
                        }
                        "text" | "blob" => row.try_get::<String, _>(index * 2 + 1)?,
                        _ => {
                            return Err(
                                format!("unexpected SQLite storage class: {storage}").into()
                            );
                        }
                    };
                    cells.push(format!("{storage}:{value}"));
                }
                rows.push(cells);
            }
            rows.sort();
            tables.insert(name, rows);
        }
        Ok(tables)
    }
}

#[tokio::test]
async fn same_owner_takeover_and_exact_release_preserve_other_attempts() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
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
    let mut f = Fixture::new("stateful-runtime").await?;
    f.start("first", /*lease_ms*/ 60_000).await?;
    let before = f.durable().await?;
    let mut writer = f.writer().await?;
    let mut transaction = writer.begin_write().await?;
    sqlx::query("UPDATE stateful_runs SET updated_at_ms=999")
        .execute(transaction.connection())
        .await?;
    let error =
        commit_project_verification_start(transaction, &f.run, "other", /*lease_ms*/ 60_000)
            .await
            .unwrap_err();
    assert!(matches!(
        error,
        ProjectVerificationError::Runtime(StatefulRunStoreError::Acceptance(
            crate::AcceptanceError::VerificationLeased(_)
        ))
    ));
    assert!(matches!(
        writer.begin_write().await,
        Err(ProjectWriteError::UnfinishedOperation)
    ));
    writer.close().await?;
    assert_eq!(f.durable().await?, before);
    Ok(())
}

#[derive(Clone, Copy)]
enum Refusal {
    Foreign,
    Missing,
    Terminal,
    InvalidOwner,
}

#[tokio::test]
async fn ownership_state_and_input_refusals_leave_all_project_rows_unchanged() -> Result {
    for case in [
        Refusal::Foreign,
        Refusal::Missing,
        Refusal::Terminal,
        Refusal::InvalidOwner,
    ] {
        let mut f = Fixture::new("stateful-runtime").await?;
        match case {
            Refusal::Foreign => {
                sqlx::query(
                    "UPDATE stateful_runs SET project_id='00000000-0000-4000-8000-000000000001'",
                )
                .execute(&mut f.observer)
                .await?;
            }
            Refusal::Missing => {
                sqlx::query("DELETE FROM stateful_runs")
                    .execute(&mut f.observer)
                    .await?;
            }
            Refusal::Terminal => {
                sqlx::query("UPDATE stateful_runs SET status='cancelled'")
                    .execute(&mut f.observer)
                    .await?;
            }
            Refusal::InvalidOwner => {}
        }
        let before = f.durable().await?;
        let mut writer = f.writer().await?;
        let mut transaction = writer.begin_write().await?;
        sqlx::query("UPDATE stateful_runs SET updated_at_ms=999")
            .execute(transaction.connection())
            .await?;
        let owner = if matches!(case, Refusal::InvalidOwner) {
            ""
        } else {
            "owner"
        };
        let error =
            commit_project_verification_start(transaction, &f.run, owner, /*lease_ms*/ 60_000)
                .await
                .unwrap_err();
        assert!(match case {
            Refusal::Foreign => matches!(error, ProjectVerificationError::ProjectMismatch),
            Refusal::Missing => matches!(
                error,
                ProjectVerificationError::Runtime(StatefulRunStoreError::RunNotFound(_))
            ),
            Refusal::Terminal => matches!(
                error,
                ProjectVerificationError::Runtime(StatefulRunStoreError::AcceptanceRunState(_))
            ),
            Refusal::InvalidOwner => matches!(
                error,
                ProjectVerificationError::Runtime(StatefulRunStoreError::InvalidRun(_))
            ),
        });
        writer.close().await?;
        assert_eq!(f.durable().await?, before);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Operation {
    Start,
    End,
}

#[tokio::test]
async fn both_operations_refuse_unclaimed_domain_or_foreign_and_missing_runs() -> Result {
    for domain in ["other-runtime", "stateful-runtime"] {
        for case in [Refusal::Foreign, Refusal::Missing] {
            let mut f = Fixture::new(domain).await?;
            if domain == "stateful-runtime" {
                match case {
                    Refusal::Foreign => {
                        sqlx::query("UPDATE stateful_runs SET project_id='00000000-0000-4000-8000-000000000001'").execute(&mut f.observer).await?;
                    }
                    Refusal::Missing => {
                        sqlx::query("DELETE FROM stateful_runs")
                            .execute(&mut f.observer)
                            .await?;
                    }
                    Refusal::Terminal | Refusal::InvalidOwner => unreachable!(),
                }
            }
            for operation in [Operation::Start, Operation::End] {
                let before = f.durable().await?;
                let mut writer = f.writer().await?;
                let mut transaction = writer.begin_write().await?;
                sqlx::query("UPDATE stateful_runs SET updated_at_ms=999")
                    .execute(transaction.connection())
                    .await?;
                let error = match operation {
                    Operation::Start => commit_project_verification_start(
                        transaction,
                        &f.run,
                        "owner",
                        /*lease_ms*/ 60_000,
                    )
                    .await
                    .map(|_| ())
                    .unwrap_err(),
                    Operation::End => commit_project_verification_end(
                        transaction,
                        &f.run,
                        "owner",
                        /*attempt*/ 1,
                    )
                    .await
                    .unwrap_err(),
                };
                if domain == "other-runtime" {
                    assert!(matches!(
                        error,
                        ProjectVerificationError::Writer(ProjectWriteError::Registry(
                            codex_project_store::StoreRegistryError::IncompatibleHistory
                        ))
                    ));
                } else {
                    assert!(match case {
                        Refusal::Foreign =>
                            matches!(error, ProjectVerificationError::ProjectMismatch),
                        Refusal::Missing => matches!(
                            error,
                            ProjectVerificationError::Runtime(StatefulRunStoreError::RunNotFound(
                                _
                            ))
                        ),
                        Refusal::Terminal | Refusal::InvalidOwner => unreachable!(),
                    });
                }
                writer.close().await?;
                assert_eq!(f.durable().await?, before);
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn successful_start_commits_prior_domain_writes_and_terminal_end_is_allowed() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
    let mut writer = f.writer().await?;
    let mut transaction = writer.begin_write().await?;
    sqlx::query("UPDATE stateful_runs SET updated_at_ms=117")
        .execute(transaction.connection())
        .await?;
    assert_eq!(
        commit_project_verification_start(transaction, &f.run, "owner", /*lease_ms*/ 60_000)
            .await?,
        1
    );
    writer.close().await?;
    let updated: i64 = sqlx::query_scalar("SELECT updated_at_ms FROM stateful_runs WHERE id='run'")
        .fetch_one(&mut f.observer)
        .await?;
    assert_eq!(
        (updated, f.lease().await?),
        (117, (1, Some("owner".into()), Some(60_000)))
    );
    sqlx::query("UPDATE stateful_runs SET status='cancelled'")
        .execute(&mut f.observer)
        .await?;
    f.end("owner", /*attempt*/ 1).await?;
    assert_eq!(f.lease().await?, (1, None, None));
    Ok(())
}

#[tokio::test]
async fn durable_observer_detects_nul_suffix_text_changes() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
    f.start("owner", /*lease_ms*/ 60_000).await?;
    sqlx::query("UPDATE stateful_acceptance_ledgers SET stalled_fingerprint=''")
        .execute(&mut f.observer)
        .await?;
    let before = f.durable().await?;
    sqlx::query("UPDATE stateful_acceptance_ledgers SET stalled_fingerprint=?")
        .bind("\0changed")
        .execute(&mut f.observer)
        .await?;
    assert_ne!(f.durable().await?, before);
    Ok(())
}
