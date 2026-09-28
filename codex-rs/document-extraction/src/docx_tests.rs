use std::io::Write;

use pretty_assertions::assert_eq;
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

fn extractor() -> DocumentExtractor {
    DocumentExtractor::production()
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
            version: "2".to_owned(),
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
    let extractor = extractor();
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
    let extractor = extractor();
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
    let extractor = extractor();
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
    let extractor = extractor();
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
fn repeated_extraction_matches_as_a_whole_document() {
    let extractor = extractor();
    let bytes = package(&document(r#"<w:p><w:r><w:t>cached</w:t></w:r></w:p>"#), &[]);

    let cold = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();
    let second = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(second, cold);
}

#[test]
fn rejects_corrupt_zip_and_xml_depth() {
    let extractor = extractor();
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, b"not a zip"),
        Err(ExtractionError::Corrupt)
    ));

    let limits = ExtractionLimits {
        max_xml_depth: 3,
        ..ExtractionLimits::default()
    };
    let extractor = DocumentExtractor::with_limits(limits);
    let bytes = package(
        &document(r#"<w:p><w:r><w:t>too deep</w:t></w:r></w:p>"#),
        &[],
    );

    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(ExtractionLimit::XmlDepth))
    ));
}

#[test]
fn empty_structural_elements_preserve_ordinals() {
    let extractor = extractor();
    let bytes = package(
        &document(
            r#"<w:p/><w:p><w:r><w:t>body</w:t></w:r></w:p><w:tbl/><w:tbl><w:tr/><w:tr><w:tc/><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
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
                        locator: "body/p[2]".to_owned(),
                    },
                    text: "body".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[2]/tr[2]/tc[2]/p[1]".to_owned(),
                    },
                    text: "cell".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );
}

#[test]
fn empty_body_and_expanded_inline_controls_are_structural() {
    let extractor = extractor();
    let bytes = package(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>a</w:t><w:tab></w:tab><w:br></w:br><w:cr></w:cr><w:t>b</w:t></w:r></w:p></w:body></w:document>"#,
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
                text: "a\t\n\nb".to_owned(),
                notices: Vec::new(),
            }],
        )
    );

    let empty_body = package(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#,
        &[],
    );
    let result = extractor
        .extract(DocumentFormat::Docx, &empty_body)
        .unwrap();
    assert_eq!(
        result,
        expected_document(
            &empty_body,
            ExtractionStatus::Complete,
            Vec::new(),
            Vec::new()
        )
    );
}

#[test]
fn transparent_wrappers_and_nested_tables_have_unique_locators() {
    let extractor = extractor();
    let bytes = package(
        &document(
            r#"<w:sdt><w:sdtContent><w:p><w:r><w:t>wrapped</w:t></w:r></w:p><w:customXml><w:smartTag><w:p><w:r><w:t>deep</w:t></w:r></w:p></w:smartTag></w:customXml></w:sdtContent></w:sdt><w:tbl><w:tr><w:tc><w:p><w:r><w:t>outer</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>nested</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc><w:tc><w:p><w:r><w:t>second</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
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
                        locator: "body/p[1]".to_owned(),
                    },
                    text: "wrapped".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[2]".to_owned(),
                    },
                    text: "deep".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/p[1]".to_owned(),
                    },
                    text: "outer".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/tbl[1]/tr[1]/tc[1]/p[1]".to_owned(),
                    },
                    text: "nested".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[2]/p[1]".to_owned(),
                    },
                    text: "second".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );
}

#[test]
fn accepts_alternate_word_namespace_prefixes() {
    let extractor = extractor();
    let transitional = package(
        r#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:p><d:hyperlink><d:r><d:t>linked</d:t></d:r></d:hyperlink></d:p></d:body></d:document>"#,
        &[],
    );
    let strict = package(
        r#"<d:document xmlns:d="http://purl.oclc.org/ooxml/wordprocessingml/main"><d:body><d:p><d:hyperlink><d:r><d:t>linked</d:t></d:r></d:hyperlink></d:p></d:body></d:document>"#,
        &[],
    );
    let transitional_result = extractor
        .extract(DocumentFormat::Docx, &transitional)
        .unwrap();
    let result = extractor.extract(DocumentFormat::Docx, &strict).unwrap();
    assert_eq!(
        result,
        expected_document(
            &strict,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: "docx-paragraph".to_owned(),
                    locator: "body/p[1]".to_owned(),
                },
                text: "linked".to_owned(),
                notices: Vec::new(),
            }],
        )
    );
    assert_eq!(result.blocks, transitional_result.blocks);
    assert_eq!(
        result.canonical_representation_digest,
        transitional_result.canonical_representation_digest
    );
    assert_ne!(
        result.original_fingerprint,
        transitional_result.original_fingerprint
    );
    assert_ne!(result.original_bytes, transitional_result.original_bytes);
}

#[test]
fn ignored_subtrees_preserve_real_structure() {
    let extractor = extractor();
    let bytes = package(
        &document(
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>before</w:t></w:r><w:drawing><w:tbl><w:tr><w:tc><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:drawing><w:r><w:t>after</w:t></w:r></w:p><w:p><w:r><w:t>second</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>nested</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc><w:tc><w:p><w:r><w:t>next-cell</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>next-row</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>tail</w:t></w:r></w:p>"#,
        ),
        &[],
    );

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(
        result,
        expected_document(
            &bytes,
            ExtractionStatus::Partial,
            vec![ExtractionNotice::UnsupportedPart {
                part: "w:drawing".to_owned(),
            }],
            vec![
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/p[1]".to_owned(),
                    },
                    text: "beforeafter".to_owned(),
                    notices: vec![ExtractionNotice::UnsupportedPart {
                        part: "w:drawing".to_owned(),
                    }],
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/p[2]".to_owned(),
                    },
                    text: "second".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[1]/tbl[1]/tr[1]/tc[1]/p[1]".to_owned(),
                    },
                    text: "nested".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[1]/tc[2]/p[1]".to_owned(),
                    },
                    text: "next-cell".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/tbl[1]/tr[2]/tc[1]/p[1]".to_owned(),
                    },
                    text: "next-row".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]".to_owned(),
                    },
                    text: "tail".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );
}

