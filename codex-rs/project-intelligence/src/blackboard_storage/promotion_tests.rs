use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const DECISION: &str =
    "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop.";

async fn proposal(
    store: &BlackboardStore,
    admission: &codex_state::ThreadProjectAdmission,
    event: &str,
    text: &str,
    category: ProposalCategory,
) -> BlackboardEntryId {
    let seal = store
        .observe_source(observation(event, text), text)
        .await
        .unwrap();
    let results = store
        .propose_sources(
            admission,
            HierarchyNodeId::parse("node-project").unwrap(),
            "turn-1",
            vec![SourceProposal {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                part_index: 0,
                // A cited range with surrounding whitespace still applies only exact words.
                spans: vec![SourceSpan {
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    role: SourceSpanRole::Body,
                }],
                category,
                interpretation: "Model reading of the user's choice.".to_string(),
                dependency: None,
                temporal: None,
                event_status: None,
                attribution: None,
                speaker_span: None,
            }],
        )
        .await
        .unwrap();
    BlackboardEntryId::parse(results[0].entry_id.clone().unwrap()).unwrap()
}

fn apply(id: &BlackboardEntryId, revision: u64, action: &str) -> PromotionRequest {
    PromotionRequest {
        entry_id: id.clone(),
        expected_revision: revision,
        category: PromotionCategory::Decision,
        action_id: action.to_string(),
    }
}

async fn root_contents(store: &BlackboardStore) -> Vec<String> {
    store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".into(),
            max_entries: 256,
        })
        .await
        .unwrap()
        .data
        .into_iter()
        .map(|hit| hit.entry.value.content)
        .collect()
}

