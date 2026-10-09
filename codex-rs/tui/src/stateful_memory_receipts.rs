//! Memory receipts in the TUI: the quiet line shown when the user's words were saved,
//! kept for review or refused, and the one-step `/memory apply` and `/memory undo` controls.
//! Receipts and controls never start a model turn.

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
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
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::history_cell::PlainHistoryCell;
use crate::stateful_memory::preview;

/// The receipt lines for one committed (or refused) capture, Apply or Undo. A receipt that
/// saved nothing new (everything was already saved) shows nothing.
pub(crate) fn receipt_cell(receipt: &StatefulMemoryReceipt) -> Option<PlainHistoryCell> {
    let count = |status: StatefulMemoryReceiptStatus| {
        receipt
            .members
            .iter()
            .filter(|member| member.status == status)
            .count()
    };
    let quoted = |status: StatefulMemoryReceiptStatus| {
        receipt
            .members
            .iter()
            .filter(|member| member.status == status)
            .map(|member| preview(&member.text))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    match receipt.kind {
        StatefulMemoryReceiptKind::Rules => {
            let saved = count(StatefulMemoryReceiptStatus::Saved);
            let not_restored = count(StatefulMemoryReceiptStatus::NotRestored);
            if saved > 0 {
                let noun = if saved == 1 { "rule" } else { "rules" };
                lines.push(
                    vec![
                        "• ".dim(),
                        format!("Saved {saved} project {noun} in your words: ").into(),
                        quoted(StatefulMemoryReceiptStatus::Saved).into(),
                    ]
                    .into(),
                );
                let present = count(StatefulMemoryReceiptStatus::AlreadyPresent);
                if present > 0 {
                    lines.push(format!("  {present} already saved before").dim().into());
                }
            }
            if not_restored > 0 {
                lines.push(
                    vec![
                        "• ".dim(),
                        "Not restored (you forgot these earlier): ".into(),
                        quoted(StatefulMemoryReceiptStatus::NotRestored).into(),
                    ]
                    .into(),
                );
                let held = count(StatefulMemoryReceiptStatus::Refused);
                if held > 0 {
                    lines.push(
                        format!(
                            "  Held back with them, nothing saved: {}",
                            quoted(StatefulMemoryReceiptStatus::Refused)
                        )
                        .dim()
                        .into(),
                    );
                }
                lines.push(
                    "  To use a forgotten rule again: /memory add rule <text>"
                        .dim()
                        .into(),
                );
            }
        }
        StatefulMemoryReceiptKind::Proposals => {
            let kept = receipt
                .members
                .iter()
                .filter(|member| {
                    matches!(
                        member.status,
                        StatefulMemoryReceiptStatus::Proposed
                            | StatefulMemoryReceiptStatus::Pending
                    )
                })
                .collect::<Vec<_>>();
            if kept.is_empty() {
                return None;
            }
            lines.push(vec!["• ".dim(), "Kept for your review, not applied:".into()].into());
            for member in kept {
                lines.push(
                    vec![
                        format!("  {}: ", category_noun(member.category)).dim(),
                        preview(&member.text).into(),
                        " (assistant's reading)".dim(),
                    ]
                    .into(),
                );
                if let Some(command) = apply_command(member) {
                    lines.push(format!("    Apply: {command}").cyan().into());
                }
            }
        }
        StatefulMemoryReceiptKind::Promotion => {
            let member = receipt.members.first()?;
            lines.push(
                vec![
                    "• ".dim(),
                    format!("Applied as your {}: ", category_noun(member.category)).into(),
                    preview(&member.text).into(),
                ]
                .into(),
            );
        }
        StatefulMemoryReceiptKind::Undo => {
            let undone = count(StatefulMemoryReceiptStatus::Undone);
            let restored = count(StatefulMemoryReceiptStatus::ProposalRestored);
            let untouched = count(StatefulMemoryReceiptStatus::Untouched);
            let mut parts = Vec::new();
            if undone > 0 {
                parts.push(format!("{undone} no longer used"));
            }
            if restored > 0 {
                parts.push(format!("{restored} returned to review, not applied"));
            }
            if untouched > 0 {
                parts.push(format!("{untouched} saved earlier left as it was"));
            }
            lines.push(vec!["• ".dim(), format!("Undone: {}", parts.join(", ")).into()].into());
        }
    }
    if lines.is_empty() {
        return None;
    }
    if receipt.undoable {
        lines.push(
            format!("  Undo: /memory undo {}", receipt.receipt_id)
                .dim()
                .into(),
        );
    }
    Some(PlainHistoryCell::new(lines))
}

fn category_noun(category: StatefulMemoryReceiptCategory) -> &'static str {
    match category {
        StatefulMemoryReceiptCategory::Rule => "rule",
        StatefulMemoryReceiptCategory::Background => "about you",
        StatefulMemoryReceiptCategory::AttributedContext => "someone else's words",
        StatefulMemoryReceiptCategory::Decision => "decision",
        StatefulMemoryReceiptCategory::BrainstormOption => "option",
        StatefulMemoryReceiptCategory::RuledOut => "ruled-out approach",
        StatefulMemoryReceiptCategory::OpenCheck => "open check",
        StatefulMemoryReceiptCategory::Note | StatefulMemoryReceiptCategory::Other => "note",
    }
}

