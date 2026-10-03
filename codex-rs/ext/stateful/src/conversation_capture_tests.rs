use std::sync::Mutex;

use codex_extension_api::ExtensionData;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::CandidateLifecycle;
use codex_project_intelligence::CategoryQuery;
use codex_project_intelligence::KnowledgeCategory as Category;
use codex_project_intelligence::KnowledgeScope;
use codex_project_intelligence::ScopeKind;
use codex_project_intelligence::ScopeState;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

use super::CaptureTurn;
use super::capture_completed_answer;
use super::observe_agent_message;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::events::GroupReceipt;
use crate::events::KnowledgeCategory;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";

const ANSWER: &str = "Root cause found: the vendored Click records the parameter source late.

What was ruled out:

- Environment variable `SHIPIT_CONFIG`: absent during reproduction.
- Incorrect home-path expansion: the expected temporary home path was used.
- `--env` and `--dry-run`: both variants fail identically.

Open checks:
- Confirm the tests import the vendored click, not the installed one.

Decision: fix the ordering in the vendored Click.
Reason: both symptoms come from the same ordering.

Decision: leave the README unchanged.
";

#[derive(Default)]
struct Events(Mutex<Vec<StatefulEvent>>);

impl StatefulEventSink for Events {
    fn emit(&self, event: StatefulEvent) {
        self.0.lock().expect("events").push(event);
    }
}

impl Events {
    /// Category and saved/already/omitted/failed counts of each group receipt, then the
    /// item texts of each.
    fn receipts(&self) -> Vec<(KnowledgeCategory, [u32; 4], Vec<String>)> {
        self.0
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| match event {
                StatefulEvent::KnowledgeGroupCaptured(GroupReceipt {
                    category,
                    saved,
                    already_present,
                    omitted,
                    failed,
                    items,
                    ..
                }) => Some((
                    *category,
                    [*saved, *already_present, *omitted, *failed],
                    items.iter().map(|item| item.text.clone()).collect(),
                )),
                _ => None,
            })
            .collect()
    }
}

fn message(id: &str, text: &str, phase: Option<MessagePhase>) -> AgentMessageItem {
    AgentMessageItem {
        id: id.to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase,
        memory_citation: None,
        delivery: None,
        questions: None,
    }
}

/// A turn store as the hooks leave it after the turn's messages completed.
fn completed_turn(turn_id: &str, messages: &[AgentMessageItem]) -> ExtensionData {
    let turn_store = ExtensionData::new("turn");
    turn_store.insert(CaptureTurn {
        turn_id: turn_id.to_string(),
    });
    for message in messages {
        observe_agent_message(&turn_store, message);
    }
    turn_store
}

async fn capture(services: &ProjectIntelligenceServices, events: &Events, turn: &ExtensionData) {
    capture_completed_answer(services, Some(events), PROJECT_ID, THREAD_ID, turn).await;
}

/// Content, category and the context payload's details of every current entry of `kinds`.
async fn stored(
    services: &ProjectIntelligenceServices,
    categories: &[Category],
) -> Vec<(String, Option<Category>, Option<String>, Value)> {
    let store = services.blackboard().await.expect("store");
    let (entries, _) = store
        .categorized_entries(CategoryQuery {
            project_id: PROJECT_ID,
            categories,
            legacy_kinds: &[BlackboardKind::RejectedApproach],
            lifecycle: CandidateLifecycle::Current,
            topic: &[],
            changed_since_ms: None,
            limit: 50,
        })
        .await
        .expect("entries");
    entries
        .into_iter()
        .map(|entry| {
            let context = entry.context;
            let details = context
                .as_ref()
                .and_then(|context| context.payload.as_deref())
                .map(|payload| serde_json::from_str::<Value>(payload).expect("payload"))
                .map(|payload| payload["details"].clone())
                .unwrap_or(Value::Null);
            (
                entry.content,
                context.as_ref().map(|context| context.category),
                context.and_then(|context| context.scope_id),
                details,
            )
        })
        .collect()
}

/// With no model memory call, the final answer's three ruled-out items, its open check and
/// both decisions are saved as separate entries in the answer's order, the decision without
/// a reason says so, and each kind gets one counted receipt. Mid-turn commentary is not
/// read, and reading the same completed answer again changes nothing.
#[tokio::test]
async fn a_completed_answer_saves_each_unit_once() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let events = Events::default();
    let turn = completed_turn(
        "turn-1",
        &[
            message("item-1", ANSWER, None),
            message(
                "item-2",
                "Ruled out:\n- a commentary claim\n",
                Some(MessagePhase::Commentary),
            ),
        ],
    );
    capture(&services, &events, &turn).await;
    capture(
        &services,
        &events,
        &completed_turn("turn-1", &[message("item-1", ANSWER, None)]),
    )
    .await;

    assert_eq!(
        events.receipts(),
        vec![
            (
                KnowledgeCategory::RuledOut,
                [3, 0, 0, 0],
                vec![
                    "Environment variable `SHIPIT_CONFIG`: absent during reproduction.".to_string(),
                    "Incorrect home-path expansion: the expected temporary home path was used."
                        .to_string(),
                    "`--env` and `--dry-run`: both variants fail identically.".to_string(),
                ]
            ),
            (
                KnowledgeCategory::OpenCheck,
                [1, 0, 0, 0],
                vec![
                    "Confirm the tests import the vendored click, not the installed one."
                        .to_string()
                ]
            ),
            (
                KnowledgeCategory::Decision,
                [2, 0, 0, 0],
                vec![
                    "Decision: fix the ordering in the vendored Click.\nReason: both symptoms come from the same ordering.".to_string(),
                    "Decision: leave the README unchanged.\nReason: not recorded in the answer".to_string(),
                ]
            ),
        ]
    );
    // Groups of one answer commit one after another; compare them in a fixed order.
    let mut stored = stored(&services, &[Category::Decision, Category::OpenCheck])
        .await
        .into_iter()
        .map(|(_, category, _, details)| {
            (
                category.map(Category::as_str),
                details["state"].clone(),
                details["reasonStatus"].clone(),
                details["reason"]["text"].clone(),
            )
        })
        .collect::<Vec<_>>();
    stored.sort_by_key(|row| format!("{row:?}"));
    assert_eq!(
        stored,
        vec![
            (
                Some("decision"),
                Value::Null,
                Value::String("notRecorded".to_string()),
                Value::Null,
            ),
            (
                Some("decision"),
                Value::Null,
                Value::String("recorded".to_string()),
                Value::String("both symptoms come from the same ordering.".to_string()),
            ),
            (
                Some("open_check"),
                Value::String("open".to_string()),
                Value::Null,
                Value::Null,
            ),
        ]
    );
}

