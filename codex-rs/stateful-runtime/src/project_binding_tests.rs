use super::*;
use codex_project_store::ProjectWriteError;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn matching_run_start_preserves_the_same_guard_and_prior_domain_write() -> Result {
    for timestamp in [0, 9_007_199_254_740_993_i64] {
        let f = fixture().await?;
        sqlx::query("UPDATE stateful_runs SET created_at_ms=? WHERE id='run'")
            .bind(timestamp)
            .execute(&f.store.pool)
            .await?;
        let mut expected = durable(&f).await?;
        let mut writer =
            ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
        let transaction =
            revalidate_project_run_start(stage(&mut writer).await?, &f.run.id, timestamp).await?;
        assert_eq!(transaction.identity(), &f.identity);
        transaction.commit().await?;
        expected.3 = vec!["changed".to_string()];
        assert_eq!(durable(&f).await?, expected);
        writer.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn incorrect_or_negative_expected_start_rolls_back_and_makes_owner_close_only() -> Result {
    for mismatch in ["negative", "different"] {
        let f = fixture().await?;
        let before = durable(&f).await?;
        let expected = if mismatch == "negative" {
            -1
        } else {
            f.run
                .created_at_ms
                .checked_add(1)
                .ok_or("fixture timestamp overflow")?
        };
        let mut writer =
            ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
        assert!(matches!(
            revalidate_project_run_start(stage(&mut writer).await?, &f.run.id, expected,).await,
            Err(ProjectRunCompletionError::RunStartMismatch)
        ));
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        assert_eq!(durable(&f).await?, before);
    }
    Ok(())
}

#[tokio::test]
async fn missing_and_foreign_runs_refuse_without_publishing_prior_writes() -> Result {
    for case in ["missing", "foreign"] {
        let f = fixture().await?;
        let id = if case == "missing" {
            StatefulRunId::parse("missing")?
        } else {
            sqlx::query("UPDATE stateful_runs SET project_id='foreign' WHERE id=?")
                .bind(f.run.id.as_str())
                .execute(&f.store.pool)
                .await?;
            f.run.id.clone()
        };
        let before = durable(&f).await?;
        let mut writer =
            ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
        let error =
            revalidate_project_run_start(stage(&mut writer).await?, &id, f.run.created_at_ms)
                .await
                .err()
                .ok_or("binding must refuse")?;
        match case {
            "missing" => assert!(matches!(
                error,
                ProjectRunCompletionError::Runtime(StatefulRunStoreError::RunNotFound(_))
            )),
            "foreign" => assert!(matches!(error, ProjectRunCompletionError::ProjectMismatch)),
            _ => unreachable!("closed fixture cases"),
        }
        writer.close().await?;
        assert_eq!(durable(&f).await?, before);
    }
    Ok(())
}

#[tokio::test]
async fn malformed_stored_timestamp_is_not_coerced_to_a_matching_integer() -> Result {
    for (sql, expected_start) in [
        (
            "UPDATE stateful_runs SET created_at_ms='broken' WHERE id='run'",
            0,
        ),
        (
            "UPDATE stateful_runs SET created_at_ms=1.25 WHERE id='run'",
            1,
        ),
    ] {
        let f = fixture().await?;
        let before = durable(&f).await?;
        sqlx::query(sql).execute(&f.store.pool).await?;
        let malformed: (String, String) = sqlx::query_as(
            "SELECT typeof(created_at_ms),quote(created_at_ms) FROM stateful_runs WHERE id='run'",
        )
        .fetch_one(&f.store.pool)
        .await?;
        let mut writer =
            ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
        assert!(matches!(
            revalidate_project_run_start(stage(&mut writer).await?, &f.run.id, expected_start,)
                .await,
            Err(ProjectRunCompletionError::RunStartMismatch)
        ));
        writer.close().await?;
        assert_eq!(sqlx::query_as::<_, (String,String)>("SELECT typeof(created_at_ms),quote(created_at_ms) FROM stateful_runs WHERE id='run'")
            .fetch_one(&f.store.pool).await?, malformed);
        // Only repair the deliberately invalid fixture scalar after checking its
        // exact stored representation; typed reads can then compare all state.
        sqlx::query("UPDATE stateful_runs SET created_at_ms=? WHERE id='run'")
            .bind(f.run.created_at_ms)
            .execute(&f.store.pool)
            .await?;
        assert_eq!(durable(&f).await?, before);
    }
    Ok(())
}

#[tokio::test]
async fn a_matching_run_start_does_not_bypass_runtime_acceptance() -> Result {
    let f = fixture().await?;
    let mut acceptance = crate::acceptance_test_support::settled_commit(
        &f.store, &f.run.id, "owner", /*validated_obligation_sequence*/ None,
    )
    .await;
    acceptance.ledger_revision += 1;
    let before = durable(&f).await?;
    let mut writer = ProjectStoreWriter::open(&f.location, domains().await?, &f.identity).await?;
    let transaction =
        revalidate_project_run_start(stage(&mut writer).await?, &f.run.id, f.run.created_at_ms)
            .await?;
    assert!(matches!(
        commit_project_run(transaction, request(&f, &acceptance)).await,
        Err(ProjectRunCompletionError::Runtime(
            StatefulRunStoreError::AcceptanceChanged
        ))
    ));
    writer.close().await?;
    assert_eq!(durable(&f).await?, before);
    Ok(())
}
