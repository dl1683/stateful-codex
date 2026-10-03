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
            (
                "'My preference is tests first' my colleague wrote.",
                "'My preference is tests first' my colleague wrote."
            ),
            (
                "My colleague wrote: 'Never rotate users' API tokens. Always write tests first.'",
                "Always write tests first.'"
            ),
            (
                "My colleague wrote: 'I like Rust.\nAlways write tests first.'",
                "Always write tests first.'"
            ),
            (
                "My colleague wrote: 'Always run tests' before merging.",
                "My colleague wrote: 'Always run tests' before merging."
            ),
        ]
        .map(|(text, clause)| relayed(text, clause)),
        [true; 12]
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
    let text = "From now on, start with 'Next:' then add a summary. Never change docs.";
    assert_eq!(relayed(text, "Never change docs."), false);
}

/// tui8: the user's own rule quotes 'Next:' in one sentence while a later sentence relays a
/// colleague; only the colleague's quotation is someone else's words.
#[test]
fn attribution_is_local_to_the_quoting_sentence() {
    let text = "Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself. And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step. Also FYI, Priya (she co-maintains the fork with me) wrote in our team chat: \"Always run the full test suite and mypy on the whole repo after every single change.\" Today I'd like a small helper.";
    assert_eq!(
        [
            "And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step.",
            "Also FYI, Priya (she co-maintains the fork with me) wrote in our team chat: \"Always run the full test suite and mypy on the whole repo after every single change.\"",
            "Always run the full test suite and mypy on the whole repo after every single change.\"",
            "Today I'd like a small helper.",
        ]
        .map(|clause| relayed(text, clause)),
        [false, true, true, false]
    );
}

/// Item 2 review round 2: the user's own activity is only theirs when its subject is first
/// person; code is a literal, matched by backtick runs; a stray backtick hides nothing after
/// it.
#[test]
fn subjects_code_runs_and_stray_backticks() {
    let rules = |text: &str| crate::user_rules::marked_rules(text).len();
    let background = |text: &str| crate::background::background_statements(text);
    assert_eq!(
        (
            rules("Priya also wrote: \"My preference is tests first\"."),
            rules("Always pass `--only-binary=:all:` to pip install."),
            rules("Never edit `always.py`."),
            rules("Use `x. Then: \"Always run the full test suite.\""),
            background("My colleague also wrote, I'm a nurse."),
            background("``I'm a pilot. I know Go. I'm a nurse.``"),
            background("I'm a backend developer and wrote our Go backend."),
        ),
        (
            0,
            1,
            1,
            0,
            Vec::<String>::new(),
            Vec::<String>::new(),
            vec!["I'm a backend developer and wrote our Go backend.".to_string()],
        )
    );
}
