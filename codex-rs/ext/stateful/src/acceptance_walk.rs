//! Streamed, iterative enumeration for roots that do not own a repository.
//! Only pending directories and listed paths are retained. Validating each directory before
//! entering it supplies the ancestor chain for the shared leaf reader, without retaining or
//! repeatedly scanning every ancestor of a deep tree. Output order comes from the manifest's
//! BTreeMap, not filesystem enumeration order.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use crate::acceptance_identity_content::contained;
use crate::acceptance_identity_content::entry;
use crate::acceptance_identity_content::leaf_digest;
use crate::acceptance_identity_content::reparse;
use crate::acceptance_identity_content::retain;
use crate::acceptance_identity_reason::Bound;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_workspace::Limits;

#[cfg(windows)]
#[path = "acceptance_walk_windows.rs"]
mod windows;

const EXCLUDED: &[&str] = &[
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    ".tox",
    ".blackboard",
];

fn excluded_directory(name: &OsStr) -> bool {
    EXCLUDED.iter().any(|excluded| name == OsStr::new(excluded))
}

/// Only directory components are excluded; a leaf named `target` is still an input.
pub(super) fn excluded_untracked(relative: &Path) -> bool {
    relative.parent().is_some_and(|parent| {
        parent
            .components()
            .any(|component| excluded_directory(component.as_os_str()))
    })
}

/// Enumeration totals shared across all walk roots in one identity.
#[derive(Default)]
pub(super) struct WalkBudget {
    visits: usize,
    directories: usize,
}

pub(super) fn walk(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    retained: &mut usize,
    hashed: &mut u64,
    budget: &mut WalkBudget,
    limits: Limits,
    deadline: Instant,
) -> Result<(), IdentityGap> {
    if Instant::now() >= deadline {
        return Err(IdentityGap::Time);
    }
    if limits.pending == 0 {
        return Err(IdentityGap::Bound(Bound::Pending));
    }
    retain(root, retained, limits)?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if Instant::now() >= deadline {
            return Err(IdentityGap::Time);
        }
        budget.directories += 1;
        if budget.directories > limits.directories {
            return Err(IdentityGap::Bound(Bound::Directories));
        }
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|_| entry(&directory, EntryKind::Unreadable))?;
        if metadata.file_type().is_symlink() || reparse(&metadata) {
            return Err(entry(&directory, EntryKind::Reparse));
        }
        if !metadata.is_dir() {
            return Err(entry(&directory, EntryKind::Special));
        }
        contained(&directory)?;
        let entries =
            std::fs::read_dir(&directory).map_err(|_| entry(&directory, EntryKind::Unreadable))?;
        for item in entries {
            if Instant::now() >= deadline {
                return Err(IdentityGap::Time);
            }
            budget.visits += 1;
            if budget.visits > limits.visits {
                return Err(IdentityGap::Bound(Bound::Visits));
            }
            let item = item.map_err(|_| entry(&directory, EntryKind::Unreadable))?;
            let path = item.path();
            if path.as_os_str().as_encoded_bytes().len() > limits.path_length {
                return Err(IdentityGap::Bound(Bound::PathLength));
            }
            if item.file_name() == ".git" {
                return Err(entry(&path, EntryKind::EmbeddedRepository));
            }
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|_| entry(&path, EntryKind::Unreadable))?;
            let kind = metadata.file_type();
            let symlink = kind.is_symlink();
            #[cfg(windows)]
            let symlink = symlink && windows::is_symlink(&path)?;
            if !symlink && reparse(&metadata) {
                return Err(entry(&path, EntryKind::Reparse));
            }
            if kind.is_dir() {
                if excluded_directory(&item.file_name()) {
                    continue;
                }
                if pending.len() >= limits.pending {
                    return Err(IdentityGap::Bound(Bound::Pending));
                }
                retain(&path, retained, limits)?;
                pending.push(path);
            } else if kind.is_file() || symlink {
                if files.len() >= limits.entries {
                    return Err(IdentityGap::Bound(Bound::Entries));
                }
                retain(&path, retained, limits)?;
                let digest = leaf_digest(root, &path, &metadata, hashed, limits, deadline)?;
                files.insert(path, digest);
            } else {
                return Err(entry(&path, EntryKind::Special));
            }
        }
        *retained -= directory.as_os_str().as_encoded_bytes().len();
    }
    Ok(())
}
