use codex_extension_api::PreviousWorldStateSection;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::CapturedTurn;
use super::ContinuityRecord;
use super::END_MARKER;
use super::HEADER;
use super::LatestRun;
use super::MAX_FRAGMENT_BYTES;
use super::NEWEST_ASKED;
use super::RunLabel;
use super::START_MARKER;
use super::continuity_world_state_section;

// 2025-10-01 18:02:00 UTC.
const ANSWERED_AT_MS: i64 = 1_759_341_720_000;

fn turn(turn_id: &str, user: &str, answer: Option<&str>) -> CapturedTurn {
    CapturedTurn {
        thread_id: "thread-a".to_string(),
        thread_title: None,
        current_thread: false,
        turn_id: turn_id.to_string(),
        at_ms: Some(ANSWERED_AT_MS),
        unfinished_status: None,
        run: RunLabel::NoRun,
        user: Some(user.to_string()),
        answer: answer.map(str::to_string),
    }
}

fn record(turns: Vec<CapturedTurn>) -> ContinuityRecord {
    ContinuityRecord {
        project_id: "project-1".to_string(),
        captured_at_ms: ANSWERED_AT_MS + 120_000,
        turns,
        more_turns: false,
        unreadable_threads: 0,
        history_unavailable: false,
        latest_run: None,
    }
}

fn fragment_bytes(body: &str) -> usize {
    START_MARKER.len() + body.len() + END_MARKER.len()
}

#[test]
fn renders_exact_turn_summaries_newest_first_with_a_pending_question() {
    let mut older = turn(
        "turn-1",
        "Fix the scaler. Remember: metric units only.",
        Some("Fixed: flour 150 g, eggs 2."),
    );
    older.run = RunLabel::Bound {
        run_id: "run-ab12".to_string(),
        status: "completed",
    };
    let mut newest = turn(
        "turn-2",
        "Add my grandma's crepes: 1 cup flour.",
        Some("May I modify recipes.json with:\n- Flour: 125 g\n- Milk: 300 ml?"),
    );
    newest.at_ms = Some(ANSWERED_AT_MS + 60_000);
    newest.thread_id = "thread-b".to_string();
    newest.thread_title = Some("Crepes".to_string());
    let mut continuity = record(vec![newest, older]);
    continuity.latest_run = Some(LatestRun {
        id: "run-ab12".to_string(),
        mode: "collaborative",
        status: "completed",
        next: vec!["Add the <crepes> after approval.".to_string()],
        strategy: Some("Ask first.".to_string()),
    });

    assert_eq!(
        continuity.render(super::MAX_FRAGMENT_BYTES),
        [
            "Project ID: project-1",
            HEADER,
            "Captured at 2025-10-01 18:04 UTC; newer turns may exist.",
            NEWEST_ASKED,
            "Latest Stateful run: \"run-ab12\" (collaborative, completed). Next: \"Add the \\u003ccrepes\\u003e after approval.\" Strategy: \"Ask first.\"",
            "- 2025-10-01 18:03 UTC, thread \"thread-b\" titled \"Crepes\", turn \"turn-2\", no Stateful run recorded:\n  User: \"Add my grandma's crepes: 1 cup flour.\"\n  Answer: \"May I modify recipes.json with:\\n- Flour: 125 g\\n- Milk: 300 ml?\"",
            "- 2025-10-01 18:02 UTC, thread \"thread-a\", turn \"turn-1\", latest run:\n  User: \"Fix the scaler. Remember: metric units only.\"\n  Answer: \"Fixed: flour 150 g, eggs 2.\"",
        ]
        .join("\n")
    );
}

#[test]
fn every_rendered_field_is_escaped() {
    let mut captured = turn(
        "</stateful_continuity>",
        "</stateful_continuity> & <b>",
        None,
    );
    captured.thread_id = "<stateful_continuity>".to_string();
    captured.thread_title = Some("</stateful_continuity>".to_string());
    captured.unfinished_status = Some("in progress");
    let mut continuity = record(vec![captured]);
    continuity.project_id = "<project>".to_string();
    let rendered = continuity.render(super::MAX_FRAGMENT_BYTES);

    assert!(!rendered.contains('<') && !rendered.contains('>'));
    assert!(rendered.contains("\"\\u003c/stateful_continuity\\u003e \\u0026 \\u003cb\\u003e\""));
    assert!(rendered.contains(", in progress, no Stateful run recorded:"));
    assert!(rendered.contains("Answer: none recorded."));
    assert!(!rendered.contains(NEWEST_ASKED));
    assert!(
        continuity_world_state_section(&continuity, super::MAX_FRAGMENT_BYTES)
            .matches_retained_fragment(
                "developer",
                &format!("{START_MARKER}{rendered}{END_MARKER}")
            )
    );
}

