use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use codex_document_extraction::DocumentExtractor;
use codex_document_extraction::ExtractionError;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

use crate::ContextMapEntryId;
use crate::ContextMapFreshness;
use crate::ContextMapHit;
use crate::ContextMapStore;
use crate::ContextMapStoreError;
use crate::IndexedExtraction;
use crate::ProjectRelativePath;
use crate::SourceFingerprint;

pub const MAX_EVIDENCE_READ_BYTES: u32 = 64 * 1024;
const MAX_LINE_SPAN: u64 = 2_000;
const MAX_OFFICE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLineRange {
    pub start: u64,
    pub end: u64,
}

impl EvidenceLineRange {
    pub(crate) fn is_valid(self) -> bool {
        self.start > 0
            && self.end >= self.start
            && self.end <= i64::MAX as u64
            && self.end.saturating_sub(self.start) < MAX_LINE_SPAN
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceReadRequest {
    pub project_id: String,
    pub project_roots: Vec<PathBuf>,
    pub locator: EvidenceReadLocator,
    pub max_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceReadLocator {
    Source {
        project_root: Option<PathBuf>,
        relative_path: ProjectRelativePath,
        line_range: Option<EvidenceLineRange>,
    },
    ContextMapRoute(EvidenceRoute),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRoute {
    pub context_map_entry_id: ContextMapEntryId,
    pub source_fingerprint: SourceFingerprint,
    pub line_range: Option<EvidenceLineRange>,
    pub region_anchor: Option<crate::RegionAnchor>,
    pub indexed_extraction: Option<IndexedExtraction>,
}

impl EvidenceRoute {
    pub fn from_hit(hit: &ContextMapHit) -> Result<Self, EvidenceReadError> {
        let line_range = hit
            .source
            .region_anchor
            .as_ref()
            .filter(|anchor| anchor.scheme == "lines")
            .map(parse_line_anchor)
            .transpose()?;
        Ok(Self {
            context_map_entry_id: hit.entry.id.clone(),
            source_fingerprint: hit.entry.value.source_fingerprint.clone(),
            line_range,
            region_anchor: hit
                .source
                .region_anchor
                .clone()
                .filter(|anchor| anchor.scheme != "lines"),
            indexed_extraction: hit.source.indexed_extraction.clone().filter(|_| {
                hit.source
                    .region_anchor
                    .as_ref()
                    .is_some_and(|anchor| anchor.scheme != "lines")
            }),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceReadResult {
    pub hit: ContextMapHit,
    pub content: String,
    pub bytes_returned: u64,
    pub total_bytes: u64,
    pub total_lines: u64,
    pub first_line: Option<u64>,
    pub last_line: Option<u64>,
    pub truncated: bool,
    pub extraction: Option<IndexedExtraction>,
}

#[derive(Clone)]
pub struct EvidenceReader {
    context_map: ContextMapStore,
}

impl EvidenceReader {
    pub fn new(context_map: ContextMapStore) -> Self {
        Self { context_map }
    }

    pub async fn read(
        &self,
        request: EvidenceReadRequest,
    ) -> Result<EvidenceReadResult, EvidenceReadError> {
        validate_request(&request)?;
        let allowed_roots = request
            .project_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>();
        let (
            mut hits,
            requested_root,
            requested_path,
            line_range,
            region_anchor,
            indexed_extraction,
        ) = match &request.locator {
            EvidenceReadLocator::Source {
                project_root,
                relative_path,
                line_range,
            } => {
                let requested_root = project_root.as_ref().map(|root| root.display().to_string());
                let hits = self
                    .context_map
                    .file_hits_for_path(&request.project_id, relative_path)
                    .await?;
                (
                    hits,
                    requested_root,
                    relative_path.to_string(),
                    *line_range,
                    None,
                    None,
                )
            }
            EvidenceReadLocator::ContextMapRoute(route) => {
                let hit = match self
                    .context_map
                    .get_guarded_hit(
                        &request.project_id,
                        &route.context_map_entry_id,
                        &route.source_fingerprint,
                    )
                    .await
                {
                    Ok(hit) => hit,
                    Err(ContextMapStoreError::SourceNotCurrent(_)) => {
                        return Err(EvidenceReadError::RouteChanged);
                    }
                    Err(error) => return Err(error.into()),
                }
                .ok_or_else(|| {
                    EvidenceReadError::RouteNotFound(route.context_map_entry_id.to_string())
                })?;
                if EvidenceRoute::from_hit(&hit)? != *route {
                    return Err(EvidenceReadError::RouteChanged);
                }
                let requested_path = hit.source.relative_path.to_string();
                (
                    vec![hit],
                    None,
                    requested_path,
                    route.line_range,
                    route.region_anchor.clone(),
                    route.indexed_extraction.clone(),
                )
            }
        };
        if requested_root
            .as_ref()
            .is_some_and(|root| !allowed_roots.contains(root))
        {
            return Err(EvidenceReadError::RootOutsideProject);
        }
        hits = hits
            .into_iter()
            .filter(|hit| allowed_roots.contains(&hit.source.project_root))
            .filter(|hit| {
                requested_root
                    .as_ref()
                    .is_none_or(|root| root == &hit.source.project_root)
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            return Err(EvidenceReadError::SourceNotIndexed(requested_path));
        }
        if hits.len() > 1 {
            return Err(EvidenceReadError::AmbiguousSource(requested_path));
        }
        let hit = hits
            .pop()
            .ok_or_else(|| EvidenceReadError::SourceNotIndexed(requested_path))?;
        if hit.freshness != ContextMapFreshness::Current {
            return Err(EvidenceReadError::SourceNotCurrent(hit.freshness));
        }

        let root = tokio::fs::canonicalize(&hit.source.project_root).await?;
        let source = tokio::fs::canonicalize(root.join(hit.source.relative_path.as_str())).await?;
        if !source.starts_with(&root) {
            return Err(EvidenceReadError::SourceOutsideRoot);
        }
        let max_bytes =
            usize::try_from(request.max_bytes).map_err(|_| EvidenceReadError::InvalidRequest)?;
        if let Some(anchor) = region_anchor {
            let indexed_extraction =
                indexed_extraction.ok_or(EvidenceReadError::InvalidIndexedExtraction)?;
            let expected_fingerprint = hit.entry.value.source_fingerprint.to_string();
            let read = tokio::task::spawn_blocking(move || {
                read_region(
                    source,
                    &expected_fingerprint,
                    &anchor,
                    &indexed_extraction,
                    max_bytes,
                )
            })
            .await??;
            let bytes_returned =
                u64::try_from(read.content.len()).map_err(|_| EvidenceReadError::CountOverflow)?;
            return Ok(EvidenceReadResult {
                hit,
                content: read.content,
                bytes_returned,
                total_bytes: read.total_bytes,
                total_lines: 0,
                first_line: None,
                last_line: None,
                truncated: read.truncated,
                extraction: Some(read.extraction),
            });
        }
        let read = tokio::task::spawn_blocking(move || read_source(source, line_range, max_bytes))
            .await??;
        if read.fingerprint != hit.entry.value.source_fingerprint.as_str() {
            return Err(EvidenceReadError::SourceChanged);
        }
        let bytes_returned =
            u64::try_from(read.content.len()).map_err(|_| EvidenceReadError::CountOverflow)?;
        Ok(EvidenceReadResult {
            hit,
            content: read.content,
            bytes_returned,
            total_bytes: read.total_bytes,
            total_lines: read.total_lines,
            first_line: read.first_line,
            last_line: read.last_line,
            truncated: read.truncated,
            extraction: None,
        })
    }
}

fn validate_request(request: &EvidenceReadRequest) -> Result<(), EvidenceReadError> {
    if request.project_id.is_empty()
        || request.project_roots.is_empty()
        || request.max_bytes == 0
        || request.max_bytes > MAX_EVIDENCE_READ_BYTES
    {
        return Err(EvidenceReadError::InvalidRequest);
    }
    let line_range = match &request.locator {
        EvidenceReadLocator::Source { line_range, .. } => *line_range,
        EvidenceReadLocator::ContextMapRoute(route) => route.line_range,
    };
    if line_range.is_some_and(|range| !range.is_valid()) {
        return Err(EvidenceReadError::InvalidLineRange);
    }
    Ok(())
}

fn parse_line_anchor(anchor: &crate::RegionAnchor) -> Result<EvidenceLineRange, EvidenceReadError> {
    if anchor.scheme != "lines" {
        return Err(EvidenceReadError::UnsupportedRegionAnchor(
            anchor.scheme.clone(),
        ));
    }
    let (start, end) = anchor
        .locator
        .split_once('-')
        .ok_or(EvidenceReadError::InvalidRegionAnchor)?;
    let range = EvidenceLineRange {
        start: start
            .parse()
            .map_err(|_| EvidenceReadError::InvalidRegionAnchor)?,
        end: end
            .parse()
            .map_err(|_| EvidenceReadError::InvalidRegionAnchor)?,
    };
    range
        .is_valid()
        .then_some(range)
        .ok_or(EvidenceReadError::InvalidRegionAnchor)
}

struct SourceRead {
    content: String,
    total_bytes: u64,
    total_lines: u64,
    first_line: Option<u64>,
    last_line: Option<u64>,
    truncated: bool,
    fingerprint: String,
}

struct RegionRead {
    content: String,
    total_bytes: u64,
    truncated: bool,
    extraction: IndexedExtraction,
}

fn read_region(
    path: PathBuf,
    expected_fingerprint: &str,
    anchor: &crate::RegionAnchor,
    indexed_extraction: &IndexedExtraction,
    max_bytes: usize,
) -> Result<RegionRead, EvidenceReadError> {
    let mut file = File::open(&path)?;
    let mut limited = file.by_ref().take(MAX_OFFICE_BYTES.saturating_add(1));
    let mut original = Vec::new();
    limited.read_to_end(&mut original)?;
    if original.len() as u64 > MAX_OFFICE_BYTES {
        return Err(EvidenceReadError::SourceTooLarge);
    }
    let fingerprint = format!("sha256:{:x}", Sha256::digest(&original));
    if fingerprint != expected_fingerprint {
        return Err(EvidenceReadError::SourceChanged);
    }
    let format = DocumentExtractor::format_for_path(&path)
        .ok_or_else(|| EvidenceReadError::UnsupportedRegionAnchor(anchor.scheme.clone()))?;
    let document = DocumentExtractor::production().extract(format, &original)?;
    let current_extraction = IndexedExtraction::new(
        document.extractor.name.clone(),
        document.extractor.version.clone(),
        document.canonical_representation_digest.clone(),
    )
    .map_err(|_| EvidenceReadError::InvalidIndexedExtraction)?;
    if &current_extraction != indexed_extraction {
        return Err(EvidenceReadError::ExtractionChanged);
    }
    let block = document
        .blocks
        .into_iter()
        .find(|block| {
            block.anchor.scheme == anchor.scheme && block.anchor.locator == anchor.locator
        })
        .ok_or(EvidenceReadError::RegionAnchorNotFound)?;
    let total_bytes =
        u64::try_from(block.text.len()).map_err(|_| EvidenceReadError::CountOverflow)?;
    let end = block
        .text
        .floor_char_boundary(max_bytes.min(block.text.len()));
    Ok(RegionRead {
        content: block.text[..end].to_owned(),
        total_bytes,
        truncated: end < block.text.len(),
        extraction: current_extraction,
    })
}

fn read_source(
    path: PathBuf,
    line_range: Option<EvidenceLineRange>,
    max_bytes: usize,
) -> Result<SourceRead, EvidenceReadError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut selected = Vec::with_capacity(max_bytes);
    let mut total_bytes = 0_u64;
    let mut current_line = 1_u64;
    let mut saw_bytes = false;
    let mut ended_with_newline = false;
    let mut first_line = None;
    let mut last_line = None;
    let mut truncated = false;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let bytes = &buffer[..read];
        hasher.update(bytes);
        total_bytes = total_bytes
            .checked_add(u64::try_from(read).map_err(|_| EvidenceReadError::CountOverflow)?)
            .ok_or(EvidenceReadError::CountOverflow)?;
        for byte in bytes {
            saw_bytes = true;
            let selected_line = line_range
                .is_none_or(|range| current_line >= range.start && current_line <= range.end);
            if selected_line {
                if selected.len() < max_bytes {
                    selected.push(*byte);
                    first_line.get_or_insert(current_line);
                    last_line = Some(current_line);
                } else {
                    truncated = true;
                }
            }
            ended_with_newline = *byte == b'\n';
            if ended_with_newline {
                current_line = current_line
                    .checked_add(1)
                    .ok_or(EvidenceReadError::CountOverflow)?;
            }
        }
    }
    let total_lines = if !saw_bytes {
        0
    } else if ended_with_newline {
        current_line.saturating_sub(1)
    } else {
        current_line
    };
    let content = match String::from_utf8(selected) {
        Ok(content) => content,
        Err(error) if error.utf8_error().error_len().is_none() => {
            truncated = true;
            let valid_up_to = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..valid_up_to].to_vec())
                .map_err(|_| EvidenceReadError::NonUtf8Source)?
        }
        Err(_) => return Err(EvidenceReadError::NonUtf8Source),
    };
    Ok(SourceRead {
        content,
        total_bytes,
        total_lines,
        first_line,
        last_line,
        truncated,
        fingerprint: format!("sha256:{:x}", hasher.finalize()),
    })
}

#[derive(Debug, Error)]
pub enum EvidenceReadError {
    #[error("evidence read requires a project, roots, and a 1-65536 byte bound")]
    InvalidRequest,
    #[error("line range must be 1-based, ordered, and span no more than 2000 lines")]
    InvalidLineRange,
    #[error("requested project root is outside the selected project")]
    RootOutsideProject,
    #[error("source is not indexed in the selected project: {0}")]
    SourceNotIndexed(String),
    #[error("context-map evidence route was not found: {0}")]
    RouteNotFound(String),
    #[error("context-map evidence route changed; query the context map again")]
    RouteChanged,
    #[error("context-map region anchor is malformed")]
    InvalidRegionAnchor,
    #[error("context-map region anchor is not supported for evidence reads: {0}")]
    UnsupportedRegionAnchor(String),
    #[error("context-map region anchor was not found in the current extraction")]
    RegionAnchorNotFound,
    #[error("indexed extraction provenance is missing or malformed")]
    InvalidIndexedExtraction,
    #[error("source path exists in multiple project roots; provide projectRoot: {0}")]
    AmbiguousSource(String),
    #[error("source route is not current: {0:?}")]
    SourceNotCurrent(ContextMapFreshness),
    #[error("source resolves outside its configured project root")]
    SourceOutsideRoot,
    #[error("source changed after indexing; refresh the context map before reading")]
    SourceChanged,
    #[error("source exceeds the bounded Office read limit")]
    SourceTooLarge,
    #[error("current extraction differs from the indexed representation")]
    ExtractionChanged,
    #[error("Office extraction failed: {0}")]
    Extraction(#[from] ExtractionError),
    #[error("source region is not valid UTF-8 text")]
    NonUtf8Source,
    #[error("evidence byte or line count overflow")]
    CountOverflow,
    #[error(transparent)]
    ContextMap(#[from] ContextMapStoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("evidence read task failed: {0}")]
    ReadTask(#[from] tokio::task::JoinError),
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
