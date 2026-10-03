use pretty_assertions::assert_eq;

use super::AnswerUnitKind;
use super::AnswerUnits;
use super::MAX_UNIT_BYTES;
use super::SourceText;
use super::answer_units;

/// The texts of `units`, after checking each span covers exactly its words in `answer`.
fn texts<'a>(answer: &str, units: &'a [SourceText]) -> Vec<&'a str> {
    for unit in units {
        let source = answer[unit.span.clone()]
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(source, unit.text);
    }
    units.iter().map(|unit| unit.text.as_str()).collect()
}

/// The debug2 session-1 final answer, as delivered.
const DEBUG2_S1: &str = "Root cause found: this is a bug in the interaction between `ConfigPath` and the vendored Click snapshot.

This explains the behavior:

- A present `~/.shipit/config.toml` works because the type returns early when the file exists.
- A fresh laptop with no config file fails.

I reproduced both reported commands with an empty temporary home:

```text
deploy api --dry-run                 -> exit 2
```

What was ruled out:

- Environment variable `SHIPIT_CONFIG`: absent during reproduction.
- Incorrect home-path expansion: the expected temporary home path was used.
- TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.
- `--env` and `--dry-run`: both variants fail identically.
- Test/runtime code changes: the worktree remains clean.

The tests pass because they cover a present default file and explicit missing files.

No code has been changed.";

#[test]
fn every_ruled_out_item_is_its_own_unit() {
    let units = answer_units(DEBUG2_S1);
    assert_eq!(
        texts(DEBUG2_S1, &units.ruled_out),
        vec![
            "Environment variable `SHIPIT_CONFIG`: absent during reproduction.",
            "Incorrect home-path expansion: the expected temporary home path was used.",
            "TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.",
            "`--env` and `--dry-run`: both variants fail identically.",
            "Test/runtime code changes: the worktree remains clean.",
        ]
    );
    assert_eq!(
        (
            units.open_checks.len(),
            units.decisions.len(),
            units.omitted.len()
        ),
        (0, 0, 0)
    );
}

#[test]
fn items_keep_their_continuation_lines_sub_items_and_code() {
    let answer = "## Ruled out\r\n\r\n1. Clock skew: both hosts use NTP.\r\n   Checked with:\r\n   ```\r\n   chronyc tracking\r\n   ```\r\n   - offset under 1ms\r\n2. DNS caching: flushed, same result.\r\n";
    let units = answer_units(answer);
    assert_eq!(
        texts(answer, &units.ruled_out),
        vec![
            "Clock skew: both hosts use NTP.\nChecked with:\n```\nchronyc tracking\n```\n- offset under 1ms",
            "DNS caching: flushed, same result.",
        ]
    );
}

#[test]
fn tentative_lists_and_items_are_not_rejections() {
    let answer = "Not ruled out yet:\n- the cache\n\nPossible causes we rejected too early:\n- the proxy\n\n**Ruled out:**\n- the network: same failure offline\n- the disk: still possible, not checked\n- Possibly SHIPIT_CONFIG; not checked yet\n";
    let units = answer_units(answer);
    assert_eq!(
        texts(answer, &units.ruled_out),
        vec!["the network: same failure offline"]
    );
}

#[test]
fn inline_quoted_and_fenced_lists_are_left_alone() {
    let answer = "Still ruled out: laptop differences, `--dry-run`, path expansion.\n\n> Ruled out:\n> - quoted item\n\n````\nRuled out:\n```\n- fenced item\n````\n\n\"Decision: use Redis\" was the old note.\n";
    assert_eq!(answer_units(answer), AnswerUnits::default());
}

#[test]
fn a_quoted_identifier_inside_an_item_is_kept() {
    let answer = "Ruled out:\n- \"SHIPIT_CONFIG\" contamination: unset in both shells\n";
    let units = answer_units(answer);
    assert_eq!(
        texts(answer, &units.ruled_out),
        vec!["\"SHIPIT_CONFIG\" contamination: unset in both shells"]
    );
}

