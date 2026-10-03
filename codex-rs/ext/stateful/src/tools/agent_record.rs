//! Committing one model-recorded knowledge item and its recipe grounding.
//!
//! A new record commits its entry, its recipe observation (when the host saw the named
//! command succeed) and its journal row in one writer transaction; a replay or a record
//! of an already-bound key saves nothing.

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::CreateOutcome;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::NewBlackboardEntry;
use codex_protocol::models::ResponseItem;

use crate::capture_policy::RecordOutcome;
use crate::recipe_applicability::is_recipe;
use crate::services::ProjectIntelligenceServices;

use super::recipe_grounding;

/// Where a record is made: the thread, turn and conversation it comes from.
pub(super) struct RecordSource<'a> {
    pub(super) thread_id: &'a str,
    pub(super) turn_id: &'a str,
    pub(super) history: &'a [ResponseItem],
}

/// Creates a record that replaces nothing.
pub(super) async fn create(
    services: &ProjectIntelligenceServices,
    store: &BlackboardStore,
    source: &RecordSource<'_>,
    id: BlackboardEntryId,
    value: NewBlackboardEntry,
) -> Result<RecordOutcome, String> {
    let mut grounding = recipe_grounding::resolve(
        services,
        source.thread_id,
        value.kind,
        &value.content,
        source.history,
    );
    let context = grounding
        .as_mut()
        .and_then(|grounding| grounding.context.take());
    let change = ChangeRecord {
        operation: ChangeOperation::Saved,
        origin: ChangeOrigin::ModelTool,
        category: category(value.kind, &value.content),
        action_id: None,
        thread_id: Some(source.thread_id.to_string()),
        turn_id: Some(source.turn_id.to_string()),
        group_id: None,
        preview: value.content.clone(),
    };
    match store
        .create_agent_entry(id, value, context, change)
        .await
        .map_err(|error| error.to_string())?
    {
        (entry, CreateOutcome::Created) => Ok(RecordOutcome::Created {
            entry,
            recipe: grounding.map(|grounding| grounding.label),
        }),
        (entry, CreateOutcome::AlreadyPresent) => Ok(RecordOutcome::AlreadyPresent(entry)),
    }
}

fn category(kind: BlackboardKind, content: &str) -> KnowledgeCategory {
    if is_recipe(kind, content) {
        return KnowledgeCategory::Recipe;
    }
    match kind {
        BlackboardKind::Decision => KnowledgeCategory::Decision,
        BlackboardKind::RejectedApproach => KnowledgeCategory::RuledOut,
        BlackboardKind::Question => KnowledgeCategory::OpenCheck,
        BlackboardKind::Instruction => KnowledgeCategory::Rule,
        BlackboardKind::Fact
        | BlackboardKind::Claim
        | BlackboardKind::Number
        | BlackboardKind::Strategy
        | BlackboardKind::Contradiction
        | BlackboardKind::Failure
        | BlackboardKind::Signal
        | BlackboardKind::Note => KnowledgeCategory::Note,
    }
}
