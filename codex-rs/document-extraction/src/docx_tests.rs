use std::io::Write;

use pretty_assertions::assert_eq;
use tempfile::tempdir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::DocumentExtractor;
use crate::DocumentFormat;
use crate::ExtractedBlock;
use crate::ExtractionAnchor;
use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;
use crate::ExtractionNotice;
use crate::ExtractionStatus;

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
        result.blocks,
        vec![ExtractedBlock {
            anchor: ExtractionAnchor {
                scheme: "docx-paragraph".to_owned(),
                locator: "body/p[1]".to_owned(),
            },
            text: "  Café\t\nfin".to_owned(),
            notices: Vec::new(),
        }]
    );
    assert_eq!(result.status, ExtractionStatus::Complete);
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
        result.blocks,
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
        ]
    );
}

#[test]
fn splits_long_paragraphs_at_utf8_boundaries() {
    let (_directory, extractor) = extractor();
    let text = format!("{}x", "é".repeat(3_072));
    let xml = document(&format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#));
    let bytes = package(&xml, &[]);

    let result = extractor.extract(DocumentFormat::Docx, &bytes).unwrap();

    assert_eq!(result.blocks.len(), 2);
    assert_eq!(
        result.blocks,
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
        ]
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

    assert_eq!(result.status, ExtractionStatus::Partial);
    assert_eq!(result.blocks.len(), 1);
    assert_eq!(result.blocks[0].text, "inserted");
    assert!(result.notices.contains(&ExtractionNotice::TrackedChanges));
    assert!(result.notices.contains(&ExtractionNotice::UnsupportedPart {
        part: "word/header1.xml".to_owned()
    }));
    assert!(result.notices.contains(&ExtractionNotice::UnsupportedPart {
        part: "word/comments.xml".to_owned()
    }));
    assert!(result.notices.contains(&ExtractionNotice::UnsupportedPart {
        part: "w:drawing".to_owned()
    }));
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
