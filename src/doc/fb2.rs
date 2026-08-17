//! FB2 reading: the FictionBook XML converts into small XHTML chapters
//! that walk through the same pipeline EPUB chapters do, one canonical
//! book path. Binaries decode from base64 into image sources; nothing
//! here knows about layout or paint.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use base64::Engine;

use crate::doc::epub::{Book, TocEntry};
use crate::doc::images::{BookSource, SourceEntry};
use crate::doc::model::Document;

/// Declared uncompressed size past this refuses a zip-wrapped book.
const CEILING: u64 = 1 << 30;

/// Why an FB2 file cannot open, in plain sentences.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    NotFb2,
    TooLarge,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::NotFb2 => "This file is not a readable FB2 book.",
            Refusal::TooLarge => "This book is too large to open.",
        })
    }
}

impl std::error::Error for Refusal {}

/// The book past the prefix: the remaining converted chapters, the
/// walker mid-book, and the binaries not yet handed to the store.
pub struct Job {
    chapters: Vec<(String, String)>,
    next: usize,
    walker: crate::doc::html::Walker,
    binaries: HashMap<String, BookSource>,
    sources: Vec<SourceEntry>,
    title: Option<String>,
    book_id: Option<String>,
}

impl Job {
    pub fn has_chapters(&self) -> bool {
        self.next < self.chapters.len()
    }

    /// Image sources with their header dimensions since the last take;
    /// sizes reach layout ahead of any pixel.
    pub fn take_sources(&mut self) -> Vec<SourceEntry> {
        std::mem::take(&mut self.sources)
    }

    /// Walks the next chapter and hands over the binaries it referenced.
    fn step(&mut self) {
        let position = self.next;
        self.next += 1;
        if position > 0 {
            self.walker.chapter_break(position);
        }
        let (path, xhtml) = &self.chapters[position];
        self.walker.set_chapter(path);
        self.walker.walk_chapter(xhtml);
        for src in self.walker.take_images() {
            if let Some(source) = self.binaries.remove(&src) {
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
    let bytes = unwrap_zip(bytes)?;
    let text = decode(&bytes);
    let tree = roxmltree::Document::parse_with_options(
        &text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|_| Refusal::NotFb2)?;
    let root = tree.root_element();
    if root.tag_name().name() != "FictionBook" {
        return Err(Refusal::NotFb2.into());
    }

    let title_info = root
        .descendants()
        .find(|n| n.tag_name().name() == "title-info");
    let child_text = |node: roxmltree::Node, name: &str| {
        node.children()
            .find(|n| n.tag_name().name() == name)
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };
    let title = title_info.and_then(|n| child_text(n, "book-title"));
    let book_id = root
        .descendants()
        .find(|n| n.tag_name().name() == "document-info")
        .and_then(|n| child_text(n, "id"));
    let cover = title_info
        .and_then(|n| n.children().find(|c| c.tag_name().name() == "coverpage"))
        .and_then(|c| c.children().find(|n| n.tag_name().name() == "image"))
        .and_then(href_key);

    let binaries = read_binaries(root);
    let plan = plan(root, cover.as_deref());
    if plan.chapters.is_empty() {
        return Err(Refusal::NotFb2.into());
    }

    let chapters: Vec<(String, String)> = plan
        .chapters
        .iter()
        .map(|chap| (chap.path.clone(), emit_chapter(chap, &plan.ids)))
        .collect();

    let mut walker = crate::doc::html::Walker::new();
    walker.set_book_files(chapters.iter().map(|(path, _)| path.clone()).collect());
    let mut job = Job {
        chapters,
        next: 0,
        walker,
        binaries,
        sources: Vec::new(),
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
    };
    let toc = plan.toc;
    let job = (job.has_chapters() || !job.sources.is_empty()).then_some(job);
    Ok((document, toc, job))
}

/// Continues the book on the parse worker, the EPUB manner: remaining
/// chapters walk and their image sources hand over as they appear.
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

/// The whole book, synchronously: the prefix, the remaining chapters,
/// and every image decoded in place. The tests' view of the same
/// pipeline the app streams through.
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
        },
        images,
        toc,
    })
}

/// A zip wrapper (`.fb2.zip`, `.fbz`) opens to its single FB2 entry;
/// plain bytes pass through.
fn unwrap_zip(bytes: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    if !bytes.starts_with(b"PK") {
        return Ok(bytes);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| Refusal::NotFb2)?;
    let mut name = None;
    for index in 0..zip.len() {
        let Ok(entry) = zip.by_index_raw(index) else {
            continue;
        };
        if entry.name().to_ascii_lowercase().ends_with(".fb2") {
            if entry.size() > CEILING {
                return Err(Refusal::TooLarge.into());
            }
            name = Some(entry.name().to_string());
            break;
        }
    }
    let name = name.ok_or(Refusal::NotFb2)?;
    let mut file = zip.by_name(&name).map_err(|_| Refusal::NotFb2)?;
    let mut inner = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut inner).map_err(|_| Refusal::NotFb2)?;
    Ok(inner)
}

