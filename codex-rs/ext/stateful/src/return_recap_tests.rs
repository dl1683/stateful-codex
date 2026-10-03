use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::RecapDecision;
use super::ReturnRecap;
use super::bounded;
use super::commit_line;
use super::decision;
use super::return_recap;
use crate::memory_controls::ControlOrigin;
use crate::memory_controls::forget_entry;
use crate::services::ProjectIntelligenceServices;

struct Seed<'a> {
    store: &'a BlackboardStore,
    node_id: HierarchyNodeId,
}

impl Seed<'_> {
    async fn add(
        &self,
        id: &str,
        (kind, provenance): (BlackboardKind, BlackboardProvenanceKind),
        content: &str,
        context: KnowledgeContext,
        verification: BlackboardVerification,
    ) {
        self.store
            .create_entry_with_context(
                BlackboardEntryId::parse(id).expect("id"),
                NewBlackboardEntry {
                    project_id: "project-1".to_string(),
                    node_id: self.node_id.clone(),
                    kind,
                    content: content.to_string(),
                    structured_value: None,
                    confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
                    verification,
                    importance: BlackboardImportance::Normal,
                    root_promotion: RootPromotion::Promoted,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: BlackboardProvenance {
                        kind: provenance,
                        source_id: "source".to_string(),
                    },
                },
                context.clone(),
                ChangeRecord {
                    operation: ChangeOperation::Saved,
                    origin: ChangeOrigin::HostCapture,
                    category: context.category,
                    action_id: None,
                    thread_id: Some("thread-1".to_string()),
                    turn_id: None,
                    group_id: None,
                    preview: content.to_string(),
                },
            )
            .await
            .expect("seed");
    }
}

/// Only current knowledge that applies here is shown: no rule that needs a check, no stale
/// decision, nothing from another investigation, no forgotten commit; the assistant's
/// conclusion is marked as reported; commits read as `<sha>: <subject>`.
#[tokio::test]
async fn the_recap_shows_only_what_applies() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let seed = Seed { store, node_id };
    let human = |category| KnowledgeContext::new(category, KnowledgeAuthority::HumanDirect);
    let observed = |category| KnowledgeContext::new(category, KnowledgeAuthority::HostObserved);
    let user_rule = (BlackboardKind::Instruction, BlackboardProvenanceKind::User);
    let commit = (BlackboardKind::Fact, BlackboardProvenanceKind::Maintenance);
    let unverified = BlackboardVerification::Unverified;
    seed.add(
        "rule-current",
        user_rule,
        "Cite \u{a7} 4.2 \u{2014} never the summary.",
        human(KnowledgeCategory::Rule),
        unverified,
    )
    .await;
    seed.add(
        "rule-check",
        user_rule,
        "Use the old schema.",
        KnowledgeContext {
            validity: KnowledgeValidity::NeedsCheck,
            ..human(KnowledgeCategory::Rule)
        },
        unverified,
    )
    .await;
    seed.add(
        "rule-elsewhere",
        user_rule,
        "Do not change code until we agree.",
        KnowledgeContext {
            scope_id: Some("other-investigation".to_string()),
            ..human(KnowledgeCategory::Rule)
        },
        unverified,
    )
    .await;
    seed.add(
        "decision-agent",
        (BlackboardKind::Decision, BlackboardProvenanceKind::Agent),
        "Use SQLite. Reason: one file per project",
        KnowledgeContext::new(
            KnowledgeCategory::Decision,
            KnowledgeAuthority::AssistantReported,
        ),
        unverified,
    )
    .await;
    seed.add(
        "decision-stale",
        (BlackboardKind::Decision, BlackboardProvenanceKind::User),
        "Use Postgres.",
        human(KnowledgeCategory::Decision),
        BlackboardVerification::Stale,
    )
    .await;
    seed.add(
        "stateful-commit-kept",
        commit,
        "Commit abcdef0123456789 (committed 2026-10-02) in C:/repo, found in the workspace history after 2026-10-02 18:00; who made it is not recorded (full message: git show abcdef0123456789): Draft \u{a7} 3 \u{2014} conclusion",
        observed(KnowledgeCategory::CommitObservation),
        unverified,
    )
    .await;
    seed.add(
        "stateful-commit-gone",
        commit,
        "Commit 9999999999 (committed 2026-10-01) in C:/repo (full message: git show 9999999999): Old work",
        observed(KnowledgeCategory::CommitObservation),
        unverified,
    )
    .await;
    let gone = store
        .get_entry(
            "project-1",
            &BlackboardEntryId::parse("stateful-commit-gone").expect("id"),
        )
        .await
        .expect("read")
        .expect("entry");
    forget_entry(
        store,
        "project-1",
        &gone.id,
        gone.revision,
        &ControlOrigin {
            thread_id: "thread-1".to_string(),
            action_id: None,
        },
    )
    .await
    .expect("forget");

    let threads = InMemoryThreadStore::default();
    let recap = return_recap(store, &threads, "project-1", "thread-1")
        .await
        .expect("recap");
    assert_eq!(
        ReturnRecap {
            as_of_ms: 0,
            ..recap
        },
        ReturnRecap {
            as_of_ms: 0,
            last_work: None,
            rules: vec!["Cite \u{a7} 4.2 \u{2014} never the summary.".to_string()],
            more_rules: 0,
            decisions: vec![RecapDecision {
                text: "Use SQLite.".to_string(),
                reason: Some("one file per project".to_string()),
                reported: true,
            }],
            more_decisions: 0,
            open_checks: Vec::new(),
            more_open_checks: 0,
            commits: vec!["abcdef01: Draft \u{a7} 3 \u{2014} conclusion".to_string()],
            more_commits: 0,
            capture_incomplete: 0,
            history_complete: true,
        }
    );
}

