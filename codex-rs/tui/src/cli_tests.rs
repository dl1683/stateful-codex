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

    assert!(Cli::try_parse_from(["codex", "--stateful", "autonomous"]).is_err());
}
