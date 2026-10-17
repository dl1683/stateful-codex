use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn valid_legacy_requirement_population_can_exceed_the_byte_budget() -> Result {
    let f = Fixture::new("stateful-runtime").await?;
    let mut run = f.store.get_run(&f.run).await?.ok_or("run")?;
    for index in 0..2 {
        let id = crate::SteeringId::parse(format!("large-{index}"))?;
        let steering = f
            .store
            .submit_steering(
                id.clone(),
                crate::NewSteeringInstruction {
                    project_id: run.value.project_id.clone(),
                    run_id: f.run.clone(),
                    input: "s".repeat(16 * 1024),
                    affected_obligation_ids: Vec::new(),
                },
            )
            .await?;
        (run, _) = f
            .store
            .apply_steering(
                &id,
                crate::SteeringApplication {
                    expected_steering_revision: steering.revision,
                    expected_run_revision: run.revision,
                    strategy: format!("Large strategy {index}"),
                },
            )
            .await?;
    }
    let request = f.store.acceptance_request(&f.run).await?;
    let mut ledger = f.store.acceptance_ledger(&f.run).await?;
    for _ in 0..4 {
        let changes = (0..8)
            .map(|_| AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Deliverable,
                statement: "Preserve the complete scope.".into(),
                request_span: Some(crate::RequestSpan {
                    start: 0,
                    end: request.len(),
                }),
                terms: CriterionTerms {
                    required: true,
                    ..CriterionTerms::default()
                },
            })
            .collect();
        ledger = f
            .store
            .revise_acceptance(
                &f.run,
                ledger.revision,
                changes,
                "large-scope",
                /*project_root*/ None,
            )
            .await?;
    }
    assert_eq!(ledger.criteria.len(), 32);
    assert!(
        ledger
            .criteria
            .iter()
            .map(|item| item.requirement.len())
            .sum::<usize>()
            > 1024 * 1024
    );
    let before = f.durable().await?;
    let mut reader = f.reader().await?;
    assert!(matches!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await,
        Err(ProjectAcceptanceReadError::ReadLimit)
    ));
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_pending_capacity_is_accepted_at_budget_and_refused_beyond_it() -> Result {
    let f = Fixture::new("stateful-runtime").await?;
    f.seed().await?;
    f.mutate("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<4096) INSERT INTO stateful_acceptance_pending SELECT 'acceptance','p'||i,i FROM n").await?;
    let mut reader = f.reader().await?;
    let legacy = f.store.acceptance_ledger(&f.run).await?;
    assert_eq!(legacy.pending_commands, 4096);
    assert_eq!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await?,
        legacy
    );
    // The unchanged legacy writer permits another command. The project reader
    // refuses that valid population rather than truncate the pending count.
    f.store.begin_command(&f.run, "beyond-budget").await?;
    assert_eq!(
        f.store.acceptance_ledger(&f.run).await?.pending_commands,
        4097
    );
    let before = f.durable().await?;
    assert!(matches!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await,
        Err(ProjectAcceptanceReadError::ReadLimit)
    ));
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_reconciliations_remain_complete_at_budget_and_refuse_the_next() -> Result {
    let f = Fixture::new("stateful-runtime").await?;
    let mut run = f.store.get_run(&f.run).await?.ok_or("run")?;
    let mut ledger = f.seed().await?;
    let mut reader = f.reader().await?;
    for index in 0..65 {
        let id = crate::SteeringId::parse(format!("steer-{index}"))?;
        let steering = f
            .store
            .submit_steering(
                id.clone(),
                crate::NewSteeringInstruction {
                    project_id: run.value.project_id.clone(),
                    run_id: f.run.clone(),
                    input: format!("Retain caveat {index}."),
                    affected_obligation_ids: Vec::new(),
                },
            )
            .await?;
        (run, _) = f
            .store
            .apply_steering(
                &id,
                crate::SteeringApplication {
                    expected_steering_revision: steering.revision,
                    expected_run_revision: run.revision,
                    strategy: format!("Strategy {index}"),
                },
            )
            .await?;
    }
    let request = f.store.acceptance_request(&f.run).await?;
    ledger = f
        .store
        .revise_acceptance(
            &f.run,
            ledger.revision,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Deliverable,
                statement: "Preserve all applied scope.".into(),
                request_span: Some(crate::RequestSpan {
                    start: 0,
                    end: request.len(),
                }),
                terms: CriterionTerms {
                    required: true,
                    ..CriterionTerms::default()
                },
            }],
            "scope",
            /*project_root*/ None,
        )
        .await?;
    for index in 0..65 {
        ledger = f
            .store
            .revise_acceptance(
                &f.run,
                ledger.revision,
                vec![AcceptanceChange::ReconcileSteering {
                    steering_id: format!("steer-{index}"),
                    reason: "Retain scope.".into(),
                }],
                "reconcile",
                /*project_root*/ None,
            )
            .await?;
        if index == 63 {
            assert_eq!(ledger.reconciled_steering.len(), 64);
            assert_eq!(
                read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await?,
                ledger
            );
        }
    }
    let before = f.durable().await?;
    assert_eq!(ledger.reconciled_steering.len(), 64); // Legacy truncates; new reads refuse.
    assert!(matches!(
        read_project_acceptance_ledger(reader.begin_snapshot().await?, &f.run).await,
        Err(ProjectAcceptanceReadError::ReadLimit)
    ));
    reader.close().await?;
    assert_eq!(f.durable().await?, before);
    f.store.pool.close().await;
    Ok(())
}
