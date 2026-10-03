use pretty_assertions::assert_eq;

use super::RuleClause;
use super::RuleStanding;
use super::clause_containing;
use super::marked_rules;

fn standing(text: &str) -> RuleClause {
    RuleClause {
        text: text.to_string(),
        standing: RuleStanding::Standing,
    }
}

/// The tui7 opening message: rules in prose, plus a one-off that must not become a rule.
#[test]
fn prose_rules_are_captured_and_one_offs_are_not() {
    let message = "Morning! I'm picking up work on humanize today (this checkout is my fork). Before writing anything I'd like you to get oriented: how the package is laid out. A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history. Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed; I'll ask when I want the full run. The test env is the venv one level up (../venv); its python has everything installed and it's already first on PATH. No code changes yet, just the exploration and the plan. Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.";
    assert_eq!(
        marked_rules(message),
        vec![
            standing(
                "A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history."
            ),
            standing(
                "Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed; I'll ask when I want the full run."
            ),
            standing(
                "Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take."
            ),
        ]
    );
}

/// The hand4 and know1 openings: a header introduces a numbered list of preferences.
#[test]
fn list_items_inherit_a_preferences_header() {
    let message = "Hi, I maintain this click checkout. First give me a short orientation.\n\nMy working preferences for the whole week:\n1. Only run the test files relevant to your change, never the whole suite unless I ask.\n2. Don't touch CHANGES.md or anything under docs/ unless I ask.\n3. End every reply with a single line starting with \"Next:\" saying what you'd do next.\n\nThen do one small fix now: in --help, an enum Choice option shows the default as RED.\n1. This numbered step is a task step.";
    assert_eq!(
        marked_rules(message),
        vec![
            standing(
                "1. Only run the test files relevant to your change, never the whole suite unless I ask."
            ),
            standing("2. Don't touch CHANGES.md or anything under docs/ unless I ask."),
            standing(
                "3. End every reply with a single line starting with \"Next:\" saying what you'd do next."
            ),
        ]
    );
}

#[test]
fn task_limited_and_narrative_clauses_are_not_standing() {
    assert_eq!(
        marked_rules(
            "Never run migrations during this pass. I never got the old build to work. Should we always use uv? Never mind the docs."
        ),
        vec![RuleClause {
            text: "Never run migrations during this pass.".to_string(),
            standing: RuleStanding::Pending,
        },]
    );
}

#[test]
fn a_quote_resolves_to_its_whole_clause() {
    let message = "Please never run the full suite unless I ask. Run lint before you commit.";
    assert_eq!(
        (
            clause_containing(message, "run the full   suite"),
            clause_containing(message, "suite unless I ask. Run lint"),
            clause_containing(message, "Run"),
            clause_containing(message, "rewrite history"),
        ),
        (
            Some("Please never run the full suite unless I ask.".to_string()),
            None,
            Some("Run lint before you commit.".to_string()),
            None,
        )
    );
}

#[test]
fn reported_advice_and_task_limited_lists_never_become_standing_rules() {
    let message = "The previous assistant told me to run the whole suite every time.\nMy preferences for this task:\n1. Never touch the migrations.\n\nMy standing preferences for all our work on this, today and in later sessions:\n1. Cite the file and section for every factual claim.";
    assert_eq!(
        marked_rules(message),
        vec![
            RuleClause {
                text: "1. Never touch the migrations.".to_string(),
                standing: RuleStanding::Pending,
            },
            standing("1. Cite the file and section for every factual claim."),
        ]
    );
}

#[test]
fn explicit_task_limits_win_and_mentions_of_an_assistant_are_not_relayed_speech() {
    let message = "For this task:\n- Do not modify docs.\n\nMy standing preferences for this task only:\n- Never touch migrations.\n\nNever treat an assistant answer as evidence of my preferences.";
    assert_eq!(
        marked_rules(message),
        vec![
            RuleClause {
                text: "- Do not modify docs.".to_string(),
                standing: RuleStanding::Pending,
            },
            RuleClause {
                text: "- Never touch migrations.".to_string(),
                standing: RuleStanding::Pending,
            },
            standing("Never treat an assistant answer as evidence of my preferences."),
        ]
    );
    assert_eq!(
        (
            super::inherited_scope(message, "- Do not modify docs."),
            super::inherited_scope(
                message,
                "Never treat an assistant answer as evidence of my preferences."
            ),
        ),
        (Some(super::HeaderScope::Pending), None)
    );
}

#[test]
fn relayed_lists_and_blank_lines_keep_their_header() {
    let message = "The assistant suggested these standing preferences:
- Never run the whole suite.

For this task:

- Never touch migrations.";
    assert_eq!(
        (
            marked_rules(message),
            super::inherited_scope(message, "- Never run the whole suite."),
            super::inherited_scope(message, "- Never touch migrations."),
        ),
        (
            vec![RuleClause {
                text: "- Never touch migrations.".to_string(),
                standing: RuleStanding::Pending,
            }],
            Some(super::HeaderScope::Reported),
            Some(super::HeaderScope::Pending),
        )
    );
}

