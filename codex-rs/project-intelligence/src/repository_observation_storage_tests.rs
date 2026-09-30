use std::borrow::Cow;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::*;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sqlite(temp_dir: &TempDir) -> SqliteConfig {
    SqliteConfig::new_for_testing(temp_dir.path().abs())
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

/// Clean branch, detached SHA-256 with uncollected dirty content, unborn, and non-Git roots.
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
            not_git_root("C:\\workspace\\notes"),
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

    let store = RepositoryObservationStore::open(&sqlite(&temp_dir))
        .await
        .expect("store should open");
    store.record(observation).await.expect("record");
    assert_eq!(
        store.get("project-1", &id).await.expect("get"),
        Some(expected.clone())
    );
    assert_eq!(store.get("project-2", &id).await.expect("get"), None);
    store.pool.close().await;

    let reopened = RepositoryObservationStore::open(&sqlite(&temp_dir))
        .await
        .expect("store should reopen");
    assert_eq!(
        reopened.get("project-1", &id).await.expect("get"),
        Some(expected)
    );
}

#[tokio::test]
async fn identical_retry_succeeds_and_different_data_conflicts() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = RepositoryObservationStore::open(&sqlite(&temp_dir))
        .await
        .expect("store should open");
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
    let store = RepositoryObservationStore::open(&sqlite(&temp_dir))
        .await
        .expect("store should open");
    let mut conflicting_reasons = multi_root_observation("observation-1", "project-1");
    conflicting_reasons.roots[0].worktree = RepositoryWorktree::Unknown {
        reason: RepositoryUnknownReason::Timeout,
    };
    let mut omitted_but_complete = multi_root_observation("observation-2", "project-1");
    omitted_but_complete.omitted_root_count = 3;

    let errors = [
        store.record(conflicting_reasons).await,
        store.record(omitted_but_complete).await,
    ]
    .map(|result| match result {
        Err(RepositoryObservationStoreError::InvalidObservation(error)) => Some(error),
        Ok(()) | Err(_) => None,
    });
    assert_eq!(
        errors,
        [
            Some(RepositoryObservationError::ConflictingUnknownReasons(
                "C:\\workspace\\notes".to_string()
            )),
            Some(RepositoryObservationError::OmittedRootsWithCompleteCoverage),
        ]
    );
}

#[tokio::test]
async fn recording_does_not_advance_the_intelligence_revision() {
    let temp_dir = TempDir::new().expect("temp dir");
    let hierarchy = HierarchyStore::open(&sqlite(&temp_dir))
        .await
        .expect("hierarchy store should open");
    hierarchy
        .create_node(
            HierarchyNodeId::parse("node-project").expect("valid node ID"),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node should insert");
    let before = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("status");

    let store = RepositoryObservationStore::open(&sqlite(&temp_dir))
        .await
        .expect("store should open");
    store
        .record(multi_root_observation("observation-1", "project-1"))
        .await
        .expect("record");

    let after = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("status");
    assert_eq!((before.revision, after), (1, before));
}

#[tokio::test]
async fn opening_upgrades_a_database_created_by_earlier_migrations() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sqlite = sqlite(&temp_dir);
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
                .filter(|migration| migration.version < 15)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    sqlite
        .run_migrations(&pool, &legacy)
        .await
        .expect("legacy migrations");
    sqlx::query("INSERT INTO project_intelligence_revisions (project_id, revision) VALUES (?, ?)")
        .bind("project-1")
        .bind(7_i64)
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
