use std::io::Cursor;
use std::io::Read;
use std::io::Seek;

use quick_xml::events::BytesStart;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use zip::ZipArchive;

use crate::ExtractedBlock;
use crate::ExtractedDocument;
use crate::ExtractionAnchor;
use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;
use crate::ExtractionNotice;
use crate::ExtractionStatus;
use crate::archive;

const SCHEME: &str = "docx-paragraph";
const WORD_NS: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";

pub(crate) fn extract(
    bytes: &[u8],
    limits: &ExtractionLimits,
    original_fingerprint: String,
    original_bytes: u64,
) -> Result<ExtractedDocument, ExtractionError> {
    archive::preflight(bytes, limits)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| ExtractionError::Corrupt)?;
    let unsupported_parts = collect_unsupported_parts(&mut archive)?;
    let document_xml = read_document_xml(&mut archive)?;
    parse_document(
        &document_xml,
        limits,
        original_fingerprint,
        original_bytes,
        unsupported_parts,
    )
}

fn collect_unsupported_parts<R: Read + Seek>(
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

fn read_document_xml<R: Read + Seek>(
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

fn parse_document(
    document_xml: &[u8],
    limits: &ExtractionLimits,
    original_fingerprint: String,
    original_bytes: u64,
    unsupported_parts: Vec<String>,
) -> Result<ExtractedDocument, ExtractionError> {
    if limits.max_canonical_block_bytes == 0 {
        return Err(ExtractionError::LimitExceeded(
            ExtractionLimit::CanonicalBlockBytes,
        ));
    }

    let mut state = ParserState::new(limits);
    for part in unsupported_parts {
        state.add_notice(ExtractionNotice::UnsupportedPart { part });
    }

    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    loop {
        match reader.read_resolved_event_into(&mut buffer) {
            Ok((resolve, Event::Start(start))) => state.start(&resolve, &start)?,
            Ok((resolve, Event::Empty(empty))) => state.empty(&resolve, &empty)?,
            Ok((resolve, Event::End(end))) => state.end(&resolve, end.name())?,
            Ok((_, Event::Text(text))) => state.text(&text)?,
            Ok((_, Event::CData(cdata))) => state.cdata(&cdata)?,
            Ok((_, Event::Eof)) => break,
            Ok((_, Event::Decl(_)))
            | Ok((_, Event::PI(_)))
            | Ok((_, Event::Comment(_)))
            | Ok((_, Event::DocType(_)))
            | Ok((_, Event::GeneralRef(_))) => {}
            _ => {}
        }
        buffer.clear();
    }
    if state.depth != 0 || !state.body_seen {
        return Err(ExtractionError::Corrupt);
    }

    let status =
        if state.notices.is_empty() && state.blocks.iter().all(|block| block.notices.is_empty()) {
            ExtractionStatus::Complete
        } else {
            ExtractionStatus::Partial
        };
    Ok(ExtractedDocument {
        original_fingerprint,
        original_bytes,
        extractor: crate::ExtractorIdentity {
            name: "codex-docx".to_owned(),
            version: "1".to_owned(),
        },
        canonical_representation_digest: String::new(),
        status,
        notices: state.notices,
        blocks: state.blocks,
    })
}

struct ParserState<'a> {
    limits: &'a ExtractionLimits,
    depth: usize,
    stack: Vec<Element>,
    body_seen: bool,
    body_paragraphs: usize,
    body_tables: usize,
    tables: Vec<TableContext>,
    rows: Vec<RowContext>,
    cells: Vec<CellContext>,
    paragraph: Option<Paragraph>,
    ignored_depth: usize,
    blocks: Vec<ExtractedBlock>,
    extracted_text_bytes: u64,
    notices: Vec<ExtractionNotice>,
}

struct TableContext {
    path: String,
    rows: usize,
}

struct RowContext {
    path: String,
    cells: usize,
}

struct CellContext {
    path: String,
    paragraphs: usize,
    tables: usize,
}

struct Element {
    name: Vec<u8>,
    local: Vec<u8>,
    word: bool,
}

impl Element {
    fn new(resolve: &ResolveResult<'_>, name: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            local: local_name(name),
            word: is_word_namespace(resolve),
        }
    }
}

struct Paragraph {
    locator: String,
    text: String,
    notices: Vec<ExtractionNotice>,
}

impl<'a> ParserState<'a> {
    fn new(limits: &'a ExtractionLimits) -> Self {
        Self {
            limits,
            depth: 0,
            stack: Vec::new(),
            body_seen: false,
            body_paragraphs: 0,
            body_tables: 0,
            tables: Vec::new(),
            rows: Vec::new(),
            cells: Vec::new(),
            paragraph: None,
            ignored_depth: 0,
            blocks: Vec::new(),
            extracted_text_bytes: 0,
            notices: Vec::new(),
        }
    }

