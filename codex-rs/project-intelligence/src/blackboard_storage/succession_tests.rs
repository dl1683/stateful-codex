use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::SupersededEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardStoreError;
use crate::BlackboardVerification;
use crate::ConfidenceScore;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootBlackboardQuery;
use crate::RootPromotion;

const PROJECT_ID: &str = "project-1";

// Complete logical bytes for every PI table in this bounded-text fixture.
async fn database_rows(store: &BlackboardStore) -> Vec<Vec<String>> {
    let mut transaction = store.pool.begin().await.unwrap();
    let tables = sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB 'sqlite_*' ORDER BY name")
        .fetch_all(&mut *transaction).await.unwrap();
    let mut result = Vec::new();
    for table in tables {
        let columns =
            sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut *transaction)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|name| format!("quote(\"{name}\")"))
            .collect::<Vec<_>>()
            .join(",");
        result.push(
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
                "SELECT json_array({fields}) AS cells FROM \"{table}\" ORDER BY cells"
            )))
            .fetch_all(&mut *transaction)
            .await
            .unwrap(),
        );
    }
    transaction.commit().await.unwrap();
    result
}

#[tokio::test]
async fn predecessor_fan_in_is_bounded_before_materialization_and_survives_reopen() {
    let home = TempDir::new().unwrap();
    let mut store = store(&home).await;
    let successor = BlackboardEntryId::parse("successor").unwrap();
    let value = decision("Current short note.", RootPromotion::Promoted);
    let mut replaced = Vec::new();
    for i in 0..4 {
        let id = BlackboardEntryId::parse(format!("predecessor-{i:08}")).unwrap();
        store
            .create_entry(
                id.clone(),
                decision(
                    &format!("Original short note {i}."),
                    RootPromotion::NotPromoted,
                ),
            )
            .await
            .unwrap();
        replaced.push(SupersededEntry {
            id,
            expected_revision: 1,
        });
    }
    let initial = store
        .create_successor_from_model(successor.clone(), value.clone(), replaced.clone())
        .await
        .unwrap();
    assert_eq!(
        store
            .create_successor_from_model(successor.clone(), value.clone(), replaced.clone())
            .await
            .unwrap(),
        initial
    );
    let mut previous = 4;
    for count in [64, 4096] {
        for i in previous..count {
            let id = BlackboardEntryId::parse(format!("predecessor-{i:08}")).unwrap();
            let entry = store
                .create_entry(
                    id.clone(),
                    decision(
                        &format!("Original short note {i}."),
                        RootPromotion::NotPromoted,
                    ),
                )
                .await
                .unwrap();
            let mut update = super::super::context_bounds::tests::retire(&entry);
            update.state = BlackboardEntryState::Superseded;
            update.superseded_by = Some(successor.clone());
            store
                .update_entry_from_model(PROJECT_ID, &id, update)
                .await
                .unwrap();
        }
        previous = count;
        let before = database_rows(&store).await;
        for attempt in 0..2 {
            let mut connection = store.pool.acquire().await.unwrap();
            // Actual production selectors, measured before full entry loads/dedup/display.
            let root_ids = super::newest_predecessor_ids(
                &mut connection,
                PROJECT_ID,
                &[
                    successor.clone(),
                    successor.clone(),
                    BlackboardEntryId::parse("absent").unwrap(),
                ],
            )
            .await
            .unwrap();
            let preview_ids =
                super::predecessor_ids(&mut connection, PROJECT_ID, &successor, /*limit*/ 3)
                    .await
                    .unwrap();
            let history_ids =
                super::predecessor_ids(&mut connection, PROJECT_ID, &successor, /*limit*/ 30)
                    .await
                    .unwrap();
            let replay_ids = super::predecessor_ids(
                &mut connection,
                PROJECT_ID,
                &successor,
                (super::MAX_SUPERSEDED_ENTRIES + 1) as u32,
            )
            .await
            .unwrap();
            assert_eq!(
                (
                    root_ids.len(),
                    preview_ids.len(),
                    history_ids.len(),
                    replay_ids.len()
                ),
                (1, 3, 30, 5)
            );
            drop(connection);
            let packet = store
                .root_projection(RootBlackboardQuery {
                    project_id: PROJECT_ID.to_string(),
                    max_entries: 1,
                })
                .await
                .unwrap();
            assert_eq!(
                packet
                    .data
                    .iter()
                    .map(|hit| &hit.entry.id)
                    .collect::<Vec<_>>(),
                vec![&successor]
            );
            let root = store
                .newest_predecessors(PROJECT_ID, std::slice::from_ref(&successor))
                .await
                .unwrap();
            let (preview, preview_more) = store
                .predecessor_page(PROJECT_ID, &successor, /*limit*/ 3)
                .await
                .unwrap();
            let (history, history_more) = store
                .predecessor_page(PROJECT_ID, &successor, /*limit*/ 30)
                .await
                .unwrap();
            assert_eq!(
                (
                    root.len(),
                    preview.len(),
                    history.len(),
                    preview_more,
                    history_more
                ),
                (1, 3, 30, true, true)
            );
            assert_eq!(
                preview
                    .iter()
                    .map(|entry| entry.id.to_string())
                    .collect::<Vec<_>>(),
                preview_ids
            );
            assert_eq!(
                history
                    .iter()
                    .map(|entry| entry.id.to_string())
                    .collect::<Vec<_>>(),
                history_ids
            );
            let newest = sqlx::query_scalar::<_, String>("SELECT id FROM blackboard_entries WHERE id LIKE 'predecessor-%' ORDER BY updated_at_ms DESC, id LIMIT 1").fetch_one(&store.pool).await.unwrap();
            assert_eq!(root[0].1.id.as_str(), newest);
            assert!(matches!(
                store.superseded_by(PROJECT_ID, &successor).await,
                Err(BlackboardStoreError::EntryIdentityConflict(_))
            ));
            for model in [false, true] {
                let result = if model {
                    store
                        .create_successor_from_model(
                            successor.clone(),
                            value.clone(),
                            replaced.clone(),
                        )
                        .await
                } else {
                    store
                        .create_successor(successor.clone(), value.clone(), replaced.clone())
                        .await
                };
                assert!(matches!(
                    result,
                    Err(BlackboardStoreError::EntryIdentityConflict(_))
                ));
            }
            assert_eq!(database_rows(&store).await, before);
            if attempt == 0 {
                store.pool.close().await;
                store = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
                    .await
                    .unwrap();
            }
        }
    }
    assert_eq!(
        store
            .predecessor_page(PROJECT_ID, &successor, /*limit*/ 0)
            .await
            .unwrap(),
        (Vec::new(), true)
    );
    assert_eq!(
        store
            .predecessor_page(
                PROJECT_ID,
                &BlackboardEntryId::parse("absent").unwrap(),
                u32::MAX
            )
            .await
            .unwrap(),
        (Vec::new(), false)
    );
    assert_eq!(
        store
            .predecessor_page(PROJECT_ID, &successor, u32::MAX)
            .await
            .unwrap()
            .0
            .len(),
        200
    );
    store.pool.close().await;
}

