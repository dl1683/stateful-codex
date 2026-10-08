use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::tests::legacy_rows;
use crate::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const PROJECT: &str = "project-1";

#[tokio::test]
async fn c2r2_structured_fields_cover_existing_copies_update_succession_and_limits() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let mut value = rule("Purchase receipt identifier.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    value.structured_value = Some(BlackboardStructuredValue {
        value: "QX704".to_string(),
        unit: None,
    });
    let old = store
        .create_entry(
            BlackboardEntryId::parse("a-old-copy").unwrap(),
            value.clone(),
        )
        .await
        .unwrap();
    value.structured_value = Some(BlackboardStructuredValue {
        value: "17".to_string(),
        unit: Some("QX704".to_string()),
    });
    let unit = store
        .create_entry(
            BlackboardEntryId::parse("a-unit-copy").unwrap(),
            value.clone(),
        )
        .await
        .unwrap();
    value.structured_value = Some(BlackboardStructuredValue {
        value: "18".to_string(),
        unit: Some("kits".to_string()),
    });
    let independent = store
        .create_entry(
            BlackboardEntryId::parse("z-control").unwrap(),
            value.clone(),
        )
        .await
        .unwrap();
    let direct = store
        .create_entry(BlackboardEntryId::parse("direct").unwrap(), rule("QX704"))
        .await
        .unwrap();
    store
        .update_entry(PROJECT, &direct.id, retire(&direct))
        .await
        .unwrap();
    let original = legacy_rows(&store).await;
    let before = snapshot(&store).await;
    for entry in [&old, &unit] {
        assert_eq!(
            store
                .get_source_eligible_entry(PROJECT, &entry.id)
                .await
                .unwrap(),
            None
        );
        assert!(store.get_hit(PROJECT, &entry.id).await.unwrap().is_none());
        assert_eq!(
            store.get_entry(PROJECT, &entry.id).await.unwrap(),
            Some(entry.clone())
        );
    }
    assert_eq!(
        store
            .root_projection(RootBlackboardQuery {
                project_id: PROJECT.to_string(),
                max_entries: 1
            })
            .await
            .unwrap()
            .data[0]
            .entry,
        independent
    );
    for field in ["value", "unit"] {
        let mut blocked = value.clone();
        let structured = blocked.structured_value.as_mut().unwrap();
        if field == "value" {
            structured.value = "QX704".to_string();
        } else {
            structured.unit = Some("QX704".to_string());
        }
        assert!(matches!(
            store
                .create_entry(
                    BlackboardEntryId::parse(format!("new-{field}")).unwrap(),
                    blocked.clone()
                )
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
        let mut update = retire(&independent);
        update.state = BlackboardEntryState::Active;
        update.structured_value = blocked.structured_value.clone();
        assert!(matches!(
            store
                .update_entry_from_model(PROJECT, &independent.id, update)
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
        assert!(matches!(
            store
                .create_successor_from_model(
                    BlackboardEntryId::parse(format!("successor-{field}")).unwrap(),
                    blocked,
                    vec![SupersededEntry {
                        id: independent.id.clone(),
                        expected_revision: independent.revision
                    }]
                )
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
    }
    assert_eq!(snapshot(&store).await, before);
    assert_eq!(legacy_rows(&store).await, original);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(reopened.get_hit(PROJECT, &old.id).await.unwrap(), None);
    assert_eq!(reopened.get_hit(PROJECT, &unit.id).await.unwrap(), None);
    assert_eq!(
        reopened
            .get_source_eligible_entry(PROJECT, &independent.id)
            .await
            .unwrap(),
        Some(independent)
    );
    assert_eq!(legacy_rows(&reopened).await, original);
}

#[tokio::test]
async fn c2r2_legacy_word_cycle_current_binding_survives_paged_upgrade_and_forget() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    // Faithful old-store fixture: pre-0021 writes permitted A -> B -> A. Seed the
    // original immutable revisions and erase only rebuildable alias projections.
    let mut value = rule("Old meridian fact.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    let old = store
        .create_entry(
            BlackboardEntryId::parse("legacy-cycle").unwrap(),
            value.clone(),
        )
        .await
        .unwrap();
    let mut revise = retire(&old);
    revise.state = BlackboardEntryState::Active;
    revise.content = "Current cedar fact.".to_string();
    let changed = store
        .update_entry_from_model(PROJECT, &old.id, revise)
        .await
        .unwrap();
    // 63 interleaved old metadata revisions force the final A onto the next page.
    let mut control_value = value.clone();
    control_value.content = "Prior aspen wording.".to_string();
    let mut control = store
        .create_entry(BlackboardEntryId::parse("control").unwrap(), control_value)
        .await
        .unwrap();
    let mut current = retire(&control);
    current.state = BlackboardEntryState::Active;
    current.content = "Independent current cedar wording.".to_string();
    control = store
        .update_entry_from_model(PROJECT, &control.id, current)
        .await
        .unwrap();
    for _ in 0..62 {
        let mut update = retire(&control);
        update.state = BlackboardEntryState::Active;
        control = store
            .update_entry_from_model(PROJECT, &control.id, update)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE blackboard_entries SET revision = 3 WHERE id = ?")
        .bind(old.id.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blackboard_entry_revisions SELECT entry_id, 3, kind, 'Old meridian fact.', structured_value, structured_unit, confidence_basis_points, verification, importance, root_promotion, state, superseded_by, provenance_kind, provenance_source_id, recorded_at_ms, agent_run_id FROM blackboard_entry_revisions WHERE entry_id = ? AND revision = 2").bind(old.id.as_str()).execute(&store.pool).await.unwrap();
    let cycle = store.get_entry(PROJECT, &old.id).await.unwrap().unwrap();
    let relation = store
        .create_relation(
            BlackboardRelationId::parse("legacy-endpoint").unwrap(),
            NewBlackboardRelation {
                project_id: PROJECT.to_string(),
                from_entry_id: control.id.clone(),
                to_entry_id: cycle.id.clone(),
                kind: BlackboardRelationKind::Supports,
                note: Some("Independent relation note.".to_string()),
                confidence: ConfidenceScore::from_basis_points(/*value*/ 9000).unwrap(),
                provenance: control.value.provenance.clone(),
            },
        )
        .await
        .unwrap();
    let original = legacy_rows(&store).await;
    sqlx::query("DELETE FROM capture_identity_aliases")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM capture_current_words")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id, watermark) SELECT 'project-1', MAX(rowid) FROM blackboard_entry_revisions WHERE 1 ON CONFLICT(project_id) DO UPDATE SET after_rowid = 0, watermark = excluded.watermark").execute(&store.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER stop_cycle_page BEFORE UPDATE ON capture_identity_coverage WHEN OLD.after_rowid >= 64 BEGIN SELECT RAISE(ABORT, 'cold continuation'); END").execute(&store.pool).await.unwrap();
    assert!(store.maintain_capture_identities(PROJECT).await.is_err());
    sqlx::query("DROP TRIGGER stop_cycle_page")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(legacy_rows(&store).await, original);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert!(reopened.maintain_capture_identities(PROJECT).await.unwrap());
    assert_eq!(legacy_rows(&reopened).await, original);
    let direct = reopened
        .create_entry(
            BlackboardEntryId::parse("fresh-direct-A").unwrap(),
            rule("Old meridian fact."),
        )
        .await
        .unwrap();
    reopened
        .update_entry(PROJECT, &direct.id, retire(&direct))
        .await
        .unwrap();
    // A live B-after-A is still eligible; B's metadata can change without restoration.
    let before = snapshot(&reopened).await;
    assert_eq!(reopened.get_hit(PROJECT, &cycle.id).await.unwrap(), None);
    assert_eq!(
        reopened
            .get_source_eligible_entry(PROJECT, &cycle.id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        reopened.get_entry(PROJECT, &cycle.id).await.unwrap(),
        Some(cycle.clone())
    );
    let queried = reopened
        .query(BlackboardQuery {
            project_id: PROJECT.to_string(),
            text: None,
            within_node: None,
            root_promotion: None,
            entry_scope: BlackboardEntryScope::All,
            max_results: 50,
        })
        .await
        .unwrap();
    assert!(!queried.data.iter().any(|hit| hit.entry.id == cycle.id));
    assert!(
        !reopened
            .get_hit(PROJECT, &control.id)
            .await
            .unwrap()
            .unwrap()
            .relations
            .iter()
            .any(|item| item.id == relation.id)
    );
    for root in [
        reopened
            .root_projection(RootBlackboardQuery {
                project_id: PROJECT.to_string(),
                max_entries: 256,
            })
            .await
            .unwrap(),
        reopened
            .root_projection_for_thread(
                RootBlackboardQuery {
                    project_id: PROJECT.to_string(),
                    max_entries: 256,
                },
                "thread-1",
            )
            .await
            .unwrap()
            .0,
    ] {
        assert!(!root.data.iter().any(|hit| hit.entry.id == cycle.id));
        assert!(root.data.iter().any(|hit| hit.entry.id == control.id));
    }
    let mut update = retire(&control);
    update.state = BlackboardEntryState::Active;
    update.content = cycle.value.content.clone();
    assert!(matches!(
        reopened
            .update_entry_from_model(PROJECT, &control.id, update)
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    assert_eq!(snapshot(&reopened).await, before);
    assert_eq!(changed.value.content, "Current cedar fact.");
    reopened.pool.close().await;
    let cold = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(cold.get_hit(PROJECT, &cycle.id).await.unwrap(), None);
    assert!(
        !cold
            .get_hit(PROJECT, &control.id)
            .await
            .unwrap()
            .unwrap()
            .relations
            .iter()
            .any(|item| item.id == relation.id)
    );
    assert_eq!(
        cold.get_source_eligible_entry(PROJECT, &control.id)
            .await
            .unwrap(),
        Some(control)
    );
}
