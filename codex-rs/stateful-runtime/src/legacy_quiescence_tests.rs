use super::*;
use crate::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    _home: tempfile::TempDir,
    sqlite: SqliteConfig,
    store: StatefulRunStore,
    connection: sqlx::pool::PoolConnection<sqlx::Sqlite>,
}

impl Fixture {
    async fn quiesce(&self, project: &str) -> std::result::Result<(), LegacyQuiescenceError> {
        let mut installer = LegacyRuntimeQuiescence::open(&self.sqlite).await?;
        let result = installer.quiesce_project(project).await;
        installer.close().await?;
        result
    }

    async fn new() -> Result<Self> {
        let home = tempfile::tempdir()?;
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let store = StatefulRunStore::open(&sqlite).await?;
        for name in ["a", "b"] {
            let id = StatefulRunId::parse(name)?;
            store
                .create_run(
                    id.clone(),
                    NewStatefulRun {
                        project_id: name.into(),
                        thread_ids: vec![name.into()],
                        goal: "Retain exact Marathi: नमस्कार".into(),
                        mode: WorkflowMode::Autonomous,
                        budget: RunBudget {
                            max_continuations: 4,
                            max_elapsed_seconds: 600,
                        },
                    },
                )
                .await?;
            store
                .append_obligation(
                    name.into(),
                    NewObligation {
                        project_id: name.into(),
                        run_id: id.clone(),
                        packet: ObligationPacket {
                            next: vec!["Retain this source.".into()],
                            ..Default::default()
                        },
                        provenance_source_id: "source".into(),
                    },
                )
                .await?;
            store
                .submit_steering(
                    SteeringId::parse(name)?,
                    NewSteeringInstruction {
                        project_id: name.into(),
                        run_id: id.clone(),
                        input: "Keep this caveat.".into(),
                        affected_obligation_ids: Vec::new(),
                    },
                )
                .await?;
            store
                .record_turn_attribution(NewStatefulTurnMeasurement {
                    run_id: id.clone(),
                    project_id: name.into(),
                    thread_id: name.into(),
                    turn_id: name.into(),
                    status: StatefulTurnStatus::Completed,
                    duration_ms: 1,
                    attribution_counters: StatefulAttributionCounters::default(),
                })
                .await?;
            crate::acceptance_test_support::settled_commit(&store, &id, "owner", None).await;
            store.begin_command(&id, "pending").await?;
            store
                .claim_autonomous_continuation(
                    &id,
                    AutonomousClaimRequest {
                        owner_id: "owner".into(),
                        previous_turn_id: "previous".into(),
                        lease_duration_ms: 60_000,
                    },
                )
                .await?;
            for statement in [
                "INSERT INTO stateful_host_answers VALUES (?,'thread','turn','answer','basis',1)",
                "INSERT INTO stateful_acceptance_request_parts VALUES (?,1,'steering')",
                "INSERT INTO stateful_acceptance_steering VALUES (?,'steering','reason',1)",
            ] {
                sqlx::query(statement)
                    .bind(name)
                    .execute(&store.pool)
                    .await?;
            }
        }
        let connection = store.pool.acquire().await?;
        Ok(Self {
            _home: home,
            sqlite,
            store,
            connection,
        })
    }
}

async fn snapshot(connection: &mut SqliteConnection) -> Result<Vec<(String, Vec<String>)>> {
    let mut result = Vec::new();
    for table in TABLES {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *connection)
                .await?;
        let projection = columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(",");
        let rows = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT json_array(rowid,{projection}) FROM {} ORDER BY rowid",
            quote(table)
        )))
        .fetch_all(&mut *connection)
        .await?;
        result.push((table.to_string(), rows));
    }
    Ok(result)
}

fn refused(error: sqlx::Error) {
    let error = error.as_database_error().expect("SQLite refusal");
    assert_eq!(
        (error.code().as_deref(), error.message()),
        (Some("1811"), "stateful project runtime writes are quiesced")
    );
}

