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
            &restored
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

#[tokio::test]
async fn noop_action_survives_forget_and_store_restart() {
    let home = TempDir::new().expect("home");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let id;
    {
        let services = ProjectIntelligenceServices::new(sqlite.clone());
        let node = services.project_node_id("project-1").await.expect("node");
        let store = services.blackboard().await.expect("store");
        let actor = |action: &str| crate::memory_controls::MemoryActor {
            action_id: Some(action.to_string()),
            ..Default::default()
        };
        let (entry, first) = add_entry(
            store,
            &actor("original"),
            "project-1",
            node.clone(),
            MemoryAddition::Rule { scope: None },
            "Never push.",
        )
        .await
        .expect("first");
        id = entry.id.clone();
        let (_, noop) = add_entry(
            store,
            &actor("noop"),
            "project-1",
            node,
            MemoryAddition::Rule { scope: None },
            "Never push.",
        )
        .await
        .expect("noop");
        assert_eq!(
            (first, noop),
            (AddOutcome::Added, AddOutcome::AlreadyPresent)
        );
        forget_entry(store, &actor("forget"), "project-1", &id, entry.revision)
            .await
            .expect("forget");
    }
    let services = ProjectIntelligenceServices::new(sqlite);
    let store = services.blackboard().await.expect("reopened");
    let actor = crate::memory_controls::MemoryActor {
        action_id: Some("noop".to_string()),
        ..Default::default()
    };
    let (entry, outcome) = add_entry(
        store,
        &actor,
        "project-1",
        services.project_node_id("project-1").await.expect("node"),
        MemoryAddition::Rule { scope: None },
        "Never push.",
    )
    .await
    .expect("retry");
    assert_eq!(
        (entry.id, entry.state, outcome),
        (
            id,
            codex_project_intelligence::BlackboardEntryState::Tombstoned,
            AddOutcome::AlreadyDone
        )
    );
}

#[tokio::test]
async fn direct_rules_and_corrections_keep_insertion_order_after_restart() {
    let home = TempDir::new().expect("home");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let services = ProjectIntelligenceServices::new(sqlite.clone());
    let node = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let actor = |action: &str| crate::memory_controls::MemoryActor {
        action_id: Some(action.to_string()),
        ..Default::default()
    };
    let (first, _) = add_entry(
        store,
        &actor("first"),
        "project-1",
        node.clone(),
        MemoryAddition::Rule { scope: None },
        "Always preserve reasons.",
    )
    .await
    .expect("first");
    add_entry(
        store,
        &actor("second"),
        "project-1",
        node,
        MemoryAddition::Rule { scope: None },
        "Never push.",
    )
    .await
    .expect("second");
    crate::memory_controls::correct_entry(
        store,
        &actor("correction"),
        "project-1",
        &first.id,
        first.revision,
        "Always preserve complete reasons.",
    )
    .await
    .expect("correct");
    let reopened = codex_project_intelligence::BlackboardStore::open(&sqlite)
        .await
        .expect("reopen");
    let root = reopened
        .root_projection(codex_project_intelligence::RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("root");
    assert_eq!(
        (
            root.data
                .into_iter()
                .map(|hit| hit.entry.value.content)
                .collect::<Vec<_>>(),
            root.omitted_entries
        ),
        (vec!["Always preserve complete reasons.".to_string()], 1)
    );
    let root = reopened
        .root_projection(codex_project_intelligence::RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 10,
        })
        .await
        .expect("root");
    assert_eq!(
        root.data
            .into_iter()
            .map(|hit| hit.entry.value.content)
            .collect::<Vec<_>>(),
        vec!["Always preserve complete reasons.", "Never push."]
    );
}

/// Separate clients sharing the durable store agree on matching retries, and a losing
/// mismatched request cannot leave an entry, context, outcome, or journal change.
#[tokio::test]
async fn concurrent_clients_replay_only_the_matching_full_request() {
    let state_home = TempDir::new().expect("home");
    let sqlite = SqliteConfig::new_for_testing(state_home.path().abs());
    let left = ProjectIntelligenceServices::new(sqlite.clone());
    let right = ProjectIntelligenceServices::new(sqlite.clone());
    let node = left.project_node_id("project-1").await.expect("node");
    let a = left.blackboard().await.expect("left");
    let b = right.blackboard().await.expect("right");
    for (index, other_words) in ["Matching words", "Conflicting words"]
        .into_iter()
        .enumerate()
    {
        let actor = crate::memory_controls::MemoryActor {
            thread_id: Some("thread-1".to_string()),
            action_id: Some(format!("concurrent-{index}")),
        };
        let words = if index == 0 {
            "Matching words"
        } else {
            "Winning or losing words"
        };
        let (first, second) = tokio::join!(
            add_entry(
                a,
                &actor,
                "project-1",
                node.clone(),
                MemoryAddition::Rule { scope: None },
                words
            ),
            add_entry(
                b,
                &actor,
                "project-1",
                node.clone(),
                MemoryAddition::Rule { scope: None },
                other_words
            ),
        );
        if index == 0 {
            let (first, first_outcome) = first.expect("first");
            let (second, second_outcome) = second.expect("second");
            assert_eq!(first, second);
            assert!(matches!(
                (first_outcome, second_outcome),
                (AddOutcome::Added, AddOutcome::AlreadyDone)
                    | (AddOutcome::AlreadyDone, AddOutcome::Added)
            ));
        } else {
            assert!(matches!(
                (first, second),
                (
                    Ok(_),
                    Err(crate::memory_controls::MemoryControlError::Refused(_))
                ) | (
                    Err(crate::memory_controls::MemoryControlError::Refused(_)),
                    Ok(_)
                )
            ));
        }
        assert_eq!(
            a.latest_change_sequence("project-1")
                .await
                .expect("sequence"),
            index as u64 + 1
        );
        let changes = a
            .memory_changes(
                "project-1",
                /*thread_id*/ None,
                /*after*/ 0,
                /*limit*/ 10,
            )
            .await
            .expect("changes");
        assert_eq!(changes.len(), index + 1);
        let winner_ids = changes
            .into_iter()
            .map(|change| change.entry_id.expect("entry"))
            .collect::<std::collections::BTreeSet<_>>();
        let reopened = codex_project_intelligence::BlackboardStore::open(&sqlite)
            .await
            .expect("reopen");
        for store in [a, b, &reopened] {
            let root = store
                .root_projection(codex_project_intelligence::RootBlackboardQuery {
                    project_id: "project-1".to_string(),
                    max_entries: 10,
                })
                .await
                .expect("root");
            assert_eq!(
                root.data
                    .into_iter()
                    .map(|hit| hit.entry.id.to_string())
                    .collect::<std::collections::BTreeSet<_>>(),
                winner_ids
            );
            for words in [words, other_words] {
                let id = crate::rule_identity::user_rule_entry_id(
                    "project-1",
                    /*scope_id*/ None,
                    words,
                    /*generation*/ 0,
                )
                .expect("id");
                let expected = winner_ids.contains(id.as_str());
                assert_eq!(
                    (
                        store
                            .get_entry("project-1", &id)
                            .await
                            .expect("entry")
                            .is_some(),
                        store
                            .knowledge_context("project-1", &id)
                            .await
                            .expect("context")
                            .is_some()
                    ),
                    (expected, expected)
                );
            }
        }
    }
}
