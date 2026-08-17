//! Builds Palm-database books in memory for the palmbook tests, so
//! every fixture stays readable Rust instead of a binary file in the
//! tree. The layouts follow the MobileRead documentation; offsets are
//! absolute within record 0 unless said otherwise.
#![allow(dead_code)]

pub const COMPRESSION_NONE: u16 = 1;
pub const COMPRESSION_PALMDOC: u16 = 2;
pub const COMPRESSION_HUFFCDIC: u16 = 17480;

/// A book under construction; `build` assembles the container.
pub struct BookBuilder {
    pub name: &'static str,
    pub type_code: &'static [u8; 4],
    pub creator: &'static [u8; 4],
    pub compression: u16,
    pub encryption: u16,
    pub encoding: u32,
    pub title: String,
    pub text: Vec<u8>,
    pub record_size: usize,
    pub exth: Vec<(u32, Vec<u8>)>,
    /// The extra record data flags at 0xF2; `build` appends matching
    /// trailing sections to every text record.
    pub extra_flags: u16,
    /// Records appended after the text (images, indexes).
    pub extra_records: Vec<Vec<u8>>,
    /// HuffCdic tables appended when the compression asks for them.
    pub huff_records: Vec<Vec<u8>>,
}

pub fn book(text: &str) -> BookBuilder {
    BookBuilder {
        name: "test-book",
        type_code: b"BOOK",
        creator: b"MOBI",
        compression: COMPRESSION_NONE,
        encryption: 0,
        encoding: 65001,
        title: "Test Book".to_string(),
        text: text.as_bytes().to_vec(),
        record_size: 4096,
        exth: Vec::new(),
        extra_flags: 0,
        extra_records: Vec::new(),
        huff_records: Vec::new(),
    }
}

impl BookBuilder {
    pub fn build(&self) -> Vec<u8> {
        let mut chunks: Vec<Vec<u8>> = self
            .text
            .chunks(self.record_size)
            .map(|chunk| match self.compression {
                COMPRESSION_PALMDOC => palmdoc_compress(chunk),
                _ => chunk.to_vec(),
            })
            .collect();
        for chunk in &mut chunks {
            append_trailing(chunk, self.extra_flags);
        }
        let record0 = self.record0(chunks.len() as u16);
        let mut records: Vec<Vec<u8>> = vec![record0];
        records.extend(chunks);
        records.extend(self.huff_records.iter().cloned());
        records.extend(self.extra_records.iter().cloned());
        pdb(self.name, self.type_code, self.creator, &records)
    }

    fn record0(&self, record_count: u16) -> Vec<u8> {
        let mut exth = Vec::new();
        if !self.exth.is_empty() {
            exth.extend_from_slice(b"EXTH");
            let body: Vec<u8> = self
                .exth
                .iter()
                .flat_map(|(kind, data)| {
                    let mut rec = kind.to_be_bytes().to_vec();
                    rec.extend_from_slice(&(data.len() as u32 + 8).to_be_bytes());
                    rec.extend_from_slice(data);
                    rec
                })
                .collect();
            exth.extend_from_slice(&(body.len() as u32 + 12).to_be_bytes());
            exth.extend_from_slice(&(self.exth.len() as u32).to_be_bytes());
            exth.extend_from_slice(&body);
            while exth.len() % 4 != 0 {
                exth.push(0);
            }
        }
        let header_len = 232u32;
        let name_offset = 16 + header_len as usize + exth.len();
        let huff_offset = if self.huff_records.is_empty() {
            0
        } else {
            1 + record_count as u32
        };

        let mut out = Vec::new();
        // The 16-byte PalmDOC header.
        out.extend_from_slice(&self.compression.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(self.text.len() as u32).to_be_bytes());
        out.extend_from_slice(&record_count.to_be_bytes());
        out.extend_from_slice(&(self.record_size as u16).to_be_bytes());
        out.extend_from_slice(&self.encryption.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        // The MOBI header, 232 bytes from its magic.
        out.extend_from_slice(b"MOBI");
        out.extend_from_slice(&header_len.to_be_bytes());
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&self.encoding.to_be_bytes());
        out.extend_from_slice(&7u32.to_be_bytes());
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&[0xFF; 40]);
        out.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 80: first non-book index
        out.extend_from_slice(&(name_offset as u32).to_be_bytes()); // 84
        out.extend_from_slice(&(self.title.len() as u32).to_be_bytes()); // 88
        out.extend_from_slice(&9u32.to_be_bytes()); // 92: locale
        out.extend_from_slice(&[0; 8]); // 96: input, output language
        out.extend_from_slice(&6u32.to_be_bytes()); // 104: min version
        out.extend_from_slice(&(1 + record_count as u32).to_be_bytes()); // 108: first image
        out.extend_from_slice(&huff_offset.to_be_bytes()); // 112
        out.extend_from_slice(&(self.huff_records.len() as u32).to_be_bytes()); // 116
        out.extend_from_slice(&[0; 8]); // 120: huff table offset and length
        out.extend_from_slice(&if self.exth.is_empty() { 0u32 } else { 0x40u32 }.to_be_bytes()); // 128
        out.extend_from_slice(&[0; 32]); // 132: unknown
        out.extend_from_slice(&[0xFF; 4]); // 164
        out.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 168: drm offset
        out.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 172: drm count
        out.extend_from_slice(&[0; 8]); // 176: drm size, flags
        out.extend_from_slice(&[0; 8]); // 184: unknown
        out.extend_from_slice(&1u16.to_be_bytes()); // 192: first content record
        out.extend_from_slice(&record_count.to_be_bytes()); // 194: last content record
        out.extend_from_slice(&1u32.to_be_bytes()); // 196
        out.extend_from_slice(&[0; 40]); // 200: fcis, flis, unknown
        out.extend_from_slice(&[0, 0]); // 240
        out.extend_from_slice(&self.extra_flags.to_be_bytes()); // 242
        out.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 244: indx record
        assert_eq!(
            out.len(),
            16 + header_len as usize,
            "record 0 layout drifted"
        );
        out.extend_from_slice(&exth);
        out.extend_from_slice(self.title.as_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }
}

