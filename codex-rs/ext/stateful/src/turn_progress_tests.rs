use super::*;
use pretty_assertions::assert_eq;

#[test]
fn progress_receipts_stay_turn_and_run_bound_and_are_consumed_once() {
    let progress = TurnProgress::default();
    let first = StatefulRunId::parse("first").expect("run");
    let second = StatefulRunId::parse("second").expect("run");
    progress.record("turn-1", &first);
    progress.record("turn-2", &second);
    assert_eq!(progress.take("another-turn"), None);
    assert_eq!(progress.take("turn-1"), Some(first));
    assert_eq!(progress.take("turn-1"), None);
    assert_eq!(progress.take("turn-2"), Some(second));
}

#[test]
fn progress_registry_evicts_old_turns_and_replaces_a_rebound_turn() {
    let progress = TurnProgress::default();
    let first = StatefulRunId::parse("first").expect("run");
    let second = StatefulRunId::parse("second").expect("run");
    for index in 0..65 {
        progress.record(&format!("turn-{index}"), &first);
    }
    assert_eq!(progress.take("turn-0"), None);
    progress.record("turn-1", &second);
    assert_eq!(progress.take("turn-1"), Some(second));
    assert_eq!(progress.take("turn-64"), Some(first));
}