#[tokio::test]
async fn c456_apply_quotes_exact_decision_and_undo_restores_the_proposal() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = format!("  {DECISION}\n");
    let id = proposal(
        &store,
        &admission,
        "decision",
        &text,
        ProposalCategory::Decision,
    )
    .await;
    assert_eq!(root_contents(&store).await, Vec::<String>::new());

    let stale = store
        .promote_proposal(&admission, apply(&id, 2, "apply-stale"))
        .await;
    assert!(matches!(
        stale,
        Err(BlackboardStoreError::RevisionConflict {
            expected: 2,
            actual: 1
        })
    ));
    let receipt = store
        .promote_proposal(&admission, apply(&id, 1, "apply-1"))
        .await
        .unwrap();
    assert_eq!(
        (receipt.kind.as_str(), receipt.saved, receipt.members.len()),
        ("promotion-v1", 1, 1)
    );
    let applied = store.get_entry("project-1", &id).await.unwrap().unwrap();
    assert_eq!(
        (
            applied.revision,
            applied.value.kind,
            applied.value.content.as_str(),
            applied.value.root_promotion,
            applied.value.provenance.kind,
        ),
        (
            2,
            BlackboardKind::Decision,
            DECISION,
            RootPromotion::Promoted,
            BlackboardProvenanceKind::User
        )
    );
    let context = store
        .knowledge_context("project-1", &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (context.category, context.authority),
        (KnowledgeCategory::Decision, KnowledgeAuthority::HumanDirect)
    );
    let payload: serde_json::Value =
        serde_json::from_str(context.payload.as_deref().unwrap()).unwrap();
    assert_eq!(payload["temporal"]["sourceTime"]["type"], "hostObserved");
    assert_eq!(payload["promotion"]["quoted"]["startByte"], 2);
    assert_eq!(root_contents(&store).await, vec![DECISION.to_string()]);

    // Same action replays its receipt; the action cannot be reused for anything else.
    assert_eq!(
        store
            .promote_proposal(&admission, apply(&id, 1, "apply-1"))
            .await
            .unwrap(),
        receipt
    );
    assert!(matches!(
        store
            .promote_proposal(&admission, apply(&id, 2, "apply-1"))
            .await,
        Err(BlackboardStoreError::ActionAlreadyRecorded(_))
    ));

    let undone = store
        .undo_capture_group(&admission, &receipt.group_id, "undo-1")
        .await
        .unwrap();
    assert_eq!((undone.kind.as_str(), undone.saved), ("undo-v1", 1));
    let restored = store.get_entry("project-1", &id).await.unwrap().unwrap();
    assert_eq!(
        (
            restored.revision,
            restored.value.kind,
            restored.value.content.as_str(),
            restored.value.root_promotion
        ),
        (
            3,
            BlackboardKind::Note,
            "Model reading of the user's choice.",
            RootPromotion::NotPromoted
        )
    );
    assert!(
        store
            .proposal_context("project-1", &id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(root_contents(&store).await, Vec::<String>::new());
    let before = snapshot(&store).await;
    assert_eq!(
        store
            .undo_capture_group(&admission, &receipt.group_id, "undo-1")
            .await
            .unwrap(),
        undone
    );
    // Undo does not resurrect: the undone words are retired, so their source is fenced from
    // automatic use; only a fresh explicit add restores them.
    assert!(matches!(
        store
            .promote_proposal(&admission, apply(&id, 3, "apply-again"))
            .await,
        Err(BlackboardStoreError::SourceExcluded)
    ));
    assert_eq!(snapshot(&store).await, before);
}

#[tokio::test]
async fn c456_ruled_out_keeps_whole_reason_and_model_cannot_flip_it() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "It's not the cache - clearing it didn't change the timing.";
    let id = proposal(
        &store,
        &admission,
        "ruled-out",
        text,
        ProposalCategory::RuledOut,
    )
    .await;
    store
        .promote_proposal(
            &admission,
            PromotionRequest {
                category: PromotionCategory::RuledOut,
                ..apply(&id, 1, "apply-ruled-out")
            },
        )
        .await
        .unwrap();
    let applied = store.get_entry("project-1", &id).await.unwrap().unwrap();
    assert_eq!(
        (applied.value.kind, applied.value.content.as_str()),
        (BlackboardKind::RejectedApproach, text)
    );
    let payload: serde_json::Value = serde_json::from_str(
        store
            .knowledge_context("project-1", &id)
            .await
            .unwrap()
            .unwrap()
            .payload
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["ruledOut"]["speaker"], "userAct");
    assert_eq!(root_contents(&store).await, vec![text.to_string()]);
    let before = snapshot(&store).await;
    // Neither a model revision nor a model retirement can flip or silently drop it.
    let mut flipped = retire(&applied);
    flipped.state = BlackboardEntryState::Active;
    flipped.kind = BlackboardKind::Decision;
    assert!(matches!(
        store
            .update_entry_from_model("project-1", &id, flipped)
            .await,
        Err(BlackboardStoreError::ModelMutationRefused)
    ));
    assert!(matches!(
        store
            .update_entry_from_model("project-1", &id, retire(&applied))
            .await,
        Err(BlackboardStoreError::ModelMutationRefused)
    ));
    assert_eq!(snapshot(&store).await, before);
    // The user's Forget does remove it from automatic use.
    store
        .update_entry("project-1", &id, retire(&applied))
        .await
        .unwrap();
    assert_eq!(root_contents(&store).await, Vec::<String>::new());
}

#[tokio::test]
async fn c456_someone_elses_words_or_unresolved_scope_never_apply_as_a_rule() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Lena wrote: Always use author-year citations.";
    let seal = store
        .observe_source(observation("lena", text), text)
        .await
        .unwrap();
    let results = store
        .propose_sources(
            &admission,
            HierarchyNodeId::parse("node-project").unwrap(),
            "turn-1",
            vec![SourceProposal {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                part_index: 0,
                spans: vec![SourceSpan {
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    role: SourceSpanRole::Body,
                }],
                category: ProposalCategory::Rule,
                interpretation: "Lena's citation preference.".to_string(),
                dependency: None,
                temporal: None,
                event_status: None,
                attribution: Some(ProposalAttribution::ReportedThirdParty),
                speaker_span: None,
            }],
        )
        .await
        .unwrap();
    let id = BlackboardEntryId::parse(results[0].entry_id.clone().unwrap()).unwrap();
    let before = snapshot(&store).await;
    assert!(matches!(
        store
            .promote_proposal(
                &admission,
                PromotionRequest {
                    category: PromotionCategory::Rule,
                    ..apply(&id, 1, "apply-lena")
                },
            )
            .await,
        Err(BlackboardStoreError::InvalidSource)
    ));
    assert_eq!(snapshot(&store).await, before);
}

