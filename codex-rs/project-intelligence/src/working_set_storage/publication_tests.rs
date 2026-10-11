use codex_state::ThreadProjectAdmission;
use pretty_assertions::assert_eq;

use super::super::DATABASE_NAME;
use super::super::WorkingSetInvalidation;
use super::super::WorkingSetProjection;
use super::super::WorkingSetStore;
use super::super::WorkingSetStoreError;
use super::super::digest;
use super::super::tests::Fixture;
use super::super::tests::PROJECT;
use super::super::tests::THREAD;
use super::super::tests::fixture;
use super::super::tests::forget;
use super::super::tests::key;
use super::super::tests::update;
use super::*;

async fn admission(fixture: &Fixture) -> ThreadProjectAdmission {
    let state = fixture
        .sqlite
        .open_read_write_pool(&fixture.sqlite.state_db_path())
        .await
        .unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS threads(id TEXT PRIMARY KEY, project_id TEXT, project_binding_generation INTEGER NOT NULL DEFAULT 1)").execute(&state).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO threads(id, project_id) VALUES (?, ?)")
        .bind(THREAD)
        .bind(PROJECT)
        .execute(&state)
        .await
        .unwrap();
    state.close().await;
    let thread = serde_json::from_str(&format!("\"{THREAD}\"")).unwrap();
    ThreadProjectAdmission::acquire(&fixture.sqlite, thread, PROJECT)
        .await
        .unwrap()
        .unwrap()
}

async fn admit(
    fixture: &Fixture,
    projection: &WorkingSetProjection,
    texts: &[&str],
) -> PublicationAdmission {
    let admission = admission(fixture).await;
    fixture
        .store
        .admit_publication(&admission, &request(projection, texts))
        .await
        .unwrap()
}

fn request(projection: &WorkingSetProjection, texts: &[&str]) -> PublicationRequest {
    PublicationRequest {
        key: key(),
        window_id: "window-1".to_string(),
        history_generation: 3,
        projection_revision: projection.revision,
        eligibility_generation: projection.eligibility_generation,
        capsule_digest: digest("capsule"),
        source_turn_ids: vec!["turn-1".to_string()],
        source_texts: texts.iter().map(ToString::to_string).collect(),
    }
}

fn admitted(admission: PublicationAdmission) -> PublicationTicket {
    match admission {
        PublicationAdmission::Admitted(ticket) => ticket,
        PublicationAdmission::Refused(refusal) => panic!("refused: {refusal:?}"),
    }
}

#[tokio::test]
async fn retirement_before_admission_refuses_and_after_admission_makes_ticket_stale() {
    let fixture = fixture().await;
    let projection = fixture.write(/*expected_revision*/ 0, &[1]).await;
    // Retirement commits between preparation and admission: refused, never published.
    forget(&fixture.store.pool, "Use the cedar route.").await;
    let current = fixture.generation().await;
    assert_eq!(
        admit(&fixture, &projection, &[]).await,
        PublicationAdmission::Refused(PublicationRefusal::EligibilityChanged { current })
    );
    // The stored projection predates the retirement and must be revalidated first.
    let revalidated = PublicationRequest {
        eligibility_generation: current,
        ..request(&projection, &[])
    };
    let admission = admission(&fixture).await;
    assert_eq!(
        fixture
            .store
            .admit_publication(&admission, &revalidated)
            .await
            .unwrap(),
        PublicationAdmission::Refused(PublicationRefusal::ProjectionChanged)
    );
    drop(admission);
    let projection = fixture.write(/*expected_revision*/ 1, &[2]).await;
    assert_eq!(
        admit(&fixture, &projection, &["Use the cedar route."]).await,
        PublicationAdmission::Refused(PublicationRefusal::SourceIneligible)
    );
    let first = admitted(admit(&fixture, &projection, &["Use the oak route."]).await);
    let ticket = admitted(admit(&fixture, &projection, &[]).await);
    // One pending publication per thread.
    assert_eq!(
        fixture.store.ticket_status(&first).await.unwrap(),
        TicketStatus::Cancelled
    );
    assert_eq!(
        fixture.store.ticket_status(&ticket).await.unwrap(),
        TicketStatus::Admitted
    );

    // Retirement after admission, before persistence is confirmed: stale and cancelled.
    forget(&fixture.store.pool, "Use the birch route.").await;
    let current_generation = fixture.generation().await;
    assert_eq!(
        fixture.store.confirm_installed(&ticket).await.unwrap(),
        TicketStatus::Stale { current_generation }
    );
    assert_eq!(
        fixture.store.ticket_status(&ticket).await.unwrap(),
        TicketStatus::Cancelled
    );

    // Without an intervening retirement the ticket installs; a later retirement makes the
    // installed snapshot stale, so only an invalidation notice may follow.
    let projection = fixture.write(/*expected_revision*/ 2, &[3]).await;
    let ticket = admitted(admit(&fixture, &projection, &[]).await);
    assert_eq!(
        fixture.store.confirm_installed(&ticket).await.unwrap(),
        TicketStatus::Installed
    );
    forget(&fixture.store.pool, "Use the elm route.").await;
    let current_generation = fixture.generation().await;
    assert_eq!(
        fixture.store.ticket_status(&ticket).await.unwrap(),
        TicketStatus::Stale { current_generation }
    );
}

