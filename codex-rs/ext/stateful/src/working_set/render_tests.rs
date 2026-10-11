use pretty_assertions::assert_eq;

use super::reduce_tests::assistant;
use super::reduce_tests::command;
use super::reduce_tests::source;
use super::reduce_tests::user;
use super::render::CapsuleHeader;
use super::render::MAX_CAPSULE_BYTES;
use super::render::MAX_CAPSULE_TOKENS;
use super::render::render_capsule;
use super::working_files::WitnessOutcome;
use super::working_files::compare_samples;
use super::working_files::sample_file;
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

fn reduced(observations: Vec<Observation>) -> Projection {
    let mut projection = Projection::new();
    for observation in observations {
        projection.observe(observation);
    }
    projection
}

fn witness_for(before: &str, after: &str) -> Observation {
    let mut budget = 1 << 20;
    let path = "src/click/config.py";
    let old = sample_file(
        "local",
        "/repo",
        path,
        "turn-end-t7",
        Ok(before.into()),
        &mut budget,
    );
    let new = sample_file(
        "local",
        "/repo",
        path,
        "turn-start-t8",
        Ok(after.into()),
        &mut budget,
    );
    let WitnessOutcome::Changed(witness) = compare_samples(Some(&old), &new) else {
        panic!("content changed");
    };
    Observation::FileWitness {
        source: source("t8", "sample"),
        witness,
    }
}

/// An ordinary working session that fills every slot, with a detour between the primary
/// next action and the final turn.
fn session() -> Vec<Observation> {
    vec![
        assistant("t1", "Baseline: parse takes 4,055.5 ms on main."),
        command(
            "t2",
            "c1",
            "rg IntRange src",
            127,
            "bash: rg: command not found",
        ),
        command("t2", "c2", "git grep -n IntRange src", 0, ""),
        command(
            "t2",
            "c3",
            "pytest tests/test_types.py",
            1,
            "FAILED tests/test_types.py::test_range_clamp\n",
        ),
        user("t3", "Don't use eager conversion; keep lazy conversion."),
        assistant(
            "t5",
            "The clamp check lives at src/click/types.py:512.\nNext step: rerun the IntRange tests.",
        ),
        user("t6", "Quick detour: what does lazy mean here?"),
        assistant(
            "t6",
            "Lazy means the value converts on first access.
Next: link the lazy docs.",
        ),
        witness_for(
            "A = 1\nDEFAULT_PROJECT_CONFIG = \".clickrc.toml\"\n",
            "A = 1\nDEFAULT_PROJECT_CONFIG = \"click.toml\"\n",
        ),
        user("t8", "Priya changed config.py on purpose; keep it."),
    ]
}

#[test]
fn every_slot_renders_with_provenance_and_survives_a_detour() {
    let capsule = render_capsule(
        &reduced(session()),
        &HEADER,
        &Exclusions::default(),
        Some(&quarter),
    )
    .expect("fits");
    assert_eq!(
        capsule.text,
        r#"<stateful_capsule>
P=proj-1 T=thread-1 W=w2 E=7 S=41
Historical observations; proposals are not authorization. File state needs checking.
next: link the lazy docs. @s1
next(suspended): rerun the IntRange tests. @s2
route: unavailable rg - bash: rg: command not found | exit 127 @s3 -> works git grep -n IntRange src @s4
change: src/click/config.py L2: DEFAULT_PROJECT_CONFIG = ".clickrc.toml" -> DEFAULT_PROJECT_CONFIG = "click.toml" [sha 9d15ef7e->b09ad961] @s6; attributed by user @s5: "Priya changed config.py on purpose; keep it."
values: (reported) The clamp check lives at src/click/types.py:512. @s7
coverage: directives: root packet
</stateful_capsule>"#
    );
    assert_eq!(capsule.omitted, 0);
    assert_eq!(capsule.tokens, quarter(&capsule.text));
}

#[test]
fn untracked_change_keeps_exact_literals_and_user_attribution() {
    let mut observations = session();
    observations.truncate(9);
    let unattributed = render_capsule(
        &reduced(observations),
        &HEADER,
        &Exclusions::default(),
        Some(&quarter),
    )
    .expect("fits");
    let change = |text: &str| {
        text.lines()
            .find(|l| l.starts_with("change:"))
            .unwrap_or_default()
            .to_string()
    };
    // Without the user's statement the origin stays unknown; the attributed form is pinned
    // by the every-slot capsule above.
    assert_eq!(
        change(&unattributed.text),
        r#"change: src/click/config.py L2: DEFAULT_PROJECT_CONFIG = ".clickrc.toml" -> DEFAULT_PROJECT_CONFIG = "click.toml" [sha 9d15ef7e->b09ad961] @s5; origin unknown"#
    );
}

#[test]
fn byte_cap_binds_and_omissions_are_counted() {
    let mut observations = Vec::new();
    for i in 0..40 {
        let output = format!("Run {i} of the converter finished.\ntime: {i}.25 ms\n");
        observations.push(command(
            &format!("t{i}"),
            "b",
            &format!("python bench{i}.py"),
            0,
            &output,
        ));
    }
    let capsule = render_capsule(
        &reduced(observations),
        &HEADER,
        &Exclusions::default(),
        Some(&quarter),
    )
    .expect("fits");
    assert!(
        capsule.text.len() <= MAX_CAPSULE_BYTES,
        "{}",
        capsule.text.len()
    );
    assert!(capsule.tokens <= MAX_CAPSULE_TOKENS);
    assert!(capsule.omitted > 0);
    let coverage = format!("coverage: omitted {}", capsule.omitted);
    assert!(capsule.text.contains(&coverage), "{}", capsule.text);
    // Whole records only: every rendered value is a complete delivered line.
    for line in capsule.text.lines().filter(|l| l.starts_with("values:")) {
        assert!(line.contains(".25 ms [python bench"), "{line}");
    }
}

