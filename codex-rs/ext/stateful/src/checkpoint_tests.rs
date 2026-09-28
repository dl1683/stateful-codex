use codex_extension_api::ToolCallOutcome;
use pretty_assertions::assert_eq;

use super::CHECKPOINT_TOOL_CALLS;
use super::CheckpointCounter;

const DONE: ToolCallOutcome = ToolCallOutcome::Completed { success: true };

#[test]
fn checkpoint_epochs_advance_only_at_thresholds_and_reset_on_obligation() {
    let counter = CheckpointCounter::default();
    let mut epochs = Vec::new();
    for _ in 0..(CHECKPOINT_TOOL_CALLS * 2) {
        counter.record(/*stateful_tool*/ None, &DONE);
        epochs.push(counter.due_epoch());
    }
    assert_eq!(epochs[6], None);
    assert_eq!(epochs[7], Some(1));
    assert_eq!(epochs[14], Some(1));
    assert_eq!(epochs[15], Some(2));

    counter.record(
        Some("obligation_update"),
        &ToolCallOutcome::Completed { success: false },
    );
    assert_eq!(counter.due_epoch(), Some(2));
    counter.record(Some("obligation_update"), &DONE);
    assert_eq!(counter.due_epoch(), None);
}
