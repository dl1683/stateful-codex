use super::*;
use crate::DomainUpgradeState;
use crate::PreparedDomainUpgrade;
use pretty_assertions::assert_eq;
use std::time::Duration;

const INDEX: &str = "CREATE INDEX alpha_value ON alpha(value);";

fn alpha(suffix: &[&'static str]) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            std::iter::once("CREATE TABLE alpha(id INTEGER PRIMARY KEY, value TEXT NOT NULL);")
                .chain(suffix.iter().copied())
                .enumerate()
                .map(|(i, sql)| {
                    Migration::new(
                        i as i64 + 1,
                        "fixture".into(),
                        MigrationType::Simple,
                        sqlx::SqlStr::from_static(sql),
                        /*no_tx*/ false,
                    )
                })
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn successor(suffix: &[&'static str]) -> TestResult<PreparedDomains> {
    let bravo = Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            /*version*/ 1,
            "fixture".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static(
                "CREATE TABLE bravo(id INTEGER PRIMARY KEY REFERENCES alpha(id), value TEXT NOT NULL);",
            ),
            /*no_tx*/ false,
        )]),
        ..Migrator::DEFAULT
    };
    Ok(PreparedDomains::prepare(&[
        DomainManifest {
            domain: "alpha",
            migrations: &alpha(suffix),
        },
        DomainManifest {
            domain: "bravo",
            migrations: &bravo,
        },
    ])
    .await?)
}

async fn seed(writer: &mut ProjectStoreWriter) -> TestResult {
    let mut transaction = writer.begin_write().await?;
    sqlx::raw_sql("INSERT INTO alpha VALUES(1, '₹ Marathi'), (2, '₹ Marathi'); INSERT INTO bravo VALUES(1, 'first'), (2, 'second');").execute(transaction.connection()).await?;
    transaction.commit().await?;
    Ok(())
}

#[tokio::test]
async fn upgrade_preserves_rows_and_identity_and_switches_the_owned_manifest() -> TestResult {
    let (_directory, location, identity) = fixture().await?;
    let plan = PreparedDomainUpgrade::new(domains().await?, successor(&[INDEX]).await?)?;
    assert_eq!(
        plan.reconcile(&location, &identity).await?,
        DomainUpgradeState::Predecessor
    );
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    seed(&mut writer).await?;
    let before = read_values(&location).await?;
    let mut old_reader = ProjectStoreReader::open(&location, domains().await?)
        .await?
        .into_domain_reader(&DomainManifest {
            domain: "alpha",
            migrations: &alpha(&[]),
        })
        .await?;
    let mut connection = ProjectStoreReader::open(&location, domains().await?)
        .await?
        .connection;
    let mut held = connection.begin().await?;
    let old_schema: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema ORDER BY name")
            .fetch_all(&mut *held)
            .await?;
    writer.upgrade_domains(&plan).await?;
    writer.begin_write().await?.rollback().await?;
    writer.close().await?;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_schema ORDER BY name")
            .fetch_all(&mut *held)
            .await?,
        old_schema
    );
    held.commit().await?;
    connection.close().await?;
    assert!(old_reader.begin_snapshot().await.is_err());
    old_reader.close().await?;
    assert!(
        ProjectStoreReader::open(&location, domains().await?)
            .await
            .is_err()
    );
    let mut connection = ProjectStoreReader::open(&location, successor(&[INDEX]).await?)
        .await?
        .connection;
    let after: Vec<(i64, String, String)> = sqlx::query_as("SELECT alpha.id, alpha.value, bravo.value FROM alpha JOIN bravo USING(id) ORDER BY alpha.id").fetch_all(&mut connection).await?;
    connection.close().await?;
    assert_eq!(
        (plan.reconcile(&location, &identity).await?, after),
        (DomainUpgradeState::Successor, before)
    );
    Ok(())
}

#[tokio::test]
async fn failed_suffix_and_mismatched_plan_require_awaited_close() -> TestResult {
    for plan in [
        PreparedDomainUpgrade::new(
            domains().await?,
            successor(&["CREATE UNIQUE INDEX alpha_value ON alpha(value);"]).await?,
        )?,
        PreparedDomainUpgrade::new(
            successor(&[INDEX]).await?,
            successor(&[INDEX, "CREATE INDEX alpha_id ON alpha(id);"]).await?,
        )?,
    ] {
        let (_directory, location, identity) = fixture().await?;
        let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
        seed(&mut writer).await?;
        let before = read_values(&location).await?;
        assert!(writer.upgrade_domains(&plan).await.is_err());
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        assert_eq!(read_values(&location).await?, before);
    }
    Ok(())
}

#[tokio::test]
async fn invalid_plans_wrong_identity_and_unknown_cohort_refuse() -> TestResult {
    assert!(PreparedDomainUpgrade::new(domains().await?, domains().await?).is_err());
    assert!(PreparedDomainUpgrade::new(successor(&[INDEX]).await?, domains().await?).is_err());
    assert!(
        PreparedDomainUpgrade::new(
            successor(&[INDEX]).await?,
            successor(&["CREATE UNIQUE INDEX alpha_value ON alpha(value);"]).await?
        )
        .is_err()
    );
    let (_directory, location, mut identity) = fixture().await?;
    let plan = PreparedDomainUpgrade::new(domains().await?, successor(&[INDEX]).await?)?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    let actual = identity.clone();
    identity.store_uuid = uuid::Uuid::new_v4();
    assert!(matches!(
        plan.reconcile(&location, &identity).await,
        Err(ProjectWriteError::IdentityMismatch)
    ));
    sqlx::query("CREATE TABLE unknown(value TEXT)")
        .execute(&mut writer.connection)
        .await?;
    writer.close().await?;
    assert!(plan.reconcile(&location, &actual).await.is_err());
    Ok(())
}

#[tokio::test]
async fn cancellation_after_commit_entry_reconciles_durable_successor() -> TestResult {
    let (_directory, location, identity) = fixture().await?;
    let plan = PreparedDomainUpgrade::new(domains().await?, successor(&[INDEX]).await?)?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    seed(&mut writer).await?;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut entered_tx = Some(entered_tx);
    writer
        .connection
        .lock_handle()
        .await?
        .set_commit_hook(move || {
            if let Some(sender) = entered_tx.take() {
                let _ = sender.send(());
            }
            release_rx
                .recv_timeout(Duration::from_secs(/*secs*/ 10))
                .is_ok()
        });
    let mut upgrade = Box::pin(writer.upgrade_domains(&plan));
    tokio::select! {
        result = &mut upgrade => panic!("upgrade completed before release: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(/*secs*/ 5), entered_rx) => { result??; }
    }
    drop(upgrade);
    assert!(matches!(
        writer.begin_write().await,
        Err(ProjectWriteError::UnfinishedOperation)
    ));
    release_tx.send(())?;
    writer.close().await?;
    assert_eq!(
        plan.reconcile(&location, &identity).await?,
        DomainUpgradeState::Successor
    );
    Ok(())
}