fn decision(content: &str, promotion: RootPromotion) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
        kind: BlackboardKind::Decision,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: promotion,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-1".to_string(),
        },
    }
}

async fn store(temp_dir: &TempDir) -> BlackboardStore {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy opens")
        .create_node(
            HierarchyNodeId::parse("node-project").expect("node ID"),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node");
    BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard opens")
}

/// "ONE decimal place" becomes "TWO decimal places": one current value, promoted like its
/// predecessor, with the old value kept as superseded history; a retry is idempotent and a
/// stale revision changes nothing.
#[tokio::test]
async fn a_successor_replaces_its_predecessor_atomically() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let one_id = BlackboardEntryId::parse("decision-one").expect("ID");
    let one = store
        .create_entry(
            one_id.clone(),
            decision(
                "Default formatting uses ONE decimal place.",
                RootPromotion::Promoted,
            ),
        )
        .await
        .expect("ONE stored");
    let two_id = BlackboardEntryId::parse("decision-two").expect("ID");
    let two_value = decision(
        "Default formatting uses TWO decimal places (the requirement changed).",
        RootPromotion::NotPromoted,
    );
    let replaced = vec![SupersededEntry {
        id: one_id.clone(),
        expected_revision: one.revision,
    }];

    let succession = store
        .create_successor(two_id.clone(), two_value.clone(), replaced.clone())
        .await
        .expect("TWO replaces ONE");
    let retry = store
        .create_successor(two_id.clone(), two_value.clone(), replaced.clone())
        .await
        .expect("retry returns the stored result");
    let stale = store
        .create_successor(
            BlackboardEntryId::parse("decision-three").expect("ID"),
            decision("Three decimals.", RootPromotion::NotPromoted),
            replaced,
        )
        .await
        .expect_err("ONE is no longer current");
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 10,
        })
        .await
        .expect("root");

    assert_eq!(
        (
            succession.successor.value.root_promotion,
            succession.superseded[0].state,
            succession.superseded[0].superseded_by.clone(),
            succession.superseded[0].value.content.clone(),
            retry == succession,
            matches!(stale, BlackboardStoreError::RevisionConflict { .. }),
            root.data
                .iter()
                .map(|hit| hit.entry.id.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            RootPromotion::Promoted,
            BlackboardEntryState::Superseded,
            Some(two_id),
            "Default formatting uses ONE decimal place.".to_string(),
            true,
            true,
            vec!["decision-two".to_string()],
        )
    );
    assert!(
        store
            .get_entry(
                PROJECT_ID,
                &BlackboardEntryId::parse("decision-three").expect("ID")
            )
            .await
            .expect("lookup")
            .is_none()
    );
}

