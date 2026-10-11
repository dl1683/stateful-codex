use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::SqlitePool;
use tempfile::TempDir;

use super::*;

pub(super) const PROJECT: &str = "project-1";
pub(super) const THREAD: &str = "00000000-0000-0000-0000-000000000001";

pub(super) struct Fixture {
    _home: TempDir,
    pub(super) sqlite: SqliteConfig,
    pub(super) store: WorkingSetStore,
}

pub(super) async fn fixture() -> Fixture {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = WorkingSetStore::open(&sqlite).await.unwrap();
    Fixture {
        _home: home,
        sqlite,
        store,
    }
}

/// The exclusion row a Forget writes for `text`; any writer of it advances the generation.
pub(super) async fn forget(pool: &SqlitePool, text: &str) {
    sqlx::query("INSERT INTO capture_source_exclusions VALUES (?, ?, 0, ?, 'entry')")
        .bind(PROJECT)
        .bind(digest(text))
        .bind(text.len() as i64)
        .execute(pool)
        .await
        .unwrap();
}

impl Fixture {
    pub(super) async fn generation(&self) -> u64 {
        self.store.eligibility_generation(PROJECT).await.unwrap()
    }

    pub(super) async fn write(
        &self,
        expected_revision: u64,
        events: &[u64],
    ) -> WorkingSetProjection {
        let update = update(expected_revision, self.generation().await, events);
        self.store.write_projection(update).await.unwrap()
    }
}

pub(super) fn key() -> WorkingSetKey {
    WorkingSetKey {
        project_id: PROJECT.to_string(),
        binding_generation: 1,
        thread_id: THREAD.to_string(),
        projection_version: 1,
    }
}

pub(super) fn update(
    expected_revision: u64,
    prepared_generation: u64,
    events: &[u64],
) -> WorkingSetUpdate {
    WorkingSetUpdate {
        key: key(),
        run_id: None,
        expected_revision,
        prepared_generation,
        events: events
            .iter()
            .map(|&sequence| WorkingSetSourceEvent {
                sequence,
                digest: format!("event-{sequence}"),
            })
            .collect(),
        payload: format!("payload through {events:?}"),
    }
}

#[tokio::test]
async fn watermark_advances_only_across_contiguous_events() {
    let fixture = fixture().await;
    let first = fixture.write(/*expected_revision*/ 0, &[1, 2]).await;
    let chained = [1, 2].iter().fold(String::new(), |prefix, sequence| {
        digest(&format!("{prefix}\n{sequence}\nevent-{sequence}"))
    });
    let watermark = SourceWatermark {
        sequence: 2,
        digest: chained,
    };
    assert_eq!(
        (
            first.revision,
            first.watermark.clone(),
            first.run_id.clone()
        ),
        (1, watermark, None)
    );
    for (events, expected, found) in [(&[4][..], 3, 4), (&[2][..], 3, 2), (&[3, 5][..], 4, 5)] {
        let error = fixture
            .store
            .write_projection(update(1, 0, events))
            .await
            .unwrap_err();
        assert!(
            matches!(error, WorkingSetStoreError::WatermarkGap { expected: e, found: f } if (e, f) == (expected, found)),
            "{error}"
        );
    }
    assert!(matches!(
        fixture.store.write_projection(update(0, 0, &[3])).await,
        Err(WorkingSetStoreError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
    // A rejected gap left the committed projection untouched, also after reopening.
    let reopened = WorkingSetStore::open(&fixture.sqlite).await.unwrap();
    assert_eq!(reopened.view(&key()).await.unwrap().projection, Some(first));
    let next = fixture.write(/*expected_revision*/ 1, &[3]).await;
    assert_eq!((next.revision, next.watermark.sequence), (2, 3));

    // Preparation that predates an eligibility change cannot persist derived text.
    forget(&fixture.store.pool, "Use the cedar route.").await;
    assert!(matches!(
        fixture.store.write_projection(update(2, 0, &[4])).await,
        Err(WorkingSetStoreError::EligibilityChanged {
            prepared: 0,
            current: 1
        })
    ));
    // A newer projection version replaces the older derived row.
    let mut newer = update(0, 1, &[1]);
    newer.key.projection_version = 2;
    fixture.store.write_projection(newer).await.unwrap();
    assert_eq!(fixture.store.view(&key()).await.unwrap().projection, None);
}

#[tokio::test]
async fn corrupt_projection_and_file_samples_fail_closed() {
    let fixture = fixture().await;
    fixture.write(/*expected_revision*/ 0, &[1]).await;
    sqlx::query("UPDATE working_set_projections SET payload = 'tampered'")
        .execute(&fixture.store.pool)
        .await
        .unwrap();
    let corrupt = fixture
        .store
        .view(&key())
        .await
        .unwrap()
        .projection
        .unwrap();
    assert_eq!(
        (corrupt.payload.as_str(), corrupt.invalidation),
        ("", Some(WorkingSetInvalidation::Corrupt))
    );

    let sample = |path: usize| WorkingSetFileSample {
        executor_id: "local".to_string(),
        root: "C:/workspace".to_string(),
        relative_path: format!("src/file{path}.rs"),
        sample_event: "turn-1/item-1".to_string(),
        coverage: FileSampleCoverage::Complete,
        content: Some(format!("const VALUE: usize = {path};\n")),
    };
    for path in 0..MAX_WORKING_SET_FILES {
        fixture
            .store
            .record_file_sample(&key(), &sample(path))
            .await
            .unwrap();
    }
    assert!(matches!(
        fixture
            .store
            .record_file_sample(&key(), &sample(MAX_WORKING_SET_FILES))
            .await,
        Err(WorkingSetStoreError::FileLimit)
    ));
    // Re-sampling an already tracked file is allowed at the limit.
    fixture
        .store
        .record_file_sample(&key(), &sample(0))
        .await
        .unwrap();
    sqlx::query("UPDATE working_set_file_samples SET content = 'tampered' WHERE relative_path = 'src/file1.rs'")
        .execute(&fixture.store.pool)
        .await
        .unwrap();
    let files = fixture.store.view(&key()).await.unwrap().files;
    let unreadable = WorkingSetFileSample {
        coverage: FileSampleCoverage::Unreadable,
        content: None,
        ..sample(1)
    };
    assert_eq!(
        (files.len(), &files[0], &files[1]),
        (MAX_WORKING_SET_FILES, &sample(0), &unreadable)
    );
}
