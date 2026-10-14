use super::*;
use crate::acceptance_workspace::LIMITS;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn enumerate(
    root: &Path,
    limits: Limits,
) -> (
    Result<(), IdentityGap>,
    WalkBudget,
    BTreeMap<PathBuf, String>,
) {
    let mut budget = WalkBudget::new(limits, Instant::now() + limits.budget);
    let mut files = BTreeMap::new();
    let mut retained = 0;
    let mut hashed = 0;
    let result = walk(
        &root.canonicalize().unwrap(),
        &mut files,
        &mut retained,
        &mut hashed,
        &mut budget,
        |directory| std::fs::read_dir(directory),
    );
    (result, budget, files)
}

#[test]
fn w2_w14_wide_files_stop_at_the_fourth_listed_name() {
    let project = TempDir::new().unwrap();
    for index in 0..20_000 {
        std::fs::write(project.path().join(format!("file{index}")), "x").unwrap();
    }
    let (result, budget, files) = enumerate(
        project.path(),
        Limits {
            entries: 3,
            ..LIMITS
        },
    );
    assert_eq!(result, Err(IdentityGap::Bound(Bound::Entries)));
    assert_eq!((budget.visits, files.len()), (4, 3));
}

#[test]
fn w14_wide_directories_overflow_pending_without_collection() {
    let project = TempDir::new().unwrap();
    for index in 0..30 {
        std::fs::create_dir(project.path().join(format!("dir{index}"))).unwrap();
    }
    let (result, budget, _) = enumerate(
        project.path(),
        Limits {
            pending: 5,
            ..LIMITS
        },
    );
    assert_eq!(result, Err(IdentityGap::Bound(Bound::Pending)));
    assert_eq!(budget.visits, 5);
}

#[test]
fn w2_visits_include_excluded_directories() {
    let project = TempDir::new().unwrap();
    std::fs::create_dir(project.path().join(".venv")).unwrap();
    std::fs::write(project.path().join(".venv/hidden"), "not visited").unwrap();
    assert_eq!(
        enumerate(
            project.path(),
            Limits {
                visits: 0,
                ..LIMITS
            }
        )
        .0,
        Err(IdentityGap::Bound(Bound::Visits))
    );
    let (result, budget, files) = enumerate(project.path(), LIMITS);
    assert_eq!(result, Ok(()));
    assert_eq!((budget.visits, budget.directories, files.len()), (1, 1, 0));
}

#[test]
fn w2_remaining_bounds_return_exact_reasons() {
    let project = TempDir::new().unwrap();
    std::fs::write(project.path().join("input"), "12345").unwrap();
    for (limits, bound) in [
        (Limits { bytes: 4, ..LIMITS }, Bound::Bytes),
        (
            Limits {
                directories: 0,
                ..LIMITS
            },
            Bound::Directories,
        ),
        (
            Limits {
                pending: 0,
                ..LIMITS
            },
            Bound::Pending,
        ),
        (
            Limits {
                path_bytes: 1,
                ..LIMITS
            },
            Bound::RetainedBytes,
        ),
        (
            Limits {
                path_length: 1,
                ..LIMITS
            },
            Bound::PathLength,
        ),
    ] {
        assert_eq!(
            enumerate(project.path(), limits).0,
            Err(IdentityGap::Bound(bound))
        );
    }
    assert_eq!(
        enumerate(
            project.path(),
            Limits {
                budget: std::time::Duration::ZERO,
                ..LIMITS
            }
        )
        .0,
        Err(IdentityGap::Time)
    );
}

#[test]
fn w2_w14_deep_visits_stop_at_fifty_one() {
    let project = TempDir::new().unwrap();
    let mut directory = project.path().canonicalize().unwrap();
    for _ in 0..200 {
        directory.push("d");
        std::fs::create_dir(&directory).unwrap();
    }
    let (result, budget, _) = enumerate(
        project.path(),
        Limits {
            visits: 50,
            ..LIMITS
        },
    );
    assert_eq!(result, Err(IdentityGap::Bound(Bound::Visits)));
    assert_eq!(budget.visits, 51);
}

