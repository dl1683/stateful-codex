use serde::Deserialize;
use serde::Serialize;

/// The safety boundary that stopped an extraction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExtractionLimit {
    OriginalBytes,
    ZipEntries,
    ZipEntryBytes,
    ZipTotalBytes,
    XmlDepth,
    XmlAttributes,
    ExtractedBlocks,
    ExtractedTextBytes,
    CanonicalBlockBytes,
    XlsxSheets,
    XlsxCells,
    XlsxSharedStrings,
    XlsxUsedArea,
    CacheEntryBytes,
    CacheBytes,
}

/// Production limits for archive preflight and canonical extraction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtractionLimits {
    pub max_original_bytes: u64,
    pub max_zip_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
    pub max_xml_depth: usize,
    pub max_attributes_per_element: usize,
    pub max_extracted_blocks: usize,
    pub max_extracted_text_bytes: u64,
    pub max_canonical_block_bytes: usize,
    pub max_xlsx_sheets: usize,
    pub max_xlsx_cells: usize,
    pub max_xlsx_shared_strings: usize,
    pub max_xlsx_used_area: u64,
    pub max_cache_entry_bytes: u64,
    pub max_cache_bytes: u64,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_original_bytes: 64 * 1024 * 1024,
            max_zip_entries: 512,
            max_entry_bytes: 32 * 1024 * 1024,
            max_total_uncompressed_bytes: 128 * 1024 * 1024,
            max_xml_depth: 128,
            max_attributes_per_element: 256,
            max_extracted_blocks: 4_096,
            max_extracted_text_bytes: 16 * 1024 * 1024,
            max_canonical_block_bytes: 6 * 1024,
            max_xlsx_sheets: 128,
            max_xlsx_cells: 250_000,
            max_xlsx_shared_strings: 100_000,
            max_xlsx_used_area: 1_000_000,
            max_cache_entry_bytes: 24 * 1024 * 1024,
            max_cache_bytes: 512 * 1024 * 1024,
        }
    }
}
