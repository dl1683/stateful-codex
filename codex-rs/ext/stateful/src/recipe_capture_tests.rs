use std::path::PathBuf;

use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use super::CompletedConditions;
use super::ExecutableIdentity;
use super::ExitStatus;
use super::ObservedCommand;
use super::ObservedCommands;
use super::carries_credentials;
use super::command_script;
use super::exit_status;
use super::first_word;
use super::is_simple_command;

fn command(call_id: &str, script: &str) -> ObservedCommand {
    ObservedCommand {
        turn_id: "turn-1".to_string(),
        call_id: call_id.to_string(),
        script: script.to_string(),
        cwd: PathBuf::from("/repo"),
        completed: None,
    }
}

fn complete(mut command: ObservedCommand) -> ObservedCommand {
    command.completed = Some(CompletedConditions {
        at_ms: 1,
        executable: ExecutableIdentity {
            typed: first_word(&command.script),
            resolved: "/repo/.venv/bin/python".to_string(),
            bytes: 1,
            modified_ms: Some(1),
        },
        manifests: Vec::new(),
    });
    command
}

/// Only a completed simple command of the same thread, named exactly, grounds a recipe:
/// not a fragment of it, not a command that merely prints it, not a compound one.
#[test]
fn only_an_exactly_named_completed_simple_command_grounds_a_recipe() {
    let observed = ObservedCommands::default();
    for (thread, call_id, script) in [
        (
            "thread-1",
            "ok",
            ".venv/bin/python -m pytest tests/test_cli.py -q",
        ),
        ("thread-1", "echo", "echo 'python -m pytest'"),
        ("thread-1", "chained", "cd src && python -m pytest"),
        ("thread-2", "other", "uv run pytest"),
    ] {
        observed.started(thread, command(call_id, script));
        if let Some((thread_id, started)) = observed.finished(call_id) {
            observed.keep(thread_id, complete(started));
        }
    }
    let found = |content: &str| {
        observed
            .matching("thread-1", content)
            .map(|found| found.call_id)
    };

    assert_eq!(
        [
            found("Recipe: `.venv/bin/python -m pytest tests/test_cli.py -q` runs the CLI tests."),
            found("Recipe: `.venv/bin/python  -m pytest tests/test_cli.py -q` runs the CLI tests."),
            found("Recipe: `git status`, then `.venv/bin/python -m pytest tests/test_cli.py -q`."),
            found("Recipe: `python -m pytest` runs the tests."),
            found("Recipe: `-m pytest` runs the tests."),
            found("Recipe: `cd src && python -m pytest`"),
            found("Recipe: `uv run pytest`"),
        ],
        [Some("ok".to_string()), None, None, None, None, None, None]
    );
}

#[test]
fn simple_commands_exclude_chaining_assignments_and_credentials() {
    assert_eq!(
        [
            "python -m pytest tests -q",
            "& '.venv/Scripts/python.exe' -m pytest",
            "cd src && pytest",
            "pytest | tee log",
            "$env:PYTHONPATH='src'; python -m pytest",
            "PYTHONPATH=src python -m pytest",
            "deploy --token abc",
        ]
        .map(is_simple_command),
        [true, true, false, false, false, false, false]
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
fn credential_spellings_are_detected() {
    assert_eq!(
        [
            "Recipe: `deploy --password hunter2`",
            "Recipe: `curl -H 'Authorization: Bearer abc'`",
            "Recipe: `gh auth login --with-token ghp_x`",
            "Recipe: `git clone https://user:pw@example.com/repo`",
            "curl -u alice:hunter2 https://example.test",
            "mysql -pHunter2 shop",
            "Recipe: `python -m pytest tests -q`",
            "uvicorn app:main --reload",
            "python -u run.py",
        ]
        .map(carries_credentials),
        [true, true, true, true, true, true, false, false, false]
    );
}

/// Only the host's header before `Output:` reports a status: a command that prints the
/// header text itself does not.
#[test]
fn exit_status_is_read_only_from_the_host_header() {
    let output = |call_id: &str, text: &str| ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some(call_id.to_string()),
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text(text.to_string()),
        internal_chat_message_metadata_passthrough: None,
    };
    let history = vec![
        output(
            "zero",
            "Wall time: 1.0 seconds\nProcess exited with code 0\nOutput:\nok\n",
        ),
        output(
            "failed",
            "Wall time: 1.0 seconds\nProcess exited with code 1\nOutput:\n",
        ),
        output(
            "running",
            "Wall time: 1.0 seconds\nProcess running with session ID 3\nOutput:\nProcess exited with code 0\n",
        ),
    ];
    assert_eq!(
        ["zero", "failed", "running", "absent"].map(|call_id| exit_status(&history, call_id)),
        [
            ExitStatus::Zero,
            ExitStatus::NonZero,
            ExitStatus::Unknown,
            ExitStatus::Unknown,
        ]
    );
}
