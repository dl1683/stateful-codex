//! Bounded content identity of the project workspace around a check. The host lists the
//! project's files and hashes their content before and after a matched check; any difference
//! outside the active criteria's pins makes the check a mutation of inputs nothing pins. File
//! metadata (size, modification time) is never identity.
//!
//! Every project root must be inside a git work tree. The listed files are those git tracks or
//! would track (untracked files that `.gitignore` does not exclude), and the repository state
//! (symbolic or detached `HEAD`, the staged index, and framed refs) is a separate
//! typed part of the identity that no file pin excludes. Outside git, or on any git failure,
//! the identity is unavailable, which fails the check closed. It is also unavailable for
//! anything it cannot represent exactly: submodules (gitlinks), nested repositories or other
//! non-regular entries, and path names that are not exact on this platform. A listed path
//! inside an embedded repository (any directory strictly below the root, between the root and
//! the path, that holds a `.git` file or directory) is refused too, even when the parent
//! tracks it as a regular file: the embedded repository's own state is not part of the
//! identity. Each such directory is examined once; a boundary that cannot be determined is
//! also unavailable. Enumeration is
//! streamed and bounded in paths, output bytes, hashed bytes and time, and it runs in one of
//! the shared reader slots, held until the worker exits.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceState;
use sha2::Digest;
use sha2::Sha256;

use crate::acceptance_identity_content::content_digest;
use crate::acceptance_identity_content::retain;
use crate::acceptance_identity_process::records;
use crate::acceptance_identity_reason::Bound;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityChange;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_identity_reason::Side;
use crate::acceptance_identity_worker::read_identity;

/// Content identity of the workspace.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Manifest {
    /// Content digest of every listed path.
    files: BTreeMap<PathBuf, String>,
    /// State keyed by the captured canonical roots.
    roots: BTreeMap<PathBuf, RootState>,
    first_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RootState {
    Git(RepositoryState),
    #[expect(dead_code, reason = "Walk mode belongs to stage 2B")]
    Walk,
}

/// Symbolic/object HEAD and the digests of the staged index and refs.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RepositoryState {
    head: HeadState,
    index: String,
    refs: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HeadState {
    Branch {
        name: String,
        object: Option<String>,
    },
    Detached {
        object: String,
    },
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
    /// Retained listed and cached ancestor path bytes.
    pub(crate) path_bytes: usize,
    /// Maximum path length in encoded bytes.
    pub(crate) path_length: usize,
    /// Time for the whole identity.
    pub(crate) budget: Duration,
}

pub(crate) const LIMITS: Limits = Limits {
    entries: 50_000,
    bytes: 512 * 1024 * 1024,
    git_output: 16 * 1024 * 1024,
    path_bytes: 16 * 1024 * 1024,
    path_length: 4096,
    budget: Duration::from_secs(10),
};

/// Git file mode of a submodule entry.
const GITLINK_MODE: &[u8] = b"160000";

/// Whether the repository state, or any file outside every active criterion's pins, differs
/// between two identities; an unavailable identity counts as a difference.
pub(crate) fn changed_unpinned(
    ledger: &AcceptanceLedger,
    start: Result<&Manifest, &IdentityGap>,
    end: Result<&Manifest, &IdentityGap>,
) -> Option<IdentityChange> {
    let start = match start {
        Ok(start) => start,
        Err(gap) => return Some(IdentityChange::Gap(Side::Start, gap.clone())),
    };
    let end = match end {
        Ok(end) => end,
        Err(gap) => return Some(IdentityChange::Gap(Side::End, gap.clone())),
    };
    if start.first_root != end.first_root || start.roots.keys().ne(end.roots.keys()) {
        return Some(IdentityChange::Roots);
    }
    if start.roots != end.roots {
        return Some(IdentityChange::Repository);
    }
    let root = &start.first_root;
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
        .find(|path| differs(path) && !pinned.contains(*path))
        .cloned()
        .map(IdentityChange::Path)
}

/// Take a slot before any filesystem call, and use one deadline for the entire worker.
pub(crate) async fn workspace_manifest(roots: &[String]) -> Result<Manifest, IdentityGap> {
    let roots = roots.to_vec();
    read_identity(LIMITS.budget, move |deadline| {
        let mut canonical = Vec::with_capacity(roots.len());
        for root in roots {
            if Instant::now() >= deadline {
                return Err(IdentityGap::Time);
            }
            let root = std::fs::canonicalize(root).map_err(|_| IdentityGap::Roots)?;
            if !std::fs::metadata(&root)
                .map_err(|_| IdentityGap::Roots)?
                .is_dir()
            {
                return Err(IdentityGap::Roots);
            }
            canonical.push(root);
        }
        identify(&canonical, LIMITS, deadline)
    })
    .await
}

/// Takes the identity of canonical roots using the caller's single deadline.
pub(crate) fn identify(
    roots: &[PathBuf],
    limits: Limits,
    deadline: Instant,
) -> Result<Manifest, IdentityGap> {
    if Instant::now() >= deadline {
        return Err(IdentityGap::Time);
    }
    if roots.is_empty() {
        return Err(IdentityGap::Roots);
    }
    let mut manifest = Manifest {
        first_root: roots[0].clone(),
        ..Manifest::default()
    };
    let mut hashed = 0_u64;
    let mut ancestors = HashMap::new();
    let mut retained = 0;
    for root in roots {
        let (state, listed) =
            git_listing(root, manifest.files.len(), &mut retained, limits, deadline)?;
        manifest.roots.insert(root.clone(), RootState::Git(state));
        for relative in listed {
            if manifest.files.len() >= limits.entries {
                return Err(IdentityGap::Bound(Bound::Entries));
            }
            if Instant::now() >= deadline {
                return Err(IdentityGap::Time);
            }
            let digest = content_digest(
                root,
                &relative,
                &mut ancestors,
                &mut retained,
                &mut hashed,
                limits,
                deadline,
            )?;
            manifest.files.insert(root.join(relative), digest);
        }
    }
    if Instant::now() >= deadline {
        return Err(IdentityGap::Time);
    }
    Ok(manifest)
}