// Portable counterpart to the Windows-only 5000-level witness. Even macOS's
// pathname limit accommodates this 200-level, one-character-component fixture.
#[test]
fn w14_portable_depth_succeeds_and_low_limits_refuse_exactly() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    let mut directory = root.clone();
    for _ in 0..200 {
        directory.push("d");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("f"), "x").unwrap();
    }
    let limits = Limits {
        path_bytes: 512 * 1024,
        path_length: 1024,
        budget: std::time::Duration::from_secs(/*secs*/ 30),
        ..LIMITS
    };
    let (result, budget, files) = enumerate(&root, limits);
    assert_eq!(
        (result, budget.visits, budget.directories, files.len()),
        (Ok(()), 400, 201, 200)
    );
    for (limits, bound) in [
        (
            Limits {
                visits: 50,
                ..limits
            },
            Bound::Visits,
        ),
        (
            Limits {
                directories: 25,
                ..limits
            },
            Bound::Directories,
        ),
        (
            Limits {
                pending: 25,
                ..limits
            },
            Bound::Pending,
        ),
        (
            Limits {
                path_bytes: 1024,
                ..limits
            },
            Bound::RetainedBytes,
        ),
        (
            Limits {
                path_length: root.as_os_str().as_encoded_bytes().len() + 20,
                ..limits
            },
            Bound::PathLength,
        ),
    ] {
        assert_eq!(enumerate(&root, limits).0, Err(IdentityGap::Bound(bound)));
    }
}

#[test]
#[cfg(windows)]
fn w14_five_thousand_directories_are_walked_iteratively() {
    let started = Instant::now();
    let project = TempDir::new().unwrap();
    let mut directory = project.path().canonicalize().unwrap();
    for _ in 0..5_000 {
        directory.push("d");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("f"), "x").unwrap();
    }
    eprintln!("created 5000 directories in {:?}", started.elapsed());
    // The frozen 1 MiB/4096-byte fixture cannot hold 5000 increasingly long listed
    // paths. Raise only test limits; production still enforces both frozen caps.
    let limits = Limits {
        path_bytes: 128 * 1024 * 1024,
        path_length: 32_000,
        budget: std::time::Duration::from_secs(/*secs*/ 900),
        ..LIMITS
    };
    let (result, budget, files) = enumerate(project.path(), limits);
    eprintln!("enumerated 5000 directories at {:?}", started.elapsed());
    let bounded = enumerate(
        project.path(),
        Limits {
            visits: 50,
            ..limits
        },
    )
    .0;
    // Remove bottom-up rather than asking the fixture cleanup to recurse 5000 levels.
    for _ in 0..5_000 {
        std::fs::remove_file(directory.join("f")).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        directory.pop();
    }
    eprintln!("cleaned up 5000 directories at {:?}", started.elapsed());
    assert_eq!(result, Ok(()));
    assert_eq!(
        (budget.visits, budget.directories, files.len()),
        (10_000, 5_001, 5_000)
    );
    assert_eq!(bounded, Err(IdentityGap::Bound(Bound::Visits)));
}

#[test]
fn bounds_are_totals_across_roots() {
    let holder = TempDir::new().unwrap();
    let roots = ["a", "b"].map(|name| {
        let root = holder.path().join(name);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("input"), "xxx").unwrap();
        root.canonicalize().unwrap()
    });
    for (limits, bound) in [
        (
            Limits {
                entries: 1,
                ..LIMITS
            },
            Bound::Entries,
        ),
        (
            Limits {
                visits: 1,
                ..LIMITS
            },
            Bound::Visits,
        ),
        (
            Limits {
                directories: 1,
                ..LIMITS
            },
            Bound::Directories,
        ),
        (
            Limits {
                pending: 1,
                ..LIMITS
            },
            Bound::Pending,
        ),
        (Limits { bytes: 4, ..LIMITS }, Bound::Bytes),
    ] {
        assert_eq!(
            crate::acceptance_workspace::identify(&roots, limits, Instant::now() + limits.budget),
            Err(IdentityGap::Bound(bound))
        );
    }
}

#[test]
fn retained_bytes_include_simultaneously_pending_directories() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    for name in ["a", "b", "c"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    let limits = Limits {
        path_bytes: root.as_os_str().as_encoded_bytes().len(),
        ..LIMITS
    };
    assert_eq!(
        enumerate(&root, limits).0,
        Err(IdentityGap::Bound(Bound::RetainedBytes))
    );
}