#[tokio::test]
async fn invalid_successions_are_refused() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let id = BlackboardEntryId::parse("decision-self").expect("ID");
    let error = store
        .create_successor(
            id.clone(),
            decision("Self.", RootPromotion::NotPromoted),
            vec![SupersededEntry {
                id,
                expected_revision: 1,
            }],
        )
        .await
        .expect_err("an entry cannot replace itself");
    assert!(matches!(error, BlackboardStoreError::InvalidSuccession));
}

/// A reused successor ID with different content, or a retry after the successor itself was
/// replaced, is not a retry.
#[tokio::test]
async fn conflicting_retries_are_refused() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let one_id = BlackboardEntryId::parse("decision-one").expect("ID");
    let one = store
        .create_entry(one_id.clone(), decision("ONE.", RootPromotion::Promoted))
        .await
        .expect("ONE");
    let two_id = BlackboardEntryId::parse("decision-two").expect("ID");
    let to_two = vec![SupersededEntry {
        id: one_id,
        expected_revision: one.revision,
    }];
    let two = store
        .create_successor(
            two_id.clone(),
            decision("TWO.", RootPromotion::NotPromoted),
            to_two.clone(),
        )
        .await
        .expect("TWO")
        .successor;
    let different = store
        .create_successor(
            two_id.clone(),
            decision("TWO, reworded.", RootPromotion::NotPromoted),
            to_two.clone(),
        )
        .await
        .expect_err("different content under the same ID");
    store
        .create_successor(
            BlackboardEntryId::parse("decision-three").expect("ID"),
            decision("THREE.", RootPromotion::NotPromoted),
            vec![SupersededEntry {
                id: two_id.clone(),
                expected_revision: two.revision,
            }],
        )
        .await
        .expect("THREE");
    let stale_retry = store
        .create_successor(two_id, decision("TWO.", RootPromotion::NotPromoted), to_two)
        .await
        .expect_err("TWO is no longer current");
    assert_eq!(
        (
            matches!(different, BlackboardStoreError::EntryIdentityConflict(_)),
            matches!(stale_retry, BlackboardStoreError::EntryIdentityConflict(_)),
        ),
        (true, true)
    );
}

/// A retry must name exactly the entries the successor replaced, at the revisions it named.
#[tokio::test]
async fn a_retry_must_name_the_same_replacement() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let a_id = BlackboardEntryId::parse("decision-a").expect("ID");
    let b_id = BlackboardEntryId::parse("decision-b").expect("ID");
    let a = store
        .create_entry(a_id.clone(), decision("A.", RootPromotion::NotPromoted))
        .await
        .expect("A");
    let b = store
        .create_entry(b_id.clone(), decision("B.", RootPromotion::NotPromoted))
        .await
        .expect("B");
    let merged_id = BlackboardEntryId::parse("decision-ab").expect("ID");
    let both = vec![
        SupersededEntry {
            id: a_id.clone(),
            expected_revision: a.revision,
        },
        SupersededEntry {
            id: b_id,
            expected_revision: b.revision,
        },
    ];
    store
        .create_successor(
            merged_id.clone(),
            decision("A and B.", RootPromotion::NotPromoted),
            both.clone(),
        )
        .await
        .expect("merge");
    let retry = |replaced: Vec<SupersededEntry>| {
        store.create_successor(
            merged_id.clone(),
            decision("A and B.", RootPromotion::NotPromoted),
            replaced,
        )
    };
    let exact = retry(both).await;
    let only_a = retry(vec![SupersededEntry {
        id: a_id.clone(),
        expected_revision: a.revision,
    }])
    .await;
    let wrong_revision = retry(vec![
        SupersededEntry {
            id: a_id,
            expected_revision: 7,
        },
        SupersededEntry {
            id: BlackboardEntryId::parse("decision-b").expect("ID"),
            expected_revision: b.revision,
        },
    ])
    .await;
    assert_eq!(
        (
            exact.is_ok(),
            matches!(only_a, Err(BlackboardStoreError::EntryIdentityConflict(_))),
            matches!(
                wrong_revision,
                Err(BlackboardStoreError::EntryIdentityConflict(_))
            ),
        ),
        (true, true, true)
    );
}
