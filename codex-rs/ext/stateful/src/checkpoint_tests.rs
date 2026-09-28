use codex_extension_api::ToolCallOutcome;
use pretty_assertions::assert_eq;

use super::CHECKPOINT_TOOL_CALLS;
use super::RunActivity;

const DONE: ToolCallOutcome = ToolCallOutcome::Completed { success: true };

#[test]
fn checkpoint_counts_successful_direct_calls_per_run() {
    let activity = RunActivity::default();
    activity.observe_run("run-1");
    let mut epochs = Vec::new();
    for _ in 0..(CHECKPOINT_TOOL_CALLS * 2) {
        activity.record(/*direct*/ true, /*stateful_tool*/ None, DONE);
        activity.record(/*direct*/ false, /*stateful_tool*/ None, DONE);
        activity.record(
            /*direct*/ true,
            /*stateful_tool*/ None,
            ToolCallOutcome::Completed { success: false },
        );
        epochs.push(activity.due_epoch());
    }
    assert_eq!(
        (epochs[6], epochs[7], epochs[14], epochs[15]),
        (None, Some(1), Some(1), Some(2))
    );

    activity.record(/*direct*/ true, Some("stateful_run_update"), DONE);
    assert_eq!(activity.due_epoch(), Some(2));
    activity.record(/*direct*/ true, Some("obligation_update"), DONE);
    assert_eq!(activity.due_epoch(), None);
}

#[test]
fn counts_reset_when_a_new_run_starts() {
    let activity = RunActivity::default();
    activity.observe_run("run-1");
    for _ in 0..CHECKPOINT_TOOL_CALLS {
        activity.record(/*direct*/ true, /*stateful_tool*/ None, DONE);
    }
    activity.observe_run("run-1");
    assert_eq!(activity.due_epoch(), Some(1));
    activity.observe_run("run-2");
    assert_eq!(activity.due_epoch(), None);
}
