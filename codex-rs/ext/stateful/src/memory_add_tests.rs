use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::AddOutcome;
use super::MemoryAddition;
use super::add_entry;
use crate::memory_controls::MemorySection;
use crate::memory_controls::forget_entry;
use crate::memory_controls::memory_section;
use crate::services::ProjectIntelligenceServices;

/// Each kind lands in its review section with the user's authority, a repeated rule is found
/// rather than stored twice, a decision keeps its reason, and a forgotten rule added again
/// by the user comes back.
#[tokio::test]
async fn the_user_adds_each_kind_directly() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let add = |addition: MemoryAddition, content: &'static str, action: &'static str| {
        let node_id = node_id.clone();
        async move {
            add_entry(
                store,
                &crate::memory_controls::MemoryActor {
                    thread_id: None,
                    action_id: Some(action.to_string()),
                },
                "project-1",
                node_id,
                addition,
                content,
            )
            .await
            .expect("added")
        }
    };
    let rule = add(
        MemoryAddition::Rule { scope: None },
        "Always end with a Next: line.",
        "a1",
    )
    .await;
    let repeated = add(
        MemoryAddition::Rule { scope: None },
        "Always end with a Next: line.",
        "a2",
    )
    .await;
    let background = add(MemoryAddition::Background, "I'm a Go developer.", "a3").await;
    let decision = add(
        MemoryAddition::Decision {
            reason: Some("dashboards read it as minutes".to_string()),
        },
        "Months use mth.",
        "a4",
    )
    .await;
    let retried = add(
        MemoryAddition::Decision {
            reason: Some("dashboards read it as minutes".to_string()),
        },
        "Months use mth.",
        "a4",
    )
    .await;
    // The same action identity for other words is refused, never applied.
    let reused = add_entry(
        store,
        &crate::memory_controls::MemoryActor {
            thread_id: None,
            action_id: Some("a4".to_string()),
        },
        "project-1",
        node_id.clone(),
        MemoryAddition::Note,
        "Something else.",
    )
    .await;
    assert!(matches!(
        reused,
        Err(crate::memory_controls::MemoryControlError::Refused(_))
    ));
    let note = add(MemoryAddition::Note, "The CI runs on Windows.", "a5").await;
    forget_entry(
        store,
        &crate::memory_controls::MemoryActor::default(),
        "project-1",
        &rule.0.id,
        rule.0.revision,
    )
    .await
    .expect("forget");
    // A retry of the first action after the forget returns what it made, without restoring
    // it; a new action restores the words.
    let retry = add(
        MemoryAddition::Rule { scope: None },
        "Always end with a Next: line.",
        "a1",
    )
    .await;
    let restored = add(
        MemoryAddition::Rule { scope: None },
        "Always end with a Next: line.",
        "a6",
    )
    .await;
    // A scoped rule needs an open investigation in the thread; it then applies only there.
    let scoped_rule = |thread_id: &'static str, action: &'static str| {
        let node_id = node_id.clone();
        async move {
            add_entry(
                store,
                &crate::memory_controls::MemoryActor {
                    thread_id: Some(thread_id.to_string()),
                    action_id: Some(action.to_string()),
                },
                "project-1",
                node_id,
                MemoryAddition::Rule {
                    scope: Some("this whole investigation".to_string()),
                },
                "Do not change any code.",
            )
            .await
        }
    };
    let unbound = scoped_rule("thread-2", "a7").await;
    assert!(matches!(
        unbound,
        Err(crate::memory_controls::MemoryControlError::Refused(_))
    ));
    store
        .open_scope(&codex_project_intelligence::KnowledgeScope {
            project_id: "project-1".to_string(),
            scope_id: "scope-1".to_string(),
            kind: codex_project_intelligence::ScopeKind::Investigation,
            title: "the config bug".to_string(),
            state: codex_project_intelligence::ScopeState::Open,
            end_condition: None,
            opened_source: "test".to_string(),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .expect("scope");
    store
        .bind_thread_scope("project-1", "thread-1", "scope-1")
        .await
        .expect("bind");
    let scoped = scoped_rule("thread-1", "a8").await.expect("scoped");
    let scope_of = store
        .knowledge_context("project-1", &scoped.0.id)
        .await
        .expect("context")
        .and_then(|context| context.scope_id);
    assert_eq!(scope_of, Some("scope-1".to_string()));
    let summary = |(entry, outcome): &(codex_project_intelligence::BlackboardEntry, AddOutcome)| {
        (
            memory_section(entry),
            entry.value.kind,
            entry.value.content.clone(),
            entry.value.provenance.kind,
            entry.value.root_promotion,
            *outcome,
        )
    };
    let user = BlackboardProvenanceKind::User;
    let promoted = RootPromotion::Promoted;
    assert_eq!(
        [
            &rule,
            &repeated,
            &background,
            &decision,
            &retried,
            &note,
            &restored,
            &scoped
        ]
        .map(summary),
        [
            (
                MemorySection::UserRule,
                BlackboardKind::Instruction,
                "Always end with a Next: line.".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
            (
                MemorySection::UserRule,
                BlackboardKind::Instruction,
                "Always end with a Next: line.".to_string(),
                user,
                promoted,
                AddOutcome::AlreadyPresent
            ),
            (
                MemorySection::Background,
                BlackboardKind::Fact,
                "I'm a Go developer.".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
            (
                MemorySection::Decision,
                BlackboardKind::Decision,
                "Months use mth. Reason: dashboards read it as minutes".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
            (
                MemorySection::Decision,
                BlackboardKind::Decision,
                "Months use mth. Reason: dashboards read it as minutes".to_string(),
                user,
                promoted,
                AddOutcome::AlreadyDone
            ),
            (
                MemorySection::Knowledge,
                BlackboardKind::Fact,
                "The CI runs on Windows.".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
            (
                MemorySection::UserRule,
                BlackboardKind::Instruction,
                "Always end with a Next: line.".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
            (
                MemorySection::UserRule,
                BlackboardKind::Instruction,
                "Do not change any code.".to_string(),
                user,
                promoted,
                AddOutcome::Added
            ),
        ]
    );
    assert_eq!(
        (
            retry.0.id.clone(),
            retry.0.state,
            retry.1,
            restored.0.id == rule.0.id
        ),
        (
            rule.0.id.clone(),
            codex_project_intelligence::BlackboardEntryState::Tombstoned,
            AddOutcome::AlreadyDone,
            false
        )
    );
}

/// A direct rule makes the same words apply when they were kept as a task-limited rule.
#[tokio::test]
async fn a_direct_rule_promotes_a_kept_task_limited_rule() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let pending = crate::rule_group::capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        "Never run migrations during this pass.",
    )
    .await
    .remove(0)
    .entry;
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let (added, outcome) = add_entry(
        store,
        &crate::memory_controls::MemoryActor {
            thread_id: None,
            action_id: Some("action-1".to_string()),
        },
        "project-1",
        node_id,
        MemoryAddition::Rule { scope: None },
        "Never run migrations during this pass.",
    )
    .await
    .expect("added");
    assert_eq!(
        (added.id == pending.id, memory_section(&added), outcome),
        (true, MemorySection::UserRule, AddOutcome::Added)
    );
}

/// Item 3 review 2: an action is bound to its complete request (a decision with a reason is
/// not the same request as one whose words contain "Reason:"), and a scoped rule must name
/// the investigation the thread continues and carry no ending of its own.
#[tokio::test]
async fn an_action_is_bound_to_its_whole_request_and_scope() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let actor = |action: &str| crate::memory_controls::MemoryActor {
        thread_id: Some("thread-1".to_string()),
        action_id: Some(action.to_string()),
    };
    let refused = |result: Result<_, crate::memory_controls::MemoryControlError>| {
        matches!(
            result,
            Err(crate::memory_controls::MemoryControlError::Refused(_))
        )
    };
    add_entry(
        store,
        &actor("a1"),
        "project-1",
        node_id.clone(),
        MemoryAddition::Decision {
            reason: Some("Y".to_string()),
        },
        "X",
    )
    .await
    .expect("decision");
    let same_text_other_request = add_entry(
        store,
        &actor("a1"),
        "project-1",
        node_id.clone(),
        MemoryAddition::Decision { reason: None },
        "X Reason: Y",
    )
    .await;
    store
        .open_scope(&codex_project_intelligence::KnowledgeScope {
            project_id: "project-1".to_string(),
            scope_id: "scope-a".to_string(),
            kind: codex_project_intelligence::ScopeKind::Investigation,
            title: "Ground rules for the parser bug".to_string(),
            state: codex_project_intelligence::ScopeState::Open,
            end_condition: None,
            opened_source: "test".to_string(),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .expect("scope");
    store
        .bind_thread_scope("project-1", "thread-1", "scope-a")
        .await
        .expect("bind");
    let scoped = |action: &'static str, scope: &'static str| {
        let node_id = node_id.clone();
        let actor = actor(action);
        async move {
            add_entry(
                store,
                &actor,
                "project-1",
                node_id,
                MemoryAddition::Rule {
                    scope: Some(scope.to_string()),
                },
                "Never push.",
            )
            .await
        }
    };
    // A retry of an addition that found the words already saved (spaced differently)
    // replays that outcome.
    let spaced = |action: &'static str, content: &'static str| {
        let node_id = node_id.clone();
        let actor = crate::memory_controls::MemoryActor {
            thread_id: None,
            action_id: Some(action.to_string()),
        };
        async move {
            add_entry(
                store,
                &actor,
                "project-1",
                node_id,
                MemoryAddition::Rule { scope: None },
                content,
            )
            .await
            .map(|(_, outcome)| outcome)
            .ok()
        }
    };
    let first_saved = spaced("n1", "Always run the linter.").await;
    let first_spaced = spaced("n2", "Always run  the linter.").await;
    let retried_spaced = spaced("n2", "Always run  the linter.").await;
    let other_investigation = scoped("a2", "the cache investigation").await;
    let with_ending = scoped("a3", "this investigation, until we agree").await;
    let by_title = scoped("a4", "Ground rules for the parser bug").await;
    let by_id = scoped("a5", "scope-a").await;
    assert_eq!(
        (first_saved, first_spaced, retried_spaced),
        (
            Some(AddOutcome::Added),
            Some(AddOutcome::AlreadyPresent),
            Some(AddOutcome::AlreadyDone)
        )
    );
    assert_eq!(
        (
            refused(same_text_other_request),
            refused(other_investigation),
            refused(with_ending),
            by_title.map(|(_, outcome)| outcome).ok(),
            by_id.map(|(_, outcome)| outcome).ok(),
        ),
        (
            true,
            true,
            true,
            Some(AddOutcome::Added),
            Some(AddOutcome::AlreadyPresent)
        )
    );
}
