use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootPromotion;
use pretty_assertions::assert_eq;

use super::ResolvedRootBlackboard;
use super::RootBlackboardStatus;
use super::USER_RULES_HEADER;
use super::render_root_blackboard;

fn hit(
    id: &str,
    kind: BlackboardKind,
    provenance: BlackboardProvenanceKind,
    content: &str,
) -> BlackboardHit {
    BlackboardHit::new(
        BlackboardEntry {
            id: BlackboardEntryId::parse(id).expect("entry ID"),
            value: NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: HierarchyNodeId::parse("node-root").expect("node ID"),
                kind,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: provenance,
                    source_id: "turn-1".to_string(),
                },
            },
            state: BlackboardEntryState::Active,
            superseded_by: None,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        },
        BlackboardEvidenceFreshness::NotApplicable,
    )
}

/// User rules lead the root under their own heading; an agent-recorded rule (tui7's
/// merged entry with a one-off "no code changes" restriction) is counted but never shown.
#[test]
fn user_rules_lead_and_agent_recorded_rules_are_not_applied() {
    let status = RootBlackboardStatus::Available(ResolvedRootBlackboard::new(
        RootBlackboardProjection {
            project_id: "project-1".to_string(),
            revision: 3,
            data: vec![
                hit(
                    "rule-next",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::User,
                    "End each of your replies with a line starting with 'Next:'.",
                ),
                hit(
                    "agent-rule",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::Agent,
                    "No code changes during the initial orientation and planning pass.",
                ),
                hit(
                    "recipe",
                    BlackboardKind::Fact,
                    BlackboardProvenanceKind::Agent,
                    "Recipe: run tests with PYTHONPATH=src python -m pytest tests/test_x.py.",
                ),
            ],
            omitted_entries: 0,
            candidate_entries: 0,
        },
        Default::default(),
        None,
    ));
    let mut output = String::new();
    render_root_blackboard(&mut output, &status);
    let headings = output
        .lines()
        .filter(|line| !line.starts_with("- E") && !line.starts_with("For durableLearning"))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            headings,
            output.contains("Next:"),
            output.contains("No code changes"),
            output.contains("PYTHONPATH=src"),
        ),
        (
            vec![
                "Project intelligence revision: 3",
                "Root blackboard (active, explicitly promoted knowledge):",
                USER_RULES_HEADER,
                "Other promoted knowledge:",
                "- 1 agent-recorded rules are not applied: they are not the user's own words. blackboard_query lists them; treat them as unconfirmed.",
            ],
            true,
            false,
            true,
        )
    );
}

/// Rules come out of the projection itself, so aliases stay contiguous for deltas and
/// completion; and large knowledge cannot push the user's rules out of the packet.
#[test]
fn quarantine_keeps_aliases_contiguous_and_rules_are_never_evicted() {
    let quarantined = ResolvedRootBlackboard::new(
        RootBlackboardProjection {
            project_id: "project-1".to_string(),
            revision: 1,
            data: vec![
                hit(
                    "rule",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::User,
                    "Never commit.",
                ),
                hit(
                    "hidden",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::Agent,
                    "Invented.",
                ),
                hit(
                    "fact",
                    BlackboardKind::Fact,
                    BlackboardProvenanceKind::Agent,
                    "A fact.",
                ),
            ],
            omitted_entries: 0,
            candidate_entries: 0,
        },
        Default::default(),
        None,
    );
    assert_eq!(
        quarantined
            .projection
            .data
            .iter()
            .map(|hit| hit.entry.id.to_string())
            .collect::<Vec<_>>(),
        vec!["rule".to_string(), "fact".to_string()]
    );

    let mut data = (0..5)
        .map(|index| {
            hit(
                &format!("rule-{index}"),
                BlackboardKind::Instruction,
                BlackboardProvenanceKind::User,
                &format!("Rule {index}: {}", "never do this. ".repeat(55)),
            )
        })
        .collect::<Vec<_>>();
    data.extend((0..20).map(|index| {
        hit(
            &format!("fact-{index:02}"),
            BlackboardKind::Fact,
            BlackboardProvenanceKind::Agent,
            &format!("Fact {index}: {}", "detail ".repeat(400)),
        )
    }));
    let status = RootBlackboardStatus::Available(ResolvedRootBlackboard::new(
        RootBlackboardProjection {
            project_id: "project-1".to_string(),
            revision: 2,
            data,
            omitted_entries: 0,
            candidate_entries: 0,
        },
        Default::default(),
        None,
    ));
    let mut output = String::new();
    render_root_blackboard(&mut output, &status);
    assert_eq!(
        (0..5)
            .map(|index| output.contains(&format!("Rule {index}:")))
            .collect::<Vec<_>>(),
        vec![true; 5]
    );
    assert!(output.contains("root entries omitted by the context bound"));
}

