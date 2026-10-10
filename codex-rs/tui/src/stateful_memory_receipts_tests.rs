use codex_app_server_protocol::StatefulMemoryReceipt;
use codex_app_server_protocol::StatefulMemoryReceiptCategory;
use codex_app_server_protocol::StatefulMemoryReceiptKind;
use codex_app_server_protocol::StatefulMemoryReceiptMember;
use codex_app_server_protocol::StatefulMemoryReceiptStatus;
use pretty_assertions::assert_eq;

use super::receipt_cell;
use crate::history_cell::HistoryCell;

fn member(
    entry: Option<(&str, u64)>,
    category: StatefulMemoryReceiptCategory,
    status: StatefulMemoryReceiptStatus,
    text: &str,
) -> StatefulMemoryReceiptMember {
    StatefulMemoryReceiptMember {
        entry_id: entry.map(|(id, _)| id.to_string()),
        revision: entry.map(|(_, revision)| revision),
        category,
        status,
        text: text.to_string(),
        text_shortened: false,
        applies_text: None,
        applies_text_shortened: false,
    }
}

fn receipt(
    kind: StatefulMemoryReceiptKind,
    receipt_id: &str,
    members: Vec<StatefulMemoryReceiptMember>,
    undoable: bool,
) -> StatefulMemoryReceipt {
    StatefulMemoryReceipt {
        project_id: "project-1".to_string(),
        thread_id: "thread-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        receipt_id: receipt_id.to_string(),
        kind,
        members,
        undoable,
    }
}

fn render(receipt: &StatefulMemoryReceipt) -> String {
    receipt_cell(receipt)
        .map(|cell| {
            cell.display_lines(/*width*/ 200)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "(nothing shown)".to_string())
}

#[test]
fn memory_receipts_show_saved_kept_applied_refused_and_undone() {
    use StatefulMemoryReceiptCategory as Category;
    use StatefulMemoryReceiptKind as Kind;
    use StatefulMemoryReceiptStatus as Status;
    let rules = receipt(
        Kind::Rules,
        "rules-1",
        vec![
            member(
                Some(("rule-a", 1)),
                Category::Rule,
                Status::Saved,
                "Always end each reply with Next:",
            ),
            member(
                Some(("rule-b", 1)),
                Category::Rule,
                Status::Saved,
                "Don't touch vendor/ or any changelog.",
            ),
            member(
                Some(("rule-c", 3)),
                Category::Rule,
                Status::AlreadyPresent,
                "Never push.",
            ),
        ],
        /*undoable*/ true,
    );
    let refused = receipt(
        Kind::Rules,
        "rules-2",
        vec![
            member(
                None,
                Category::Rule,
                Status::NotRestored,
                "Don't touch vendor/ or any changelog.",
            ),
            member(None, Category::Rule, Status::Refused, "Never push."),
        ],
        /*undoable*/ false,
    );
    let proposals = receipt(
        Kind::Proposals,
        "proposal-1",
        vec![
            StatefulMemoryReceiptMember {
                applies_text: Some(
                    "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop, and the export stays in plain files that anyone can open without extra tools."
                        .to_string(),
                ),
                ..member(
                    Some(("proposal-d", 1)),
                    Category::Decision,
                    Status::Proposed,
                    "Use SQLite, not Postgres: must run offline on a laptop.",
                )
            },
            // Prospective words longer than the receipt can show whole: no Apply.
            StatefulMemoryReceiptMember {
                applies_text: Some(format!("We'll keep the {} ledger", "é".repeat(110))),
                applies_text_shortened: true,
                ..member(
                    Some(("proposal-l", 1)),
                    Category::Decision,
                    Status::Proposed,
                    "Keep the ledger format.",
                )
            },
            // Scope unresolved (or a partial citation): kept, but no Apply is offered.
            member(
                Some(("proposal-r", 1)),
                Category::RuledOut,
                Status::Pending,
                "For this task only, the cache is not the cause.",
            ),
            member(
                Some(("proposal-n", 1)),
                Category::Note,
                Status::Pending,
                "Timing measured on the CI runner.",
            ),
        ],
        /*undoable*/ true,
    );
    let applied = receipt(
        Kind::Promotion,
        "promotion-1",
        vec![StatefulMemoryReceiptMember {
            text_shortened: true,
            ..member(
                Some(("proposal-d", 2)),
                Category::Decision,
                Status::Saved,
                "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop.",
            )
        }],
        /*undoable*/ true,
    );
    let undone = receipt(
        Kind::Undo,
        "undo-1",
        vec![
            member(
                Some(("rule-a", 2)),
                Category::Rule,
                Status::Undone,
                "Always end each reply with Next:",
            ),
            member(
                Some(("rule-c", 3)),
                Category::Rule,
                Status::Untouched,
                "Never push.",
            ),
        ],
        /*undoable*/ false,
    );
    let rendered = [rules, refused, proposals, applied, undone]
        .iter()
        .map(render)
        .collect::<Vec<_>>()
        .join("\n---\n");
    insta::assert_snapshot!("memory_receipts", rendered);
}

#[test]
fn already_saved_rules_show_nothing_again() {
    let repeated = receipt(
        StatefulMemoryReceiptKind::Rules,
        "rules-3",
        vec![member(
            Some(("rule-c", 3)),
            StatefulMemoryReceiptCategory::Rule,
            StatefulMemoryReceiptStatus::AlreadyPresent,
            "Never push.",
        )],
        /*undoable*/ false,
    );
    assert_eq!(render(&repeated), "(nothing shown)");
}
