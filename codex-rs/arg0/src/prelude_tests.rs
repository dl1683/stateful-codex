#[cfg(unix)]
use super::Arg0Prelude;
#[cfg(unix)]
use super::create_alias;
use pretty_assertions::assert_eq;

#[test]
fn refused_hook_stops_normal_initialization() {
    let error = crate::arg0_dispatch_with_prelude(|| anyhow::bail!("prelude refused"))
        .err().expect("refused before setup");
    assert_eq!(error.to_string(), "prelude refused");
}

#[cfg(unix)]
#[test]
fn disposable_aliases_are_independent_executable_files() -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("original");
    std::fs::write(&executable, b"#!/bin/sh\nexit 7\n")?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    for name in ["apply_patch", "applypatch", "codex-linux-sandbox", "codex-execve-wrapper"] {
        let alias = directory.path().join(name);
        create_alias(&executable, &alias, Arg0Prelude::Disposable)?;
        let metadata = std::fs::symlink_metadata(&alias)?;
        assert!(metadata.is_file());
        assert_ne!(metadata.ino(), std::fs::metadata(&executable)?.ino());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(std::process::Command::new(&alias).status()?.code(), Some(7));
        std::fs::write(alias, b"changed")?;
        assert_eq!(std::fs::read(&executable)?, b"#!/bin/sh\nexit 7\n");
    }
    let normal = directory.path().join("normal");
    create_alias(&executable, &normal, Arg0Prelude::Normal)?;
    assert!(std::fs::symlink_metadata(normal)?.file_type().is_symlink());
    assert!(create_alias(&executable, &directory.path().join("missing/alias"), Arg0Prelude::Disposable).is_err());
    Ok(())
}
