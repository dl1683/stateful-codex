use std::io::Cursor;
use std::io::Read;

use zip::ZipArchive;

use crate::ExtractionError;
use crate::UnsupportedPartCategory;

pub(super) struct Package {
    pub(super) document_xml: Vec<u8>,
    pub(super) unsupported_parts: Vec<(UnsupportedPartCategory, u16)>,
}

pub(super) fn read(bytes: &[u8]) -> Result<Package, ExtractionError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| ExtractionError::Corrupt)?;
    let unsupported_parts = collect_unsupported_parts(&mut archive)?;
    let document_xml = read_document_xml(&mut archive)?;
    Ok(Package {
        document_xml,
        unsupported_parts,
    })
}

fn collect_unsupported_parts<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<(UnsupportedPartCategory, u16)>, ExtractionError> {
    let mut parts = std::collections::BTreeMap::<UnsupportedPartCategory, u16>::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| ExtractionError::Corrupt)?;
        let name = entry.name();
        if let Some(category) = unsupported_part_category(name) {
            parts
                .entry(category)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
    }
    Ok(parts.into_iter().collect())
}

fn unsupported_part_category(name: &str) -> Option<UnsupportedPartCategory> {
    if name.starts_with("word/header") || name.starts_with("word/footer") {
        Some(UnsupportedPartCategory::HeadersFooters)
    } else if name == "word/comments.xml" {
        Some(UnsupportedPartCategory::Comments)
    } else if name == "word/footnotes.xml" || name == "word/endnotes.xml" {
        Some(UnsupportedPartCategory::FootnotesEndnotes)
    } else if name.starts_with("word/embeddings/") || name.starts_with("word/media/") {
        Some(UnsupportedPartCategory::DrawingsEmbeddedObjects)
    } else {
        None
    }
}

fn read_document_xml<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<u8>, ExtractionError> {
    let mut entry = archive
        .by_name("word/document.xml")
        .map_err(|_| ExtractionError::Corrupt)?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|_| ExtractionError::Corrupt)?;
    Ok(bytes)
}
