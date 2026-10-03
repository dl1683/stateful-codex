//! How a model's writes to project memory are recorded in the journal of committed changes:
//! the category a new entry is counted under, and which operation (if any) a revision of an
//! existing entry is. A revision that changes nothing is not journaled.

use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::KnowledgeCategory as PiCategory;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;

/// The journal category of a model-written entry.
pub(super) fn written_category(value: &NewBlackboardEntry) -> PiCategory {
    match value.kind {
        BlackboardKind::Decision => PiCategory::Decision,
        BlackboardKind::Instruction => PiCategory::Rule,
        BlackboardKind::Fact if value.content.starts_with("Recipe:") => PiCategory::Recipe,
        BlackboardKind::Question => PiCategory::OpenCheck,
        BlackboardKind::RejectedApproach => PiCategory::RuledOut,
        BlackboardKind::Fact
        | BlackboardKind::Claim
        | BlackboardKind::Number
        | BlackboardKind::Strategy
        | BlackboardKind::Contradiction
        | BlackboardKind::Failure
        | BlackboardKind::Signal
        | BlackboardKind::Note => PiCategory::Note,
    }
}

/// What a model's revision of `before` into `update` is, or `None` when it changes nothing.
pub(super) fn update_operation(
    before: &NewBlackboardEntry,
    update: &BlackboardEntryUpdate,
) -> Option<ChangeOperation> {
    match update.state {
        BlackboardEntryState::Tombstoned => return Some(ChangeOperation::Forgotten),
        BlackboardEntryState::Superseded => return Some(ChangeOperation::Invalidated),
        BlackboardEntryState::Active => {}
    }
    if update.kind != before.kind || update.content != before.content {
        return Some(ChangeOperation::Corrected);
    }
    let invalidated = matches!(
        update.verification,
        BlackboardVerification::Stale | BlackboardVerification::Disputed
    ) && !matches!(
        before.verification,
        BlackboardVerification::Stale | BlackboardVerification::Disputed
    );
    if invalidated {
        return Some(ChangeOperation::Invalidated);
    }
    if update.root_promotion == RootPromotion::Promoted
        && before.root_promotion != RootPromotion::Promoted
    {
        return Some(ChangeOperation::Promoted);
    }
    // Demotion, a new verification, value, confidence, importance, evidence or premises
    // revise the entry without new words.
    let revised = update.root_promotion != before.root_promotion
        || update.verification != before.verification
        || update.structured_value != before.structured_value
        || update.confidence != before.confidence
        || update.importance != before.importance
        || update.evidence != before.evidence
        || update.premises != before.premises;
    revised.then_some(ChangeOperation::Corrected)
}

#[cfg(test)]
#[path = "journal_policy_tests.rs"]
mod tests;
