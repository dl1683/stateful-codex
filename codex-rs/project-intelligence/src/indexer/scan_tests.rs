use std::fs;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

#[test]
fn coverage_describes_searchable_content_instead_of_bytes_scanned() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let concise_path = temp_dir.path().join("concise.md");
    let summarized_path = temp_dir.path().join("summarized.md");
    fs::write(&concise_path, "# One\nsecond\nthird\n").expect("concise fixture should write");
    fs::write(&summarized_path, "# One\nsecond\nthird\nfourth\n")
        .expect("summarized fixture should write");

    let concise = scan_file(temp_dir.path(), &concise_path).expect("concise file should scan");
    let summarized =
        scan_file(temp_dir.path(), &summarized_path).expect("summarized file should scan");

    assert_eq!(concise.coverage, ContextMapCoverage::Complete);
    assert_eq!(summarized.coverage, ContextMapCoverage::Partial);
}
