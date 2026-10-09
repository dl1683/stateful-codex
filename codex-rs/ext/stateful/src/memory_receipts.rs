//! Receipts of committed memory captures, and the user's one-step controls over them: Apply
//! a retained proposal, or Undo a receipt. Receipts derive from committed groups only; a
//! refused capture reports its refusal, never a save.

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::CaptureGroup;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::MemberOutcome;
use codex_project_intelligence::PromotionCategory;
use codex_project_intelligence::PromotionRequest;

use crate::events::MAX_RECEIPT_TEXT_BYTES;
use crate::events::receipt_text;
use crate::memory_controls::MemoryControlError;

/// What a receipt reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptKind {
    /// Standing rules the user stated in a supported declaration.
    Rules,
    /// Material kept for review; nothing applied.
    Proposals,
    /// A proposal the user applied.
    Promotion,
    /// A receipt the user undid.
    Undo,
}

/// What became of one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatus {
    Saved,
    AlreadyPresent,
    /// Kept for review, not applied.
    Proposed,
    /// Kept, not applied, with an unresolved dependency.
    Pending,
    /// Recognized but not stored whole; the exact source remains.
    Omitted,
    /// Words the user forgot earlier; not restored by a later statement.
    NotRestored,
    /// Held back with the rest of a refused declaration.
    Refused,
    /// Retired by an Undo.
    Undone,
    /// An Undo returned an applied proposal to review.
    ProposalRestored,
    /// An Undo left this member as it was (it existed before the receipt).
    Untouched,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptMember {
    pub entry_id: Option<String>,
    pub revision: Option<u64>,
    pub category: KnowledgeCategory,
    pub status: ReceiptStatus,
    /// At most `MAX_RECEIPT_TEXT_BYTES` bytes.
    pub text: String,
    /// Whether `text` is shorter than the stored words.
    pub shortened: bool,
    /// For a kept proposal that can be applied: the user's exact words an Apply would settle
    /// (at most `MAX_RECEIPT_TEXT_BYTES` bytes), distinct from the assistant's reading in
    /// `text`. Absent when Apply would refuse.
    pub applies: Option<String>,
    pub applies_shortened: bool,
}

/// One committed (or refused) capture, as the user's clients show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryReceipt {
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    /// The committed group; `undo` takes it.
    pub receipt_id: String,
    pub kind: ReceiptKind,
    pub members: Vec<ReceiptMember>,
    /// Whether one Undo can reverse what this receipt saved.
    pub undoable: bool,
}

impl ReceiptMember {
    pub(crate) fn new(
        entry: Option<(String, u64)>,
        category: KnowledgeCategory,
        status: ReceiptStatus,
        content: &str,
    ) -> Self {
        let (entry_id, revision) = entry.unzip();
        Self {
            entry_id,
            revision,
            category,
            status,
            text: receipt_text(content),
            shortened: content.len() > MAX_RECEIPT_TEXT_BYTES,
            applies: None,
            applies_shortened: false,
        }
    }

    /// The exact words an Apply would settle.
    pub(crate) fn with_applies(mut self, words: &str) -> Self {
        self.applies = Some(receipt_text(words));
        self.applies_shortened = words.len() > MAX_RECEIPT_TEXT_BYTES;
        self
    }
}

impl MemoryReceipt {
    /// The receipt of a committed group whose members all have `category`.
    pub(crate) fn of_group(
        group: &CaptureGroup,
        kind: ReceiptKind,
        category: KnowledgeCategory,
    ) -> Self {
        let members = group
            .members
            .iter()
            .map(|member| {
                let status = match (kind, member.outcome) {
                    (ReceiptKind::Undo, MemberOutcome::Saved) => {
                        if member
                            .reason
                            .as_deref()
                            .is_some_and(|reason| reason.contains("proposalRestored"))
                        {
                            ReceiptStatus::ProposalRestored
                        } else {
                            ReceiptStatus::Undone
                        }
                    }
                    (ReceiptKind::Undo, _) => ReceiptStatus::Untouched,
                    (_, MemberOutcome::Saved) => ReceiptStatus::Saved,
                    (_, MemberOutcome::AlreadyPresent) => ReceiptStatus::AlreadyPresent,
                    (_, MemberOutcome::Pending) => ReceiptStatus::Pending,
                    (_, MemberOutcome::Omitted) => ReceiptStatus::Omitted,
                    (_, MemberOutcome::NotRestored) => ReceiptStatus::NotRestored,
                    (_, MemberOutcome::Failed) => ReceiptStatus::Refused,
                };
                // The committed preview is cropped; its durable whole length says whether.
                let length = member
                    .reason
                    .as_deref()
                    .and_then(|reason| serde_json::from_str::<serde_json::Value>(reason).ok())
                    .and_then(|reason| reason.get("length").and_then(serde_json::Value::as_u64));
                let mut receipt = ReceiptMember::new(
                    member.entry_id.clone().zip(member.revision),
                    category,
                    status,
                    &member.preview,
                );
                receipt.shortened = receipt.shortened
                    || length.is_none_or(|length| length > member.preview.len() as u64);
                receipt
            })
            .collect::<Vec<_>>();
        let undoable = kind != ReceiptKind::Undo
            && members.iter().any(|member| {
                matches!(
                    member.status,
                    ReceiptStatus::Saved | ReceiptStatus::Proposed | ReceiptStatus::Pending
                )
            });
        Self {
            project_id: group.project_id.clone(),
            thread_id: group.thread_id.clone().unwrap_or_default(),
            turn_id: group.turn_id.clone(),
            receipt_id: group.group_id.clone(),
            kind,
            members,
            undoable,
        }
    }
}