/// The repository state and the tracked and untracked non-ignored paths under `root`,
/// relative to it; a gap outside a git work tree or when the listing cannot be taken exactly
/// (a git failure, a submodule, an inexact name, or a bound exceeded).
fn git_listing(
    root: &Path,
    already_listed: usize,
    retained: &mut usize,
    limits: Limits,
    deadline: Instant,
) -> Result<(RepositoryState, BTreeSet<PathBuf>), IdentityGap> {
    let collect = |arguments: &[&str]| {
        let mut output = Vec::new();
        let succeeded = git_records(root, arguments, b'\n', limits, deadline, |record| {
            output.extend_from_slice(record);
            Ok(())
        })?;
        Ok::<_, IdentityGap>((succeeded, output))
    };
    if collect(&["rev-parse", "--is-inside-work-tree"])? != (0, b"true".to_vec()) {
        return Err(IdentityGap::Git);
    }
    let symbolic = collect(&["symbolic-ref", "--quiet", "HEAD"])?;
    let (resolved, object) = collect(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
    let object = if resolved == 0 && !object.is_empty() {
        Some(String::from_utf8(object).map_err(|_| IdentityGap::Git)?)
    } else if resolved == 1 && object.is_empty() {
        None
    } else {
        return Err(IdentityGap::Git);
    };
    let head = match symbolic {
        (0, branch) if branch.starts_with(b"refs/heads/") => {
            let name = String::from_utf8(branch).map_err(|_| IdentityGap::Git)?;
            if object.is_none() && collect(&["show-ref", "--verify", "--quiet", &name])?.0 != 1 {
                return Err(IdentityGap::Git);
            }
            HeadState::Branch { name, object }
        }
        (1, branch) if branch.is_empty() => HeadState::Detached {
            object: object.ok_or(IdentityGap::Git)?,
        },
        _ => return Err(IdentityGap::Git),
    };
    let mut refs = Sha256::new();
    if git_records(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname) %(objecttype) %(refname) %(symref)",
        ],
        b'\n',
        limits,
        deadline,
        |record| {
            refs.update((record.len() as u64).to_be_bytes());
            refs.update(record);
            Ok(())
        },
    )? != 0
    {
        return Err(IdentityGap::Git);
    }
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
            let (entry, path) = record.split_at(
                record
                    .iter()
                    .position(|byte| *byte == b'\t')
                    .ok_or(IdentityGap::Git)?,
            );
            if entry.starts_with(GITLINK_MODE) {
                return Err(IdentityGap::Entry {
                    path: root.join(exact_path(&path[1..])?),
                    kind: EntryKind::Submodule,
                });
            }
            add_path(
                root,
                &mut paths,
                &path[1..],
                already_listed,
                retained,
                limits,
            )
        },
    )?;
    let untracked = git_records(
        root,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        0,
        limits,
        deadline,
        |record| add_path(root, &mut paths, record, already_listed, retained, limits),
    )?;
    if staged != 0 || untracked != 0 {
        return Err(IdentityGap::Git);
    }
    Ok((
        RepositoryState {
            head,
            index: format!("sha256:{:x}", index.finalize()),
            refs: format!("sha256:{:x}", refs.finalize()),
        },
        paths,
    ))
}

/// Adds one git path, exactly as git names it, within the entry bound.
fn add_path(
    root: &Path,
    paths: &mut BTreeSet<PathBuf>,
    name: &[u8],
    already_listed: usize,
    retained: &mut usize,
    limits: Limits,
) -> Result<(), IdentityGap> {
    let path = exact_path(name)?;
    if !paths.contains(&path) {
        if paths.len() + already_listed >= limits.entries {
            return Err(IdentityGap::Bound(Bound::Entries));
        }
        retain(&root.join(&path), retained, limits)?;
        paths.insert(path);
    }
    Ok(())
}

/// The path git names with these bytes, without lossy identity conversion.
fn exact_path(name: &[u8]) -> Result<PathBuf, IdentityGap> {
    if name.is_empty() {
        return Err(IdentityGap::Entry {
            path: PathBuf::new(),
            kind: EntryKind::NotContained,
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(name.to_vec())))
    }
    #[cfg(not(unix))]
    {
        let exact = std::str::from_utf8(name).map_err(|_| IdentityGap::Entry {
            path: PathBuf::from(String::from_utf8_lossy(name).into_owned()),
            kind: EntryKind::InexactName,
        })?;
        Ok(PathBuf::from(OsString::from(exact)))
    }
}

/// Streams the `terminator`-separated records of a read-only git query in `root` to
/// `on_record`, within the output-byte bound and the deadline, and returns its exit code.
/// A gap means git cannot run, the callback refuses a record, or a bound is
/// exceeded (the process is then killed).
/// Repository configuration cannot start helper programs: the file-system monitor is disabled
/// and optional locks are off.
fn git_records(
    root: &Path,
    arguments: &[&str],
    terminator: u8,
    limits: Limits,
    deadline: Instant,
    on_record: impl FnMut(&[u8]) -> Result<(), IdentityGap>,
) -> Result<i32, IdentityGap> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .args(arguments)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0");
    records(command, terminator, limits.git_output, deadline, on_record)
}

#[cfg(test)]
#[path = "acceptance_workspace_tests.rs"]
mod tests;