#[test]
fn overlapping_roots_have_identical_manifests_under_both_directory_pop_orders() {
    let project = TempDir::new().unwrap();
    let outer = project.path().canonicalize().unwrap().join("outer");
    for relative in ["a", "b"] {
        std::fs::create_dir_all(outer.join(relative)).unwrap();
        std::fs::write(outer.join(relative).join("f"), "x").unwrap();
    }
    let mut manifests = Vec::new();
    for reverse in [false, true] {
        for (bytes, expected) in [(3, Ok(())), (2, Err(IdentityGap::Bound(Bound::Bytes)))] {
            let limits = Limits {
                entries: 2,
                bytes,
                ..LIMITS
            };
            let mut budget = WalkBudget::new(limits, Instant::now() + limits.budget);
            let mut files = BTreeMap::new();
            let mut retained = 0;
            let mut hashed = 0;
            let mut entered = Vec::new();
            let result = [outer.join("a"), outer.clone()]
                .into_iter()
                .try_for_each(|root| {
                    walk(
                        &root,
                        &mut files,
                        &mut retained,
                        &mut hashed,
                        &mut budget,
                        |directory| {
                            entered.push(directory.to_path_buf());
                            // Force both stack-pop orders without changing production streaming.
                            let mut entries = std::fs::read_dir(directory)?
                                .collect::<std::io::Result<Vec<_>>>()?;
                            entries.sort_by_key(std::fs::DirEntry::file_name);
                            if reverse {
                                entries.reverse();
                            }
                            Ok(entries.into_iter().map(Ok))
                        },
                    )
                });
            assert_eq!(result, expected, "reverse enumeration: {reverse}");
            if result.is_ok() {
                let order = if reverse {
                    ["a", "", "a", "b"]
                } else {
                    ["a", "", "b", "a"]
                };
                assert_eq!(entered, order.map(|relative| outer.join(relative)));
                assert_eq!(hashed, 3);
                assert_eq!(
                    files.keys().cloned().collect::<Vec<_>>(),
                    ["a/f", "b/f"].map(|relative| outer.join(relative))
                );
                manifests.push(files);
            }
        }
    }
    assert_eq!(manifests[0], manifests[1]);
}

#[test]
fn sibling_subtree_eligibility_is_independent_of_enumeration_order() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    for relative in ["a", "b", "a/x", "a/y"] {
        std::fs::create_dir(root.join(relative)).unwrap();
    }
    let total_bytes = std::iter::once(root.clone())
        .chain(["a", "b", "a/x", "a/y"].map(|relative| root.join(relative)))
        .map(|path| path.as_os_str().as_encoded_bytes().len())
        .sum::<usize>();
    for reverse in [false, true] {
        for (limits, expected) in [
            (
                Limits {
                    pending: 2,
                    ..LIMITS
                },
                Err(IdentityGap::Bound(Bound::Pending)),
            ),
            (
                Limits {
                    pending: 4,
                    ..LIMITS
                },
                Err(IdentityGap::Bound(Bound::Pending)),
            ),
            (
                Limits {
                    path_bytes: total_bytes - root.join("b").as_os_str().as_encoded_bytes().len(),
                    ..LIMITS
                },
                Err(IdentityGap::Bound(Bound::RetainedBytes)),
            ),
            (
                Limits {
                    pending: 5,
                    path_bytes: total_bytes,
                    ..LIMITS
                },
                Ok(()),
            ),
        ] {
            let mut budget = WalkBudget::new(limits, Instant::now() + limits.budget);
            let mut files = BTreeMap::new();
            let mut retained = 0;
            let mut hashed = 0;
            let mut entered = Vec::new();
            let result = walk(
                &root,
                &mut files,
                &mut retained,
                &mut hashed,
                &mut budget,
                |directory| {
                    entered.push(directory.to_path_buf());
                    // Only the witness collects/sorts, to inject both orders reliably.
                    // Production uses read_dir and still streams each entry.
                    let mut entries =
                        std::fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
                    entries.sort_by_key(std::fs::DirEntry::file_name);
                    if reverse {
                        entries.reverse();
                    }
                    Ok(entries.into_iter().map(Ok))
                },
            );
            assert_eq!(result, expected, "reverse enumeration: {reverse}");
            if result.is_ok() {
                let order = if reverse {
                    ["", "a", "a/x", "a/y", "b"]
                } else {
                    ["", "b", "a", "a/y", "a/x"]
                };
                assert_eq!(entered, order.map(|relative| root.join(relative)));
                assert_eq!(
                    (
                        budget.visits,
                        budget.directories,
                        budget.scheduled,
                        retained,
                        files,
                        hashed
                    ),
                    (4, 5, 5, total_bytes, BTreeMap::new(), 0)
                );
            }
        }
    }
}
