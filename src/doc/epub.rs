//! EPUB package layer: the archive, the container and OPF, and the
//! whole-book assembly into a Document. The XHTML walking lives in
//! `doc::html`; nothing here knows about layout or paint.

use std::io::{Cursor, Read};
use std::sync::Arc;

use crate::doc::model::Document;

/// Declared uncompressed sizes summed past this refuse the archive.
/// Real books are tens of megabytes; only a hostile file is bigger.
const CEILING: u64 = 1 << 30;

/// Why a book cannot open. Each reason renders as its plain sentence
/// through the same path that refuses binaries.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    NotEpub,
    NoPackage,
    Drm,
    FixedLayout,
    TooLarge,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::NotEpub => "This file is not an EPUB book.",
            Refusal::NoPackage => "This EPUB book has no readable package.",
            Refusal::Drm => "This book is DRM-protected and cannot be opened.",
            Refusal::FixedLayout => "Fixed-layout books are not supported.",
            Refusal::TooLarge => "This book is too large to open.",
        })
    }
}

impl std::error::Error for Refusal {}

/// The open zip. Reads are by full path from the archive root.
pub struct Archive {
    zip: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Archive {
    pub fn open(bytes: Vec<u8>) -> anyhow::Result<Archive> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Refusal::NotEpub)?;
        let mut archive = Archive { zip };
        let mut declared = 0u64;
        for index in 0..archive.zip.len() {
            if let Ok(entry) = archive.zip.by_index_raw(index) {
                declared = declared.saturating_add(entry.size());
            }
        }
        if declared > CEILING {
            return Err(Refusal::TooLarge.into());
        }
        Ok(archive)
    }

    /// The entry's bytes, or None when no entry has that name.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut file = self.zip.by_name(path).ok()?;
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    fn has(&self, path: &str) -> bool {
        self.zip.index_for_name(path).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestItem {
    pub id: String,
    /// OPF-relative as written; `resolve` turns it into an archive path.
    pub href: String,
    pub media_type: String,
    pub properties: String,
}

/// Where the table of contents lives, as a manifest index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocSource {
    Nav(usize),
    Ncx(usize),
    None,
}

pub struct Package {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub identifier: Option<String>,
    pub manifest: Vec<ManifestItem>,
    /// Manifest indices in reading order, as written; `linear` is ignored.
    pub spine: Vec<usize>,
    pub toc: TocSource,
    /// The OPF's directory inside the archive, `""` or ending in `/`;
    /// manifest hrefs resolve against it.
    pub root: String,
}

/// Joins an href onto the package root: `../` segments collapse and
/// percent escapes decode, since hrefs are URLs and zip names are not.
pub fn resolve(root: &str, href: &str) -> String {
    crate::doc::html::join_href(root, href)
}

/// XML parsing with DTDs allowed: the NCX spec mandates a DOCTYPE and
/// real books carry one on every XML file. roxmltree caps entity depth
/// and expansion, so a hostile internal subset stays bounded.
fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
}

pub fn read_package(archive: &mut Archive) -> anyhow::Result<Package> {
    let container = archive
        .read("META-INF/container.xml")
        .ok_or(Refusal::NoPackage)?;
    let container = String::from_utf8_lossy(&container).into_owned();
    let tree = parse_xml(&container).map_err(|_| Refusal::NoPackage)?;
    let opf_path = tree
        .descendants()
        .find(|n| {
            n.has_tag_name((
                "urn:oasis:names:tc:opendocument:xmlns:container",
                "rootfile",
            ))
        })
        .and_then(|n| n.attribute("full-path"))
        .ok_or(Refusal::NoPackage)?
        .to_string();
    let root = match opf_path.rfind('/') {
        Some(slash) => opf_path[..=slash].to_string(),
        None => String::new(),
    };

    let opf = archive.read(&opf_path).ok_or(Refusal::NoPackage)?;
    let opf = String::from_utf8_lossy(&opf).into_owned();
    let tree = parse_xml(&opf).map_err(|_| Refusal::NoPackage)?;

    let fixed = tree.descendants().any(|n| {
        n.tag_name().name() == "meta"
            && n.attribute("property") == Some("rendition:layout")
            && n.text().is_some_and(|t| t.trim() == "pre-paginated")
    });
    if fixed {
        return Err(Refusal::FixedLayout.into());
    }

    let dc = |name: &str| {
        tree.descendants()
            .find(|n| n.tag_name().name() == name)
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };

    let mut manifest = Vec::new();
    for node in tree.descendants().filter(|n| n.tag_name().name() == "item") {
        let (Some(id), Some(href), Some(media_type)) = (
            node.attribute("id"),
            node.attribute("href"),
            node.attribute("media-type"),
        ) else {
            continue;
        };
        manifest.push(ManifestItem {
            id: id.to_string(),
            href: href.to_string(),
            media_type: media_type.to_string(),
            properties: node.attribute("properties").unwrap_or_default().to_string(),
        });
    }

    let index_of = |id: &str| manifest.iter().position(|item| item.id == id);
    let mut spine = Vec::new();
    let mut ncx_id = None;
    for node in tree.descendants() {
        match node.tag_name().name() {
            "spine" => ncx_id = node.attribute("toc").map(str::to_string),
            "itemref" => {
                if let Some(index) = node.attribute("idref").and_then(index_of) {
                    spine.push(index);
                }
            }
            _ => {}
        }
    }

    let toc = match manifest
        .iter()
        .position(|item| item.properties.split_whitespace().any(|p| p == "nav"))
    {
        Some(nav) => TocSource::Nav(nav),
        None => match ncx_id.as_deref().and_then(index_of) {
            Some(ncx) => TocSource::Ncx(ncx),
            None => TocSource::None,
        },
    };

    let package = Package {
        title: dc("title"),
        creator: dc("creator"),
        identifier: dc("identifier"),
        manifest,
        spine,
        toc,
        root,
    };
    check_encryption(archive, &package)?;
    Ok(package)
}

