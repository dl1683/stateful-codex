use super::activation::progress;
use super::activation::tool;
use super::*;
use codex_app_server_protocol::StatefulHostAnswer;
use codex_app_server_protocol::StatefulHostAnswerOutcome;
use pretty_assertions::assert_eq;
use runtime::PARTIAL_RESULT_HEADER;
use runtime::PARTIAL_UNVERIFIED_BASIS;
use serde_json::json;

/// B2, M2: host retention copying stale next is not a model progress write in the new turn.
async fn retention(retain_first: bool) -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let continue_text = format!(
        "Working.\n{}",
        BLOCK.replace("disposition: answer", "disposition: continue")
    );
    let retain_second = tool(
        "retain",
        "stateful_run_update",
        json!({"expectedRevision":2,"status":"completed","openIssues":["x"]}),
    );
    let script = if retain_first {
        vec![
            run_update("retain", "completed", &["x"]),
            message("first", &continue_text),
            message("final", ANSWER),
        ]
    } else {
        vec![
            progress(),
            message("first", &continue_text),
            retain_second,
            message("final", ANSWER),
        ]
    };
    let requests = script.len();
    let mock = responses::mount_sse_sequence(&h.responses_server, script).await;
    let id = h.start_turn().await?;
    h.outcome(&id, /*next*/ false).await?;
    assert_eq!(
        h.read(&run.id).await?.run.expect("run").status,
        StatefulRunStatus::Running
    );
    let (_, warnings, _) = h.outcome(&id, /*next*/ true).await?;
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Open issue (retained): x"))
    );
    let read = h.read(&run.id).await?;
    let durable = read.run.as_ref().expect("run");
    let record = read.host_answer.as_ref().expect("record");
    let recovery = read.recovery.as_ref().expect("recovery");
    assert_eq!(
        read,
        StatefulRunReadResponse {
            run: Some(StatefulRun {
                status: StatefulRunStatus::Answered,
                revision: run.revision + 2,
                continuations_used: 1,
                updated_at: durable.updated_at,
                result: Some(format!(
                    "{PARTIAL_RESULT_HEADER}- Open issue (retained): x\n\n{ANSWER}"
                )),
                ..run
            }),
            host_answer: Some(StatefulHostAnswer {
                turn_id: record.turn_id.clone(),
                answer: ANSWER.into(),
                basis: PARTIAL_UNVERIFIED_BASIS.into(),
                committed_at: record.committed_at,
                outcome: StatefulHostAnswerOutcome::Partial
            }),
            recovery: Some(codex_app_server_protocol::StatefulRunRecovery {
                lease_expires_at: recovery.lease_expires_at,
                previous_turn_id: Some(id.clone()),
                last_continuation_claimed_at: recovery.last_continuation_claimed_at
            }),
        }
    );
    assert!(recovery.last_continuation_claimed_at.is_some());
    assert_eq!(mock.requests().len(), requests);
    Ok(())
}

/// B3, C1: retained issues and accepted omission-only contracts survive pause/restart/resume.
async fn durable(omission: bool) -> Result<()> {
    let mut h = harness().await?;
    // Thread creation is lazy: materialize its rollout before testing a real restart.
    responses::mount_sse_once(
        &h.responses_server,
        message("initial", "Thread history initialized."),
    )
    .await;
    h.turn().await?;
    let mut run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let store = h.store().await?;
    let id = runtime::StatefulRunId::parse(&run.id)?;
    let items = if omission {
        let (ledger, _) = store.propose_uncovered(&id).await?;
        store
            .revise_acceptance(
                &id,
                ledger.revision,
                vec![runtime::AcceptanceChange::Accept {
                    ordinal: 1,
                    kind: None,
                    terms: runtime::TermsUpdate::default(),
                }],
                "accept",
                /*project_root*/ None,
            )
            .await?;
        let _: codex_app_server_protocol::StatefulRunPauseResponse = h
            .server
            .request(|request_id| ClientRequest::StatefulRunPause {
                request_id,
                params: codex_app_server_protocol::StatefulRunPauseParams {
                    run_id: run.id.clone(),
                    expected_revision: run.revision,
                },
            })
            .await?;
        run.revision += 1;
        vec![format!(
            "C1 ({QUESTION}): not certified by this terminal answer path"
        )]
    } else {
        store
            .apply_obligation_operation(
                &id,
                runtime::ObligationOperation::Retain {
                    issues: vec!["x".into()],
                },
                &runtime::OperationReceiptKey::parse("retain")?,
                "source",
            )
            .await?;
        vec!["Open issue (retained): x".to_string()]
    };
    assert!(h.server.shutdown_gracefully().await?.success());
    let restarted = TestAppServer::builder()
        .with_codex_home(h.codex_home.path())
        .build_initialized()
        .await?;
    drop(std::mem::replace(&mut h.server, restarted));
    let _: codex_app_server_protocol::ThreadResumeResponse = h
        .server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: codex_app_server_protocol::ThreadResumeParams {
                thread_id: h.thread_id.clone(),
                ..Default::default()
            },
        })
        .await?;
    if omission {
        let _: codex_app_server_protocol::StatefulRunResumeResponse = h
            .server
            .request(|request_id| ClientRequest::StatefulRunResume {
                request_id,
                params: codex_app_server_protocol::StatefulRunResumeParams {
                    run_id: run.id.clone(),
                    expected_revision: run.revision,
                },
            })
            .await?;
        run.revision += 1;
    }
    responses::mount_sse_once(&h.responses_server, message("final", &ready())).await;
    let turn = h.start_turn().await?;
    let (_, warnings, _) = h.outcome(&turn, /*next*/ false).await?;
    assert_eq!(warnings.len(), 1);
    h.assert_terminal(run, &turn, (&ready(), ANSWER), &items, 2)
        .await?;
    Ok(())
}

