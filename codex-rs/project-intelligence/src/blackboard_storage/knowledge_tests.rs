use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::CreateOutcome;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardVerification;
use crate::ChangeOperation;
use crate::ChangeOrigin;
use crate::ChangeRecord;
use crate::ConfidenceScore;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::KnowledgeScope;
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootPromotion;
use crate::ScopeKind;
use crate::ScopeState;
use crate::SupersededEntry;

const PROJECT_ID: &str = "project-1";

#[tokio::test]
async fn leave_then_independent_join_returns_the_actual_bound_snapshot() {
    let home = TempDir::new().expect("home");
    let first = store(&home).await;
    first
        .open_scope(&KnowledgeScope {
            project_id: PROJECT_ID.to_string(),
            scope_id: "scope-rejoin".to_string(),
            kind: ScopeKind::Investigation,
            title: "Parser".to_string(),
            state: ScopeState::Open,
            end_condition: None,
            opened_source: "user-message:thread-1/turn-1".to_string(),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .expect("open");
    first
        .bind_thread_scope(PROJECT_ID, "thread-1", "scope-rejoin")
        .await
        .expect("initial join");
    let second = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("independent store");
    first
        .unbind_thread_scope(PROJECT_ID, "thread-1")
        .await
        .expect("leave");
    second
        .bind_thread_scope(PROJECT_ID, "thread-1", "scope-rejoin")
        .await
        .expect("concurrent rejoin");
    let (bound, scopes) = first
        .thread_scopes(PROJECT_ID, "thread-1")
        .await
        .expect("returned snapshot");
    assert_eq!(bound, scopes.first().cloned());
}

fn rule(content: &str) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
        kind: BlackboardKind::Instruction,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::User,
            source_id: "user-message:thread-1/turn-1".to_string(),
        },
    }
}

fn change(operation: ChangeOperation, preview: &str) -> ChangeRecord {
    ChangeRecord {
        operation,
        origin: ChangeOrigin::HostCapture,
        category: KnowledgeCategory::Rule,
        action_id: None,
        thread_id: Some("thread-1".to_string()),
        turn_id: Some("turn-1".to_string()),
        group_id: Some("group-1".to_string()),
        preview: preview.to_string(),
    }
}