#[test]
fn recap_text_keeps_reasons_and_cuts_on_characters() {
    assert_eq!(
        (
            decision(
                "Months use mth. Reason: dashboards read \u{a7} 3 as minutes",
                /*reported*/ false
            ),
            commit_line("A note that is not a commit observation."),
            commit_line("Commit \u{2014}\u{2014}\u{2014} (committed yesterday): kept text"),
        ),
        (
            RecapDecision {
                text: "Months use mth.".to_string(),
                reason: Some("dashboards read \u{a7} 3 as minutes".to_string()),
                reported: false,
            },
            "A note that is not a commit observation.".to_string(),
            "Commit \u{2014}\u{2014}\u{2014} (committed yesterday): kept text".to_string(),
        )
    );
    let long = "\u{2014}".repeat(100);
    let cut = bounded(&long);
    assert_eq!(
        (
            cut.len() <= 240,
            cut.ends_with('\u{2026}'),
            cut.chars().count()
        ),
        (true, true, 80)
    );
}

/// Rules are shown in the order the user stated them (capture position), not by recency,
/// and the ones not shown are counted.
#[tokio::test]
async fn recap_rules_keep_the_stated_order() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let seed = Seed { store, node_id };
    // Stored newest-stated first, so storage order and stated order disagree.
    for position in (1..=6_u64).rev() {
        seed.add(
            &format!("rule-{position}"),
            (BlackboardKind::Instruction, BlackboardProvenanceKind::User),
            &format!("Rule {position}."),
            KnowledgeContext {
                source_sequence: Some(position),
                unit_ordinal: Some(0),
                ..KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect)
            },
            BlackboardVerification::Unverified,
        )
        .await;
    }
    let recap = return_recap(
        store,
        &InMemoryThreadStore::default(),
        "project-1",
        "thread-1",
    )
    .await
    .expect("recap");
    assert_eq!(
        (recap.rules, recap.more_rules),
        (
            (1..=5)
                .map(|position| format!("Rule {position}."))
                .collect::<Vec<_>>(),
            1
        )
    );
}
