use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardVerification;
use crate::CandidateLifecycle;
use crate::CaptureGroup;
use crate::CaptureMember;
use crate::CaptureSource;
use crate::CaptureUnit;
use crate::ChangeOperation;
use crate::ChangeOrigin;
use crate::ChangeRecord;
use crate::ConfidenceScore;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::MemberOutcome;
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootPromotion;

const PROJECT_ID: &str = "project-1";
const GROUP_ID: &str = "ruled-out-1";

fn value(kind: BlackboardKind, content: &str) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
        kind,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(7_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::NotPromoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "assistant-answer:thread-1/turn-1/item-1".to_string(),
        },
    }
}

fn context(ordinal: u32) -> KnowledgeContext {
    KnowledgeContext {
        unit_ordinal: Some(ordinal),
        group_id: Some(GROUP_ID.to_string()),
        ..KnowledgeContext::new(
            KnowledgeCategory::RuledOut,
            KnowledgeAuthority::AssistantReported,
        )
    }
}

fn unit(ordinal: u32, content: &str) -> CaptureUnit {
    CaptureUnit::Entry {
        id: BlackboardEntryId::parse(format!("ruled-out-{content}")).expect("ID"),
        retired_identities: vec![
            BlackboardEntryId::parse(format!("elsewhere-{content}")).expect("ID"),
        ],
        value: Box::new(value(BlackboardKind::RejectedApproach, content)),
        context: context(ordinal),
        change: ChangeRecord {
            operation: ChangeOperation::Saved,
            origin: ChangeOrigin::HostCapture,
            category: KnowledgeCategory::RuledOut,
            action_id: None,
            thread_id: Some("thread-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            group_id: Some(GROUP_ID.to_string()),
            preview: content.to_string(),
        },
    }
}

fn group() -> CaptureGroup {
    CaptureGroup {
        project_id: PROJECT_ID.to_string(),
        group_id: GROUP_ID.to_string(),
        thread_id: Some("thread-1".to_string()),
        turn_id: Some("turn-1".to_string()),
        kind: "ruledOut".to_string(),
        ..CaptureGroup::default()
    }
}

fn source(digest: &str) -> CaptureSource {
    CaptureSource {
        locator: "assistant-answer:thread-1/turn-1/item-1".to_string(),
        digest: digest.to_string(),
    }
}

fn member(ordinal: u32, entry_id: Option<&str>, outcome: MemberOutcome) -> CaptureMember {
    CaptureMember {
        ordinal,
        entry_id: entry_id.map(str::to_string),
        outcome,
        note: None,
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

async fn forget(store: &BlackboardStore, entry: &BlackboardEntry) {
    store
        .update_entry(
            PROJECT_ID,
            &entry.id,
            BlackboardEntryUpdate {
                expected_revision: entry.revision,
                kind: entry.value.kind,
                content: entry.value.content.clone(),
                structured_value: None,
                confidence: entry.value.confidence,
                verification: entry.value.verification,
                importance: entry.value.importance,
                root_promotion: entry.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: entry.value.provenance.clone(),
            },
        )
        .await
        .expect("forget");
}

/// Every unit is judged in one transaction: new words are saved with their context and a
/// journal row, words already saved are listed, forgotten words stay forgotten, a model
/// entry of the same words gets the context, and an overlength unit is listed as omitted.
/// The same source committed again changes nothing.
#[tokio::test]
async fn a_capture_commits_whole_and_replays_unchanged() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let earlier = store
        .create_entry(
            BlackboardEntryId::parse("ruled-out-b").expect("ID"),
            value(BlackboardKind::RejectedApproach, "b"),
        )
        .await
        .expect("earlier");
    let forgotten = store
        .create_entry(
            BlackboardEntryId::parse("ruled-out-c").expect("ID"),
            value(BlackboardKind::RejectedApproach, "c"),
        )
        .await
        .expect("forgotten");
    forget(&store, &forgotten).await;
    let elsewhere = store
        .create_entry(
            BlackboardEntryId::parse("elsewhere-e").expect("ID"),
            value(BlackboardKind::RejectedApproach, "e"),
        )
        .await
        .expect("elsewhere");
    forget(&store, &elsewhere).await;
    let model_write = store
        .create_entry(
            BlackboardEntryId::parse("model-write").expect("ID"),
            value(BlackboardKind::RejectedApproach, "d"),
        )
        .await
        .expect("model write");
    let units = vec![
        unit(0, "a"),
        unit(1, "b"),
        unit(2, "c"),
        CaptureUnit::Existing {
            id: model_write.id.clone(),
            revision: model_write.revision,
            context: context(3),
        },
        CaptureUnit::Omitted {
            note: "too long: xxxx".to_string(),
        },
        unit(5, "e"),
    ];
    let (committed, entries) = store
        .commit_capture(&group(), &source("digest-1"), units.clone())
        .await
        .expect("commit");
    assert_eq!(
        (
            committed.group.clone(),
            committed.members.clone(),
            committed.newly_committed
        ),
        (
            CaptureGroup {
                recognized: 6,
                saved: 1,
                already_present: 2,
                omitted: 3,
                ..group()
            },
            vec![
                member(0, Some("ruled-out-a"), MemberOutcome::Saved),
                member(1, Some("ruled-out-b"), MemberOutcome::AlreadyPresent),
                CaptureMember {
                    note: Some("c".to_string()),
                    ..member(2, None, MemberOutcome::NotRestored)
                },
                member(3, Some("model-write"), MemberOutcome::AlreadyPresent),
                CaptureMember {
                    note: Some("too long: xxxx".to_string()),
                    ..member(4, None, MemberOutcome::Omitted)
                },
                CaptureMember {
                    note: Some("e".to_string()),
                    ..member(5, None, MemberOutcome::NotRestored)
                },
            ],
            true,
        )
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>(),
        vec!["ruled-out-a", "ruled-out-b", "model-write"]
    );
    let contexts = store
        .knowledge_contexts(
            PROJECT_ID,
            &[
                entries[0].id.clone(),
                earlier.id.clone(),
                model_write.id.clone(),
            ],
        )
        .await
        .expect("contexts");
    assert_eq!(
        (
            contexts.get("ruled-out-a"),
            contexts.get("ruled-out-b"),
            contexts.get("model-write")
        ),
        (Some(&context(0)), None, Some(&context(3)))
    );
    let journal = store
        .memory_changes(PROJECT_ID, None, /*after*/ 0, /*limit*/ 10)
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1);

    let (replayed, _) = store
        .commit_capture(&group(), &source("digest-1"), units)
        .await
        .expect("replay");
    assert_eq!(
        replayed,
        crate::CommittedCapture {
            newly_committed: false,
            ..committed.clone()
        }
    );
    assert_eq!(
        store.capture(PROJECT_ID, GROUP_ID).await.expect("read"),
        Some(committed)
    );
}

/// Recall's category read: captured ruled-out entries and legacy rejected approaches, newest
/// first and a group in its order; other categories, retired entries and obsolete ones are
/// not candidates; the limit reports more.
#[tokio::test]
async fn categorized_entries_are_read_before_any_ranking() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let legacy = store
        .create_entry(
            BlackboardEntryId::parse("legacy").expect("ID"),
            value(BlackboardKind::RejectedApproach, "legacy"),
        )
        .await
        .expect("legacy");
    let retired = store
        .create_entry(
            BlackboardEntryId::parse("retired").expect("ID"),
            value(BlackboardKind::RejectedApproach, "retired"),
        )
        .await
        .expect("retired");
    forget(&store, &retired).await;
    store
        .create_entry(
            BlackboardEntryId::parse("unrelated").expect("ID"),
            value(BlackboardKind::Fact, "unrelated"),
        )
        .await
        .expect("unrelated");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let mut obsolete = unit(2, "obsolete");
    if let CaptureUnit::Entry { context, .. } = &mut obsolete {
        context.validity = crate::KnowledgeValidity::Obsolete;
    }
    store
        .commit_capture(
            &group(),
            &source("digest-2"),
            vec![unit(0, "first"), unit(1, "second"), obsolete],
        )
        .await
        .expect("commit");
    let (entries, more) = store
        .categorized_entries(
            PROJECT_ID,
            &[KnowledgeCategory::RuledOut],
            &[BlackboardKind::RejectedApproach],
            CandidateLifecycle::Current,
            /*limit*/ 10,
        )
        .await
        .expect("read");
    assert_eq!(
        (
            entries
                .iter()
                .map(|entry| (entry.id.to_string(), entry.context.is_some()))
                .collect::<Vec<_>>(),
            more
        ),
        (
            vec![
                ("ruled-out-first".to_string(), true),
                ("ruled-out-second".to_string(), true),
                (legacy.id.to_string(), false),
            ],
            false
        )
    );
    let (limited, more) = store
        .categorized_entries(
            PROJECT_ID,
            &[KnowledgeCategory::RuledOut],
            &[BlackboardKind::RejectedApproach],
            CandidateLifecycle::Current,
            /*limit*/ 2,
        )
        .await
        .expect("read");
    assert_eq!((limited.len(), more), (2, true));
    let (retired_entries, _) = store
        .categorized_entries(
            PROJECT_ID,
            &[KnowledgeCategory::RuledOut],
            &[BlackboardKind::RejectedApproach],
            CandidateLifecycle::Retired,
            /*limit*/ 10,
        )
        .await
        .expect("read");
    assert_eq!(
        retired_entries
            .iter()
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>(),
        vec![retired.id.to_string()]
    );
}

