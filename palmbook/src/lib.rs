//! Reader for Palm-database books: the PDB container, the MOBI record 0
//! with its EXTH metadata, PalmDOC and HuffCdic decompression, and the
//! rawml assembly. Everything reads zero-copy from one byte slice, every
//! offset is bounds-checked, and a lying container errors, never panics.
//!
//! The layouts follow the MobileRead format documentation.

pub mod huffcdic;
pub mod indx;
pub mod kf8;
pub mod palmdoc;

/// Why a container cannot be read. `Drm` is a refusal to relay to the
/// user; the others mark a file that is not, or no longer is, a book.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    NotPalm,
    Truncated,
    Drm,
    Corrupt(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotPalm => f.write_str("not a Palm-database book"),
            Error::Truncated => f.write_str("the container ends early"),
            Error::Drm => f.write_str("the book is DRM-protected"),
            Error::Corrupt(what) => write!(f, "corrupt container: {what}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn be16(bytes: &[u8], at: usize) -> Result<u16> {
    let slice = bytes.get(at..at + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn be32(bytes: &[u8], at: usize) -> Result<u32> {
    let slice = bytes.get(at..at + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// The PDB shell: the 78-byte header and the record table, records read
/// as slices of the input.
pub struct Pdb<'a> {
    bytes: &'a [u8],
    name: String,
    type_code: [u8; 4],
    creator: [u8; 4],
    /// Byte range per record; consecutive offsets bound each record.
    ranges: Vec<(usize, usize)>,
}

impl<'a> Pdb<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Pdb<'a>> {
        let head = bytes.get(..78).ok_or(Error::Truncated)?;
        let name_end = head[..32].iter().position(|&b| b == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&head[..name_end]).into_owned();
        let type_code = [head[60], head[61], head[62], head[63]];
        let creator = [head[64], head[65], head[66], head[67]];
        let count = be16(bytes, 76)? as usize;
        let table = bytes.get(78..78 + count * 8).ok_or(Error::Truncated)?;
        let mut starts = Vec::with_capacity(count);
        for entry in table.chunks_exact(8) {
            let offset = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
            starts.push(offset);
        }
        let mut ranges = Vec::with_capacity(count);
        for (index, &start) in starts.iter().enumerate() {
            let end = starts.get(index + 1).copied().unwrap_or(bytes.len());
            if start > end || end > bytes.len() {
                return Err(Error::Truncated);
            }
            ranges.push((start, end));
        }
        Ok(Pdb {
            bytes,
            name,
            type_code,
            creator,
            ranges,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn type_code(&self) -> [u8; 4] {
        self.type_code
    }

    pub fn creator(&self) -> [u8; 4] {
        self.creator
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn record(&self, index: usize) -> Result<&'a [u8]> {
        let &(start, end) = self.ranges.get(index).ok_or(Error::Truncated)?;
        Ok(&self.bytes[start..end])
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Compression {
    None,
    PalmDoc,
    HuffCdic,
}

/// The text encoding record 0 declares; anything else reads as 1252,
/// the format's default.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TextEncoding {
    Cp1252,
    Utf8,
}

/// The KF8 header fields, record indexes relative to the book's own
/// record 0; 0xFFFFFFFF marks an absent table.
#[derive(Debug, Clone, Copy)]
pub struct Kf8Header {
    pub fdst: u32,
    pub fdst_count: u32,
    pub ncx: u32,
    pub frag: u32,
    pub skel: u32,
    pub guide: u32,
}

/// A MOBI book over its container: record 0 parsed, the text records
/// decompressable, the metadata at hand.
pub struct Book<'a> {
    pdb: Pdb<'a>,
    /// The book's record 0 inside the PDB; nonzero for the KF8 half of
    /// a dual file. Every record index in the headers is relative to it.
    start: usize,
    version: u32,
    compression: Compression,
    encoding: TextEncoding,
    text_length: usize,
    record_count: usize,
    extra_flags: u16,
    huff_record: usize,
    huff_count: usize,
    first_image: Option<usize>,
    title: Option<String>,
    exth: Vec<(u32, Vec<u8>)>,
    kf8: Option<Kf8Header>,
    /// The INDX record field at 0xF4, the NCX of a MOBI6 book.
    mobi6_indx: u32,
}

impl<'a> Book<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Book<'a>> {
        Book::open_at(bytes, 0)
    }

    /// Opens the book whose record 0 sits at `start`, the KF8 half of a
    /// dual file when `start` is its boundary.
    pub fn open_at(bytes: &'a [u8], start: usize) -> Result<Book<'a>> {
        let pdb = Pdb::open(bytes)?;
        if &pdb.creator() != b"MOBI" || &pdb.type_code() != b"BOOK" {
            return Err(Error::NotPalm);
        }
        let record0 = pdb.record(start)?;
        let compression = match be16(record0, 0)? {
            1 => Compression::None,
            2 => Compression::PalmDoc,
            17480 => Compression::HuffCdic,
            _ => return Err(Error::Corrupt("unknown compression")),
        };
        let text_length = be32(record0, 4)? as usize;
        let record_count = be16(record0, 8)? as usize;
        if be16(record0, 12)? != 0 {
            return Err(Error::Drm);
        }
        if record0.get(16..20) != Some(b"MOBI") {
            return Err(Error::Corrupt("no MOBI header"));
        }
        let header_len = be32(record0, 20)? as usize;
        let encoding = match be32(record0, 28)? {
            65001 => TextEncoding::Utf8,
            _ => TextEncoding::Cp1252,
        };
        let version = be32(record0, 36)?;
        let name_offset = be32(record0, 84)? as usize;
        let name_length = be32(record0, 88)? as usize;
        let title = record0
            .get(name_offset..name_offset + name_length)
            .map(|raw| decode(raw, encoding))
            .filter(|t| !t.is_empty());
        let first_image = match be32(record0, 108)? {
            0xFFFF_FFFF => None,
            index => Some(index as usize),
        };
        let huff_record = be32(record0, 112)? as usize;
        let huff_count = be32(record0, 116)? as usize;
        let exth_present = be32(record0, 128)? & 0x40 != 0;
        // The extra data flags live at 0xF2 only in headers long enough
        // to carry them.
        let extra_flags = if header_len >= 228 {
            be16(record0, 242)?
        } else {
            0
        };
        let exth = if exth_present {
            read_exth(record0, 16 + header_len)?
        } else {
            Vec::new()
        };
        if record_count >= pdb.len() - start {
            return Err(Error::Truncated);
        }
        // The KF8 index fields, read only where the header reaches them.
        let kf8 = if version >= 8 {
            let field = |offset: usize| -> Result<u32> {
                if offset + 4 <= 16 + header_len {
                    be32(record0, offset)
                } else {
                    Ok(0xFFFF_FFFF)
                }
            };
            Some(Kf8Header {
                fdst: field(192)?,
                fdst_count: field(196)?,
                ncx: field(244)?,
                frag: field(248)?,
                skel: field(252)?,
                guide: field(260)?,
            })
        } else {
            None
        };
        let mobi6_indx = if version < 8 && header_len >= 232 {
            be32(record0, 244).unwrap_or(0xFFFF_FFFF)
        } else {
            0xFFFF_FFFF
        };
        Ok(Book {
            pdb,
            start,
            version,
            compression,
            encoding,
            text_length,
            record_count,
            extra_flags,
            huff_record,
            huff_count,
            first_image,
            title,
            exth,
            kf8,
            mobi6_indx,
        })
    }

    /// The MOBI header version: 6 for the old flow, 8 and up for KF8.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Bytes to text per the book's declared encoding.
    pub fn text(&self, bytes: &[u8]) -> String {
        decode(bytes, self.encoding)
    }

    /// The MOBI6 NCX index record, when the header carries one.
    pub fn mobi6_ncx(&self) -> Option<usize> {
        (self.version < 8 && self.mobi6_indx != 0xFFFF_FFFF && self.mobi6_indx != 0)
            .then_some(self.mobi6_indx as usize)
    }

    /// The KF8 index fields, present on version 8 books.
    pub fn kf8_header(&self) -> Option<Kf8Header> {
        self.kf8
    }

    /// Where the KF8 half of a dual file starts, from EXTH 121.
    pub fn kf8_boundary(&self) -> Option<usize> {
        self.exth
            .iter()
            .find(|(kind, _)| *kind == 121)
            .and_then(|(_, data)| data.get(..4))
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .filter(|&at| at != 0xFFFF_FFFF)
    }

    pub fn compression(&self) -> Compression {
        self.compression
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// The full name from record 0, updated titles in EXTH taking over.
    pub fn title(&self) -> Option<String> {
        self.exth_string(503).or_else(|| self.title.clone())
    }

    pub fn exth(&self) -> &[(u32, Vec<u8>)] {
        &self.exth
    }

    pub fn exth_string(&self, kind: u32) -> Option<String> {
        self.exth
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, data)| decode(data, self.encoding))
            .filter(|t| !t.is_empty())
    }

    /// The record index the book's images start at, when it has any.
    pub fn first_image(&self) -> Option<usize> {
        self.first_image
    }

    /// A record by book-relative index; the KF8 half of a dual file
    /// counts from its own record 0.
    pub fn record(&self, index: usize) -> Result<&'a [u8]> {
        self.pdb.record(self.start + index)
    }

    pub fn record_count(&self) -> usize {
        self.pdb.len() - self.start
    }

    /// The book text whole: every text record stripped of its trailing
    /// entries, decompressed, and truncated to the declared length.
    pub fn rawml(&self) -> Result<Vec<u8>> {
        let coder = match self.compression {
            Compression::HuffCdic => {
                if self.huff_count == 0 {
                    return Err(Error::Corrupt("no huffman tables"));
                }
                let huff = self.record(self.huff_record)?;
                let mut cdics = Vec::with_capacity(self.huff_count - 1);
                for index in 1..self.huff_count {
                    cdics.push(self.record(self.huff_record + index)?);
                }
                Some(huffcdic::HuffCdic::new(huff, &cdics)?)
            }
            _ => None,
        };
        let mut out = Vec::with_capacity(self.text_length);
        for index in 1..=self.record_count {
            let record = self.record(index)?;
            let cut = record.len() - trailing_size(record, self.extra_flags).min(record.len());
            let body = &record[..cut];
            match self.compression {
                Compression::None => out.extend_from_slice(body),
                Compression::PalmDoc => palmdoc::decompress(body, &mut out)?,
                Compression::HuffCdic => {
                    let coder = coder.as_ref().expect("tables built above");
                    out.extend_from_slice(&coder.unpack(body)?);
                }
            }
        }
        // A MOBI6 book pads its last record past the declared length; a
        // KF8 book declares only flow 0's length while the records
        // carry every flow, so its assembly is trusted whole.
        if self.version < 8 {
            out.truncate(self.text_length);
        }
        Ok(out)
    }
}

/// Bytes to text per the declared encoding, replacement on the bytes
/// the encoding cannot carry.
pub fn decode(bytes: &[u8], encoding: TextEncoding) -> String {
    match encoding {
        TextEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        TextEncoding::Cp1252 => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

/// Windows-1252 to a char; the low half is ASCII and the 0x80 row holds
/// the punctuation Unicode moved.
fn cp1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '•', '–', '—', '˜',
        '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match byte {
        0x00..=0x7F => byte as char,
        0x80..=0x9F => HIGH[(byte - 0x80) as usize],
        _ => char::from_u32(byte as u32).expect("latin-1 range is valid"),
    }
}

/// The EXTH block: magic, then typed records, each length inclusive.
fn read_exth(record0: &[u8], at: usize) -> Result<Vec<(u32, Vec<u8>)>> {
    if record0.get(at..at + 4) != Some(b"EXTH") {
        return Err(Error::Corrupt("no EXTH where the flag points"));
    }
    let count = be32(record0, at + 8)? as usize;
    let mut out = Vec::with_capacity(count.min(64));
    let mut cursor = at + 12;
    for _ in 0..count {
        let kind = be32(record0, cursor)?;
        let length = be32(record0, cursor + 4)? as usize;
        if length < 8 {
            return Err(Error::Corrupt("EXTH record shorter than its header"));
        }
        let data = record0
            .get(cursor + 8..cursor + length)
            .ok_or(Error::Truncated)?;
        out.push((kind, data.to_vec()));
        cursor += length;
    }
    Ok(out)
}

/// How many trailing bytes the extra-data flags put after this record's
/// content: one backward-encoded entry per high bit, then the multibyte
/// overlap whose size byte carries its count.
fn trailing_size(record: &[u8], flags: u16) -> usize {
    let mut size = 0usize;
    let mut test = flags >> 1;
    while test != 0 {
        if test & 1 != 0 {
            size += backward_entry(&record[..record.len().saturating_sub(size)]);
        }
        test >>= 1;
    }
    if flags & 1 != 0 {
        let end = record.len().saturating_sub(size);
        if end > 0 {
            size += (record[end - 1] as usize & 3) + 1;
        }
    }
    size.min(record.len())
}

/// A backward-encoded variable-width integer at the slice's end: 7-bit
/// groups read backwards, the leftmost byte flagged with 0x80; the
/// value counts the whole entry, itself included.
fn backward_entry(bytes: &[u8]) -> usize {
    let mut value = 0usize;
    let mut shift = 0u32;
    let mut index = bytes.len();
    while index > 0 && shift < 28 {
        index -= 1;
        let byte = bytes[index];
        value |= ((byte & 0x7F) as usize) << shift;
        shift += 7;
        if byte & 0x80 != 0 {
            break;
        }
    }
    value
}
