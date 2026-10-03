use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::MemoryControlError;
use super::MemorySection;
use super::correct_entry;
use super::forget_entry;
use super::memory_section;
use crate::rule_capture::RuleSource;
use crate::rule_capture::capture_marked_rules;
use crate::rule_capture::store_user_rule;
use crate::rule_capture::user_rule_entry_id;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

const RULE: &str = "From now on, never run the whole test suite.";

fn services(state_home: &TempDir) -> ProjectIntelligenceServices {
    ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()))
}

/// Forgetting a rule retires it; replaying the message that stated it does not restore it.
#[tokio::test]
async fn a_forgotten_rule_stays_forgotten_when_its_message_is_quoted() {
    let state_home = TempDir::new().expect("state home");
    let services = services(&state_home);
    let stated_at_ms = crate::rule_capture::now_ms() - 1;
    let rule = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        RULE,
    )
    .await
    .remove(0)
    .entry;
    let store = services.blackboard().await.expect("store");
    let forgotten = forget_entry(store, "project-1", &rule.id, rule.revision)
        .await
        .expect("forget");
    let replayed = store_user_rule(
        &services,
        /*event_sink*/ None,
        "project-1",
        RuleSource {
            thread_id: "thread-1",
            turn_id: "turn-1",
            receipt_turn_id: "turn-2",
            stated_at_ms,
        },
        RULE,
        RuleStanding::Standing,
    )
    .await;
    assert_eq!(
        (
            forgotten.state,
            forgotten.value.provenance.kind,
            replayed.is_err()
        ),
        (
            BlackboardEntryState::Tombstoned,
            BlackboardProvenanceKind::User,
            true
        )
    );
}

/// A corrected rule takes its new wording's identity and stays applied; the old wording
/// keeps its authorship as history; a retry returns the same correction.
#[tokio::test]
async fn correcting_a_rule_replaces_it_and_a_retry_is_idempotent() {
    let state_home = TempDir::new().expect("state home");
    let services = services(&state_home);
    let rule = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        RULE,
    )
    .await
    .remove(0)
    .entry;
    let store = services.blackboard().await.expect("store");
    let corrected = "Never run the whole test suite; run the affected tests.";
    let first = correct_entry(store, "project-1", &rule.id, rule.revision, corrected)
        .await
        .expect("correct");
    let retry = correct_entry(store, "project-1", &rule.id, rule.revision, corrected)
        .await
        .expect("retry");
    let stale = forget_entry(store, "project-1", &rule.id, rule.revision).await;
    assert_eq!(
        (
            first.successor.id.clone(),
            memory_section(&first.successor),
            first.superseded[0].state,
            first.superseded[0].value.provenance.source_id.clone(),
            retry.successor.id == first.successor.id,
            matches!(stale, Err(MemoryControlError::Store(_))),
        ),
        (
            user_rule_entry_id("project-1", corrected, 0).expect("id"),
            MemorySection::UserRule,
            BlackboardEntryState::Superseded,
            rule.value.provenance.source_id.clone(),
            true,
            true,
        )
    );
}

/// Correcting an agent's rule makes the user's words a standing rule; correcting a decision
/// keeps it a decision, now in the user's words and without the old evidence.
#[tokio::test]
async fn corrections_of_agent_rules_and_decisions() {
    let state_home = TempDir::new().expect("state home");
    let services = services(&state_home);
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let entry = |kind, content: &str| codex_project_intelligence::NewBlackboardEntry {
        project_id: "project-1".to_string(),
        node_id: node_id.clone(),
        kind,
        content: content.to_string(),
        structured_value: None,
        confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(8_000)
            .expect("confidence"),
        verification: codex_project_intelligence::BlackboardVerification::Unverified,
        importance: codex_project_intelligence::BlackboardImportance::High,
        root_promotion: RootPromotion::Candidate,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: codex_project_intelligence::BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-1".to_string(),
        },
    };
    let id = |value: &str| BlackboardEntryId::parse(value).expect("id");
    let agent_rule = store
        .create_entry(
            id("agent-rule"),
            entry(BlackboardKind::Instruction, "Prefer tabs."),
        )
        .await
        .expect("agent rule");
    let decision = store
        .create_entry(
            id("decision"),
            entry(BlackboardKind::Decision, "Use one decimal place."),
        )
        .await
        .expect("decision");
    let rule = correct_entry(
        store,
        "project-1",
        &agent_rule.id,
        1,
        "Indent with four spaces.",
    )
    .await
    .expect("rule");
    let corrected = correct_entry(
        store,
        "project-1",
        &decision.id,
        1,
        "Use two decimal places.",
    )
    .await
    .expect("decision");
    assert_eq!(
        (
            memory_section(&agent_rule),
            memory_section(&rule.successor),
            memory_section(&corrected.successor),
            corrected.successor.value.provenance.kind,
            corrected.superseded[0].value.provenance.kind,
        ),
        (
            MemorySection::UnverifiedRule,
            MemorySection::UserRule,
            MemorySection::Decision,
            BlackboardProvenanceKind::User,
            BlackboardProvenanceKind::Agent,
        )
    );
}
