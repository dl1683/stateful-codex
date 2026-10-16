//! Inactive composition only. Synthetic acceptance rows are not checker receipts.
//! No native routing, source certificate, cancellation or lost-ack guarantee.
use std::collections::BTreeMap;

use codex_project_intelligence::PreparedProjectNoLearning;
use codex_project_intelligence::ProjectKnowledgeReader;
use codex_project_intelligence::ProjectKnowledgeWriteError;
use codex_project_intelligence::revalidate_project_no_reusable_learning;
use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreReader;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use codex_project_store::StoreIdentity;
use codex_stateful_runtime::AcceptanceCommit;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::ProjectRunCompletion;
use codex_stateful_runtime::ProjectRunCompletionError;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::VerificationClaim;
use codex_stateful_runtime::commit_project_run;
use codex_stateful_runtime::revalidate_project_run_start;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::Row;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

static PI: sqlx::migrate::Migrator = sqlx::migrate!("../../project-intelligence/migrations");
static RT: sqlx::migrate::Migrator = sqlx::migrate!("../../stateful-runtime/migrations");
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Rows = Vec<BTreeMap<String, String>>;

#[derive(Debug, PartialEq)]
struct Durable {
    schema: Vec<(String, String, String, Option<String>)>,
    tables: BTreeMap<String, Rows>,
}

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    observer: SqliteConnection,
    prepared: PreparedProjectNoLearning,
    run: StatefulRunId,
    acceptance: AcceptanceCommit,
}

async fn domains() -> Result<PreparedDomains> {
    Ok(PreparedDomains::prepare(&[
        DomainManifest {
            domain: "project-intelligence",
            migrations: &PI,
        },
        DomainManifest {
            domain: "stateful-runtime",
            migrations: &RT,
        },
    ])
    .await?)
}

