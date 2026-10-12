use std::sync::Arc;

use codex_extension_api::CodexErrorDetails;
use codex_extension_api::ExtensionData;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::TurnAbortReason;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use pretty_assertions::assert_eq;

use super::MAX_WARNING_BYTES;
use super::ReceiptCheck;
use super::RunCommitReceipt;
use super::TurnRunCommits;
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
    assert_eq!(replies.map(is_status_only), [false; 7], "{replies:?}");
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

fn receipt_for(run: &StatefulRun) -> RunCommitReceipt {
    let commits = TurnRunCommits::default();
    commits.record("turn", run);
    commits.take("turn").expect("recorded receipt")
}

async fn store_with_run(home: &tempfile::TempDir, run_id: &str) -> (StatefulRunStore, StatefulRun) {
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens");
    let run = store
        .create_run(
            StatefulRunId::parse(run_id).expect("run id"),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Answer.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    (store, run)
}

/// A turn's receipt is replaced by its later commit, ends with the turn on every path
/// (stop without a final, abort, error), and only the oldest of more than 64 turns is
/// evicted.
#[tokio::test]
async fn receipts_end_with_their_turn() {
    let home = tempfile::TempDir::new().expect("state home");
    let (_store, run) = store_with_run(&home, "run-a").await;
    let later = StatefulRun {
        revision: run.revision + 1,
        ..run.clone()
    };
    let extension = crate::StatefulExtension {
        projects: Arc::new(codex_thread_store::InMemoryThreadStore::default()),
        services: None,
        event_sink: None,
        autonomous: None,
        attribution: Default::default(),
        visible_root: Default::default(),
        run_activity: Default::default(),
        run_commits: Default::default(),
    };
    let commits = &extension.run_commits;
    for turn in ["stop", "abort", "error", "kept"] {
        commits.record(turn, &run);
    }
    commits.record("kept", &later);
    let thread_store = ExtensionData::new("thread-1");
    let session_store = ExtensionData::new("session");
    extension
        .on_turn_stop(TurnStopInput {
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &ExtensionData::new("stop"),
        })
        .await;
    extension
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &ExtensionData::new("abort"),
        })
        .await;
    extension
        .on_turn_error(TurnErrorInput {
            turn_id: "error",
            error: CodexErrorInfo::Other,
            error_details: &CodexErrorDetails::TurnAborted,
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &ExtensionData::new("error"),
        })
        .await;
    assert_eq!(
        ["stop", "abort", "error", "kept"].map(|turn| commits.take(turn)),
        [None, None, None, Some(receipt_for(&later))]
    );

    for turn in 0..=64 {
        commits.record(&format!("turn-{turn}"), &run);
    }
    assert_eq!(
        (commits.take("turn-0"), commits.take("turn-1")),
        (None, Some(receipt_for(&run)))
    );
}

/// A receipt for a different run than the turn's bound run is never quoted, even when that
/// other run still matches it exactly.
#[tokio::test]
async fn a_receipt_from_another_run_is_unavailable() {
    let home = tempfile::TempDir::new().expect("state home");
    let (store, other) = store_with_run(&home, "run-b").await;
    let bound = StatefulRunId::parse("run-a").expect("run id");
    let receipt = receipt_for(&other);

    assert_eq!(
        (
            ReceiptCheck::read(&store, Some(receipt.clone()), &bound).await,
            ReceiptCheck::read(&store, Some(receipt.clone()), &other.id).await,
        ),
        (ReceiptCheck::NoReceipt, ReceiptCheck::Matched(receipt))
    );
}