/// Refuses a book whose `encryption.xml` covers anything but fonts.
/// Font obfuscation is common and harmless here, since Oryx never uses
/// book fonts.
fn check_encryption(archive: &mut Archive, package: &Package) -> anyhow::Result<()> {
    if !archive.has("META-INF/encryption.xml") {
        return Ok(());
    }
    let Some(bytes) = archive.read("META-INF/encryption.xml") else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let Ok(tree) = parse_xml(&text) else {
        return Err(Refusal::Drm.into());
    };
    for node in tree.descendants() {
        if node.tag_name().name() != "CipherReference" {
            continue;
        }
        let Some(uri) = node.attribute("URI") else {
            return Err(Refusal::Drm.into());
        };
        let path = crate::doc::html::percent_decode(uri.trim_start_matches('/')).into_owned();
        if !is_font(package, &path) {
            return Err(Refusal::Drm.into());
        }
    }
    Ok(())
}

/// Whether an archive path names a font, by manifest media type first
/// and extension as the fallback for entries the manifest misses.
fn is_font(package: &Package, path: &str) -> bool {
    let manifest_font = package.manifest.iter().any(|item| {
        resolve(&package.root, &item.href) == path
            && (item.media_type.starts_with("font/")
                || item.media_type == "application/vnd.ms-opentype"
                || item.media_type == "application/font-sfnt"
                || item.media_type == "application/font-woff")
    });
    let ext = path.rsplit('.').next().unwrap_or_default();
    manifest_font || matches!(ext, "otf" | "ttf" | "woff" | "woff2")
}

