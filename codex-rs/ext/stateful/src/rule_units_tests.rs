use pretty_assertions::assert_eq;

use super::marked_rules;
use super::rule_for_clause;
use crate::user_rules::MAX_RULE_BYTES;
use crate::user_rules::RuleClause;
use crate::user_rules::RuleStanding;

fn standing(text: &str) -> RuleClause {
    RuleClause {
        text: text.to_string(),
        standing: RuleStanding::Standing,
    }
}

/// tui8's opening: two rules in two sentences, background, and a colleague's quoted habit.
#[test]
fn both_rules_of_a_message_that_also_relays_a_colleague_are_captured() {
    let message = "Hi! Quick intro since this is our first session together: I'm a backend developer, mostly Go for the last six years, so my Python is a bit rusty, and I maintain this humanize fork for our internal ops dashboards. Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself. And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step. Also FYI, Priya (she co-maintains the fork with me) wrote in our team chat: \"Always run the full test suite and mypy on the whole repo after every single change.\" Today I'd like a small helper, naturalrate, for transfer speeds: 1536000 bytes per second should read '1.5 MB/s'. First get oriented - how naturalsize is implemented, where the public API is exported, how the tests are organised - and propose a short plan. No code yet.";
    assert_eq!(
        marked_rules(message),
        vec![
            standing(
                "Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself."
            ),
            standing(
                "And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step."
            ),
        ]
    );
}

/// horizon3's opening: a multi-sentence list item is one rule, and rules keep their order.
#[test]
fn a_list_item_is_one_rule_with_all_its_sentences() {
    let message = "Hi. I maintain an internal fork of python-humanize for our dashboards, and I'll be working on it with you over the next few weeks, roughly one session a day. Standing rules for this project, please follow them in every session:\n1. Only run the tests relevant to what you changed, never the whole suite.\n2. Never install anything into my global Python. If you need an environment, make a local venv inside the repo.\n3. Don't touch docs/ or any changelog.\n4. End every reply with one line starting with `Next:` that suggests the next step.\n   It should name one concrete step.\n\nToday's task: add a compact output style to precisedelta.";
    assert_eq!(
        marked_rules(message),
        vec![
            standing("1. Only run the tests relevant to what you changed, never the whole suite."),
            standing(
                "2. Never install anything into my global Python. If you need an environment, make a local venv inside the repo."
            ),
            standing("3. Don't touch docs/ or any changelog."),
            standing(
                "4. End every reply with one line starting with `Next:` that suggests the next step. It should name one concrete step."
            ),
        ]
    );
}

/// debug2's opening: ground rules for a whole investigation keep their scope and their
/// ending condition, and a today-only item under a standing header stays pending.
#[test]
fn investigation_ground_rules_keep_their_scope_and_end_condition() {
    let message = "I need help chasing a bug in shipit.\n\nPlease investigate and find the root cause. Some ground rules for this whole investigation, which may take a few days:\n- Do NOT change any code until we have agreed on the root cause. Reading, running things and throwaway experiments outside the repo files are fine.\n- Keep a running list of the hypotheses you have ruled out and the evidence that ruled each one out, so we don't go round in circles.\n- For today only, skip the slow integration tests.\n\nTell me what you found.";
    let scope = "Some ground rules for this whole investigation, which may take a few days:";
    assert_eq!(
        (
            marked_rules(message),
            marked_rules("During this investigation, never push a branch until I say so."),
        ),
        (
            vec![
                standing(&format!(
                    "{scope} Do NOT change any code until we have agreed on the root cause. Reading, running things and throwaway experiments outside the repo files are fine."
                )),
                standing(&format!(
                    "{scope} Keep a running list of the hypotheses you have ruled out and the evidence that ruled each one out, so we don't go round in circles."
                )),
                RuleClause {
                    text: format!("{scope} For today only, skip the slow integration tests."),
                    standing: RuleStanding::Pending,
                },
            ],
            vec![standing(
                "During this investigation, never push a branch until I say so."
            )],
        )
    );
}

/// An unheaded list item is judged whole: its unmarked condition stays with its rule, and an
/// explicit task limit anywhere in it keeps all of it from applying.
#[test]
fn an_unheaded_list_item_keeps_its_conditions() {
    let message = "Quick notes:\n- Never install anything globally. If you need an environment, use a local venv.\n- Never touch the lockfile. This applies for this task only.\n- Check the README first.";
    assert_eq!(
        marked_rules(message),
        vec![
            standing(
                "- Never install anything globally. If you need an environment, use a local venv."
            ),
            RuleClause {
                text: "- Never touch the lockfile. This applies for this task only.".to_string(),
                standing: RuleStanding::Pending,
            },
        ]
    );
}

/// Quoted, fenced or blockquoted instructions are someone's words, never the user's rule,
/// so pasting an old message cannot bring a retired rule back; a quoted term still can be
/// part of the user's own rule.
#[test]
fn quoted_fenced_and_blockquoted_instructions_are_not_rules() {
    assert_eq!(
        [
            "\"From now on, never commit.\"",
            "Here is what I wrote last week: \"From now on, never commit.\"",
            "> From now on, never commit.",
            "```\nFrom now on, never commit.\n```",
            "Standing rules:\n- \"Always run mypy.\"",
            "From now on, end each reply with a line starting with 'Next:'.",
        ]
        .map(|text| marked_rules(text).len()),
        [0, 0, 0, 0, 0, 1]
    );
}

