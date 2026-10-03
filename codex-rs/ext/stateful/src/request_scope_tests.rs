use pretty_assertions::assert_eq;

use codex_extension_api::PreviousWorldStateSection;
use codex_protocol::user_input::UserInput;
use serde_json::json;

use super::END_MARKER;
use super::LEGACY_RETIREMENT;
use super::MAX_WINDOW_NOTE_BYTES;
use super::RequestHead;
use super::RequestScope;
use super::SELF_CONTAINED_NOTE;
use super::START_MARKER;
use super::ScopeNotePlan;
use super::WIDENED_NOTE;
use super::classify;
use super::classify_input;

#[test]
fn classifies_requests_by_their_reference_to_earlier_work() {
    let cases = [
        (
            "In `src/click/core.py`, fix the off-by-one index calculation in `format_commands` at line 140.",
            RequestScope::SelfContained,
        ),
        (
            "Add a unit test for parse_duration covering negative inputs",
            RequestScope::SelfContained,
        ),
        // A deictic word in a request that names its own subject points at that subject.
        (
            "Add a unit test for parse_duration that covers negative inputs",
            RequestScope::SelfContained,
        ),
        (
            "Rename the helper in utils.py to snake_case and update its callers",
            RequestScope::SelfContained,
        ),
        (
            "Implement yesterday's second option in src/click/core.py",
            RequestScope::Continuity,
        ),
        ("yes, as proposed", RequestScope::Continuity),
        (
            "Yes please add the crepes recipe to recipes.json now",
            RequestScope::Continuity,
        ),
        (
            "Morning! Where were we on naturalrate?",
            RequestScope::Continuity,
        ),
        (
            "Add another supported per unit for weeks to rate.py",
            RequestScope::SelfContained,
        ),
        (
            "What do you remember about me and my working style?",
            RequestScope::Continuity,
        ),
        (
            "Requirement changed: default formatting must now use TWO decimal places",
            RequestScope::Continuity,
        ),
        (
            "Summarize this week's decisions with the why and open items",
            RequestScope::Continuity,
        ),
        (
            "finish whatever is left, including anything I noted down myself",
            RequestScope::Continuity,
        ),
        ("", RequestScope::Continuity),
        ("fix it", RequestScope::Continuity),
        (
            "Implement the second approach in parser.rs",
            RequestScope::Continuity,
        ),
        (
            "Make it handle Windows paths in path_utils.py",
            RequestScope::SelfContained,
        ),
        (
            "Add Windows path handling to path_utils.py too",
            RequestScope::SelfContained,
        ),
        (
            "You have permission to edit recipes.json for the crepes",
            RequestScope::Continuity,
        ),
        (
            "Implement your recommended fix in parser.py",
            RequestScope::Continuity,
        ),
        (
            "Fix the bug we identified in parser.rs",
            RequestScope::Continuity,
        ),
        (
            "Apply the change you outlined to config loading in settings.py",
            RequestScope::Continuity,
        ),
        (
            "Implement the patch you described in parser.rs",
            RequestScope::Continuity,
        ),
        // Shared work named explicitly stays continuity next to a named file.
        ("Apply your approach to number.py", RequestScope::Continuity),
        (
            "Fix the issue you found in parser.rs",
            RequestScope::Continuity,
        ),
        // Without a named subject a deictic word points at earlier work.
        (
            "Please fix that bug in the parser module",
            RequestScope::Continuity,
        ),
        (
            "Now do the same for the other functions too",
            RequestScope::Continuity,
        ),
        // Field requests from the horizon runs that name their own subject.
        (
            "Bug report from finance: ordinal(-3) comes out as \"-3th\" on the ledger page. Negative numbers should get the right suffix, like \"-3rd\", \"-11th\", \"-22nd\". Please fix it.",
            RequestScope::SelfContained,
        ),
        (
            "The dashboard team wants numbers to fit narrow cells too, not just durations. Look at number.py and give me your three best ideas for compact number output, ranked, with a sentence on the tradeoff of each. Don't implement anything yet.",
            RequestScope::SelfContained,
        ),
        (
            "Finance found another one: intcomma(-0.5, 0) shows \"-0\" in the quarterly report. A negative zero makes no sense to them. Can you fix that?",
            RequestScope::SelfContained,
        ),
        (
            "time.py is getting long and the compact duration code is spread around in it. Pull the compact-formatting pieces into their own private module so precisedelta and naturaldelta share one implementation. No behaviour change.",
            RequestScope::SelfContained,
        ),
        (
            "New request from the reports team: natural_list should be able to say \"or\" instead of \"and\", and optionally use an Oxford comma (\"a, b, and c\"). Current output must stay the default.",
            RequestScope::SelfContained,
        ),
        (
            "Saw this in a log: intword(10**110) printed \"10000000000.0 googol\". That's silly. Make huge numbers past a googol come out sensibly.",
            RequestScope::SelfContained,
        ),
    ];
    let actual = cases
        .iter()
        .map(|(text, _)| (*text, classify(text)))
        .collect::<Vec<_>>();
    assert_eq!(actual, cases.to_vec());
}

