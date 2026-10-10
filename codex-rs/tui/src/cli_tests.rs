use super::*;
use clap::Parser;
use pretty_assertions::assert_eq;

#[test]
fn stateful_cli_requires_a_goal_and_preserves_the_explicit_mode() {
    let cli = Cli::try_parse_from([
        "codex",
        "--stateful",
        "collaborative",
        "--stateful-project",
        "project-1",
        "Investigate the project",
    ])
    .expect("valid Stateful CLI");
    assert_eq!(cli.stateful_mode, Some(StatefulModeCliArg::Collaborative));
    assert_eq!(cli.stateful_project.as_deref(), Some("project-1"));
    assert_eq!(cli.prompt.as_deref(), Some("Investigate the project"));

    // A run mode without a prompt parses; startup refuses it for lack of a goal.
    let goalless = Cli::try_parse_from(["codex", "--stateful", "autonomous"]).expect("parses");
    assert_eq!(
        (goalless.stateful_mode, goalless.prompt),
        (Some(StatefulModeCliArg::Autonomous), None)
    );
}

#[test]
fn a_bare_stateful_flag_selects_ask() {
    let parsed = [
        vec!["codex", "--stateful"],
        vec!["codex", "What did we decide?", "--stateful"],
        vec!["codex", "--stateful", "ask", "What did we decide?"],
    ]
    .into_iter()
    .map(|args| {
        let cli = Cli::try_parse_from(args).expect("valid Ask CLI");
        (cli.stateful_mode, cli.prompt)
    })
    .collect::<Vec<_>>();
    assert_eq!(
        parsed,
        vec![
            (Some(StatefulModeCliArg::Ask), None),
            (
                Some(StatefulModeCliArg::Ask),
                Some("What did we decide?".to_string())
            ),
            (
                Some(StatefulModeCliArg::Ask),
                Some("What did we decide?".to_string())
            ),
        ]
    );
}
