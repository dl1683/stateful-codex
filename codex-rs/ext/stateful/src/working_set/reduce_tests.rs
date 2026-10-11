use pretty_assertions::assert_eq;

use super::*;

pub(super) fn source(turn: &str, item: &str) -> SourceRef {
    SourceRef {
        thread_id: "thread-1".to_string(),
        turn_id: turn.to_string(),
        item_id: item.to_string(),
        invocation: Some(format!("host-{turn}-{item}")),
        digest: "d".repeat(64),
        binding_generation: 1,
        ..SourceRef::default()
    }
}

pub(super) fn assistant(turn: &str, text: &str) -> Observation {
    Observation::AssistantMessage {
        source: source(turn, "msg"),
        text: text.to_string(),
    }
}

pub(super) fn user(turn: &str, text: &str) -> Observation {
    Observation::UserMessage {
        source: source(turn, "user"),
        text: text.to_string(),
    }
}

pub(super) fn reduced(observations: Vec<Observation>) -> Projection {
    let mut projection = Projection::new();
    for observation in observations {
        projection.observe(observation);
    }
    projection
}

pub(super) fn summary(projection: &Projection) -> Vec<(CandidateKind, Status, Option<&str>)> {
    projection
        .candidates
        .iter()
        .map(|c| (c.kind, c.status, c.text.as_deref()))
        .collect()
}

#[test]
fn questions_stay_proposed_and_plan_completion_resolves() {
    let projection = reduced(vec![
        assistant("t1", "Done.\nNext step: should I rerun the IntRange tests?"),
        assistant("t1", "Next: update the changelog. Then stop."),
        Observation::PlanStepCompleted {
            text: "Update the changelog. Then stop.".to_string(),
        },
    ]);
    assert_eq!(
        summary(&projection),
        vec![
            (
                CandidateKind::Next,
                Status::Proposed,
                Some("should I rerun the IntRange tests?")
            ),
            (
                CandidateKind::Next,
                Status::Resolved,
                Some("update the changelog. Then stop.")
            ),
        ]
    );
}

#[test]
fn next_action_survives_a_detour_and_prose_is_not_captured() {
    let projection = reduced(vec![
        assistant("t5", "**Next step:** Run the full test suite."),
        user(
            "t6",
            "Quick detour: what does lazy mean here? Never import json eagerly.",
        ),
        assistant(
            "t6",
            "I chose lazy conversion because it is faster: 4,055.5 ms baseline.",
        ),
    ]);
    assert_eq!(
        summary(&projection),
        vec![(
            CandidateKind::Next,
            Status::Active,
            Some("Run the full test suite.")
        )]
    );
}

#[test]
fn only_user_messages_naming_a_task_file_are_kept_for_attribution() {
    let mut observations = vec![
        Observation::AppliedFileChange {
            path: "src/config.py".to_string(),
        },
        user("t1", "Keep the public API stable."),
    ];
    for i in 0..10 {
        observations.push(user(&format!("u{i}"), &format!("config.py note {i}")));
    }
    let projection = reduced(observations);
    let kept: Vec<_> = projection
        .candidates
        .iter()
        .map(|c| (c.kind, c.status, c.text.as_deref().unwrap_or_default()))
        .collect();
    assert_eq!(kept.len(), 10);
    assert_eq!(
        kept[0],
        (
            CandidateKind::Attribution,
            Status::Resolved,
            "config.py note 0"
        )
    );
    assert_eq!(
        kept[9],
        (
            CandidateKind::Attribution,
            Status::Active,
            "config.py note 9"
        )
    );
}

#[test]
fn markdown_wrapped_locators_are_extracted() {
    let projection = reduced(vec![
        assistant(
            "t1",
            "Click looks up `default_map` at **`src/click/core.py:2616`** (`ctx.lookup_default`).",
        ),
        assistant(
            "t2",
            "It is set at **[`src/a/b.py:12`](/C:/w/src/a/b.py:12)**, then read.",
        ),
        assistant("t3", "Today the lookup is `src/click/core.py:2616`."),
    ]);
    let found: Vec<_> = projection
        .candidates
        .iter()
        .map(|c| (c.subject.as_deref(), c.sources.len()))
        .collect();
    assert_eq!(
        found,
        vec![
            (Some("src/click/core.py:2616"), 2),
            (Some("src/a/b.py:12"), 1)
        ]
    );
    assert_eq!(
        projection.candidates[0].text.as_deref(),
        Some("Click looks up default_map at src/click/core.py:2616 (ctx.lookup_default).")
    );
}

#[test]
fn replayed_sources_coalesce_and_unknown_provenance_is_debt() {
    let mut projection = reduced(vec![
        assistant("t1", "Failure at src/click/core.py:2616 in parse."),
        assistant("t1", "Failure at src/click/core.py:2616 in parse."),
    ]);
    assert_eq!(projection.candidates.len(), 1);
    assert_eq!(projection.candidates[0].sources.len(), 1);
    let mut unknown = source("t2", "msg");
    unknown.thread_id.clear();
    projection.observe(Observation::AssistantMessage {
        source: unknown,
        text: "See src/a.py:3.".to_string(),
    });
    assert_eq!(
        (projection.candidates.len(), projection.capacity_debt),
        (1, 1)
    );
}

#[test]
fn saturation_keeps_unresolved_records_and_counts_debt() {
    let mut projection = Projection::new();
    for i in 0..(MAX_CANDIDATES + 3) {
        projection.observe(assistant(
            &format!("t{i}"),
            &format!("See src/f{i}.py:{i}."),
        ));
    }
    assert_eq!(projection.candidates.len(), MAX_CANDIDATES);
    assert_eq!(projection.capacity_debt, 3);
    assert_eq!(
        projection.candidates[0].text.as_deref(),
        Some("See src/f0.py:0.")
    );
    let payload = projection.validated_payload().expect("bounded payload");
    assert_eq!(Projection::from_payload(&payload), Some(projection.clone()));

    let mut overlong = projection.clone();
    overlong.candidates[0].sources = vec![source("a", "b"); MAX_REFERENCES + 1];
    assert_eq!(overlong.validated_payload(), Err(CapacityDebt::Candidate));
    let mut stale = projection;
    stale.grammar_version += 1;
    assert_eq!(Projection::from_payload(&stale.to_payload()), None);
}
