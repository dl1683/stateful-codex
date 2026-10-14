//! Validated ancestor chains, stable absence, and bounded reads of listed workspace entries.
use std::collections::HashMap;
use std::fs::Metadata;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use sha2::Digest;
use sha2::Sha256;

use crate::acceptance_identity_reason::Bound;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_observation::hash_bounded;
use crate::acceptance_observation::open_nonblocking;
use crate::acceptance_workspace::Limits;

pub(super) fn entry(path: &Path, kind: EntryKind) -> IdentityGap {
    IdentityGap::Entry {
        path: path.to_path_buf(),
        kind,
    }
}

pub(super) fn retain(path: &Path, retained: &mut usize, limits: Limits) -> Result<(), IdentityGap> {
    let bytes = path.as_os_str().as_encoded_bytes().len();
    if bytes > limits.path_length {
        return Err(IdentityGap::Bound(Bound::PathLength));
    }
    *retained = retained.saturating_add(bytes);
    if *retained > limits.path_bytes {
        return Err(IdentityGap::Bound(Bound::RetainedBytes));
    }
    Ok(())
}

pub(super) fn content_digest(
    root: &Path,
    relative: &Path,
    ancestors: &mut HashMap<PathBuf, bool>,
    retained: &mut usize,
    hashed: &mut u64,
    limits: Limits,
    deadline: Instant,
) -> Result<String, IdentityGap> {
    let path = root.join(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(entry(&path, EntryKind::NotContained));
    }
    let mut directory = root.to_path_buf();
    for part in relative.parent().into_iter().flat_map(Path::components) {
        if Instant::now() >= deadline {
            return Err(IdentityGap::Time);
        }
        directory.push(part);
        let present = if let Some(present) = ancestors.get(&directory) {
            *present
        } else {
            retain(&directory, retained, limits)?;
            let present = match std::fs::symlink_metadata(&directory) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => return Err(entry(&directory, EntryKind::Unreadable)),
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || reparse(&metadata) {
                        return Err(entry(&directory, EntryKind::Reparse));
                    }
                    if !metadata.is_dir() {
                        return Err(entry(&directory, EntryKind::Special));
                    }
                    contained(&directory)?;
                    match std::fs::symlink_metadata(directory.join(".git")) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => return Err(entry(&directory, EntryKind::Unreadable)),
                        Ok(_) => return Err(entry(&directory, EntryKind::EmbeddedRepository)),
                    }
                    true
                }
            };
            ancestors.insert(directory.clone(), present);
            present
        };
        if !present {
            return Ok("absent".to_string());
        }
    }
    let metadata = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("absent".to_string());
        }
        Err(_) => return Err(entry(&path, EntryKind::Unreadable)),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(&path).map_err(|_| entry(&path, EntryKind::Unreadable))?;
        let canonical =
            std::fs::canonicalize(&path).map_err(|_| entry(&path, EntryKind::EscapingLink))?;
        if !canonical.starts_with(root) {
            return Err(entry(&path, EntryKind::EscapingLink));
        }
        let metadata =
            std::fs::metadata(&canonical).map_err(|_| entry(&path, EntryKind::Unreadable))?;
        if metadata.is_dir() {
            return Err(entry(&path, EntryKind::DirectoryLink));
        }
        let digest = read_regular(&canonical, &metadata, hashed, limits, deadline)?;
        return Ok(format!(
            "link:{:x}:{digest}",
            Sha256::digest(target.as_os_str().as_encoded_bytes())
        ));
    }
    read_regular(&path, &metadata, hashed, limits, deadline)
}

pub(super) fn check_handle(
    path: &Path,
    metadata: &Metadata,
    length: u64,
) -> Result<(), IdentityGap> {
    if reparse(metadata) {
        return Err(entry(path, EntryKind::Reparse));
    }
    if !metadata.is_file() {
        return Err(entry(path, EntryKind::Special));
    }
    if metadata.len() != length {
        return Err(entry(path, EntryKind::Unreadable));
    }
    Ok(())
}

fn read_regular(
    path: &Path,
    metadata: &Metadata,
    hashed: &mut u64,
    limits: Limits,
    deadline: Instant,
) -> Result<String, IdentityGap> {
    check_handle(path, metadata, metadata.len())?;
    contained(path)?;
    if Instant::now() >= deadline {
        return Err(IdentityGap::Time);
    }
    let file = open_nonblocking(path).map_err(|_| entry(path, EntryKind::Unreadable))?;
    let opened = file
        .metadata()
        .map_err(|_| entry(path, EntryKind::Unreadable))?;
    check_handle(path, &opened, metadata.len())?;
    contained(path)?;
    *hashed = hashed.saturating_add(opened.len());
    if *hashed > limits.bytes {
        return Err(IdentityGap::Bound(Bound::Bytes));
    }
    hash_bounded(file, opened.len(), &|| Instant::now() >= deadline).map_err(|_| {
        if Instant::now() >= deadline {
            IdentityGap::Time
        } else {
            entry(path, EntryKind::Unreadable)
        }
    })
}

fn contained(path: &Path) -> Result<(), IdentityGap> {
    let canonical = std::fs::canonicalize(path).map_err(|_| entry(path, EntryKind::Unreadable))?;
    if canonical != path {
        return Err(entry(path, EntryKind::NotContained));
    }
    Ok(())
}

fn reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