#[tokio::test]
async fn already_open_native_clients_are_excluded_and_other_projects_survive_reopen() -> Result {
    let mut f = Fixture::new().await?;
    let a = StatefulRunId::parse("a")?;
    let current = f.store.get_run(&a).await?.unwrap();
    let update = StatefulRunUpdate {
        expected_revision: current.revision,
        status: StatefulRunStatus::Paused,
        strategy: None,
        result: None,
    };
    let before = Some(f.store.update_run(&a, update.clone()).await?);
    for table in TABLES {
        let field = if table == "stateful_runs" {
            "id"
        } else {
            "run_id"
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {} SET rowid=rowid WHERE {field}=?",
            quote(table)
        )))
        .bind("a")
        .execute(&mut *f.connection)
        .await?;
    }
    let objects = snapshot(&mut f.connection).await?;
    assert!(
        objects.iter().all(|(_, rows)| !rows.is_empty()),
        "all fourteen tables populated"
    );
    f.quiesce("a").await?;
    for table in TABLES {
        let (field, value) = if table == "stateful_runs" {
            ("project_id", "a")
        } else {
            ("run_id", "a")
        };
        for sql in [
            format!(
                "UPDATE {} SET {field}={field} WHERE {field}=?",
                quote(table)
            ),
            format!("DELETE FROM {} WHERE {field}=?", quote(table)),
            format!("INSERT INTO {} ({field}) VALUES (?)", quote(table)),
        ] {
            refused(
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(value)
                    .execute(&mut *f.connection)
                    .await
                    .unwrap_err(),
            );
        }
    }
    assert_eq!(snapshot(&mut f.connection).await?, objects);
    let update = StatefulRunUpdate {
        expected_revision: before.as_ref().unwrap().revision,
        ..update
    };
    let error = f.store.update_run(&a, update.clone()).await.unwrap_err();
    assert!(error.to_string().contains("runtime writes are quiesced"));
    assert_eq!(f.store.get_run(&a).await?, before);
    let b = StatefulRunId::parse("b")?;
    f.store
        .update_run(
            &b,
            StatefulRunUpdate {
                expected_revision: f.store.get_run(&b).await?.unwrap().revision,
                ..update
            },
        )
        .await?;
    let reopened = StatefulRunStore::open(&f.sqlite).await?;
    f.quiesce("a").await?;
    assert_eq!(reopened.get_run(&a).await?, before);
    f.quiesce("b").await?;
    assert!(reopened.begin_command(&b, "after").await.is_err());
    Ok(())
}

#[tokio::test]
async fn replace_collisions_membership_moves_and_statement_conflicts_cannot_bypass() -> Result {
    let mut f = Fixture::new().await?;
    sqlx::query("UPDATE stateful_run_threads SET rowid=-1 WHERE run_id='a'")
        .execute(&mut *f.connection)
        .await?;
    f.quiesce("a").await?;
    sqlx::query("PRAGMA recursive_triggers=OFF")
        .execute(&mut *f.connection)
        .await?;
    let before = snapshot(&mut f.connection).await?;
    for statement in [
        "UPDATE stateful_runs SET project_id='b' WHERE id='a'",
        "UPDATE stateful_runs SET project_id='a' WHERE id='b'",
        "UPDATE OR REPLACE stateful_runs SET id='a' WHERE id='b'",
        "UPDATE OR REPLACE stateful_runs SET rowid=(SELECT rowid FROM stateful_runs WHERE id='a') WHERE id='b'",
        "INSERT OR REPLACE INTO stateful_run_threads(rowid,run_id,position,thread_id) SELECT rowid,'b',8,'new' FROM stateful_run_threads WHERE run_id='a'",
        "UPDATE OR REPLACE stateful_acceptance_criteria SET criterion_id=(SELECT criterion_id FROM stateful_acceptance_criteria WHERE run_id='a') WHERE run_id='b'",
        "UPDATE OR REPLACE stateful_turn_measurements SET thread_id='a',turn_id='a' WHERE run_id='b'",
        "UPDATE stateful_run_threads SET run_id='b' WHERE run_id='a'",
        "UPDATE stateful_run_threads SET run_id='a' WHERE run_id='b'",
        "INSERT OR IGNORE INTO stateful_run_threads VALUES ('a',8,'new')",
        "INSERT INTO stateful_run_threads VALUES ('a',0,'a') ON CONFLICT(run_id,thread_id) DO UPDATE SET position=excluded.position",
        "INSERT INTO stateful_run_threads VALUES ('b',9,'another')",
        "INSERT OR REPLACE INTO stateful_run_threads SELECT * FROM stateful_run_threads WHERE run_id='a'",
    ] {
        refused(
            sqlx::query(statement)
                .execute(&mut *f.connection)
                .await
                .unwrap_err(),
        );
        assert_eq!(snapshot(&mut f.connection).await?, before, "{statement}");
    }
    Ok(())
}

