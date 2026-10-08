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

pub(super) async fn legacy_rows(store: &BlackboardStore) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    for table in [
        "blackboard_entries",
        "blackboard_entry_revisions",
        "knowledge_context",
        "memory_changes",
    ] {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&store.pool)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|name| format!("quote(\"{name}\")"))
            .collect::<Vec<_>>()
            .join(",");
        result.push(
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT json_array({fields}) AS cells FROM {table} ORDER BY cells"
            )))
            .fetch_all(&store.pool)
            .await
            .unwrap(),
        );
    }
    result
}

#[tokio::test]
async fn c2r1_legacy_metadata_revisions_preserve_active_words_and_real_retirements() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let mut value = rule("Active meridian note.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    let id = BlackboardEntryId::parse("legacy-active").unwrap();
    let mut active = store.create_entry(id.clone(), value.clone()).await.unwrap();
    for index in 0..66 {
        let mut update = retire(&active);
        update.state = BlackboardEntryState::Active;
        update.confidence = ConfidenceScore::from_basis_points(5000 + index).unwrap();
        active = store
            .update_entry_from_model(PROJECT, &id, update)
            .await
            .unwrap();
    }
    value.content = "Old corrected wording.".to_string();
    let changed_id = BlackboardEntryId::parse("legacy-changed").unwrap();
    let changed = store
        .create_entry(changed_id.clone(), value.clone())
        .await
        .unwrap();
    let mut correction = retire(&changed);
    correction.state = BlackboardEntryState::Active;
    correction.content = "Current corrected wording.".to_string();
    let changed = store
        .update_entry_from_model(PROJECT, &changed_id, correction)
        .await
        .unwrap();
    value.content = "Actually forgotten wording.".to_string();
    let forgotten_id = BlackboardEntryId::parse("legacy-forgotten").unwrap();
    let forgotten = store
        .create_entry(forgotten_id.clone(), value)
        .await
        .unwrap();
    store
        .update_entry(PROJECT, &forgotten_id, retire(&forgotten))
        .await
        .unwrap();
    let original = legacy_rows(&store).await;
    sqlx::query("DELETE FROM capture_identity_aliases")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id, watermark) SELECT 'project-1', MAX(rowid) FROM blackboard_entry_revisions").execute(&store.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER stop_after_identity_page BEFORE UPDATE ON capture_identity_coverage WHEN OLD.after_rowid >= 64 BEGIN SELECT RAISE(ABORT, 'cold continuation'); END").execute(&store.pool).await.unwrap();
    assert!(store.maintain_capture_identities(PROJECT).await.is_err());
    sqlx::query("DROP TRIGGER stop_after_identity_page")
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
    assert_eq!(
        reopened
            .get_source_eligible_entry(PROJECT, &id)
            .await
            .unwrap(),
        Some(active.clone())
    );
    assert_eq!(
        reopened
            .get_source_eligible_entry(PROJECT, &changed_id)
            .await
            .unwrap(),
        Some(changed)
    );
    assert!(
        reopened
            .source_text_eligible(PROJECT, &active.value.content)
            .await
            .unwrap()
    );
    let seal = reopened
        .observe_source(
            super::super::source_fixture::observation("active-original", &active.value.content),
            &active.value.content,
        )
        .await
        .unwrap();
    assert_eq!(
        reopened
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &seal.digest,
                /*start*/ 0,
                active.value.content.len() as u32
            )
            .await
            .unwrap()
            .exact_text,
        active.value.content
    );
    for retired in ["Old corrected wording.", "Actually forgotten wording."] {
        assert!(
            !reopened
                .source_text_eligible(PROJECT, retired)
                .await
                .unwrap()
        );
        let mut copy = active.value.clone();
        copy.content = retired.to_string();
        assert!(matches!(
            reopened
                .create_entry(
                    BlackboardEntryId::parse(format!("copy-{retired}")).unwrap(),
                    copy
                )
                .await,
            Err(BlackboardStoreError::RetiredIdentity)
        ));
    }
    let mut update = retire(&active);
    update.state = BlackboardEntryState::Active;
    update.root_promotion = RootPromotion::Candidate;
    let revised = reopened
        .update_entry_from_model(PROJECT, &id, update)
        .await
        .unwrap();
    assert_eq!(
        (revised.value.content, revised.revision),
        (active.value.content, active.revision + 1)
    );
}

#[tokio::test]
async fn c2r1_container_policy_covers_create_update_succession_and_existing_reads() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let mut value = rule("Rule: Never push.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    let enclosing_id = BlackboardEntryId::parse("existing-container").unwrap();
    store
        .create_entry(enclosing_id.clone(), value.clone())
        .await
        .unwrap();
    value.content = "Independent cedar note.".to_string();
    let independent_id = BlackboardEntryId::parse("independent").unwrap();
    let independent = store
        .create_entry(independent_id.clone(), value.clone())
        .await
        .unwrap();
    let forgotten_id = BlackboardEntryId::parse("forgotten-rule").unwrap();
    let forgotten = store
        .create_entry(forgotten_id.clone(), rule("Never push."))
        .await
        .unwrap();
    store
        .update_entry(PROJECT, &forgotten_id, retire(&forgotten))
        .await
        .unwrap();
    let before = snapshot(&store).await;
    assert_eq!(
        store
            .get_source_eligible_entry(PROJECT, &enclosing_id)
            .await
            .unwrap(),
        None
    );
    assert!(
        store
            .get_hit(PROJECT, &enclosing_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .root_projection(RootBlackboardQuery {
                project_id: PROJECT.to_string(),
                max_entries: 1
            })
            .await
            .unwrap()
            .data
            .len(),
        1
    );
    value.content = "Rule: Never push.".to_string();
    assert!(matches!(
        store
            .create_entry(
                BlackboardEntryId::parse("new-container").unwrap(),
                value.clone()
            )
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    let mut revise = retire(&independent);
    revise.state = BlackboardEntryState::Active;
    revise.content = value.content.clone();
    assert!(matches!(
        store
            .update_entry_from_model(PROJECT, &independent_id, revise)
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    assert!(matches!(
        store
            .create_successor_from_model(
                BlackboardEntryId::parse("successor-container").unwrap(),
                value,
                vec![SupersededEntry {
                    id: independent_id,
                    expected_revision: independent.revision
                }]
            )
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    assert_eq!(snapshot(&store).await, before);
}

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
    value.content = "Heading: Never push.".to_string();
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
