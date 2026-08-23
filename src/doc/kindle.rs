//! Kindle books: palmbook opens the container, KF8 parts or the MOBI6
//! flow become XHTML chapters, and everything walks through the same
//! pipeline EPUB chapters do. Links rewrite to chapter anchors, images
//! resolve by record reference, and DRM refuses before any content.
//!
//! Opening reads only the tables and one link scan; each chapter's
//! anchor injection, decoding and rewriting runs as it walks, so a big
//! book pays for its prefix, not for every chapter.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::doc::epub::{Book, Refusal, TocEntry};
use crate::doc::images::{BookSource, SourceEntry};
use crate::doc::model::Document;

/// One KF8 part awaiting its walk: the stitched bytes and the anchors
/// to inject, in descending offset order.
struct Pending {
    body: Vec<u8>,
    injections: Vec<(usize, usize)>,
}

/// The chapters before rendering: KF8 parts, or the MOBI6 flow with its
/// split points and anchor targets.
enum Content {
    Kf8 {
        parts: Vec<Pending>,
        /// Fragment number to its chapter path, for link rewriting.
        frag_paths: HashMap<usize, String>,
    },
    Mobi6 {
        rawml: Vec<u8>,
        starts: Vec<usize>,
        /// Anchor targets, ascending.
        targets: Vec<usize>,
    },
}

/// The book past the prefix: the remaining chapters, the walker
/// mid-book, and the container bytes the image records still live in.
/// The buffer dies with the job when the walk completes.
pub struct Job {
    bytes: Vec<u8>,
    content: Content,
    paths: Vec<String>,
    encoding: palmbook::TextEncoding,
    /// Image number to its byte range in the container.
    resources: HashMap<usize, (usize, usize)>,
    next: usize,
    walker: crate::doc::html::Walker,
    sources: Vec<SourceEntry>,
    handed: HashSet<String>,
    title: Option<String>,
    book_id: Option<String>,
}

impl Job {
    pub fn has_chapters(&self) -> bool {
        self.next < self.paths.len()
    }

    pub fn take_sources(&mut self) -> Vec<SourceEntry> {
        std::mem::take(&mut self.sources)
    }

    /// One chapter rendered to XHTML: anchors injected on the bytes,
    /// the declared encoding decoded, references rewritten.
    fn render(&mut self, position: usize) -> String {
        match &mut self.content {
            Content::Kf8 { parts, frag_paths } => {
                let pending = &mut parts[position];
                let mut body = std::mem::take(&mut pending.body);
                for &(offset, fid) in &pending.injections {
                    let at = anchor_point(&body, offset);
                    let anchor = format!("<a id=\"fid{fid}\"></a>");
                    body.splice(at..at, anchor.bytes());
                }
                let text = palmbook::decode(&body, self.encoding);
                let text = rewrite_pos_links(&text, |fid| {
                    frag_paths.get(&fid).map(|path| format!("{path}#fid{fid}"))
                });
                rewrite_embeds(&text)
            }
            Content::Mobi6 {
                rawml,
                starts,
                targets,
            } => {
                let start = starts[position];
                let end = starts.get(position + 1).copied().unwrap_or(rawml.len());
                let mut body = rawml[start..end].to_vec();
                let here = targets.partition_point(|&pos| pos < start)
                    ..targets.partition_point(|&pos| pos < end);
                for &pos in targets[here].iter().rev() {
                    let at = anchor_point(&body, pos - start);
                    let anchor = format!("<a id=\"fp{pos}\"></a>");
                    body.splice(at..at, anchor.bytes());
                }
                let text = palmbook::decode(&body, self.encoding);
                let text = rewrite_filepos(&text, |pos| {
                    format!("m6c{}#fp{pos}", chapter_of(starts, pos))
                });
                rewrite_recindex(&text)
            }
        }
    }

    fn step(&mut self) {
        let position = self.next;
        self.next += 1;
        if position > 0 {
            self.walker.chapter_break(position);
        }
        let xhtml = self.render(position);
        self.walker.set_chapter(&self.paths[position]);
        self.walker.walk_chapter(&xhtml);
        for src in self.walker.take_images() {
            if !self.handed.insert(src.clone()) {
                continue;
            }
            let range = src
                .strip_prefix("res")
                .and_then(|n| n.parse::<usize>().ok())
                .and_then(|n| self.resources.get(&n));
            if let Some(&(start, end)) = range {
                let source = BookSource::Raster(self.bytes[start..end].to_vec());
                let dims = crate::doc::images::probe_source(&source);
                self.sources.push((src, source, dims));
            }
        }
        self.walker.take_svgs();
    }
}

