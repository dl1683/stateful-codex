use std::io::Write;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::archive;
use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;

fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut bytes));
        for (name, contents) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }
    bytes
}

#[test]
fn preflight_counts_actual_entry_bytes() {
    let bytes = zip_with_entries(&[("word/document.xml", b"0123456789")]);
    let limits = ExtractionLimits {
        max_entry_bytes: 9,
        ..ExtractionLimits::default()
    };

    assert!(matches!(
        archive::preflight(&bytes, &limits),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::ZipEntryBytes
        ))
    ));
}

#[test]
fn preflight_enforces_entry_count() {
    let bytes = zip_with_entries(&[("a", b"a"), ("b", b"b")]);
    let limits = ExtractionLimits {
        max_zip_entries: 1,
        ..ExtractionLimits::default()
    };

    assert!(matches!(
        archive::preflight(&bytes, &limits),
        Err(ExtractionError::LimitExceeded(ExtractionLimit::ZipEntries))
    ));
}
