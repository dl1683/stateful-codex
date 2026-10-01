use std::borrow::Cow;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::*;

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sqlite(temp_dir: &TempDir) -> SqliteConfig {
    SqliteConfig::new_for_testing(temp_dir.path().abs())
}

async fn open_store(temp_dir: &TempDir) -> RepositoryObservationStore {
    RepositoryObservationStore::open(&sqlite(temp_dir))
        .await
        .expect("store should open")
}

fn observation_id(value: &str) -> RepositoryObservationId {
    RepositoryObservationId::parse(value).expect("valid observation ID")
}

fn root(
    project_root: &str,
    head: RepositoryHead,
    worktree: RepositoryWorktree,
) -> RepositoryRootObservation {
    RepositoryRootObservation {
        project_root: project_root.to_string(),
        git_worktree_root: Some(project_root.to_string()),
        head,
        worktree,
    }
}

fn not_git_root(project_root: &str) -> RepositoryRootObservation {
    RepositoryRootObservation {
        git_worktree_root: None,
        ..root(
            project_root,
            RepositoryHead::Unknown {
                reason: RepositoryUnknownReason::NotGit,
            },
            RepositoryWorktree::Unknown {
                reason: RepositoryUnknownReason::NotGit,
            },
        )
    }
}

/// Clean branch, detached SHA-256 with uncollected dirty content, unborn, and a
/// non-Git root whose spelling contains a newline.
fn multi_root_observation(id: &str, project_id: &str) -> RepositoryObservation {
    RepositoryObservation {
        id: observation_id(id),
        project_id: project_id.to_string(),
        started_at_ms: 1_000,
        completed_at_ms: 1_250,
        roots_digest: "sha256:roots".to_string(),
        roots_coverage: RepositoryRootsCoverage::Complete,
        omitted_root_count: 0,
        roots: vec![
            not_git_root("/srv/notes\nwith newline"),
            root(
                "C:\\workspace\\app",
                RepositoryHead::Commit {
                    oid: SHA1.to_string(),
                    head_ref: Some("refs/heads/main".to_string()),
                },
                RepositoryWorktree::Clean,
            ),
            root(
                "C:\\workspace\\lib",
                RepositoryHead::Commit {
                    oid: SHA256.to_string(),
                    head_ref: None,
                },
                RepositoryWorktree::Dirty {
                    coverage: RepositoryDirtyCoverage::Unknown {
                        reason: RepositoryUnknownReason::DirtyFingerprintNotCollected,
                    },
                },
            ),
            root(
                "C:\\workspace\\new",
                RepositoryHead::Unborn {
                    head_ref: Some("refs/heads/main".to_string()),
                },
                RepositoryWorktree::Clean,
            ),
        ],
    }
}

fn sorted(mut observation: RepositoryObservation) -> RepositoryObservation {
    observation
        .roots
        .sort_by(|left, right| left.project_root.cmp(&right.project_root));
    observation
}

#[tokio::test]
async fn multi_root_observation_round_trips_across_reopen() {
    let temp_dir = TempDir::new().expect("temp dir");
    let observation = multi_root_observation("observation-1", "project-1");
    let expected = sorted(observation.clone());
    let id = observation.id.clone();

    let store = open_store(&temp_dir).await;
    store.record(observation).await.expect("record");
    assert_eq!(
        store.get("project-1", &id).await.expect("get"),
        Some(expected.clone())
    );
    assert_eq!(store.get("project-2", &id).await.expect("get"), None);
    store.pool.close().await;

    let reopened = open_store(&temp_dir).await;
    assert_eq!(
        reopened.get("project-1", &id).await.expect("get"),
        Some(expected)
    );
}

#[tokio::test]
async fn identical_retry_succeeds_and_different_data_conflicts() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = open_store(&temp_dir).await;
    let observation = multi_root_observation("observation-1", "project-1");
    let id = observation.id.clone();
    store.record(observation.clone()).await.expect("record");
    store
        .record(observation.clone())
        .await
        .expect("identical retry should succeed");

    let mut changed = observation.clone();
    changed.roots[0].worktree = RepositoryWorktree::Clean;
    changed.roots[0].head = RepositoryHead::Unborn { head_ref: None };
    let error = store.record(changed).await.expect_err("conflict");
    assert!(
        matches!(&error, RepositoryObservationStoreError::IdentityConflict(conflict) if conflict == "observation-1"),
        "{error:?}"
    );
    let foreign = multi_root_observation("observation-1", "project-2");
    let error = store.record(foreign).await.expect_err("conflict");
    assert!(
        matches!(error, RepositoryObservationStoreError::IdentityConflict(_)),
        "{error:?}"
    );
    assert_eq!(
        store.get("project-1", &id).await.expect("get"),
        Some(sorted(observation))
    );
}

