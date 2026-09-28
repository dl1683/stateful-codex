use std::io::Cursor;
use std::io::Read;

use zip::ZipArchive;

use crate::ExtractionError;
use crate::ExtractionLimit;
use crate::ExtractionLimits;

pub(crate) fn preflight(bytes: &[u8], limits: &ExtractionLimits) -> Result<(), ExtractionError> {
    if contains_encrypted_entry(bytes) {
        return Err(ExtractionError::Encrypted);
    }
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

fn contains_encrypted_entry(bytes: &[u8]) -> bool {
    let Some(end) = bytes.windows(4).rposition(|window| window == b"PK\x05\x06") else {
        return false;
    };
    if end + 20 > bytes.len() {
        return false;
    }
    let central_offset =
        u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    let central_size =
        u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap()) as usize;
    let Some(central_end) = central_offset.checked_add(central_size) else {
        return false;
    };
    if central_end > bytes.len() {
        return false;
    }
    let mut position = central_offset;
    while position + 46 <= central_end && bytes[position..].starts_with(b"PK\x01\x02") {
        let flags = u16::from_le_bytes(bytes[position + 8..position + 10].try_into().unwrap());
        if flags & 1 != 0 {
            return true;
        }
        let name_length =
            u16::from_le_bytes(bytes[position + 28..position + 30].try_into().unwrap()) as usize;
        let extra_length =
            u16::from_le_bytes(bytes[position + 30..position + 32].try_into().unwrap()) as usize;
        let comment_length =
            u16::from_le_bytes(bytes[position + 32..position + 34].try_into().unwrap()) as usize;
        let Some(next) = position
            .checked_add(46)
            .and_then(|value| value.checked_add(name_length))
            .and_then(|value| value.checked_add(extra_length))
            .and_then(|value| value.checked_add(comment_length))
        else {
            return false;
        };
        position = next;
    }
    false
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
