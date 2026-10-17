use super::*;
use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::StoreIdentity;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use std::borrow::Cow;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "project_binding_tests.rs"]
mod binding;
type Durable = (
    Option<StatefulRun>,
    Option<StatefulObligation>,
    crate::AcceptanceLedger,
    Vec<String>,
);

struct Fixture {
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    store: crate::StatefulRunStore,
    run: StatefulRun,
    _directory: tempfile::TempDir,
}

async fn domains() -> Result<PreparedDomains> {
    let other = Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            /*version*/ 1,
            "other".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static("CREATE TABLE other_domain(value TEXT NOT NULL);"),
            /*no_tx*/ false,
        )]),
        ..Migrator::DEFAULT
    };
    Ok(PreparedDomains::prepare(&[
        DomainManifest {
            domain: "stateful-runtime",
            migrations: crate::project_store_domain().unwrap().migrations,
        },
        DomainManifest {
            domain: "test-domain",
            migrations: &other,
        },
    ])
    .await?)
}

async fn fixture() -> Result<Fixture> {
    let directory = tempfile::tempdir()?;
    let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
    let identity = codex_project_store::initialize_local_store(&location).await?;
    let sqlite = SqliteConfig::new_for_testing(directory.path().abs());
    let pool = sqlite
        .open_read_write_pool(location.database_path().as_path())
        .await?;
    let mut connection = pool.acquire().await?;
    domains().await?.install(&mut connection).await?;
    drop(connection);
    let store = crate::StatefulRunStore { pool };
    let run = store
        .create_run(
            StatefulRunId::parse("run")?,
            crate::NewStatefulRun {
                project_id: identity.project_uuid.to_string(),
                thread_ids: vec!["thread".into()],
                goal: "Do the work.".into(),
                mode: crate::WorkflowMode::Autonomous,
                budget: crate::RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await?;
    Ok(Fixture {
        location,
        identity,
        store,
        run,
        _directory: directory,
    })
}

fn obligation(f: &Fixture, text: &str) -> (String, NewObligation) {
    (
        "final".into(),
        NewObligation {
            project_id: f.identity.project_uuid.to_string(),
            run_id: f.run.id.clone(),
            packet: crate::ObligationPacket {
                examined: vec![text.into()],
                ..Default::default()
            },
            provenance_source_id: "test".into(),
        },
    )
}

fn request<'a>(f: &'a Fixture, commit: &'a AcceptanceCommit) -> ProjectRunCompletion<'a> {
    ProjectRunCompletion {
        run_id: &f.run.id,
        update: StatefulRunUpdate {
            expected_revision: f.run.revision,
            status: StatefulRunStatus::Completed,
            strategy: None,
            result: Some("done".into()),
        },
        acceptance: commit,
        final_obligation: Some(obligation(f, "done")),
    }
}

async fn stage(writer: &mut ProjectStoreWriter) -> Result<ProjectWriteTransaction<'_>> {
    let mut transaction = writer.begin_write().await?;
    sqlx::query("INSERT INTO other_domain VALUES ('changed')")
        .execute(transaction.connection())
        .await?;
    Ok(transaction)
}

async fn durable(f: &Fixture) -> Result<Durable> {
    Ok((
        f.store.get_run(&f.run.id).await?,
        f.store.latest_obligation(&f.run.id).await?,
        f.store.acceptance_ledger(&f.run.id).await?,
        sqlx::query_scalar("SELECT value FROM other_domain ORDER BY value")
            .fetch_all(&f.store.pool)
            .await?,
    ))
}

#[tokio::test]
async fn acceptance_commits_prior_domain_write_run_and_final_obligation_together() -> Result {
    let f = fixture().await?;
    let commit = crate::acceptance_test_support::settled_commit(
        &f.store, &f.run.id, "owner", /*validated_obligation_sequence*/ None,
    )
    .await;
    let mut writer = ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
    let (run, obligation) =
        commit_project_run(stage(&mut writer).await?, request(&f, &commit)).await?;
    let expected = StatefulRun {
        status: StatefulRunStatus::Completed,
        result: Some("done".into()),
        revision: f.run.revision + 1,
        updated_at_ms: run.updated_at_ms,
        ..f.run.clone()
    };
    let persisted = durable(&f).await?;
    let expected_other = vec!["changed".to_string()];
    assert_eq!(
        (run, persisted.0, persisted.1, persisted.3),
        (expected.clone(), Some(expected), obligation, expected_other)
    );
    writer.close().await?;
    f.store.pool.close().await;
    Ok(())
}

#[derive(Clone, Copy)]
enum Refusal {
    Stale,
    ForeignRun,
    ForeignFinal,
    ForeignChild,
    Steering,
}

#[tokio::test]
async fn stale_acceptance_ownership_and_steering_refusals_drop_all_domain_writes() -> Result {
    for case in [
        Refusal::Stale,
        Refusal::ForeignRun,
        Refusal::ForeignFinal,
        Refusal::ForeignChild,
        Refusal::Steering,
    ] {
        let f = fixture().await?;
        let mut commit = crate::acceptance_test_support::settled_commit(
            &f.store, &f.run.id, "owner", /*validated_obligation_sequence*/ None,
        )
        .await;
        match case {
            Refusal::Stale => commit.ledger_revision += 1,
            Refusal::ForeignRun => {
                sqlx::query("UPDATE stateful_runs SET project_id='foreign'")
                    .execute(&f.store.pool)
                    .await?;
            }
            Refusal::ForeignFinal => {}
            Refusal::ForeignChild => {
                let (id, value) = obligation(&f, "child");
                f.store.append_obligation(id, value).await?;
                sqlx::query("UPDATE stateful_obligations SET project_id='foreign'")
                    .execute(&f.store.pool)
                    .await?;
            }
            Refusal::Steering => {
                f.store
                    .submit_steering(
                        crate::SteeringId::parse("steer")?,
                        crate::NewSteeringInstruction {
                            project_id: f.run.value.project_id.clone(),
                            run_id: f.run.id.clone(),
                            input: "Change scope".into(),
                            affected_obligation_ids: vec![],
                        },
                    )
                    .await?;
            }
        }
        let before = durable(&f).await?;
        let mut request = request(&f, &commit);
        if matches!(case, Refusal::ForeignFinal) {
            request.final_obligation.as_mut().unwrap().1.project_id = "foreign".into();
        }
        let mut writer =
            ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
        let error = commit_project_run(stage(&mut writer).await?, request)
            .await
            .unwrap_err();
        assert!(match case {
            Refusal::Stale => matches!(
                error,
                ProjectRunCompletionError::Runtime(StatefulRunStoreError::AcceptanceChanged)
            ),
            Refusal::Steering => matches!(
                error,
                ProjectRunCompletionError::Runtime(
                    StatefulRunStoreError::UnresolvedSteering { .. }
                )
            ),
            Refusal::ForeignRun | Refusal::ForeignFinal | Refusal::ForeignChild =>
                matches!(error, ProjectRunCompletionError::ProjectMismatch),
        });
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        assert_eq!(durable(&f).await?, before);
        f.store.pool.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn late_obligation_collision_rolls_back_terminal_and_lease_mutations() -> Result {
    let f = fixture().await?;
    let (id, value) = obligation(&f, "earlier");
    let original = f.store.append_obligation(id, value).await?;
    let commit = crate::acceptance_test_support::settled_commit(
        &f.store,
        &f.run.id,
        "owner",
        Some(original.sequence),
    )
    .await;
    let before = durable(&f).await?;
    let mut writer = ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
    assert!(matches!(
        commit_project_run(stage(&mut writer).await?, request(&f, &commit)).await,
        Err(ProjectRunCompletionError::Runtime(
            StatefulRunStoreError::ObligationIdentityConflict(_)
        ))
    ));
    writer.close().await?;
    assert_eq!(durable(&f).await?, before);
    f.store.pool.close().await;
    Ok(())
}
