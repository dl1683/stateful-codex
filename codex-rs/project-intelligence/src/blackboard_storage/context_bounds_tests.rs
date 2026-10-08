use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;

use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::ContextFields;
use super::StoredContext;
use super::read_context_row;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardStoreError;
use crate::ChangeOperation;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::RootBlackboardQuery;
use crate::SupersededEntry;

const PROJECT: &str = "project-1";

// Hash every cell, including all historical revisions, contexts, aliases, receipts and
// journal rows. Archival bytes are read in 4 KiB chunks, independently of consumer queries.
pub(in crate::blackboard_storage) async fn snapshot(store: &BlackboardStore) -> Vec<u8> {
    let mut transaction = store.pool.begin().await.unwrap();
    let tables = sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB 'sqlite_*' ORDER BY name")
        .fetch_all(&mut *transaction).await.unwrap();
    let mut hash = Sha256::new();
    for table in tables {
        hash.update(table.as_bytes());
        let columns =
            sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut *transaction)
                .await
                .unwrap();
        let count = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{table}\""
        )))
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        let keys = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info(?) WHERE pk > 0 ORDER BY pk",
        )
        .bind(&table)
        .fetch_all(&mut *transaction)
        .await
        .unwrap();
        let order = if keys.is_empty() { &columns } else { &keys };
        let order = order
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        for id in 0..count {
            hash.update(id.to_le_bytes());
            for column in &columns {
                let (kind, length) = sqlx::query_as::<_, (String, i64)>(sqlx::AssertSqlSafe(format!(
                    "SELECT typeof(\"{column}\"), COALESCE(length(CAST(\"{column}\" AS BLOB)), 0) FROM \"{table}\" ORDER BY {order} LIMIT 1 OFFSET ?")))
                    .bind(id).fetch_one(&mut *transaction).await.unwrap();
                hash.update(kind.as_bytes());
                hash.update(length.to_le_bytes());
                for start in (1..=length).step_by(/*step*/ 4096) {
                    let bytes = sqlx::query_scalar::<_, Vec<u8>>(sqlx::AssertSqlSafe(format!(
                        "SELECT substr(CAST(\"{column}\" AS BLOB), ?, 4096) FROM \"{table}\" ORDER BY {order} LIMIT 1 OFFSET ?")))
                        .bind(start).bind(id).fetch_one(&mut *transaction).await.unwrap();
                    hash.update(bytes);
                }
            }
        }
    }
    transaction.commit().await.unwrap();
    hash.finalize().to_vec()
}

