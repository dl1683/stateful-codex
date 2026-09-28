use pretty_assertions::assert_eq;

use super::*;

#[test]
fn indexed_extraction_round_trips_as_one_typed_object() {
    let extraction = IndexedExtraction::new("codex-docx", "2", "sha256:representation")
        .expect("valid extraction identity");
    let encoded = serde_json::to_string(&extraction).expect("extraction should serialize");
    assert_eq!(
        serde_json::from_str::<IndexedExtraction>(&encoded).expect("extraction should parse"),
        extraction
    );
}

#[test]
fn indexed_extraction_rejects_partial_fields() {
    assert!(IndexedExtraction::new("", "2", "sha256:digest").is_err());
    assert!(IndexedExtraction::new("codex-docx", "", "sha256:digest").is_err());
    assert!(IndexedExtraction::new("codex-docx", "2", "").is_err());
}
