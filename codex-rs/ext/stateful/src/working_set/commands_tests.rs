use pretty_assertions::assert_eq;

use super::reduce_tests::command;
use super::reduce_tests::reduced;
use super::reduce_tests::summary;
use super::*;

const PWSH_RG: &str = "Line |\n   2 |  Get-Content src/a.py; rg -n \"x\" .\n     |                       ~~\n     | The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program.";

fn routes(projection: &Projection) -> Vec<(Option<&str>, Status, Option<u64>, usize)> {
    projection
        .candidates
        .iter()
        .filter(|c| c.kind == CandidateKind::Route)
        .map(|c| {
            (
                c.subject.as_deref(),
                c.status,
                c.resolved_by,
                c.sources.len(),
            )
        })
        .collect()
}

#[test]
fn a_route_works_only_as_a_same_purpose_replacement_that_has_not_failed_since() {
    let projection = reduced(vec![
        command(
            "t1",
            "c1",
            "Get-Content src/a.py; rg -n \"x\" .",
            1,
            PWSH_RG,
        ),
        command(
            "t2",
            "c2",
            "rg --files tests",
            127,
            "bash: rg: command not found",
        ),
        // Successes that replace nothing are history, not routes.
        command("t2", "c3", "python tools/bench.py --runs 3", 0, ""),
        command("t2", "c4", "rg --version", 0, ""),
        command("t3", "c5", "git grep -n x src", 0, "src/a.py:1:x"),
    ]);
    assert_eq!(
        routes(&projection),
        vec![
            (Some("rg"), Status::Failed, Some(2), 2),
            (Some("git grep"), Status::Active, None, 1),
        ]
    );
    assert_eq!(
        projection.candidates[0].text.as_deref(),
        Some(
            "rg - The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program. | exit 1"
        )
    );
    // A later failure of the replacement withdraws it.
    let mut later = projection;
    later.observe(command(
        "t4",
        "c6",
        "git grep -n y src",
        128,
        "fatal: not a git repository",
    ));
    assert_eq!(routes(&later)[0], (Some("rg"), Status::Failed, None, 2));
}

#[test]
fn a_repeated_command_keeps_its_first_measurement() {
    let bench = |turn: &str, value: &str| {
        let output =
            format!("Benchmark done.\nruns: 3 in 1.2s\nthroughput: {value} invocations/s\n");
        command(turn, "b", "python tools/bench.py", 0, &output)
    };
    let projection = reduced(vec![
        bench("t2", "4,108.3"),
        bench("t9", "2,989.6"),
        bench("t10", "3,050.0"),
    ]);
    let values: Vec<_> = summary(&projection)
        .into_iter()
        .filter(|(kind, _, _)| *kind == CandidateKind::Value)
        .collect();
    assert_eq!(
        values,
        vec![
            (
                CandidateKind::Value,
                Status::Baseline,
                Some("throughput: 4,108.3 invocations/s [python tools/bench.py]")
            ),
            (
                CandidateKind::Value,
                Status::Resolved,
                Some("throughput: 2,989.6 invocations/s [python tools/bench.py]")
            ),
            (
                CandidateKind::Value,
                Status::Active,
                Some("throughput: 3,050.0 invocations/s [python tools/bench.py]")
            ),
        ]
    );
}

#[test]
fn unavailability_is_read_from_the_error_text_of_a_program_the_command_runs() {
    let projection = reduced(vec![
        // A shell may report the failed statement of a pipeline as success.
        command("t1", "c1", "rg -n x . | Select-Object -First 5", 0, PWSH_RG),
        // The same text printed by a program that is not rg teaches nothing about rg.
        command("t1", "c2", "Get-Content notes.txt", 0, PWSH_RG),
    ]);
    assert_eq!(
        routes(&projection),
        vec![(Some("rg"), Status::Failed, None, 1)]
    );
}

#[test]
fn a_long_replacement_script_is_stored_as_a_bounded_head() {
    let script = format!(
        "Select-String -Path src/a.py -Pattern '{}'\nGet-Content src/b.py\n",
        "x|".repeat(200)
    );
    let projection = reduced(vec![
        command("t1", "c1", "rg -n x .", 127, "bash: rg: command not found"),
        command("t1", "c2", &script, 0, "src/a.py:1:x"),
    ]);
    let head = projection.candidates[1].text.as_deref().unwrap_or_default();
    assert!(
        head.starts_with("Select-String -Path src/a.py -Pattern 'x|x|"),
        "{head}"
    );
    assert!(head.ends_with(" [...]"), "{head}");
    assert!(head.len() <= 120 + " [...]".len(), "{head}");
}