#[tokio::test]
async fn admission_refuses_binding_mismatch_and_missing_or_drifted_projection() {
    let fixture = fixture().await;
    let projection = fixture.write(/*expected_revision*/ 0, &[1]).await;
    let admission = admission(&fixture).await;
    let mut rebound = request(&projection, &[]);
    rebound.key.binding_generation = 2;
    let mut missing = request(&projection, &[]);
    missing.key.projection_version = 9;
    let drifted = PublicationRequest {
        projection_revision: 7,
        ..request(&projection, &[])
    };
    for (request, refusal) in [
        (rebound, PublicationRefusal::BindingChanged),
        (missing, PublicationRefusal::ProjectionMissing),
        (drifted, PublicationRefusal::ProjectionChanged),
    ] {
        assert_eq!(
            fixture
                .store
                .admit_publication(&admission, &request)
                .await
                .unwrap(),
            PublicationAdmission::Refused(refusal)
        );
    }
}

#[tokio::test]
async fn concurrent_retirement_and_admission_serialize_through_separate_connections() {
    let fixture = fixture().await;
    // Independent pools stand in for another process: only SQLite locking orders them.
    let other = WorkingSetStore::open(&fixture.sqlite).await.unwrap();
    let path = fixture.sqlite.home().join(DATABASE_NAME);
    let writer = fixture.sqlite.open_read_write_pool(&path).await.unwrap();
    for round in 0..8_u64 {
        let projection = fixture.write(round, &[round + 1]).await;
        let admission = admission(&fixture).await;
        let publication = request(&projection, &[]);
        let text = format!("Race rule {round}.");
        let (admitted, ()) = tokio::join!(
            other.admit_publication(&admission, &publication),
            forget(&writer, &text),
        );
        drop(admission);
        let current_generation = fixture.generation().await;
        match admitted.unwrap() {
            PublicationAdmission::Refused(refusal) => assert_eq!(
                refusal,
                PublicationRefusal::EligibilityChanged {
                    current: current_generation
                }
            ),
            PublicationAdmission::Admitted(ticket) => assert_eq!(
                fixture.store.confirm_installed(&ticket).await.unwrap(),
                TicketStatus::Stale { current_generation }
            ),
        }
    }
}

#[tokio::test]
async fn invalidation_cancels_tickets_and_requires_genesis_rebuild() {
    let fixture = fixture().await;
    let projection = fixture.write(/*expected_revision*/ 0, &[1, 2]).await;
    let ticket = admitted(admit(&fixture, &projection, &[]).await);
    for _ in 0..40 {
        fixture
            .store
            .invalidate(&key(), WorkingSetInvalidation::Binding)
            .await
            .unwrap();
    }
    assert_eq!(
        fixture.store.ticket_status(&ticket).await.unwrap(),
        TicketStatus::Cancelled
    );
    let view = fixture.store.view(&key()).await.unwrap();
    let invalidation = view
        .projection
        .and_then(|projection| projection.invalidation);
    assert_eq!(invalidation, Some(WorkingSetInvalidation::Binding));
    let records: (i64, i64, String) = sqlx::query_as(
        "SELECT COUNT(*), MIN(sequence), MAX(reason) FROM working_set_invalidations",
    )
    .fetch_one(&fixture.store.pool)
    .await
    .unwrap();
    assert_eq!(records, (32, 9, "binding".to_string()));
    assert!(matches!(
        fixture
            .store
            .write_projection(update(1, view.eligibility_generation, &[3]))
            .await,
        Err(WorkingSetStoreError::Invalidated(
            WorkingSetInvalidation::Binding
        ))
    ));
    let rebuilt = fixture.write(/*expected_revision*/ 0, &[1]).await;
    assert_eq!(
        (
            rebuilt.revision,
            rebuilt.watermark.sequence,
            rebuilt.invalidation
        ),
        (2, 1, None)
    );
}

#[tokio::test]
async fn corrupt_projection_refuses_admission() {
    let fixture = fixture().await;
    let projection = fixture.write(/*expected_revision*/ 0, &[1]).await;
    sqlx::query("UPDATE working_set_projections SET payload = 'tampered'")
        .execute(&fixture.store.pool)
        .await
        .unwrap();
    assert_eq!(
        admit(&fixture, &projection, &[]).await,
        PublicationAdmission::Refused(PublicationRefusal::Invalidated(
            WorkingSetInvalidation::Corrupt
        ))
    );
}
