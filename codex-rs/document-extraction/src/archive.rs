use std::io::Cursor;
use std::io::Read;

use zip::ZipArchive;

use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;

pub(crate) fn preflight(bytes: &[u8], limits: &ExtractionLimits) -> Result<(), ExtractionError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| ExtractionError::Corrupt)?;
    if archive.len() > limits.max_zip_entries {
        return Err(ExtractionError::LimitExceeded(ExtractionLimit::ZipEntries));
    }

    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ExtractionError::Corrupt)?;
        if entry.encrypted() {
            return Err(ExtractionError::Encrypted);
        }
        read_bounded(&mut entry, limits.max_entry_bytes, &mut total, limits)?;
    }
    Ok(())
}

fn read_bounded<R: Read>(
    reader: &mut R,
    entry_limit: u64,
    total: &mut u64,
    limits: &ExtractionLimits,
) -> Result<(), ExtractionError> {
    let mut buffer = [0u8; 8 * 1024];
    let mut entry_bytes = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| ExtractionError::Corrupt)?;
        if read == 0 {
            return Ok(());
        }
        entry_bytes += read as u64;
        *total += read as u64;
        if entry_bytes > entry_limit {
            return Err(ExtractionError::LimitExceeded(
                ExtractionLimit::ZipEntryBytes,
            ));
        }
        if *total > limits.max_total_uncompressed_bytes {
            return Err(ExtractionError::LimitExceeded(
                ExtractionLimit::ZipTotalBytes,
            ));
        }
    }
}
