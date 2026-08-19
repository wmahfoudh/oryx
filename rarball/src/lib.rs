//! Reader for RAR archives: both container generations, the RAR 1.5
//! to 4 header chain and RAR5's vint-framed blocks, the entry walk,
//! and stored extraction with CRC verification. Everything reads
//! zero-copy from one byte slice, every offset is bounds-checked, and
//! a lying container errors, never panics.
//!
//! The layouts follow the rarlab format notes, cross-checked against
//! the nwaples/rardecode reader.

mod v4;
mod v5;

use std::borrow::Cow;

/// Why an archive cannot be read. `Encrypted` and `Unsupported` are
/// refusals to relay to the user; the others mark a file that is not,
/// or no longer is, an archive.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    NotRar,
    Truncated,
    Encrypted,
    Corrupt(&'static str),
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRar => f.write_str("not a RAR archive"),
            Error::Truncated => f.write_str("the archive ends early"),
            Error::Encrypted => f.write_str("the archive is encrypted"),
            Error::Corrupt(what) => write!(f, "corrupt archive: {what}"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Standard CRC32 over a compile-time table; both generations seal
/// headers and data with it.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    const fn table() -> [u32; 256] {
        let mut table = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut crc = n as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
                bit += 1;
            }
            table[n] = crc;
            n += 1;
        }
        table
    }
    static TABLE: [u32; 256] = table();
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc = (crc >> 8) ^ TABLE[((crc ^ byte as u32) & 0xFF) as usize];
    }
    !crc
}

/// How an entry's data is packed. Stored extracts; the compressed
/// methods are recognized and refused.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Method {
    Stored,
    /// `generation` is the format's declared algorithm version (15 to
    /// 29 in the old chain, 0 or 1 in RAR5); `method` its 1-to-5
    /// compression level.
    Compressed {
        generation: u8,
        method: u8,
    },
}

/// One archive member, as the walk listed it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub packed_size: u64,
    pub unpacked_size: u64,
    /// CRC32 of the unpacked data; RAR5 entries may omit it.
    pub crc: Option<u32>,
    pub method: Method,
    pub directory: bool,
    pub encrypted: bool,
    /// The packed bytes' range in the input.
    pub(crate) data: std::ops::Range<usize>,
}

/// The open archive: one borrowed byte slice and the walked entries.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

const SIG4: &[u8] = b"Rar!\x1a\x07\x00";
const SIG5: &[u8] = b"Rar!\x1a\x07\x01\x00";

impl<'a> Archive<'a> {
    /// Walks every header. An archive whose headers are encrypted, or
    /// whose header chain lies about its sizes, errors here; entry
    /// data is only ranged, never touched.
    pub fn open(bytes: &'a [u8]) -> Result<Archive<'a>> {
        let entries = if bytes.starts_with(SIG5) {
            v5::walk(bytes)?
        } else if bytes.starts_with(SIG4) {
            v4::walk(bytes)?
        } else {
            return Err(Error::NotRar);
        };
        Ok(Archive { bytes, entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry's unpacked bytes. A stored entry is a verified
    /// subslice of the input; compressed methods are refused until a
    /// decompressor exists, and encrypted entries always refuse.
    pub fn extract(&self, entry: &Entry) -> Result<Cow<'a, [u8]>> {
        if entry.encrypted {
            return Err(Error::Encrypted);
        }
        match entry.method {
            Method::Stored => {
                let data = self.bytes.get(entry.data.clone()).ok_or(Error::Truncated)?;
                if let Some(declared) = entry.crc {
                    if crc32(data) != declared {
                        return Err(Error::Corrupt("data crc"));
                    }
                }
                Ok(Cow::Borrowed(data))
            }
            Method::Compressed { .. } => Err(Error::Unsupported("compressed entry")),
        }
    }
}

pub(crate) fn le16(bytes: &[u8], at: usize) -> Result<u16> {
    let slice = bytes.get(at..at + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

pub(crate) fn le32(bytes: &[u8], at: usize) -> Result<u32> {
    let slice = bytes.get(at..at + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