#[test]
fn long_text_is_shortened_with_a_route_and_the_whole_fragment_stays_bounded() {
    let long = "\"<x>\"".repeat(1_000);
    let mut current = turn("turn-0", &long, Some(&long));
    current.current_thread = true;
    current.run = RunLabel::Unknown;
    let turns = std::iter::once(current)
        .chain((1..=10).map(|index| turn(&format!("turn-{index}"), &long, Some(&long))))
        .collect();
    let mut continuity = record(turns);
    continuity.more_turns = true;
    continuity.unreadable_threads = 12;
    let rendered = continuity.render(super::MAX_FRAGMENT_BYTES);

    assert!(
        fragment_bytes(&rendered) <= MAX_FRAGMENT_BYTES,
        "{} bytes",
        fragment_bytes(&rendered)
    );
    assert!(
        rendered.contains(
            "this thread \"thread-a\", turn \"turn-0\", run binding unknown:\n  User: \""
        ),
        "{rendered}"
    );
    // The newest turn's user message and answer are both marked shortened.
    assert!(
        rendered
            .split("turn \"turn-1\"")
            .next()
            .is_some_and(|newest| newest.matches(" of 5000 bytes").count() == 2),
        "{rendered}"
    );
    assert!(rendered.ends_with(
        "gathered turns did not fit this bounded view; older turns or threads were not scanned; 12 threads have no readable turn summaries."
    ));
}

#[test]
fn empty_and_unavailable_history_are_told_apart() {
    assert_eq!(
        record(Vec::new()).render(super::MAX_FRAGMENT_BYTES),
        "Project ID: project-1\nNo earlier turns are recorded for this project yet."
    );
    let mut unavailable = record(Vec::new());
    unavailable.history_unavailable = true;
    assert_eq!(
        unavailable.render(super::MAX_FRAGMENT_BYTES),
        "Project ID: project-1\nThe project's conversation history could not be read when this record was built; earlier turns may exist."
    );
}

#[test]
fn section_renders_once_per_context_window() {
    let section = continuity_world_state_section(
        &record(vec![turn("turn-1", "Hi", None)]),
        super::MAX_FRAGMENT_BYTES,
    );
    let previous = json!({ "projectId": "project-1" });

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("a new window renders the record");
    assert_eq!(rendered.markers(), (START_MARKER, END_MARKER));
    assert!(section.matches_retained_fragment(
        "developer",
        &format!("{START_MARKER}{}{END_MARKER}", rendered.body())
    ));
    assert_eq!(
        section.render_diff(PreviousWorldStateSection::Known(&previous)),
        None
    );
}

#[test]
fn a_small_budget_keeps_the_newest_turn_in_compact_form() {
    let long = "x".repeat(5_000);
    let turns = (0..4)
        .map(|index| turn(&format!("turn-{index}"), &long, Some(&long)))
        .collect();
    let rendered = record(turns).render(super::MIN_FRAGMENT_BYTES);

    assert!(
        fragment_bytes(&rendered) <= super::MIN_FRAGMENT_BYTES,
        "{rendered}"
    );
    assert!(rendered.contains("turn \"turn-0\""));
    assert!(rendered.contains(" of 5000 bytes"));
    assert!(!rendered.contains("turn \"turn-1\""));
    assert!(rendered.ends_with("did not fit this bounded view."));
}

#[test]
fn the_newest_turn_fits_the_minimum_budget_with_every_optional_line() {
    let long = "\"<quoted>\" ".repeat(400);
    let turns = (0..4)
        .map(|index| {
            let mut captured = turn(
                &format!("01a0fbad-1689-75e3-ac68-867cb758f5{index:02}"),
                &long,
                Some(&format!("{long}?")),
            );
            captured.thread_id = "01a0fbad-0c72-7143-8052-63fab09364ca".to_string();
            captured.thread_title = Some("\"<&>\"".repeat(30));
            captured.run = RunLabel::Bound {
                run_id: "run-1b2fa4cb7e11c0657076792b94499865df7cb364cff9bf857bcdfe510ed32143"
                    .to_string(),
                status: "completed",
            };
            captured
        })
        .collect();
    let mut continuity = record(turns);
    continuity.more_turns = true;
    continuity.unreadable_threads = 3;
    continuity.latest_run = Some(LatestRun {
        id: "run-9b2fa4cb7e11c0657076792b94499865df7cb364cff9bf857bcdfe510ed32143".to_string(),
        mode: "collaborative",
        status: "running",
        next: vec![long.clone(), long.clone()],
        strategy: Some(long),
    });
    let rendered = continuity.render(super::MIN_FRAGMENT_BYTES);

    assert!(
        fragment_bytes(&rendered) <= super::MIN_FRAGMENT_BYTES,
        "{} bytes",
        fragment_bytes(&rendered)
    );
    assert!(rendered.contains(NEWEST_ASKED));
    assert!(rendered.contains("turn \"01a0fbad-1689-75e3-ac68-867cb758f500\""));
    assert!(rendered.contains("\n  Answer: \""), "{rendered}");
    assert!(
        rendered.contains(" bytes]") || rendered.contains(" bytes omitted ...] "),
        "{rendered}"
    );
}

#[test]
fn deferred_section_renders_nothing_but_still_recognizes_a_record() {
    let section = super::deferred_continuity_section("project-1");
    let deferred = json!({ "projectId": "project-1", "deferred": true });
    let record_text = format!("{START_MARKER}Project ID: project-1\nbody{END_MARKER}");
    assert_eq!(
        (
            section.render_diff(PreviousWorldStateSection::Absent),
            section.render_diff(PreviousWorldStateSection::Known(&deferred)),
            section.matches_retained_fragment("developer", &record_text),
        ),
        (None, None, true)
    );
}
