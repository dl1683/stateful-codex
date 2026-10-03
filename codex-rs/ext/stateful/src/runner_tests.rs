use pretty_assertions::assert_eq;

use super::CommandForm;
use super::invoked_runner;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn a_shell_script_argv_keeps_its_exact_source_and_other_argv_is_display_only() {
    let script = CommandForm::of(&argv(&[
        "bash",
        "-lc",
        "TMPDIR=/tmp/good python -m pytest -q --basetemp=/tmp/good",
    ]));
    assert_eq!(
        script,
        CommandForm::Script {
            shell: "bash".to_string(),
            source: "TMPDIR=/tmp/good python -m pytest -q --basetemp=/tmp/good".to_string(),
        }
    );
    assert!(script.replayable());
    assert_eq!(script.runner(), Some("pytest"));
    let pwsh = CommandForm::of(&argv(&[
        r"C:\Program Files\PowerShell\7\pwsh.exe",
        "-Command",
        "$env:TMP='C:\\t'; python -m pytest",
    ]));
    assert_eq!(pwsh.shell(), Some("pwsh"));
    assert_eq!(pwsh.runner(), Some("pytest"));
    // A program that is not a shell keeps its argv; `-c` here is pytest's own option.
    let direct = CommandForm::of(&argv(&["python", "-m", "pytest", "-c", "pytest.ini"]));
    assert!(!direct.replayable());
    assert_eq!(direct.runner(), Some("pytest"));
    let metacharacters = CommandForm::of(&argv(&["pytest", "-k", "a;b"]));
    assert!(!metacharacters.replayable());
    assert_eq!(metacharacters.runner(), Some("pytest"));
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
        ("python script.py -m pytest", None),
        ("rg 'pytest|tox' README.md", None),
        ("Write-Output 'setup; pytest -q'", None),
    ] {
        assert_eq!(invoked_runner(source), runner, "{source}");
    }
}
