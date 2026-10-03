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
use crate::CategoryQuery;
use crate::ChangeOperation;
use crate::ChangeOrigin;
use crate::ChangeRecord;
use crate::ConfidenceScore;
use crate::ExistingMatch;
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
        identity_keys: vec![format!("key-{content}"), format!("project-key-{content}")],
        existing: ExistingMatch::Reuse,
        value: Box::new(value(BlackboardKind::RejectedApproach, content)),
        context: Box::new(context(ordinal)),
        change: Box::new(ChangeRecord {
            operation: ChangeOperation::Saved,
            origin: ChangeOrigin::HostCapture,
            category: KnowledgeCategory::RuledOut,
            action_id: None,
            thread_id: Some("thread-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            group_id: Some(GROUP_ID.to_string()),
            preview: content.to_string(),
        }),
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

fn query<'a>(lifecycle: CandidateLifecycle, topic: &'a [String], limit: u32) -> CategoryQuery<'a> {
    CategoryQuery {
        project_id: PROJECT_ID,
        categories: &[KnowledgeCategory::RuledOut],
        legacy_kinds: &[BlackboardKind::RejectedApproach],
        lifecycle,
        topic,
        changed_since_ms: None,
        limit,
    }
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
/// Indexes every entry not yet indexed, giving each the key `key-<content>`.
async fn index_all(store: &BlackboardStore) {
    loop {
        let (batch, covered, newest) = store
            .identity_backfill_batch(PROJECT_ID, /*limit*/ 100)
            .await
            .expect("batch");
        if covered >= newest {
            return;
        }
        let through = batch.last().map_or(newest, |entry| entry.rowid);
        let identities = batch
            .iter()
            .map(|entry| (format!("key-{}", entry.content), entry.id.clone()))
            .collect::<Vec<_>>();
        store
            .record_identity_backfill(PROJECT_ID, covered, through, &identities)
            .await
            .expect("record");
    }
}

/// Every unit is judged in one transaction against the identity index: new words are
/// saved with context, journal row and identity; words active under any identity are the
/// same unit (a model entry gets the context); words forgotten under any identity, however
/// old, stay forgotten; an omitted unit is listed. The same source again changes nothing.
#[tokio::test]
async fn a_capture_commits_whole_and_replays_unchanged() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    for (id, content) in [("model-b", "b"), ("model-c", "c"), ("model-d", "d")] {
        store
            .create_entry(
                BlackboardEntryId::parse(id).expect("ID"),
                value(BlackboardKind::RejectedApproach, content),
            )
            .await
            .expect("model write");
    }
    let forgotten = store
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse("model-c").expect("ID"),
        )
        .await
        .expect("read")
        .expect("c");
    forget(&store, &forgotten).await;
    index_all(&store).await;
    let units = vec![
        unit(0, "a"),
        unit(1, "b"),
        unit(2, "c"),
        CaptureUnit::Omitted {
            note: "too long: xxxx".to_string(),
        },
    ];
    let (committed, entries) = store
        .commit_capture(&group(), &source("digest-1"), units.clone())
        .await
        .expect("commit");
    assert_eq!(
        (committed.group.clone(), committed.members.clone()),
        (
            CaptureGroup {
                recognized: 4,
                saved: 1,
                already_present: 1,
                omitted: 2,
                ..group()
            },
            vec![
                member(0, Some("ruled-out-a"), MemberOutcome::Saved),
                member(1, Some("model-b"), MemberOutcome::AlreadyPresent),
                CaptureMember {
                    note: Some("c".to_string()),
                    ..member(2, None, MemberOutcome::NotRestored)
                },
                CaptureMember {
                    note: Some("too long: xxxx".to_string()),
                    ..member(3, None, MemberOutcome::Omitted)
                },
            ],
        )
    );
    let contexts = store
        .knowledge_contexts(PROJECT_ID, &[entries[0].id.clone(), entries[1].id.clone()])
        .await
        .expect("contexts");
    assert_eq!(
        (contexts.get("ruled-out-a"), contexts.get("model-b")),
        (Some(&context(0)), Some(&context(1)))
    );
    let (replayed, _) = store
        .commit_capture(&group(), &source("digest-1"), units)
        .await
        .expect("replay");
    assert_eq!(
        (replayed.newly_committed, replayed.members),
        (false, committed.members)
    );
    // The saved unit's own identity is indexed: forgetting it keeps it forgotten.
    let saved = store
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse("ruled-out-a").expect("ID"),
        )
        .await
        .expect("read")
        .expect("a");
    forget(&store, &saved).await;
    index_all(&store).await;
    let mut again = group();
    again.group_id = "ruled-out-2".to_string();
    let (later, _) = store
        .commit_capture(&again, &source("digest-2"), vec![unit(0, "a")])
        .await
        .expect("commit");
    assert_eq!(later.members[0].outcome, MemberOutcome::NotRestored);
}

