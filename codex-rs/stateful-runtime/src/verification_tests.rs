use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;

#[tokio::test]
#[expect(
    clippy::disallowed_methods,
    reason = "closed-pool validation regression must not open or migrate a database"
)]
async fn overflow_is_reported_before_closed_pool_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new().connect_lazy("sqlite::memory:")?;
    pool.close().await;
    let store = StatefulRunStore { pool };
    let run = StatefulRunId::parse("run")?;
    assert!(matches!(
        store.end_verification(&run, "owner", u64::MAX).await,
        Err(StatefulRunStoreError::CountOverflow)
    ));
    Ok(())
}