#[tokio::test]
async fn new_context_components_are_bounded_and_identity_reads_select_only_needed_fields() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let id = BlackboardEntryId::parse("bounded-context").unwrap();
    let mut context = KnowledgeContext::new(
        KnowledgeCategory::Note,
        KnowledgeAuthority::AssistantReported,
    );
    context.end_condition = Some("\u{e9}".repeat(/*n*/ 1000));
    context.group_id = Some("g".repeat(/*n*/ 512));
    context.payload = Some(format!("{{\"speaker\":\"{}\"}}", "\u{e9}".repeat(/*n*/ 80)));
    let mut value = rule("Ordinary note");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    store
        .create_entry_with_context(
            id.clone(),
            value.clone(),
            context.clone(),
            change(ChangeOperation::Saved, "note"),
        )
        .await
        .unwrap();
    assert_eq!(
        store.knowledge_context(PROJECT, &id).await.unwrap(),
        Some(context.clone())
    );
    let mut connection = store.pool.acquire().await.unwrap();
    let identity = read_context_row(
        &mut connection,
        PROJECT,
        id.as_str(),
        ContextFields::Identity,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        (identity.end_condition, identity.group_id, identity.payload),
        (None, None, None)
    );
    let successor = read_context_row(
        &mut connection,
        PROJECT,
        id.as_str(),
        ContextFields::Successor,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        (
            successor.end_condition,
            successor.group_id,
            successor.payload
        ),
        (
            context.end_condition.clone(),
            context.group_id.clone(),
            None
        )
    );
    drop(connection);
    let before = snapshot(&store).await;
    for unsupported in [
        KnowledgeContext {
            end_condition: Some("\u{e9}".repeat(/*n*/ 1001)),
            ..context.clone()
        },
        KnowledgeContext {
            scope_id: Some("s".repeat(/*n*/ 513)),
            ..context.clone()
        },
        KnowledgeContext {
            group_id: Some("g".repeat(/*n*/ 513)),
            ..context.clone()
        },
        KnowledgeContext {
            payload: Some(format!("{{\"speaker\":\"{}\"}}", "\u{e9}".repeat(/*n*/ 81))),
            ..context.clone()
        },
        KnowledgeContext {
            payload: Some(format!("{{\"path\":\"{}\"}}", "p".repeat(/*n*/ 513))),
            ..context.clone()
        },
        KnowledgeContext {
            payload: Some("x".repeat(/*n*/ 1048576)),
            ..context.clone()
        },
    ] {
        assert!(matches!(
            store
                .create_entry_with_context(
                    BlackboardEntryId::parse("oversized-new").unwrap(),
                    value.clone(),
                    unsupported,
                    change(ChangeOperation::Saved, "note")
                )
                .await,
            Err(BlackboardStoreError::UnsupportedContext)
        ));
        assert_eq!(snapshot(&store).await, before);
    }
    store.pool.close().await;
}

pub(in crate::blackboard_storage) fn retire(
    entry: &crate::BlackboardEntry,
) -> BlackboardEntryUpdate {
    BlackboardEntryUpdate {
        expected_revision: entry.revision,
        kind: entry.value.kind,
        content: entry.value.content.clone(),
        structured_value: entry.value.structured_value.clone(),
        confidence: entry.value.confidence,
        verification: entry.value.verification,
        importance: entry.value.importance,
        root_promotion: entry.value.root_promotion,
        evidence: entry.value.evidence.clone(),
        premises: entry.value.premises.clone(),
        state: BlackboardEntryState::Tombstoned,
        superseded_by: None,
        provenance: entry.value.provenance.clone(),
    }
}