#[test]
fn a_list_ends_at_the_next_paragraph() {
    let answer = "Ruled out:\n- one\n\nNext I will check the proxy.\n- unrelated bullet\n";
    let units = answer_units(answer);
    assert_eq!(texts(answer, &units.ruled_out), vec!["one"]);
}

#[test]
fn open_checks_are_listed_separately() {
    let answer = "Open checks:\n- Confirm the tests import the vendored click, not the installed one.\n- Run the CLI on a clean laptop.\n\nRuled out:\n- `--env`\n";
    let units = answer_units(answer);
    assert_eq!(texts(answer, &units.ruled_out), vec!["`--env`"]);
    assert_eq!(
        texts(answer, &units.open_checks),
        vec![
            "Confirm the tests import the vendored click, not the installed one.",
            "Run the CLI on a clean laptop.",
        ]
    );
}

#[test]
fn a_decision_keeps_its_reason_alternatives_and_condition() {
    let answer = "**Decision:** store sessions in SQLite.\n**Why:** it is embedded and already a dependency;\nno server to run.\nAlternatives considered:\n- Redis\n- flat JSON files\nRevisit if: we need multi-host access.\n\nThat is all.";
    let units = answer_units(answer);
    let [decision] = units.decisions.as_slice() else {
        panic!("expected one decision, got {units:?}");
    };
    assert_eq!(
        (
            texts(answer, std::slice::from_ref(&decision.choice)),
            decision
                .reason
                .as_ref()
                .map(|reason| texts(answer, std::slice::from_ref(reason))),
            texts(answer, &decision.alternatives),
            decision
                .reconsider_if
                .as_ref()
                .map(|condition| texts(answer, std::slice::from_ref(condition))),
        ),
        (
            vec!["store sessions in SQLite."],
            Some(vec![
                "it is embedded and already a dependency;\nno server to run."
            ]),
            vec!["Redis", "flat JSON files"],
            Some(vec!["we need multi-host access."]),
        )
    );
    let whole = &answer[decision.span()];
    assert_eq!(
        (whole.starts_with("store"), whole.ends_with("access.")),
        (true, true)
    );
}

#[test]
fn a_decision_without_a_reason_says_so() {
    let answer = "Decision: keep the per-day schedule.\n";
    let units = answer_units(answer);
    assert_eq!(
        units
            .decisions
            .iter()
            .map(super::DecisionUnit::content)
            .collect::<Vec<_>>(),
        vec!["Decision: keep the per-day schedule.\nReason: not recorded in the answer"]
    );
}

#[test]
fn an_inline_because_is_the_reason() {
    let answer = "Decision: use rustls because the target has no OpenSSL.";
    let units = answer_units(answer);
    let [decision] = units.decisions.as_slice() else {
        panic!("expected one decision, got {units:?}");
    };
    assert_eq!(
        (
            texts(answer, std::slice::from_ref(&decision.choice)),
            decision
                .reason
                .as_ref()
                .map(|reason| texts(answer, std::slice::from_ref(reason))),
        ),
        (vec!["use rustls"], Some(vec!["the target has no OpenSSL."]))
    );
}

#[test]
fn an_unmade_decision_is_not_a_decision() {
    let answer = "Decision: pending your input.\n\nDecision: TBD after the benchmark.\n";
    assert_eq!(answer_units(answer), AnswerUnits::default());
}

#[test]
fn an_overlength_unit_is_omitted_not_cut() {
    let long = "x".repeat(MAX_UNIT_BYTES + 1);
    let answer = format!("Ruled out:\n- {long}\n- short\n");
    let units = answer_units(&answer);
    assert_eq!(texts(&answer, &units.ruled_out), vec!["short"]);
    assert_eq!(
        units.omitted,
        vec![(AnswerUnitKind::RuledOut, "x".repeat(80))]
    );
}

#[test]
fn typographic_apostrophes_still_mark_a_tentative_heading() {
    assert_eq!(
        answer_units("Couldn\u{2019}t be ruled out:\n- the cache\n"),
        AnswerUnits::default()
    );
}