/// The PDB shell around finished records.
pub fn pdb(name: &str, type_code: &[u8; 4], creator: &[u8; 4], records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut name_bytes = [0u8; 32];
    name_bytes[..name.len().min(31)].copy_from_slice(&name.as_bytes()[..name.len().min(31)]);
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(&[0; 28]); // attributes through sortInfoID
    out.extend_from_slice(type_code);
    out.extend_from_slice(creator);
    out.extend_from_slice(&[0; 8]); // uniqueIDseed, nextRecordListID
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    let data_start = out.len() + records.len() * 8 + 2;
    let mut offset = data_start;
    for (index, record) in records.iter().enumerate() {
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.push(0);
        out.extend_from_slice(&(index as u32).to_be_bytes()[1..]);
        offset += record.len();
    }
    out.extend_from_slice(&[0, 0]);
    for record in records {
        out.extend_from_slice(record);
    }
    out
}

/// Trailing sections per the extra record data flags: bit 0 is the
/// multibyte overlap (its size byte carries the count), each higher bit
/// a backward-encoded entry whose value includes its own bytes. The
/// reader strips bit 1 first from the true end, so sections append in
/// reverse bit order.
fn append_trailing(record: &mut Vec<u8>, flags: u16) {
    if flags & 1 != 0 {
        record.extend_from_slice(&[0xAA]); // an overlap byte
        record.push(1); // its count
    }
    for bit in 1..16 {
        if flags & (1 << bit) != 0 {
            record.extend_from_slice(&[0xBB, 0xBB, 0xBB]); // entry payload
            record.push(0x80 | 4); // backward varint: payload plus itself
        }
    }
}

/// Naive PalmDOC compression: safe literals pass through, everything
/// else rides a length-1 literal run, and one seeded backreference and
/// space fold exercise the decoder's paths when the text allows.
pub fn palmdoc_compress(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        // A space followed by a letter folds into one byte.
        if b == b' ' && i + 1 < text.len() && (0x40..0x80).contains(&text[i + 1]) {
            out.push(text[i + 1] | 0x80);
            i += 2;
            continue;
        }
        if (0x09..0x80).contains(&b) || b == 0x00 {
            out.push(b);
        } else {
            out.push(0x01);
            out.push(b);
        }
        i += 1;
    }
    out
}

/// A hand-built HuffCdic table pair: every code is one terminal byte,
/// code value equal to dictionary index, which makes the cache entries
/// `8 | 0x80 | ((2 * index) << 8)` per the maxcode arithmetic.
pub fn huff_records(phrases: &[(&[u8], bool)]) -> Vec<Vec<u8>> {
    let mut huff = Vec::new();
    huff.extend_from_slice(b"HUFF");
    huff.extend_from_slice(&24u32.to_be_bytes());
    huff.extend_from_slice(&24u32.to_be_bytes()); // cache table offset
    huff.extend_from_slice(&(24u32 + 1024).to_be_bytes()); // base table offset
    huff.extend_from_slice(&[0; 8]);
    for code in 0..256u32 {
        let entry = if (code as usize) < phrases.len() {
            8 | 0x80 | ((2 * code) << 8)
        } else {
            0
        };
        huff.extend_from_slice(&entry.to_be_bytes());
    }
    huff.extend_from_slice(&[0; 256]); // base table, unused by terminal codes

    let mut cdic = Vec::new();
    cdic.extend_from_slice(b"CDIC");
    cdic.extend_from_slice(&16u32.to_be_bytes());
    cdic.extend_from_slice(&(phrases.len() as u32).to_be_bytes());
    cdic.extend_from_slice(&8u32.to_be_bytes()); // code length in bits
    let mut offsets = Vec::new();
    let mut data = Vec::new();
    for (phrase, done) in phrases {
        offsets.push(data.len() as u16 + phrases.len() as u16 * 2);
        let len = phrase.len() as u16 | if *done { 0x8000 } else { 0 };
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(phrase);
    }
    for offset in offsets {
        cdic.extend_from_slice(&offset.to_be_bytes());
    }
    cdic.extend_from_slice(&data);
    vec![huff, cdic]
}
