use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::rule_group::capture_marked_rules;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

const MESSAGE: &str = "A couple of ways I like to work: please never run git commit yourself. No code changes yet. Never run migrations during this pass. End each of your replies with a line starting with 'Next:'.";

#[tokio::test]
async fn marked_rules_are_stored_once_in_the_users_words() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));

    let first = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        MESSAGE,
    )
    .await;
    let again = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-2",
        "turn-7",
        MESSAGE,
    )
    .await;

    let summarize = |captured: &[super::CapturedRule]| {
        captured
            .iter()
            .map(|rule| {
                (
                    rule.entry.value.content.clone(),
                    rule.entry.value.root_promotion,
                    rule.entry.value.provenance.kind,
                    rule.entry.value.provenance.source_id.clone(),
                    rule.standing,
                    rule.newly_stored,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = |newly_stored: bool| {
        vec![
            (
                "A couple of ways I like to work: please never run git commit yourself."
                    .to_string(),
                RootPromotion::Promoted,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Standing,
                newly_stored,
            ),
            (
                "Never run migrations during this pass.".to_string(),
                RootPromotion::Candidate,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Pending,
                newly_stored,
            ),
            (
                "End each of your replies with a line starting with 'Next:'.".to_string(),
                RootPromotion::Promoted,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Standing,
                newly_stored,
            ),
        ]
    };
    assert_eq!(
        (summarize(&first), summarize(&again)),
        (expected(true), expected(false))
    );

    // Only the standing rules reach the root, and they come first.
    let root = services
        .blackboard()
        .await
        .expect("blackboard")
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 256,
        })
        .await
        .expect("root");
    assert_eq!(
        (root.data.len(), root.candidate_entries),
        (2, 1),
        "{root:?}"
    );
}

/// horizon1: rules must stay in the packet as memory grows. User rules are selected before
/// any other promoted entry, so the root limit and byte budget cannot push them out.
#[tokio::test]
async fn user_rules_are_selected_before_more_important_knowledge() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services
        .project_node_id("project-1")
        .await
        .expect("project node");
    let store = services.blackboard().await.expect("blackboard");
    for index in 0..3 {
        store
            .create_entry(
                codex_project_intelligence::BlackboardEntryId::parse(format!("a-critical-{index}"))
                    .expect("entry ID"),
                codex_project_intelligence::NewBlackboardEntry {
                    project_id: "project-1".to_string(),
                    node_id: node_id.clone(),
                    kind: codex_project_intelligence::BlackboardKind::Fact,
                    content: format!("Critical fact {index}."),
                    structured_value: None,
                    confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(
                        9_000,
                    )
                    .expect("confidence"),
                    verification: codex_project_intelligence::BlackboardVerification::Unverified,
                    importance: codex_project_intelligence::BlackboardImportance::Critical,
                    root_promotion: RootPromotion::Promoted,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: codex_project_intelligence::BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Agent,
                        source_id: "turn-0".to_string(),
                    },
                },
            )
            .await
            .expect("fact stored");
    }
    capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        "From now on, run only the tests relevant to the change.",
    )
    .await;
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("root");
    assert_eq!(
        root.data
            .iter()
            .map(|hit| hit.entry.value.content.as_str())
            .collect::<Vec<_>>(),
        vec!["From now on, run only the tests relevant to the change."]
    );
}

/// Restating a pending rule as standing, in the same words, makes it apply.
#[tokio::test]
async fn restating_a_pending_rule_as_standing_promotes_it() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let pending = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        "My preferences for this task:\n- Never run migrations.",
    )
    .await;
    let restated = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-2",
        "My preferences for all our work:\n- Never run migrations.",
    )
    .await;
    let summary = |rules: &[super::CapturedRule]| {
        rules
            .iter()
            .map(|rule| (rule.entry.value.root_promotion, rule.newly_stored))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (summary(&pending), summary(&restated)),
        (
            vec![(RootPromotion::Candidate, true)],
            vec![(RootPromotion::Promoted, true)]
        )
    );
}

/// Restating a retired rule makes it current again.
#[tokio::test]
async fn restating_a_retired_rule_reactivates_it() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let message = "From now on, never run the whole test suite.";
    let stored = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        message,
    )
    .await
    .remove(0)
    .entry;
    let store = services.blackboard().await.expect("blackboard");
    store
        .update_entry(
            "project-1",
            &stored.id,
            codex_project_intelligence::BlackboardEntryUpdate {
                expected_revision: stored.revision,
                kind: stored.value.kind,
                content: stored.value.content.clone(),
                structured_value: None,
                confidence: stored.value.confidence,
                verification: stored.value.verification,
                importance: stored.value.importance,
                root_promotion: stored.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: codex_project_intelligence::BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: stored.value.provenance.clone(),
            },
        )
        .await
        .expect("retire");
    let restated = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-2",
        message,
    )
    .await
    .remove(0);
    assert_eq!(
        (
            restated.entry.state,
            restated.entry.value.root_promotion,
            restated.newly_stored
        ),
        (
            codex_project_intelligence::BlackboardEntryState::Active,
            RootPromotion::Promoted,
            true
        )
    );
}