#[test]
fn indented_continuations_stay_under_their_list_header() {
    let relayed = "The assistant suggested these preferences:
- Prefer targeted tests,
  keeping the changes small.
- Never run the whole suite.";
    let task = "For this task:
- Prefer targeted tests,
  keeping the changes small.
- Never run the whole suite.";
    assert_eq!(
        (
            marked_rules(relayed),
            super::inherited_scope(relayed, "- Never run the whole suite."),
            marked_rules(task),
            super::inherited_scope(task, "keeping the changes small."),
        ),
        (
            Vec::new(),
            Some(super::HeaderScope::Reported),
            [
                "- Prefer targeted tests,",
                "keeping the changes small.",
                "- Never run the whole suite."
            ]
            .map(|text| RuleClause {
                text: text.to_string(),
                standing: RuleStanding::Pending,
            })
            .to_vec(),
            Some(super::HeaderScope::Pending),
        )
    );
}

#[test]
fn nested_headers_and_requests_about_rules() {
    let nested = "My working preferences:
- Work carefully.
  For this task:
  - Never run migrations.";
    let relayed = "My working preferences:
- Work carefully.
  The assistant suggested these preferences:
  - Never run migrations.";
    let request = "Also, a new teammate is joining: list the standing rules I gave you when we started working on this fork, word for word if you can.";
    assert_eq!(
        (
            marked_rules(nested),
            super::inherited_scope(nested, "- Never run migrations."),
            marked_rules(relayed),
            marked_rules(request),
            super::asks_about_rules(request),
        ),
        (
            vec![
                RuleClause {
                    text: "- Work carefully.".to_string(),
                    standing: RuleStanding::Standing,
                },
                RuleClause {
                    text: "- Never run migrations.".to_string(),
                    standing: RuleStanding::Pending,
                },
            ],
            Some(super::HeaderScope::Pending),
            vec![RuleClause {
                text: "- Work carefully.".to_string(),
                standing: RuleStanding::Standing,
            }],
            Vec::new(),
            true,
        )
    );
}

#[test]
fn neutral_nested_headers_keep_the_enclosing_restriction_and_rules_about_rules_are_rules() {
    let task = "For this task:
- Check the setup.
  Details:
  - Never run migrations.";
    let relayed = "The assistant suggested these preferences:
- Check the setup.
  Details:
  - Never run migrations.";
    let standing = "From now on, quote my instructions word for word.";
    assert_eq!(
        (
            marked_rules(task),
            super::inherited_scope(task, "- Never run migrations."),
            marked_rules(relayed),
            super::inherited_scope(relayed, "- Never run migrations."),
            marked_rules(standing),
            super::asks_about_rules(standing),
            super::asks_about_rules("Show me the rules you follow here."),
        ),
        (
            ["- Check the setup.", "- Never run migrations."]
                .map(|text| RuleClause {
                    text: text.to_string(),
                    standing: RuleStanding::Pending,
                })
                .to_vec(),
            Some(super::HeaderScope::Pending),
            Vec::new(),
            Some(super::HeaderScope::Reported),
            vec![RuleClause {
                text: standing.to_string(),
                standing: RuleStanding::Standing,
            }],
            false,
            true,
        )
    );
}

#[test]
fn consecutive_nested_headers_and_standing_retrieval_rules() {
    let task = "For this task:
- Check the setup.
  Details:
    Migration policy:
    - Never run migrations.";
    let repeat = "From now on, repeat my instructions word for word.";
    assert_eq!(
        (
            super::inherited_scope(task, "- Never run migrations."),
            marked_rules(task)
                .iter()
                .all(|rule| rule.standing == RuleStanding::Pending),
            super::asks_about_rules(repeat),
            super::asks_about_rules("Repeat my instructions word for word."),
        ),
        (Some(super::HeaderScope::Pending), true, false, true)
    );
}

#[test]
fn background_is_the_users_description_of_themselves_and_the_work() {
    let text = "Hi! I know Python well but only a little Rust. I'm not changing any code, just reading. From now on, explain Rust ideas with Python comparisons. Do you know tokio? The assistant said I'm a beginner.";
    assert_eq!(
        super::background_statements(text),
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
            super::background_statements(text),
            super::asks_about_rules("List the preferences I gave you for all our work."),
            super::asks_about_rules("From now on, repeat my instructions word for word."),
        ),
        (vec!["I know Go well.".to_string()], true, false)
    );
}

#[test]
fn background_skips_lines_that_quote_show_code_or_relay() {
    assert_eq!(
        (
            super::background_statements(
                "Translate this sentence: \"I know Python. I'm a beginner.\"\nShe wrote: I'm a doctor.\n`I'm a pilot.`\nI'm a teacher."
            ),
            super::asks_about_rules("- List the preferences I gave you for all our work."),
        ),
        (vec!["I'm a teacher.".to_string()], true)
    );
}
