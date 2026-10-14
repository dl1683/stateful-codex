use super::*;
use pretty_assertions::assert_eq;

#[cfg(unix)]
#[test]
fn w7_fifo_is_refused_without_blocking_the_walk() {
    let project = TempDir::new().unwrap();
    let fifo = project.path().join("fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let start = Instant::now();
    assert_eq!(
        snapshot(project.path()),
        Err(IdentityGap::Entry {
            path: project.path().canonicalize().unwrap().join("fifo"),
            kind: EntryKind::Special,
        })
    );
    assert!(start.elapsed() < Duration::from_secs(/*secs*/ 2));
    // Windows counterpart: w7_walk_escaping_junction_is_refused_after_preflight.
}

#[cfg(windows)]
#[test]
fn w7_walk_escaping_junction_is_refused_after_preflight() {
    let project = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("data"), "outside").unwrap();
    let junction = project.path().join("src");
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .status()
        .unwrap();
    if !status.success() {
        eprintln!("not run (preflight): directory junction");
        return;
    }
    let identity = snapshot(project.path());
    std::fs::remove_dir(&junction).unwrap();
    assert_eq!(
        identity,
        Err(IdentityGap::Entry {
            path: project.path().canonicalize().unwrap().join("src"),
            kind: EntryKind::Reparse,
        })
    );
}

#[test]
fn w14_unreadable_directory_is_refused_after_preflight() {
    let project = TempDir::new().unwrap();
    let blocked = project.path().canonicalize().unwrap().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(/*mode*/ 0o000))
            .unwrap();
    }
    #[cfg(windows)]
    {
        assert!(
            Command::new("icacls")
                .arg(&blocked)
                .args([
                    "/deny",
                    &format!("{}:(R)", std::env::var("USERNAME").unwrap())
                ])
                .status()
                .unwrap()
                .success()
        );
    }
    let denied = std::fs::read_dir(&blocked).is_err();
    let identity = snapshot(project.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(/*mode*/ 0o755))
            .unwrap();
    }
    #[cfg(windows)]
    {
        assert!(
            Command::new("icacls")
                .arg(&blocked)
                .args(["/remove:d", &std::env::var("USERNAME").unwrap()])
                .status()
                .unwrap()
                .success()
        );
    }
    if !denied {
        eprintln!("not run (preflight): directory read denial does not take effect");
        return;
    }
    assert_eq!(
        identity,
        Err(IdentityGap::Entry {
            path: blocked,
            kind: EntryKind::Unreadable
        })
    );
}

#[test]
fn unreadable_root_git_marker_never_discovers_an_ignoring_parent_after_preflight() {
    let parent = super::super::repository();
    let project = parent.path().canonicalize().unwrap().join("build");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("input"), "input").unwrap();
    assert_eq!(
        super::super::git(parent.path(), &["check-ignore", "build/input"]),
        "build/input"
    );
    super::super::git(&project, &["init", "--quiet"]);
    assert!(snapshot(&project).is_ok());
    let marker = project.join(".git");
    #[cfg(unix)]
    let permissions = std::fs::metadata(&marker).unwrap().permissions();
    #[cfg(unix)]
    let applied = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(/*mode*/ 0o000)).is_ok()
    };
    #[cfg(windows)]
    let applied = Command::new("icacls")
        .arg(&marker)
        .args([
            "/deny",
            &format!("{}:(R)", std::env::var("USERNAME").unwrap()),
        ])
        .status()
        .is_ok_and(|status| status.success());
    if !applied {
        eprintln!("not run (preflight): root .git read denial could not be applied");
        return;
    }
    // Metadata must remain available: denial of reading the owned repository is
    // a Git gap, rather than an Unreadable gap from the marker metadata probe.
    let denied = std::fs::symlink_metadata(&marker).is_ok()
        && std::fs::read_dir(&marker)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied);
    let identity = denied.then(|| snapshot(&project));
    #[cfg(unix)]
    std::fs::set_permissions(&marker, permissions).unwrap();
    #[cfg(windows)]
    assert!(
        Command::new("icacls")
            .arg(&marker)
            .args(["/remove:d", &std::env::var("USERNAME").unwrap()])
            .status()
            .unwrap()
            .success()
    );
    if !denied {
        eprintln!("not run (preflight): root .git directory read denial does not take effect");
        return;
    }
    assert_eq!(identity, Some(Err(IdentityGap::Git)));
}

#[cfg(unix)]
#[test]
fn walk_preserves_non_utf8_names() {
    use std::os::unix::ffi::OsStringExt;
    let project = TempDir::new().unwrap();
    let name = OsString::from_vec(b"input\xff".to_vec());
    std::fs::write(project.path().join(&name), "x").unwrap();
    assert_eq!(
        snapshot(project.path())
            .unwrap()
            .files
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![project.path().canonicalize().unwrap().join(name)]
    );
}
