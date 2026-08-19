//! The RAR5 container: CRC32-sealed vint-framed blocks, the file
//! header's flag-dependent fields, and the extra-area records that
//! mark per-entry encryption.

use crate::{crc32, le32, Entry, Error, Method, Result};

// Block types 1 (main) and 3 (service) carry nothing the walk needs
// and skip like any unknown block.
const BLOCK_FILE: u64 = 2;
const BLOCK_ENCRYPTION: u64 = 4;
const BLOCK_END: u64 = 5;

/// Block flags.
const HAS_EXTRA: u64 = 0x0001;
const HAS_DATA: u64 = 0x0002;

/// File flags.
const FILE_DIRECTORY: u64 = 0x0001;
const FILE_MTIME: u64 = 0x0002;
const FILE_CRC: u64 = 0x0004;

/// Extra-area record: the entry is encrypted.
const RECORD_ENCRYPTION: u64 = 0x01;

/// Little-endian base-128; ten bytes bound a 64-bit value.
fn vint(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for step in 0..10 {
        let byte = *bytes.get(*at).ok_or(Error::Truncated)?;
        *at += 1;
        value |= ((byte & 0x7F) as u64) << (7 * step);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(Error::Corrupt("vint"))
}

pub(crate) fn walk(bytes: &[u8]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut at = 8usize;
    while at < bytes.len() {
        let declared = le32(bytes, at)?;
        let mut cursor = at + 4;
        let sealed_start = cursor;
        let header_size = vint(bytes, &mut cursor)? as usize;
        let header_end = cursor.checked_add(header_size).ok_or(Error::Truncated)?;
        let sealed = bytes
            .get(sealed_start..header_end)
            .ok_or(Error::Truncated)?;
        if crc32(sealed) != declared {
            return Err(Error::Corrupt("header crc"));
        }
        let head_type = vint(bytes, &mut cursor)?;
        let flags = vint(bytes, &mut cursor)?;
        let extra_size = if flags & HAS_EXTRA != 0 {
            vint(bytes, &mut cursor)? as usize
        } else {
            0
        };
        let data_size = if flags & HAS_DATA != 0 {
            vint(bytes, &mut cursor)?
        } else {
            0
        };
        let data_len: usize = data_size.try_into().map_err(|_| Error::Truncated)?;
        let block_end = header_end.checked_add(data_len).ok_or(Error::Truncated)?;
        if block_end > bytes.len() {
            return Err(Error::Truncated);
        }
        match head_type {
            // Whole-archive encryption: every later block is ciphertext.
            BLOCK_ENCRYPTION => return Err(Error::Encrypted),
            BLOCK_FILE => {
                entries.push(file_entry(
                    bytes, cursor, header_end, extra_size, data_size, header_end,
                )?);
            }
            BLOCK_END => break,
            _ => {}
        }
        at = block_end;
    }
    Ok(entries)
}

/// Reads one file header between `cursor` and `header_end`; the extra
/// area is the header's last `extra_size` bytes, `data_at` is where
/// the packed bytes start.
fn file_entry(
    bytes: &[u8],
    mut cursor: usize,
    header_end: usize,
    extra_size: usize,
    data_size: u64,
    data_at: usize,
) -> Result<Entry> {
    let file_flags = vint(bytes, &mut cursor)?;
    let unpacked_size = vint(bytes, &mut cursor)?;
    let _attributes = vint(bytes, &mut cursor)?;
    if file_flags & FILE_MTIME != 0 {
        cursor = cursor.checked_add(4).ok_or(Error::Truncated)?;
    }
    let crc = if file_flags & FILE_CRC != 0 {
        let value = le32(bytes, cursor)?;
        cursor += 4;
        Some(value)
    } else {
        None
    };
    let compression = vint(bytes, &mut cursor)?;
    let _host = vint(bytes, &mut cursor)?;
    let name_len = vint(bytes, &mut cursor)? as usize;
    let fields_end = header_end.checked_sub(extra_size).ok_or(Error::Truncated)?;
    let name_end = cursor.checked_add(name_len).ok_or(Error::Truncated)?;
    if name_end > fields_end {
        return Err(Error::Corrupt("name size"));
    }
    let name = String::from_utf8_lossy(&bytes[cursor..name_end]).into_owned();
    let encrypted = extra_encrypted(bytes.get(fields_end..header_end).ok_or(Error::Truncated)?)?;
    let method_bits = ((compression >> 7) & 0x7) as u8;
    let method = if method_bits == 0 {
        Method::Stored
    } else {
        Method::Compressed {
            generation: (compression & 0x3F) as u8,
            method: method_bits,
        }
    };
    let data_len: usize = data_size.try_into().map_err(|_| Error::Truncated)?;
    Ok(Entry {
        name,
        packed_size: data_size,
        unpacked_size,
        crc,
        method,
        directory: file_flags & FILE_DIRECTORY != 0,
        encrypted,
        data: data_at..data_at + data_len,
    })
}

/// Walks the extra area's records for a file-encryption record.
fn extra_encrypted(area: &[u8]) -> Result<bool> {
    let mut at = 0usize;
    while at < area.len() {
        let size = vint(area, &mut at)? as usize;
        let record_end = at.checked_add(size).ok_or(Error::Corrupt("extra size"))?;
        if record_end > area.len() {
            return Err(Error::Corrupt("extra size"));
        }
        let mut inner = at;
        let record_type = vint(area, &mut inner)?;
        if record_type == RECORD_ENCRYPTION {
            return Ok(true);
        }
        at = record_end;
    }
    Ok(false)
}
