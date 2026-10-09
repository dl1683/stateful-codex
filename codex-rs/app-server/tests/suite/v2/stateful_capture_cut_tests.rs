//! Delivered-tool witnesses for the historical completion capability cut.
use super::*;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use pretty_assertions::assert_eq;

async fn read_run(
    server: &mut TestAppServer,
    run_id: &str,
) -> Result<codex_app_server_protocol::StatefulRun> {
    let response: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id.to_string()),
                thread_id: None,
            },
        })
        .await?;
    Ok(response.run.expect("run remains readable"))
}

#[tokio::test]
async fn c2cut_historical_completion_refuses_without_mutation_and_current_root_completes()
-> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.clone(),
                thread_id: thread.clone(),
                goal: "Complete with eligible project knowledge.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "cut-run".to_string(),
            },
        })
        .await?;
    // A project root holding only the files the completion's acceptance check pins.
    let acceptance_root = home.path().join("project");
    std::fs::create_dir(&acceptance_root)?;
    crate::suite::v2::stateful_acceptance_support::write_acceptance_files(&acceptance_root)?;
    let _: codex_app_server_protocol::ProjectUpdateResponse = server
        .request(|request_id| ClientRequest::ProjectUpdate {
            request_id,
            params: codex_app_server_protocol::ProjectUpdateParams {
                project_id: project.clone(),
                name: None,
                roots: Some(vec![codex_app_server_protocol::ProjectRoot {
                    path: codex_utils_absolute_path::AbsolutePathBuf::try_from(
                        acceptance_root.clone(),
                    )
                    .expect("temporary project root is absolute"),
                }]),
                metadata: None,
            },
        })
        .await?;
    crate::suite::v2::stateful_acceptance_support::seed_admitted_plan(
        home.path(),
        &started.run.id,
        &[],
    )
    .await?;
    let recorded = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records": [record("forgotten", "QX704")]}),
    )
    .await?;
    assert_eq!(recorded["recorded"], json!(1));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let original = store
        .root_projection(pi::RootBlackboardQuery {
            project_id: project.clone(),
            max_entries: 256,
        })
        .await?
        .data
        .remove(0)
        .entry;
    forget(
        &mut server,
        &project,
        &thread,
        original.id.as_str(),
        original.revision,
    )
    .await?;

    for attempt in 0..2 {
        let archived = store.get_entry(&project, &original.id).await?.unwrap();
        assert_eq!(archived.state, pi::BlackboardEntryState::Tombstoned);
        assert_eq!(archived.value.content, "QX704");
        let before = read_run(&mut server, &started.run.id).await?;
        let obligations_before: ObligationListResponse = server
            .request(|request_id| ClientRequest::ObligationList {
                request_id,
                params: ObligationListParams {
                    run_id: started.run.id.clone(),
                    cursor: None,
                    limit: Some(10),
                },
            })
            .await?;
        let root = store
            .root_projection(pi::RootBlackboardQuery {
                project_id: project.clone(),
                max_entries: 256,
            })
            .await?;
        let rejected = model_output(
            &mut server,
            &responses_server,
            &thread,
            "stateful_run_update",
            json!({
                "expectedRevision": before.revision, "status": "completed", "openIssues": [],
                "completionDisposition": "durableLearning", "result": "Ready.",
                "rootRevision": root.revision, "materialRootFindings": [],
                "materialHistoricalFindings": [{"entryId": archived.id, "revision": archived.revision}],
                "completionIdempotencyKey": format!("cut-final-{attempt}"),
                "finalObligation": {"learning": ["A reusable conclusion."]}
            }),
        ).await?;
        assert!(
            rejected.contains("materialHistoricalFindings is unsupported"),
            "{rejected}"
        );
        assert!(!rejected.contains("QX704"));
        assert_eq!(read_run(&mut server, &started.run.id).await?, before);
        let obligations_after: ObligationListResponse = server
            .request(|request_id| ClientRequest::ObligationList {
                request_id,
                params: ObligationListParams {
                    run_id: started.run.id.clone(),
                    cursor: None,
                    limit: Some(10),
                },
            })
            .await?;
        assert_eq!(obligations_after, obligations_before);
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    let recorded = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records": [record("current", "Keep the Cedar kit.")]}),
    )
    .await?;
    assert_eq!(recorded["recorded"], json!(1));
    let current = read_run(&mut server, &started.run.id).await?;
    let root = store
        .root_projection(pi::RootBlackboardQuery {
            project_id: project,
            max_entries: 256,
        })
        .await?;
    assert_eq!(root.data.len(), 1);
    model_output(
        &mut server,
        &responses_server,
        &thread,
        "exec_command",
        json!({
            "cmd": crate::suite::v2::stateful_acceptance_support::CHECK_COMMAND,
            "workdir": acceptance_root.to_string_lossy(),
            "yield_time_ms": 10_000
        }),
    )
    .await?;
    let completed = model_call(
        &mut server,
        &responses_server,
        &thread,
        "stateful_run_update",
        json!({
            "expectedRevision": current.revision, "status": "completed", "openIssues": [],
            "completionDisposition": "durableLearning", "result": "Ready.",
            "rootRevision": root.revision, "materialRootFindings": ["E1"],
            "completionIdempotencyKey": "cut-current-final",
            "finalObligation": {"learning": ["Keep the Cedar kit."]}
        }),
    )
    .await?;
    assert_eq!(completed["status"], "completed");
    assert!(
        completed["finalAnswerChecklist"]
            .to_string()
            .contains("rootFinding")
    );
    assert!(!completed.to_string().contains("QX704"));
    let durable = read_run(&mut server, &started.run.id).await?;
    assert_eq!(durable.status, StatefulRunStatus::Completed);
    let result = durable.result.unwrap();
    assert!(result.contains("Keep the Cedar kit."));
    assert!(!result.contains("QX704"));
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}
