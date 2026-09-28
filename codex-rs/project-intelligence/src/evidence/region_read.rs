use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use codex_document_extraction::DocumentExtractor;
use sha2::Digest;
use sha2::Sha256;

use crate::EvidenceReadError;
use crate::IndexedExtraction;
use crate::RegionAnchor;

const MAX_OFFICE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct RegionRead {
    pub(super) content: String,
    pub(super) total_bytes: u64,
    pub(super) truncated: bool,
    pub(super) extraction: IndexedExtraction,
}

pub(super) fn read_region(
    path: PathBuf,
    expected_fingerprint: &str,
    anchor: &RegionAnchor,
    indexed_extraction: &IndexedExtraction,
    max_bytes: usize,
) -> Result<RegionRead, EvidenceReadError> {
    // The path can be replaced after open; hashing and extracting this same buffer makes the read
    // linearizable at open without adding a platform-specific file-identity mechanism.
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
