use pretty_assertions::assert_eq;

use super::command_source;
use super::invoked_runner;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn a_shell_script_argv_keeps_its_exact_source() {
    assert_eq!(
        command_source(&argv(&[
            "bash",
            "-lc",
            "TMPDIR=/tmp/good python -m pytest -q --basetemp=/tmp/good"
        ])),
        (
            "TMPDIR=/tmp/good python -m pytest -q --basetemp=/tmp/good".to_string(),
            Some("bash".to_string())
        )
    );
    assert_eq!(
        command_source(&argv(&[
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            "-Command",
            "$env:TMP='C:\\t'; python -m pytest"
        ])),
        (
            "$env:TMP='C:\\t'; python -m pytest".to_string(),
            Some("pwsh".to_string())
        )
    );
    assert_eq!(
        command_source(&argv(&["git", "commit", "-m", "two words"])),
        ("git commit -m \"two words\"".to_string(), None)
    );
}

#[test]
fn runners_are_recognized_by_what_runs_not_by_mentions() {
    for (source, runner) in [
        ("pytest -q", Some("pytest")),
        ("TMPDIR=/x python -m pytest tests", Some("pytest")),
        (
            "$env:TMP='C:\\t'; python -m pytest --basetemp=.t",
            Some("pytest"),
        ),
        ("cd src && uv run pytest", Some("pytest")),
        ("cargo test -p codex-core", Some("cargo test")),
        ("npm run test", Some("test task")),
        ("npx jest", Some("jest")),
        ("rg pytest README.md", None),
        ("echo pytest", None),
        ("cat tests/test_cargo_test.py", None),
        ("python script.py --pytest", None),
    ] {
        assert_eq!(invoked_runner(source), runner, "{source}");
    }
}
