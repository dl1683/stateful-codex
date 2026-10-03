use pretty_assertions::assert_eq;

use super::Quotations;

fn relayed(text: &str, clause: &str) -> bool {
    Quotations::new(text).relays_clause(clause)
}

#[test]
fn relayed_quotations_are_found_in_every_form() {
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
            (
                "My colleague wrote: \"I like Rust. Always write tests first.\"",
                "Always write tests first.\""
            ),
            (
                "My colleague wrote:\n\"Always write tests first.\"",
                "\"Always write tests first.\""
            ),
            (
                "\"Never touch docs.\" That is what my colleague wrote.",
                "\"Never touch docs.\""
            ),
            (
                "My colleague wrote: 'Never touch users' docs. Always write tests first.'",
                "Always write tests first.'"
            ),
            ("My colleague wrote: \"Never commit.\"", "My colleague wrote: \"Never commit.\""),
        ]
        .map(|(text, clause)| relayed(text, clause)),
        [true; 8]
    );
}

#[test]
fn the_users_own_rules_may_quote_terms() {
    assert_eq!(
        [
            "From now on, never use the phrase \"as I mentioned\".",
            "From now on, never use the word \"simply\" in docs.",
            "End each reply with a line starting with 'Next:'.",
            "I'm sure the users' notes are fine; never delete them.",
            "Never change files that are not mine.",
        ]
        .map(|text| relayed(text, text)),
        [false; 5]
    );
}