#[tokio::test]
async fn invalid_observations_are_rejected_before_storage() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = open_store(&temp_dir).await;
    let mut short_object_id = multi_root_observation("observation-1", "project-1");
    short_object_id.roots[1].head = RepositoryHead::Commit {
        oid: "0123".to_string(),
        head_ref: None,
    };
    let mut omitted_but_complete = multi_root_observation("observation-2", "project-1");
    omitted_but_complete.omitted_root_count = 3;

    let errors = [
        store.record(short_object_id).await,
        store.record(omitted_but_complete).await,
    ]
    .map(|result| match result {
        Err(RepositoryObservationStoreError::InvalidObservation(error)) => Some(error),
        Ok(()) | Err(_) => None,
    });
    assert_eq!(
        errors,
        [
            Some(RepositoryObservationError::InvalidObjectId(
                "0123".to_string()
            )),
            Some(RepositoryObservationError::OmittedRootsWithCompleteCoverage),
        ]
    );
}

/// Opens a database migrated only through versions below `first_missing_version`.
async fn open_legacy_pool(sqlite: &SqliteConfig, first_missing_version: i64) -> SqlitePool {
    tokio::fs::create_dir_all(sqlite.home())
        .await
        .expect("sqlite home");
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
        .await
        .expect("pool");
    let legacy = Migrator {
        migrations: Cow::Owned(
            MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version < first_missing_version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    sqlite
        .run_migrations(&pool, &legacy)
        .await
        .expect("legacy migrations");
    pool
}

#[tokio::test]
async fn upgraded_database_records_without_advancing_the_intelligence_revision() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sqlite = sqlite(&temp_dir);
    let pool = open_legacy_pool(&sqlite, /*first_missing_version*/ 15).await;
    sqlx::query("INSERT INTO project_intelligence_revisions (project_id, revision) VALUES (?, ?)")
        .bind("project-1")
        .bind(/*value*/ 7_i64)
        .execute(&pool)
        .await
        .expect("legacy revision");
    pool.close().await;

    let store = RepositoryObservationStore::open(&sqlite)
        .await
        .expect("store should upgrade the database");
    let observation = multi_root_observation("observation-1", "project-1");
    let id = observation.id.clone();
    store.record(observation.clone()).await.expect("record");

    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
    )
    .bind("project-1")
    .fetch_one(&store.pool)
    .await
    .expect("revision");
    assert_eq!(
        (store.get("project-1", &id).await.expect("get"), revision),
        (Some(sorted(observation)), 7)
    );
}

/// Each root has independent unknown reasons, including the Git failure reasons.
fn mixed_reason_observation(id: &str) -> RepositoryObservation {
    RepositoryObservation {
        roots: vec![
            root(
                "/repo/a",
                RepositoryHead::Unknown {
                    reason: RepositoryUnknownReason::Timeout,
                },
                RepositoryWorktree::Unknown {
                    reason: RepositoryUnknownReason::GitCommandFailed,
                },
            ),
            root(
                "/repo/b",
                RepositoryHead::Unknown {
                    reason: RepositoryUnknownReason::InvalidGitOutput,
                },
                RepositoryWorktree::Dirty {
                    coverage: RepositoryDirtyCoverage::Unknown {
                        reason: RepositoryUnknownReason::DirtyFingerprintNotCollected,
                    },
                },
            ),
            root(
                "/repo/c",
                RepositoryHead::Commit {
                    oid: SHA1.to_string(),
                    head_ref: None,
                },
                RepositoryWorktree::Unknown {
                    reason: RepositoryUnknownReason::OutputLimit,
                },
            ),
        ],
        ..multi_root_observation(id, "project-1")
    }
}