#[tokio::test]
async fn c456_undo_retires_only_new_members_and_conflicts_on_changed_member() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let existing = BlackboardEntryId::parse("explicit-never-push").unwrap();
    store
        .create_entry_with_context(
            existing.clone(),
            rule("Never push."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            change(ChangeOperation::Saved, "explicit"),
        )
        .await
        .unwrap();
    let group = |event: &str, texts: &[&str]| {
        let text = format!("Ground rules for this project:\n- {}", texts.join("\n- "));
        (
            event.to_string(),
            text,
            texts
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>(),
        )
    };
    let mut receipts = Vec::new();
    for (event, text, units) in [
        group("first", &["Never push.", "Always test first."]),
        group(
            "second",
            &["Never run git commit.", "Use British spelling."],
        ),
    ] {
        let seal = store
            .observe_source(observation(&event, &text), &text)
            .await
            .unwrap();
        let members = units
            .iter()
            .map(|unit| {
                let start = text.find(unit.as_str()).unwrap() as u32;
                SourceCaptureMember {
                    write: CaptureEntryWrite {
                        candidates: vec![
                            BlackboardEntryId::parse(format!("auto-{event}-{start}")).unwrap(),
                        ],
                        value: rule(unit),
                        context: KnowledgeContext::new(
                            KnowledgeCategory::Rule,
                            KnowledgeAuthority::HumanDirect,
                        ),
                        change: change(ChangeOperation::Saved, unit),
                    },
                    seal: seal.clone(),
                    spans: vec![SourceSpan {
                        start_byte: start,
                        end_byte: start + unit.len() as u32,
                        role: SourceSpanRole::Body,
                    }],
                }
            })
            .collect();
        receipts.push(
            store
                .write_source_group(
                    &admission,
                    SourceCaptureGroup {
                        project_id: "project-1".to_string(),
                        action_id: format!("admit-{event}"),
                        group_id: format!("rules-{event}"),
                        members,
                        existing: ExistingWording::AcknowledgeIdentical,
                    },
                )
                .await
                .unwrap(),
        );
    }
    let first = &receipts[0];
    assert_eq!(
        (
            first.saved,
            first.already_present,
            first.members[0].outcome,
            first.members[0].entry_id.as_deref()
        ),
        (
            1,
            1,
            MemberOutcome::AlreadyPresent,
            Some("explicit-never-push")
        )
    );
    let undone = store
        .undo_capture_group(&admission, &first.group_id, "undo-first")
        .await
        .unwrap();
    assert_eq!((undone.saved, undone.already_present), (1, 1));
    let explicit = store
        .get_entry("project-1", &existing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (explicit.state, explicit.revision),
        (BlackboardEntryState::Active, 1)
    );
    let new_id = BlackboardEntryId::parse(first.members[1].entry_id.clone().unwrap()).unwrap();
    assert_eq!(
        store
            .get_entry("project-1", &new_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        BlackboardEntryState::Tombstoned
    );
    assert_eq!(
        store
            .undo_capture_group(&admission, &first.group_id, "undo-first")
            .await
            .unwrap(),
        undone
    );

    // A later change to one member conflicts the whole group; nothing is undone.
    let second = &receipts[1];
    let changed = BlackboardEntryId::parse(second.members[1].entry_id.clone().unwrap()).unwrap();
    let current = store
        .get_entry("project-1", &changed)
        .await
        .unwrap()
        .unwrap();
    let mut update = retire(&current);
    update.state = BlackboardEntryState::Active;
    update.importance = BlackboardImportance::Critical;
    store
        .update_entry("project-1", &changed, update)
        .await
        .unwrap();
    let before = snapshot(&store).await;
    assert!(matches!(
        store
            .undo_capture_group(&admission, &second.group_id, "undo-second")
            .await,
        Err(BlackboardStoreError::RevisionConflict { .. })
    ));
    assert_eq!(snapshot(&store).await, before);
}

async fn custom_proposal(
    store: &BlackboardStore,
    admission: &codex_state::ThreadProjectAdmission,
    event: &str,
    text: &str,
    category: ProposalCategory,
    end: usize,
    dependency: Option<ProposalDependency>,
) -> BlackboardEntryId {
    let seal = store
        .observe_source(observation(event, text), text)
        .await
        .unwrap();
    let results = store
        .propose_sources(
            admission,
            HierarchyNodeId::parse("node-project").unwrap(),
            "turn-1",
            vec![SourceProposal {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                part_index: 0,
                spans: vec![SourceSpan {
                    start_byte: 0,
                    end_byte: end as u32,
                    role: SourceSpanRole::Body,
                }],
                category,
                interpretation: format!("Reading {event}: SQLite because it must work offline."),
                dependency,
                temporal: None,
                event_status: None,
                attribution: None,
                speaker_span: None,
            }],
        )
        .await
        .unwrap();
    BlackboardEntryId::parse(results[0].entry_id.clone().unwrap()).unwrap()
}

#[tokio::test]
async fn c456r1_partial_citations_and_unresolved_dependencies_never_apply() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let reason = DECISION.find(", because").unwrap();
    let partial = custom_proposal(
        &store,
        &admission,
        "partial",
        DECISION,
        ProposalCategory::Decision,
        reason,
        /*dependency*/ None,
    )
    .await;
    let scoped_decision = custom_proposal(
        &store,
        &admission,
        "scoped-decision",
        "For this task only, we'll use SQLite.",
        ProposalCategory::Decision,
        "For this task only, we'll use SQLite.".len(),
        Some(ProposalDependency::Scope),
    )
    .await;
    let scoped_rejection = custom_proposal(
        &store,
        &admission,
        "scoped-rejection",
        "For this task only, it's not the cache.",
        ProposalCategory::RuledOut,
        "For this task only, it's not the cache.".len(),
        Some(ProposalDependency::Scope),
    )
    .await;
    let before = snapshot(&store).await;
    for (id, category) in [
        (&partial, PromotionCategory::Decision),
        (&scoped_decision, PromotionCategory::Decision),
        (&scoped_rejection, PromotionCategory::RuledOut),
    ] {
        assert!(matches!(
            store.proposal_application("project-1", id, category).await,
            Err(BlackboardStoreError::InvalidSource)
        ));
        assert!(matches!(
            store
                .promote_proposal(
                    &admission,
                    PromotionRequest {
                        category,
                        ..apply(id, 1, &format!("apply-{id}"))
                    },
                )
                .await,
            Err(BlackboardStoreError::InvalidSource)
        ));
    }
    assert_eq!(snapshot(&store).await, before);
    assert_eq!(root_contents(&store).await, Vec::<String>::new());
}

