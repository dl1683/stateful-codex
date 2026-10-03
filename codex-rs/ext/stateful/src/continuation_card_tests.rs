use pretty_assertions::assert_eq;

use super::focus_on_request;
use crate::continuity::CapturedTurn;
use crate::continuity::ContinuityRecord;
use crate::continuity::RunLabel;

fn turn(turn_id: &str, user: &str, answer: &str) -> CapturedTurn {
    CapturedTurn {
        thread_id: "thread-a".to_string(),
        thread_title: None,
        current_thread: false,
        turn_id: turn_id.to_string(),
        at_ms: Some(1_759_341_720_000),
        unfinished_status: None,
        run: RunLabel::NoRun,
        user: Some(user.to_string()),
        answer: Some(answer.to_string()),
    }
}

/// Newest first, as gathered: two recent turns, then older work on several subjects.
fn record() -> ContinuityRecord {
    ContinuityRecord {
        project_id: "project-1".to_string(),
        captured_at_ms: 1_759_341_720_000,
        turns: vec![
            turn("t6", "Fix the ordinal suffix for 11.", "Fixed: 11th."),
            turn(
                "t5",
                "Extract the compact helpers.",
                "Moved them to _compact.py.",
            ),
            turn(
                "t4",
                "Add signed durations.",
                "Added signed=False to precisedelta.",
            ),
            turn(
                "t3",
                "Brainstorm compact number formatting ideas.",
                "Top idea: width-aware compact(value, max_chars).",
            ),
            turn(
                "t2",
                "Why do the intcomma tests fail?",
                "A locale fixture leaked.",
            ),
            turn("t1", "Set up the project.", "Created the virtualenv."),
        ],
        more_turns: false,
        unrelated_omitted: 0,
        unreadable_threads: 0,
        history_unavailable: false,
        latest_run: None,
    }
}

fn kept(request: Option<&str>) -> (Vec<String>, usize) {
    let mut record = record();
    focus_on_request(&mut record, request);
    (
        record.turns.into_iter().map(|turn| turn.turn_id).collect(),
        record.unrelated_omitted,
    )
}

#[test]
fn a_continuation_keeps_recent_and_related_turns_and_counts_the_rest() {
    let ids = |ids: &[&str]| ids.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        [
            kept(Some(
                "Build the top idea from the compact formatting brainstorm."
            )),
            kept(Some("Continue where we left off, please.")),
            kept(Some("Apply your fix to the intcomma tests.")),
            kept(None),
        ],
        [
            (ids(&["t6", "t5", "t4", "t3"]), 2),
            (ids(&["t6", "t5", "t4", "t3", "t2", "t1"]), 0),
            (ids(&["t6", "t5", "t4", "t3", "t2", "t1"]), 0),
            (ids(&["t6", "t5", "t4", "t3", "t2", "t1"]), 0),
        ]
    );
}

#[test]
fn omitted_turns_are_disclosed_with_their_retrieval_route() {
    let mut record = record();
    focus_on_request(
        &mut record,
        Some("Build the top idea from the compact formatting brainstorm."),
    );
    let body = record.render(8 * 1024);
    assert!(
        body.contains(
            "Not shown: 2 older turns unrelated to this request. conversation_read lists and returns them."
        ),
        "{body}"
    );
}