#[tokio::test]
async fn legacy_context_materialization_is_bounded_before_root_policy_and_writer_reads() {
    for field in [
        "payload",
        "end_condition",
        "group_id",
        "scope_id",
        "speaker",
        "authority",
    ] {
        let home = TempDir::new().unwrap();
        let mut store = store(&home).await;
        let id = BlackboardEntryId::parse("legacy-note").unwrap();
        let mut value = rule("Short ordinary historical note.");
        value.kind = BlackboardKind::Note;
        value.provenance.kind = BlackboardProvenanceKind::Agent;
        let context = KnowledgeContext::new(
            KnowledgeCategory::Note,
            KnowledgeAuthority::AssistantReported,
        );
        let entry = store
            .create_entry_with_context(
                id.clone(),
                value.clone(),
                context,
                change(ChangeOperation::Saved, "note"),
            )
            .await
            .unwrap()
            .0;
        let (column, bytes) = match field {
            "payload" => (
                "payload",
                format!("{{\"speaker\":\"{}\"}}", "x".repeat(/*n*/ 1048576)),
            ),
            "speaker" => (
                "payload",
                format!("{{\"speaker\":\"{}\"}}", "\u{e9}".repeat(/*n*/ 81)),
            ),
            other => (other, "x".repeat(/*n*/ 1048576)),
        };
        let mut connection = store.pool.acquire().await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE knowledge_context SET {column} = ? WHERE entry_id = ?"
        )))
        .bind(bytes)
        .bind(id.as_str())
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        drop(connection);
        let before = snapshot(&store).await;
        for reopen in 0..2 {
            let mut connection = store.pool.acquire().await.unwrap();
            // Observe the actual SQL result allocation: unsupported rows materialize no text.
            let selected = read_context_row(
                &mut connection,
                PROJECT,
                id.as_str(),
                super::ContextFields::Complete,
            )
            .await
            .unwrap()
            .unwrap();
            let text = [
                &selected.category,
                &selected.authority,
                &selected.validity,
                &selected.scope_id,
                &selected.end_condition,
                &selected.group_id,
                &selected.payload,
            ];
            let materialized_bytes: usize = text
                .iter()
                .filter_map(|field| field.as_ref())
                .map(String::len)
                .sum();
            let allocated_text_capacity: usize = text
                .iter()
                .filter_map(|field| field.as_ref())
                .map(String::capacity)
                .sum();
            assert_eq!((materialized_bytes, allocated_text_capacity), (0, 0));
            assert_eq!(
                selected,
                StoredContext {
                    supported: false,
                    category: None,
                    authority: None,
                    validity: None,
                    scope_id: None,
                    end_condition: None,
                    source_sequence: None,
                    unit_ordinal: None,
                    group_id: None,
                    payload: None
                }
            );
            drop(connection);
            assert!(matches!(
                store.knowledge_context(PROJECT, &id).await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            assert!(matches!(
                store.knowledge_policy(PROJECT, &id).await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            assert!(matches!(
                store
                    .write_capture(crate::CaptureWrite {
                        project_id: PROJECT.into(),
                        units: vec![crate::CaptureEntryWrite {
                            candidates: vec![id.clone()],
                            value: value.clone(),
                            context: KnowledgeContext::new(
                                KnowledgeCategory::Note,
                                KnowledgeAuthority::AssistantReported
                            ),
                            change: change(ChangeOperation::Saved, "note")
                        }]
                    })
                    .await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            assert!(matches!(
                store
                    .update_entry_from_model(PROJECT, &id, retire(&entry))
                    .await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            let mut revise = retire(&entry);
            revise.state = BlackboardEntryState::Active;
            revise.content = "Changed note".into();
            assert!(matches!(
                store.update_entry_from_model(PROJECT, &id, revise).await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            assert!(matches!(
                store
                    .create_successor(
                        BlackboardEntryId::parse("corrected-note").unwrap(),
                        value.clone(),
                        vec![SupersededEntry {
                            id: id.clone(),
                            expected_revision: entry.revision
                        }]
                    )
                    .await,
                Err(BlackboardStoreError::UnsupportedContext)
            ));
            let query = RootBlackboardQuery {
                project_id: PROJECT.into(),
                max_entries: 1,
            };
            if field == "scope_id" {
                assert!(
                    store
                        .root_projection(query.clone())
                        .await
                        .unwrap()
                        .data
                        .is_empty()
                );
                assert!(
                    store
                        .root_projection_for_thread(query, "thread")
                        .await
                        .unwrap()
                        .0
                        .data
                        .is_empty()
                );
            } else {
                assert!(matches!(
                    store.root_projection(query.clone()).await,
                    Err(BlackboardStoreError::UnsupportedContext)
                ));
                assert!(matches!(
                    store.root_projection_for_thread(query, "thread").await,
                    Err(BlackboardStoreError::UnsupportedContext)
                ));
            }
            assert_eq!(
                snapshot(&store).await,
                before,
                "field={field}, reopen={reopen}"
            );
            store.pool.close().await;
            // All the same operations must remain bounded after closing and reopening.
            if reopen == 0 {
                store = BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
                    codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
                ))
                .await
                .unwrap();
            }
        }
        let reopened = BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
        ))
        .await
        .unwrap();
        assert!(matches!(
            reopened.knowledge_context(PROJECT, &id).await,
            Err(BlackboardStoreError::UnsupportedContext)
        ));
        assert_eq!(snapshot(&reopened).await, before);
        reopened.pool.close().await;
    }
}