#[tokio::test]
async fn c456r1_apply_records_said_time_and_replays_truthfully_after_forget() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let id = proposal(
        &store,
        &admission,
        "timed",
        DECISION,
        ProposalCategory::Decision,
    )
    .await;
    let seal_time = store
        .proposal_context("project-1", &id)
        .await
        .unwrap()
        .map(|proposal| proposal.source.source_id)
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let receipt = store
        .promote_proposal(&admission, apply(&id, 1, "apply-timed"))
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(
        store
            .knowledge_context("project-1", &id)
            .await
            .unwrap()
            .unwrap()
            .payload
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let said = payload["temporal"]["sourceTime"]["unixMs"]
        .as_i64()
        .unwrap();
    let promoted = payload["promotion"]["promotedAtMs"].as_i64().unwrap();
    let observed = store
        .read_source_page(
            "project-1",
            &seal_time,
            payload["promotion"]["quoted"]["digest"].as_str().unwrap(),
            1,
            /*offset*/ 0,
        )
        .await
        .unwrap()
        .seal
        .recorded_at_ms;
    // The said time is when the user's message was observed, not when it was applied.
    assert_eq!(said, observed);
    assert!(
        promoted > said,
        "promotion {promoted} not after said {said}"
    );
    let receipt_length = receipt.members[0]
        .reason
        .as_deref()
        .map(|reason| serde_json::from_str::<serde_json::Value>(reason).unwrap()["length"].clone());
    assert_eq!(receipt_length, Some(serde_json::json!(DECISION.len())));
    let applied = store.get_entry("project-1", &id).await.unwrap().unwrap();
    store
        .update_entry("project-1", &id, retire(&applied))
        .await
        .unwrap();
    let before = snapshot(&store).await;
    for reopened in [false, true] {
        let store = if reopened {
            BlackboardStore::open(&codex_state::SqliteConfig::new_for_testing(
                codex_utils_absolute_path::test_support::PathExt::abs(home.path()),
            ))
            .await
            .unwrap()
        } else {
            store.clone()
        };
        assert!(matches!(
            store
                .promote_proposal(&admission, apply(&id, 1, "apply-timed"))
                .await,
            Err(BlackboardStoreError::EntryNotActive(_))
        ));
        assert_eq!(snapshot(&store).await, before);
    }
}
