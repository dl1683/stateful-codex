use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;

use super::UserMessage;
use super::UserMessageRegistry;

fn text(value: &str) -> Vec<UserInput> {
    vec![UserInput::Text {
        text: value.to_string(),
        text_elements: Vec::new(),
    }]
}

#[test]
fn log_resolves_a_quote_to_its_clause_within_the_project() {
    let registry = UserMessageRegistry::default();
    registry.record(
        "thread-1",
        "project-1",
        "turn-1",
        &text("Hi. Please never run git commit yourself. Thanks."),
        /*after_change*/ None,
    );
    registry.record(
        "thread-1",
        "project-1",
        "turn-1",
        &text("Hi. Please never run git commit yourself. Thanks."),
        /*after_change*/ None,
    );
    registry.record(
        "thread-1",
        "project-2",
        "turn-2",
        &text("Use uv for everything."),
        /*after_change*/ None,
    );
    registry.record(
        "thread-1",
        "project-1",
        "turn-3",
        &text("   "),
        /*after_change*/ None,
    );
    let first = UserMessage {
        project_id: "project-1".to_string(),
        turn_id: "turn-1".to_string(),
        text: "Hi. Please never run git commit yourself. Thanks.".to_string(),
        received_at_ms: registry
            .find("thread-1", "project-1", "never run git commit")
            .map_or(0, |(message, _)| message.received_at_ms),
        after_change: None,
    };
    assert_eq!(
        (
            registry.find("thread-1", "project-1", "never run git   commit"),
            registry.find("thread-1", "project-1", "Use uv"),
            registry.find("thread-1", "project-1", "yourself. Thanks"),
            registry.find("thread-2", "project-1", "never run git commit"),
        ),
        (
            Some((first, "Please never run git commit yourself.".to_string())),
            None,
            None,
            None,
        )
    );
}

#[test]
fn a_long_message_keeps_only_whole_lines() {
    let registry = UserMessageRegistry::default();
    let long = format!(
        "{}\nDo not run the whole test suite for this task.",
        "x".repeat(16_352)
    );
    registry.record(
        "thread-1",
        "project-1",
        "turn-1",
        &text(&long),
        /*after_change*/ None,
    );
    assert_eq!(
        registry.find("thread-1", "project-1", "whole test suite"),
        None
    );
}

/// Review round 1: a message over the bound keeps only whole units, so a rule never loses an
/// indented qualifier that did not fit.
#[test]
fn a_long_message_never_keeps_an_item_without_its_continuation() {
    let registry = UserMessageRegistry::default();
    let filler = "word ".repeat(3_000);
    let qualifier = format!("  Only {}for this task only.", "very ".repeat(2_000));
    let message = format!("{filler}\nStanding rules:\n- Never touch docs.\n{qualifier}");
    registry.record(
        "thread-1",
        "project-1",
        "turn-1",
        &text(&message),
        /*after_change*/ None,
    );
    assert_eq!(
        (
            registry
                .find("thread-1", "project-1", "Never touch docs")
                .is_some(),
            registry
                .find("thread-1", "project-1", "Standing rules")
                .is_some(),
        ),
        (false, true)
    );
}
