use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::entry_update;
use super::fixture;
use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::blackboard_storage::effective_hit::attributed::AttributedContent;
use crate::blackboard_storage::effective_hit::attributed::AttributedReadError;
use crate::blackboard_storage::effective_hit::attributed::read_on;

fn projection(entry: &BlackboardEntry) -> AttributedContent {
    AttributedContent {
        id: entry.id.clone(),
        node_id: entry.value.node_id.clone(),
        revision: entry.revision,
        created_at_ms: entry.created_at_ms,
        updated_at_ms: entry.updated_at_ms,
        kind: entry.value.kind,
        state: entry.state,
        content: entry.value.content.clone(),
        provenance: entry.value.provenance.clone(),
    }
}

#[tokio::test]
async fn scalar_parity_and_snapshot_retirement_do_not_change_native_reads() {
    let home = TempDir::new().unwrap();
    let (_, store, entry, _) = fixture(&home).await;
    let expected = Some(projection(
        &store
            .get_hit("project-1", &entry.id)
            .await
            .unwrap()
            .unwrap()
            .entry,
    ));
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        expected
    );
    assert_eq!(read_on(&mut tx, "other", &entry.id).await.unwrap(), None);
    assert_eq!(
        read_on(
            &mut tx,
            "project-1",
            &BlackboardEntryId::parse("missing").unwrap()
        )
        .await
        .unwrap(),
        None
    );
    let mut update = entry_update(entry.value.clone(), entry.revision);
    update.state = BlackboardEntryState::Tombstoned;
    store
        .update_entry("project-1", &entry.id, update)
        .await
        .unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        expected
    );
    tx.commit().await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        None
    );
    tx.commit().await.unwrap();
    assert_eq!(store.get_hit("project-1", &entry.id).await.unwrap(), None);
}

#[tokio::test]
async fn coverage_and_structure_refuse_instead_of_hiding_corruption_as_a_miss() {
    let home = TempDir::new().unwrap();
    let (_, store, entry, _) = fixture(&home).await;
    sqlx::query(
        "INSERT INTO capture_identity_coverage(project_id,watermark) VALUES ('project-1',0)",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    assert!(matches!(
        read_on(&mut tx, "project-1", &entry.id).await,
        Err(AttributedReadError::CoverageIncomplete)
    ));
    tx.commit().await.unwrap();
    assert_eq!(store.get_hit("project-1", &entry.id).await.unwrap(), None);
    sqlx::query("DELETE FROM capture_identity_coverage")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE blackboard_entries SET revision=9007199254740993 WHERE id=?")
        .bind(entry.id.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    assert!(matches!(
        read_on(&mut tx, "project-1", &entry.id).await,
        Err(AttributedReadError::InvalidEntry)
    ));
    tx.commit().await.unwrap();
    sqlx::query("UPDATE blackboard_entries SET revision=1.5 WHERE id=?")
        .bind(entry.id.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    assert!(matches!(
        read_on(&mut tx, "project-1", &entry.id).await,
        Err(AttributedReadError::InvalidClock)
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn text_guards_enforce_utf8_byte_boundaries_without_loading_omitted_payloads() {
    let home = TempDir::new().unwrap();
    let (_, store, entry, _) = fixture(&home).await;
    let content = "\u{092e}".repeat(/*n*/ 682) + "ab";
    sqlx::query("UPDATE blackboard_entry_revisions SET content=?,structured_value=zeroblob(1000000) WHERE entry_id=?").bind(&content).bind(entry.id.as_str()).execute(&store.pool).await.unwrap();
    let mut expected = projection(&entry);
    expected.content = content;
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        Some(expected)
    );
    tx.commit().await.unwrap();
    for sql in [
        "UPDATE blackboard_entry_revisions SET content=printf('%02049d',0)",
        "UPDATE blackboard_entry_revisions SET content=x'61'",
        "UPDATE blackboard_entry_revisions SET content=CAST(x'ff' AS TEXT)",
        "UPDATE blackboard_entry_revisions SET content=''",
        "UPDATE blackboard_entry_revisions SET kind=printf('%033d',0)",
        "UPDATE blackboard_entry_revisions SET kind='unknown'",
        "UPDATE blackboard_entry_revisions SET provenance_source_id=''",
        "UPDATE blackboard_entry_revisions SET provenance_kind='unknown'",
        "UPDATE blackboard_entry_revisions SET provenance_source_id=zeroblob(513)",
        "UPDATE blackboard_entry_revisions SET provenance_source_id=printf('%0513d',0)",
        "UPDATE blackboard_entries SET node_id=printf('%0513d',0)",
    ] {
        let mut tx = store.pool.begin().await.unwrap();
        sqlx::query("PRAGMA defer_foreign_keys=ON")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("PRAGMA ignore_check_constraints=ON")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            matches!(
                read_on(&mut tx, "project-1", &entry.id).await,
                Err(AttributedReadError::UnsupportedContent)
            ),
            "{sql}"
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn proposals_superseded_user_words_and_source_exclusions_share_native_policy() {
    let home = TempDir::new().unwrap();
    let (_, store, entry, _) = fixture(&home).await;
    let expected = Some(projection(&entry));
    let mut tx = store.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO capture_groups VALUES ('project-1','proposal',NULL,NULL,'proposal-v1',1,1,0,0,1,0,0,0)").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO capture_group_members VALUES ('project-1','proposal',0,?,1,'pending','bounded',NULL)").bind(entry.id.as_str()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO knowledge_context(entry_id,revision,project_id,category,authority,validity,payload) VALUES (?,1,'project-1','note','assistant_reported','current',?)")
        .bind(entry.id.as_str()).bind(r#"{"proposal":{}}"#).execute(&mut *tx).await.unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        None
    );
    sqlx::query("UPDATE knowledge_context SET authority='human_direct',payload=?")
        .bind(r#"{"promotion":{}}"#)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        expected
    );
    tx.rollback().await.unwrap();

    store
        .create_entry(
            BlackboardEntryId::parse("successor").unwrap(),
            entry.value.clone(),
        )
        .await
        .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    sqlx::query("UPDATE blackboard_entry_revisions SET provenance_kind='user',state='superseded',superseded_by='successor' WHERE entry_id=?").bind(entry.id.as_str()).execute(&mut *tx).await.unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        None
    );
    tx.rollback().await.unwrap();

    let mut tx = store.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO capture_sources VALUES ('source','project-1','event','thread','turn',1,1,0,'digest','{}',1,0)").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO capture_entry_sources VALUES (?,'source',0,10,'quote')")
        .bind(entry.id.as_str())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        expected
    );
    sqlx::query("INSERT INTO capture_source_exclusions VALUES ('project-1','digest',0,10,?)")
        .bind(entry.id.as_str())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        read_on(&mut tx, "project-1", &entry.id).await.unwrap(),
        None
    );
    tx.rollback().await.unwrap();
}
