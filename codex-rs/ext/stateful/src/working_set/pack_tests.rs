use pretty_assertions::assert_eq;

use super::directives::RuleEntry;
use super::directives::RuleStatus;
use super::directives_tests::RULE_NEW;
use super::directives_tests::RULE_OLD;
use super::directives_tests::quote;
use super::pack::ComposeInput;
use super::pack::compose_capsule;
use super::reduce_tests::assistant;
use super::reduce_tests::command;
use super::reduce_tests::reduced;
use super::reduce_tests::user;
use super::render::CapsuleHeader;
use super::render::MAX_CAPSULE_BYTES;
use super::render::MAX_CAPSULE_TOKENS;
use super::semantic::SemanticKind;
use super::semantic::SemanticLedger;
use super::semantic::SemanticRecord;
use super::semantic::SupersessionLink;
use super::*;

const HEADER: CapsuleHeader<'static> = CapsuleHeader {
    project: "proj-1",
    thread: "thread-1",
    window: "w2",
    eligibility_generation: 7,
    watermark: 41,
};

fn quarter(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn compose(projection: &Projection, rules: &[RuleEntry], ledger: &SemanticLedger) -> String {
    let counter: &dyn Fn(&str) -> usize = &quarter;
    let capsule = compose_capsule(&ComposeInput {
        projection,
        header: &HEADER,
        exclusions: &Exclusions::default(),
        rules,
        ledger,
        counter: Some(counter),
    })
    .expect("fits");
    assert!(capsule.text.len() <= MAX_CAPSULE_BYTES);
    assert!(quarter(&capsule.text) <= MAX_CAPSULE_TOKENS);
    capsule.text
}

fn measurement(value: &str, sequence: u64) -> SemanticRecord {
    SemanticRecord {
        kind: SemanticKind::Measurement,
        main: quote(value, false, sequence),
        subject: quote("throughput", false, sequence),
        qualifier: None,
        detail: None,
        request_sha256: "r".repeat(64),
    }
}

#[test]
fn packs_in_authority_order_with_the_old_rule_never_active() {
    let rg = "The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program.";
    let projection = reduced(vec![
        user("t2", RULE_OLD),
        assistant("t3", "Next step: write section 4."),
        command("t3", "c1", "rg -n x .", 1, rg),
        command("t3", "c2", "git grep -n x src", 0, "src/a.py:1:x"),
        user("t8", RULE_NEW),
        user("t9", "Draft section 4 next."),
    ]);
    let mut ledger = SemanticLedger::default();
    ledger.records.push(measurement("4,108.3 invocations/s", 2));
    ledger.records.push(SemanticRecord {
        kind: SemanticKind::Rejection,
        main: quote("tools/claims_index.py is wrong", false, 3),
        subject: quote("tools/claims_index.py", false, 3),
        qualifier: Some(quote("it pairs README ids with sorted files", false, 3)),
        detail: None,
        request_sha256: "r".repeat(64),
    });
    ledger.links.push(SupersessionLink {
        old: quote(RULE_OLD, true, 2),
        new: quote(RULE_NEW, true, 8),
        correction: quote("Change to my scope rule", true, 8),
    });
    let rules = vec![RuleEntry {
        id: "r1".to_string(),
        text: "Cite papers by arXiv id.".to_string(),
        status: RuleStatus::Active,
        superseded_by: None,
    }];
    let text = compose(&projection, &rules, &ledger);
    let body: Vec<&str> = text.lines().skip(3).collect();
    assert_eq!(
        body,
        vec![
            "rule(user, store): \"Cite papers by arXiv id.\"",
            "direction(user; proposed successor, link unconfirmed): \"Change to my scope rule: bring them into the main sections wherever they're relevant, and drop the separate background paragraph.\" @s1",
            "user(newest): \"Draft section 4 next.\" @s2",
            "next: write section 4. @s3",
            "rejected(reported): \"tools/claims_index.py is wrong\" because \"it pairs README ids with sorted files\" @s4,s5,s6",
            "route: unavailable rg - The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program. | exit 1 @s7 -> works git grep -n x src @s8",
            "measured(reported; run unknown): \"4,108.3 invocations/s\" of \"throughput\" @s9,s10",
            "coverage: older direction unresolved",
            "</stateful_capsule>",
        ]
    );
    assert!(!text.contains("short background paragraph"));
}

#[test]
fn whole_records_are_omitted_and_oversize_direction_becomes_a_marker() {
    let long = format!(
        "Please follow this plan: {}",
        "step and step again. ".repeat(95)
    );
    let projection = reduced(vec![user("t1", RULE_OLD), user("t2", &long)]);
    let mut ledger = SemanticLedger::default();
    for n in 0..40 {
        ledger.records.push(measurement(
            &format!("{n},000.5 invocations/s in run {n}"),
            n,
        ));
    }
    let text = compose(&projection, &[], &ledger);
    assert!(text.contains("user(newest): too long to carry; see history @s1"));
    assert!(!text.contains("short background paragraph"));
    // Omitted optional records are not missing direction.
    assert!(!text.contains("current direction unavailable"));
    // Every rendered measurement is whole; the rest are counted, never shortened.
    let shown = text.lines().filter(|l| l.starts_with("measured(")).count();
    assert!(shown > 0 && shown < 40);
    assert!(text.contains(&format!("coverage: omitted {}", 40 - shown)));
    assert!(
        text.lines()
            .filter(|l| l.starts_with("measured("))
            .all(|l| l.contains(" invocations/s in run "))
    );
}
