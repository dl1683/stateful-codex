use pretty_assertions::assert_eq;

use super::*;

const ANSWER: &str = "parse_config returns Result<Config, Error>.";

fn block(lines: &[&str]) -> String {
    format!("{ANSWER}\n\n{}", lines.join("\n"))
}

fn not_answered(reason: &str) -> Readiness {
    Readiness::NotAnswered(reason.to_string())
}

/// Only a final answer that explicitly declares no open issues is ready; every other
/// declaration, a missing block, or a malformed one keeps the ordinary route.
#[test]
fn readiness_admits_only_an_explicit_answer_with_no_open_issues() {
    let answered = block(&[
        "[stateful-outcome]",
        "disposition: answer",
        "open-issues: none",
        "[/stateful-outcome]",
    ]);
    assert_eq!(readiness(&answered), Readiness::Answered);
    // Trailing whitespace and CRLF line endings are tolerated.
    assert_eq!(
        readiness(&format!("{}\r\n  \n", answered.replace('\n', "\r\n"))),
        Readiness::Answered
    );

    let cases = [
        (ANSWER.to_string(), "the final message has no outcome block"),
        (String::new(), "the final message has no outcome block"),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: continue",
                "open-issues: none",
                "[/stateful-outcome]",
            ]),
            "the model declared that work continues",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: blocked",
                "open-issues:",
                "- The API key is missing.",
                "[/stateful-outcome]",
            ]),
            "the model declared that it is blocked",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: answer",
                "open-issues:",
                "- I could not check the Windows path.",
                "[/stateful-outcome]",
            ]),
            "the answer declares open issues",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: answer",
                "open-issues:",
                "[/stateful-outcome]",
            ]),
            "open-issues lists no issue; write open-issues: none",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: answer",
                "[/stateful-outcome]",
            ]),
            "the outcome block has no open-issues line",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: done",
                "open-issues: none",
                "[/stateful-outcome]",
            ]),
            "the outcome block has an unknown disposition",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: answer",
                "open-issues: none",
            ]),
            "the outcome block is not closed",
        ),
        (
            format!(
                "{}\nMore text after the block.",
                block(&[
                    "[stateful-outcome]",
                    "disposition: answer",
                    "open-issues: none",
                    "[/stateful-outcome]",
                ])
            ),
            "text follows the outcome block",
        ),
        (
            format!(
                "{ANSWER} [stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]"
            ),
            "the outcome block does not start on its own line",
        ),
        (
            format!(
                "[stateful-outcome]\n{}",
                block(&[
                    "[stateful-outcome]",
                    "disposition: answer",
                    "open-issues: none",
                    "[/stateful-outcome]",
                ])
            ),
            "the final message has more than one outcome block",
        ),
        (
            block(&[
                "[stateful-outcome]",
                "disposition: answer",
                "open-issues:",
                "* not a dash item",
                "[/stateful-outcome]",
            ]),
            "an open issue line is malformed",
        ),
    ];
    for (message, reason) in cases {
        assert_eq!(readiness(&message), not_answered(reason), "{message:?}");
    }

    let mut many = vec!["[stateful-outcome]", "disposition: blocked", "open-issues:"];
    many.extend(std::iter::repeat_n("- issue", MAX_OPEN_ISSUES + 1));
    many.push("[/stateful-outcome]");
    assert_eq!(
        readiness(&block(&many)),
        not_answered("the outcome block lists too many open issues")
    );
    let long_issue = format!("- {}", "x".repeat(MAX_OUTCOME_BLOCK_BYTES));
    assert_eq!(
        readiness(&block(&[
            "[stateful-outcome]",
            "disposition: blocked",
            "open-issues:",
            &long_issue,
            "[/stateful-outcome]",
        ])),
        not_answered("the outcome block is too long")
    );
}

/// T27: candidacy is granted once per run per process and consumed by the first turn that
/// binds to the run, whatever that turn does; a replayed start never re-grants it.
#[test]
fn candidacy_is_single_use_and_never_regranted() {
    let candidates = HostAnswerCandidates::default();
    assert!(candidates.grant("run-1", "thread-1"));
    // Replayed start of the same run.
    assert!(!candidates.grant("run-1", "thread-1"));
    // A turn on another thread consumes it without holding it.
    assert!(!candidates.take_for_turn("run-1", "thread-2"));
    assert!(!candidates.take_for_turn("run-1", "thread-1"));
    assert!(!candidates.grant("run-1", "thread-1"));

    assert!(candidates.grant("run-2", "thread-1"));
    assert!(candidates.take_for_turn("run-2", "thread-1"));
    assert!(!candidates.take_for_turn("run-2", "thread-1"));
    assert!(!candidates.grant("run-2", "thread-1"));

    // A run this process saw bound to a turn before any grant (a turn started first) is
    // never granted afterwards.
    assert!(!candidates.take_for_turn("run-3", "thread-1"));
    assert!(!candidates.grant("run-3", "thread-1"));
}
