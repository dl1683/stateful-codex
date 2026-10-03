use pretty_assertions::assert_eq;

use super::background_statements;

#[test]
fn background_is_the_users_description_of_themselves_and_the_work() {
    let text = "Hi! I know Python well but only a little Rust. I'm not changing any code, just reading. From now on, explain Rust ideas with Python comparisons. Do you know tokio? The assistant said I'm a beginner.";
    assert_eq!(
        background_statements(text),
        vec![
            "I know Python well but only a little Rust.".to_string(),
            "I'm not changing any code, just reading.".to_string(),
        ]
    );
}

#[test]
fn quoted_fenced_and_relayed_text_is_not_background_and_requests_stay_requests() {
    let text = "She wrote:\n> I'm a doctor.\n```\nI'm a pilot.\n```\nMy notes:\nI am a nurse.\n\nI know Go well.";
    assert_eq!(
        (
            background_statements(text),
            crate::user_rules::asks_about_rules(
                "List the preferences I gave you for all our work."
            ),
            crate::user_rules::asks_about_rules(
                "From now on, repeat my instructions word for word."
            ),
        ),
        // The quoted, fenced and introduced lines are left out; the user's own line after
        // the blank line is theirs.
        (vec!["I know Go well.".to_string()], true, false)
    );
}

#[test]
fn background_skips_lines_that_quote_show_code_or_relay() {
    assert_eq!(
        (
            background_statements(
                "Translate this sentence: \"I know Python. I'm a beginner.\"\nShe wrote: I'm a doctor.\n`I'm a pilot.`\nI'm a teacher."
            ),
            crate::user_rules::asks_about_rules(
                "- List the preferences I gave you for all our work."
            ),
        ),
        // Only the user's own unquoted line is theirs.
        (vec!["I'm a teacher.".to_string()], true)
    );
}

#[test]
fn quoted_self_descriptions_are_never_background() {
    assert_eq!(
        [
            "Translate \u{2018}I know Python. I'm a beginner.\u{2019}",
            "Translate \"I know Python.\nI'm a beginner.\nI work as a nurse.\" into French.",
            "Translate 'I know Python.\nI'm a beginner.'",
            "I'm a teacher.",
        ]
        .map(background_statements),
        [
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["I'm a teacher.".to_string()],
        ]
    );
}

/// tui8: the user's self-description counts even though the same message relays a
/// colleague, and the colleague's quoted words never do.
#[test]
fn a_colleagues_quote_elsewhere_does_not_hide_the_users_background() {
    let message = "Hi! Quick intro since this is our first session together: I'm a backend developer, mostly Go for the last six years, so my Python is a bit rusty, and I maintain this humanize fork for our internal ops dashboards. Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself. Also FYI, Priya (she co-maintains the fork with me) wrote in our team chat: \"I'm a stickler, I know mypy well.\" She said I'm new to typing. I know pytest. Today I'd like a small helper.";
    assert_eq!(
        background_statements(message),
        vec![
            "I'm a backend developer, mostly Go for the last six years, so my Python is a bit rusty, and I maintain this humanize fork for our internal ops dashboards.".to_string(),
            "I know pytest.".to_string(),
        ]
    );
}

/// Speech reported without quotation marks, and anything after an unclosed single quote,
/// may be someone else's words; the user's own sentence before them is kept.
#[test]
fn reported_and_ambiguous_words_are_not_background() {
    assert_eq!(
        [
            "My colleague said I'm new to Rust.",
            "Quick note: I'm a nurse. She wrote: 'I'm a pilot and I know Go",
            "I'm a Rust beginner: mostly Python before.",
        ]
        .map(background_statements),
        [
            Vec::new(),
            vec!["I'm a nurse.".to_string()],
            vec!["I'm a Rust beginner: mostly Python before.".to_string()],
        ]
    );
}
