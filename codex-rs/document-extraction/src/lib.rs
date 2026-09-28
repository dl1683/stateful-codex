//! Bounded, deterministic extraction of Office document representations.

mod archive;
mod cache;
mod canonical;
mod docx;
mod limits;

pub use limits::ExtractionLimit;
pub use limits::ExtractionLimits;

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// An Office format supported by the extraction boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DocumentFormat {
    Docx,
    Xlsx,
}

/// A deterministic warning attached to an extracted document or block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExtractionNotice {
    TrackedChanges,
    UnsupportedPart { part: String },
    LimitReached { limit: ExtractionLimit },
}

/// The identity of the extractor and representation rules that produced a result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtractorIdentity {
    pub name: String,
    pub version: String,
}

impl ExtractorIdentity {
    pub(crate) fn for_format(format: DocumentFormat) -> Self {
        match format {
            DocumentFormat::Docx => Self {
                name: "codex-docx".to_owned(),
                version: "2".to_owned(),
            },
            DocumentFormat::Xlsx => Self {
                name: "codex-xlsx".to_owned(),
                version: "1".to_owned(),
            },
        }
    }
}

/// A bounded structural location within an extracted representation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtractionAnchor {
    pub scheme: String,
    pub locator: String,
}

/// One bounded canonical block and its local extraction warnings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtractedBlock {
    pub anchor: ExtractionAnchor,
    pub text: String,
    pub notices: Vec<ExtractionNotice>,
}

/// Whether extraction covered the supported representation completely.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExtractionStatus {
    Complete,
    Partial,
}

/// A complete deterministic observation of one original Office file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtractedDocument {
    pub original_fingerprint: String,
    pub original_bytes: u64,
    pub extractor: ExtractorIdentity,
    pub canonical_representation_digest: String,
    pub status: ExtractionStatus,
    pub notices: Vec<ExtractionNotice>,
    pub blocks: Vec<ExtractedBlock>,
}

/// Errors that prevent a safe extraction result from being returned.
#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("unsupported document format")]
    UnsupportedFormat,
    #[error("encrypted document")]
    Encrypted,
    #[error("corrupt document")]
    Corrupt,
    #[error("extraction limit exceeded: {0:?}")]
    LimitExceeded(ExtractionLimit),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The extractor owns a persistent cache and immutable production limits.
#[derive(Clone)]
pub struct DocumentExtractor {
    cache: cache::ExtractionCache,
    limits: ExtractionLimits,
}

impl DocumentExtractor {
    /// Creates an extractor using the production safety limits.
    pub fn production(cache_root: PathBuf) -> Result<Self, ExtractionError> {
        Self::with_limits(cache_root, ExtractionLimits::default())
    }

    /// Creates an extractor with explicit limits, primarily for bounded tests.
    pub fn with_limits(
        cache_root: PathBuf,
        limits: ExtractionLimits,
    ) -> Result<Self, ExtractionError> {
        std::fs::create_dir_all(&cache_root)?;
        Ok(Self {
            cache: cache::ExtractionCache::new(cache_root),
            limits,
        })
    }

    /// Returns the Office format identified by a path extension.
    pub fn format_for_path(path: &Path) -> Option<DocumentFormat> {
        let extension = path.extension()?.to_str()?;
        match extension {
            value if value.eq_ignore_ascii_case("docx") => Some(DocumentFormat::Docx),
            value if value.eq_ignore_ascii_case("xlsx") => Some(DocumentFormat::Xlsx),
            _ => None,
        }
    }

    /// Hashes, extracts, and cache-validates an original Office byte sequence.
    pub fn extract(
        &self,
        format: DocumentFormat,
        original_bytes: &[u8],
    ) -> Result<ExtractedDocument, ExtractionError> {
        if original_bytes.len() as u64 > self.limits.max_original_bytes {
            return Err(ExtractionError::LimitExceeded(
                ExtractionLimit::OriginalBytes,
            ));
        }

        let original_fingerprint = canonical::digest_bytes(original_bytes);
        let identity = ExtractorIdentity::for_format(format);
        if let Some(document) = self
            .cache
            .load(&original_fingerprint, &identity, &self.limits)
            .filter(|document| {
                document.original_fingerprint == original_fingerprint
                    && document.original_bytes == original_bytes.len() as u64
                    && document.extractor == identity
                    && canonical::representation_digest(document)
                        == document.canonical_representation_digest
            })
        {
            return Ok(document);
        }

        let mut document = match format {
            DocumentFormat::Docx => docx::extract(
                original_bytes,
                &self.limits,
                original_fingerprint,
                original_bytes.len() as u64,
            )?,
            DocumentFormat::Xlsx => return Err(ExtractionError::UnsupportedFormat),
        };
        document.canonical_representation_digest = canonical::representation_digest(&document);
        self.cache.store(&document, &self.limits);
        Ok(document)
    }
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod archive_tests;

#[cfg(test)]
#[path = "docx_tests.rs"]
mod docx_tests;
