use anyhow::Result;
use pretty_assertions::assert_eq;

#[test]
fn memory_input_errors_exit_two_before_initialization_and_help_succeeds() -> Result<()> {
    for target in [
        "malformed",
        "entry@bad",
        "entry@0",
        "@1",
        "entry@-1",
        "entry@",
    ] {
        for action in ["forget", "correct"] {
            let mut command = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
            command.args(["memory", "--thread", "invalid-thread", action, target]);
            if action == "correct" {
                command.arg("Preserve §3.2–§4 — α.");
            }
            let output = command.output()?;
            assert_eq!(output.status.code(), Some(2), "{action} {target}");
        }
    }
    for arguments in [
        vec!["memory", "--help"],
        vec!["memory", "--thread", "invalid-thread", "correct", "--help"],
    ] {
        let output = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
            .args(arguments)
            .output()?;
        assert_eq!(output.status.code(), Some(0));
    }
    let output = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .args(["memory", "--thread", "invalid-thread", "forget"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}