/// Opens a book to its prefix: whole chapters until the parse-prefix
/// target is crossed, no image decoded. The continuation carries the
/// rest; None means a small book with no images at all.
pub fn open_prefix(bytes: Vec<u8>) -> anyhow::Result<(Document, Vec<TocEntry>, Option<Job>)> {
    let prepared = {
        let book = open_palm(&bytes)?;
        prepare(&book, &bytes)?
    };
    let Prepared {
        content,
        paths,
        toc,
        resources,
        css,
        encoding,
        title,
        book_id,
    } = prepared;

    let mut table = crate::doc::html::EmphasisTable::default();
    for sheet in &css {
        table.add_css(sheet);
    }
    let mut walker = crate::doc::html::Walker::new();
    walker.set_emphasis(table);
    walker.set_book_files(paths.iter().cloned().collect());
    let mut job = Job {
        bytes,
        content,
        paths,
        encoding,
        resources,
        next: 0,
        walker,
        sources: Vec::new(),
        handed: HashSet::new(),
        title,
        book_id,
    };
    while job.has_chapters() && job.walker.source_len() < crate::doc::stream::PREFIX_TARGET {
        job.step();
    }
    let (blocks, source, details) = job.walker.snapshot();
    let document = Document {
        blocks,
        source: Arc::from(source),
        details,
        title: job.title.clone(),
        anchors: job.walker.anchors().iter().cloned().collect(),
        book_id: job.book_id.clone(),
        code_file: false,
        plain_file: false,
        comic_file: false,
    };
    let job = (job.has_chapters() || !job.sources.is_empty()).then_some(job);
    Ok((document, toc, job))
}

/// Continues the book on the parse worker, the EPUB manner.
pub fn run(
    mut job: Job,
    bail: &dyn Fn() -> bool,
    sources: crate::doc::images::SourceSink,
) -> Option<crate::doc::stream::Delivered> {
    sources(job.take_sources());
    while job.has_chapters() {
        if bail() {
            return None;
        }
        job.step();
        sources(job.take_sources());
    }
    let Job { walker, .. } = job;
    let anchors = walker.anchors().to_vec();
    let (blocks, source, details) = walker.finish();
    Some(crate::doc::stream::Delivered {
        blocks,
        details,
        source: Some(Arc::from(source)),
        anchors,
    })
}

/// The whole book, synchronously: the tests' view of the same pipeline
/// the app streams through.
pub fn open_book(bytes: Vec<u8>) -> anyhow::Result<Book> {
    let (document, toc, job) = open_prefix(bytes)?;
    let Some(mut job) = job else {
        return Ok(Book {
            document,
            images: Vec::new(),
            toc,
        });
    };
    while job.has_chapters() {
        job.step();
    }
    let image_sources = job.take_sources();
    let title = job.title.clone();
    let book_id = job.book_id.clone();
    let Job { walker, .. } = job;
    let anchors = walker.anchors().iter().cloned().collect();
    let (blocks, source, details) = walker.finish();
    let images = image_sources
        .into_iter()
        .filter_map(|(key, source, _)| {
            crate::doc::images::decode_source(&source).map(|image| (key, image))
        })
        .collect();
    Ok(Book {
        document: Document {
            blocks,
            source: Arc::from(source),
            details,
            title,
            anchors,
            book_id,
            code_file: false,
            plain_file: false,
            comic_file: false,
        },
        images,
        toc,
    })
}

/// Opens the container and picks the KF8 half of a dual file.
fn open_palm(bytes: &[u8]) -> anyhow::Result<palmbook::Book<'_>> {
    let book = palmbook::Book::open(bytes).map_err(refuse)?;
    if book.version() >= 8 {
        return Ok(book);
    }
    if let Some(boundary) = book.kf8_boundary() {
        if let Ok(inner) = palmbook::Book::open_at(bytes, boundary) {
            if inner.version() >= 8 {
                return Ok(inner);
            }
        }
    }
    Ok(book)
}

fn refuse(error: palmbook::Error) -> anyhow::Error {
    match error {
        palmbook::Error::Drm => Refusal::Drm.into(),
        palmbook::Error::Truncated | palmbook::Error::Corrupt(_) => {
            anyhow::anyhow!("This Kindle book is damaged and cannot be opened.")
        }
        palmbook::Error::NotPalm => {
            anyhow::anyhow!("This file is not a readable Kindle book.")
        }
    }
}

