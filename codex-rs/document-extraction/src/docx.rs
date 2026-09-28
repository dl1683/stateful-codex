use crate::ExtractedDocument;
use crate::ExtractionError;
use crate::ExtractionLimits;

pub(crate) fn extract(
    _bytes: &[u8],
    _limits: &ExtractionLimits,
    _original_fingerprint: String,
) -> Result<ExtractedDocument, ExtractionError> {
    Err(ExtractionError::UnsupportedFormat)
}