#[tokio::test]
async fn schema_history_and_registry_damage_refuse_without_partial_installation() -> Result {
    for mutation in [
        "CREATE TABLE unknown(value TEXT)",
        "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=1",
        "CREATE TABLE _stateful_runtime_quiescence(project_id TEXT)",
    ] {
        let mut f = Fixture::new().await?;
        sqlx::raw_sql(mutation).execute(&mut *f.connection).await?;
        let before = schema(&mut f.connection).await?;
        assert!(matches!(
            f.quiesce("a").await,
            Err(LegacyQuiescenceError::Incompatible)
        ));
        assert_eq!(schema(&mut f.connection).await?, before);
    }
    let mut f = Fixture::new().await?;
    f.quiesce("a").await?;
    let guard: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='_stateful_quiescence_registry_UPDATE'",
    )
    .fetch_one(&mut *f.connection)
    .await?;
    let before = schema(&mut f.connection).await?;
    for mutation in [
        "DELETE FROM _stateful_runtime_quiescence",
        "UPDATE _stateful_runtime_quiescence SET project_id='b'",
        "INSERT OR REPLACE INTO _stateful_runtime_quiescence SELECT * FROM _stateful_runtime_quiescence",
        "INSERT OR REPLACE INTO _stateful_runtime_quiescence(rowid,project_id,format,migration_digest) SELECT rowid,'b',format,migration_digest FROM _stateful_runtime_quiescence",
    ] {
        assert!(
            sqlx::query(mutation)
                .execute(&mut *f.connection)
                .await
                .is_err()
        );
        assert_eq!(schema(&mut f.connection).await?, before);
    }
    sqlx::raw_sql("DROP TRIGGER _stateful_quiescence_registry_UPDATE; UPDATE _stateful_runtime_quiescence SET migration_digest='unknown'").execute(&mut *f.connection).await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(guard.as_str()))
        .execute(&mut *f.connection)
        .await?;
    assert!(matches!(
        f.quiesce("b").await,
        Err(LegacyQuiescenceError::Incompatible)
    ));
    let projects: Vec<String> = sqlx::query_scalar(
        "SELECT project_id FROM _stateful_runtime_quiescence ORDER BY project_id",
    )
    .fetch_all(&mut *f.connection)
    .await?;
    assert_eq!(projects, vec!["a"]);
    Ok(())
}

