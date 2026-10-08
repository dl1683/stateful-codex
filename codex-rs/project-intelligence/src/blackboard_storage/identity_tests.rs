use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use crate::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

#[tokio::test]
async fn c2_unicode_retirement_aliases_precede_limits_and_cross_writer_family_authority() {
    for (old, repeated) in [
        ("DNS: absent.", "DNS: absent."),
        ("DNS:\u{a0}absent.", "DNS: absent."),
        ("DNS - absent.", "DNS: absent."),
        ("ÜBER", "über"),
        ("é", "e\u{301}"),
        ("Straße", "STRASSE"),
        ("Σς", "σσ"),
    ] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let id = BlackboardEntryId::parse("retired").unwrap();
        let entry = store.create_entry(id.clone(), rule(old)).await.unwrap();
        store
            .update_entry(PROJECT, &id, retire(&entry))
            .await
            .unwrap();
        for ordinal in 0..300 {
            let mut value = rule("D NS: absent.");
            value.provenance.kind = BlackboardProvenanceKind::Agent;
            store
                .create_entry(
                    BlackboardEntryId::parse(format!("decoy-{ordinal}")).unwrap(),
                    value,
                )
                .await
                .unwrap();
        }
        let before = snapshot(&store).await;
        let mut value = rule(repeated);
        value.kind = BlackboardKind::Note;
        value.provenance.kind = BlackboardProvenanceKind::Agent;
        assert!(matches!(
            store
                .create_entry_with_context(
                    BlackboardEntryId::parse("new-category").unwrap(),
                    value,
                    KnowledgeContext::new(
                        KnowledgeCategory::Note,
                        KnowledgeAuthority::AssistantReported
                    ),
                    change(ChangeOperation::Saved, repeated)
                )
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
        assert_eq!(snapshot(&store).await, before);
        assert_eq!(
            retirement_capture_words(old),
            retirement_capture_words(repeated)
        );
    }
}

#[tokio::test]
async fn c2_disjoint_scope_control_and_ambiguous_primary_aliases_fail_closed() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    for scope in ["inv-A", "inv-B"] {
        sqlx::query("INSERT INTO knowledge_scopes(project_id, scope_id, kind, title, state, opened_source, created_at_ms, updated_at_ms) VALUES ('project-1', ?, 'investigation', 'Validated fixture', 'open', 'host-fixture', 1, 1)").bind(scope).execute(&store.pool).await.unwrap();
    }
    let make_context = |scope: &str| KnowledgeContext {
        scope_id: Some(scope.to_string()),
        ..KnowledgeContext::new(
            KnowledgeCategory::Note,
            KnowledgeAuthority::AssistantReported,
        )
    };
    let mut value = rule("Never push.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    let id = BlackboardEntryId::parse("scope-a").unwrap();
    let entry = store
        .create_entry_with_context(
            id.clone(),
            value.clone(),
            make_context("inv-A"),
            change(ChangeOperation::Saved, "Never push."),
        )
        .await
        .unwrap()
        .0;
    store
        .update_entry(PROJECT, &id, retire(&entry))
        .await
        .unwrap();
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("scope-b").unwrap(),
            value.clone(),
            make_context("inv-B"),
            change(ChangeOperation::Saved, "Never push."),
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .create_entry_with_context(
                BlackboardEntryId::parse("project-overlap").unwrap(),
                value.clone(),
                KnowledgeContext::new(
                    KnowledgeCategory::Note,
                    KnowledgeAuthority::AssistantReported
                ),
                change(ChangeOperation::Saved, "Never push.")
            )
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    assert!(matches!(
        store
            .create_entry_with_context(
                BlackboardEntryId::parse("invented-scope-escape").unwrap(),
                value.clone(),
                make_context("unresolved-invented"),
                change(ChangeOperation::Saved, "Never push.")
            )
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    let mut punctuation = value.clone();
    punctuation.content = "a-b".to_string();
    let punctuation_id = BlackboardEntryId::parse("punctuation-sensitive").unwrap();
    let punctuation_entry = store
        .create_entry_with_context(
            punctuation_id.clone(),
            punctuation,
            make_context("inv-A"),
            change(ChangeOperation::Saved, "a-b"),
        )
        .await
        .unwrap()
        .0;
    store
        .update_entry(PROJECT, &punctuation_id, retire(&punctuation_entry))
        .await
        .unwrap();
    value.content = "ab".to_string();
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("distinct-ab").unwrap(),
            value,
            make_context("inv-A"),
            change(ChangeOperation::Saved, "ab"),
        )
        .await
        .unwrap();
    // Two historical exact identities are not merged arbitrarily.
    for name in ["ambiguous-one", "ambiguous-two"] {
        store
            .create_entry_with_context(
                BlackboardEntryId::parse(name).unwrap(),
                rule("Never push."),
                KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
                ChangeRecord {
                    origin: ChangeOrigin::DirectControl,
                    ..change(ChangeOperation::Saved, "Never push.")
                },
            )
            .await
            .unwrap();
    }
    let before = snapshot(&store).await;
    assert!(matches!(
        store
            .write_capture(CaptureWrite {
                project_id: PROJECT.to_string(),
                units: vec![CaptureEntryWrite {
                    candidates: vec![BlackboardEntryId::parse("ambiguous-new").unwrap()],
                    value: rule("NEVER PUSH."),
                    context: KnowledgeContext::new(
                        KnowledgeCategory::Rule,
                        KnowledgeAuthority::HumanDirect
                    ),
                    change: ChangeRecord {
                        origin: ChangeOrigin::DirectControl,
                        ..change(ChangeOperation::Saved, "NEVER PUSH.")
                    }
                }]
            })
            .await,
        Err(BlackboardStoreError::AmbiguousIdentity)
    ));
    assert_eq!(snapshot(&store).await, before);
}

#[tokio::test]
async fn c2_direct_canonical_reuse_correction_retirement_and_fresh_restore() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let request = |action: &str, candidate: &str, text: &str| CaptureWrite {
        project_id: PROJECT.to_string(),
        units: vec![CaptureEntryWrite {
            candidates: vec![BlackboardEntryId::parse(candidate).unwrap()],
            value: rule(text),
            context: KnowledgeContext::new(
                KnowledgeCategory::Rule,
                KnowledgeAuthority::HumanDirect,
            ),
            change: ChangeRecord {
                origin: ChangeOrigin::DirectControl,
                action_id: Some(action.to_string()),
                group_id: Some(format!("fingerprint-{action}")),
                ..change(ChangeOperation::Saved, text)
            },
        }],
    };
    let entry = store
        .write_capture(request("first", "old-id", "Never push."))
        .await
        .unwrap()
        .entries[0]
        .0
        .clone();
    let reused = store
        .write_capture(request("second", "different-id", "NEVER  PUSH."))
        .await
        .unwrap()
        .entries;
    assert_eq!(reused, vec![(entry.clone(), MemberOutcome::AlreadyPresent)]);
    let successor = BlackboardEntryId::parse("successor").unwrap();
    store
        .create_successor_recorded(
            successor.clone(),
            rule("Never commit."),
            vec![SupersededEntry {
                id: entry.id.clone(),
                expected_revision: entry.revision,
            }],
            Some(&ChangeRecord {
                origin: ChangeOrigin::DirectControl,
                ..change(ChangeOperation::Corrected, "Never commit.")
            }),
        )
        .await
        .unwrap();
    let current = store.get_entry(PROJECT, &successor).await.unwrap().unwrap();
    store
        .update_entry(PROJECT, &successor, retire(&current))
        .await
        .unwrap();
    for words in ["Never push.", "Never commit."] {
        let mut value = rule(words);
        value.kind = BlackboardKind::Note;
        value.provenance.kind = BlackboardProvenanceKind::Agent;
        assert!(matches!(
            store
                .create_entry(
                    BlackboardEntryId::parse(format!("auto-{}", words.len())).unwrap(),
                    value
                )
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
    }
    assert!(matches!(
        store
            .write_capture(request("first", "old-id", "Never push."))
            .await,
        Err(BlackboardStoreError::ActionAlreadyRecorded(_))
    ));
    assert_eq!(
        store
            .write_capture(request("fresh", "deliberate-new-id", "Never push."))
            .await
            .unwrap()
            .entries[0]
            .1,
        MemberOutcome::Saved
    );
}

#[tokio::test]
async fn c2_identity_backfill_cold_cursor_and_unknown_source_fail_closed() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    for index in 0..300 {
        store
            .create_entry(
                BlackboardEntryId::parse(format!("legacy-{index}")).unwrap(),
                rule(&format!("Legacy words {index}.")),
            )
            .await
            .unwrap();
    }
    // Reproduce the pre-alias upgrade domain without rewriting applied migration SQL.
    sqlx::query("DELETE FROM capture_identity_aliases")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id, watermark) SELECT 'project-1', MAX(rowid) FROM blackboard_entry_revisions").execute(&store.pool).await.unwrap();
    let mut value = rule("new assistant note");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    assert!(matches!(
        store
            .create_entry(BlackboardEntryId::parse("auto").unwrap(), value.clone())
            .await,
        Err(BlackboardStoreError::IdentityCoverageIncomplete)
    ));
    sqlx::query("CREATE TRIGGER fail_second_identity_page BEFORE UPDATE ON capture_identity_coverage WHEN OLD.after_rowid >= 64 BEGIN SELECT RAISE(ABORT, 'after first durable page'); END").execute(&store.pool).await.unwrap();
    assert!(store.maintain_capture_identities(PROJECT).await.is_err());
    sqlx::query("DROP TRIGGER fail_second_identity_page")
        .execute(&store.pool)
        .await
        .unwrap();
    let after: i64 = sqlx::query_scalar(
        "SELECT after_rowid FROM capture_identity_coverage WHERE project_id = 'project-1'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(after, 64);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert!(reopened.maintain_capture_identities(PROJECT).await.unwrap());
    reopened
        .create_entry(BlackboardEntryId::parse("auto").unwrap(), value)
        .await
        .unwrap();
    let known_sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM capture_sources")
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    assert_eq!(known_sources, 0);
}