/// Bytes to text: UTF-16 by BOM, then the declared encoding when it is
/// windows-1251, common in the wild; UTF-8 lossy otherwise.
fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] | [0xEF, 0xBB, 0xBF, ..] => {
            return crate::doc::epub::decode(bytes)
        }
        _ => {}
    }
    if let Some(encoding) = declared_encoding(bytes) {
        if encoding.eq_ignore_ascii_case("windows-1251") || encoding.eq_ignore_ascii_case("cp1251")
        {
            return decode_1251(bytes);
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// The `encoding` value of the XML declaration, read from the raw bytes
/// since the declaration itself is ASCII.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(200)];
    let head = std::str::from_utf8(&head[..head.iter().position(|&b| b == b'>')?])
        .ok()?
        .to_ascii_lowercase();
    let after = &head[head.find("encoding")? + "encoding".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &after[1..];
    Some(value[..value.find(quote)?].to_string())
}

/// Windows-1251 to text; the low half is ASCII and the table carries
/// the high half.
fn decode_1251(bytes: &[u8]) -> String {
    const HIGH: [char; 128] = [
        'Ђ', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', //
        'ђ', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '•', '–', '—', '\u{98}', '™', 'љ',
        '›', 'њ', 'ќ', 'ћ', 'џ', //
        '\u{A0}', 'Ў', 'ў', 'Ј', '¤', 'Ґ', '¦', '§', 'Ё', '©', 'Є', '«', '¬', '\u{AD}', '®',
        'Ї', //
        '°', '±', 'І', 'і', 'ґ', 'µ', '¶', '·', 'ё', '№', 'є', '»', 'ј', 'Ѕ', 'ѕ', 'ї', //
        'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ж', 'З', 'И', 'Й', 'К', 'Л', 'М', 'Н', 'О', 'П', //
        'Р', 'С', 'Т', 'У', 'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Щ', 'Ъ', 'Ы', 'Ь', 'Э', 'Ю', 'Я', //
        'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', //
        'р', 'с', 'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я',
    ];
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                HIGH[(b - 0x80) as usize]
            }
        })
        .collect()
}

