//! Bounded content identity of the project workspace around a check. The host lists the
//! project's files and hashes their content before and after a matched check; any difference
//! outside the active criteria's pins makes the check a mutation of inputs nothing pins. File
//! metadata (size, modification time) is never identity.
//!
//! Every project root must be inside a git work tree. The listed files are those git tracks or
//! would track (untracked files that `.gitignore` does not exclude), and the repository state
//! (the commit `HEAD` names, or a verified unborn `HEAD`, and the staged index) is a separate
//! typed part of the identity that no file pin excludes. Outside git, or on any git failure,
//! the identity is unavailable, which fails the check closed. It is also unavailable for
//! anything it cannot represent exactly: submodules (gitlinks), nested repositories or other
//! non-regular entries, and path names that are not exact on this platform. Enumeration is
//! streamed and bounded in paths, output bytes, hashed bytes and time, and it runs in one of
//! the shared reader slots, held until the worker exits.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
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

/// Content identity of the workspace.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Manifest {
    /// Content digest of every listed path.
    files: BTreeMap<PathBuf, String>,
    /// State of every git repository a root belongs to, by root.
    repositories: BTreeMap<PathBuf, RepositoryState>,
}

/// The commit `HEAD` names and the digest of the staged index.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RepositoryState {
    head: String,
    index: String,
}

/// Bounds of one identity.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    /// Listed paths.
    pub(crate) entries: usize,
    /// File bytes hashed.
    pub(crate) bytes: u64,
    /// Bytes read from one git enumeration.
    pub(crate) git_output: u64,
    /// Time for the whole identity.
    pub(crate) budget: Duration,
}

pub(crate) const LIMITS: Limits = Limits {
    entries: 50_000,
    bytes: 512 * 1024 * 1024,
    git_output: 16 * 1024 * 1024,
    budget: Duration::from_secs(10),
};

/// Git file mode of a submodule entry.
const GITLINK_MODE: &[u8] = b"160000";

/// Whether the repository state, or any file outside every active criterion's pins, differs
/// between two identities; an unavailable identity counts as a difference.
pub(crate) fn changed_unpinned(
    roots: &[String],
    ledger: &AcceptanceLedger,
    start: Option<&Manifest>,
    end: Option<&Manifest>,
) -> bool {
    let (Some(start), Some(end)) = (start, end) else {
        return true;
    };
    if start.repositories != end.repositories {
        return true;
    }
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
    let differs = |path: &PathBuf| start.files.get(path) != end.files.get(path);
    start
        .files
        .keys()
        .chain(end.files.keys())
        .any(|path| differs(path) && !pinned.contains(path))
}

/// The workspace's content identity, or `None` when it cannot be taken within the bounds or
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
    let worker = tokio::task::spawn_blocking(move || {
        let manifest = identify(&roots, LIMITS);
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        manifest
    });
    tokio::time::timeout(LIMITS.budget, worker)
        .await
        .ok()?
        .ok()?
}

/// Takes the identity of `roots` within `limits`; `None` when it cannot be taken exactly.
pub(crate) fn identify(roots: &[PathBuf], limits: Limits) -> Option<Manifest> {
    let deadline = Instant::now() + limits.budget;
    let mut manifest = Manifest::default();
    let mut hashed = 0_u64;
    for root in roots {
        let (state, listed) = git_listing(root, limits, deadline)?;
        manifest.repositories.insert(root.clone(), state);
        for relative in listed {
            if manifest.files.len() >= limits.entries || Instant::now() >= deadline {
                return None;
            }
            let path = root.join(relative);
            let digest = content_digest(&path, &mut hashed, limits, deadline)?;
            manifest.files.insert(path.components().collect(), digest);
        }
    }
    Some(manifest)
}