/// A unit longer than the rule bound is not captured at all, never in part.
#[test]
fn an_oversized_item_is_not_captured_in_part() {
    let long = "x".repeat(MAX_RULE_BYTES);
    let message = format!("Standing rules:\n- Never touch {long}.\n- Never push to main.");
    assert_eq!(
        marked_rules(&message),
        vec![standing("- Never push to main.")]
    );
}

/// The model's verified quote resolves to the same rule host capture stores: the whole item,
/// with the scope of its header, from either of its sentences.
#[test]
fn a_quote_from_any_sentence_resolves_to_the_whole_item() {
    let message = "Some ground rules for this whole investigation:\n- Do NOT change any code until we have agreed on the root cause. Experiments outside the repo are fine.\n  Throwaway branches are fine too.\n\nNever mind the docs.";
    let rule = Some(standing(
        "Some ground rules for this whole investigation: Do NOT change any code until we have agreed on the root cause. Experiments outside the repo are fine. Throwaway branches are fine too.",
    ));
    assert_eq!(
        (
            rule_for_clause(message, "Experiments outside the repo are fine.")
                .map(|rule| rule.clause),
            rule_for_clause(message, "Throwaway branches are fine too.").map(|rule| rule.clause),
            marked_rules(message).into_iter().next(),
        ),
        (rule.clone(), rule.clone(), rule)
    );
}

/// Review round 1: a list item that is wholly a quotation is someone's words even without a
/// marker inside; a quoted term that merely contains a marker word stays a term; a tilde line
/// inside a backtick fence does not close it.
#[test]
fn whole_quotations_terms_and_nested_fences() {
    assert_eq!(
        [
            "Standing rules:\n- 'Never commit.'\n- \"Only run scoped tests.\"",
            "Always use the label \"standing rule\" in rule receipts.",
            "```\n~~~\nFrom now on, never commit.\n```",
        ]
        .map(marked_rules),
        [
            Vec::new(),
            vec![standing(
                "Always use the label \"standing rule\" in rule receipts."
            )],
            Vec::new(),
        ]
    );
}

/// Review round 1: under an investigation header, "during this investigation" is the rule's
/// scope, and a nested header keeps the investigation's scope for its items.
#[test]
fn investigation_scope_survives_its_own_words_and_nested_headers() {
    let message = "Ground rules for this whole investigation:\n- During this investigation, never push until I say so.\n- Work carefully.\n  My preferences:\n  - Never touch docs.";
    let scope = "Ground rules for this whole investigation:";
    assert_eq!(
        marked_rules(message),
        vec![
            standing(&format!(
                "{scope} During this investigation, never push until I say so."
            )),
            standing(&format!("{scope} Work carefully.")),
            standing(&format!("{scope} Never touch docs.")),
        ]
    );
}

/// Spec item 1: independent directives joined in one sentence are separate rules; a following
/// qualification stays with its rule; a declared count is reported; an overlength unit is
/// reported as omitted rather than dropped; an investigation rule carries its end condition.
#[test]
fn prose_rules_are_complete_and_counted() {
    let long = "x".repeat(super::MAX_RULE_BYTES);
    let message = format!(
        "Three standing rules for all our work: never commit and always end each reply with a 'Next:' line. Never install anything globally. If you need an environment, make a local venv inside the repo. From now on, never touch {long}."
    );
    let marked = super::marked_rule_units(&message);
    let investigation = super::marked_rule_units(
        "During this investigation, never push a branch until we have agreed on the fix. Thanks.",
    );
    assert_eq!(
        (
            marked
                .rules
                .iter()
                .map(|rule| rule.clause.text.clone())
                .collect::<Vec<_>>(),
            marked.omitted.len(),
            marked.declared_count,
            investigation
                .rules
                .iter()
                .map(|rule| rule.scope.clone())
                .collect::<Vec<_>>(),
        ),
        (
            vec![
                "Three standing rules for all our work: never commit".to_string(),
                "always end each reply with a 'Next:' line.".to_string(),
                "Never install anything globally. If you need an environment, make a local venv inside the repo."
                    .to_string(),
            ],
            1,
            Some(3),
            vec![Some(super::ScopeHint {
                title: "During this investigation, never push a branch until we have agreed on the fix"
                    .to_string(),
                end_condition: Some("until we have agreed on the fix".to_string()),
            })],
        )
    );
}

/// write1: style rules listed after a framing for the whole piece of work are four rules,
/// standing for the project; the background beside them is the user's.
#[test]
fn a_framed_style_list_is_one_rule_per_item() {
    let message = "Help me plan an essay on protecting your voice from AI cloning.\n\nSome ground rules for this essay, for the whole time we work on it: write in the second person (\"you\"); no jargon without a one-line plain-English definition the first time it appears; British spelling; short paragraphs (three or four sentences at most).\n\nAbout me: I'm a founder, not an academic. I want this to read like a founder talking straight to creators, not like a paper.\n\nFor today, I just want us to agree on an outline.";
    let framing = "Some ground rules for this essay, for the whole time we work on it";
    assert_eq!(
        (
            marked_rules(message),
            crate::background::background_statements(message),
        ),
        (
            [
                "write in the second person (\"you\").",
                "no jargon without a one-line plain-English definition the first time it appears.",
                "British spelling.",
                "short paragraphs (three or four sentences at most).",
            ]
            .map(|item| standing(&format!("{framing}: {item}")))
            .to_vec(),
            vec!["I'm a founder, not an academic.".to_string()],
        )
    );
}
