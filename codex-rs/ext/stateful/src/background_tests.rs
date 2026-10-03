use pretty_assertions::assert_eq;

use super::background_statements;

#[test]
fn background_is_the_users_description_of_themselves_and_the_work() {
    let text = "Hi! I know Python well but only a little Rust. I'm not changing any code, just reading. From now on, explain Rust ideas with Python comparisons. Do you know tokio? The assistant said I'm a beginner.";
    assert_eq!(
        background_statements(text),
        // "I'm not changing any code" describes this piece of work, not the user: it is not
        // kept as lasting background.
        vec!["I know Python well but only a little Rust.".to_string()]
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

/// Review round 1: inline code and guillemets hold someone's words across sentences; a
/// framing naming another source makes the rest theirs; the user's own past activity
/// ("wrote our Go backend") is not reported speech.
#[test]
fn code_guillemets_sources_and_the_users_own_activity() {
    assert_eq!(
        [
            "`I'm a pilot. I know Go. I'm a nurse.`",
            "Translate \u{ab}I'm a pilot. I know Go. I'm a nurse.\u{bb}",
            "From the docs: I'm a nurse.",
            "My colleague says: I'm a nurse.",
            "I'm a backend developer and wrote our Go backend.",
        ]
        .map(background_statements),
        [
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["I'm a backend developer and wrote our Go backend.".to_string()],
        ]
    );
}

/// Item 2 review round 3: items under the user's own "About me:" header and a self-statement
/// naming code are the user's background; a self-description with project wording is not a
/// rule.
#[test]
fn about_me_lists_code_terms_and_scope_wording() {
    let message = "About me:\n- Backend developer, mostly Go.\n- Rusty Python.\n\nI know `Go` well. I'm a backend developer for this project.";
    assert_eq!(
        (
            background_statements(message),
            crate::user_rules::marked_rules("I'm a backend developer for this project."),
        ),
        (
            vec![
                "Backend developer, mostly Go.".to_string(),
                "Rusty Python.".to_string(),
                "I know `Go` well.".to_string(),
                "I'm a backend developer for this project.".to_string(),
            ],
            Vec::new(),
        )
    );
}

/// research1: a colon-framed self-description and the sentence after it are both kept.
#[test]
fn a_framed_self_description_is_kept() {
    assert_eq!(
        background_statements(
            "Some background: I'm an ML engineer moving into research on LLM scaling and capabilities. I know transformers and training well, but I don't know this literature yet, so pitch explanations at that level.\n\nGround rules for this whole project, in every session from now on:\n1. Cite the paper (arXiv id) and the section for every claim."
        ),
        vec![
            "I'm an ML engineer moving into research on LLM scaling and capabilities.".to_string(),
            "I know transformers and training well, but I don't know this literature yet, so pitch explanations at that level.".to_string(),
        ]
    );
}