    fn start(
        &mut self,
        resolve: &ResolveResult<'_>,
        event: &BytesStart<'_>,
    ) -> Result<(), ExtractionError> {
        self.check_attributes(event)?;
        let element = Element::new(resolve, event.name().as_ref());
        if self.ignored_depth > 0 {
            self.ignored_depth += 1;
            self.stack.push(element);
            self.depth += 1;
            if self.depth > self.limits.max_xml_depth {
                return Err(ExtractionError::LimitExceeded(ExtractionLimit::XmlDepth));
            }
            return Ok(());
        }

        if element.is_word(b"body") {
            self.body_seen = true;
        } else if element.is_word(b"tbl") {
            self.start_table();
        } else if element.is_word(b"tr") {
            self.start_row();
        } else if element.is_word(b"tc") {
            self.start_cell();
        } else if element.is_word(b"p") {
            self.start_paragraph();
        } else if element.is_word(b"ins")
            || element.is_word(b"del")
            || element.is_word(b"moveFrom")
            || element.is_word(b"moveTo")
            || element.is_word(b"rPrChange")
        {
            self.add_tracked_change();
        } else if is_unsupported_element(&element) {
            self.add_unsupported_element(&element);
            self.ignored_depth = 1;
        }

        self.stack.push(element);
        self.depth += 1;
        if self.depth > self.limits.max_xml_depth {
            return Err(ExtractionError::LimitExceeded(ExtractionLimit::XmlDepth));
        }
        Ok(())
    }

    fn empty(
        &mut self,
        resolve: &ResolveResult<'_>,
        event: &BytesStart<'_>,
    ) -> Result<(), ExtractionError> {
        self.start(resolve, event)?;
        self.end(resolve, event.name())
    }

    fn end(
        &mut self,
        resolve: &ResolveResult<'_>,
        name: quick_xml::name::QName<'_>,
    ) -> Result<(), ExtractionError> {
        let element_name = name.as_ref();
        if self.ignored_depth > 0 {
            self.ignored_depth -= 1;
        } else if is_word_element(resolve, name, b"p") {
            self.finish_paragraph()?;
        } else if is_word_element(resolve, name, b"tab") {
            self.append_text("\t")?;
        } else if is_word_element(resolve, name, b"br") || is_word_element(resolve, name, b"cr") {
            self.append_text("\n")?;
        }

        let popped = self.stack.pop().ok_or(ExtractionError::Corrupt)?;
        if popped.name != element_name {
            return Err(ExtractionError::Corrupt);
        }
        if is_word_element(resolve, name, b"tc") {
            self.cells.pop();
        } else if is_word_element(resolve, name, b"tr") {
            self.rows.pop();
        } else if is_word_element(resolve, name, b"tbl") {
            self.tables.pop();
        }
        self.depth = self.depth.checked_sub(1).ok_or(ExtractionError::Corrupt)?;
        Ok(())
    }

    fn text(&mut self, text: &quick_xml::events::BytesText<'_>) -> Result<(), ExtractionError> {
        if self.ignored_depth > 0
            || !self
                .stack
                .last()
                .is_some_and(|element| element.is_word(b"t"))
            || self.stack.iter().any(|element| element.is_word(b"delText"))
        {
            return Ok(());
        }
        let decoded = text.xml10_content().map_err(|_| ExtractionError::Corrupt)?;
        let value = quick_xml::escape::unescape(&decoded).map_err(|_| ExtractionError::Corrupt)?;
        self.append_text(&value)
    }

    fn cdata(&mut self, text: &quick_xml::events::BytesCData<'_>) -> Result<(), ExtractionError> {
        if self.ignored_depth > 0
            || !self
                .stack
                .last()
                .is_some_and(|element| element.is_word(b"t"))
            || self.stack.iter().any(|element| element.is_word(b"delText"))
        {
            return Ok(());
        }
        let value = text.xml10_content().map_err(|_| ExtractionError::Corrupt)?;
        self.append_text(&value)
    }

    fn check_attributes(&self, event: &BytesStart<'_>) -> Result<(), ExtractionError> {
        let mut count = 0;
        for attribute in event.attributes() {
            attribute.map_err(|_| ExtractionError::Corrupt)?;
            count += 1;
            if count > self.limits.max_attributes_per_element {
                return Err(ExtractionError::LimitExceeded(
                    ExtractionLimit::XmlAttributes,
                ));
            }
        }
        Ok(())
    }

    fn start_table(&mut self) {
        let path = if let Some(cell) = self.cells.last_mut() {
            cell.tables += 1;
            format!("{}/tbl[{}]", cell.path, cell.tables)
        } else {
            self.body_tables += 1;
            format!("body/tbl[{}]", self.body_tables)
        };
        self.tables.push(TableContext { path, rows: 0 });
    }

