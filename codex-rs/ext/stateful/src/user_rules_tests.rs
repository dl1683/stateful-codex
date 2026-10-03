use pretty_assertions::assert_eq;

use super::RuleClause;
use super::RuleStanding;
use super::clause_containing;
use super::marked_rules;

/// The standing of the rule the unit holding `clause` states; None when it is relayed.
fn scope(text: &str, clause: &str) -> Option<RuleStanding> {
    crate::rule_units::rule_for_clause(text, clause, clause).map(|rule| rule.clause.standing)
}

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
                "Only run the test files relevant to your change, never the whole suite unless I ask."
            ),
            standing("Don't touch CHANGES.md or anything under docs/ unless I ask."),
            standing(
                "End every reply with a single line starting with \"Next:\" saying what you'd do next."
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
                text: "Never touch the migrations.".to_string(),
                standing: RuleStanding::Pending,
            },
            standing("Cite the file and section for every factual claim."),
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
                text: "Do not modify docs.".to_string(),
                standing: RuleStanding::Pending,
            },
            RuleClause {
                text: "Never touch migrations.".to_string(),
                standing: RuleStanding::Pending,
            },
            standing("Never treat an assistant answer as evidence of my preferences."),
        ]
    );
    assert_eq!(
        (
            scope(message, "- Do not modify docs."),
            scope(
                message,
                "Never treat an assistant answer as evidence of my preferences."
            ),
        ),
        (Some(RuleStanding::Pending), Some(RuleStanding::Standing))
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
            scope(message, "- Never run the whole suite."),
            scope(message, "- Never touch migrations."),
        ),
        (
            vec![RuleClause {
                text: "Never touch migrations.".to_string(),
                standing: RuleStanding::Pending,
            }],
            None,
            Some(RuleStanding::Pending),
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
            scope(relayed, "- Never run the whole suite."),
            marked_rules(task),
            scope(task, "keeping the changes small."),
        ),
        (
            Vec::new(),
            None,
            // A continuation line belongs to its item: one rule per item.
            [
                "Prefer targeted tests, keeping the changes small.",
                "Never run the whole suite."
            ]
            .map(|text| RuleClause {
                text: text.to_string(),
                standing: RuleStanding::Pending,
            })
            .to_vec(),
            Some(RuleStanding::Pending),
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
            scope(nested, "- Never run migrations."),
            marked_rules(relayed),
            marked_rules(request),
            super::asks_about_rules(request),
        ),
        (
            vec![
                RuleClause {
                    text: "Work carefully.".to_string(),
                    standing: RuleStanding::Standing,
                },
                RuleClause {
                    text: "Never run migrations.".to_string(),
                    standing: RuleStanding::Pending,
                },
            ],
            Some(RuleStanding::Pending),
            vec![RuleClause {
                text: "Work carefully.".to_string(),
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
            scope(task, "- Never run migrations."),
            marked_rules(relayed),
            scope(relayed, "- Never run migrations."),
            marked_rules(standing),
            super::asks_about_rules(standing),
            super::asks_about_rules("Show me the rules you follow here."),
        ),
        (
            ["Check the setup.", "Never run migrations."]
                .map(|text| RuleClause {
                    text: text.to_string(),
                    standing: RuleStanding::Pending,
                })
                .to_vec(),
            Some(RuleStanding::Pending),
            Vec::new(),
            None,
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
            scope(task, "- Never run migrations."),
            marked_rules(task)
                .iter()
                .all(|rule| rule.standing == RuleStanding::Pending),
            super::asks_about_rules(repeat),
            super::asks_about_rules("Repeat my instructions word for word."),
        ),
        (Some(RuleStanding::Pending), true, false, true)
    );
}

/// A preference quoted from someone else, or attributed to them, is never the user's rule,
/// while the user's own rule may still quote a word.
#[test]
fn quoted_or_attributed_preferences_are_not_the_users_rules() {
    let text = "About me: I'm a backend engineer. Standing rule for all future sessions: never touch the docs folder. My colleague wrote in our chat: \"I always want tests written first\" - that's her preference, not mine.";
    assert_eq!(
        (
            marked_rules(text),
            marked_rules("From now on, never use the word \"simply\" in docs."),
            super::reports_speech(
                "My colleague wrote in our chat: \"I always want tests written first\"."
            ),
            super::reports_speech("Always run the linter; that is his preference, not mine."),
            marked_rules("From now on, never change files that are not mine."),
        ),
        (
            vec![RuleClause {
                text: "Standing rule for all future sessions: never touch the docs folder."
                    .to_string(),
                standing: RuleStanding::Standing,
            }],
            vec![RuleClause {
                text: "From now on, never use the word \"simply\" in docs.".to_string(),
                standing: RuleStanding::Standing,
            }],
            true,
            true,
            vec![RuleClause {
                text: "From now on, never change files that are not mine.".to_string(),
                standing: RuleStanding::Standing,
            }],
        )
    );
}

#[test]
fn quoted_statements_across_sentences_never_become_rules() {
    assert_eq!(
        marked_rules(
            "My colleague wrote: \"I like Rust. Always write tests first.\" From now on, never push to main."
        ),
        vec![RuleClause {
            text: "From now on, never push to main.".to_string(),
            standing: RuleStanding::Standing,
        }]
    );
}

#[test]
fn a_curly_quoted_relay_does_not_swallow_the_users_next_rule() {
    assert_eq!(
        marked_rules(
            "My colleague wrote: \u{201c}Always test first.\u{201d} From now on, never touch docs."
        ),
        vec![RuleClause {
            text: "From now on, never touch docs.".to_string(),
            standing: RuleStanding::Standing,
        }]
    );
}

/// Item 2 final review: a statement about the user or the work that names its duration is
/// not a rule ("I maintain this fork for this project."); a directive in the same words is.
#[test]
fn a_statement_naming_the_project_is_not_a_rule() {
    assert_eq!(
        (
            crate::user_rules::marked_rules("I maintain this fork for this project.").len(),
            crate::user_rules::marked_rules("We use uv for this project.").len(),
            crate::user_rules::marked_rules("I want you to use uv for this project.").len(),
            crate::user_rules::marked_rules("Use uv for this project.").len(),
        ),
        (0, 0, 1, 1)
    );
}
