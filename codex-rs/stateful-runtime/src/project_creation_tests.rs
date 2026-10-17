#![expect(clippy::disallowed_methods, reason = "disposable project creation fixtures")]
use super::*;
use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::StoreIdentity;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::sqlite::SqliteConnectOptions;

use crate::RunBudget;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    observer: SqliteConnection,
    id: StatefulRunId,
    value: NewStatefulRun,
    domain: &'static str,
}
impl Fixture {
    async fn new(domain: &'static str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let mut observer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(location.database_path().as_path()).foreign_keys(/*on*/ true),
        ).await?;
        PreparedDomains::prepare(&[DomainManifest { domain, migrations: crate::project_store_domain()?.migrations }]).await?.install(&mut observer).await?;
        let value = NewStatefulRun {
            project_id: identity.project_uuid.to_string(), thread_ids: vec!["z".into(), "a".into()],
            goal: "Preserve Marathi: नमस्कार".into(), mode: WorkflowMode::Autonomous,
            budget: RunBudget { max_continuations: 4, max_elapsed_seconds: 600 },
        };
        Ok(Self { _directory: directory, location, identity, observer, id: StatefulRunId::parse("run")?, value, domain })
    }
    async fn writer(&self) -> Result<ProjectStoreWriter> {
        Ok(ProjectStoreWriter::open(&self.location, PreparedDomains::prepare(&[DomainManifest { domain: self.domain, migrations: crate::project_store_domain()?.migrations }]).await?, &self.identity).await?)
    }
    async fn create(&self) -> Result<StatefulRun> {
        let mut writer = self.writer().await?;
        let run = commit_project_run_creation(writer.begin_write().await?, self.id.clone(), self.value.clone()).await?;
        writer.close().await?;
        Ok(run)
    }
    async fn counts(&mut self) -> Result<(i64, i64, i64)> {
        Ok(sqlx::query_as("SELECT (SELECT COUNT(*) FROM stateful_runs), (SELECT COUNT(*) FROM stateful_run_threads), (SELECT COUNT(*) FROM stateful_acceptance_pending)").fetch_one(&mut self.observer).await?)
    }
    async fn refuse(&mut self) -> Result<ProjectRunCreationError> {
        let before = self.counts().await?;
        let mut writer = self.writer().await?;
        let error = commit_project_run_creation(writer.begin_write().await?, self.id.clone(), self.value.clone()).await.unwrap_err();
        assert!(matches!(writer.begin_write().await, Err(ProjectWriteError::UnfinishedOperation)));
        writer.close().await?;
        assert_eq!(self.counts().await?, before);
        Ok(error)
    }
}

#[tokio::test]
async fn modes_replay_and_terminal_state_preserve_entire_run() -> Result {
    for mode in [WorkflowMode::Socratic, WorkflowMode::Collaborative, WorkflowMode::Autonomous] {
        let mut f = Fixture::new("stateful-runtime").await?;
        f.value.mode = mode;
        let run = f.create().await?;
        let expected = StatefulRun { id: f.id.clone(), value: f.value.clone(), status: if mode == WorkflowMode::Socratic { StatefulRunStatus::Pending } else { StatefulRunStatus::Running }, strategy: None, strategy_revision: 0, result: None, continuations_used: 0, revision: 1, created_at_ms: run.created_at_ms, updated_at_ms: run.created_at_ms };
        assert_eq!(run, expected);
        assert!(run.created_at_ms > 0);
        assert_eq!(f.create().await?, run);
        sqlx::query("UPDATE stateful_runs SET status='cancelled', revision=2, result='retained', updated_at_ms=created_at_ms+1 WHERE id='run'").execute(&mut f.observer).await?;
        assert_eq!(f.create().await?, StatefulRun { status: StatefulRunStatus::Cancelled, revision: 2, result: Some("retained".into()), updated_at_ms: run.created_at_ms+1, ..run });
    }
    Ok(())
}

#[tokio::test]
async fn bindings_conflicts_and_input_errors_do_not_commit() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
    f.value.project_id = "foreign".into();
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::ProjectMismatch));
    f.value.project_id = f.identity.project_uuid.to_string();
    f.value.goal = " invalid".into();
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::Runtime(_)));
    f.value.goal = "valid".into();
    f.create().await?;
    f.value.goal = "different".into();
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::Runtime(StatefulRunStoreError::RunIdentityConflict(_))));
    sqlx::query("UPDATE stateful_runs SET project_id='foreign' WHERE id='run'").execute(&mut f.observer).await?;
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::ProjectMismatch));
    let mut other = Fixture::new("substitute-runtime").await?;
    assert!(matches!(other.refuse().await?, ProjectRunCreationError::Writer(_)));
    Ok(())
}

#[tokio::test]
async fn prior_writes_and_partial_thread_insert_are_atomic() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
    let mut writer = f.writer().await?;
    let mut guard = writer.begin_write().await?;
    sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) VALUES ('prior','other','goal','socratic','pending',0,1,1,1)").execute(guard.connection()).await?;
    commit_project_run_creation(guard, f.id.clone(), f.value.clone()).await?;
    writer.close().await?;
    assert_eq!(f.counts().await?, (2,2,0));
    // The first thread inserts, then the duplicate violates the primary key.
    f.id = StatefulRunId::parse("partial")?;
    f.value.thread_ids = vec!["same".into(), "same".into()];
    let mut writer = f.writer().await?;
    let mut guard = writer.begin_write().await?;
    sqlx::query("UPDATE stateful_runs SET goal='must roll back' WHERE id='prior'").execute(guard.connection()).await?;
    assert!(commit_project_run_creation(guard, f.id.clone(), f.value.clone()).await.is_err());
    writer.close().await?;
    assert_eq!(f.counts().await?, (2,2,0));
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT goal FROM stateful_runs WHERE id='prior'").fetch_one(&mut f.observer).await?, "goal");
    Ok(())
}

#[tokio::test]
async fn complete_projection_byte_and_thread_boundaries_precede_loading() -> Result {
    let mut f = Fixture::new("stateful-runtime").await?;
    let original = f.create().await?;
    // Independent formula: 14 run fields plus two (thread_id,position) pairs.
    let base = "run".len()+36+f.value.goal.len()+"autonomous".len()+"running".len()
        +1+1+original.created_at_ms.to_string().len()*2+1+3+1+4;
    let strategy = "x".repeat(/*n*/ 131_072-base);
    sqlx::query("UPDATE stateful_runs SET strategy=? WHERE id='run'").bind(&strategy).execute(&mut f.observer).await?;
    assert_eq!(f.create().await?, StatefulRun { strategy: Some(strategy), ..original });
    sqlx::query("UPDATE stateful_runs SET strategy=strategy||'x' WHERE id='run'").execute(&mut f.observer).await?;
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::ReadLimit));
    sqlx::query("UPDATE stateful_runs SET strategy=NULL WHERE id='run'").execute(&mut f.observer).await?;
    for position in 2..64 {
        let thread = format!("thread{position}");
        sqlx::query("INSERT INTO stateful_run_threads VALUES ('run',?,?)").bind(position).bind(&thread).execute(&mut f.observer).await?;
        f.value.thread_ids.push(thread);
    }
    assert_eq!(f.create().await?.value, f.value);
    sqlx::query("INSERT INTO stateful_run_threads VALUES ('run',64,'overflow')").execute(&mut f.observer).await?;
    assert!(matches!(f.refuse().await?, ProjectRunCreationError::ReadLimit));
    Ok(())
}
