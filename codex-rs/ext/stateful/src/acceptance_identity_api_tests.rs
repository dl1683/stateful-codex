use std::path::Path;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::acceptance_identity_content::check_handle;
use crate::acceptance_identity_reason::Bound;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityChange;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_identity_reason::Side;
use crate::acceptance_identity_worker::read_identity;
use crate::acceptance_workspace::LIMITS;
use crate::acceptance_workspace::Limits;

#[test]
fn w7_directory_handle_is_special_without_a_path_race() {
    let directory = TempDir::new().unwrap();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(/*flags*/ 0x0200_0000);
    }
    let handle = options.open(directory.path()).expect("directory handle");
    let metadata = handle.metadata().unwrap();
    assert_eq!(
        check_handle(directory.path(), &metadata, metadata.len()),
        Err(IdentityGap::Entry {
            path: directory.path().to_path_buf(),
            kind: EntryKind::Special,
        })
    );
}

#[test]
fn w9_typed_time_reason_is_specific() {
    assert_eq!(
        IdentityChange::Gap(Side::Start, IdentityGap::Time).render(),
        "workspace identity unavailable at start: identity deadline expired"
    );
    assert_eq!(
        IdentityChange::Path(Path::new("input.csv").to_path_buf()).render(),
        "the check changed a file no criterion pins: input.csv"
    );
}

#[tokio::test]
async fn w13_zero_budget_precedes_filesystem_work() {
    assert_eq!(
        read_identity(Duration::ZERO, |_| -> Result<(), IdentityGap> {
            panic!("must not read roots")
        })
        .await,
        Err(IdentityGap::Time)
    );
}

#[test]
fn git_bounds_have_exact_reasons() {
    let directory = TempDir::new().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(directory.path())
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(directory.path().join("input"), "x").unwrap();
    let roots = vec![directory.path().canonicalize().unwrap()];
    for (limits, gap) in [
        (
            Limits {
                entries: 0,
                ..LIMITS
            },
            IdentityGap::Bound(Bound::Entries),
        ),
        (
            Limits { bytes: 0, ..LIMITS },
            IdentityGap::Bound(Bound::Bytes),
        ),
        (
            Limits {
                git_output: 1,
                ..LIMITS
            },
            IdentityGap::Bound(Bound::GitOutput),
        ),
        (
            Limits {
                path_bytes: 1,
                ..LIMITS
            },
            IdentityGap::Bound(Bound::RetainedBytes),
        ),
        (
            Limits {
                path_length: 1,
                ..LIMITS
            },
            IdentityGap::Bound(Bound::PathLength),
        ),
        (
            Limits {
                budget: Duration::ZERO,
                ..LIMITS
            },
            IdentityGap::Time,
        ),
    ] {
        assert_eq!(
            crate::acceptance_workspace::identify(
                &roots,
                limits,
                std::time::Instant::now() + limits.budget
            ),
            Err(gap)
        );
    }
}

#[tokio::test]
async fn worker_panic_is_typed_and_releases_its_slot() {
    use std::sync::atomic::Ordering;
    let baseline = crate::acceptance_observation::OUTSTANDING_READERS.load(Ordering::SeqCst);
    assert_eq!(
        read_identity(
            Duration::from_secs(/*secs*/ 1),
            |_| -> Result<(), IdentityGap> { panic!("fixture panic") }
        )
        .await,
        Err(IdentityGap::Worker)
    );
    assert_eq!(
        crate::acceptance_observation::OUTSTANDING_READERS.load(Ordering::SeqCst),
        baseline
    );
}

#[test]
fn ancestor_cache_counts_retained_bytes_once() {
    let directory = TempDir::new().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let ancestor = root.join("a");
    std::fs::create_dir(&ancestor).unwrap();
    std::fs::write(ancestor.join("x"), "x").unwrap();
    std::fs::write(ancestor.join("y"), "y").unwrap();
    let limits = Limits {
        path_bytes: ancestor.as_os_str().as_encoded_bytes().len(),
        ..LIMITS
    };
    let mut ancestors = std::collections::HashMap::new();
    let mut retained = 0;
    let mut hashed = 0;
    for path in ["a/x", "a/y"] {
        crate::acceptance_identity_content::content_digest(
            &root,
            Path::new(path),
            &mut ancestors,
            &mut retained,
            &mut hashed,
            limits,
            std::time::Instant::now() + limits.budget,
        )
        .unwrap();
    }
    assert_eq!(retained, limits.path_bytes);
    assert_eq!(
        ancestors,
        std::collections::HashMap::from([(ancestor, true)])
    );
    ancestors.clear();
    retained = 0;
    assert_eq!(
        crate::acceptance_identity_content::content_digest(
            &root,
            Path::new("a/x"),
            &mut ancestors,
            &mut retained,
            &mut hashed,
            Limits {
                path_bytes: limits.path_bytes - 1,
                ..limits
            },
            std::time::Instant::now() + limits.budget
        ),
        Err(IdentityGap::Bound(Bound::RetainedBytes))
    );
}
