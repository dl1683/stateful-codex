use codex_project_intelligence::BlackboardKind;
use pretty_assertions::assert_eq;

use super::routine_summary_refusal;

#[test]
fn session_and_worktree_summaries_are_refused_but_outcomes_are_kept() {
    let cases = [
        (
            BlackboardKind::Fact,
            "Yesterday's read-only investigation narrowed the bug to Click.",
        ),
        (
            BlackboardKind::Note,
            "  Status: tests pass and the fix is staged.",
        ),
        (
            BlackboardKind::Claim,
            "Worktree has uncommitted edits in core.py.",
        ),
        (
            BlackboardKind::Fact,
            "In this session we added compact formatting.",
        ),
        (
            BlackboardKind::Decision,
            "Yesterday's choice stands: fix vendored Click, because consumers rely on it.",
        ),
        (
            BlackboardKind::RejectedApproach,
            "Status: SHIPIT_CONFIG contamination ruled out; unset in both repros.",
        ),
        (
            BlackboardKind::Fact,
            "Recipe: `python -m pytest tests/test_cli.py -q` passes with .venv.",
        ),
        (
            BlackboardKind::Fact,
            "The status line renders the session name first.",
        ),
    ];

    let refused = cases
        .iter()
        .map(|(kind, content)| routine_summary_refusal(*kind, content).is_some())
        .collect::<Vec<_>>();

    assert_eq!(
        refused,
        vec![true, true, true, true, false, false, false, false]
    );
}
