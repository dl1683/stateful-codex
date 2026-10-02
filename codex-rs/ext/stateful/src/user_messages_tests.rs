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
    );
    registry.record(
        "thread-1",
        "project-1",
        "turn-1",
        &text("Hi. Please never run git commit yourself. Thanks."),
    );
    registry.record(
        "thread-1",
        "project-2",
        "turn-2",
        &text("Use uv for everything."),
    );
    registry.record("thread-1", "project-1", "turn-3", &text("   "));
    let first = UserMessage {
        project_id: "project-1".to_string(),
        turn_id: "turn-1".to_string(),
        text: "Hi. Please never run git commit yourself. Thanks.".to_string(),
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
    registry.record("thread-1", "project-1", "turn-1", &text(&long));
    assert_eq!(
        registry.find("thread-1", "project-1", "whole test suite"),
        None
    );
}