/// The exact command that applies a kept proposal, for the categories Apply supports.
fn apply_command(member: &StatefulMemoryReceiptMember) -> Option<String> {
    let category = match member.category {
        StatefulMemoryReceiptCategory::Rule => "rule",
        StatefulMemoryReceiptCategory::Decision => "decision",
        StatefulMemoryReceiptCategory::RuledOut => "ruled-out",
        StatefulMemoryReceiptCategory::Background
        | StatefulMemoryReceiptCategory::AttributedContext
        | StatefulMemoryReceiptCategory::BrainstormOption
        | StatefulMemoryReceiptCategory::OpenCheck
        | StatefulMemoryReceiptCategory::Note
        | StatefulMemoryReceiptCategory::Other => return None,
    };
    let (entry_id, revision) = (member.entry_id.as_ref()?, member.revision?);
    Some(format!("/memory apply {entry_id}@{revision} {category}"))
}

/// `rule`, `decision` or `ruled-out`.
pub(crate) fn apply_category(word: &str) -> Option<StatefulMemoryApplyCategory> {
    match word.to_ascii_lowercase().as_str() {
        "rule" => Some(StatefulMemoryApplyCategory::Rule),
        "decision" => Some(StatefulMemoryApplyCategory::Decision),
        "ruled-out" | "ruledout" | "rejected" => Some(StatefulMemoryApplyCategory::RuledOut),
        _ => None,
    }
}

/// Applies `entry_id@revision` with a fresh action identity; the result is the receipt.
pub(crate) async fn apply(
    handle: &AppServerRequestHandle,
    thread_id: &str,
    project_id: &str,
    entry_id: String,
    revision: u64,
    category: StatefulMemoryApplyCategory,
) -> Result<PlainHistoryCell, String> {
    let response: StatefulMemoryApplyResponse = handle
        .request_typed(ClientRequest::StatefulMemoryApply {
            request_id: request_id("apply"),
            params: StatefulMemoryApplyParams {
                thread_id: thread_id.to_string(),
                expected_project_id: project_id.to_string(),
                entry_id,
                expected_revision: revision,
                category,
                client_action_id: uuid::Uuid::new_v4().to_string(),
            },
        })
        .await
        .map_err(|error| format!("Nothing was applied: {error}"))?;
    receipt_cell(&response.receipt).ok_or_else(|| "Nothing was applied.".to_string())
}

/// Undoes one receipt with a fresh action identity; the result is the Undo's receipt.
pub(crate) async fn undo(
    handle: &AppServerRequestHandle,
    thread_id: &str,
    project_id: &str,
    receipt_id: String,
) -> Result<PlainHistoryCell, String> {
    let response: StatefulMemoryUndoResponse = handle
        .request_typed(ClientRequest::StatefulMemoryUndo {
            request_id: request_id("undo"),
            params: StatefulMemoryUndoParams {
                thread_id: thread_id.to_string(),
                expected_project_id: project_id.to_string(),
                receipt_id,
                client_action_id: uuid::Uuid::new_v4().to_string(),
            },
        })
        .await
        .map_err(|error| format!("Nothing was undone: {error}"))?;
    receipt_cell(&response.receipt).ok_or_else(|| "Nothing was undone.".to_string())
}

fn request_id(action: &str) -> RequestId {
    RequestId::String(format!(
        "stateful-tui-memory-{action}-{}",
        uuid::Uuid::new_v4()
    ))
}

#[cfg(test)]
#[path = "stateful_memory_receipts_tests.rs"]
mod tests;
