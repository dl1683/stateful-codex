use std::fs;
use std::io::Cursor;
use std::io::Write;

use pretty_assertions::assert_eq;
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::indexer::regions::MAX_REGION_DESCRIPTION_BYTES;

fn generated_docx(body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
    writer
        .start_file("word/document.xml", SimpleFileOptions::default())
        .expect("document part should start");
    writer
        .write_all(
            format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
            )
            .as_bytes(),
        )
        .expect("document part should write");
    writer.finish().expect("package should finish");
    bytes
}

fn mark_encrypted(bytes: &mut [u8]) {
    for index in 0..bytes.len().saturating_sub(4) {
        let offset = if bytes[index..].starts_with(&0x0403_4b50u32.to_le_bytes()) {
            Some(6)
        } else if bytes[index..].starts_with(&0x0201_4b50u32.to_le_bytes()) {
            Some(8)
        } else {
            None
        };
        if let Some(offset) = offset {
            let flags = u16::from_le_bytes([bytes[index + offset], bytes[index + offset + 1]]) | 1;
            bytes[index + offset..index + offset + 2].copy_from_slice(&flags.to_le_bytes());
        }
    }
}

#[test]
fn coverage_describes_searchable_content_instead_of_bytes_scanned() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let concise_path = temp_dir.path().join("concise.md");
    let region_path = temp_dir.path().join("region.md");
    let truncated_path = temp_dir.path().join("truncated.md");
    fs::write(&concise_path, "# One\nsecond\nthird\n").expect("concise fixture should write");
    fs::write(&region_path, "# One\nsecond\nthird\nfourth\n").expect("region fixture should write");
    fs::write(
        &truncated_path,
        "x".repeat(MAX_REGION_DESCRIPTION_BYTES + 1),
    )
    .expect("truncated fixture should write");

    let concise = scan_file(temp_dir.path(), &concise_path).expect("concise file should scan");
    let region = scan_file(temp_dir.path(), &region_path).expect("region file should scan");
    let truncated =
        scan_file(temp_dir.path(), &truncated_path).expect("truncated file should scan");

    assert_eq!(concise.coverage, ContextMapCoverage::Complete);
    assert_eq!(region.coverage, ContextMapCoverage::Complete);
    assert_eq!(truncated.coverage, ContextMapCoverage::Partial);
}

#[test]
fn regions_split_before_searchable_text_would_be_truncated() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let path = temp_dir.path().join("scorecard.md");
    let content = (1..=64)
        .map(|line| {
            let marker = if line == 59 {
                "decisive_routed_result"
            } else {
                "ordinary_measurement"
            };
            format!("{line:02}: {marker} {}", "x".repeat(180))
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, content).expect("scorecard fixture should write");

    let file = scan_file(temp_dir.path(), &path).expect("scorecard should scan");

    assert!(file.regions.len() > 1);
    assert!(
        file.regions
            .iter()
            .all(|region| region.description.len() <= MAX_REGION_DESCRIPTION_BYTES)
    );
    assert!(
        file.regions
            .iter()
            .all(|region| region.coverage == ContextMapCoverage::Complete)
    );
    let decisive = file
        .regions
        .iter()
        .find(|region| region.description.contains("decisive_routed_result"))
        .expect("decisive line should remain searchable");
    let (start, end) = decisive
        .anchor
        .locator
        .split_once('-')
        .expect("line anchor should have a range");
    assert!(start.parse::<usize>().expect("start line") <= 59);
    assert!(end.parse::<usize>().expect("end line") >= 59);
}

#[test]
fn project_region_budget_preserves_the_complete_file_inventory() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let content = (1..=130)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(temp_dir.path().join("a.md"), &content).expect("first fixture should write");
    fs::write(temp_dir.path().join("b.md"), content).expect("second fixture should write");

    let scan = scan_roots_with_limits(
        &[temp_dir.path().to_path_buf()],
        ScanLimits {
            max_files: 10,
            max_project_regions: 1,
        },
        scan_file,
    )
    .expect("project should scan");

    assert_eq!(scan.files.len(), 2);
    assert_eq!(
        scan.files
            .iter()
            .map(|file| file.regions.len())
            .sum::<usize>(),
        1
    );
    assert!(
        scan.files
            .iter()
            .all(|file| file.coverage == ContextMapCoverage::Partial)
    );
    assert!(scan.truncated);
    assert!(scan.inventory_complete);
}

#[test]
fn docx_region_budget_marks_coverage_partial() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    fs::write(
        temp_dir.path().join("budget.docx"),
        generated_docx(
            r#"<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>"#,
        ),
    )
    .expect("DOCX fixture should write");
    let scan = scan_roots_with_limits(
        &[temp_dir.path().to_path_buf()],
        ScanLimits {
            max_files: 10,
            max_project_regions: 1,
        },
        scan_file,
    )
    .expect("DOCX should scan");
    assert_eq!(scan.files.len(), 1);
    assert_eq!(scan.files[0].regions.len(), 1);
    assert_eq!(scan.files[0].coverage, ContextMapCoverage::Partial);
    assert!(scan.truncated);
    assert_eq!(scan.files_skipped, 0);
}

#[test]
fn docx_extraction_failures_are_partial_files_without_skips_or_archive_names() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    fs::write(
        temp_dir.path().join("corrupt.docx"),
        b"word/secret-entry-name.xml is not a ZIP",
    )
    .expect("corrupt fixture should write");
    let mut encrypted = generated_docx(r#"<w:p><w:r><w:t>encrypted</w:t></w:r></w:p>"#);
    mark_encrypted(&mut encrypted);
    fs::write(temp_dir.path().join("encrypted.docx"), encrypted)
        .expect("encrypted fixture should write");
    let scan = scan_roots(&[temp_dir.path().to_path_buf()]).expect("failure scan should complete");
    assert_eq!(scan.files.len(), 2);
    assert_eq!(scan.files_skipped, 0);
    assert!(scan.files.iter().all(|file| {
        file.regions.is_empty()
            && file.coverage == ContextMapCoverage::Partial
            && file.description.len() <= 2_048
            && !file.description.contains("secret-entry-name")
    }));
    assert!(
        scan.files
            .iter()
            .any(|file| file.description.contains("corrupt"))
    );
    assert!(
        scan.files
            .iter()
            .any(|file| file.description.contains("encrypted"))
    );
    let oversized = scan_docx(
        temp_dir.path(),
        "oversized.docx".to_string(),
        crate::SourceFingerprint::parse("sha256:oversized").expect("valid fingerprint"),
        None,
    )
    .expect("bounded over-limit result should index");
    assert!(oversized.regions.is_empty());
    assert_eq!(oversized.coverage, ContextMapCoverage::Partial);
    assert!(oversized.description.contains("limit-original-bytes"));
}
