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
    /// The MOBI header version: 6 for the old flow, 8 for KF8.
    pub version: u32,
    /// Overrides the declared text length; real KF8 books declare flow
    /// 0's length while the records carry every flow.
    pub declared_text_length: Option<u32>,
    /// KF8 record indexes, absolute in the record list; 0xFFFFFFFF when
    /// absent. `fdst` pairs the record index with the flow count.
    pub fdst: (u32, u32),
    pub skelidx: u32,
    pub fragidx: u32,
    pub ncxidx: u32,
    pub guideidx: u32,
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
        version: 6,
        declared_text_length: None,
        fdst: (0xFFFF_FFFF, 0),
        skelidx: 0xFFFF_FFFF,
        fragidx: 0xFFFF_FFFF,
        ncxidx: 0xFFFF_FFFF,
        guideidx: 0xFFFF_FFFF,
    }
}

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

impl BookBuilder {
    pub fn build(&self) -> Vec<u8> {
        pdb(self.name, self.type_code, self.creator, &self.records())
    }

    /// The record list before the PDB shell, for composing dual files.
    pub fn records(&self) -> Vec<Vec<u8>> {
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
        records
    }

    /// How many records precede the extra records, for computing the
    /// absolute index of an appended table.
    pub fn extra_base(&self) -> u32 {
        (1 + self.text.chunks(self.record_size).count() + self.huff_records.len()) as u32
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
        let header_len: usize = if self.version >= 8 { 264 } else { 232 };
        let name_offset = 16 + header_len + exth.len();
        let huff_offset = if self.huff_records.is_empty() {
            0
        } else {
            1 + record_count as u32
        };

        let mut out = vec![0u8; 16 + header_len];
        // The 16-byte PalmDOC header.
        put16(&mut out, 0, self.compression);
        let declared = self.declared_text_length.unwrap_or(self.text.len() as u32);
        put32(&mut out, 4, declared);
        put16(&mut out, 8, record_count);
        put16(&mut out, 10, self.record_size as u16);
        put16(&mut out, 12, self.encryption);
        // The MOBI header, addressed by offset.
        out[16..20].copy_from_slice(b"MOBI");
        put32(&mut out, 20, header_len as u32);
        put32(&mut out, 24, 2); // mobi type: book
        put32(&mut out, 28, self.encoding);
        put32(&mut out, 32, 7); // unique id
        put32(&mut out, 36, self.version);
        out[40..80].fill(0xFF); // reserved
        put32(&mut out, 80, 0xFFFF_FFFF); // first non-book index
        put32(&mut out, 84, name_offset as u32);
        put32(&mut out, 88, self.title.len() as u32);
        put32(&mut out, 92, 9); // locale
        put32(&mut out, 104, self.version); // min version
        put32(&mut out, 108, 1 + record_count as u32); // first image
        put32(&mut out, 112, huff_offset);
        put32(&mut out, 116, self.huff_records.len() as u32);
        put32(&mut out, 128, if self.exth.is_empty() { 0 } else { 0x40 });
        put32(&mut out, 168, 0xFFFF_FFFF); // drm offset
        put32(&mut out, 172, 0xFFFF_FFFF); // drm count
        if self.version >= 8 {
            put32(&mut out, 192, self.fdst.0);
            put32(&mut out, 196, self.fdst.1);
            put16(&mut out, 242, self.extra_flags);
            put32(&mut out, 244, self.ncxidx);
            put32(&mut out, 248, self.fragidx);
            put32(&mut out, 252, self.skelidx);
            put32(&mut out, 256, 0xFFFF_FFFF); // datp index
            put32(&mut out, 260, self.guideidx);
        } else {
            put16(&mut out, 192, 1); // first content record
            put16(&mut out, 194, record_count);
            put32(&mut out, 196, 1);
            put16(&mut out, 242, self.extra_flags);
            put32(&mut out, 244, 0xFFFF_FFFF); // indx record
        }
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

/// One index entry under construction: the name bytes and the tag
/// values, which must match the TAGX table handed to `indx_records`.
pub struct IndxEntry {
    pub name: Vec<u8>,
    pub tags: Vec<(u8, Vec<u64>)>,
}

/// A forward variable-width value: seven bits per byte, the final byte
/// flagged with 0x80.
pub fn varint(value: u64) -> Vec<u8> {
    let mut groups = Vec::new();
    let mut v = value;
    loop {
        groups.push((v & 0x7F) as u8);
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    groups.reverse();
    *groups.last_mut().expect("at least one group") |= 0x80;
    groups
}

/// An INDX pair (header record and one data record) plus the CNCX
/// record when given. The TAGX quads are `(tag, values, mask, end)`
/// with single-bit masks in one control byte.
pub fn indx_records(
    tagx: &[(u8, u8, u8, u8)],
    entries: &[IndxEntry],
    cncx: Option<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let mut header = vec![0u8; 56];
    header[0..4].copy_from_slice(b"INDX");
    put32(&mut header, 4, 56); // header length: TAGX follows
    put32(&mut header, 24, 1); // one data record
    put32(&mut header, 28, 65001);
    put32(&mut header, 36, entries.len() as u32); // total entries
    put32(&mut header, 52, cncx.iter().len() as u32); // cncx records
    header.extend_from_slice(b"TAGX");
    header.extend_from_slice(&(12 + tagx.len() as u32 * 4).to_be_bytes());
    header.extend_from_slice(&1u32.to_be_bytes()); // one control byte
    for &(tag, values, mask, end) in tagx {
        header.extend_from_slice(&[tag, values, mask, end]);
    }

    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for entry in entries {
        offsets.push(56 + body.len());
        body.push(entry.name.len() as u8);
        body.extend_from_slice(&entry.name);
        let mut control = 0u8;
        for &(tag, _, mask, end) in tagx {
            if end == 0 && entry.tags.iter().any(|(t, _)| *t == tag) {
                control |= mask;
            }
        }
        body.push(control);
        for &(tag, _, _, end) in tagx {
            if end != 0 {
                continue;
            }
            if let Some((_, values)) = entry.tags.iter().find(|(t, _)| *t == tag) {
                for &value in values {
                    body.extend_from_slice(&varint(value));
                }
            }
        }
    }
    let idxt_at = 56 + body.len();
    let mut data = vec![0u8; 56];
    data[0..4].copy_from_slice(b"INDX");
    put32(&mut data, 4, 56);
    put32(&mut data, 20, idxt_at as u32);
    put32(&mut data, 24, entries.len() as u32);
    data.extend_from_slice(&body);
    data.extend_from_slice(b"IDXT");
    for offset in offsets {
        data.extend_from_slice(&(offset as u16).to_be_bytes());
    }

    let mut out = vec![header, data];
    out.extend(cncx);
    out
}

/// A CNCX record from strings, returning the record and each string's
/// offset for tag values.
pub fn cncx(strings: &[&str]) -> (Vec<u8>, Vec<u64>) {
    let mut record = Vec::new();
    let mut offsets = Vec::new();
    for text in strings {
        offsets.push(record.len() as u64);
        record.extend_from_slice(&varint(text.len() as u64));
        record.extend_from_slice(text.as_bytes());
    }
    (record, offsets)
}

/// The FDST record over flow boundaries.
pub fn fdst(bounds: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"FDST");
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(&(bounds.len() as u32).to_be_bytes());
    for &(start, end) in bounds {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
    }
    out
}

/// One skeleton for the KF8 composite: its text and the fragments that
/// stitch into it, each with its position in the growing skeleton.
pub struct Skeleton {
    pub text: &'static str,
    pub fragments: Vec<(usize, &'static str)>,
}

/// A whole KF8 book: flow 0 laid out as skeletons with their fragments
/// behind them, a CSS flow after it, the FDST, and the skeleton and
/// fragment indexes with their CNCX of aid names.
pub fn kf8_book(skeletons: &[Skeleton], css: &str) -> BookBuilder {
    let mut flow0 = Vec::new();
    let mut skel_entries = Vec::new();
    let mut frag_entries = Vec::new();
    let mut aids = Vec::new();
    for (index, skeleton) in skeletons.iter().enumerate() {
        let skelpos = flow0.len();
        flow0.extend_from_slice(skeleton.text.as_bytes());
        for (fragment, _) in skeleton.fragments.iter().zip(0u64..) {
            flow0.extend_from_slice(fragment.1.as_bytes());
            aids.push(format!("aid-{index}-{}", fragment.0));
        }
        skel_entries.push((skelpos, skeleton.text.len(), index));
    }
    let aid_refs: Vec<&str> = aids.iter().map(String::as_str).collect();
    let (cncx_record, offsets) = cncx(&aid_refs);
    let mut aid = 0usize;
    let mut seq = 0u64;
    for (index, skeleton) in skeletons.iter().enumerate() {
        let (skelpos, _, _) = skel_entries[index];
        for &(insert, fragment) in &skeleton.fragments {
            frag_entries.push(IndxEntry {
                name: (skelpos + insert).to_string().into_bytes(),
                tags: vec![
                    (2, vec![offsets[aid]]),
                    (3, vec![index as u64]),
                    (4, vec![seq]),
                    (6, vec![0, fragment.len() as u64]),
                ],
            });
            aid += 1;
            seq += 1;
        }
    }
    let skel_indx = indx_records(
        &[(1, 1, 0x01, 0), (6, 2, 0x02, 0), (0, 0, 0, 1)],
        &skel_entries
            .iter()
            .map(|&(pos, len, index)| IndxEntry {
                name: format!("SKEL{index:010}").into_bytes(),
                tags: vec![
                    (1, vec![skeletons[index].fragments.len() as u64]),
                    (6, vec![pos as u64, len as u64]),
                ],
            })
            .collect::<Vec<_>>(),
        None,
    );
    let frag_indx = indx_records(
        &[
            (2, 1, 0x01, 0),
            (3, 1, 0x02, 0),
            (4, 1, 0x04, 0),
            (6, 2, 0x08, 0),
            (0, 0, 0, 1),
        ],
        &frag_entries,
        Some(cncx_record),
    );

    let flow0_len = flow0.len() as u32;
    let mut rawml = flow0;
    rawml.extend_from_slice(css.as_bytes());
    let rawml_len = rawml.len() as u32;

    let mut builder = book("");
    builder.text = rawml;
    builder.version = 8;
    builder.declared_text_length = Some(flow0_len);
    let base = builder.extra_base();
    builder.skelidx = base;
    builder.fragidx = base + skel_indx.len() as u32;
    let fdst_index = builder.fragidx + frag_indx.len() as u32;
    builder.fdst = (fdst_index, 2);
    builder.extra_records = skel_indx;
    builder.extra_records.extend(frag_indx);
    builder
        .extra_records
        .push(fdst(&[(0, flow0_len), (flow0_len, rawml_len)]));
    builder
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
