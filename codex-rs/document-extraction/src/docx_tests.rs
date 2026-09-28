use std::fs;
use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::tempdir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::DocumentExtractor;
use crate::DocumentFormat;
use crate::ExtractedBlock;
use crate::ExtractedDocument;
use crate::ExtractionAnchor;
use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;
use crate::ExtractionNotice;
use crate::ExtractionStatus;
use crate::ExtractorIdentity;
use sha2::Digest;
use sha2::Sha256;

fn package(document: &str, extra_parts: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut bytes));
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(document.as_bytes()).unwrap();
        for (name, contents) in extra_parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    bytes
}

fn document(body: &str) -> String {
    format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    )
}

fn extractor() -> (tempfile::TempDir, DocumentExtractor) {
    let directory = tempdir().unwrap();
    let extractor = DocumentExtractor::production(directory.path().join("cache")).unwrap();
    (directory, extractor)
}

fn one_cache_json(root: &std::path::Path) -> PathBuf {
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                return path;
            }
        }
    }
    panic!("expected a cache entry")
}

fn expected_document(
    original: &[u8],
    status: ExtractionStatus,
    notices: Vec<ExtractionNotice>,
    blocks: Vec<ExtractedBlock>,
) -> ExtractedDocument {
    let mut document = ExtractedDocument {
        original_fingerprint: format!("sha256:{:x}", Sha256::digest(original)),
        original_bytes: original.len() as u64,
        extractor: ExtractorIdentity {
            name: "codex-docx".to_owned(),
            version: "1".to_owned(),
        },
        canonical_representation_digest: String::new(),
        status,
        notices,
        blocks,
    };
    let mut canonical = Vec::new();
    push_canonical(&mut canonical, &document.extractor.name);
    push_canonical(&mut canonical, &document.extractor.version);
    canonical.push(match document.status {
        ExtractionStatus::Complete => 0,
        ExtractionStatus::Partial => 1,
    });
    push_canonical_notices(&mut canonical, &document.notices);
    for block in &document.blocks {
        push_canonical(&mut canonical, &block.anchor.scheme);
        push_canonical(&mut canonical, &block.anchor.locator);
        push_canonical(&mut canonical, &block.text);
        push_canonical_notices(&mut canonical, &block.notices);
    }
    document.canonical_representation_digest = format!("sha256:{:x}", Sha256::digest(canonical));
    document
}

fn push_canonical(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn push_canonical_notices(output: &mut Vec<u8>, notices: &[ExtractionNotice]) {
    for notice in notices {
        match notice {
            ExtractionNotice::TrackedChanges => push_canonical(output, "tracked-changes"),
            ExtractionNotice::UnsupportedPart { part } => {
                push_canonical(output, "unsupported-part");
                push_canonical(output, part);
            }
            ExtractionNotice::LimitReached { limit } => {
                push_canonical(output, "limit-reached");
                push_canonical(output, &format!("{limit:?}"));
            }
        }
    }
    output.extend_from_slice(&0u64.to_le_bytes());
}

#[test]
fn extracts_visible_text_across_runs_with_controls() {
    let (_directory, extractor) = extractor();
    let bytes = package(
        &document(
            r#"<w:p><w:r><w:t xml:space="preserve">  Café</w:t></w:r><w:r><w:tab/><w:br/><w:t>fin</w:t></w:r></w:p><w:p/>"#,
        ),
        &[],
    );

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(
        result,
        expected_document(
            &bytes,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: "docx-paragraph".to_owned(),
                    locator: "body/p[1]".to_owned(),
                },
                text: "  Café\t\nfin".to_owned(),
                notices: Vec::new(),
            }],
        )
    );
}

#[test]
fn locates_each_non_empty_table_paragraph() {
    let (_directory, extractor) = extractor();
    let bytes = package(
        &document(
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>three</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        ),
        &[],
    );

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(
        result,
        expected_document(
            &bytes,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/p[1]".to_owned(),
                    },
                    text: "one".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/p[2]".to_owned(),
                    },
                    text: "two".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[2]/p[1]".to_owned(),
                    },
                    text: "three".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );
}

#[test]
fn splits_long_paragraphs_at_utf8_boundaries() {
    let (_directory, extractor) = extractor();
    let text = format!("{}x", "é".repeat(3_072));
    let xml = document(&format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#));
    let bytes = package(&xml, &[]);

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(
        result,
        expected_document(
            &bytes,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]@bytes[0:6144]".to_owned(),
                    },
                    text: "é".repeat(3_072),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]@bytes[6144:6145]".to_owned(),
                    },
                    text: "x".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );
}