/// Chapter bytes to text: UTF-16 by BOM, UTF-8 otherwise, both lossy.
fn decode(bytes: &[u8]) -> String {
    let wide = |bytes: &[u8], read: fn([u8; 2]) -> u16| {
        char::decode_utf16(bytes.chunks_exact(2).map(|pair| read([pair[0], pair[1]])))
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => wide(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => wide(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// An opened book: the document and the decoded images the chapters
/// referenced, keyed by their book-internal source for the image store.
#[derive(Debug)]
pub struct Book {
    pub document: Document,
    pub images: Vec<(String, image::RgbaImage)>,
    pub toc: Vec<TocEntry>,
}

use crate::doc::images::BookSource;

/// The book past the prefix: the archive, the walker mid-book, and the
/// image sources not yet handed over. `run` continues it on the parse
/// worker; the archive buffer dies with the job when the walk
/// completes.
pub struct BookJob {
    archive: Archive,
    package: Package,
    walker: crate::doc::html::Walker,
    next: usize,
    sources: Vec<crate::doc::images::SourceEntry>,
    seen: std::collections::HashSet<String>,
}

impl BookJob {
    pub fn has_chapters(&self) -> bool {
        self.next < self.package.spine.len()
    }

    /// Image sources with their header dimensions since the last take,
    /// for the store; sizes reach layout ahead of any pixel, and the
    /// pixels themselves decode on demand from the store.
    pub fn take_sources(&mut self) -> Vec<crate::doc::images::SourceEntry> {
        std::mem::take(&mut self.sources)
    }

    /// Walks the next spine item and queues what it referenced: plain
    /// images as their archive bytes, inline svgs with their archive
    /// references inlined as data URIs so resvg can resolve them.
    fn step(&mut self) {
        let position = self.next;
        self.next += 1;
        if position > 0 {
            self.walker.chapter_break(position);
        }
        let item = self.package.spine[position];
        let path = resolve(&self.package.root, &self.package.manifest[item].href);
        let base = match path.rfind('/') {
            Some(slash) => path[..slash].to_string(),
            None => String::new(),
        };
        self.walker.set_chapter(&path);
        let Some(bytes) = self.archive.read(&path) else {
            return;
        };
        self.walker.walk_chapter(&decode(&bytes));

        for src in self.walker.take_images() {
            if src.starts_with("http") || !self.seen.insert(src.clone()) {
                continue;
            }
            if let Some(bytes) = self.archive.read(&src) {
                let source = BookSource::Raster(bytes);
                let dims = crate::doc::images::probe_source(&source);
                self.sources.push((src, source, dims));
            }
        }
        for svg in self.walker.take_svgs() {
            let mut markup = svg.markup;
            for href in &svg.refs {
                let target = crate::doc::html::join_href(&base, href);
                if let Some(raw) = self.archive.read(&target) {
                    use base64::Engine;
                    let data = format!(
                        "\"data:{};base64,{}\"",
                        media_type_of(&target),
                        base64::engine::general_purpose::STANDARD.encode(raw)
                    );
                    markup = markup.replace(&format!("\"{href}\""), &data);
                }
            }
            let source = BookSource::Svg(markup);
            let dims = crate::doc::images::probe_source(&source);
            self.sources.push((svg.key, source, dims));
        }
    }
}

/// Opens a book to its prefix: whole spine items until the parse-prefix
/// target is crossed, no image decoded, the table of contents parsed
/// while the archive is at hand. The continuation carries the rest;
/// None means a small book with no images at all.
pub fn open_prefix(bytes: Vec<u8>) -> anyhow::Result<(Document, Vec<TocEntry>, Option<BookJob>)> {
    let mut archive = Archive::open(bytes)?;
    let package = read_package(&mut archive)?;
    let mut table = crate::doc::html::EmphasisTable::default();
    for item in &package.manifest {
        if item.media_type.eq_ignore_ascii_case("text/css") {
            if let Some(css) = archive.read(&resolve(&package.root, &item.href)) {
                table.add_css(&String::from_utf8_lossy(&css));
            }
        }
    }
    let mut walker = crate::doc::html::Walker::new();
    walker.set_emphasis(table);
    walker.set_book_files(
        package
            .spine
            .iter()
            .map(|&item| resolve(&package.root, &package.manifest[item].href))
            .collect(),
    );
    let toc = read_toc(&mut archive, &package);
    let mut job = BookJob {
        archive,
        package,
        walker,
        next: 0,
        sources: Vec::new(),
        seen: std::collections::HashSet::new(),
    };
    while job.has_chapters() && job.walker.source_len() < crate::doc::stream::PREFIX_TARGET {
        job.step();
    }
    let (blocks, source, details) = job.walker.snapshot();
    let document = Document {
        blocks,
        source: Arc::from(source),
        details,
        title: job.package.title.clone(),
        anchors: job.walker.anchors().iter().cloned().collect(),
        book_id: job.package.identifier.clone(),
        code_file: false,
        plain_file: false,
    };
    let job = (job.has_chapters() || !job.sources.is_empty()).then_some(job);
    Ok((document, toc, job))
}

/// One table-of-contents entry as authored: label, nesting depth, and
/// the target as an archive path with an optional fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub label: String,
    pub depth: u8,
    pub path: String,
    pub fragment: Option<String>,
}

/// The book's table of contents: the EPUB3 nav document, or the EPUB2
/// NCX when that is all the book has. Empty when neither reads.
pub fn read_toc(archive: &mut Archive, package: &Package) -> Vec<TocEntry> {
    let (index, ncx) = match package.toc {
        TocSource::Nav(index) => (index, false),
        TocSource::Ncx(index) => (index, true),
        TocSource::None => return Vec::new(),
    };
    let path = resolve(&package.root, &package.manifest[index].href);
    let base = match path.rfind('/') {
        Some(slash) => path[..slash].to_string(),
        None => String::new(),
    };
    let Some(bytes) = archive.read(&path) else {
        return Vec::new();
    };
    let text = decode(&bytes);
    if ncx {
        read_ncx(&text, &base)
    } else {
        read_nav(&text, &base)
    }
}

/// Splits an href into an archive path and fragment against a base.
fn toc_target(base: &str, href: &str) -> (String, Option<String>) {
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (href, None),
    };
    (crate::doc::html::join_href(base, path), fragment)
}

/// The nav document's `epub:type="toc"` list, or the first `<nav>` when
/// the type is absent; nested `<ol>` levels nest the entries.
fn read_nav(xhtml: &str, base: &str) -> Vec<TocEntry> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    fn attr_of(node: &Handle, key: &str) -> Option<String> {
        if let NodeData::Element { attrs, .. } = &node.data {
            for a in attrs.borrow().iter() {
                if a.name.local.as_ref().eq_ignore_ascii_case(key) {
                    return Some(a.value.to_string());
                }
            }
        }
        None
    }

    fn tag_of(node: &Handle) -> Option<String> {
        match &node.data {
            NodeData::Element { name, .. } => Some(name.local.as_ref().to_ascii_lowercase()),
            _ => None,
        }
    }

    fn text_of(node: &Handle, out: &mut String) {
        match &node.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            _ => {
                for child in node.children.borrow().iter() {
                    text_of(child, out);
                }
            }
        }
    }

    fn find_nav(node: &Handle, fallback: &mut Option<Handle>) -> Option<Handle> {
        if tag_of(node).as_deref() == Some("nav") {
            if attr_of(node, "epub:type").as_deref() == Some("toc") {
                return Some(node.clone());
            }
            if fallback.is_none() {
                *fallback = Some(node.clone());
            }
        }
        for child in node.children.borrow().iter() {
            if let Some(nav) = find_nav(child, fallback) {
                return Some(nav);
            }
        }
        None
    }

    fn read_list(node: &Handle, base: &str, depth: u8, out: &mut Vec<TocEntry>) {
        for child in node.children.borrow().iter() {
            match tag_of(child).as_deref() {
                Some("li") => {
                    let mut label = String::new();
                    let mut href = None;
                    for inner in child.children.borrow().iter() {
                        match tag_of(inner).as_deref() {
                            Some("a") => {
                                text_of(inner, &mut label);
                                href = attr_of(inner, "href");
                            }
                            Some("span") if label.is_empty() => text_of(inner, &mut label),
                            _ => {}
                        }
                    }
                    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !label.is_empty() {
                        let (path, fragment) = match &href {
                            Some(h) => toc_target(base, h),
                            None => (String::new(), None),
                        };
                        out.push(TocEntry {
                            label,
                            depth,
                            path,
                            fragment,
                        });
                    }
                    for inner in child.children.borrow().iter() {
                        if tag_of(inner).as_deref() == Some("ol") {
                            read_list(inner, base, depth + 1, out);
                        }
                    }
                }
                Some("ol") => read_list(child, base, depth, out),
                _ => {}
            }
        }
    }

    let dom = html5ever::parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .one(xhtml.as_bytes());
    let mut fallback = None;
    let nav = find_nav(&dom.document, &mut fallback).or(fallback);
    let mut out = Vec::new();
    if let Some(nav) = nav {
        for child in nav.children.borrow().iter() {
            if tag_of(&child.clone()).as_deref() == Some("ol") {
                read_list(child, base, 0, &mut out);
            }
        }
    }
    out
}