/// Applies the proposal `entry_id@expected_revision` as the user's own rule, decision or
/// ruled-out approach, with no model turn. `action_id` identifies the user's action.
pub async fn apply_proposal(
    store: &BlackboardStore,
    admission: &codex_state::ThreadProjectAdmission,
    entry_id: &BlackboardEntryId,
    expected_revision: u64,
    category: PromotionCategory,
    action_id: &str,
) -> Result<MemoryReceipt, MemoryControlError> {
    let knowledge = match category {
        PromotionCategory::Rule => KnowledgeCategory::Rule,
        PromotionCategory::Decision => KnowledgeCategory::Decision,
        PromotionCategory::RuledOut => KnowledgeCategory::RuledOut,
    };
    let group = store
        .promote_proposal(
            admission,
            PromotionRequest {
                entry_id: entry_id.clone(),
                expected_revision,
                category,
                action_id: action_id.to_string(),
            },
        )
        .await
        .map_err(|error| refusal(error, "apply"))?;
    Ok(MemoryReceipt::of_group(
        &group,
        ReceiptKind::Promotion,
        knowledge,
    ))
}

/// Undoes the receipt `receipt_id` with no model turn. Only what that receipt newly saved
/// is retired (an applied proposal returns to review); anything changed since conflicts.
pub async fn undo_receipt(
    store: &BlackboardStore,
    admission: &codex_state::ThreadProjectAdmission,
    receipt_id: &str,
    action_id: &str,
) -> Result<MemoryReceipt, MemoryControlError> {
    let group = store
        .undo_capture_group(admission, receipt_id, action_id)
        .await
        .map_err(|error| refusal(error, "undo"))?;
    let mut receipt = MemoryReceipt::of_group(&group, ReceiptKind::Undo, KnowledgeCategory::Note);
    for member in &mut receipt.members {
        if let Some(id) = member
            .entry_id
            .as_deref()
            .and_then(|id| BlackboardEntryId::parse(id).ok())
            && let Ok(Some(context)) = store.knowledge_context(admission.project_id(), &id).await
        {
            member.category = context.category;
        }
    }
    Ok(receipt)
}

fn refusal(error: BlackboardStoreError, action: &str) -> MemoryControlError {
    let reason = match &error {
        BlackboardStoreError::RevisionConflict { expected, actual } => format!(
            "the memory changed since it was shown (expected revision {expected}, found {actual})"
        ),
        BlackboardStoreError::RetiredIdentity | BlackboardStoreError::SourceExcluded => {
            "these words were forgotten earlier; an Apply never restores them (add them again explicitly to restore them)"
                .to_string()
        }
        BlackboardStoreError::EntryIdentityConflict(id) => {
            format!("the same words are already current as {id}")
        }
        BlackboardStoreError::InvalidSource => match action {
            "apply" => "only a kept proposal that cites the user's whole message (at most 4,096 bytes), with no unresolved scope or other dependency, at its current revision, can be applied".to_string(),
            _ => "this receipt has nothing left that its Undo can reverse".to_string(),
        },
        BlackboardStoreError::EntryNotActive(what) => format!("{what} is no longer current"),
        BlackboardStoreError::EntryNotFound(what) => format!("not found: {what}"),
        BlackboardStoreError::ActionAlreadyRecorded(_) => {
            "this action identity was already used for something else".to_string()
        }
        _ => return MemoryControlError::Store(error),
    };
    MemoryControlError::Refused(format!("nothing to {action}: {reason}"))
}