struct Prepared {
    content: Content,
    paths: Vec<String>,
    toc: Vec<TocEntry>,
    resources: HashMap<usize, (usize, usize)>,
    css: Vec<String>,
    encoding: palmbook::TextEncoding,
    title: Option<String>,
    book_id: Option<String>,
}

fn prepare(book: &palmbook::Book, bytes: &[u8]) -> anyhow::Result<Prepared> {
    let title = book.title();
    let book_id = book.exth_string(113).or_else(|| book.exth_string(104));
    let (content, paths, toc, css) = if book.version() >= 8 {
        prepare_kf8(book)?
    } else {
        prepare_mobi6(book)?
    };
    // Every record from the first image on is addressable by number;
    // only the ones chapters reference are ever copied out.
    let mut resources = HashMap::new();
    if let Some(first) = book.first_image() {
        for number in 1..=book.record_count().saturating_sub(first) {
            if let Ok(record) = book.record(first + number - 1) {
                let start = record.as_ptr() as usize - bytes.as_ptr() as usize;
                resources.insert(number, (start, start + record.len()));
            }
        }
    }
    Ok(Prepared {
        content,
        paths,
        toc,
        resources,
        css,
        encoding: book.encoding(),
        title,
        book_id,
    })
}

type Shaped = (Content, Vec<String>, Vec<TocEntry>, Vec<String>);

/// KF8: reassembled parts become chapters. Anchor targets come from the
/// `kindle:pos` links across the parts and the outline; injection and
/// rewriting wait for each chapter's walk.
fn prepare_kf8(book: &palmbook::Book) -> anyhow::Result<Shaped> {
    let kf8 = palmbook::kf8::read(book).map_err(refuse)?;
    let path_of = |part: usize| format!("k8p{part}");

    let mut targets: HashSet<usize> = HashSet::new();
    for part in &kf8.parts {
        collect_pos_fids(&part.body, &mut targets);
    }
    for point in &kf8.toc {
        if let Some((fid, _)) = point.target {
            targets.insert(fid as usize);
        }
    }

    let mut injections: Vec<Vec<(usize, usize)>> = vec![Vec::new(); kf8.parts.len()];
    for &fid in &targets {
        if let Some(fragment) = kf8.fragments.get(fid) {
            injections[fragment.part].push((fragment.offset, fid));
        }
    }
    let frag_paths: HashMap<usize, String> = kf8
        .fragments
        .iter()
        .enumerate()
        .map(|(fid, fragment)| (fid, path_of(fragment.part)))
        .collect();

    let paths: Vec<String> = (0..kf8.parts.len()).map(path_of).collect();
    let parts: Vec<Pending> = kf8
        .parts
        .into_iter()
        .zip(injections)
        .map(|(part, mut injections)| {
            injections.sort_by_key(|&(offset, _)| std::cmp::Reverse(offset));
            Pending {
                body: part.body,
                injections,
            }
        })
        .collect();

    let toc = kf8
        .toc
        .iter()
        .filter_map(|point| {
            let (fid, _) = point.target?;
            let path = frag_paths.get(&(fid as usize))?.clone();
            Some(TocEntry {
                label: point.label.clone(),
                depth: point.depth,
                path,
                fragment: Some(format!("fid{fid}")),
            })
        })
        .collect();

    let css = kf8
        .flows
        .iter()
        .skip(1)
        .filter_map(|flow| {
            let text = std::str::from_utf8(flow).ok()?;
            (text.contains('{') && !text.trim_start().starts_with('<')).then(|| text.to_string())
        })
        .collect();
    Ok((Content::Kf8 { parts, frag_paths }, paths, toc, css))
}

