use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::entry_update;
use super::fixture;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEvidenceFreshness;
use crate::BlackboardHit;
use crate::blackboard_storage::effective_hit::read_on;

#[tokio::test]
async fn exact_read_retains_snapshot_across_retirement_and_refreshes_after_finish() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (_, store, entry, _) = fixture(&temp_dir).await;
    let expected = Some(BlackboardHit::new(
        entry.clone(),
        BlackboardEvidenceFreshness::Current,
    ));
    assert_eq!(store.get_hit("project-1", &entry.id).await.unwrap(), expected);
    let mut reader = store.pool.begin().await.unwrap();
    assert_eq!(read_on(&mut reader, "project-1", &entry.id).await.unwrap(), expected);

    let mut update = entry_update(entry.value.clone(), entry.revision);
    update.state = BlackboardEntryState::Tombstoned;
    store.update_entry("project-1", &entry.id, update).await.unwrap();

    assert_eq!(read_on(&mut reader, "project-1", &entry.id).await.unwrap(), expected);
    reader.commit().await.unwrap();
    let mut reader = store.pool.begin().await.unwrap();
    assert_eq!(read_on(&mut reader, "project-1", &entry.id).await.unwrap(), None);
    reader.commit().await.unwrap();
    assert_eq!(store.get_hit("project-1", &entry.id).await.unwrap(), None);
    assert_eq!(store.get_entry("project-1", &entry.id).await.unwrap().unwrap().state,
        BlackboardEntryState::Tombstoned);
}

#[tokio::test]
async fn exact_read_keeps_coverage_and_project_refusals_as_misses() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (_, store, entry, _) = fixture(&temp_dir).await;
    let mut reader = store.pool.begin().await.unwrap();
    assert_eq!(read_on(&mut reader, "other-project", &entry.id).await.unwrap(), None);
    assert_eq!(read_on(&mut reader, "project-1", &BlackboardEntryId::parse("missing").unwrap()).await.unwrap(), None);
    reader.commit().await.unwrap();

    sqlx::query("INSERT INTO capture_identity_coverage (project_id, watermark, blocked_reason) VALUES (?, 0, ?)")
        .bind("project-1").bind("fixture incomplete coverage").execute(&store.pool).await.unwrap();
    let mut reader = store.pool.begin().await.unwrap();
    assert_eq!(read_on(&mut reader, "project-1", &entry.id).await.unwrap(), None);
    reader.commit().await.unwrap();
    assert_eq!(store.get_hit("project-1", &entry.id).await.unwrap(), None);
    assert_eq!(store.get_entry("project-1", &entry.id).await.unwrap(), Some(entry));
}