impl Fixture {
    #[expect(
        clippy::disallowed_methods,
        reason = "disposable project composition fixture"
    )]
    async fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let mut observer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(location.database_path().as_path())
                .foreign_keys(/*on*/ true),
        )
        .await?;
        domains().await?.install(&mut observer).await?;
        let project = identity.project_uuid.to_string();
        sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,kind,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node',?,'project','','active',1,0,0)")
            .bind(&project).execute(&mut observer).await?;
        for id in ["one", "two"] {
            sqlx::query("INSERT INTO blackboard_entries VALUES (?,?,'node',1,0,0)")
                .bind(id)
                .bind(&project)
                .execute(&mut observer)
                .await?;
            sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES (?,1,'note','existing user note',5000,'unverified','normal','not_promoted','active','user','synthetic',0)")
                .bind(id).execute(&mut observer).await?;
        }
        sqlx::query("INSERT INTO capture_identity_aliases VALUES (?,'one','v1','note','user','','existing user note','existing user note',0)")
            .bind(&project).execute(&mut observer).await?;
        sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms,max_continuations,max_elapsed_seconds,continuations_used) VALUES ('run',?,'Do work.','autonomous','running',0,1,100,100,4,600,0)")
            .bind(&project).execute(&mut observer).await?;
        // Deliberately synthetic host admission and evidence; no command is run.
        sqlx::raw_sql("INSERT INTO stateful_run_threads VALUES ('run',0,'thread');
            INSERT INTO stateful_acceptance_ledgers(run_id,revision,workspace_generation,observed_executions,stalled_completions,stalled_fingerprint,verification_attempt,verification_owner,verification_lease_expires_at_ms,updated_at_ms)
            VALUES ('run',1,0,1,0,'',1,'synthetic',9223372036854775807,100);
            INSERT INTO stateful_acceptance_criteria(run_id,ordinal,criterion_id,origin,kind,state,statement,requirement,required,depends_on_json,expected_observation,span_start,span_end,artifacts_json,checker_json,check_command,check_cwd,plan_criterion_revision,plan_checker_digest,plan_ledger_revision,revision,ledger_revision,created_at_ms,updated_at_ms)
            VALUES ('run',1,'criterion','user','check','active','Do work.','Do work.',1,'[]','exit 0',0,8,'[\"out\"]','[\"check\"]','synthetic-check','.',1,'checker',1,1,1,100,100);
            INSERT INTO stateful_acceptance_evidence(run_id,ordinal,source,outcome,command,exit_code,output_tail,output_digest,artifact_digest,checker_digest,workspace_generation,criterion_revision,source_id,observed_at_ms)
            VALUES ('run',1,'hostCommand','passed','synthetic-check',0,'','output','artifact','checker',0,1,'synthetic',100);")
            .execute(&mut observer).await?;
        let mut reader = ProjectKnowledgeReader::from_project_store(
            ProjectStoreReader::open(&location, domains().await?).await?,
        )
        .await?;
        let prepared = reader
            .prepare_no_reusable_learning(&identity, /*run_created_at_ms*/ 100)
            .await?;
        reader.close().await?;
        let observed = |digest: &str| {
            BTreeMap::from([(
                1,
                ArtifactState::Observed {
                    digest: digest.into(),
                    missing: vec![],
                },
            )])
        };
        Ok(Self {
            _directory: directory,
            location,
            identity,
            observer,
            prepared,
            run: StatefulRunId::parse("run")?,
            acceptance: AcceptanceCommit {
                ledger_revision: 1,
                workspace_generation: 0,
                artifacts: observed("artifact"),
                checkers: observed("checker"),
                verification: VerificationClaim {
                    owner: "synthetic".into(),
                    attempt: 1,
                },
                validated_obligation_sequence: None,
            },
        })
    }

    async fn writer(&self) -> Result<ProjectStoreWriter> {
        Ok(ProjectStoreWriter::open(&self.location, domains().await?, &self.identity).await?)
    }

    async fn stage<'a>(
        &self,
        writer: &'a mut ProjectStoreWriter,
    ) -> Result<ProjectWriteTransaction<'a>> {
        let mut transaction = writer.begin_write().await?;
        sqlx::query("UPDATE hierarchy_nodes SET updated_at_ms=17 WHERE id='node'")
            .execute(transaction.connection())
            .await?;
        Ok(transaction)
    }

    fn request(&self) -> ProjectRunCompletion<'_> {
        ProjectRunCompletion {
            run_id: &self.run,
            update: StatefulRunUpdate {
                expected_revision: 1,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some("synthetic done".into()),
            },
            acceptance: &self.acceptance,
            final_obligation: None,
        }
    }

    async fn validate<'a>(
        &self,
        transaction: ProjectWriteTransaction<'a>,
    ) -> Result<ProjectWriteTransaction<'a>> {
        let transaction =
            revalidate_project_run_start(transaction, &self.run, self.prepared.run_created_at_ms())
                .await?;
        Ok(revalidate_project_no_reusable_learning(transaction, &self.prepared).await?)
    }

    async fn durable(&mut self) -> Result<Durable> {
        let schema: Vec<(String, String, String, Option<String>)> =
            sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
                .fetch_all(&mut self.observer)
                .await?;
        let mut tables = BTreeMap::new();
        for (_, name, _, _) in schema.iter().filter(|row| row.0 == "table") {
            let mut columns: Vec<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                    .bind(name)
                    .fetch_all(&mut self.observer)
                    .await?;
            let without_rowid: bool = sqlx::query_scalar(
                "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?",
            )
            .bind(name)
            .fetch_one(&mut self.observer)
            .await?;
            if !without_rowid {
                columns.insert(/*index*/ 0, "_rowid_".into());
            }
            // Identifiers come exclusively from this disposable fixture's schema.
            let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
            let expressions = columns
                .iter()
                .map(|name| format!("quote({})", quoted(name)))
                .collect::<Vec<_>>()
                .join(",");
            let query = format!("SELECT {expressions} FROM {}", quoted(name));
            let mut rows = Vec::new();
            for row in sqlx::query(sqlx::AssertSqlSafe(query))
                .fetch_all(&mut self.observer)
                .await?
            {
                let values = columns
                    .iter()
                    .enumerate()
                    .map(|(index, name)| Ok((name.clone(), row.try_get::<String, _>(index)?)))
                    .collect::<std::result::Result<BTreeMap<_, _>, sqlx::Error>>()?;
                rows.push(values);
            }
            rows.sort();
            tables.insert(name.clone(), rows);
        }
        Ok(Durable { schema, tables })
    }
}