/// MOBI6: the flow splits at pagebreaks into chapters. `filepos`
/// targets and the NCX feed the anchor set; splitting, injection and
/// rewriting wait for each chapter's walk.
fn prepare_mobi6(book: &palmbook::Book) -> anyhow::Result<Shaped> {
    let rawml = book.rawml().map_err(refuse)?;
    let starts = pagebreak_starts(&rawml);
    let path_of = |chapter: usize| format!("m6c{chapter}");

    let mut targets = filepos_targets(&rawml);
    let mut toc = Vec::new();
    if let Some(at) = book.mobi6_ncx() {
        if let Ok(index) = palmbook::indx::Index::read(book, at) {
            let mut depths: Vec<u8> = Vec::new();
            for (position, entry) in index.entries.iter().enumerate() {
                let depth = entry
                    .first(21)
                    .map(|parent| parent as usize)
                    .filter(|&parent| parent < position)
                    .map(|parent| depths[parent].saturating_add(1))
                    .unwrap_or(0);
                depths.push(depth);
                let label = entry
                    .first(3)
                    .and_then(|offset| index.text(offset))
                    .unwrap_or_default();
                let Some(pos) = entry.first(1).map(|pos| pos as usize) else {
                    continue;
                };
                if label.is_empty() || pos >= rawml.len() {
                    continue;
                }
                targets.insert(pos);
                toc.push(TocEntry {
                    label,
                    depth,
                    path: path_of(chapter_of(&starts, pos)),
                    fragment: Some(format!("fp{pos}")),
                });
            }
        }
    }

    let paths: Vec<String> = (0..starts.len()).map(path_of).collect();
    let mut targets: Vec<usize> = targets.into_iter().collect();
    targets.sort_unstable();
    Ok((
        Content::Mobi6 {
            rawml,
            starts,
            targets,
        },
        paths,
        toc,
        Vec::new(),
    ))
}

/// The chapter a byte position falls in.
fn chapter_of(starts: &[usize], pos: usize) -> usize {
    starts
        .partition_point(|&start| start <= pos)
        .saturating_sub(1)
}

/// Chapter boundaries: the flow's start and every pagebreak tag.
fn pagebreak_starts(rawml: &[u8]) -> Vec<usize> {
    let mut starts = vec![0usize];
    let needle = b"<mbp:pagebreak";
    let mut at = 0;
    while at + needle.len() <= rawml.len() {
        if rawml[at..at + needle.len()].eq_ignore_ascii_case(needle) {
            if at > 0 {
                starts.push(at);
            }
            at += needle.len();
        } else {
            at += 1;
        }
    }
    starts
}

/// Every byte offset a `filepos=` attribute points at.
fn filepos_targets(rawml: &[u8]) -> HashSet<usize> {
    let mut targets = HashSet::new();
    let needle = b"filepos=";
    let mut at = 0;
    while at + needle.len() <= rawml.len() {
        if &rawml[at..at + needle.len()] == needle {
            let mut cursor = at + needle.len();
            if rawml.get(cursor) == Some(&b'"') || rawml.get(cursor) == Some(&b'\'') {
                cursor += 1;
            }
            let digits_start = cursor;
            while rawml.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            if let Ok(pos) = std::str::from_utf8(&rawml[digits_start..cursor])
                .unwrap_or_default()
                .parse::<usize>()
            {
                if pos < rawml.len() {
                    targets.insert(pos);
                }
            }
            at = cursor;
        } else {
            at += 1;
        }
    }
    targets
}

/// Where an anchor may inject at or before `offset`: on the tag start
/// the position names, or back at the nearest tag boundary when it
/// points into text, so the anchor never lands inside a tag.
fn anchor_point(body: &[u8], offset: usize) -> usize {
    let mut at = offset.min(body.len());
    if body.get(at) == Some(&b'<') {
        return at;
    }
    while at > 0 {
        at -= 1;
        match body[at] {
            b'<' => return at,
            b'>' => return at + 1,
            _ => {}
        }
    }
    0
}

/// Base32 as Kindle writes it: digits then A through V.
fn base32(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let mut value = 0usize;
    for c in text.chars() {
        let digit = match c {
            '0'..='9' => c as usize - '0' as usize,
            'A'..='V' => c as usize - 'A' as usize + 10,
            'a'..='v' => c as usize - 'a' as usize + 10,
            _ => return None,
        };
        value = value.checked_mul(32)?.checked_add(digit)?;
    }
    Some(value)
}

fn is_base32(c: char) -> bool {
    c.is_ascii_digit() || ('A'..='V').contains(&c) || ('a'..='v').contains(&c)
}

/// The fragment numbers `kindle:pos` links point at, scanned over the
/// raw part bytes.
fn collect_pos_fids(body: &[u8], targets: &mut HashSet<usize>) {
    let needle = b"kindle:pos:fid:";
    let mut at = 0;
    while at + needle.len() <= body.len() {
        if &body[at..at + needle.len()] == needle {
            let mut cursor = at + needle.len();
            let digits_start = cursor;
            while body
                .get(cursor)
                .is_some_and(|&b| is_base32(b as char) && b.is_ascii())
            {
                cursor += 1;
            }
            if let Some(fid) = std::str::from_utf8(&body[digits_start..cursor])
                .ok()
                .and_then(base32)
            {
                targets.insert(fid);
            }
            at = cursor;
        } else {
            at += 1;
        }
    }
}