#[test]
fn marks_unsupported_parts_and_tracked_changes_partial() {
    let (_directory, extractor) = extractor();
    let bytes = package(
        &document(
            r#"<w:p><w:ins><w:r><w:t>inserted</w:t></w:r></w:ins><w:del><w:r><w:delText>removed</w:delText></w:r></w:del><w:drawing><w:txbxContent><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:txbxContent></w:drawing></w:p>"#,
        ),
        &[
            ("word/header1.xml", "header"),
            ("word/comments.xml", "comment"),
        ],
    );

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(
        result,
        expected_document(
            &bytes,
            ExtractionStatus::Partial,
            vec![
                ExtractionNotice::UnsupportedPart {
                    part: "word/header1.xml".to_owned(),
                },
                ExtractionNotice::UnsupportedPart {
                    part: "word/comments.xml".to_owned(),
                },
                ExtractionNotice::TrackedChanges,
                ExtractionNotice::UnsupportedPart {
                    part: "w:drawing".to_owned(),
                },
            ],
            vec![ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: "docx-paragraph".to_owned(),
                    locator: "body/p[1]".to_owned(),
                },
                text: "inserted".to_owned(),
                notices: vec![
                    ExtractionNotice::TrackedChanges,
                    ExtractionNotice::UnsupportedPart {
                        part: "w:drawing".to_owned(),
                    },
                ],
            }],
        )
    );
}

#[test]
fn cache_hit_matches_cold_extraction_as_a_whole_document() {
    let (_directory, extractor) = extractor();
    let bytes = package(&document(r#"<w:p><w:r><w:t>cached</w:t></w:r></w:p>"#), &[]);

    let cold = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();
    let cached = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(cached, cold);
}

#[test]
fn cache_hits_are_isolated_by_current_limits_policy() {
    let directory = tempdir().unwrap();
    let cache_root = directory.path().join("cache");
    let permissive = DocumentExtractor::production(cache_root.clone()).unwrap();
    let bytes = package(&document(r#"<w:p><w:r><w:t>strict</w:t></w:r></w:p>"#), &[]);
    permissive.extract(DocumentFormat::Docx, &bytes).unwrap();

    let strict = DocumentExtractor::with_limits(
        cache_root,
        ExtractionLimits {
            max_extracted_text_bytes: 1,
            ..ExtractionLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        strict.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::ExtractedTextBytes
        ))
    ));
}

#[test]
fn corrupt_and_mismatched_cache_entries_fall_back_to_extraction() {
    let directory = tempdir().unwrap();
    let cache_root = directory.path().join("cache");
    let extractor = DocumentExtractor::production(cache_root.clone()).unwrap();
    let bytes = package(&document(r#"<w:p><w:r><w:t>cache</w:t></w:r></w:p>"#), &[]);
    let expected = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();
    let cache_path = one_cache_json(&cache_root);

    fs::write(&cache_path, b"not json").unwrap();
    assert_eq!(
        extractor.extract(DocumentFormat::Docx, &bytes).unwrap(),
        expected
    );

    let mut mismatched = expected.clone();
    mismatched.original_fingerprint = "sha256:wrong".to_owned();
    fs::write(&cache_path, serde_json::to_vec(&mismatched).unwrap()).unwrap();
    assert_eq!(
        extractor.extract(DocumentFormat::Docx, &bytes).unwrap(),
        expected
    );

    let mut bad_digest = expected.clone();
    bad_digest.canonical_representation_digest = "sha256:wrong".to_owned();
    fs::write(&cache_path, serde_json::to_vec(&bad_digest).unwrap()).unwrap();
    assert_eq!(
        extractor.extract(DocumentFormat::Docx, &bytes).unwrap(),
        expected
    );
}

#[test]
fn rejects_corrupt_zip_and_xml_depth() {
    let (_directory, extractor) = extractor();
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, b"not a zip"),
        Err(ExtractionError::Corrupt)
    ));

    let limits = ExtractionLimits {
        max_xml_depth: 3,
        ..ExtractionLimits::default()
    };
    let directory = tempdir().unwrap();
    let extractor = DocumentExtractor::with_limits(directory.path().join("cache"), limits).unwrap();
    let bytes = package(
        &document(r#"<w:p><w:r><w:t>too deep</w:t></w:r></w:p>"#),
        &[],
    );

    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(ExtractionLimit::XmlDepth))
    ));
}