/// A model entry holding exactly one item's words becomes that member (no duplicate); once
/// the user forgets both items (the host's and the model's), a later answer repeating them
/// in an investigation restores neither, and its new item is saved in that investigation.
#[tokio::test]
async fn answers_reconcile_with_existing_memory() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let store = services.blackboard().await.expect("store");
    let node_id = services.project_node_id(PROJECT_ID).await.expect("node");
    store
        .create_entry(
            BlackboardEntryId::parse("model-write").expect("ID"),
            codex_project_intelligence::NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id,
                kind: BlackboardKind::RejectedApproach,
                content: "`--env` and `--dry-run`:  both variants fail identically.".to_string(),
                structured_value: None,
                confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(7_000)
                    .expect("confidence"),
                verification: codex_project_intelligence::BlackboardVerification::Unverified,
                importance: codex_project_intelligence::BlackboardImportance::Normal,
                root_promotion: codex_project_intelligence::RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: codex_project_intelligence::BlackboardProvenance {
                    kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
                    source_id: "call-1".to_string(),
                },
            },
        )
        .await
        .expect("model write");
    let events = Events::default();
    let first = "Ruled out:\n- Environment variable `SHIPIT_CONFIG`: absent during reproduction.\n- `--env` and `--dry-run`: both variants fail identically.\n";
    capture(
        &services,
        &events,
        &completed_turn("turn-1", &[message("item-1", first, None)]),
    )
    .await;
    let saved = stored(&services, &[Category::RuledOut]).await;
    // The user forgets both items: the host-saved one and the model's own matched entry.
    let (current, _) = store
        .categorized_entries(CategoryQuery {
            project_id: PROJECT_ID,
            categories: &[Category::RuledOut],
            legacy_kinds: &[],
            lifecycle: CandidateLifecycle::Current,
            topic: &[],
            changed_since_ms: None,
            limit: 10,
        })
        .await
        .expect("entries");
    for forgotten in current {
        crate::memory_controls::forget_entry(store, PROJECT_ID, &forgotten.id, forgotten.revision)
            .await
            .expect("forget");
    }
    store
        .open_scope(&KnowledgeScope {
            project_id: PROJECT_ID.to_string(),
            scope_id: "scope-1".to_string(),
            kind: ScopeKind::Investigation,
            title: "this whole investigation".to_string(),
            state: ScopeState::Open,
            end_condition: None,
            opened_source: "user-message:thread-1/turn-0".to_string(),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .expect("scope");
    store
        .bind_thread_scope(PROJECT_ID, THREAD_ID, "scope-1")
        .await
        .expect("bind");
    let second = format!("{first}- Path expansion: same failure with an absolute path.\n");
    capture(
        &services,
        &events,
        &completed_turn("turn-2", &[message("item-1", &second, None)]),
    )
    .await;

    assert_eq!(
        (
            saved
                .iter()
                .map(|(content, category, scope, _)| (content.clone(), *category, scope.clone()))
                .collect::<Vec<_>>(),
            events
                .receipts()
                .into_iter()
                .map(|(category, counts, _)| (category, counts))
                .collect::<Vec<_>>(),
        ),
        (
            vec![
                (
                    "Environment variable `SHIPIT_CONFIG`: absent during reproduction.".to_string(),
                    Some(Category::RuledOut),
                    None
                ),
                (
                    "`--env` and `--dry-run`:  both variants fail identically.".to_string(),
                    Some(Category::RuledOut),
                    None
                ),
            ],
            vec![
                (KnowledgeCategory::RuledOut, [1, 1, 0, 0]),
                (KnowledgeCategory::RuledOut, [1, 0, 2, 0]),
            ]
        )
    );
}

/// A turn whose only message is mid-turn commentary has no final answer to read. (Aborted
/// and failed turns never reach this capture: only the completed-turn hook calls it.)
#[tokio::test]
async fn nothing_is_read_without_a_completed_answer() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let events = Events::default();
    capture(
        &services,
        &events,
        &completed_turn(
            "turn-1",
            &[message(
                "item-1",
                "Ruled out:\n- the proxy\n",
                Some(MessagePhase::Commentary),
            )],
        ),
    )
    .await;
    assert_eq!(events.receipts(), Vec::new());
}