/// Rewrites `kindle:pos:fid:X:off:Y` references through the resolver;
/// an unresolvable fid keeps its text and loses the dead link.
fn rewrite_pos_links(text: &str, resolve: impl Fn(usize) -> Option<String>) -> String {
    let needle = "kindle:pos:fid:";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find(needle) {
        out.push_str(&rest[..found]);
        let after = &rest[found + needle.len()..];
        let fid_text: String = after.chars().take_while(|&c| is_base32(c)).collect();
        let mut consumed = needle.len() + fid_text.len();
        let tail = &after[fid_text.len()..];
        if let Some(off) = tail.strip_prefix(":off:") {
            let off_text: String = off.chars().take_while(|&c| is_base32(c)).collect();
            consumed += ":off:".len() + off_text.len();
        }
        match base32(&fid_text).and_then(&resolve) {
            Some(target) => out.push_str(&target),
            None => out.push_str(&rest[found..found + consumed]),
        }
        rest = &rest[found + consumed..];
    }
    out.push_str(rest);
    out
}

/// Rewrites `kindle:embed:NNNN[?mime=...]` references to resource keys.
fn rewrite_embeds(text: &str) -> String {
    let needle = "kindle:embed:";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find(needle) {
        out.push_str(&rest[..found]);
        let after = &rest[found + needle.len()..];
        let id: String = after.chars().take_while(|&c| is_base32(c)).collect();
        let mut consumed = needle.len() + id.len();
        let tail = &after[id.len()..];
        if let Some(query) = tail.strip_prefix('?') {
            let extent = query.find(['"', '\'']).unwrap_or(query.len());
            consumed += 1 + extent;
        }
        match base32(&id) {
            Some(number) if number > 0 => out.push_str(&format!("res{number}")),
            _ => out.push_str(&rest[found..found + consumed]),
        }
        rest = &rest[found + consumed..];
    }
    out.push_str(rest);
    out
}

/// Rewrites `filepos=NNN` attributes into chapter-anchor hrefs.
fn rewrite_filepos(text: &str, target: impl Fn(usize) -> String) -> String {
    let needle = "filepos=";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find(needle) {
        out.push_str(&rest[..found]);
        let after = &rest[found + needle.len()..];
        let quoted = after.starts_with('"') || after.starts_with('\'');
        let digits_at = usize::from(quoted);
        let digits: String = after[digits_at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let mut consumed = needle.len() + digits_at + digits.len();
        if quoted && after[digits_at + digits.len()..].starts_with(after.chars().next().unwrap()) {
            consumed += 1;
        }
        match digits.parse::<usize>() {
            Ok(pos) => out.push_str(&format!("href=\"{}\"", target(pos))),
            Err(_) => out.push_str(&rest[found..found + consumed]),
        }
        rest = &rest[found + consumed..];
    }
    out.push_str(rest);
    out
}

/// Rewrites `recindex="NNN"` image references to resource keys.
fn rewrite_recindex(text: &str) -> String {
    let needle = "recindex=";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find(needle) {
        out.push_str(&rest[..found]);
        let after = &rest[found + needle.len()..];
        let quoted = after.starts_with('"') || after.starts_with('\'');
        let digits_at = usize::from(quoted);
        let digits: String = after[digits_at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let mut consumed = needle.len() + digits_at + digits.len();
        if quoted && after[digits_at + digits.len()..].starts_with(after.chars().next().unwrap()) {
            consumed += 1;
        }
        match digits.parse::<usize>() {
            Ok(number) if number > 0 => out.push_str(&format!("src=\"res{number}\"")),
            _ => out.push_str(&rest[found..found + consumed]),
        }
        rest = &rest[found + consumed..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `?mime=` query is measured in bytes, so a multibyte character
    /// inside it leaves the text after the reference intact.
    #[test]
    fn an_embed_reference_with_a_multibyte_query_rewrites_whole() {
        assert_eq!(
            rewrite_embeds("<img src=\"kindle:embed:0001?mime=é\"/> après"),
            "<img src=\"res1\"/> après"
        );
        assert_eq!(
            rewrite_embeds("<img src=\"kindle:embed:0002?mime=image/jpeg\"/>"),
            "<img src=\"res2\"/>"
        );
        assert_eq!(
            rewrite_embeds("kindle:embed:0000?mime=x\" kept"),
            "kindle:embed:0000?mime=x\" kept",
            "a zero index is no resource and stays as written"
        );
    }
}
