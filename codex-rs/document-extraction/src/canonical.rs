use sha2::Digest;
use sha2::Sha256;

use crate::ExtractedDocument;
use crate::ExtractionLimits;
use crate::ExtractionNotice;
use crate::ExtractionStatus;

pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(crate) fn limits_digest(limits: &ExtractionLimits) -> String {
    let bytes = match serde_json::to_vec(limits) {
        Ok(bytes) => bytes,
        Err(error) => unreachable!("extraction limits are serializable: {error}"),
    };
    digest_bytes(&bytes)
}

pub(crate) fn representation_digest(document: &ExtractedDocument) -> String {
    let mut canonical = Vec::new();
    push(&mut canonical, &document.extractor.name);
    push(&mut canonical, &document.extractor.version);
    canonical.push(match document.status {
        ExtractionStatus::Complete => 0,
        ExtractionStatus::Partial => 1,
    });
    push_notices(&mut canonical, &document.notices);
    for block in &document.blocks {
        push(&mut canonical, &block.anchor.scheme);
        push(&mut canonical, &block.anchor.locator);
        push(&mut canonical, &block.text);
        push_notices(&mut canonical, &block.notices);
    }
    digest_bytes(&canonical)
}

fn push(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn push_notices(output: &mut Vec<u8>, notices: &[ExtractionNotice]) {
    for notice in notices {
        match notice {
            ExtractionNotice::TrackedChanges => push(output, "tracked-changes"),
            ExtractionNotice::UnsupportedPart { part } => {
                push(output, "unsupported-part");
                push(output, part);
            }
            ExtractionNotice::LimitReached { limit } => {
                push(output, "limit-reached");
                push(output, &format!("{limit:?}"));
            }
        }
    }
    output.extend_from_slice(&0u64.to_le_bytes());
}