/// The NCX's navMap; navPoint nesting nests the entries.
fn read_ncx(xml: &str, base: &str) -> Vec<TocEntry> {
    fn read_points(node: roxmltree::Node, base: &str, depth: u8, out: &mut Vec<TocEntry>) {
        for point in node
            .children()
            .filter(|n| n.tag_name().name() == "navPoint")
        {
            let label = point
                .children()
                .find(|n| n.tag_name().name() == "navLabel")
                .and_then(|l| l.children().find(|n| n.tag_name().name() == "text"))
                .and_then(|t| t.text())
                .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let href = point
                .children()
                .find(|n| n.tag_name().name() == "content")
                .and_then(|c| c.attribute("src"));
            if !label.is_empty() {
                let (path, fragment) = match href {
                    Some(h) => toc_target(base, h),
                    None => (String::new(), None),
                };
                out.push(TocEntry {
                    label,
                    depth,
                    path,
                    fragment,
                });
            }
            read_points(point, base, depth + 1, out);
        }
    }

    let Ok(tree) = parse_xml(xml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(map) = tree.descendants().find(|n| n.tag_name().name() == "navMap") {
        read_points(map, base, 0, &mut out);
    }
    out
}

/// A target's source offset: the exact `path#fragment` anchor, or the
/// chapter's start when the fragment misses. None when the whole file
/// is absent from the book.
pub fn resolve_target(doc: &Document, path: &str, fragment: Option<&str>) -> Option<usize> {
    if let Some(fragment) = fragment {
        if let Some(&offset) = doc.anchors.get(&format!("{path}#{fragment}")) {
            return Some(offset);
        }
    }
    doc.anchors.get(path).copied()
}

/// Continues the book on the parse worker: remaining chapters walk and
/// their image sources hand over as they appear, so every size is known
/// before its block lays out. The delivery is the full model over the
/// grown source; a bail between chapters delivers nothing.
pub fn run(
    mut job: BookJob,
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
    let BookJob { walker, .. } = job;
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
/// and every image decoded in place. The tests' and the export path's
/// view of the same pipeline the app streams through.
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
    let title = job.package.title.clone();
    let book_id = job.package.identifier.clone();
    let BookJob { walker, .. } = job;
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

fn media_type_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default();
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
