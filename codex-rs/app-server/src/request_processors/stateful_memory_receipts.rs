//! `statefulMemory/apply` and `statefulMemory/undo`: the user's one-step controls over
//! committed captures, with no model turn, plus the receipt shape shared with the
//! `statefulMemory/captured` notification.

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryApplyParams;
use codex_app_server_protocol::StatefulMemoryApplyResponse;
use codex_app_server_protocol::StatefulMemoryReceipt;
use codex_app_server_protocol::StatefulMemoryReceiptCategory;
use codex_app_server_protocol::StatefulMemoryReceiptKind;
use codex_app_server_protocol::StatefulMemoryReceiptMember;
use codex_app_server_protocol::StatefulMemoryReceiptStatus;
use codex_app_server_protocol::StatefulMemoryUndoParams;
use codex_app_server_protocol::StatefulMemoryUndoResponse;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::PromotionCategory;
use codex_stateful_extension::MemoryControlError;
use codex_stateful_extension::MemoryReceipt;
use codex_stateful_extension::ReceiptKind;
use codex_stateful_extension::ReceiptStatus;
use codex_stateful_extension::StatefulEvent;

use super::BlackboardRequestProcessor;
use super::blackboard_error;
use crate::error_code::invalid_params;

impl BlackboardRequestProcessor {
    pub(crate) async fn memory_apply(
        &self,
        params: StatefulMemoryApplyParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let admission = self
            .admit_memory(&params.thread_id, &params.expected_project_id)
            .await?;
        action_id(&params.client_action_id)?;
        let id = BlackboardEntryId::parse(params.entry_id)
            .map_err(|error| invalid_params(error.to_string()))?;
        let category = match params.category {
            StatefulMemoryApplyCategory::Rule => PromotionCategory::Rule,
            StatefulMemoryApplyCategory::Decision => PromotionCategory::Decision,
            StatefulMemoryApplyCategory::RuledOut => PromotionCategory::RuledOut,
        };
        let receipt = codex_stateful_extension::apply_proposal(
            self.store().await?,
            &admission,
            &id,
            params.expected_revision,
            category,
            &params.client_action_id,
        )
        .await
        .map_err(control_error)?;
        self.publish(&receipt);
        Ok(Some(
            StatefulMemoryApplyResponse {
                receipt: api_memory_receipt(receipt),
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_undo(
        &self,
        params: StatefulMemoryUndoParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let admission = self
            .admit_memory(&params.thread_id, &params.expected_project_id)
            .await?;
        action_id(&params.client_action_id)?;
        if params.receipt_id.is_empty() || params.receipt_id.len() > 512 {
            return Err(invalid_params("receiptId must be 1-512 bytes"));
        }
        let receipt = codex_stateful_extension::undo_receipt(
            self.store().await?,
            &admission,
            &params.receipt_id,
            &params.client_action_id,
        )
        .await
        .map_err(control_error)?;
        self.publish(&receipt);
        Ok(Some(
            StatefulMemoryUndoResponse {
                receipt: api_memory_receipt(receipt),
            }
            .into(),
        ))
    }

    /// Revision hints for every changed entry, then the receipt itself.
    fn publish(&self, receipt: &MemoryReceipt) {
        for member in &receipt.members {
            if let (Some(entry_id), Some(revision)) = (&member.entry_id, member.revision)
                && member.status != ReceiptStatus::Untouched
            {
                self.event_sink.emit(StatefulEvent::BlackboardUpdated {
                    project_id: receipt.project_id.clone(),
                    entity_kind: codex_stateful_extension::BlackboardEntityKind::Entry,
                    entity_id: entry_id.clone(),
                    revision,
                });
            }
        }
        self.event_sink
            .emit(StatefulEvent::MemoryReceipt(receipt.clone()));
    }
}

fn action_id(value: &str) -> Result<(), JSONRPCErrorError> {
    if value.trim().is_empty() || value.len() > 128 {
        return Err(invalid_params("clientActionId must be 1-128 bytes"));
    }
    Ok(())
}

fn control_error(error: MemoryControlError) -> JSONRPCErrorError {
    match error {
        MemoryControlError::Refused(message) => {
            invalid_params(format!("{message}; nothing was changed"))
        }
        MemoryControlError::Store(error) => blackboard_error(error),
    }
}

/// The wire shape of a receipt.
pub(crate) fn api_memory_receipt(receipt: MemoryReceipt) -> StatefulMemoryReceipt {
    StatefulMemoryReceipt {
        project_id: receipt.project_id,
        thread_id: receipt.thread_id,
        turn_id: receipt.turn_id,
        receipt_id: receipt.receipt_id,
        kind: match receipt.kind {
            ReceiptKind::Rules => StatefulMemoryReceiptKind::Rules,
            ReceiptKind::Proposals => StatefulMemoryReceiptKind::Proposals,
            ReceiptKind::Promotion => StatefulMemoryReceiptKind::Promotion,
            ReceiptKind::Undo => StatefulMemoryReceiptKind::Undo,
        },
        members: receipt
            .members
            .into_iter()
            .map(|member| StatefulMemoryReceiptMember {
                entry_id: member.entry_id,
                revision: member.revision,
                category: match member.category {
                    KnowledgeCategory::Rule => StatefulMemoryReceiptCategory::Rule,
                    KnowledgeCategory::Background => StatefulMemoryReceiptCategory::Background,
                    KnowledgeCategory::AttributedContext => {
                        StatefulMemoryReceiptCategory::AttributedContext
                    }
                    KnowledgeCategory::Decision => StatefulMemoryReceiptCategory::Decision,
                    KnowledgeCategory::BrainstormOption => {
                        StatefulMemoryReceiptCategory::BrainstormOption
                    }
                    KnowledgeCategory::RuledOut => StatefulMemoryReceiptCategory::RuledOut,
                    KnowledgeCategory::OpenCheck => StatefulMemoryReceiptCategory::OpenCheck,
                    KnowledgeCategory::Note => StatefulMemoryReceiptCategory::Note,
                    KnowledgeCategory::Recipe
                    | KnowledgeCategory::CodeObservation
                    | KnowledgeCategory::CommitObservation
                    | KnowledgeCategory::Legacy => StatefulMemoryReceiptCategory::Other,
                },
                status: match member.status {
                    ReceiptStatus::Saved => StatefulMemoryReceiptStatus::Saved,
                    ReceiptStatus::AlreadyPresent => StatefulMemoryReceiptStatus::AlreadyPresent,
                    ReceiptStatus::Proposed => StatefulMemoryReceiptStatus::Proposed,
                    ReceiptStatus::Pending => StatefulMemoryReceiptStatus::Pending,
                    ReceiptStatus::Omitted => StatefulMemoryReceiptStatus::Omitted,
                    ReceiptStatus::NotRestored => StatefulMemoryReceiptStatus::NotRestored,
                    ReceiptStatus::Refused => StatefulMemoryReceiptStatus::Refused,
                    ReceiptStatus::Undone => StatefulMemoryReceiptStatus::Undone,
                    ReceiptStatus::ProposalRestored => {
                        StatefulMemoryReceiptStatus::ProposalRestored
                    }
                    ReceiptStatus::Untouched => StatefulMemoryReceiptStatus::Untouched,
                },
                text: member.text,
                text_shortened: member.shortened,
            })
            .collect(),
        undoable: receipt.undoable,
    }
}
