use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;

use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::ContextFields;
use super::read_context_row;
use crate::BlackboardEntryId;
use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardStoreError;
use crate::ChangeOperation;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;

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
    context.end_condition = Some("Ã©".repeat(/*n*/ 1000));
    context.group_id = Some("g".repeat(/*n*/ 512));
    context.payload = Some(format!("{{\"speaker\":\"{}\"}}", "Ã©".repeat(/*n*/ 80)));
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
            end_condition: Some("Ã©".repeat(/*n*/ 1001)),
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
            payload: Some(format!("{{\"speaker\":\"{}\"}}", "Ã©".repeat(/*n*/ 81))),
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

