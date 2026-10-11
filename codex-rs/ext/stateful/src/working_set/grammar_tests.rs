use pretty_assertions::assert_eq;

use super::grammar::command_key;
use super::grammar::labeled_value;
use super::grammar::locators;
use super::grammar::plain;
use super::grammar::programs;
use super::grammar::unavailable_program;

#[test]
fn locators_are_file_line_tokens_after_markdown() {
    assert_eq!(
        locators(&plain(
            "at **[`src/click/core.py:2616`](/C:/x.py:2616)**, then `a.rs:3`."
        )),
        ["src/click/core.py:2616", "a.rs:3"]
    );
    assert_eq!(
        locators("http://host:8080 and 12:30 and v1.2:3x"),
        Vec::<String>::new()
    );
}

#[test]
fn labeled_output_lines_are_strict_label_value_unit() {
    let label = |line: &str| labeled_value(line).map(|(name, value)| (name.to_string(), value));
    assert_eq!(
        label("throughput: 4,108.3 invocations/s"),
        Some((
            "throughput".to_string(),
            "4,108.3 invocations/s".to_string()
        ))
    );
    assert_eq!(
        label("  p95 latency: 12.5 ms"),
        Some(("p95 latency".to_string(), "12.5 ms".to_string()))
    );
    assert_eq!(label("status: ok"), None);
    assert_eq!(label("invocations: 12325 in 3.000s"), None);
    assert_eq!(
        label("elapsed: 3.000s"),
        Some(("elapsed".to_string(), "3.000s".to_string()))
    );
    assert_eq!(label(r"src\a.py:12:    x = 1"), None);
}

#[test]
fn unavailable_programs_come_from_the_shell_error_format() {
    let pwsh =
        "     | The term 'rg' is not recognized as a name of a cmdlet, function, script file";
    let cmd = "'fd' is not recognized as an internal or external command,";
    let assignment = "The term 'PYTHONPATH=src' is not recognized as a name of a cmdlet";
    assert_eq!(unavailable_program(pwsh).as_deref(), Some("rg"));
    assert_eq!(unavailable_program(cmd).as_deref(), Some("fd"));
    assert_eq!(
        unavailable_program("bash: line 1: ag: command not found").as_deref(),
        Some("ag")
    );
    assert_eq!(unavailable_program(assignment), None);
    assert_eq!(unavailable_program("2 failed"), None);
}

#[test]
fn assignments_are_not_programs_or_command_identity() {
    assert_eq!(
        programs("PYTHONPATH=src python -m x; git grep -n y | sort"),
        ["python", "git grep", "sort"]
    );
    assert_eq!(
        programs("$env:PYTHONPATH = 'src'; $p = Get-Content a.py; set X=1 && pytest -q"),
        ["pytest"]
    );
    assert_eq!(
        programs("Select-String -Pattern 'a|b'; git grep -n \"c|d\" | sort"),
        ["select-string", "git grep", "sort"]
    );
    assert_eq!(
        command_key("$env:PYTHONPATH='src'; python -m pytest -q"),
        command_key("python -m pytest -q")
    );
}
