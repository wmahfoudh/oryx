//! KF8: the flow table, the skeleton and fragment reassembly into whole
//! XHTML parts, and the NCX outline. Stitching is positional arithmetic
//! over the verified tables; a table that contradicts the text errors,
//! never mis-stitches silently.

use crate::indx::Index;
use crate::{Book, Error};

/// One reassembled XHTML part.
pub struct Part {
    pub name: String,
    pub body: Vec<u8>,
}

/// Where one fragment landed: its part, its byte range there, and the
/// aid the fragment table names it by. `kindle:pos:fid` links resolve
/// through this table by fragment index.
pub struct Fragment {
    pub part: usize,
    pub offset: usize,
    pub length: usize,
    pub aid: String,
}

/// One outline point: the label, its nesting depth, and the
/// fragment-and-offset target when the index carries one.
pub struct TocPoint {
    pub label: String,
    pub depth: u8,
    pub target: Option<(u32, u32)>,
}

pub struct Kf8 {
    pub parts: Vec<Part>,
    /// Flow 0 is emptied: its bytes move into the parts. The rest are
    /// stylesheets and inline resources the parts reference by number.
    pub flows: Vec<Vec<u8>>,
    pub fragments: Vec<Fragment>,
    pub toc: Vec<TocPoint>,
}

/// Reads the KF8 payload of an opened book. The book must be the KF8
/// half: version 8, opened at the boundary when the file is dual.
pub fn read(book: &Book) -> Result<Kf8, Error> {
    if book.version() < 8 {
        return Err(Error::Corrupt("not a KF8 book"));
    }
    let header = book.kf8_header().ok_or(Error::Corrupt("no KF8 header"))?;
    let rawml = book.rawml()?;

    let mut flows = read_flows(book, header.fdst, header.fdst_count, rawml)?;
    let flow0 = std::mem::take(&mut flows[0]);

    let none = 0xFFFF_FFFF;
    if header.skel == none || header.frag == none {
        return Err(Error::Corrupt("no skeleton tables"));
    }
    let skel = Index::read(book, header.skel as usize)?;
    let frag = Index::read(book, header.frag as usize)?;

    let mut parts = Vec::with_capacity(skel.entries.len());
    let mut fragments = Vec::new();
    let mut frag_cursor = 0usize;
    for (index, entry) in skel.entries.iter().enumerate() {
        let count = entry
            .first(1)
            .ok_or(Error::Corrupt("a skeleton without a fragment count"))?
            as usize;
        let geometry = entry
            .tag(6)
            .filter(|values| values.len() >= 2)
            .ok_or(Error::Corrupt("a skeleton without its geometry"))?;
        let (skelpos, skellen) = (geometry[0] as usize, geometry[1] as usize);
        if skelpos + skellen > flow0.len() {
            return Err(Error::Corrupt("a skeleton past the flow"));
        }
        let mut body = flow0[skelpos..skelpos + skellen].to_vec();
        let mut baseptr = skelpos + skellen;
        let mut placed: Vec<usize> = Vec::new();
        let mut file_number = None;
        for _ in 0..count {
            let entry = frag
                .entries
                .get(frag_cursor)
                .ok_or(Error::Corrupt("the fragment table ends early"))?;
            frag_cursor += 1;
            let insert = std::str::from_utf8(&entry.name)
                .ok()
                .and_then(|name| name.parse::<usize>().ok())
                .ok_or(Error::Corrupt("a fragment with no position"))?;
            let length = entry
                .tag(6)
                .filter(|values| values.len() >= 2)
                .ok_or(Error::Corrupt("a fragment without its geometry"))?[1]
                as usize;
            let aid = entry
                .first(2)
                .and_then(|offset| frag.text(offset))
                .unwrap_or_default();
            if file_number.is_none() {
                file_number = entry.first(3);
            }
            let insert = insert
                .checked_sub(skelpos)
                .filter(|&at| at <= body.len())
                .ok_or(Error::Corrupt("a fragment outside its skeleton"))?;
            if baseptr + length > flow0.len() {
                return Err(Error::Corrupt("a fragment past the flow"));
            }
            let slice = &flow0[baseptr..baseptr + length];
            for &earlier in &placed {
                let fragment: &mut Fragment = &mut fragments[earlier];
                if fragment.offset >= insert {
                    fragment.offset += length;
                }
            }
            placed.push(fragments.len());
            fragments.push(Fragment {
                part: index,
                offset: insert,
                length,
                aid,
            });
            body.splice(insert..insert, slice.iter().copied());
            baseptr += length;
        }
        let number = file_number.unwrap_or(index as u64);
        parts.push(Part {
            name: format!("part{number:04}.xhtml"),
            body,
        });
    }

    let toc = if header.ncx == none {
        Vec::new()
    } else {
        read_ncx(book, header.ncx as usize)?
    };
    Ok(Kf8 {
        parts,
        flows,
        fragments,
        toc,
    })
}

/// The FDST flow boundaries cut over the rawml; no table means one flow.
fn read_flows(book: &Book, fdst: u32, count: u32, rawml: Vec<u8>) -> Result<Vec<Vec<u8>>, Error> {
    if fdst == 0xFFFF_FFFF || count <= 1 {
        return Ok(vec![rawml]);
    }
    let record = book.record(fdst as usize)?;
    if record.get(..4) != Some(b"FDST") {
        return Err(Error::Corrupt("no FDST magic"));
    }
    let sections = crate::be32(record, 8)? as usize;
    let mut flows = Vec::with_capacity(sections);
    for index in 0..sections {
        let start = crate::be32(record, 12 + index * 8)? as usize;
        let end = crate::be32(record, 16 + index * 8)? as usize;
        if start > end || end > rawml.len() {
            return Err(Error::Corrupt("a flow past the text"));
        }
        flows.push(rawml[start..end].to_vec());
    }
    if flows.is_empty() {
        flows.push(rawml);
    }
    Ok(flows)
}

/// The NCX outline: labels from the CNCX, nesting from the parent tag,
/// targets from the fragment-and-offset pair.
fn read_ncx(book: &Book, at: usize) -> Result<Vec<TocPoint>, Error> {
    let index = Index::read(book, at)?;
    let mut depths: Vec<u8> = Vec::with_capacity(index.entries.len());
    let mut toc = Vec::with_capacity(index.entries.len());
    for (position, entry) in index.entries.iter().enumerate() {
        let label = entry
            .first(3)
            .and_then(|offset| index.text(offset))
            .unwrap_or_default();
        let depth = entry
            .first(21)
            .map(|parent| parent as usize)
            .filter(|&parent| parent < position)
            .map(|parent| depths[parent].saturating_add(1))
            .unwrap_or(0);
        depths.push(depth);
        let target = entry
            .tag(6)
            .filter(|values| values.len() >= 2)
            .map(|values| (values[0] as u32, values[1] as u32));
        if !label.is_empty() {
            toc.push(TocPoint {
                label,
                depth,
                target,
            });
        }
    }
    Ok(toc)
}
