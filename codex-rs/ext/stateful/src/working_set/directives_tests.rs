use pretty_assertions::assert_eq;

use super::directives::CurrentDirection;
use super::directives::RouteWithdrawal;
use super::directives::RuleEntry;
use super::directives::RuleStatus;
use super::directives::current_direction;
use super::directives::route_withdrawal;
use super::reduce_tests::command;
use super::reduce_tests::reduced;
use super::reduce_tests::user;
use super::semantic::Quote;
use super::semantic::SemanticKind;
use super::semantic::SemanticRecord;
use super::semantic::SupersessionLink;
use super::*;

pub(super) const RULE_OLD: &str =
    "Anything earlier (the scaling laws, GPT-3) goes only in a short background paragraph.";
pub(super) const RULE_NEW: &str = "Change to my scope rule: bring them into the main sections wherever they're relevant, and drop the separate background paragraph.";

pub(super) fn quote(text: &str, user: bool, sequence: u64) -> Quote {
    Quote {
        text: text.to_string(),
        user,
        item_id: format!("item-{sequence}"),
        turn_id: format!("turn-{sequence}"),
        part: 0,
        start: 0,
        end: text.len(),
        part_sha256: "p".repeat(64),
        span_sha256: "s".repeat(64),
        revision: format!("rev-{sequence}"),
        sequence,
    }
}

fn rule(id: &str, status: RuleStatus, successor: Option<&str>) -> RuleEntry {
    RuleEntry {
        id: id.to_string(),
        text: format!("rule {id}"),
        status,
        superseded_by: successor.map(str::to_string),
    }
}

#[test]
fn every_user_phrasing_is_captured_and_only_the_newest_is_current() {
    let projection = reduced(vec![
        user("t2", RULE_OLD),
        user("t8", RULE_NEW),
        user("t9", "What do you think of section 3?"),
    ]);
    let texts: Vec<Option<&str>> = projection
        .statements
        .iter()
        .map(|s| s.text.as_deref())
        .collect();
    assert_eq!(
        texts,
        vec![
            Some(RULE_OLD),
            Some(RULE_NEW),
            Some("What do you think of section 3?")
        ]
    );
    let direction = current_direction(&[], &projection, &[], &Exclusions::default());
    assert_eq!(
        direction,
        CurrentDirection {
            rules: Vec::new(),
            unresolved_rules: 0,
            successor: None,
            newest: projection.statements.last(),
            older_unresolved: true,
        }
    );
}

#[test]
fn store_successors_are_active_and_broken_chains_never_resurrect_predecessors() {
    let rules = vec![
        rule("a", RuleStatus::Superseded, Some("b")),
        rule("b", RuleStatus::Superseded, Some("c")),
        rule("c", RuleStatus::Active, None),
        rule("d", RuleStatus::Superseded, Some("missing")),
        rule("e", RuleStatus::Superseded, Some("f")),
        rule("f", RuleStatus::Retired, None),
        rule("g", RuleStatus::Active, Some("c")),
        rule("h", RuleStatus::Active, None),
    ];
    let projection = Projection::new();
    let exclusions = Exclusions {
        turns: Vec::new(),
        retired_texts: vec!["rule h".to_string()],
    };
    let direction = current_direction(&rules, &projection, &[], &exclusions);
    // Only the active successor c; d and e have no live successor, and the excluded h is
    // withheld.
    assert_eq!(direction.rules, vec![&rules[2]]);
    assert_eq!(direction.unresolved_rules, 2);
}

#[test]
fn an_advisory_link_carries_the_new_user_wording_and_suppresses_the_old() {
    let projection = reduced(vec![
        user("t2", RULE_OLD),
        user("t8", RULE_NEW),
        user("t9", "Draft section 4 next."),
    ]);
    let links = vec![SupersessionLink {
        old: quote(RULE_OLD, true, 2),
        new: quote(RULE_NEW, true, 8),
        correction: quote("Change to my scope rule", true, 8),
    }];
    let direction = current_direction(&[], &projection, &links, &Exclusions::default());
    assert_eq!(direction.successor, Some(&links[0].new));
    // A link cannot mutate stored rules, and a later link superseding the new wording, or an
    // assistant-worded successor, leaves no current successor.
    let rules = vec![rule("u", RuleStatus::Active, None)];
    let mut chained = links.clone();
    chained.push(SupersessionLink {
        old: quote(RULE_NEW, true, 8),
        new: quote("Use only the main sections.", false, 9),
        correction: quote("Use only", true, 9),
    });
    let direction = current_direction(&rules, &projection, &chained, &Exclusions::default());
    assert_eq!(
        (direction.rules, direction.successor),
        (vec![&rules[0]], None)
    );
}

fn rejection(text: &str) -> SemanticRecord {
    SemanticRecord {
        kind: SemanticKind::Rejection,
        main: quote(text, false, 3),
        subject: quote(text, false, 3),
        qualifier: Some(quote("it mis-attributes ids", false, 3)),
        detail: None,
        request_sha256: "r".repeat(64),
    }
}

#[test]
fn rejections_and_later_user_mentions_withdraw_a_working_replacement() {
    let rg = "The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program.";
    let observations = vec![
        command("t1", "c1", "rg -n x .", 1, rg),
        command("t1", "c2", "git grep -n x src", 0, "src/a.py:1:x"),
    ];
    let projection = reduced(observations.clone());
    let fix = &projection.candidates[1];
    assert_eq!(fix.subject.as_deref(), Some("git grep"));
    // Exit zero and a family match alone keep the label; unrelated rejections do not touch it.
    let unrelated = rejection("tools/claims_index.py is wrong");
    assert_eq!(route_withdrawal(fix, &projection, &[&unrelated]), None);
    let rejected = rejection("git grep misses generated files");
    assert_eq!(
        route_withdrawal(fix, &projection, &[&rejected]),
        Some(RouteWithdrawal::Rejected)
    );
    let mut later = observations;
    later.push(user("t2", "Stop using git grep here."));
    let projection = reduced(later);
    assert_eq!(
        route_withdrawal(&projection.candidates[1], &projection, &[]),
        Some(RouteWithdrawal::Unknown)
    );
}
