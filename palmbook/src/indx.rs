//! The INDX machinery KF8 tables ride: a header record with its TAGX
//! tag table, data records with IDXT entry offsets, and CNCX string
//! records. Entries decode into tag-to-values maps through control
//! bytes and forward variable-width values.

use crate::{be16, be32, Book, Error};

/// One index entry: its name and the decoded tag values.
pub struct Entry {
    pub name: Vec<u8>,
    tags: Vec<(u8, Vec<u64>)>,
}

impl Entry {
    pub fn tag(&self, tag: u8) -> Option<&[u64]> {
        self.tags
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_slice())
    }

    pub fn first(&self, tag: u8) -> Option<u64> {
        self.tag(tag).and_then(|values| values.first().copied())
    }
}

/// A whole index: the entries in order and the CNCX text they point at.
pub struct Index {
    pub entries: Vec<Entry>,
    cncx: Vec<u8>,
}

impl Index {
    /// Reads the index whose header record sits at `at` in the book.
    pub fn read(book: &Book, at: usize) -> Result<Index, Error> {
        let header = book.record(at)?;
        if header.get(..4) != Some(b"INDX") {
            return Err(Error::Corrupt("no INDX magic"));
        }
        let header_len = be32(header, 4)? as usize;
        let data_records = be32(header, 24)? as usize;
        let cncx_records = be32(header, 52)? as usize;

        // TAGX follows the header: the tag table entries and how many
        // control bytes each entry carries.
        if header.get(header_len..header_len + 4) != Some(b"TAGX") {
            return Err(Error::Corrupt("no TAGX after the INDX header"));
        }
        let tagx_len = be32(header, header_len + 4)? as usize;
        let control_bytes = be32(header, header_len + 8)? as usize;
        let mut tagx = Vec::new();
        let table = header
            .get(header_len + 12..header_len + tagx_len)
            .ok_or(Error::Truncated)?;
        for quad in table.chunks_exact(4) {
            tagx.push((quad[0], quad[1], quad[2], quad[3]));
        }

        let mut entries = Vec::new();
        for data_index in 1..=data_records {
            let data = book.record(at + data_index)?;
            if data.get(..4) != Some(b"INDX") {
                return Err(Error::Corrupt("no INDX magic on a data record"));
            }
            let idxt_at = be32(data, 20)? as usize;
            let count = be32(data, 24)? as usize;
            if data.get(idxt_at..idxt_at + 4) != Some(b"IDXT") {
                return Err(Error::Corrupt("no IDXT where the header points"));
            }
            let mut offsets = Vec::with_capacity(count + 1);
            for index in 0..count {
                offsets.push(be16(data, idxt_at + 4 + index * 2)? as usize);
            }
            offsets.push(idxt_at);
            for pair in offsets.windows(2) {
                let entry = data.get(pair[0]..pair[1]).ok_or(Error::Truncated)?;
                entries.push(read_entry(entry, &tagx, control_bytes)?);
            }
        }

        let mut cncx = Vec::new();
        for index in 0..cncx_records {
            cncx.extend_from_slice(book.record(at + 1 + data_records + index)?);
        }
        Ok(Index { entries, cncx })
    }

    /// The CNCX string at a tag-carried offset.
    pub fn text(&self, offset: u64) -> Option<String> {
        let at = offset as usize;
        let (length, consumed) = varint(self.cncx.get(at..)?)?;
        let bytes = self
            .cncx
            .get(at + consumed..at + consumed + length as usize)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// One entry: length-prefixed name, the control bytes, then the values
/// of every tag the control bits mark present, in TAGX order.
fn read_entry(
    entry: &[u8],
    tagx: &[(u8, u8, u8, u8)],
    control_bytes: usize,
) -> Result<Entry, Error> {
    let name_len = *entry.first().ok_or(Error::Truncated)? as usize;
    let name = entry.get(1..1 + name_len).ok_or(Error::Truncated)?.to_vec();
    let controls = entry
        .get(1 + name_len..1 + name_len + control_bytes)
        .ok_or(Error::Truncated)?;
    let mut cursor = 1 + name_len + control_bytes;

    // First pass: which tags are present and how many values each has.
    // A fully-set multi-bit mask means the count arrives as a byte
    // total instead; single-bit masks mean one entry.
    let mut plan: Vec<(u8, Option<u64>, Option<u64>, u8)> = Vec::new();
    let mut control_index = 0usize;
    for &(tag, values_per, mask, end) in tagx {
        if end & 1 != 0 {
            control_index += 1;
            continue;
        }
        let control = *controls.get(control_index).ok_or(Error::Truncated)?;
        let value = control & mask;
        if value == 0 {
            continue;
        }
        if value == mask {
            if mask.count_ones() > 1 {
                let (total, consumed) =
                    varint(entry.get(cursor..).ok_or(Error::Truncated)?).ok_or(Error::Truncated)?;
                cursor += consumed;
                plan.push((tag, None, Some(total), values_per));
            } else {
                plan.push((tag, Some(1), None, values_per));
            }
        } else {
            let mut mask = mask;
            let mut value = value;
            while mask & 1 == 0 {
                mask >>= 1;
                value >>= 1;
            }
            plan.push((tag, Some(value as u64), None, values_per));
        }
    }

    let mut tags = Vec::with_capacity(plan.len());
    for (tag, count, byte_total, values_per) in plan {
        let mut values = Vec::new();
        match (count, byte_total) {
            (Some(count), _) => {
                for _ in 0..count * values_per as u64 {
                    let (value, consumed) = varint(entry.get(cursor..).ok_or(Error::Truncated)?)
                        .ok_or(Error::Truncated)?;
                    cursor += consumed;
                    values.push(value);
                }
            }
            (None, Some(total)) => {
                let mut used = 0u64;
                while used < total {
                    let (value, consumed) = varint(entry.get(cursor..).ok_or(Error::Truncated)?)
                        .ok_or(Error::Truncated)?;
                    cursor += consumed;
                    used += consumed as u64;
                    values.push(value);
                }
            }
            (None, None) => {}
        }
        tags.push((tag, values));
    }
    Ok(Entry { name, tags })
}

/// A forward variable-width value: seven bits per byte, the final byte
/// flagged with 0x80. Returns the value and the bytes consumed.
fn varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, &byte) in bytes.iter().take(10).enumerate() {
        value = (value << 7) | (byte & 0x7F) as u64;
        if byte & 0x80 != 0 {
            return Some((value, index + 1));
        }
    }
    None
}
