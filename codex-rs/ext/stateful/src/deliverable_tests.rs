use codex_extension_api::CodexErrorDetails;
use codex_extension_api::ExtensionData;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStopInput;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::TurnAbortReason;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn status_only_replies_are_recognized() {
    let replies = [
        "Task completed.",
        "**Done!**",
        "run completed successfully",
        "Work done.",
        "Task completed. All done.",
        "Done.\nAll done.",
        "Task complete. Finished.",
        "Task\ncompleted.",
        "All\ndone.",
        "Task\ncompleted successfully.",
        "**Done!** All done.",
        "**Done.**\n**All done.**",
        "`Task completed.` `All done.`",
    ];
    assert_eq!(replies.map(is_status_only), [true; 13], "{replies:?}");
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
        "Task completed. The answer is 7.",
        "Done. See src/lib.rs",
        "Renamed parse_config to load_config in src/config.rs.",
        "Task completed. All tests pass in 3 files.",
        "**`Done`**",
        "**Done.** All done. The answer is 7.",
        "done.done",
        "done.finished",
        "Done. task.done",
    ];
    assert_eq!(replies.map(is_status_only), [false; 16], "{replies:?}");
}

/// A run warns once and another run again; 64 runs are retained, the 65th evicts only the
/// oldest.
#[test]
fn each_run_warns_once() {
    let warned = WarnedRuns::default();
    let run = |index: usize| StatefulRunId::parse(format!("run-{index}")).expect("run id");
    let first = (0..64).all(|index| warned.first(&run(index)));
    let retained = [warned.first(&run(0)), warned.first(&run(63))];
    let added = warned.first(&run(64));
    let evicted = [warned.first(&run(1)), warned.first(&run(0))];
    assert_eq!(
        (first, retained, added, evicted),
        (true, [false, false], true, [false, true])
    );
}

/// 20-digit revision; reply cut in `é`; 5 KiB result cut before `😀`; 2 KiB prior warning.
#[test]
fn warning_never_exceeds_1200_bytes() {
    let reply = format!("{}é done", "a".repeat(59));
    let prefix = format!(
        "This final reply may omit the deliverable: it contains only a status line (\"{}\"). This turn's recorded result (completed, revision 18446744073709551615): ",
        "a".repeat(59)
    );
    let reserved = " [shortened at 5120 of 5120 bytes]".len();
    let head = MAX_WARNING_BYTES - prefix.len() - reserved - 2;
    let result = format!("{}😀{}", "r".repeat(head), "s".repeat(5_120 - head - 4));
    let check = ReceiptCheck::Matched(RunCommitReceipt {
        run_id: StatefulRunId::parse("run-1").expect("run id"),
        revision: u64::MAX,
        status: StatefulRunStatus::Completed,
        result: Some(result),
    });
    let single = format!(
        "{prefix}{} [shortened at {head} of 5120 bytes]",
        "r".repeat(head)
    );
    let existing = format!("{}é{}", "w".repeat(399), "w".repeat(1_647));
    let head = MAX_WARNING_BYTES - 400 - prefix.len() - reserved;
    let combined = format!(
        "{}\n{prefix}{} [shortened at {head} of 5120 bytes]",
        "w".repeat(399),
        "r".repeat(head)
    );
    assert_eq!(
        (
            warn(TurnFinalizeOutcome::Recorded, &reply, &check),
            warn(TurnFinalizeOutcome::Warning(existing), &reply, &check),
            single.len() <= MAX_WARNING_BYTES && combined.len() <= MAX_WARNING_BYTES,
        ),
        (
            TurnFinalizeOutcome::Warning(single),
            TurnFinalizeOutcome::Warning(combined),
            true
        )
    );
}

/// Receipts: replaced by a later commit, removed at stop/abort/error, oldest of 65 evicted,
/// and one for another run than the bound run is never quoted, even while it still matches.
#[tokio::test]
async fn receipts_end_with_their_turn_and_bind_to_their_run() {
    let home = tempfile::TempDir::new().expect("state home");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let budget = RunBudget {
        max_continuations: 1,
        max_elapsed_seconds: 3_600,
    };
    let run = NewStatefulRun {
        project_id: "project-1".to_string(),
        thread_ids: vec!["thread-1".to_string()],
        goal: "Answer.".to_string(),
        mode: WorkflowMode::Collaborative,
        budget,
    };
    let id = StatefulRunId::parse("run-b").expect("run id");
    let run = store.create_run(id, run).await.expect("run is created");
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
        turn_progress: Default::default(),
    };
    let commits = &extension.run_commits;
    for turn in ["stop", "abort", "error", "kept", "other"] {
        commits.record(turn, &run);
        extension.turn_progress.record(turn, &run.id);
    }
    commits.record("kept", &later);
    let (session, thread) = (ExtensionData::new("session"), ExtensionData::new("thread"));
    let [stop, abort, error] = ["stop", "abort", "error"].map(ExtensionData::new);
    let stop = TurnStopInput {
        session_store: &session,
        thread_store: &thread,
        turn_store: &stop,
    };
    extension.on_turn_stop(stop).await;
    let abort = TurnAbortInput {
        reason: TurnAbortReason::Interrupted,
        session_store: &session,
        thread_store: &thread,
        turn_store: &abort,
    };
    extension.on_turn_abort(abort).await;
    let error = TurnErrorInput {
        turn_id: "error",
        error: CodexErrorInfo::Other,
        error_details: &CodexErrorDetails::TurnAborted,
        session_store: &session,
        thread_store: &thread,
        turn_store: &error,
    };
    extension.on_turn_error(error).await;
    let receipt = commits.take("other");
    let bound = StatefulRunId::parse("run-a").expect("run id");
    let checks = (
        ReceiptCheck::read(&store, receipt.clone(), &bound).await,
        ReceiptCheck::read(&store, receipt.clone(), &run.id).await,
    );
    let revision = |turn: &str| commits.take(turn).map(|receipt| receipt.revision);
    let ended = ["stop", "abort", "error", "kept"].map(revision);
    assert_eq!(
        ["stop", "abort", "error", "kept"].map(|turn| extension.turn_progress.take(turn)),
        [None, None, None, Some(run.id.clone())]
    );
    for turn in 0..=64 {
        commits.record(&format!("turn-{turn}"), &run);
    }
    assert_eq!(
        (checks, ended, ["turn-0", "turn-1"].map(revision)),
        (
            (
                ReceiptCheck::NoReceipt,
                ReceiptCheck::Matched(receipt.expect("receipt"))
            ),
            [None, None, None, Some(later.revision)],
            [None, Some(run.revision)]
        )
    );
}
