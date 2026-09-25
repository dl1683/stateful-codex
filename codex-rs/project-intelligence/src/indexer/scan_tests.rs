use std::fs;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::indexer::regions::MAX_REGION_DESCRIPTION_BYTES;

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
