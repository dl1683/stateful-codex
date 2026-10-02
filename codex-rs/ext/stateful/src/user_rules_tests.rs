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