#[test]
fn ignored_subtrees_still_enforce_xml_limits() {
    let deep = package(
        &document(r#"<w:p><w:drawing><w:a><w:b><w:c><w:d/></w:c></w:b></w:a></w:drawing></w:p>"#),
        &[],
    );
    let deep_extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_xml_depth: 5,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        deep_extractor.extract(DocumentFormat::Docx, &deep),
        Err(ExtractionError::LimitExceeded(ExtractionLimit::XmlDepth))
    ));

    let attributes = package(
        &document(r#"<w:p><w:drawing><w:fake a="1" b="2"/></w:drawing></w:p>"#),
        &[],
    );
    let attribute_extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_attributes_per_element: 1,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        attribute_extractor.extract(DocumentFormat::Docx, &attributes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::XmlAttributes
        ))
    ));
}

#[test]
fn refuses_scalars_larger_than_the_block_limit() {
    for (limit, text) in [(1, "é"), (3, "😀"), (3, "a😀b")] {
        let extractor = DocumentExtractor::with_limits(ExtractionLimits {
            max_canonical_block_bytes: limit,
            ..ExtractionLimits::default()
        });
        let bytes = package(
            &document(&format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#)),
            &[],
        );
        assert!(matches!(
            extractor.extract(DocumentFormat::Docx, &bytes),
            Err(ExtractionError::LimitExceeded(
                ExtractionLimit::CanonicalBlockBytes
            ))
        ));
    }
}

#[test]
fn splits_at_utf8_boundaries_without_exceeding_the_limit() {
    let extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_canonical_block_bytes: 4,
        ..ExtractionLimits::default()
    });
    let bytes = package(&document(r#"<w:p><w:r><w:t>a😀b</w:t></w:r></w:p>"#), &[]);

    assert_eq!(
        extractor.extract(DocumentFormat::Docx, &bytes).unwrap(),
        expected_document(
            &bytes,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]@bytes[0:1]".to_owned(),
                    },
                    text: "a".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]@bytes[1:5]".to_owned(),
                    },
                    text: "😀".to_owned(),
                    notices: Vec::new(),
                },
                ExtractedBlock {
                    anchor: ExtractionAnchor {
                        scheme: "docx-paragraph".to_owned(),
                        locator: "body/p[1]@bytes[5:6]".to_owned(),
                    },
                    text: "b".to_owned(),
                    notices: Vec::new(),
                },
            ],
        )
    );

    let emoji = package(&document(r#"<w:p><w:r><w:t>😀</w:t></w:r></w:p>"#), &[]);
    assert_eq!(
        extractor.extract(DocumentFormat::Docx, &emoji).unwrap(),
        expected_document(
            &emoji,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: "docx-paragraph".to_owned(),
                    locator: "body/p[1]".to_owned(),
                },
                text: "😀".to_owned(),
                notices: Vec::new(),
            }],
        )
    );
}

#[test]
fn normalizes_xml_lines_and_reads_cdata() {
    let extractor = extractor();
    let bytes = package(
        &document("<w:p><w:r><w:t>a\r\nb<![CDATA[<raw>\r\nc]]></w:t></w:r></w:p>"),
        &[],
    );
    let bom_bytes = package(
        &format!(
            "\u{feff}{}",
            document(r#"<w:p><w:r><w:t>bom</w:t></w:r></w:p>"#)
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
                text: "a\nb<raw>\nc".to_owned(),
                notices: Vec::new(),
            }],
        )
    );
    let result = extractor.extract(DocumentFormat::Docx, &bom_bytes).unwrap();
    assert_eq!(
        result,
        expected_document(
            &bom_bytes,
            ExtractionStatus::Complete,
            Vec::new(),
            vec![ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: "docx-paragraph".to_owned(),
                    locator: "body/p[1]".to_owned(),
                },
                text: "bom".to_owned(),
                notices: Vec::new(),
            }],
        )
    );
}

#[test]
fn enforces_attribute_text_block_and_original_limits() {
    let bytes = package(
        &document(r#"<w:p a="1" b="2"><w:r><w:t>abc</w:t></w:r></w:p>"#),
        &[],
    );
    let extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_attributes_per_element: 1,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::XmlAttributes
        ))
    ));

    let extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_extracted_text_bytes: 2,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::ExtractedTextBytes
        ))
    ));

    let extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_extracted_blocks: 1,
        max_canonical_block_bytes: 1,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::ExtractedBlocks
        ))
    ));

    let extractor = DocumentExtractor::with_limits(ExtractionLimits {
        max_original_bytes: 1,
        ..ExtractionLimits::default()
    });
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &bytes),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::OriginalBytes
        ))
    ));
}

#[test]
fn rejects_missing_document_xml_and_malformed_xml() {
    let extractor = extractor();
    let mut missing = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut missing));
        writer
            .start_file("word/styles.xml", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"styles").unwrap();
        writer.finish().unwrap();
    }
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &missing),
        Err(ExtractionError::Corrupt)
    ));

    let malformed = package(&document(r#"<w:p><w:r><w:t>bad</w:r></w:p>"#), &[]);
    assert!(matches!(
        extractor.extract(DocumentFormat::Docx, &malformed),
        Err(ExtractionError::Corrupt)
    ));
}