    fn start_row(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.rows += 1;
            let path = format!("{}/tr[{}]", table.path, table.rows);
            self.rows.push(RowContext { path, cells: 0 });
        }
    }

    fn start_cell(&mut self) {
        if let Some(row) = self.rows.last_mut() {
            row.cells += 1;
            let path = format!("{}/tc[{}]", row.path, row.cells);
            self.cells.push(CellContext {
                path,
                paragraphs: 0,
                tables: 0,
            });
        }
    }

    fn start_paragraph(&mut self) {
        let locator = if let Some(cell) = self.cells.last_mut() {
            cell.paragraphs += 1;
            format!("{}/p[{}]", cell.path, cell.paragraphs)
        } else if self.tables.is_empty()
            && self.stack.iter().any(|element| element.is_word(b"body"))
        {
            self.body_paragraphs += 1;
            format!("body/p[{}]", self.body_paragraphs)
        } else {
            return;
        };
        self.paragraph = Some(Paragraph {
            locator,
            text: String::new(),
            notices: Vec::new(),
        });
    }

    fn append_text(&mut self, text: &str) -> Result<(), ExtractionError> {
        let Some(paragraph) = self.paragraph.as_mut() else {
            return Ok(());
        };
        paragraph.text.push_str(text);
        self.extracted_text_bytes += text.len() as u64;
        if self.extracted_text_bytes > self.limits.max_extracted_text_bytes {
            return Err(ExtractionError::LimitExceeded(
                ExtractionLimit::ExtractedTextBytes,
            ));
        }
        Ok(())
    }

    fn finish_paragraph(&mut self) -> Result<(), ExtractionError> {
        let Some(paragraph) = self.paragraph.take() else {
            return Ok(());
        };
        if paragraph.text.is_empty() {
            return Ok(());
        }
        let slices = byte_slices(&paragraph.text, self.limits.max_canonical_block_bytes);
        if self.blocks.len() + slices.len() > self.limits.max_extracted_blocks {
            return Err(ExtractionError::LimitExceeded(
                ExtractionLimit::ExtractedBlocks,
            ));
        }
        for (start, end, text) in slices {
            let locator = if start == 0 && end == paragraph.text.len() {
                paragraph.locator.clone()
            } else {
                format!("{}@bytes[{start}:{end}]", paragraph.locator)
            };
            self.blocks.push(ExtractedBlock {
                anchor: ExtractionAnchor {
                    scheme: SCHEME.to_owned(),
                    locator,
                },
                text,
                notices: paragraph.notices.clone(),
            });
        }
        Ok(())
    }

    fn add_tracked_change(&mut self) {
        let notice = ExtractionNotice::TrackedChanges;
        self.add_notice(notice.clone());
        if let Some(paragraph) = self.paragraph.as_mut()
            && !paragraph.notices.contains(&notice)
        {
            paragraph.notices.push(notice);
        }
    }

    fn add_unsupported_element(&mut self, element: &Element) {
        let part = String::from_utf8_lossy(&element.name).into_owned();
        let notice = ExtractionNotice::UnsupportedPart { part };
        self.add_notice(notice.clone());
        if let Some(paragraph) = self.paragraph.as_mut()
            && !paragraph.notices.contains(&notice)
        {
            paragraph.notices.push(notice);
        }
    }

    fn add_notice(&mut self, notice: ExtractionNotice) {
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }
}

impl Element {
    fn is_word(&self, local: &[u8]) -> bool {
        self.word && self.local == local
    }
}

fn is_word_namespace(resolve: &ResolveResult<'_>) -> bool {
    matches!(resolve, ResolveResult::Bound(namespace) if namespace.0 == WORD_NS)
}

fn is_word_element(
    resolve: &ResolveResult<'_>,
    name: quick_xml::name::QName<'_>,
    local: &[u8],
) -> bool {
    is_word_namespace(resolve) && name.local_name().as_ref() == local
}

fn local_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .rposition(|byte| *byte == b':')
        .map_or_else(|| name.to_owned(), |index| name[index + 1..].to_owned())
}

fn is_unsupported_element(element: &Element) -> bool {
    element.is_word(b"drawing")
        || element.is_word(b"pict")
        || element.is_word(b"object")
        || element.is_word(b"txbxContent")
}

fn byte_slices(text: &str, max_bytes: usize) -> Vec<(usize, usize, String)> {
    let mut slices = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            let Some(next_boundary) =
                (start + 1..=text.len()).find(|offset| text.is_char_boundary(*offset))
            else {
                return slices;
            };
            end = next_boundary;
        }
        slices.push((start, end, text[start..end].to_owned()));
        start = end;
    }
    slices
}