/// The repository state and the tracked and untracked non-ignored paths under `root`,
/// relative to it; `None` outside a git work tree or when the listing cannot be taken exactly
/// (a git failure, a submodule, an inexact name, or a bound exceeded).
fn git_listing(
    root: &Path,
    limits: Limits,
    deadline: Instant,
) -> Option<(RepositoryState, BTreeSet<PathBuf>)> {
    let collect = |arguments: &[&str]| {
        let mut output = Vec::new();
        let succeeded = git_records(root, arguments, b'\n', limits, deadline, |record| {
            output.extend_from_slice(record);
            Some(())
        })?;
        Some((succeeded, output))
    };
    if collect(&["rev-parse", "--is-inside-work-tree"])? != (true, b"true".to_vec()) {
        return None;
    }
    let head = match collect(&["rev-parse", "--verify", "--quiet", "HEAD"])? {
        (true, head) if !head.is_empty() => String::from_utf8(head).ok()?,
        // `HEAD` names no commit: qualified only when it is a branch that does not exist yet.
        (false, _) => {
            let (symbolic, branch) = collect(&["symbolic-ref", "--quiet", "HEAD"])?;
            if !symbolic || branch.is_empty() {
                return None;
            }
            let branch = String::from_utf8(branch).ok()?;
            let (exists, _) = collect(&["show-ref", "--verify", "--quiet", &branch])?;
            if exists {
                return None;
            }
            format!("unborn:{branch}")
        }
        (true, _) => return None,
    };
    let mut index = Sha256::new();
    let mut paths = BTreeSet::new();
    let staged = git_records(
        root,
        &["ls-files", "-z", "--stage"],
        0,
        limits,
        deadline,
        |record| {
            index.update((record.len() as u64).to_be_bytes());
            index.update(record);
            let (entry, path) = record.split_at(record.iter().position(|byte| *byte == b'\t')?);
            if entry.starts_with(GITLINK_MODE) {
                return None;
            }
            add_path(&mut paths, &path[1..], limits)
        },
    )?;
    let untracked = git_records(
        root,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        0,
        limits,
        deadline,
        |record| add_path(&mut paths, record, limits),
    )?;
    if !staged || !untracked {
        return None;
    }
    Some((
        RepositoryState {
            head,
            index: format!("sha256:{:x}", index.finalize()),
        },
        paths,
    ))
}

/// Adds one git path, exactly as git names it, within the entry bound.
fn add_path(paths: &mut BTreeSet<PathBuf>, name: &[u8], limits: Limits) -> Option<()> {
    paths.insert(exact_path(name)?);
    (paths.len() <= limits.entries).then_some(())
}

/// The path git names with these bytes; `None` when this platform cannot represent it exactly.
fn exact_path(name: &[u8]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(OsString::from_vec(name.to_vec())))
    }
    #[cfg(not(unix))]
    {
        let name = std::str::from_utf8(name).ok()?;
        Some(PathBuf::from(OsString::from(name)))
    }
}

/// Streams the `terminator`-separated records of a read-only git query in `root` to
/// `on_record`, within the output-byte bound and the deadline, and returns whether git
/// succeeded; `None` when git cannot run, the callback refuses a record, or a bound is
/// exceeded (the process is then killed).
/// Repository configuration cannot start helper programs: the file-system monitor is disabled
/// and optional locks are off.
fn git_records(
    root: &Path,
    arguments: &[&str],
    terminator: u8,
    limits: Limits,
    deadline: Instant,
    mut on_record: impl FnMut(&[u8]) -> Option<()>,
) -> Option<bool> {
    let mut child = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .args(arguments)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let streamed = (|| {
        let stdout = child.stdout.take()?;
        let mut reader = BufReader::new(stdout.take(limits.git_output + 1));
        let mut record = Vec::new();
        let mut total = 0_u64;
        loop {
            record.clear();
            let read = reader.read_until(terminator, &mut record).ok()?;
            if read == 0 {
                return Some(());
            }
            total += read as u64;
            if total > limits.git_output || Instant::now() >= deadline {
                return None;
            }
            if record.last() == Some(&terminator) {
                record.pop();
            }
            on_record(&record)?;
        }
    })();
    if streamed.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some(child.wait().ok()?.success())
}

/// The content digest of one listed path: file bytes, a link's target, or a marker for an
/// absent path. `None` for a directory or special file (a submodule or nested repository
/// listed as a path, a FIFO, a device), which the identity cannot represent.
fn content_digest(
    path: &Path,
    hashed: &mut u64,
    limits: Limits,
    deadline: Instant,
) -> Option<String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some("absent".to_string());
        }
        Err(_) => return None,
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path).ok()?;
        return Some(format!(
            "link:{:x}",
            Sha256::digest(target.as_os_str().as_encoded_bytes())
        ));
    }
    if !metadata.is_file() {
        return None;
    }
    let file = open_nonblocking(path).ok()?;
    let length = file.metadata().ok()?.len();
    *hashed += length;
    if *hashed > limits.bytes {
        return None;
    }
    let stop = || Instant::now() >= deadline;
    hash_bounded(file, length, &stop).ok()
}

#[cfg(test)]
#[path = "acceptance_workspace_tests.rs"]
mod tests;