#[test]
fn non_text_input_keeps_continuity() {
    let text = UserInput::Text {
        text: "Rename the helper in utils.py to snake_case and update its callers".to_string(),
        text_elements: Vec::new(),
    };
    let mention = UserInput::Mention {
        name: "notes".to_string(),
        path: "notes.md".to_string(),
    };
    assert_eq!(
        (
            classify_input(std::slice::from_ref(&text)),
            classify_input(&[text, mention]),
            classify_input(&[]),
        ),
        (
            RequestScope::SelfContained,
            RequestScope::Continuity,
            RequestScope::Continuity,
        )
    );
}

#[test]
fn scope_notes_name_their_request_and_count_against_the_window() {
    let head = RequestHead("Rename the <helper> in utils.py".to_string());
    let render = |plan: ScopeNotePlan, previous: PreviousWorldStateSection<'_>| {
        let bytes = plan.window_bytes;
        let body = plan
            .section()
            .render_diff(previous)
            .map(|fragment| fragment.body().to_string());
        (body, bytes)
    };
    let note = format!(
        "Scope note for the request that begins \"Rename the \\u003chelper\\u003e in utils.py\": it {SELF_CONTAINED_NOTE}"
    );
    let note_bytes = START_MARKER.len() + note.len() + END_MARKER.len();
    let widened = format!(
        "Scope note for the request that begins \"Rename the \\u003chelper\\u003e in utils.py\": it {WIDENED_NOTE}"
    );
    let widened_bytes = START_MARKER.len() + widened.len() + END_MARKER.len();
    let first_step = json!({ "scope": "selfContained", "turnId": "turn-1", "noted": true, "windowBytes": note_bytes });
    let next_turn = json!({ "scope": "continuity", "turnId": "turn-2", "noted": false, "windowBytes": note_bytes });

    assert_eq!(
        vec![
            // A self-contained turn's first step renders its note.
            render(
                ScopeNotePlan::new(None, "turn-1", RequestScope::SelfContained, Some(&head)),
                PreviousWorldStateSection::Absent
            ),
            // Later steps of the same turn add nothing.
            render(
                ScopeNotePlan::new(
                    Some(&first_step),
                    "turn-1",
                    RequestScope::SelfContained,
                    Some(&head)
                ),
                PreviousWorldStateSection::Known(&first_step)
            ),
            // Steering widens the same request.
            render(
                ScopeNotePlan::new(
                    Some(&first_step),
                    "turn-1",
                    RequestScope::Continuity,
                    Some(&head)
                ),
                PreviousWorldStateSection::Known(&first_step)
            ),
            // A continuity turn adds nothing; the earlier note names its own request.
            render(
                ScopeNotePlan::new(Some(&first_step), "turn-2", RequestScope::Continuity, None),
                PreviousWorldStateSection::Known(&first_step)
            ),
            // A later self-contained turn gets its own note, even when an interrupted write
            // left an older continuity snapshot behind.
            render(
                ScopeNotePlan::new(
                    Some(&next_turn),
                    "turn-3",
                    RequestScope::SelfContained,
                    Some(&head)
                ),
                PreviousWorldStateSection::Known(&next_turn)
            ),
        ],
        vec![
            (Some(note.clone()), note_bytes),
            (None, note_bytes),
            (Some(widened), note_bytes + widened_bytes),
            (None, note_bytes),
            (Some(note), note_bytes * 2),
        ]
    );
}