#[tokio::test]
async fn mixed_unknown_reasons_round_trip_across_reopen() {
    let temp_dir = TempDir::new().expect("temp dir");
    let observation = mixed_reason_observation("observation-1");
    let id = observation.id.clone();
    let store = open_store(&temp_dir).await;
    store.record(observation.clone()).await.expect("record");
    store.pool.close().await;

    let reopened = open_store(&temp_dir).await;
    assert_eq!(
        reopened.get("project-1", &id).await.expect("get"),
        Some(sorted(observation))
    );
}

#[tokio::test]
async fn shared_reason_rows_migrate_to_component_reasons() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sqlite = sqlite(&temp_dir);
    let pool = open_legacy_pool(&sqlite, /*first_missing_version*/ 16).await;
    sqlx::query(
        "INSERT INTO repository_observations (
            id, project_id, format_version, started_at_ms, completed_at_ms,
            roots_digest, roots_coverage, omitted_root_count
         ) VALUES ('observation-1', 'project-1', 1, 1000, 1250, 'sha256:roots', 'complete', 0)",
    )
    .execute(&pool)
    .await
    .expect("legacy observation");
    // Clean commit, dirty with uncollected content, non-Git, unknown HEAD with a
    // clean tree, and unborn HEAD with an unknown tree.
    sqlx::query(
        "INSERT INTO repository_root_observations (
            observation_id, project_root, git_worktree_root, head_state, head_oid, head_ref,
            worktree_state, dirty_digest, dirty_coverage, unknown_reason
         ) VALUES
         ('observation-1', '/a', '/a', 'commit', ?, 'refs/heads/main',
          'clean', NULL, 'complete', NULL),
         ('observation-1', '/b', '/b', 'commit', ?, NULL,
          'dirty', NULL, 'unknown', 'dirtyFingerprintNotCollected'),
         ('observation-1', '/c', NULL, 'unknown', NULL, NULL,
          'unknown', NULL, 'unknown', 'notGit'),
         ('observation-1', '/d', '/d', 'unknown', NULL, NULL,
          'clean', NULL, 'complete', 'timeout'),
         ('observation-1', '/e', '/e', 'unborn', NULL, 'refs/heads/main',
          'unknown', NULL, 'unknown', 'unstableSample')",
    )
    .bind(SHA1)
    .bind(SHA256)
    .execute(&pool)
    .await
    .expect("legacy roots");
    pool.close().await;

    let store = RepositoryObservationStore::open(&sqlite)
        .await
        .expect("store should upgrade the database");
    let expected = RepositoryObservation {
        roots: vec![
            root(
                "/a",
                RepositoryHead::Commit {
                    oid: SHA1.to_string(),
                    head_ref: Some("refs/heads/main".to_string()),
                },
                RepositoryWorktree::Clean,
            ),
            root(
                "/b",
                RepositoryHead::Commit {
                    oid: SHA256.to_string(),
                    head_ref: None,
                },
                RepositoryWorktree::Dirty {
                    coverage: RepositoryDirtyCoverage::Unknown {
                        reason: RepositoryUnknownReason::DirtyFingerprintNotCollected,
                    },
                },
            ),
            not_git_root("/c"),
            root(
                "/d",
                RepositoryHead::Unknown {
                    reason: RepositoryUnknownReason::Timeout,
                },
                RepositoryWorktree::Clean,
            ),
            root(
                "/e",
                RepositoryHead::Unborn {
                    head_ref: Some("refs/heads/main".to_string()),
                },
                RepositoryWorktree::Unknown {
                    reason: RepositoryUnknownReason::UnstableSample,
                },
            ),
        ],
        ..multi_root_observation("observation-1", "project-1")
    };
    let id = expected.id.clone();
    assert_eq!(
        store.get("project-1", &id).await.expect("get"),
        Some(expected.clone())
    );

    store
        .record(expected.clone())
        .await
        .expect("identical retry after migration should succeed");
    let mut changed = expected.clone();
    changed.roots[3].head = RepositoryHead::Unknown {
        reason: RepositoryUnknownReason::GitCommandFailed,
    };
    let error = store.record(changed).await.expect_err("conflict");
    assert!(
        matches!(error, RepositoryObservationStoreError::IdentityConflict(_)),
        "{error:?}"
    );
    assert_eq!(
        store.get("project-1", &id).await.expect("get"),
        Some(expected)
    );
}
