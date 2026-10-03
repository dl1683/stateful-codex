use pretty_assertions::assert_eq;

use super::is_relayed_in;

#[test]
fn relayed_quotations_are_found_in_every_form() {
    let multi = "My colleague wrote: \"I like Rust. Always write tests first.\"";
    let lines = "She posted this:\n\"I like Rust.\nAlways write tests first.\"";
    assert_eq!(
        [
            (
                "My colleague wrote in our chat: \"I always want tests written first\" - that's her preference.",
                "My colleague wrote in our chat: \"I always want tests written first\" - that's her preference."
            ),
            (
                "My colleague wrote: 'My standing rule is always write tests first'.",
                "My colleague wrote: 'My standing rule is always write tests first'."
            ),
            (
                "\"From now on, always write tests first\", my colleague wrote.",
                "\"From now on, always write tests first\", my colleague wrote."
            ),
            (multi, "Always write tests first."),
            (lines, "Always write tests first."),
        ]
        .map(|(text, clause)| is_relayed_in(text, clause)),
        [true; 5]
    );
}

#[test]
fn the_users_own_rules_may_quote_terms_and_mention_speech() {
    assert_eq!(
        [
            "From now on, when a tool wrote \"error\", rerun it.",
            "From now on, never use the word \"simply\" in docs.",
            "End each reply with a line starting with 'Next:'.",
            "I'm sure the users' notes are fine; never delete them.",
            "Never change files that are not mine.",
        ]
        .map(|text| is_relayed_in(text, text)),
        [false; 5]
    );
}