#[tokio::test]
async fn redundant_ownership_and_shared_thread_ids_do_not_bypass_or_expand_exclusion() -> Result {
    let mut f = Fixture::new().await?;
    sqlx::query("UPDATE stateful_run_threads SET thread_id='shared'")
        .execute(&mut *f.connection)
        .await?;
    for table in [
        "stateful_obligations",
        "stateful_steering",
        "stateful_turn_measurements",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET project_id=CASE run_id WHEN 'a' THEN 'b' ELSE 'a' END"
        )))
        .execute(&mut *f.connection)
        .await?;
    }
    f.quiesce("a").await?;
    let before = snapshot(&mut f.connection).await?;
    for table in [
        "stateful_obligations",
        "stateful_steering",
        "stateful_turn_measurements",
    ] {
        for run in ["a", "b"] {
            refused(
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "UPDATE {table} SET project_id=project_id WHERE run_id=?"
                )))
                .bind(run)
                .execute(&mut *f.connection)
                .await
                .unwrap_err(),
            );
        }
    }
    assert_eq!(snapshot(&mut f.connection).await?, before);
    sqlx::query("UPDATE stateful_run_threads SET position=position WHERE run_id='b'")
        .execute(&mut *f.connection)
        .await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_before_commit_and_during_commit_requires_close_and_reconciliation() -> Result
{
    for phase in ["admission", "marker", "commit"] {
        let mut f = Fixture::new().await?;
        let predecessor = schema(&mut f.connection).await?;
        let rows = snapshot(&mut f.connection).await?;
        let mut installer = LegacyRuntimeQuiescence::open(&f.sqlite).await?;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut entered_tx = Some(entered_tx);
        let mut hook = move || {
            if let Some(sender) = entered_tx.take() {
                let _ = sender.send(());
            }
            release_rx
                .recv_timeout(Duration::from_secs(/*secs*/ 10))
                .is_ok()
        };
        if phase == "admission" {
            sqlx::query("BEGIN IMMEDIATE")
                .execute(&mut *f.connection)
                .await?;
        } else if phase == "commit" {
            installer
                .connection
                .lock_handle()
                .await?
                .set_commit_hook(hook);
        } else {
            installer
                .connection
                .lock_handle()
                .await?
                .set_update_hook(move |event| {
                    if event.table == "_stateful_runtime_quiescence" {
                        assert!(hook());
                    }
                });
        }
        let mut operation = Box::pin(installer.quiesce_project("a"));
        if phase == "admission" {
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(operation.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
        } else {
            tokio::select! {
                result = &mut operation => panic!("completed before release: {result:?}"),
                signal = tokio::time::timeout(Duration::from_secs(/*secs*/ 5), entered_rx) => { signal??; }
            }
        }
        drop(operation);
        assert!(matches!(
            installer.quiesce_project("b").await,
            Err(LegacyQuiescenceError::Unfinished)
        ));
        if phase == "admission" {
            sqlx::query("ROLLBACK").execute(&mut *f.connection).await?;
        } else {
            release_tx.send(())?;
        }
        installer.close().await?;
        assert_eq!(snapshot(&mut f.connection).await?, rows);
        let observed = schema(&mut f.connection).await?;
        if phase == "commit" {
            assert_ne!(observed, predecessor);
        } else {
            assert_eq!(observed, predecessor);
        }
        f.quiesce("a").await?;
        refused(
            sqlx::query("DELETE FROM stateful_runs WHERE id='a'")
                .execute(&mut *f.connection)
                .await
                .unwrap_err(),
        );
    }
    Ok(())
}

#[tokio::test]
async fn concurrent_installers_and_stale_wal_snapshot_fail_closed() -> Result {
    let mut f = Fixture::new().await?;
    sqlx::query("BEGIN").execute(&mut *f.connection).await?;
    let _ = sqlx::query("SELECT * FROM stateful_runs")
        .fetch_all(&mut *f.connection)
        .await?;
    let mut occupied = Vec::new();
    for _ in 0..4 {
        occupied.push(f.store.pool.acquire().await?);
    }
    let (a, b) = tokio::join!(f.quiesce("a"), f.quiesce("b"));
    a?;
    b?;
    let error = sqlx::query("UPDATE stateful_runs SET goal=goal WHERE id='a'")
        .execute(&mut *f.connection)
        .await
        .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("517")
    );
    sqlx::query("ROLLBACK").execute(&mut *f.connection).await?;
    refused(
        sqlx::query("UPDATE stateful_runs SET goal=goal WHERE id='a'")
            .execute(&mut *f.connection)
            .await
            .unwrap_err(),
    );
    Ok(())
}