/// History decoration never truncates the current entry, and an entry's digest records
/// whether its history was actually shown, so a later layout that fits it is a change.
#[test]
fn history_is_shown_only_when_it_fits_and_its_presence_is_part_of_the_digest() {
    let predecessor = hit(
        "one",
        BlackboardKind::Decision,
        BlackboardProvenanceKind::Agent,
        "ONE decimal place.",
    )
    .entry;
    let render = |content: &str, with_history: bool| {
        let root = ResolvedRootBlackboard::new(
            RootBlackboardProjection {
                project_id: "project-1".to_string(),
                revision: 1,
                data: vec![hit(
                    "two",
                    BlackboardKind::Decision,
                    BlackboardProvenanceKind::Agent,
                    content,
                )],
                omitted_entries: 0,
                candidate_entries: 0,
            },
            Default::default(),
            None,
        );
        let root = if with_history {
            root.with_predecessors([("two".to_string(), predecessor.clone())])
        } else {
            root
        };
        let mut output = String::new();
        let layout = render_root_blackboard(&mut output, &RootBlackboardStatus::Available(root));
        (output, layout.entries[0].digest.clone())
    };
    let near_limit = "TWO decimal places. ".repeat(140);
    let (long_plain, long_plain_digest) = render(&near_limit, false);
    let (long_with_history, long_history_digest) = render(&near_limit, true);
    let (short_plain, short_plain_digest) = render("TWO decimal places.", false);
    let (short_with_history, short_history_digest) = render("TWO decimal places.", true);
    assert_eq!(
        (
            long_with_history == long_plain,
            long_history_digest == long_plain_digest,
            long_with_history.contains("truncated; query blackboard"),
            short_with_history.contains("replaces: \"ONE decimal place.\""),
            short_plain.contains("replaces:"),
            short_history_digest == short_plain_digest,
        ),
        (true, true, false, true, false, false)
    );
}

/// An entry that needs a check says so, with the reason, and the qualification is part of
/// its digest so marking it is a change the packet reports.
#[test]
fn entries_that_need_a_check_are_qualified() {
    let render = |content: &str, needs_check: bool| {
        let root = ResolvedRootBlackboard::new(
            RootBlackboardProjection {
                project_id: "project-1".to_string(),
                revision: 1,
                data: vec![hit(
                    "plan",
                    BlackboardKind::Strategy,
                    BlackboardProvenanceKind::Agent,
                    content,
                )],
                omitted_entries: 0,
                candidate_entries: 0,
            },
            Default::default(),
            None,
        );
        let root = if needs_check {
            root.with_validity(crate::knowledge_validity::RootValidity {
                needs_check: [(
                    "plan".to_string(),
                    "a newer record says to remove the history section".to_string(),
                )]
                .into(),
            })
        } else {
            root
        };
        let mut output = String::new();
        let layout = render_root_blackboard(&mut output, &RootBlackboardStatus::Available(root));
        (output, layout.entries[0].digest.clone())
    };
    let (plain, plain_digest) = render("Include a concise history section.", false);
    let (qualified, qualified_digest) = render("Include a concise history section.", true);
    let (long, _) = render(&"Include a concise history section. ".repeat(120), true);
    assert_eq!(
        (
            plain.contains("may be outdated"),
            qualified.contains(
                "- E1 [may be outdated; check before relying on it: a newer record says to remove the history section] [high strategy;"
            ),
            plain_digest == qualified_digest,
            long.contains("[may be outdated; check before relying on it:")
                && long.contains("truncated; query blackboard"),
        ),
        (false, true, false, true)
    );
}
