use pretty_assertions::assert_eq;

use codex_extension_api::PreviousWorldStateSection;
use codex_protocol::user_input::UserInput;
use serde_json::json;

use super::CONTINUITY_NOTE;
use super::RequestScope;
use super::SELF_CONTAINED_NOTE;
use super::classify;
use super::classify_input;
use super::request_scope_section;

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
        // Deictic words are ambiguous, so even a relative "that" keeps continuity.
        (
            "Add a unit test for parse_duration that covers negative inputs",
            RequestScope::Continuity,
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
            RequestScope::Continuity,
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
            RequestScope::Continuity,
        ),
        (
            "Add Windows path handling to path_utils.py too",
            RequestScope::Continuity,
        ),
        (
            "You have permission to edit recipes.json for the crepes",
            RequestScope::Continuity,
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
fn scope_note_renders_on_change_and_when_its_note_is_missing() {
    let render = |scope: RequestScope, previous: PreviousWorldStateSection<'_>| {
        request_scope_section(scope)
            .render_diff(previous)
            .map(|fragment| fragment.body().to_string())
    };
    let self_contained = json!({ "scope": "selfContained" });
    let continuity = json!({ "scope": "continuity" });
    assert_eq!(
        vec![
            render(
                RequestScope::SelfContained,
                PreviousWorldStateSection::Absent
            ),
            render(RequestScope::Continuity, PreviousWorldStateSection::Absent),
            render(
                RequestScope::SelfContained,
                PreviousWorldStateSection::Known(&self_contained)
            ),
            render(
                RequestScope::Continuity,
                PreviousWorldStateSection::Known(&self_contained)
            ),
            render(
                RequestScope::SelfContained,
                PreviousWorldStateSection::Known(&continuity)
            ),
            render(
                RequestScope::Continuity,
                PreviousWorldStateSection::Known(&continuity)
            ),
        ],
        vec![
            Some(SELF_CONTAINED_NOTE.to_string()),
            None,
            None,
            Some(CONTINUITY_NOTE.to_string()),
            Some(SELF_CONTAINED_NOTE.to_string()),
            None,
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