#[tokio::test]
async fn no_learning_composition_commits_terminal_state_and_consumes_lease_atomically() -> Result {
    let mut f = Fixture::new().await?;
    let mut expected = f.durable().await?;
    let mut writer = f.writer().await?;
    let transaction = f.validate(f.stage(&mut writer).await?).await?;
    let (run, obligation) = commit_project_run(transaction, f.request()).await?;
    writer.close().await?;
    assert_eq!(obligation, None);
    let row = &mut expected
        .tables
        .get_mut("stateful_runs")
        .ok_or("run table")?[0];
    row.insert("status".into(), "'completed'".into());
    row.insert("result".into(), "'synthetic done'".into());
    row.insert("revision".into(), "2".into());
    row.insert("updated_at_ms".into(), run.updated_at_ms.to_string());
    let ledger = &mut expected
        .tables
        .get_mut("stateful_acceptance_ledgers")
        .ok_or("ledger table")?[0];
    ledger.insert("verification_owner".into(), "NULL".into());
    ledger.insert("verification_lease_expires_at_ms".into(), "NULL".into());
    expected
        .tables
        .get_mut("hierarchy_nodes")
        .ok_or("marker table")?[0]
        .insert("updated_at_ms".into(), "17".into());
    assert_eq!(f.durable().await?, expected);
    f.observer.close().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_refusals_preserve_all_rows_and_verification_lease() -> Result {
    for mutation in [
        "UPDATE stateful_runs SET revision=2",
        "UPDATE stateful_acceptance_ledgers SET revision=2",
        "UPDATE stateful_acceptance_ledgers SET workspace_generation=1",
        "UPDATE stateful_acceptance_ledgers SET verification_lease_expires_at_ms=0",
        "UPDATE stateful_acceptance_ledgers SET verification_attempt=2,verification_owner='replacement'",
        "INSERT INTO stateful_acceptance_pending VALUES ('run','pending',100)",
        "UPDATE stateful_acceptance_evidence SET outcome='failed',exit_code=1",
        "INSERT INTO stateful_obligations(id,project_id,run_id,packet_json,provenance_source_id,revision,created_at_ms) SELECT 'late',project_id,id,'{\"examined\":[\"synthetic\"]}','synthetic',1,100 FROM stateful_runs",
        "INSERT INTO stateful_steering SELECT 'steer',project_id,id,'Change scope.','[]','submitted',NULL,NULL,1,100,100 FROM stateful_runs",
        "INSERT INTO stateful_steering SELECT 'steer',project_id,id,'Change scope.','[]','applied',0,NULL,1,100,100 FROM stateful_runs",
        // Fires after acceptance consumes the lease, before the terminal update.
        "CREATE TRIGGER late_failure BEFORE UPDATE ON stateful_runs WHEN (SELECT verification_owner FROM stateful_acceptance_ledgers WHERE run_id=NEW.id) IS NULL BEGIN SELECT RAISE(ABORT,'synthetic late failure after lease consumption'); END",
    ] {
        let mut f = Fixture::new().await?;
        let late = mutation.starts_with("CREATE TRIGGER");
        if !late {
            sqlx::raw_sql(mutation).execute(&mut f.observer).await?;
        }
        let before = f.durable().await?;
        let mut writer = f.writer().await?;
        let mut transaction = f.validate(f.stage(&mut writer).await?).await?;
        if late {
            // Test-only fault after admission; rollback must remove this DDL too.
            sqlx::raw_sql(mutation)
                .execute(transaction.connection())
                .await?;
        }
        let error = commit_project_run(transaction, f.request())
            .await
            .unwrap_err();
        let ProjectRunCompletionError::Runtime(error) = error else {
            panic!("unexpected refusal for {mutation}: {error}");
        };
        let correct = if mutation.contains("stateful_runs SET revision") {
            matches!(
                error,
                StatefulRunStoreError::RevisionConflict {
                    expected: 1,
                    actual: 2
                }
            )
        } else if mutation.contains("'submitted'") {
            matches!(error, StatefulRunStoreError::UnresolvedSteering { .. })
        } else if mutation.starts_with("CREATE TRIGGER") {
            matches!(&error, StatefulRunStoreError::Storage(sqlx::Error::Database(value))
                if value.message() == "synthetic late failure after lease consumption")
        } else if mutation.contains("'applied'") {
            matches!(&error, StatefulRunStoreError::AcceptanceGate(reason)
                if reason.contains("reconcile it with stateful_acceptance_update"))
        } else if mutation.contains("stateful_acceptance_pending") {
            matches!(&error, StatefulRunStoreError::AcceptanceGate(reason)
                if reason.contains("have not finished or have unaccounted effects"))
        } else if mutation.contains("'failed'") {
            matches!(&error, StatefulRunStoreError::AcceptanceGate(reason)
                if reason.contains("failed (exit 1)"))
        } else {
            matches!(error, StatefulRunStoreError::AcceptanceChanged)
        };
        assert!(correct, "{mutation}: {error}");
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        assert_eq!(f.durable().await?, before, "{mutation}");
        f.observer.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn knowledge_or_run_binding_refusal_never_reaches_terminal_commit() -> Result {
    for mutation in [
        "UPDATE stateful_runs SET created_at_ms=101",
        "UPDATE stateful_runs SET project_id='foreign'",
        "UPDATE stateful_runs SET id='other'",
        "UPDATE project_intelligence_revisions SET revision=revision+1",
        "UPDATE capsule_eligibility_generations SET generation=generation+1",
        "INSERT INTO capture_identity_coverage(project_id,watermark) SELECT project_id,0 FROM stateful_runs",
        // Simulated domain mutations execute the compiled invalidation triggers.
        "UPDATE blackboard_entries SET revision=2 WHERE id='one'; INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES ('one',2,'note','new agent note',5000,'unverified','normal','not_promoted','active','agent','synthetic',100)",
        "INSERT INTO blackboard_relations(id,project_id,from_entry_id,to_entry_id,kind,note,confidence_basis_points,provenance_kind,provenance_source_id,revision,created_at_ms,updated_at_ms) SELECT 'relation',project_id,'one','two','related_to',NULL,5000,'agent','synthetic',1,100,100 FROM stateful_runs",
        "UPDATE capture_identity_aliases SET retired=1 WHERE entry_id='one'",
    ] {
        let mut f = Fixture::new().await?;
        // The absent-run case uses a different requested ID, preserving child FKs.
        if mutation == "UPDATE stateful_runs SET id='other'" {
            f.run = StatefulRunId::parse("missing")?;
        } else {
            sqlx::raw_sql(mutation).execute(&mut f.observer).await?;
        }
        let before = f.durable().await?;
        let mut writer = f.writer().await?;
        let error = f
            .validate(f.stage(&mut writer).await?)
            .await
            .err()
            .ok_or("must refuse")?;
        let correct = if mutation.contains("created_at_ms=101") {
            matches!(
                error.downcast_ref::<ProjectRunCompletionError>(),
                Some(ProjectRunCompletionError::RunStartMismatch)
            )
        } else if mutation.contains("project_id='foreign'") {
            matches!(
                error.downcast_ref::<ProjectRunCompletionError>(),
                Some(ProjectRunCompletionError::ProjectMismatch)
            )
        } else if mutation.contains("SET id='other'") {
            matches!(error.downcast_ref::<ProjectRunCompletionError>(),
                Some(ProjectRunCompletionError::Runtime(StatefulRunStoreError::RunNotFound(id))) if id == "missing")
        } else {
            matches!(
                error.downcast_ref::<ProjectKnowledgeWriteError>(),
                Some(ProjectKnowledgeWriteError::SnapshotChanged)
            )
        };
        assert!(correct, "{mutation}: {error}");
        writer.close().await?;
        assert_eq!(f.durable().await?, before, "{mutation}");
        f.observer.close().await?;
    }
    // A valid token from another admitted project must not authorize this writer.
    let mut f = Fixture::new().await?;
    let other = Fixture::new().await?;
    let before = f.durable().await?;
    let mut writer = f.writer().await?;
    let transaction = revalidate_project_run_start(
        f.stage(&mut writer).await?,
        &f.run,
        other.prepared.run_created_at_ms(),
    )
    .await?;
    assert!(matches!(
        revalidate_project_no_reusable_learning(transaction, &other.prepared).await,
        Err(ProjectKnowledgeWriteError::IdentityMismatch)
    ));
    writer.close().await?;
    assert_eq!(f.durable().await?, before);
    f.observer.close().await?;
    other.observer.close().await?;
    Ok(())
}
