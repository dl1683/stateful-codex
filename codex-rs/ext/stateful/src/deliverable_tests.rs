use codex_extension_api::TurnFinalizeOutcome;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use pretty_assertions::assert_eq;

use super::MAX_WARNING_BYTES;
use super::ReceiptCheck;
use super::RunCommitReceipt;
use super::is_status_only;
use super::warn;

#[test]
fn status_only_replies_are_recognized() {
    let replies = [
        "Task completed.",
        "**Done!**",
        "run completed successfully",
        "Work done.",
    ];
    assert_eq!(
        replies.map(is_status_only),
        [true, true, true, true],
        "{replies:?}"
    );
}

#[test]
fn real_answers_are_never_status_only() {
    let replies = [
        "42",
        "brief.md",
        "`memo.md`",
        "The answer is 7.",
        "Done — fixed X; tests pass.",
        "Run completed. The final answer is: the gate is C7-42.",
        "Run completed successfully. The deck is ready — see the final answer above for the outline.",
    ];
    assert_eq!(
        replies.map(is_status_only),
        [false; 7],
        "{replies:?}"
    );
}

fn matched(result: String) -> ReceiptCheck {
    ReceiptCheck::Matched(RunCommitReceipt {
        run_id: StatefulRunId::parse("run-1").expect("run id"),
        revision: u64::MAX,
        status: StatefulRunStatus::Completed,
        result: Some(result),
    })
}

fn warning(outcome: TurnFinalizeOutcome) -> String {
    match outcome {
        TurnFinalizeOutcome::Warning(warning) => warning,
        TurnFinalizeOutcome::NotHandled | TurnFinalizeOutcome::Recorded => {
            panic!("a status-only reply always warns")
        }
    }
}

#[test]
fn warning_never_exceeds_1200_bytes() {
    // A 60-byte cut lands inside the two-byte character: the quoted reply keeps 59 bytes.
    let reply = format!("{}é done", "a".repeat(59));
    let prefix = format!(
        "This final reply may omit the deliverable: it contains only a status line (\"{}\"). This turn's recorded result (completed, revision 18446744073709551615): ",
        "a".repeat(59)
    );
    let reserved = " [shortened at 5120 of 5120 bytes]".len();
    let available = MAX_WARNING_BYTES - prefix.len() - reserved;
    // A four-byte character straddles the excerpt budget, so the excerpt stops before it.
    let head = available - 2;
    let result = format!("{}😀{}", "r".repeat(head), "s".repeat(5_120 - head - 4));
    assert_eq!(result.len(), 5_120);

    let single = warning(warn(
        TurnFinalizeOutcome::Recorded,
        &reply,
        &matched(result.clone()),
    ));
    assert_eq!(
        single,
        format!(
            "{prefix}{} [shortened at {head} of 5120 bytes]",
            "r".repeat(head)
        )
    );
    assert!(single.len() <= MAX_WARNING_BYTES, "{}", single.len());

    // The existing warning is cut to 400 bytes on a character boundary first.
    let existing = format!("{}é{}", "w".repeat(399), "w".repeat(2_048 - 401));
    let combined = warning(warn(
        TurnFinalizeOutcome::Warning(existing),
        &reply,
        &matched(result),
    ));
    let budget = MAX_WARNING_BYTES - 399 - 1;
    let head = budget - prefix.len() - reserved;
    assert_eq!(
        combined,
        format!(
            "{}\n{prefix}{} [shortened at {head} of 5120 bytes]",
            "w".repeat(399),
            "r".repeat(head)
        )
    );
    assert!(combined.len() <= MAX_WARNING_BYTES, "{}", combined.len());
}