macro_rules! witness {
    ($name:ident, $case:expr) => {
        #[tokio::test]
        async fn $name() -> Result<()> {
            $case.await
        }
    };
}

witness!(b2_retained_across_turns, retention(/*retain_first*/ true));
witness!(
    m2_host_copy_does_not_record_progress,
    retention(/*retain_first*/ false)
);
witness!(b3_retained_across_restart, durable(/*omission*/ false));
witness!(
    c1_accepted_omission_across_pause_restart_resume,
    durable(/*omission*/ true)
);

/// A6: explicit model blocking still excludes the host-answer route.
#[tokio::test]
async fn model_blocking_remains_ineligible_for_answer_finalization() -> Result<()> {
    let mut h = harness().await?;
    let run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let release = mount_gated(
        &h.responses_server,
        vec![
            run_update("blocked", "blocked", &[]),
            message("final", ANSWER),
        ],
        /*gate*/ 1,
    )
    .await;
    let turn = h.start_turn().await?;
    h.committed(&run.id, StatefulRunStatus::Blocked).await?;
    let blocked = h.read(&run.id).await?;
    release.send(())?;
    h.outcome(&turn, /*next*/ false).await?;
    assert_eq!(h.read(&run.id).await?, blocked);
    assert_eq!(blocked.host_answer, None);
    assert_eq!(
        h.responses_server
            .received_requests()
            .await
            .unwrap_or_default()
            .len(),
        2
    );
    Ok(())
}

/// W1: a legacy DB answer still reads Answered; evidence-bound Completed has no host answer.
#[tokio::test]
async fn legacy_answer_and_completed_projections_keep_their_meaning() -> Result<()> {
    let mut h = harness().await?;
    let mut run = h.start_run(StatefulWorkflowMode::Autonomous).await?;
    let sqlite = SqliteConfig::new_for_testing(h.codex_home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join("stateful_runtime_1.sqlite"))
        .await?;
    sqlx::query("INSERT INTO stateful_host_answers VALUES (?, ?, 'legacy-turn', ?, ?, 1000)")
        .bind(&run.id)
        .bind(&h.thread_id)
        .bind(ANSWER)
        .bind(ANSWERED_UNVERIFIED_BASIS)
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE stateful_runs SET status = 'completed', result = ? WHERE id = ?")
        .bind(ANSWER)
        .bind(&run.id)
        .execute(&pool)
        .await?;
    run.status = StatefulRunStatus::Answered;
    run.result = Some(ANSWER.into());
    assert_eq!(
        h.read(&run.id).await?,
        StatefulRunReadResponse {
            run: Some(run),
            recovery: Some(codex_app_server_protocol::StatefulRunRecovery {
                lease_expires_at: None,
                previous_turn_id: None,
                last_continuation_claimed_at: None,
            }),
            host_answer: Some(StatefulHostAnswer {
                turn_id: "legacy-turn".into(),
                answer: ANSWER.into(),
                basis: ANSWERED_UNVERIFIED_BASIS.into(),
                committed_at: 1,
                outcome: StatefulHostAnswerOutcome::Answered
            }),
        }
    );
    let run = h
        .start_keyed_run(StatefulWorkflowMode::Autonomous, "verified")
        .await?;
    let store = h.store().await?;
    let id = runtime::StatefulRunId::parse(&run.id)?;
    let commit = super::super::stateful_acceptance_support::seeded_commit(
        &store, &id, "verify", /*validated_obligation_sequence*/ None,
    )
    .await?;
    let (completed, _) = store
        .complete_run_with_acceptance(
            &id,
            runtime::StatefulRunUpdate {
                expected_revision: run.revision,
                status: runtime::StatefulRunStatus::Completed,
                strategy: None,
                result: Some(ANSWER.into()),
            },
            &commit,
            /*final_obligation*/ None,
        )
        .await?;
    let read = h.read(&run.id).await?;
    assert_eq!(
        read,
        StatefulRunReadResponse {
            run: Some(StatefulRun {
                status: StatefulRunStatus::Completed,
                revision: run.revision + 1,
                result: Some(ANSWER.into()),
                updated_at: completed.updated_at_ms / 1000,
                ..run
            }),
            recovery: Some(codex_app_server_protocol::StatefulRunRecovery {
                lease_expires_at: None,
                previous_turn_id: None,
                last_continuation_claimed_at: None,
            }),
            host_answer: None,
        }
    );
    Ok(())
}
