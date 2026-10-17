#![expect(
    clippy::disallowed_methods,
    reason = "disposable project acceptance fixtures"
)]

use super::*;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectDomainReader;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreReader;
use codex_project_store::StoreRegistryError;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Row;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqlitePoolOptions;

use crate::AcceptanceChange;
use crate::AcceptanceKind;
use crate::AcceptanceOrigin;
use crate::CriterionTerms;
use crate::NewStatefulRun;
use crate::RunBudget;
use crate::StatefulRunStore;
use crate::WorkflowMode;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    store: StatefulRunStore,
    run: StatefulRunId,
    domain: &'static str,
}

impl Fixture {
    async fn new(domain: &'static str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let store = StatefulRunStore {
            pool: SqlitePoolOptions::new()
                .max_connections(/*max*/ 1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(location.database_path().as_path())
                        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                        .foreign_keys(/*on*/ true),
                )
                .await?,
        };
        let mut connection = store.pool.acquire().await?;
        PreparedDomains::prepare(&[DomainManifest {
            domain,
            migrations: &crate::storage::MIGRATOR,
        }])
        .await?
        .install(&mut connection)
        .await?;
        drop(connection);
        let run = StatefulRunId::parse("acceptance")?;
        store
            .create_run(
                run.clone(),
                NewStatefulRun {
                    project_id: identity.project_uuid.to_string(),
                    thread_ids: vec!["thread".into()],
                    goal: "Preserve the report.".into(),
                    mode: WorkflowMode::Autonomous,
                    budget: RunBudget {
                        max_continuations: 4,
                        max_elapsed_seconds: 600,
                    },
                },
            )
            .await?;
        Ok(Self {
            _directory: directory,
            location,
            store,
            run,
            domain,
        })
    }

    async fn reader(&self) -> Result<ProjectDomainReader> {
        let manifest = DomainManifest {
            domain: self.domain,
            migrations: &crate::storage::MIGRATOR,
        };
        let domains = PreparedDomains::prepare(&[manifest]).await?;
        Ok(ProjectStoreReader::open(&self.location, domains)
            .await?
            .into_domain_reader(&DomainManifest {
                domain: self.domain,
                migrations: &crate::storage::MIGRATOR,
            })
            .await?)
    }

    async fn seed(&self) -> Result<AcceptanceLedger> {
        Ok(self
            .store
            .revise_acceptance(
                &self.run,
                /*expected_revision*/ 0,
                vec![AcceptanceChange::Add {
                    origin: AcceptanceOrigin::Derived,
                    kind: AcceptanceKind::Manual,
                    statement: "à¤®à¤°à¤¾à¤ à¥€ report".into(),
                    request_span: None,
                    terms: CriterionTerms {
                        required: false,
                        ..CriterionTerms::default()
                    },
                }],
                "seed",
                /*project_root*/ None,
            )
            .await?)
    }

    async fn mutate(&self, sql: &'static str) -> Result {
        sqlx::raw_sql(sql).execute(&self.store.pool).await?;
        Ok(())
    }

    // Observe every durable cell including NULs, storage classes and REAL bits.
    async fn durable(&self) -> Result<std::collections::BTreeMap<String, Vec<Vec<String>>>> {
        let mut connection = self.store.pool.acquire().await?;
        let names: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
                .fetch_all(&mut *connection)
                .await?;
        let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
        let mut tables = std::collections::BTreeMap::new();
        for name in names {
            let columns: Vec<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                    .bind(&name)
                    .fetch_all(&mut *connection)
                    .await?;
            let fields = columns.iter().map(|name| {
                let name = quoted(name);
                format!("typeof({name}),CASE WHEN typeof({name}) IN ('text','blob') THEN hex(CAST({name} AS BLOB)) ELSE {name} END")
            }).collect::<Vec<_>>().join(",");
            let mut rows = Vec::new();
            for row in sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {fields} FROM {}",
                quoted(&name)
            )))
            .fetch_all(&mut *connection)
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
                        _ => return Err(format!("unexpected storage: {storage}").into()),
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
async fn absent_ledger_is_read_without_materializing_request_or_writing() -> Result {
    let f = Fixture::new("stateful-runtime").await?;
    let before = f.durable().await?;
    let mut reader = f.reader().await?;
    for _ in 0..2 {
        assert_eq!(
            read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await?,
            AcceptanceLedger::empty(f.run.clone())
        );
    }
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn snapshot_retains_whole_ledger_then_refreshes_latest_failure() -> Result {
    let f = Fixture::new("stateful-runtime").await?;
    let expected = f.seed().await?;
    let mut reader = f.reader().await?;
    let snapshot = reader.begin_snapshot().await?;
    f.mutate("INSERT INTO stateful_acceptance_evidence(run_id,ordinal,source,outcome,workspace_generation,criterion_revision,source_id,observed_at_ms) VALUES ('acceptance',1,'manual','observed',0,1,'old',1),('acceptance',1,'noCheck','unavailable',1,1,'latest',2);
        UPDATE stateful_acceptance_ledgers SET revision=2,workspace_generation=1;
        INSERT INTO stateful_acceptance_pending VALUES ('acceptance','pending',1);
        INSERT INTO stateful_acceptance_steering VALUES ('acceptance','steer','keep caveat',1)").await?;
    assert_eq!(
        read_project_acceptance_ledger(snapshot, &f.run).await?,
        expected
    );
    let before = f.durable().await?;
    let expected = f.store.acceptance_ledger(&f.run).await?;
    assert_eq!(
        expected.criteria[0]
            .evidence
            .as_ref()
            .map(|item| item.source_id.as_str()),
        Some("latest")
    );
    assert_eq!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await?,
        expected
    );
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn missing_foreign_or_nul_suffixed_project_refuses_without_writes() -> Result {
    for project in [None, Some("foreign"), Some("nul")] {
        let f = Fixture::new("stateful-runtime").await?;
        if let Some(project) = project {
            let sql = if project == "nul" {
                "UPDATE stateful_runs SET project_id=project_id||char(0)||'hidden'"
            } else {
                "UPDATE stateful_runs SET project_id='00000000-0000-4000-8000-000000000001'"
            };
            f.mutate(sql).await?;
        }
        let before = f.durable().await?;
        let mut reader = f.reader().await?;
        let missing = StatefulRunId::parse("missing")?;
        let run = if project.is_none() { &missing } else { &f.run };
        let error = read_project_acceptance_ledger(reader.begin_snapshot().await?, run)
            .await
            .unwrap_err();
        assert!(matches!(
            (project, error),
            (
                None,
                ProjectAcceptanceReadError::Runtime(StatefulRunStoreError::RunNotFound(_))
            ) | (Some(_), ProjectAcceptanceReadError::ProjectMismatch)
        ));
        assert!(matches!(
            reader.begin_snapshot().await,
            Err(ProjectReadError::UnfinishedOperation)
        ));
        reader.close().await?;
        assert_eq!(f.durable().await?, before);
        f.store.pool.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn identically_shaped_foreign_domain_does_not_authorize_runtime() -> Result {
    let f = Fixture::new("other-domain").await?;
    let before = f.durable().await?;
    let mut reader = f.reader().await?;
    assert!(matches!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await,
        Err(ProjectAcceptanceReadError::Snapshot(
            ProjectReadError::Registry(StoreRegistryError::IncompatibleHistory)
        ))
    ));
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn excess_rows_or_bytes_refuse_instead_of_hiding_binding_state() -> Result {
    for mutation in [
        "INSERT INTO stateful_acceptance_steering VALUES ('acceptance','s','reason',CAST(zeroblob(1048577) AS TEXT))",
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<65) INSERT INTO stateful_acceptance_steering SELECT 'acceptance','s'||i,'reason',i FROM n",
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<4097) INSERT INTO stateful_acceptance_pending SELECT 'acceptance','p'||i,i FROM n",
        "UPDATE stateful_acceptance_criteria SET statement=CAST(zeroblob(1048577) AS TEXT)",
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<129) INSERT INTO stateful_acceptance_evidence(run_id,ordinal,source,outcome,workspace_generation,criterion_revision,source_id,observed_at_ms) SELECT 'acceptance',1,'noCheck','unavailable',0,1,'source',i FROM n",
        "WITH RECURSIVE n(i) AS (VALUES(2) UNION ALL SELECT i+1 FROM n WHERE i<33) INSERT INTO stateful_acceptance_criteria(run_id,ordinal,criterion_id,origin,kind,state,statement,requirement,required,depends_on_json,artifacts_json,checker_json,revision,ledger_revision,created_at_ms,updated_at_ms) SELECT 'acceptance',i,'C'||i,'derived','manual','active','s','r',0,'[]','[]','[]',1,1,1,1 FROM n",
    ] {
        let f = Fixture::new("stateful-runtime").await?;
        f.seed().await?;
        f.mutate(mutation).await?;
        let before = f.durable().await?;
        let mut reader = f.reader().await?;
        assert!(
            matches!(
                read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await,
                Err(ProjectAcceptanceReadError::ReadLimit)
            ),
            "mutation: {mutation}"
        );
        assert!(matches!(
            reader.begin_snapshot().await,
            Err(ProjectReadError::UnfinishedOperation)
        ));
        reader.close().await?;
        assert_eq!(f.durable().await?, before);
        f.store.pool.close().await;
    }
    Ok(())
}

#[path = "project_acceptance_legacy_tests.rs"]
mod legacy;