/// Another source under a committed group's ID is refused; an entry that changed after its
/// words were matched, or an identity now holding other words, is not counted as the unit.
#[tokio::test]
async fn changed_sources_and_entries_are_not_counted() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    store
        .commit_capture(&group(), &source("digest-1"), vec![unit(0, "a")])
        .await
        .expect("commit");
    let conflict = store
        .commit_capture(&group(), &source("digest-2"), vec![unit(0, "a")])
        .await
        .map(|(committed, _)| committed.group.saved);
    let model_write = store
        .create_entry(
            BlackboardEntryId::parse("model-write").expect("ID"),
            value(BlackboardKind::RejectedApproach, "d"),
        )
        .await
        .expect("model write");
    store
        .create_entry(
            BlackboardEntryId::parse("ruled-out-b").expect("ID"),
            value(BlackboardKind::Note, "b"),
        )
        .await
        .expect("other kind under the identity");
    let mut other = group();
    other.group_id = "ruled-out-2".to_string();
    let (committed, _) = store
        .commit_capture(
            &other,
            &source("digest-3"),
            vec![
                CaptureUnit::Existing {
                    id: model_write.id.clone(),
                    revision: model_write.revision + 1,
                    context: context(0),
                },
                unit(1, "b"),
            ],
        )
        .await
        .expect("commit");
    assert_eq!(
        (
            conflict.map_err(|error| error.to_string()),
            committed
                .members
                .iter()
                .map(|member| member.outcome)
                .collect::<Vec<_>>()
        ),
        (
            Err(
                crate::BlackboardStoreError::EntryIdentityConflict(GROUP_ID.to_string())
                    .to_string()
            ),
            vec![MemberOutcome::Omitted, MemberOutcome::Omitted]
        )
    );
}
