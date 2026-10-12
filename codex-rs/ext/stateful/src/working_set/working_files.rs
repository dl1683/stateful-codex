//! N6 changed-content witness for at most eight task-touched text files, including untracked
//! ones. Equal size, mtime or Git state proves nothing: only complete sampled bytes compared
//! against the last observed content do. The caller reads bytes through the selected
//! executor filesystem; nothing here interprets an executor path as a host path.

use sha2::Digest;
use sha2::Sha256;

const MAX_WORKING_SET_FILE_BYTES: usize = 64 * 1024;
/// Total sampled bytes per collection; the reader also applies a two-second deadline.
pub(crate) const MAX_TOTAL_SAMPLE_BYTES: usize = 512 * 1024;
const MAX_HUNK_LINES: usize = 16;
const MAX_HUNK_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileSampleCoverage {
    Complete,
    Truncated,
    Binary,
    Oversized,
    Unreadable,
    OutOfRoot,
}

/// Bounded content sample of one task-touched file read through its executor filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkingSetFileSample {
    pub executor_id: String,
    pub root: String,
    pub relative_path: String,
    pub sample_event: String,
    pub coverage: FileSampleCoverage,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileWitness {
    pub path: String,
    pub before_sha: String,
    pub after_sha: String,
    pub before_event: String,
    pub after_event: String,
    /// 1-based first changed line in the earlier content.
    pub first_line: usize,
    pub old: String,
    pub new: String,
    /// The hunk exceeded 16 lines / 1 KiB and is shown as a partial hunk.
    pub partial: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WitnessOutcome {
    Unchanged,
    Changed(FileWitness),
    /// No earlier sample: current content only; before-state and origin unknown.
    NoBaseline,
    /// A truncated, binary, oversized, unreadable or escaping sample certifies nothing.
    Unknown(FileSampleCoverage),
}

impl FileWitness {
    pub(crate) fn summary(&self) -> String {
        let partial = if self.partial { " (partial hunk)" } else { "" };
        format!(
            "L{}: {} -> {} [sha {}->{}]{partial}",
            self.first_line,
            self.old,
            self.new,
            &self.before_sha[..8],
            &self.after_sha[..8]
        )
    }
}

/// Classifies one read. `remaining` is the collection's shared byte budget.
pub(crate) fn sample_file(
    executor_id: &str,
    root: &str,
    relative_path: &str,
    sample_event: &str,
    read: std::io::Result<Vec<u8>>,
    remaining: &mut usize,
) -> WorkingSetFileSample {
    let (coverage, content) = match read {
        _ if !inside_root(relative_path) => (FileSampleCoverage::OutOfRoot, None),
        Err(_) => (FileSampleCoverage::Unreadable, None),
        Ok(bytes) if bytes.len() > MAX_WORKING_SET_FILE_BYTES || bytes.len() > *remaining => {
            (FileSampleCoverage::Oversized, None)
        }
        Ok(bytes) => {
            *remaining -= bytes.len();
            match String::from_utf8(bytes) {
                Ok(text) if !text.contains('\0') => (FileSampleCoverage::Complete, Some(text)),
                _ => (FileSampleCoverage::Binary, None),
            }
        }
    };
    WorkingSetFileSample {
        executor_id: executor_id.to_string(),
        root: root.to_string(),
        relative_path: relative_path.to_string(),
        sample_event: sample_event.to_string(),
        coverage,
        content,
    }
}

/// Compares a fresh sample with the last observed content of the same file.
pub(crate) fn compare_samples(
    before: Option<&WorkingSetFileSample>,
    after: &WorkingSetFileSample,
) -> WitnessOutcome {
    let (FileSampleCoverage::Complete, Some(new)) = (after.coverage, after.content.as_deref())
    else {
        return WitnessOutcome::Unknown(after.coverage);
    };
    let Some(before) = before else {
        return WitnessOutcome::NoBaseline;
    };
    let (FileSampleCoverage::Complete, Some(old)) = (before.coverage, before.content.as_deref())
    else {
        return WitnessOutcome::Unknown(before.coverage);
    };
    let (before_sha, after_sha) = (sha(old), sha(new));
    if before_sha == after_sha {
        return WitnessOutcome::Unchanged;
    }
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let room = old_lines.len().min(new_lines.len()) - prefix;
    let suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let mut partial = false;
    let mut hunk = |lines: &[&str]| {
        let mut kept: Vec<&str> = Vec::new();
        for line in lines {
            if kept.len() == MAX_HUNK_LINES
                || kept.iter().map(|l| l.len() + 1).sum::<usize>() + line.len() > MAX_HUNK_BYTES / 2
            {
                partial = true;
                break;
            }
            kept.push(line);
        }
        kept.join("\n")
    };
    let old_hunk = hunk(&old_lines[prefix..old_lines.len() - suffix]);
    let new_hunk = hunk(&new_lines[prefix..new_lines.len() - suffix]);
    WitnessOutcome::Changed(FileWitness {
        path: after.relative_path.clone(),
        before_sha,
        after_sha,
        before_event: before.sample_event.clone(),
        after_event: after.sample_event.clone(),
        first_line: prefix + 1,
        old: old_hunk,
        new: new_hunk,
        partial,
    })
}

/// A relative path with no parent, root or drive component.
fn inside_root(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains(':')
        && path.split(['/', '\\']).all(|part| part != "..")
}

fn sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
