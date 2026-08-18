//! Comic book archives: a zip (CBZ) of page images in reading order.
//! Every page becomes one image block; the encoded sources land in the
//! media cache with header-probed dimensions, and pixels decode on
//! demand as the viewport reaches them, so open never decodes a page.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use crate::doc::epub::TocEntry;
use crate::doc::images::{self, BookSource, SourceEntry};
use crate::doc::model::{Block, BlockKind, Document};

/// Declared uncompressed size past this refuses an archive.
const CEILING: u64 = 1 << 30;

/// Why a comic archive cannot open, in plain sentences.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    NotComic,
    NoPages,
    Encrypted,
    TooLarge,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::NotComic => "This file is not a readable comic book archive.",
            Refusal::NoPages => "This comic archive holds no page images.",
            Refusal::Encrypted => "This comic archive is encrypted and cannot be opened.",
            Refusal::TooLarge => "This book is too large to open.",
        })
    }
}

impl std::error::Error for Refusal {}

/// The pages' encoded sources, riding the book-job surface to the media
/// cache; a comic streams nothing, so the job never runs.
pub struct Job {
    sources: Vec<SourceEntry>,
}

impl Job {
    pub fn has_chapters(&self) -> bool {
        false
    }

    pub fn take_sources(&mut self) -> Vec<SourceEntry> {
        std::mem::take(&mut self.sources)
    }
}

/// Whether an entry name reads as a page image.
fn is_page(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["jpg", "jpeg", "png", "gif", "webp"].iter().any(|ext| {
        lower
            .strip_suffix(ext)
            .is_some_and(|stem| stem.ends_with('.'))
    })
}

/// Orders entry names as a person reads them: digit runs compare by
/// value, so `page2` precedes `page10`; letters fold case. Ties on
/// value break on the raw bytes so distinct names never read equal.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (ca, cb) = (a[i], b[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let run = |s: &[u8], from: usize| {
                let mut end = from;
                while end < s.len() && s[end].is_ascii_digit() {
                    end += 1;
                }
                end
            };
            let (ea, eb) = (run(a, i), run(b, j));
            fn trim(s: &[u8]) -> &[u8] {
                let mut k = 0;
                while k + 1 < s.len() && s[k] == b'0' {
                    k += 1;
                }
                &s[k..]
            }
            let (va, vb) = (trim(&a[i..ea]), trim(&b[j..eb]));
            let by_value = va.len().cmp(&vb.len()).then_with(|| va.cmp(vb));
            if by_value != std::cmp::Ordering::Equal {
                return by_value;
            }
            (i, j) = (ea, eb);
        } else {
            let fold = |c: u8| c.to_ascii_lowercase();
            match fold(ca).cmp(&fold(cb)) {
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                other => return other,
            }
        }
    }
    (a.len() - i).cmp(&(b.len() - j)).then_with(|| a.cmp(b))
}

/// Opens a comic whole: the archive is walked once, page entries sort
/// by name, and every page's header gives its dimensions. The job
/// carries the encoded sources and nothing else.
pub fn open_prefix(
    bytes: Vec<u8>,
    name: &str,
) -> anyhow::Result<(Document, Vec<TocEntry>, Option<Job>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Refusal::NotComic)?;
    let mut entries: Vec<(usize, String)> = Vec::new();
    let mut declared = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|_| Refusal::NotComic)?;
        if !is_page(entry.name()) {
            continue;
        }
        if entry.encrypted() {
            return Err(Refusal::Encrypted.into());
        }
        declared = declared.saturating_add(entry.size());
        entries.push((index, entry.name().to_string()));
    }
    if declared > CEILING {
        return Err(Refusal::TooLarge.into());
    }
    if entries.is_empty() {
        return Err(Refusal::NoPages.into());
    }
    entries.sort_by(|(_, a), (_, b)| natural_cmp(a, b));

    let mut source = String::new();
    let mut blocks = Vec::new();
    let mut anchors = HashMap::new();
    let mut toc = Vec::new();
    let mut sources = Vec::new();
    for (position, (index, _)) in entries.iter().enumerate() {
        let number = position + 1;
        let key = format!("page{number}");
        let label = format!("Page {number}");
        let start = source.len();
        source.push_str(&label);
        let range = start..source.len();
        source.push('\n');
        anchors.insert(key.clone(), start);
        toc.push(TocEntry {
            label: label.clone(),
            depth: 0,
            path: key.clone(),
            fragment: None,
        });
        let mut block = Block::plain(BlockKind::Image {
            path: key.clone(),
            alt: label,
        });
        block.range = range;
        blocks.push(block);
        // Raw reads keep open cheap: a stored page is a copy, a deflated
        // page stays compressed until its decode; only the header probe
        // inflates, and only a window.
        let mut entry = zip.by_index_raw(*index).map_err(|_| Refusal::NotComic)?;
        let method = entry.compression();
        let mut raw = Vec::with_capacity(entry.compressed_size() as usize);
        entry.read_to_end(&mut raw).map_err(|_| Refusal::NotComic)?;
        let page = match method {
            zip::CompressionMethod::Stored => BookSource::Raster(raw),
            zip::CompressionMethod::Deflated => BookSource::Deflated(raw),
            _ => return Err(Refusal::NotComic.into()),
        };
        let dims = images::probe_source(&page);
        sources.push((key, page, dims));
    }

    let document = Document {
        blocks,
        source: Arc::from(source.as_str()),
        title: Some(name.to_string()),
        anchors,
        comic_file: true,
        ..Document::default()
    };
    Ok((document, toc, Some(Job { sources })))
}

/// The whole book at once, for tests: the document, the outline, and
/// the page sources the job would hand the media cache.
pub struct Book {
    pub document: Document,
    pub toc: Vec<TocEntry>,
    pub pages: Vec<SourceEntry>,
}

pub fn open_book(bytes: Vec<u8>, name: &str) -> anyhow::Result<Book> {
    let (document, toc, job) = open_prefix(bytes, name)?;
    let pages = job.map(|mut job| job.take_sources()).unwrap_or_default();
    Ok(Book {
        document,
        toc,
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::natural_cmp;
    use std::cmp::Ordering;

    #[test]
    fn natural_order_reads_like_a_person() {
        assert_eq!(natural_cmp("page2.jpg", "page10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("page10.jpg", "page2.jpg"), Ordering::Greater);
        assert_eq!(natural_cmp("Page5.jpg", "page5.png"), Ordering::Less);
        assert_eq!(natural_cmp("a/002.jpg", "b/001.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("p01.jpg", "p1.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("p1.jpg", "p1.jpg"), Ordering::Equal);
    }
}
