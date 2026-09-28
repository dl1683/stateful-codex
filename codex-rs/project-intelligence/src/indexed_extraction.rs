use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

const MAX_FIELD_BYTES: usize = 512;
const MAX_DIGEST_BYTES: usize = 512;

/// Identity of the extractor and canonical representation used for an indexed region.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedExtraction {
    pub extractor_name: String,
    pub extractor_version: String,
    pub canonical_representation_digest: String,
}

impl IndexedExtraction {
    pub fn new(
        extractor_name: impl Into<String>,
        extractor_version: impl Into<String>,
        canonical_representation_digest: impl Into<String>,
    ) -> Result<Self, IndexedExtractionError> {
        let extraction = Self {
            extractor_name: extractor_name.into(),
            extractor_version: extractor_version.into(),
            canonical_representation_digest: canonical_representation_digest.into(),
        };
        extraction.validate()?;
        Ok(extraction)
    }

    fn validate(&self) -> Result<(), IndexedExtractionError> {
        validate_field("extractor name", &self.extractor_name, MAX_FIELD_BYTES)?;
        validate_field(
            "extractor version",
            &self.extractor_version,
            MAX_FIELD_BYTES,
        )?;
        validate_field(
            "canonical representation digest",
            &self.canonical_representation_digest,
            MAX_DIGEST_BYTES,
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IndexedExtractionError {
    #[error("{0} must be non-empty, bounded, and contain no controls")]
    InvalidField(&'static str),
}

fn validate_field(
    label: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), IndexedExtractionError> {
    if value.is_empty() || value.len() > maximum_bytes || value.chars().any(char::is_control) {
        return Err(IndexedExtractionError::InvalidField(label));
    }
    Ok(())
}

#[cfg(test)]
#[path = "indexed_extraction_tests.rs"]
mod tests;