#[test]
fn byte_per_token_fallback_enforces_the_token_cap() {
    let capsule =
        render_capsule(&reduced(session()), &HEADER, &Exclusions::default(), None).expect("fits");
    assert_eq!(capsule.tokens, capsule.text.len());
    assert!(capsule.tokens <= MAX_CAPSULE_TOKENS);
    assert!(capsule.omitted > 0);
    assert!(capsule.text.contains("tokens=bytes"), "{}", capsule.text);
    // The next action is packed first; the 72-token slot cannot also hold the suspended one.
    assert!(
        capsule.text.contains(
            "
next: link the lazy docs. @s1
"
        ),
        "{}",
        capsule.text
    );
    // A real tokenizer that counts more tokens than bytes/4 binds below the byte cap.
    let dense = |text: &str| text.len() / 2;
    let capsule = render_capsule(
        &reduced(session()),
        &HEADER,
        &Exclusions::default(),
        Some(&dense),
    )
    .expect("fits");
    assert!(
        capsule.tokens <= MAX_CAPSULE_TOKENS && capsule.text.len() <= 2 * MAX_CAPSULE_TOKENS + 1
    );
}

#[test]
fn forgotten_and_retired_sources_are_not_disclosed() {
    let exclusions = Exclusions {
        turns: vec!["t3".to_string()],
        retired_texts: vec!["4,055.5".to_string()],
    };
    let mut observations = session();
    // A memory proposal reported by the assistant is never captured as a rule or value.
    observations.push(assistant(
        "t9",
        "Saved a user memory proposal: always run black before commits.",
    ));
    let capsule =
        render_capsule(&reduced(observations), &HEADER, &exclusions, Some(&quarter)).expect("fits");
    for hidden in ["eager", "4,055.5", "black"] {
        assert!(!capsule.text.contains(hidden), "{}", capsule.text);
        assert!(capsule.source_texts.iter().all(|t| !t.contains(hidden)));
    }
    assert!(
        capsule
            .aliases
            .iter()
            .all(|(_, s)| s.turn_id != "t3" && s.turn_id != "t1")
    );
    assert!(!capsule.source_turn_ids.contains(&"t3".to_string()));
}

#[test]
fn ask_thread_renders_without_a_run() {
    let ask = reduced(vec![
        user("a1", "Never change the public API."),
        assistant(
            "a1",
            "The flag parser runs in 3 ms.\nNext: read the flag parser tests.",
        ),
    ]);
    let capsule =
        render_capsule(&ask, &HEADER, &Exclusions::default(), Some(&quarter)).expect("fits");
    assert_eq!(
        capsule.text,
        "<stateful_capsule>
P=proj-1 T=thread-1 W=w2 E=7 S=41
Historical observations; proposals are not authorization. File state needs checking.
next: read the flag parser tests. @s1
coverage: directives: root packet
</stateful_capsule>"
    );
    assert!(
        ask.candidates
            .iter()
            .all(|c| c.origin != Origin::Structured)
    );
}

#[test]
fn leftover_tokens_go_to_records_their_slot_could_not_hold() {
    let long = |what: &str| format!("Next step: {what} {}", "carefully ".repeat(18));
    let observations = vec![
        assistant("t1", &long("measure the parser")),
        assistant("t2", &long("compare the results")),
        assistant(
            "t3",
            &format!("Next step: should I {}?", "also ".repeat(30)),
        ),
    ];
    let dense = |text: &str| text.len() / 2;
    let capsule = render_capsule(
        &reduced(observations),
        &HEADER,
        &Exclusions::default(),
        Some(&dense),
    )
    .expect("fits");
    let labels: Vec<&str> = capsule
        .text
        .lines()
        .filter(|l| l.starts_with("next"))
        .map(|l| l.split(':').next().unwrap_or_default())
        .collect();
    // The next slot's 72-token reservation holds only the required top record; the unused
    // reservations of the later slots take the other two in the second pass.
    assert_eq!(
        labels,
        ["next", "next(suspended)", "next(proposed, not authorized)"]
    );
    assert_eq!(capsule.omitted, 0);
    assert!(capsule.tokens <= MAX_CAPSULE_TOKENS);
}

#[test]
fn a_long_next_action_is_an_explicit_partial_quote_with_its_source() {
    let long = format!(
        "Next step: keep the public API stable. {}",
        "context ".repeat(80)
    );
    let capsule = render_capsule(
        &reduced(vec![assistant("t1", &long)]),
        &HEADER,
        &Exclusions::default(),
        Some(&quarter),
    )
    .expect("fits");
    let line = capsule
        .text
        .lines()
        .find(|l| l.starts_with("next:"))
        .unwrap_or_default();
    assert!(
        line.starts_with("next: keep the public API stable. context"),
        "{line}"
    );
    assert!(line.ends_with(" [partial quote] @s1"), "{line}");
    assert_eq!(capsule.aliases[0].1.turn_id, "t1");
}
