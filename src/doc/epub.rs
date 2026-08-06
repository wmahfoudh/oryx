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

pub fn read_package(archive: &mut Archive) -> anyhow::Result<Package> {
    let container = archive
        .read("META-INF/container.xml")
        .ok_or(Refusal::NoPackage)?;
    let container = String::from_utf8_lossy(&container).into_owned();
    let tree = roxmltree::Document::parse(&container).map_err(|_| Refusal::NoPackage)?;
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
    let tree = roxmltree::Document::parse(&opf).map_err(|_| Refusal::NoPackage)?;

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
    let Ok(tree) = roxmltree::Document::parse(&text) else {
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
}

/// The whole book, synchronously: every spine item walked in order into
/// one Document over the synthetic source, images decoding as the
/// chapter holding them parses.
pub fn open_book(bytes: Vec<u8>) -> anyhow::Result<Book> {
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
    let mut images = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (position, &item) in package.spine.iter().enumerate() {
        if position > 0 {
            walker.chapter_break(position);
        }
        let path = resolve(&package.root, &package.manifest[item].href);
        let base = match path.rfind('/') {
            Some(slash) => path[..slash].to_string(),
            None => String::new(),
        };
        walker.set_chapter_base(&base);
        let Some(bytes) = archive.read(&path) else {
            continue;
        };
        walker.walk_chapter(&decode(&bytes));
        chapter_images(&mut archive, &base, &mut walker, &mut seen, &mut images);
    }
    let (blocks, source, details) = walker.finish();
    Ok(Book {
        document: Document {
            blocks,
            source: Arc::from(source),
            details,
            title: package.title,
        },
        images,
    })
}

/// Extracts and decodes what one chapter referenced: plain images by
/// their archive path, inline svgs with their archive references
/// inlined as data URIs so resvg can resolve them.
fn chapter_images(
    archive: &mut Archive,
    base: &str,
    walker: &mut crate::doc::html::Walker,
    seen: &mut std::collections::HashSet<String>,
    images: &mut Vec<(String, image::RgbaImage)>,
) {
    for src in walker.take_images() {
        if src.starts_with("http") || !seen.insert(src.clone()) {
            continue;
        }
        if let Some(bytes) = archive.read(&src) {
            if let Some(decoded) = crate::doc::images::decode(&bytes) {
                images.push((src, decoded));
            }
        }
    }
    for svg in walker.take_svgs() {
        let mut markup = svg.markup;
        for href in &svg.refs {
            let target = crate::doc::html::join_href(base, href);
            if let Some(raw) = archive.read(&target) {
                use base64::Engine;
                let data = format!(
                    "\"data:{};base64,{}\"",
                    media_type_of(&target),
                    base64::engine::general_purpose::STANDARD.encode(raw)
                );
                markup = markup.replace(&format!("\"{href}\""), &data);
            }
        }
        if let Some(decoded) = crate::doc::images::decode(markup.as_bytes()) {
            images.push((svg.key, decoded));
        }
    }
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
