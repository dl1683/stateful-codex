//! Bounded observation, at a compaction boundary, of files the thread patched.
//!
//! Only paths inside the selected project's roots are read, on the host, with a byte limit on
//! the opened handle and a time budget for all reads together. Anything else is reported as not
//! observed; this never claims to have seen a remote executor's file.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncReadExt;

/// Files larger than this are named but not hashed or read.
const MAX_OBSERVED_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Total time all boundary reads may take, and bytes they may read.
const READ_BUDGET: Duration = Duration::from_secs(2);
const MAX_TOTAL_READ_BYTES: u64 = 16 * 1024 * 1024;

/// What was found at a path at the boundary.
pub(crate) enum Observation {
    /// The bytes read (bounded), and their length.
    Read(Vec<u8>),
    /// A short reason the file was not read, said as it is.
    NotRead(&'static str),
}

/// Reads boundary observations within one shared budget.
pub(crate) struct FileObserver {
    roots: Vec<PathBuf>,
    started: Instant,
    bytes_read: u64,
}

impl FileObserver {
    pub(crate) fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            started: Instant::now(),
            bytes_read: 0,
        }
    }

    pub(crate) async fn observe(&mut self, path: &str) -> Observation {
        const OUTSIDE: &str = "not observed (outside the project roots on this host)";
        let path = Path::new(path);
        if !path.is_absolute() || !self.roots.iter().any(|root| path.starts_with(root)) {
            return Observation::NotRead(OUTSIDE);
        }
        // Links and `..` are resolved first: the file actually read must lie inside a root.
        let real = match tokio::fs::canonicalize(path).await {
            Ok(real) => real,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Observation::NotRead("missing at the boundary");
            }
            Err(_) => return Observation::NotRead("unreadable at the boundary"),
        };
        let mut inside = false;
        for root in &self.roots {
            if let Ok(root) = tokio::fs::canonicalize(root).await
                && real.starts_with(&root)
            {
                inside = true;
                break;
            }
        }
        if !inside {
            return Observation::NotRead(OUTSIDE);
        }
        let path = real.as_path();
        let Some(remaining) = READ_BUDGET.checked_sub(self.started.elapsed()) else {
            return Observation::NotRead("not observed (boundary read budget spent)");
        };
        let allowance = MAX_OBSERVED_FILE_BYTES.min(MAX_TOTAL_READ_BYTES - self.bytes_read);
        match tokio::time::timeout(remaining, read_bounded(path, allowance)).await {
            Ok(Ok(Some(bytes))) => {
                self.bytes_read += bytes.len() as u64;
                Observation::Read(bytes)
            }
            Ok(Ok(None)) => Observation::NotRead("too large to read at the boundary"),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Observation::NotRead("missing at the boundary")
            }
            Ok(Err(_)) => Observation::NotRead("unreadable at the boundary"),
            Err(_) => Observation::NotRead("not observed (read timed out)"),
        }
    }
}

/// The file's bytes, read from one opened handle with at most `limit` bytes; `None` when it
/// holds more.
async fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
    let file = tokio::fs::File::open(path).await?;
    if !file.metadata().await?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).await?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

/// A short fingerprint of observed bytes.
pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))[..16].to_string()
}

#[cfg(test)]
#[path = "capsule_files_tests.rs"]
mod tests;