/// Quoting the message that stated a rule cannot bring the rule back once it is retired;
/// a message written afterwards can.
#[tokio::test]
async fn a_retired_rule_returns_only_from_a_later_message() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let message = "From now on, never run the whole test suite.";
    let stored = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        message,
    )
    .await
    .remove(0)
    .entry;
    let stated_before = stored.created_at_ms - 1;
    let store = services.blackboard().await.expect("blackboard");
    store
        .update_entry(
            "project-1",
            &stored.id,
            codex_project_intelligence::BlackboardEntryUpdate {
                expected_revision: stored.revision,
                kind: stored.value.kind,
                content: stored.value.content.clone(),
                structured_value: None,
                confidence: stored.value.confidence,
                verification: stored.value.verification,
                importance: stored.value.importance,
                root_promotion: stored.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: codex_project_intelligence::BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: stored.value.provenance.clone(),
            },
        )
        .await
        .expect("retire");
    let replayed = super::store_user_rule(
        &services,
        /*event_sink*/ None,
        "project-1",
        super::RuleSource {
            thread_id: "thread-1",
            turn_id: "turn-1",
            receipt_turn_id: "turn-3",
            stated_at_ms: stated_before,
            after_change: Some(0),
            placement: super::RulePlacement::project(
                codex_project_intelligence::ChangeOrigin::ModelTool,
            ),
        },
        message,
        RuleStanding::Standing,
    )
    .await
    .map(|captured| captured.newly_stored);
    let restated = super::store_user_rule(
        &services,
        /*event_sink*/ None,
        "project-1",
        super::RuleSource {
            thread_id: "thread-1",
            turn_id: "turn-4",
            receipt_turn_id: "turn-4",
            stated_at_ms: super::now_ms() + 1_000,
            after_change: Some(u64::MAX),
            placement: super::RulePlacement::project(
                codex_project_intelligence::ChangeOrigin::ModelTool,
            ),
        },
        message,
        RuleStanding::Standing,
    )
    .await
    .map(|captured| captured.newly_stored);
    assert_eq!((replayed.is_err(), restated), (true, Ok(true)));
}

/// After a retirement, a pending restatement from another thread exists; quoting the message
/// that stated the retired rule cannot promote it.
#[tokio::test]
async fn an_old_quote_cannot_promote_a_pending_restatement() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let stated_before = super::now_ms() - 1;
    let rule = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-a",
        "turn-1",
        "My working preferences:\n- Never run migrations.",
    )
    .await
    .remove(0)
    .entry;
    let store = services.blackboard().await.expect("blackboard");
    crate::memory_controls::forget_entry(
        store,
        &crate::memory_controls::MemoryActor::default(),
        "project-1",
        &rule.id,
        rule.revision,
    )
    .await
    .expect("retire");
    let pending = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-b",
        "turn-2",
        "For this task:\n- Never run migrations.",
    )
    .await
    .remove(0);
    let replayed = super::store_user_rule(
        &services,
        /*event_sink*/ None,
        "project-1",
        super::RuleSource {
            thread_id: "thread-a",
            turn_id: "turn-1",
            receipt_turn_id: "turn-3",
            stated_at_ms: stated_before,
            after_change: Some(0),
            placement: super::RulePlacement::project(
                codex_project_intelligence::ChangeOrigin::ModelTool,
            ),
        },
        "Never run migrations.",
        RuleStanding::Standing,
    )
    .await;
    let current = store
        .get_entry("project-1", &pending.entry.id)
        .await
        .expect("read")
        .expect("pending entry");
    assert_eq!(
        (replayed.is_err(), current.value.root_promotion),
        (true, RootPromotion::Candidate)
    );
}

/// Background is stored once, verbatim, as promoted user-authored knowledge.
#[tokio::test]
async fn background_is_stored_once_in_the_users_words() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let text = "I know Python well but only a little Rust.";
    for turn in ["turn-1", "turn-2"] {
        super::capture_background(
            &services,
            /*event_sink*/ None,
            "project-1",
            "thread-1",
            turn,
            text,
        )
        .await;
    }
    let store = services.blackboard().await.expect("blackboard");
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 10,
        })
        .await
        .expect("root");
    assert_eq!(
        root.data
            .iter()
            .map(|hit| (
                hit.entry.value.content.clone(),
                hit.entry.value.provenance.kind,
                hit.entry.value.root_promotion
            ))
            .collect::<Vec<_>>(),
        vec![(
            text.to_string(),
            BlackboardProvenanceKind::User,
            RootPromotion::Promoted
        )]
    );
}

/// horizon3: rules are applied and listed in the order the user wrote them, not in the order
/// of their identities.
#[tokio::test]
async fn rules_keep_the_order_the_user_wrote_them_in() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let message = "Standing rules for this project, please follow them in every session:\n1. Only run the tests relevant to what you changed, never the whole suite.\n2. Never install anything into my global Python. If you need an environment, make a local venv inside the repo.\n3. Don't touch docs/ or any changelog.\n4. End every reply with one line starting with `Next:` that suggests the next step.";
    capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        message,
    )
    .await;
    let store = services.blackboard().await.expect("blackboard");
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 16,
        })
        .await
        .expect("root");
    let review = store
        .active_review_page(
            "project-1",
            /*offset*/ 0,
            /*limit*/ 16,
            /*expected_revision*/ None,
        )
        .await
        .expect("review")
        .expect("page");
    let first_words = |content: &str| content.split(' ').take(2).collect::<Vec<_>>().join(" ");
    let expected = ["Only run", "Never install", "Don't touch", "End every"]
        .map(str::to_string)
        .to_vec();
    assert_eq!(
        (
            root.data
                .iter()
                .map(|hit| first_words(&hit.entry.value.content))
                .collect::<Vec<_>>(),
            review
                .entries
                .iter()
                .map(|entry| first_words(&entry.value.content))
                .collect::<Vec<_>>(),
        ),
        (expected.clone(), expected)
    );
}
