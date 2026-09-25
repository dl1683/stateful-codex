use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use ignore::WalkBuilder;
use sha2::Digest;
use sha2::Sha256;

use crate::ContextMapCoverage;
use crate::ProjectRelativePath;
use crate::SourceFingerprint;

use super::ProjectIndexerError;
use super::hex_digest;

const MAX_FILES: usize = 20_000;
const EXCERPT_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_BYTES: usize = 2_048;
const MAX_ROUTING_TERMS: usize = 32;
const MAX_ROUTING_TERM_BYTES: usize = 128;

pub(super) struct ScanResult {
    pub(super) files: Vec<ScannedFile>,
    pub(super) files_skipped: u64,
    pub(super) truncated: bool,
}

pub(super) struct ScannedFile {
    pub(super) project_root: String,
    pub(super) relative_path: String,
    pub(super) fingerprint: SourceFingerprint,
    pub(super) description: String,
    pub(super) routing_terms: Vec<String>,
    pub(super) coverage: ContextMapCoverage,
}

pub(super) fn scan_roots(roots: &[PathBuf]) -> Result<ScanResult, ProjectIndexerError> {
    let mut files = Vec::new();
    let mut files_skipped = 0_u64;
    let mut truncated = false;
    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(false)
            .follow_links(false)
            .standard_filters(true);
        builder.filter_entry(|entry| {
            !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "node_modules" | "target")
                )
        });
        for entry in builder.build() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    files_skipped = files_skipped.saturating_add(1);
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if files.len() == MAX_FILES {
                truncated = true;
                break;
            }
            match scan_file(root, entry.path()) {
                Ok(file) => files.push(file),
                Err(_) => files_skipped = files_skipped.saturating_add(1),
            }
        }
        if truncated {
            break;
        }
    }
    Ok(ScanResult {
        files,
        files_skipped,
        truncated,
    })
}

pub(super) fn scan_project_file(
    root: &Path,
    relative_path: &ProjectRelativePath,
) -> Result<ScannedFile, ProjectIndexerError> {
    let canonical_root = std::fs::canonicalize(root)?;
    let path = root.join(relative_path.as_str());
    let canonical_path = std::fs::canonicalize(&path)?;
    if !canonical_path.starts_with(&canonical_root)
        || !std::fs::metadata(&canonical_path)?.is_file()
    {
        return Err(ProjectIndexerError::InvalidRoot);
    }
    scan_file(root, &path)
}

fn scan_file(root: &Path, path: &Path) -> Result<ScannedFile, ProjectIndexerError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ProjectIndexerError::InvalidRoot)?;
    let relative_path = normalized_relative_path(relative)?;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut excerpt = Vec::with_capacity(EXCERPT_BYTES);
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .saturating_add(u64::try_from(read).map_err(|_| ProjectIndexerError::CountOverflow)?);
        hasher.update(&buffer[..read]);
        if excerpt.len() < EXCERPT_BYTES {
            let remaining = EXCERPT_BYTES - excerpt.len();
            excerpt.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }
    let fingerprint =
        SourceFingerprint::parse(format!("sha256:{}", hex_digest(hasher.finalize())))?;
    let text = (!excerpt.contains(&0)).then(|| String::from_utf8_lossy(&excerpt).into_owned());
    let description = describe_file(&relative_path, text.as_deref());
    let routing_terms = routing_terms(&relative_path, text.as_deref());
    let coverage = if text.is_some() && total_bytes <= EXCERPT_BYTES as u64 {
        ContextMapCoverage::Complete
    } else {
        ContextMapCoverage::Partial
    };
    Ok(ScannedFile {
        project_root: root.display().to_string(),
        relative_path,
        fingerprint,
        description,
        routing_terms,
        coverage,
    })
}

fn describe_file(relative_path: &str, text: Option<&str>) -> String {
    let mut description = relative_path.to_string();
    let Some(text) = text else {
        description.push_str(" (binary file)");
        return description;
    };
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
    {
        let remaining = MAX_DESCRIPTION_BYTES.saturating_sub(description.len() + 3);
        if remaining == 0 {
            break;
        }
        description.push_str(" | ");
        description.push_str(truncate_utf8(line, remaining));
    }
    description
}

fn routing_terms(relative_path: &str, text: Option<&str>) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let sources = std::iter::once(relative_path).chain(text.into_iter().flat_map(|text| {
        text.lines()
            .map(str::trim)
            .filter(|line| line.starts_with('#'))
            .take(16)
    }));
    for token in sources.flat_map(|source| {
        source.split(|character: char| {
            !(character.is_alphanumeric() || matches!(character, '_' | '-'))
        })
    }) {
        let token = token.trim();
        if token.len() < 2 || token.len() > MAX_ROUTING_TERM_BYTES {
            continue;
        }
        let normalized = token.to_lowercase();
        if seen.insert(normalized.clone()) {
            terms.push(normalized);
            if terms.len() == MAX_ROUTING_TERMS {
                break;
            }
        }
    }
    terms
}

pub(super) fn normalized_relative_path(path: &Path) -> Result<String, ProjectIndexerError> {
    let components = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or(ProjectIndexerError::InvalidRoot)?;
    Ok(components.join("/"))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
