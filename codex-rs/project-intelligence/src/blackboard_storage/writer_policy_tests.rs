use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardStoreError;
use crate::ChangeOperation;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::SupersededEntry;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

#[tokio::test]
async fn model_retirement_and_succession_refuse_user_authority_inside_common_transaction() {
    for (provenance, authority) in [
        (
            BlackboardProvenanceKind::User,
            Some(KnowledgeAuthority::HumanDirect),
        ),
        (
            BlackboardProvenanceKind::User,
            Some(KnowledgeAuthority::AssistantReported),
        ),
        (BlackboardProvenanceKind::User, None),
        (
            BlackboardProvenanceKind::Agent,
            Some(KnowledgeAuthority::HumanDirect),
        ),
        (BlackboardProvenanceKind::Import, None),
        (
            BlackboardProvenanceKind::Agent,
            Some(KnowledgeAuthority::LegacyUnknown),
        ),
    ] {
        let home = TempDir::new().unwrap();
        let mut store = store(&home).await;
        let id = BlackboardEntryId::parse("protected").unwrap();
        let mut value = rule("Never push.");
        value.provenance.kind = provenance;
        let entry = store.create_entry(id.clone(), value.clone()).await.unwrap();
        if let Some(authority) = authority {
            store
                .record_context(
                    &entry,
                    &KnowledgeContext::new(KnowledgeCategory::Rule, authority),
                    /*change*/ None,
                )
                .await
                .unwrap();
        }
        let before = snapshot(&store).await;
        for attempt in 0..2 {
            assert!(matches!(
                store
                    .update_entry_from_model(PROJECT, &id, retire(&entry))
                    .await,
                Err(BlackboardStoreError::ModelMutationRefused)
            ));
            let model_change = crate::ChangeRecord {
                origin: crate::ChangeOrigin::ModelTool,
                ..change(ChangeOperation::Forgotten, "model retirement")
            };
            assert!(matches!(
                store
                    .update_entry_recorded(PROJECT, &id, retire(&entry), Some(&model_change))
                    .await,
                Err(BlackboardStoreError::ModelMutationRefused)
            ));
            let mut successor = value.clone();
            successor.provenance.kind = BlackboardProvenanceKind::Agent;
            assert!(matches!(
                store
                    .create_successor_from_model(
                        BlackboardEntryId::parse("model-successor").unwrap(),
                        successor,
                        vec![SupersededEntry {
                            id: id.clone(),
                            expected_revision: entry.revision
                        }]
                    )
                    .await,
                Err(BlackboardStoreError::ModelMutationRefused)
            ));
            assert_eq!(snapshot(&store).await, before);
            if attempt == 0 {
                store.pool.close().await;
                store = BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
                    codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
                ))
                .await
                .unwrap();
            }
        }
        // An explicit host Forget still succeeds on these targets.
        assert_eq!(
            store
                .update_entry_recorded(
                    PROJECT,
                    &id,
                    retire(&entry),
                    Some(&change(ChangeOperation::Forgotten, "Never push."))
                )
                .await
                .unwrap()
                .state,
            BlackboardEntryState::Tombstoned
        );
        store.pool.close().await;
    }
}

#[tokio::test]
async fn model_succession_retry_rechecks_authority_under_the_writer_lock() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let id = BlackboardEntryId::parse("agent-target").unwrap();
    let successor_id = BlackboardEntryId::parse("agent-successor").unwrap();
    let mut value = rule("Assistant-owned note");
    value.kind = crate::BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    let target = store.create_entry(id.clone(), value.clone()).await.unwrap();
    let request = vec![SupersededEntry {
        id: id.clone(),
        expected_revision: target.revision,
    }];
    let succession = store
        .create_successor_from_model(successor_id.clone(), value.clone(), request.clone())
        .await
        .unwrap();
    let before = snapshot(&store).await;
    assert_eq!(
        store
            .create_successor_from_model(successor_id.clone(), value.clone(), request.clone())
            .await
            .unwrap(),
        succession
    );
    assert_eq!(snapshot(&store).await, before);
    // A host context write can change policy without changing the entry revision.
    store
        .record_context(
            &succession.superseded[0],
            &KnowledgeContext::new(KnowledgeCategory::Note, KnowledgeAuthority::HumanDirect),
            /*change*/ None,
        )
        .await
        .unwrap();
    let before = snapshot(&store).await;
    assert!(matches!(
        store
            .create_successor_from_model(successor_id.clone(), value.clone(), request.clone())
            .await,
        Err(BlackboardStoreError::ModelMutationRefused)
    ));
    assert_eq!(snapshot(&store).await, before);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
        codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
    ))
    .await
    .unwrap();
    assert!(matches!(
        reopened
            .create_successor_from_model(successor_id, value, request)
            .await,
        Err(BlackboardStoreError::ModelMutationRefused)
    ));
    assert_eq!(snapshot(&reopened).await, before);
    reopened.pool.close().await;
}