async fn store(temp_dir: &TempDir) -> BlackboardStore {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy opens")
        .create_node(
            HierarchyNodeId::parse("node-project").expect("node ID"),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node");
    BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard opens")
}

/// A save writes the entry, its context and one journal row together; the same save again
/// journals nothing; a correction keeps the context and is journaled; a forget (a
/// lifecycle-only revision) keeps the context.
#[tokio::test]
async fn context_and_journal_follow_the_entry() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let sequence = store
        .allocate_source_sequence(PROJECT_ID)
        .await
        .expect("sequence");
    let next = store
        .allocate_source_sequence(PROJECT_ID)
        .await
        .expect("sequence");
    let context = KnowledgeContext {
        scope_id: Some("scope-1".to_string()),
        end_condition: Some("until we agree".to_string()),
        source_sequence: Some(sequence),
        unit_ordinal: Some(1),
        group_id: Some("group-1".to_string()),
        ..KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect)
    };
    let id = BlackboardEntryId::parse("rule-1").expect("ID");
    let (entry, created) = store
        .create_entry_with_context(
            id.clone(),
            rule("Never commit."),
            context.clone(),
            change(ChangeOperation::Saved, "Never commit."),
        )
        .await
        .expect("created");
    let (_, again) = store
        .create_entry_with_context(
            id.clone(),
            rule("Never commit."),
            context.clone(),
            change(ChangeOperation::Saved, "Never commit."),
        )
        .await
        .expect("again");
    let successor_id = BlackboardEntryId::parse("rule-2").expect("ID");
    let succession = store
        .create_successor_recorded(
            successor_id.clone(),
            rule("Never commit or push."),
            vec![SupersededEntry {
                id: id.clone(),
                expected_revision: entry.revision,
            }],
            Some(&ChangeRecord {
                origin: ChangeOrigin::DirectControl,
                ..change(ChangeOperation::Corrected, "Never commit or push.")
            }),
        )
        .await
        .expect("successor");
    let successor = succession.successor;
    store
        .update_entry_recorded(
            PROJECT_ID,
            &successor_id,
            BlackboardEntryUpdate {
                expected_revision: successor.revision,
                kind: successor.value.kind,
                content: successor.value.content.clone(),
                structured_value: None,
                confidence: successor.value.confidence,
                verification: successor.value.verification,
                importance: successor.value.importance,
                root_promotion: successor.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: successor.value.provenance.clone(),
            },
            Some(&change(ChangeOperation::Forgotten, "Never commit or push.")),
        )
        .await
        .expect("forget");
    let journal = store
        .memory_changes(
            PROJECT_ID,
            Some("thread-1"),
            /*after*/ 0,
            /*limit*/ 10,
        )
        .await
        .expect("journal")
        .into_iter()
        .map(|change| {
            (
                change.sequence,
                change.entry_id,
                change.revision,
                change.record.operation,
                change.record.origin,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        (
            (sequence, next),
            (created, again),
            store
                .knowledge_context(PROJECT_ID, &successor_id)
                .await
                .expect("context"),
            journal,
            store
                .latest_change_sequence(PROJECT_ID)
                .await
                .expect("latest"),
        ),
        (
            (1, 2),
            (CreateOutcome::Created, CreateOutcome::AlreadyPresent),
            Some(context),
            vec![
                (
                    1,
                    Some("rule-1".to_string()),
                    Some(1),
                    ChangeOperation::Saved,
                    ChangeOrigin::HostCapture
                ),
                (
                    2,
                    Some("rule-2".to_string()),
                    Some(1),
                    ChangeOperation::Corrected,
                    ChangeOrigin::DirectControl
                ),
                (
                    3,
                    Some("rule-2".to_string()),
                    Some(2),
                    ChangeOperation::Forgotten,
                    ChangeOrigin::HostCapture
                ),
            ],
            3,
        )
    );
}

/// A scope opens once, binds threads, and ends once with a journal row.
#[tokio::test]
async fn scopes_open_bind_and_end_once() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let opened = store
        .open_scope(&KnowledgeScope {
            project_id: PROJECT_ID.to_string(),
            scope_id: "scope-1".to_string(),
            kind: ScopeKind::Investigation,
            title: "Ground rules for this whole investigation".to_string(),
            state: ScopeState::Open,
            end_condition: Some("until we agree".to_string()),
            opened_source: "user-message:thread-1/turn-1".to_string(),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .expect("open");
    store
        .bind_thread_scope(PROJECT_ID, "thread-2", "scope-1")
        .await
        .expect("bind");
    let bound = store
        .thread_scope(PROJECT_ID, "thread-2")
        .await
        .expect("bound")
        .map(|scope| scope.scope_id);
    let ended = store
        .end_scope(
            PROJECT_ID,
            "scope-1",
            "user-message:thread-2/turn-9",
            &change(ChangeOperation::ScopeEnded, "We agree."),
        )
        .await
        .expect("end");
    let ended_again = store
        .end_scope(
            PROJECT_ID,
            "scope-1",
            "user-message:thread-2/turn-10",
            &change(ChangeOperation::ScopeEnded, "We agree."),
        )
        .await
        .expect("end again");
    let open = store
        .scopes(PROJECT_ID, Some(ScopeState::Open))
        .await
        .expect("open scopes");
    assert_eq!(
        (
            (opened.state, opened.kind),
            bound,
            (ended, ended_again),
            open.len(),
            store
                .latest_change_sequence(PROJECT_ID)
                .await
                .expect("latest"),
        ),
        (
            (ScopeState::Open, ScopeKind::Investigation),
            Some("scope-1".to_string()),
            (true, false),
            0,
            2,
        )
    );
    assert!(
        store
            .bind_thread_scope(PROJECT_ID, "late-thread", "scope-1")
            .await
            .is_err()
    );
    assert_eq!(
        store
            .thread_scope(PROJECT_ID, "late-thread")
            .await
            .expect("binding"),
        None
    );
    let (bound, snapshot) = store
        .thread_scopes(PROJECT_ID, "thread-2")
        .await
        .expect("snapshot");
    assert_eq!(bound, snapshot.into_iter().next());
    assert_eq!(
        store
            .latest_change_sequence(PROJECT_ID)
            .await
            .expect("sequence"),
        2
    );
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(temp_dir.path().abs()))
        .await
        .expect("reopen");
    assert!(
        reopened
            .bind_thread_scope(PROJECT_ID, "late-thread", "scope-1")
            .await
            .is_err()
    );
    assert_eq!(
        reopened
            .thread_scope(PROJECT_ID, "late-thread")
            .await
            .expect("binding"),
        None
    );
    assert_eq!(
        reopened
            .latest_change_sequence(PROJECT_ID)
            .await
            .expect("sequence"),
        2
    );
}

/// Foundation: one user action binds one journaled change; a second writer of the same action
/// commits nothing (its entry is rolled back with it).
#[tokio::test]
async fn an_action_binds_one_change() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = store(&temp_dir).await;
    let with_action = |preview: &str| ChangeRecord {
        action_id: Some("action-1".to_string()),
        ..change(ChangeOperation::Saved, preview)
    };
    let first = store
        .create_entry_with_context(
            BlackboardEntryId::parse("entry-1").expect("id"),
            rule("Never push."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            with_action("Never push."),
        )
        .await
        .map(|(_, outcome)| outcome);
    let second = store
        .create_entry_with_context(
            BlackboardEntryId::parse("entry-2").expect("id"),
            rule("Never commit."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            with_action("Never commit."),
        )
        .await
        .map(|(_, outcome)| outcome)
        .map_err(|error| error.to_string());
    let second_entry = store
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse("entry-2").expect("id"),
        )
        .await
        .expect("read");
    assert_eq!(
        (first.ok(), second, second_entry.is_none()),
        (
            Some(CreateOutcome::Created),
            Err("this user action was already recorded: action-1".to_string()),
            true
        )
    );
}

fn capture_request() -> crate::CaptureWrite {
    let scope = KnowledgeScope {
        project_id: PROJECT_ID.to_string(),
        scope_id: "scope-atomic".to_string(),
        kind: ScopeKind::Investigation,
        title: "Atomic investigation".to_string(),
        state: ScopeState::Open,
        end_condition: None,
        opened_source: "user-message:thread-1/turn-1".to_string(),
        ended_source: None,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    crate::CaptureWrite {
        project_id: PROJECT_ID.to_string(),
        scope: Some(scope),
        group: Some(crate::CaptureGroup {
            project_id: PROJECT_ID.to_string(),
            group_id: "group-atomic".to_string(),
            thread_id: Some("thread-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            kind: "rules".to_string(),
            ..Default::default()
        }),
        units: ["Never push.", "Preserve whole reasons: §3.2–§4 — α."]
            .into_iter()
            .enumerate()
            .map(|(ordinal, words)| {
                crate::CaptureUnitWrite::Entry(Box::new(crate::CaptureEntryWrite {
                    candidates: vec![
                        BlackboardEntryId::parse(format!("atomic-{ordinal}")).expect("id"),
                    ],
                    value: rule(words),
                    context: KnowledgeContext {
                        scope_id: Some("scope-atomic".to_string()),
                        group_id: Some("group-atomic".to_string()),
                        ..KnowledgeContext::new(
                            KnowledgeCategory::Rule,
                            KnowledgeAuthority::HumanDirect,
                        )
                    },
                    change: change(ChangeOperation::Saved, words),
                    authority: crate::CaptureAuthority::Message,
                }))
            })
            .collect(),
    }
}

#[tokio::test]
async fn capture_rolls_back_at_group_and_member_commit_then_cold_retries() {
    for (table, trigger) in [
        ("capture_groups", "fail_group"),
        ("capture_group_members", "fail_member"),
    ] {
        let home = TempDir::new().expect("home");
        let store = store(&home).await;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {trigger} BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'injected failure'); END")))
            .execute(&store.pool).await.expect("install fault");
        assert!(store.write_capture(capture_request()).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blackboard_entries")
            .fetch_one(&store.pool)
            .await
            .expect("entries");
        assert_eq!(
            (
                count,
                store
                    .latest_change_sequence(PROJECT_ID)
                    .await
                    .expect("journal"),
                store
                    .scope(PROJECT_ID, "scope-atomic")
                    .await
                    .expect("scope"),
                store
                    .capture_group(PROJECT_ID, "group-atomic")
                    .await
                    .expect("group")
            ),
            (0, 0, None, None)
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TRIGGER {trigger}")))
            .execute(&store.pool)
            .await
            .expect("remove fault");
        store.pool.close().await;
        let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .expect("reopen");
        let committed = reopened
            .write_capture(capture_request())
            .await
            .expect("retry");
        let group = committed.group.expect("committed group");
        assert_eq!(
            (group.recognized, group.saved, group.members.len()),
            (2, 2, 2)
        );
        assert_eq!(
            reopened
                .write_capture(capture_request())
                .await
                .expect("replay")
                .group,
            Some(group)
        );
        assert_eq!(
            reopened
                .latest_change_sequence(PROJECT_ID)
                .await
                .expect("journal"),
            3
        );
    }
}

#[tokio::test]
async fn direct_noop_outcome_rolls_back_with_its_action_binding() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    let request = |action: &str| crate::CaptureWrite {
        project_id: PROJECT_ID.to_string(),
        group: None,
        scope: None,
        units: vec![crate::CaptureUnitWrite::Entry(Box::new(
            crate::CaptureEntryWrite {
                candidates: vec![BlackboardEntryId::parse("direct-atomic").expect("id")],
                value: rule("Never push."),
                context: KnowledgeContext::new(
                    KnowledgeCategory::Rule,
                    KnowledgeAuthority::HumanDirect,
                ),
                change: ChangeRecord {
                    action_id: Some(action.to_string()),
                    group_id: Some("exact-request".to_string()),
                    origin: ChangeOrigin::DirectControl,
                    ..change(ChangeOperation::Saved, "Never push.")
                },
                authority: crate::CaptureAuthority::DirectAction,
            },
        ))],
    };
    store.write_capture(request("first")).await.expect("first");
    sqlx::query("CREATE TRIGGER fail_outcome BEFORE INSERT ON capture_action_outcomes BEGIN SELECT RAISE(ABORT, 'injected outcome failure'); END")
        .execute(&store.pool).await.expect("fault");
    assert!(store.write_capture(request("noop")).await.is_err());
    assert_eq!(
        store
            .change_for_action(PROJECT_ID, "noop")
            .await
            .expect("journal"),
        None
    );
    sqlx::query("DROP TRIGGER fail_outcome")
        .execute(&store.pool)
        .await
        .expect("remove fault");
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("reopen");
    let retry = reopened
        .write_capture(request("noop"))
        .await
        .expect("retry");
    assert_eq!(retry.entries[0].1, crate::MemberOutcome::AlreadyPresent);
    assert!(matches!(
        reopened.write_capture(request("noop")).await,
        Err(super::BlackboardStoreError::ActionAlreadyRecorded(_))
    ));
    assert_eq!(
        reopened
            .latest_change_sequence(PROJECT_ID)
            .await
            .expect("sequence"),
        2
    );
}

#[tokio::test]
async fn legacy_scope_quarantine_precedes_root_limit_and_snapshot_keeps_context() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    for ordinal in 0..257 {
        store
            .create_entry(
                BlackboardEntryId::parse(format!("legacy-{ordinal}")).expect("id"),
                rule("Never change code during this investigation."),
            )
            .await
            .expect("legacy");
    }
    let id = BlackboardEntryId::parse("applicable-rule").expect("id");
    let (_, _) = store
        .create_entry_with_context(
            id.clone(),
            rule("Always preserve reasons."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            change(ChangeOperation::Saved, "Always preserve reasons."),
        )
        .await
        .expect("rule");
    let (root, scopes) = store
        .root_projection_for_thread(
            crate::RootBlackboardQuery {
                project_id: PROJECT_ID.to_string(),
                max_entries: 1,
            },
            "thread-1",
        )
        .await
        .expect("snapshot");
    assert_eq!(
        (
            root.data
                .into_iter()
                .map(|hit| hit.entry.id)
                .collect::<Vec<_>>(),
            root.omitted_entries,
            scopes.legacy_held_back,
            root.contexts.get(id.as_str()).cloned()
        ),
        (
            vec![id],
            0,
            257,
            Some(KnowledgeContext {
                source_sequence: Some(1),
                ..KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect)
            })
        )
    );
}
