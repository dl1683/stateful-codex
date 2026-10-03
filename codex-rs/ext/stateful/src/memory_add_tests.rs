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
    let scoped = add(
        MemoryAddition::Rule {
            scope: Some("For this whole investigation, until we agree on the cause".to_string()),
        },
        "Do not change any code.",
        "a7",
    )
    .await;
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
        [&rule, &repeated, &background, &decision, &retried, &note, &restored, &scoped]
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
                "For this whole investigation, until we agree on the cause: Do not change any code."
                    .to_string(),
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
