//! Bounded content identity of the project workspace around a check. The host lists the
//! project's files and hashes their content before and after a matched check; any difference
//! outside the active criteria's pins makes the check a mutation of inputs nothing pins. File
//! metadata (size, modification time) is never identity.
//!
//! In a git work tree the listed files are those git tracks or would track (untracked files
//! that `.gitignore` does not exclude), plus the commit `HEAD` names and the staged index, so
//! ordinary repositories with build outputs and dependencies are covered by what they declare
//! as source. Elsewhere every file outside version-control metadata and dependency or build
//! caches is listed. The listing is bounded in files, bytes, time and workers (one of the
//! shared reader slots, held until the worker exits); anything over budget or unreadable makes
//! the identity unavailable, which fails the check closed.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceState;
use sha2::Digest;
use sha2::Sha256;

use crate::acceptance_observation::MAX_OUTSTANDING_READERS;
use crate::acceptance_observation::OUTSTANDING_READERS;
use crate::acceptance_observation::hash_bounded;
use crate::acceptance_observation::open_nonblocking;

/// Content identity of the workspace: digest by path, plus repository state entries.
pub(crate) type Manifest = BTreeMap<PathBuf, String>;

/// Larger workspaces cannot be identified around a check; its effects are then unknown.
const MAX_MANIFEST_ENTRIES: usize = 50_000;
/// Total bytes hashed for one identity.
const MAX_MANIFEST_BYTES: u64 = 512 * 1024 * 1024;
/// Time budget for one identity.
const MANIFEST_BUDGET: Duration = Duration::from_secs(10);
/// Directories skipped outside git: version-control metadata and dependency or build caches.
const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
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
];

/// Whether any file outside every active criterion's pins differs between two identities; an
/// unavailable identity counts as a difference.
pub(crate) fn changed_unpinned(
    roots: &[String],
    ledger: &AcceptanceLedger,
    start: Option<&Manifest>,
    end: Option<&Manifest>,
) -> bool {
    let (Some(start), Some(end)) = (start, end) else {
        return true;
    };
    let Some(root) = roots
        .first()
        .and_then(|root| std::fs::canonicalize(root).ok())
    else {
        return true;
    };
    let pinned = ledger
        .criteria
        .iter()
        .filter(|criterion| criterion.state == AcceptanceState::Active)
        .flat_map(|criterion| criterion.artifacts.iter().chain(&criterion.checker))
        .map(|path| {
            let path = Path::new(path);
            if path.is_absolute() {
                path.components().collect::<PathBuf>()
            } else {
                root.join(path).components().collect::<PathBuf>()
            }
        })
        .collect::<HashSet<_>>();
    let differs = |path: &PathBuf| start.get(path) != end.get(path);
    start
        .keys()
        .chain(end.keys())
        .any(|path| differs(path) && !pinned.contains(path))
}

/// The workspace's content identity, or `None` when it cannot be taken within the budget or
/// no reader slot is free.
pub(crate) async fn workspace_manifest(roots: &[String]) -> Option<Manifest> {
    let roots = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return None;
    }
    if OUTSTANDING_READERS.fetch_add(1, Ordering::SeqCst) >= MAX_OUTSTANDING_READERS {
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let deadline = Instant::now() + MANIFEST_BUDGET;
    let worker = tokio::task::spawn_blocking(move || {
        let manifest = identify(&roots, deadline);
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        manifest
    });
    tokio::time::timeout(MANIFEST_BUDGET, worker)
        .await
        .ok()?
        .ok()?
}

fn identify(roots: &[PathBuf], deadline: Instant) -> Option<Manifest> {
    let mut manifest = Manifest::new();
    let mut hashed = 0_u64;
    for root in roots {
        let files = match git_files(root, &mut manifest) {
            Some(files) => files,
            None => walk_files(root, deadline)?,
        };
        for path in files {
            if manifest.len() >= MAX_MANIFEST_ENTRIES || Instant::now() >= deadline {
                return None;
            }
            let digest = content_digest(&path, &mut hashed, deadline)?;
            manifest.insert(path.components().collect(), digest);
        }
    }
    Some(manifest)
}

/// In a git work tree: the tracked and untracked non-ignored files under `root`, with the
/// repository's `HEAD` and staged index recorded in `manifest`. `None` outside a work tree.
fn git_files(root: &Path, manifest: &mut Manifest) -> Option<Vec<PathBuf>> {
    let inside = git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim_ascii() != b"true" {
        return None;
    }
    let head = git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_default();
    manifest.insert(
        root.join(".git#HEAD"),
        String::from_utf8_lossy(&head).trim().to_string(),
    );
    let index = git(root, &["ls-files", "-z", "--stage"])?;
    manifest.insert(
        root.join(".git#index"),
        format!("sha256:{:x}", Sha256::digest(&index)),
    );
    let listed = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut files = listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| root.join(String::from_utf8_lossy(path).as_ref()))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Some(files)
}

/// Runs a read-only git query in `root`. Repository configuration cannot start helper
/// programs: the file-system monitor is disabled and optional locks are off.
fn git(root: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .args(arguments)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Outside git: every file under `root` except skipped directories, without following links.
fn walk_files(root: &Path, deadline: Instant) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        if Instant::now() >= deadline {
            return None;
        }
        for entry in std::fs::read_dir(&directory).ok()? {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;
            if metadata.is_dir() {
                let skipped = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name));
                if !skipped {
                    directories.push(entry.path());
                }
                continue;
            }
            if files.len() >= MAX_MANIFEST_ENTRIES {
                return None;
            }
            files.push(entry.path());
        }
    }
    files.sort();
    Some(files)
}

/// The content digest of one listed path: file bytes, a link's target, or a marker for an
/// absent path or a special file (never opened).
fn content_digest(path: &Path, hashed: &mut u64, deadline: Instant) -> Option<String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some("absent".to_string());
        }
        Err(_) => return None,
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path).ok()?;
        return Some(format!("link:{}", target.to_string_lossy()));
    }
    if !metadata.is_file() {
        return Some("special".to_string());
    }
    let file = open_nonblocking(path).ok()?;
    let length = file.metadata().ok()?.len();
    *hashed += length;
    if *hashed > MAX_MANIFEST_BYTES {
        return None;
    }
    let stop = || Instant::now() >= deadline;
    hash_bounded(file, length, &stop).ok()
}

#[cfg(test)]
#[path = "acceptance_workspace_tests.rs"]
mod tests;