/// Until every entry is indexed, a capture creates nothing (forgotten words in an
/// unindexed entry could otherwise come back); an entry kept separate is created beside
/// an active match; another source under a group's ID is refused.
#[tokio::test]
async fn unindexed_entries_close_capture_and_sources_are_fixed() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    store
        .create_entry(
            BlackboardEntryId::parse("model-x").expect("ID"),
            value(BlackboardKind::RejectedApproach, "x"),
        )
        .await
        .expect("model write");
    let (closed, _) = store
        .commit_capture(&group(), &source("digest-1"), vec![unit(0, "y")])
        .await
        .expect("commit");
    index_all(&store).await;
    let mut separate = unit(0, "x");
    if let CaptureUnit::Entry { existing, .. } = &mut separate {
        *existing = ExistingMatch::KeepSeparate;
    }
    let mut other = group();
    other.group_id = "ruled-out-2".to_string();
    let (kept, _) = store
        .commit_capture(&other, &source("digest-2"), vec![separate])
        .await
        .expect("commit");
    let conflict = store
        .commit_capture(&group(), &source("digest-3"), vec![unit(0, "z")])
        .await
        .map(|(committed, _)| committed.group.saved)
        .map_err(|error| error.to_string());
    assert_eq!(
        (closed.members[0].outcome, kept.members[0].clone(), conflict),
        (
            MemberOutcome::Omitted,
            member(0, Some("ruled-out-x"), MemberOutcome::Saved),
            Err(
                crate::BlackboardStoreError::EntryIdentityConflict(GROUP_ID.to_string())
                    .to_string()
            ),
        )
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
    index_all(&store).await;
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
        .categorized_entries(query(CandidateLifecycle::Current, &[], 10))
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
        .categorized_entries(query(CandidateLifecycle::Current, &[], 2))
        .await
        .expect("read");
    assert_eq!((limited.len(), more), (2, true));
    // A topic word reaches the oldest entry even under a limit the newer ones fill.
    let topic = vec!["legacy".to_string()];
    let (on_topic, _) = store
        .categorized_entries(query(CandidateLifecycle::Current, &topic, 1))
        .await
        .expect("read");
    assert_eq!(
        on_topic
            .iter()
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>(),
        vec![legacy.id.to_string()]
    );
    // Word-for-word lookup finds the forgotten entry however old.
    let same = store
        .entries_with_words(
            PROJECT_ID,
            KnowledgeCategory::RuledOut,
            BlackboardKind::RejectedApproach,
            /*scope_id*/ None,
            "retired",
            "no-digest",
        )
        .await
        .expect("words");
    assert_eq!(
        same.iter()
            .map(|entry| (entry.id.to_string(), entry.state))
            .collect::<Vec<_>>(),
        vec![(
            retired.id.to_string(),
            crate::BlackboardEntryState::Tombstoned
        )]
    );
    let (retired_entries, _) = store
        .categorized_entries(query(CandidateLifecycle::Retired, &[], 10))
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

/// Word lookup ignores spacing and ASCII case and is bounded only after eligibility; topic
/// reads search content and recorded answer openings (not other metadata), in any case.
#[tokio::test]
async fn word_and_topic_reads_match_what_recall_and_capture_mean() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    store
        .create_entry(
            BlackboardEntryId::parse("spaced").expect("ID"),
            value(
                BlackboardKind::RejectedApproach,
                "DNS:	  Absent
   here.",
            ),
        )
        .await
        .expect("spaced");
    store
        .create_entry(
            BlackboardEntryId::parse("unicode").expect("ID"),
            value(BlackboardKind::RejectedApproach, "ÜBER cache: same result."),
        )
        .await
        .expect("unicode");
    let mut with_opening = unit(0, "plain");
    if let CaptureUnit::Entry { context, .. } = &mut with_opening {
        context.payload = Some(
            r#"{"sourceLocator":"assistant-answer:source","answerOpening":"Tokenizer bug."}"#
                .to_string(),
        );
    }
    index_all(&store).await;
    store
        .commit_capture(&group(), &source("digest-1"), vec![with_opening])
        .await
        .expect("commit");
    let same = store
        .entries_with_words(
            PROJECT_ID,
            KnowledgeCategory::RuledOut,
            BlackboardKind::RejectedApproach,
            /*scope_id*/ None,
            "dns:absenthere.",
            "no-digest",
        )
        .await
        .expect("words");
    let topic = |words: &[&str]| words.iter().map(ToString::to_string).collect::<Vec<_>>();
    let mut read = Vec::new();
    for words in [topic(&["source"]), topic(&["tokenizer"]), topic(&["über"])] {
        let (entries, _) = store
            .categorized_entries(query(CandidateLifecycle::Current, &words, 10))
            .await
            .expect("read");
        read.push(
            entries
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        (
            same.iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            read
        ),
        (
            vec!["spaced".to_string()],
            vec![
                Vec::new(),
                vec!["ruled-out-plain".to_string()],
                vec!["unicode".to_string()]
            ]
        )
    );
}
