//! The RAR 1.5-to-4 header chain: CRC16-sealed blocks, the file header
//! with its flag-dependent fields, and the packed Unicode names.

use crate::{crc32, le16, le32, Entry, Error, Method, Result};

/// Main-header flag: the archive's headers are password-protected.
const MAIN_PASSWORD: u16 = 0x0080;
/// File-header flags.
const FILE_PASSWORD: u16 = 0x0004;
const FILE_DIRECTORY: u16 = 0x00E0;
const FILE_LARGE: u16 = 0x0100;
const FILE_UNICODE: u16 = 0x0200;
/// Any block: a 32-bit data size follows the base header.
const LONG_BLOCK: u16 = 0x8000;

pub(crate) fn walk(bytes: &[u8]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut at = 7usize;
    while at < bytes.len() {
        let declared = le16(bytes, at)?;
        let head_type = *bytes.get(at + 2).ok_or(Error::Truncated)?;
        let flags = le16(bytes, at + 3)?;
        let head_size = le16(bytes, at + 5)? as usize;
        if head_size < 7 {
            return Err(Error::Corrupt("header size"));
        }
        let header = bytes.get(at..at + head_size).ok_or(Error::Truncated)?;
        if (crc32(&header[2..]) & 0xFFFF) as u16 != declared {
            return Err(Error::Corrupt("header crc"));
        }
        match head_type {
            // Main header; a password flag means every later header is
            // ciphertext, so nothing past it can be walked.
            0x73 => {
                if flags & MAIN_PASSWORD != 0 {
                    return Err(Error::Encrypted);
                }
                at += head_size;
            }
            0x74 => {
                let (entry, data_len) = file_entry(header, flags, at + head_size)?;
                let end = (at + head_size)
                    .checked_add(data_len)
                    .ok_or(Error::Truncated)?;
                if end > bytes.len() {
                    return Err(Error::Truncated);
                }
                entries.push(entry);
                at = end;
            }
            0x7B => break,
            // Every other block skips whole: its header, and its data
            // when the long-block flag declares some.
            _ => {
                let add = if flags & LONG_BLOCK != 0 {
                    le32(bytes, at + 7)? as usize
                } else {
                    0
                };
                at = at
                    .checked_add(head_size)
                    .and_then(|a| a.checked_add(add))
                    .ok_or(Error::Truncated)?;
            }
        }
    }
    Ok(entries)
}

/// Reads one file header; `data_at` is where the packed bytes start.
fn file_entry(header: &[u8], flags: u16, data_at: usize) -> Result<(Entry, usize)> {
    let pack_low = le32(header, 7)? as u64;
    let unp_low = le32(header, 11)? as u64;
    let crc = le32(header, 16)?;
    let version = *header.get(24).ok_or(Error::Truncated)?;
    let method_byte = *header.get(25).ok_or(Error::Truncated)?;
    let name_size = le16(header, 26)? as usize;
    let (packed_size, unpacked_size, name_at) = if flags & FILE_LARGE != 0 {
        let high_pack = le32(header, 32)? as u64;
        let high_unp = le32(header, 36)? as u64;
        (
            pack_low | (high_pack << 32),
            unp_low | (high_unp << 32),
            40usize,
        )
    } else {
        (pack_low, unp_low, 32usize)
    };
    let name_bytes = header
        .get(name_at..name_at + name_size)
        .ok_or(Error::Corrupt("name size"))?;
    let name = if flags & FILE_UNICODE != 0 {
        unicode_name(name_bytes)
    } else {
        plain_name(name_bytes)
    };
    let directory = flags & FILE_DIRECTORY == FILE_DIRECTORY;
    let method = match method_byte {
        0x30 => Method::Stored,
        0x31..=0x35 => Method::Compressed {
            generation: version,
            method: method_byte - 0x30,
        },
        _ => return Err(Error::Corrupt("method")),
    };
    let data_len: usize = packed_size.try_into().map_err(|_| Error::Truncated)?;
    Ok((
        Entry {
            name,
            packed_size,
            unpacked_size,
            crc: Some(crc),
            method,
            directory,
            encrypted: flags & FILE_PASSWORD != 0,
            data: data_at..data_at + data_len,
        },
        data_len,
    ))
}

/// Names without the Unicode flag are single-byte text: UTF-8 when it
/// decodes (modern archivers write it), byte-for-byte otherwise.
fn plain_name(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// The Unicode name field: the plain name, a zero, then the packed
/// form; without the zero the field is already the full name.
fn unicode_name(bytes: &[u8]) -> String {
    match bytes.iter().position(|&b| b == 0) {
        Some(zero) => {
            let plain = &bytes[..zero];
            decode_packed(plain, &bytes[zero + 1..]).unwrap_or_else(|| plain_name(plain))
        }
        None => plain_name(bytes),
    }
}

/// The packed encoding: a high-byte page, then two-bit opcodes. 0 is a
/// plain byte, 1 a byte on the page, 2 a full UTF-16 unit, 3 a run
/// copied from the plain name, optionally corrected. A malformed
/// stream answers None and the plain name stands.
fn decode_packed(plain: &[u8], packed: &[u8]) -> Option<String> {
    let mut units: Vec<u16> = Vec::new();
    let high = (*packed.first()? as u16) << 8;
    let mut at = 1usize;
    let mut flags = 0u8;
    let mut bits = 0u8;
    while at < packed.len() {
        if bits == 0 {
            flags = packed[at];
            at += 1;
            bits = 8;
        }
        bits -= 2;
        match (flags >> bits) & 3 {
            0 => {
                units.push(*packed.get(at)? as u16);
                at += 1;
            }
            1 => {
                units.push(*packed.get(at)? as u16 + high);
                at += 1;
            }
            2 => {
                let low = *packed.get(at)? as u16;
                let page = *packed.get(at + 1)? as u16;
                units.push(low | (page << 8));
                at += 2;
            }
            _ => {
                let length = *packed.get(at)?;
                at += 1;
                if length & 0x80 != 0 {
                    let correction = *packed.get(at)? as u16;
                    at += 1;
                    for _ in 0..(length & 0x7F) + 2 {
                        let byte = plain.get(units.len()).copied().unwrap_or(0) as u16;
                        units.push(((byte + correction) & 0xFF) + high);
                    }
                } else {
                    for _ in 0..length as u16 + 2 {
                        units.push(plain.get(units.len()).copied().unwrap_or(0) as u16);
                    }
                }
            }
        }
    }
    Some(String::from_utf16_lossy(&units))
}
