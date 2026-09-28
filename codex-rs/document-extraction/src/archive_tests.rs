use std::io::Write;

use zip::CompressionMethod;
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

#[test]
fn preflight_counts_actual_total_uncompressed_bytes() {
    let bytes = zip_with_entries(&[("word/document.xml", b"0123456789"), ("a", b"abcdefghij")]);
    let limits = ExtractionLimits {
        max_total_uncompressed_bytes: 19,
        ..ExtractionLimits::default()
    };

    assert!(matches!(
        archive::preflight(&bytes, &limits),
        Err(ExtractionError::LimitExceeded(
            ExtractionLimit::ZipTotalBytes
        ))
    ));
}

#[test]
fn preflight_uses_decompressed_bytes_not_forged_zip_metadata() {
    let contents = vec![b'x'; 10_000];
    let mut bytes = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut bytes));
        writer
            .start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(&contents).unwrap();
        writer.finish().unwrap();
    }
    forge_uncompressed_sizes(&mut bytes, 1);
    let limits = ExtractionLimits {
        max_entry_bytes: 9_999,
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
fn preflight_rejects_encrypted_and_malformed_archives() {
    let mut encrypted = zip_with_entries(&[("word/document.xml", b"xml")]);
    mark_encrypted(&mut encrypted);
    assert!(matches!(
        archive::preflight(&encrypted, &ExtractionLimits::default()),
        Err(ExtractionError::Encrypted)
    ));

    assert!(matches!(
        archive::preflight(b"not a zip", &ExtractionLimits::default()),
        Err(ExtractionError::Corrupt)
    ));
}

fn forge_uncompressed_sizes(bytes: &mut [u8], size: u32) {
    for index in 0..bytes.len().saturating_sub(4) {
        if bytes[index..].starts_with(&0x0403_4b50u32.to_le_bytes()) {
            bytes[index + 22..index + 26].copy_from_slice(&size.to_le_bytes());
        } else if bytes[index..].starts_with(&0x0201_4b50u32.to_le_bytes()) {
            bytes[index + 24..index + 28].copy_from_slice(&size.to_le_bytes());
        }
    }
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