#[test]
fn steering_widens_a_self_contained_turn_and_nothing_narrows_it() {
    let text = |text: &str| {
        vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]
    };
    let turn = codex_extension_api::ExtensionData::new("turn-1");
    let unclassified = RequestScope::of_turn(&turn);
    RequestScope::record_turn_start(
        &turn,
        &text("Rename the helper in utils.py to snake_case and update its callers"),
    );
    let started = RequestScope::of_turn(&turn);
    RequestScope::observe_user_message(
        &turn,
        &text("Rename the helper in utils.py to snake_case and update its callers"),
    );
    let after_restated = RequestScope::of_turn(&turn);
    RequestScope::observe_user_message(&turn, &text("use the name we decided yesterday"));
    let after_steer = RequestScope::of_turn(&turn);
    RequestScope::observe_user_message(
        &turn,
        &text("Rename the helper in utils.py to snake_case and update its callers"),
    );
    assert_eq!(
        vec![
            unclassified,
            started,
            after_restated,
            after_steer,
            RequestScope::of_turn(&turn)
        ],
        vec![
            RequestScope::Continuity,
            RequestScope::SelfContained,
            RequestScope::SelfContained,
            RequestScope::Continuity,
            RequestScope::Continuity,
        ]
    );
}

#[test]
fn notes_share_a_bounded_reserve_and_legacy_restrictions_are_retired() {
    let head = RequestHead("Rename the helper in utils.py".to_string());
    // Several narrow turns in one window: notes stop once the reserve is used, while the
    // turns stay self-contained (the record stays deferred).
    let mut previous: Option<serde_json::Value> = None;
    let mut noted = Vec::new();
    for turn in 1..=4 {
        let plan = ScopeNotePlan::new(
            previous.as_ref(),
            &format!("turn-{turn}"),
            RequestScope::SelfContained,
            Some(&head),
        );
        assert!(
            plan.window_bytes <= MAX_WINDOW_NOTE_BYTES,
            "{}",
            plan.window_bytes
        );
        let section = plan.section();
        noted.push(section.snapshot()["noted"].as_bool());
        previous = Some(section.snapshot().clone());
    }
    assert_eq!(noted, vec![Some(true), Some(true), Some(true), Some(false)]);

    // A note from before notes named their request is retired once, by any later turn.
    let legacy = json!({ "scope": "selfContained" });
    let body = ScopeNotePlan::new(Some(&legacy), "turn-9", RequestScope::Continuity, None)
        .section()
        .render_diff(PreviousWorldStateSection::Known(&legacy))
        .map(|fragment| fragment.body().to_string());
    assert_eq!(body, Some(LEGACY_RETIREMENT.to_string()));
}

#[test]
fn steered_narrow_turns_stay_within_the_reserve_and_notes_survive_a_new_window() {
    let head = RequestHead(
        "Rename the helper in utils.py to snake_case and update its callers".to_string(),
    );
    let mut previous: Option<serde_json::Value> = None;
    let mut charges = Vec::new();
    for turn in 1..=3 {
        let turn_id = format!("turn-{turn}");
        // Each narrow turn is steered to continuity at its next step.
        for scope in [RequestScope::SelfContained, RequestScope::Continuity] {
            let plan = ScopeNotePlan::new(previous.as_ref(), &turn_id, scope, Some(&head));
            charges.push(plan.window_bytes);
            previous = Some(plan.section().snapshot().clone());
        }
    }
    assert!(
        charges.iter().all(|bytes| *bytes <= MAX_WINDOW_NOTE_BYTES),
        "{charges:?}"
    );

    // A later step of the same narrow turn re-plans its note: nothing new is charged, and
    // when compaction removed the fragment the note is rendered again.
    let first = ScopeNotePlan::new(None, "turn-1", RequestScope::SelfContained, Some(&head));
    let first_bytes = first.window_bytes;
    let first_snapshot = first.section().snapshot().clone();
    let replanned = ScopeNotePlan::new(
        Some(&first_snapshot),
        "turn-1",
        RequestScope::SelfContained,
        Some(&head),
    );
    assert_eq!(replanned.window_bytes, first_bytes);
    let section = replanned.section();
    assert_eq!(
        (
            section
                .render_diff(PreviousWorldStateSection::Known(&first_snapshot))
                .is_some(),
            section
                .render_diff(PreviousWorldStateSection::Absent)
                .is_some(),
        ),
        (false, true)
    );
}
