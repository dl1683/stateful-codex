use std::io::Cursor;
use std::io::Read;

use zip::ZipArchive;

use crate::ExtractionError;

pub(super) struct Package {
    pub(super) document_xml: Vec<u8>,
    pub(super) unsupported_parts: Vec<String>,
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
) -> Result<Vec<String>, ExtractionError> {
    let mut parts = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| ExtractionError::Corrupt)?;
        let name = entry.name();
        if is_unsupported_part(name) {
            parts.push(name.to_owned());
        }
    }
    Ok(parts)
}

fn is_unsupported_part(name: &str) -> bool {
    name.starts_with("word/header")
        || name.starts_with("word/footer")
        || name == "word/comments.xml"
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        || name.starts_with("word/embeddings/")
        || name.starts_with("word/media/")
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