/// The `<binary>` elements decoded from base64, keyed by id. An SVG
/// content type keeps its markup; everything else is raster bytes.
fn read_binaries(root: roxmltree::Node) -> HashMap<String, BookSource> {
    let engine = base64::engine::general_purpose::STANDARD;
    let mut out = HashMap::new();
    for node in root.children().filter(|n| n.tag_name().name() == "binary") {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let text: String = node
            .text()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        let Ok(bytes) = engine.decode(text) else {
            continue;
        };
        let svg = node
            .attribute("content-type")
            .is_some_and(|t| t.contains("svg"));
        let source = if svg {
            BookSource::Svg(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            BookSource::Raster(bytes)
        };
        out.insert(id.to_string(), source);
    }
    out
}

/// Whether a paragraph holds at least one image and nothing else but
/// whitespace.
fn images_only(node: roxmltree::Node) -> bool {
    let mut image = false;
    for child in node.children() {
        if child.is_element() {
            if child.tag_name().name() != "image" {
                return false;
            }
            image = true;
        } else if child.text().is_some_and(|t| !t.trim().is_empty()) {
            return false;
        }
    }
    image
}

/// The image reference an `<image>` element carries, without its `#`.
fn href_key(node: roxmltree::Node) -> Option<String> {
    node.attributes()
        .find(|a| a.name() == "href")
        .map(|a| a.value().trim_start_matches('#').to_string())
        .filter(|v| !v.is_empty())
}

/// One planned chapter: the front matter, a top-level section, or a
/// whole notes body kept off the outline.
enum Chapter<'a, 'input> {
    Front {
        title: Option<roxmltree::Node<'a, 'input>>,
        cover: Option<String>,
        epigraphs: Vec<roxmltree::Node<'a, 'input>>,
    },
    Section(roxmltree::Node<'a, 'input>),
    Notes(roxmltree::Node<'a, 'input>),
}

struct Planned<'a, 'input> {
    path: String,
    kind: Chapter<'a, 'input>,
}

struct Plan<'a, 'input> {
    chapters: Vec<Planned<'a, 'input>>,
    /// Element id to the chapter path holding it; links resolve here so
    /// a note reference targets the notes chapter, not its own.
    ids: HashMap<String, String>,
    toc: Vec<TocEntry>,
}

/// Whether a body holds notes: rendered at the end, off the outline.
fn is_notes(body: roxmltree::Node) -> bool {
    body.attribute("name")
        .is_some_and(|n| matches!(n, "notes" | "comments" | "footnotes"))
}

/// Walks the bodies once, assigning every chapter its path, every id
/// its chapter, and every titled section its outline entry.
fn plan<'a, 'input>(root: roxmltree::Node<'a, 'input>, cover: Option<&str>) -> Plan<'a, 'input> {
    let mut plan = Plan {
        chapters: Vec::new(),
        ids: HashMap::new(),
        toc: Vec::new(),
    };
    let mut synth = 0usize;
    for (b, body) in root
        .children()
        .filter(|n| n.tag_name().name() == "body")
        .enumerate()
    {
        let title = body.children().find(|n| n.tag_name().name() == "title");
        if is_notes(body) {
            let path = format!("fb2n{b}");
            map_ids(body, &path, &mut plan.ids);
            plan.chapters.push(Planned {
                path,
                kind: Chapter::Notes(body),
            });
            continue;
        }
        let epigraphs: Vec<_> = body
            .children()
            .filter(|n| n.tag_name().name() == "epigraph")
            .collect();
        let cover = (b == 0).then(|| cover.map(str::to_string)).flatten();
        if title.is_some() || cover.is_some() || !epigraphs.is_empty() {
            let path = format!("fb2f{b}");
            if let Some(title) = title {
                map_ids(title, &path, &mut plan.ids);
            }
            for epigraph in &epigraphs {
                map_ids(*epigraph, &path, &mut plan.ids);
            }
            plan.chapters.push(Planned {
                path,
                kind: Chapter::Front {
                    title,
                    cover,
                    epigraphs,
                },
            });
        }
        for section in body.children().filter(|n| n.tag_name().name() == "section") {
            let path = format!("fb2c{b}_{}", plan.chapters.len());
            map_ids(section, &path, &mut plan.ids);
            plan_outline(section, 0, &path, &mut plan.toc, &mut plan.ids, &mut synth);
            plan.chapters.push(Planned {
                path,
                kind: Chapter::Section(section),
            });
        }
    }
    plan
}

/// Every element id under the node maps to the chapter path.
fn map_ids(node: roxmltree::Node, path: &str, ids: &mut HashMap<String, String>) {
    for d in node.descendants() {
        if let Some(id) = d.attribute("id") {
            ids.entry(id.to_string())
                .or_insert_with(|| path.to_string());
        }
    }
}

/// Outline entries for a section and its nested sections. A nested
/// titled section without an id gets a synthesized one to anchor its
/// entry; `emit_section` derives the same name from the same counter
/// order.
fn plan_outline(
    section: roxmltree::Node,
    depth: u8,
    chapter: &str,
    toc: &mut Vec<TocEntry>,
    ids: &mut HashMap<String, String>,
    synth: &mut usize,
) {
    let label = section
        .children()
        .find(|n| n.tag_name().name() == "title")
        .map(title_text)
        .filter(|t| !t.is_empty());
    if let Some(label) = label {
        let fragment = if depth == 0 {
            None
        } else {
            Some(section_id(section, synth))
        };
        if let Some(id) = &fragment {
            ids.entry(id.clone()).or_insert_with(|| chapter.to_string());
        }
        toc.push(TocEntry {
            label,
            depth,
            path: chapter.to_string(),
            fragment,
        });
    }
    for child in section
        .children()
        .filter(|n| n.tag_name().name() == "section")
    {
        plan_outline(child, depth + 1, chapter, toc, ids, synth);
    }
}

/// The section's own id, or a synthesized stable one. The synthesis
/// counter advances only for id-less titled sections, in document
/// order, so planning and emission agree without sharing state.
fn section_id(section: roxmltree::Node, synth: &mut usize) -> String {
    match section.attribute("id") {
        Some(id) => id.to_string(),
        None => {
            *synth += 1;
            format!("oryxs{synth}")
        }
    }
}

/// A title's text flattened to one label line.
fn title_text(title: roxmltree::Node) -> String {
    let mut out = String::new();
    for d in title.descendants().filter(|n| n.is_text()) {
        out.push_str(d.text().unwrap_or_default());
        out.push(' ');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn esc(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// One chapter as the XHTML the walker takes.
fn emit_chapter(chapter: &Planned, ids: &HashMap<String, String>) -> String {
    let mut e = Emitter {
        out: String::from("<html><body>"),
        ids,
        synth: 0,
    };
    match &chapter.kind {
        Chapter::Front {
            title,
            cover,
            epigraphs,
        } => {
            if let Some(title) = title {
                e.heading(*title, 1, None);
            }
            if let Some(cover) = cover {
                e.out.push_str("<p align=\"center\"><img src=\"");
                esc(cover, &mut e.out);
                e.out.push_str("\"/></p>");
            }
            for epigraph in epigraphs {
                e.flow(*epigraph);
            }
        }
        Chapter::Section(section) => e.section(*section, 0),
        Chapter::Notes(body) => {
            if let Some(title) = body.children().find(|n| n.tag_name().name() == "title") {
                e.heading(title, 1, None);
            }
            for child in body.children().filter(|n| n.is_element()) {
                match child.tag_name().name() {
                    "title" => {}
                    "section" => e.section(child, 1),
                    _ => e.flow(child),
                }
            }
        }
    }
    e.out.push_str("</body></html>");
    e.out
}

struct Emitter<'a> {
    out: String,
    ids: &'a HashMap<String, String>,
    /// Mirrors the planning counter for id-less titled sections.
    synth: usize,
}

impl Emitter<'_> {
    /// A section: its anchor, its title as a heading by depth, then its
    /// content, nested sections included.
    fn section(&mut self, section: roxmltree::Node, depth: u8) {
        let title = section.children().find(|n| n.tag_name().name() == "title");
        let id = match section.attribute("id") {
            Some(id) => Some(id.to_string()),
            None if depth > 0 && title.map(title_text).is_some_and(|t| !t.is_empty()) => {
                self.synth += 1;
                Some(format!("oryxs{}", self.synth))
            }
            None => None,
        };
        if let Some(id) = &id {
            self.out.push_str("<div id=\"");
            esc(id, &mut self.out);
            self.out.push_str("\">");
        }
        if let Some(title) = title {
            self.heading(title, (1 + depth).min(6), None);
        }
        for child in section.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "title" => {}
                "section" => self.section(child, depth + 1),
                _ => self.flow(child),
            }
        }
        if id.is_some() {
            self.out.push_str("</div>");
        }
    }

    /// A title's paragraphs as one heading, `<br/>` between them.
    fn heading(&mut self, title: roxmltree::Node, level: u8, id: Option<&str>) {
        self.out.push_str(&format!("<h{level}"));
        if let Some(id) = id {
            self.out.push_str(" id=\"");
            esc(id, &mut self.out);
            self.out.push('"');
        }
        self.out.push('>');
        // The schema wraps a title's lines in paragraphs, but real
        // books also write the text bare; both shapes read.
        let mut first = true;
        for child in title.children() {
            if child.is_element() && child.tag_name().name() == "p" {
                if !first {
                    self.out.push_str("<br/>");
                }
                first = false;
                self.inline_children(child);
            } else if child.is_element() && child.tag_name().name() == "empty-line" {
                continue;
            } else if child.is_element() || child.text().is_some_and(|t| !t.trim().is_empty()) {
                first = false;
                self.inline_node(child);
            }
        }
        self.out.push_str(&format!("</h{level}>"));
    }

    /// A block-level FB2 element into its XHTML counterpart.
    fn flow(&mut self, node: roxmltree::Node) {
        match node.tag_name().name() {
            // A paragraph holding only images is a block image in
            // paragraph clothing, the shape real books use; it centers
            // like the bare element. An image among text stays inline.
            "p" if images_only(node) => {
                self.out.push_str("<p align=\"center\">");
                self.inline_children(node);
                self.out.push_str("</p>");
            }
            "p" => {
                self.out.push_str("<p>");
                self.inline_children(node);
                self.out.push_str("</p>");
            }
            "subtitle" => {
                self.out.push_str("<p><strong>");
                self.inline_children(node);
                self.out.push_str("</strong></p>");
            }
            "empty-line" => self.out.push_str("<p>&#160;</p>"),
            // A block image centers: FB2 carries no stylesheet, so the
            // presentation is the reader's call, and centered is the
            // convention FB2 readers follow.
            "image" => {
                self.out.push_str("<p align=\"center\">");
                self.image(node);
                self.out.push_str("</p>");
            }
            "epigraph" | "cite" => {
                self.out.push_str("<blockquote>");
                for child in node.children().filter(|n| n.is_element()) {
                    match child.tag_name().name() {
                        "text-author" => {
                            self.out.push_str("<p><em>");
                            self.inline_children(child);
                            self.out.push_str("</em></p>");
                        }
                        _ => self.flow(child),
                    }
                }
                self.out.push_str("</blockquote>");
            }
            "poem" => {
                self.out.push_str("<blockquote>");
                for child in node.children().filter(|n| n.is_element()) {
                    match child.tag_name().name() {
                        "title" => {
                            self.out.push_str("<p><strong>");
                            for p in child.children().filter(|n| n.tag_name().name() == "p") {
                                self.inline_children(p);
                            }
                            self.out.push_str("</strong></p>");
                        }
                        "subtitle" => {
                            self.out.push_str("<p><strong>");
                            self.inline_children(child);
                            self.out.push_str("</strong></p>");
                        }
                        "stanza" => {
                            self.out.push_str("<p>");
                            let mut first = true;
                            for v in child.children().filter(|n| n.tag_name().name() == "v") {
                                if !first {
                                    self.out.push_str("<br/>");
                                }
                                first = false;
                                self.inline_children(v);
                            }
                            self.out.push_str("</p>");
                        }
                        "text-author" => {
                            self.out.push_str("<p><em>");
                            self.inline_children(child);
                            self.out.push_str("</em></p>");
                        }
                        _ => self.flow(child),
                    }
                }
                self.out.push_str("</blockquote>");
            }
            "table" => {
                self.out.push_str("<table>");
                for tr in node.children().filter(|n| n.tag_name().name() == "tr") {
                    self.out.push_str("<tr>");
                    for cell in tr.children().filter(|n| n.is_element()) {
                        let tag = match cell.tag_name().name() {
                            "th" => "th",
                            _ => "td",
                        };
                        self.out.push_str(&format!("<{tag}>"));
                        self.inline_children(cell);
                        self.out.push_str(&format!("</{tag}>"));
                    }
                    self.out.push_str("</tr>");
                }
                self.out.push_str("</table>");
            }
            _ => {
                self.out.push_str("<p>");
                self.inline_children(node);
                self.out.push_str("</p>");
            }
        }
    }

    /// Inline content: text and the FB2 style elements.
    fn inline_children(&mut self, node: roxmltree::Node) {
        for child in node.children() {
            self.inline_node(child);
        }
    }

    /// One inline node: text escapes, the FB2 style elements map.
    fn inline_node(&mut self, child: roxmltree::Node) {
        if let Some(text) = child.text().filter(|_| child.is_text()) {
            esc(text, &mut self.out);
            return;
        }
        if !child.is_element() {
            return;
        }
        match child.tag_name().name() {
            "emphasis" => self.styled("em", child),
            "strong" => self.styled("strong", child),
            "strikethrough" => self.styled("s", child),
            "code" => self.styled("code", child),
            "sub" => self.styled("sub", child),
            "sup" => self.styled("sup", child),
            "style" => self.inline_children(child),
            "image" => self.image(child),
            "a" => self.link(child),
            _ => self.inline_children(child),
        }
    }

    fn styled(&mut self, tag: &str, node: roxmltree::Node) {
        self.out.push_str(&format!("<{tag}>"));
        self.inline_children(node);
        self.out.push_str(&format!("</{tag}>"));
    }

    /// A link: internal targets resolve through the id map to the
    /// chapter holding them, so a note reference lands in the notes
    /// chapter; external http targets pass through; anything else keeps
    /// its text and loses the dead link. A note reference reads
    /// superscript, the convention FB2 readers follow.
    fn link(&mut self, node: roxmltree::Node) {
        let note = node.attribute("type") == Some("note");
        let href = node.attributes().find(|a| a.name() == "href").map(|a| {
            let value = a.value();
            match value.strip_prefix('#') {
                Some(id) => self.ids.get(id).map(|chapter| format!("{chapter}#{id}")),
                None if value.starts_with("http://") || value.starts_with("https://") => {
                    Some(value.to_string())
                }
                None => None,
            }
        });
        let Some(Some(href)) = href else {
            self.inline_children(node);
            return;
        };
        if note {
            self.out.push_str("<sup>");
        }
        self.out.push_str("<a href=\"");
        esc(&href, &mut self.out);
        self.out.push_str("\">");
        self.inline_children(node);
        self.out.push_str("</a>");
        if note {
            self.out.push_str("</sup>");
        }
    }

    fn image(&mut self, node: roxmltree::Node) {
        let Some(key) = href_key(node) else {
            return;
        };
        self.out.push_str("<img src=\"");
        esc(&key, &mut self.out);
        self.out.push('"');
        if let Some(alt) = node.attribute("alt") {
            self.out.push_str(" alt=\"");
            esc(alt, &mut self.out);
            self.out.push('"');
        }
        self.out.push_str("/>");
    }
}
