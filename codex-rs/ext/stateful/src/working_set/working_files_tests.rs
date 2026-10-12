use pretty_assertions::assert_eq;

use super::working_files::FileSampleCoverage;
use super::working_files::FileWitness;
use super::working_files::WitnessOutcome;
use super::working_files::WorkingSetFileSample;
use super::working_files::compare_samples;
use super::working_files::sample_file;

fn sample(path: &str, event: &str, read: std::io::Result<Vec<u8>>) -> WorkingSetFileSample {
    let mut budget = 1 << 20;
    sample_file("remote-linux", "/work/repo", path, event, read, &mut budget)
}

#[test]
fn untracked_file_change_has_an_exact_content_witness() {
    let path = "src/click/config.py";
    let before = sample(
        path,
        "t7-end",
        Ok(b"A = 1\nDEFAULT_PROJECT_CONFIG = \".clickrc.toml\"\nB = 2\n".to_vec()),
    );
    let after = sample(
        path,
        "t8-start",
        Ok(b"A = 1\nDEFAULT_PROJECT_CONFIG = \"click.toml\"\nB = 2\n".to_vec()),
    );
    let WitnessOutcome::Changed(witness) = compare_samples(Some(&before), &after) else {
        panic!("changed content");
    };
    assert_eq!(
        FileWitness {
            before_sha: String::new(),
            after_sha: String::new(),
            ..witness.clone()
        },
        FileWitness {
            path: path.to_string(),
            before_sha: String::new(),
            after_sha: String::new(),
            before_event: "t7-end".to_string(),
            after_event: "t8-start".to_string(),
            first_line: 2,
            old: "DEFAULT_PROJECT_CONFIG = \".clickrc.toml\"".to_string(),
            new: "DEFAULT_PROJECT_CONFIG = \"click.toml\"".to_string(),
            partial: false,
        }
    );
    assert_ne!(witness.before_sha, witness.after_sha);
    assert_eq!(
        compare_samples(Some(&after), &after),
        WitnessOutcome::Unchanged
    );
    // No earlier sample: the before-state and origin stay unknown.
    assert_eq!(compare_samples(None, &after), WitnessOutcome::NoBaseline);
}

#[test]
fn unsampleable_files_certify_nothing() {
    let text = sample("a.txt", "e", Ok(b"x\n".to_vec()));
    let cases = [
        (
            sample("../outside.txt", "e", Ok(b"x".to_vec())),
            FileSampleCoverage::OutOfRoot,
        ),
        (
            sample("C:/abs.txt", "e", Ok(b"x".to_vec())),
            FileSampleCoverage::OutOfRoot,
        ),
        (
            sample("bin.dat", "e", Ok(vec![0, 159, 146])),
            FileSampleCoverage::Binary,
        ),
        (
            sample("big.txt", "e", Ok(vec![b'a'; 64 * 1024 + 1])),
            FileSampleCoverage::Oversized,
        ),
        (
            sample("gone.txt", "e", Err(std::io::ErrorKind::NotFound.into())),
            FileSampleCoverage::Unreadable,
        ),
    ];
    for (after, coverage) in cases {
        assert_eq!(after.coverage, coverage);
        assert_eq!(
            compare_samples(Some(&text), &after),
            WitnessOutcome::Unknown(coverage)
        );
    }
    let mut budget = 3;
    let over_budget = sample_file("x", "/r", "a.txt", "e", Ok(b"abcd".to_vec()), &mut budget);
    assert_eq!(
        (over_budget.coverage, budget),
        (FileSampleCoverage::Oversized, 3)
    );
}

#[test]
fn large_hunks_are_marked_partial() {
    let old: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let new: String = (0..40).map(|i| format!("LINE {i}\n")).collect();
    let WitnessOutcome::Changed(witness) = compare_samples(
        Some(&sample("f.txt", "a", Ok(old.into()))),
        &sample("f.txt", "b", Ok(new.into())),
    ) else {
        panic!("changed");
    };
    assert_eq!((witness.partial, witness.old.lines().count()), (true, 16));
}
