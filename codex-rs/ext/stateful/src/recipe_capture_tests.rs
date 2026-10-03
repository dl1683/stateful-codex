use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::ObservedCommand;
use super::ObservedCommands;
use super::carries_credentials;
use super::command_script;
use super::first_word;

fn command(call_id: &str, script: &str) -> ObservedCommand {
    ObservedCommand {
        turn_id: "turn-1".to_string(),
        call_id: call_id.to_string(),
        script: script.to_string(),
        cwd: PathBuf::from("/repo"),
    }
}

#[test]
fn only_successful_commands_of_the_same_thread_ground_a_recipe() {
    let observed = ObservedCommands::default();
    observed.started(
        "thread-1",
        command("ok", ".venv/bin/python -m pytest tests/test_cli.py -q"),
    );
    observed.finished("ok", /*succeeded*/ true);
    observed.started("thread-1", command("failed", "python -m pytest -q"));
    observed.finished("failed", /*succeeded*/ false);
    observed.started("thread-2", command("other", "uv run pytest"));
    observed.finished("other", /*succeeded*/ true);
    let recipe = "Recipe: `.venv/bin/python  -m pytest tests/test_cli.py -q` runs the CLI tests.";

    let found = |content: &str| {
        observed
            .matching("thread-1", content)
            .map(|found| found.call_id)
    };
    assert_eq!(
        [
            found(recipe),
            found("Recipe: `python -m pytest -q`"),
            found("Recipe: `uv run pytest`"),
            found("Recipe: run the tests with pytest"),
        ],
        [Some("ok".to_string()), None, None, None]
    );
}

#[test]
fn shell_wrappers_yield_their_script_and_its_first_word() {
    let argv = |words: &[&str]| words.iter().map(ToString::to_string).collect::<Vec<_>>();
    let scripts = [
        command_script(&argv(&["/bin/bash", "-lc", "uv run pytest -q"])),
        command_script(&argv(&[
            "powershell.exe",
            "-NoProfile",
            "-Command",
            "& '.venv/Scripts/python.exe' -m pytest",
        ])),
        command_script(&argv(&["git", "status"])),
    ];
    assert_eq!(
        scripts
            .iter()
            .map(|script| (script.as_str(), first_word(script)))
            .collect::<Vec<_>>(),
        vec![
            ("uv run pytest -q", "uv".to_string()),
            (
                "& '.venv/Scripts/python.exe' -m pytest",
                ".venv/Scripts/python.exe".to_string()
            ),
            ("git status", "git".to_string()),
        ]
    );
}

#[test]
fn credential_arguments_are_detected() {
    assert_eq!(
        [
            carries_credentials("Recipe: `deploy --password hunter2`"),
            carries_credentials("Recipe: `curl -H 'Authorization: Bearer abc'`"),
            carries_credentials("Recipe: `python -m pytest tests -q`"),
        ],
        [true, true, false]
    );
}
