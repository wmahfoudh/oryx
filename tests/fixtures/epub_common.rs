//! Builds EPUB archives in memory for the epub and export tests, so
//! every fixture stays readable Rust instead of a binary file in the
//! tree. Two test binaries share it, and neither uses all of it.
#![allow(dead_code)]

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// A book under construction. Chapters land under `OEBPS/text/`, fonts
/// under `OEBPS/fonts/`; the OPF and container are derived at `build`.
pub struct BookBuilder {
    title: Option<String>,
    creator: Option<String>,
    identifier: Option<String>,
    pre_paginated: bool,
    /// OPF-relative hrefs named by `encryption.xml`, one entry each.
    encrypted: Vec<String>,
    /// (zip path, bytes, media type); spine members in insertion order.
    chapters: Vec<(String, Vec<u8>)>,
    fonts: Vec<String>,
    stylesheet: Option<String>,
    /// (OPF-relative href, bytes); media type follows the extension.
    images: Vec<(String, Vec<u8>)>,
    /// A `nav.xhtml` with the `nav` property, as written.
    nav_doc: Option<String>,
    /// A `toc.ncx` named by the spine's `toc` attribute, as written.
    ncx: Option<String>,
}

pub fn book() -> BookBuilder {
    BookBuilder {
        title: Some("Test Book".to_string()),
        creator: Some("A. Author".to_string()),
        identifier: Some("urn:test:1".to_string()),
        pre_paginated: false,
        encrypted: Vec::new(),
        chapters: Vec::new(),
        fonts: Vec::new(),
        stylesheet: None,
        images: Vec::new(),
        nav_doc: None,
        ncx: None,
    }
}

/// A solid-color PNG for image fixtures.
pub fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

impl BookBuilder {
    pub fn chapter(mut self, name: &str, xhtml: &str) -> Self {
        self.chapters
            .push((format!("text/{name}"), xhtml.as_bytes().to_vec()));
        self
    }

    /// A chapter with caller-controlled bytes; the UTF-16 case.
    pub fn chapter_bytes(mut self, name: &str, bytes: Vec<u8>) -> Self {
        self.chapters.push((format!("text/{name}"), bytes));
        self
    }

    /// The EPUB3 nav document, written to `nav.xhtml` beside the OPF.
    pub fn nav_doc(mut self, xhtml: &str) -> Self {
        self.nav_doc = Some(xhtml.to_string());
        self
    }

    /// The EPUB2 NCX, written to `toc.ncx` and named by the spine.
    pub fn ncx(mut self, xml: &str) -> Self {
        self.ncx = Some(xml.to_string());
        self
    }

    /// Drops `dc:identifier`, for the path-fallback case.
    pub fn no_identifier(mut self) -> Self {
        self.identifier = None;
        self
    }

    /// An image entry in the manifest and the archive.
    pub fn image(mut self, href: &str, bytes: Vec<u8>) -> Self {
        self.images.push((href.to_string(), bytes));
        self
    }

    /// A stylesheet manifest entry the chapters reference.
    pub fn stylesheet(mut self, css: &str) -> Self {
        self.stylesheet = Some(css.to_string());
        self
    }

    /// A font manifest entry with placeholder bytes.
    pub fn font(mut self, name: &str) -> Self {
        self.fonts.push(format!("fonts/{name}"));
        self
    }

    /// Adds an `encryption.xml` entry covering the given OPF-relative href.
    pub fn encrypted(mut self, href: &str) -> Self {
        self.encrypted.push(href.to_string());
        self
    }

    pub fn pre_paginated(mut self) -> Self {
        self.pre_paginated = true;
        self
    }

    pub fn build(self) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();

        if !self.encrypted.is_empty() {
            let mut enc = String::from(
                "<?xml version=\"1.0\"?>\n<encryption xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n",
            );
            for href in &self.encrypted {
                enc.push_str(&format!(
                    "  <EncryptedData xmlns=\"http://www.w3.org/2001/04/xmlenc#\">\n    <CipherData><CipherReference URI=\"OEBPS/{href}\"/></CipherData>\n  </EncryptedData>\n"
                ));
            }
            enc.push_str("</encryption>");
            zip.start_file("META-INF/encryption.xml", deflated).unwrap();
            zip.write_all(enc.as_bytes()).unwrap();
        }

        let mut opf = String::from(
            "<?xml version=\"1.0\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
        );
        if let Some(title) = &self.title {
            opf.push_str(&format!("    <dc:title>{title}</dc:title>\n"));
        }
        if let Some(creator) = &self.creator {
            opf.push_str(&format!("    <dc:creator>{creator}</dc:creator>\n"));
        }
        if let Some(id) = &self.identifier {
            opf.push_str(&format!(
                "    <dc:identifier id=\"uid\">{id}</dc:identifier>\n"
            ));
        }
        if self.pre_paginated {
            opf.push_str("    <meta property=\"rendition:layout\">pre-paginated</meta>\n");
        }
        opf.push_str("  </metadata>\n  <manifest>\n");
        for (i, (href, _)) in self.chapters.iter().enumerate() {
            opf.push_str(&format!(
                "    <item id=\"c{i}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>\n"
            ));
        }
        for (i, href) in self.fonts.iter().enumerate() {
            opf.push_str(&format!(
                "    <item id=\"f{i}\" href=\"{href}\" media-type=\"application/vnd.ms-opentype\"/>\n"
            ));
        }
        if self.stylesheet.is_some() {
            opf.push_str(
                "    <item id=\"css0\" href=\"style/main.css\" media-type=\"text/css\"/>\n",
            );
        }
        for (i, (href, _)) in self.images.iter().enumerate() {
            opf.push_str(&format!(
                "    <item id=\"img{i}\" href=\"{href}\" media-type=\"image/png\"/>\n"
            ));
        }
        if self.nav_doc.is_some() {
            opf.push_str(
                "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
            );
        }
        if self.ncx.is_some() {
            opf.push_str(
                "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
            );
            opf.push_str("  </manifest>\n  <spine toc=\"ncx\">\n");
        } else {
            opf.push_str("  </manifest>\n  <spine>\n");
        }
        for i in 0..self.chapters.len() {
            opf.push_str(&format!("    <itemref idref=\"c{i}\"/>\n"));
        }
        opf.push_str("  </spine>\n</package>");
        zip.start_file("OEBPS/content.opf", deflated).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();

        if let Some(css) = &self.stylesheet {
            zip.start_file("OEBPS/style/main.css", deflated).unwrap();
            zip.write_all(css.as_bytes()).unwrap();
        }
        if let Some(nav) = &self.nav_doc {
            zip.start_file("OEBPS/nav.xhtml", deflated).unwrap();
            zip.write_all(nav.as_bytes()).unwrap();
        }
        if let Some(ncx) = &self.ncx {
            zip.start_file("OEBPS/toc.ncx", deflated).unwrap();
            zip.write_all(ncx.as_bytes()).unwrap();
        }
        for (href, bytes) in &self.images {
            zip.start_file(format!("OEBPS/{href}"), deflated).unwrap();
            zip.write_all(bytes).unwrap();
        }
        for (href, bytes) in &self.chapters {
            zip.start_file(format!("OEBPS/{href}"), deflated).unwrap();
            zip.write_all(bytes).unwrap();
        }
        for href in &self.fonts {
            zip.start_file(format!("OEBPS/{href}"), deflated).unwrap();
            zip.write_all(b"not really a font").unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    /// Builds and writes the book beside the test binary, answering its
    /// path; `load::open` wants a file.
    pub fn write_to(self, name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, self.build()).unwrap();
        path
    }
}

/// A syntactically valid zip whose one stored entry declares the given
/// uncompressed size without carrying any data: local header, central
/// directory, end record. The ceiling check reads declared sizes only,
/// so the lie is enough.
pub fn zip_declaring(size: u64) -> Vec<u8> {
    let name = b"huge.bin";
    let size32 = u32::try_from(size).unwrap_or(u32::MAX);
    let mut out = Vec::new();

    // Local file header.
    out.extend_from_slice(&0x04034b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
    out.extend_from_slice(&0u32.to_le_bytes()); // dos time/date
    out.extend_from_slice(&0u32.to_le_bytes()); // crc
    out.extend_from_slice(&0u32.to_le_bytes()); // compressed size
    out.extend_from_slice(&size32.to_le_bytes()); // uncompressed size
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(name);

    let central_offset = out.len() as u32;

    // Central directory header.
    out.extend_from_slice(&0x02014b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); // version made by
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&0u16.to_le_bytes()); // method
    out.extend_from_slice(&0u32.to_le_bytes()); // dos time/date
    out.extend_from_slice(&0u32.to_le_bytes()); // crc
    out.extend_from_slice(&0u32.to_le_bytes()); // compressed size
    out.extend_from_slice(&size32.to_le_bytes()); // uncompressed size
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out.extend_from_slice(&0u16.to_le_bytes()); // disk start
    out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // local header offset
    out.extend_from_slice(name);

    let central_size = out.len() as u32 - central_offset;

    // End of central directory.
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // central dir disk
    out.extend_from_slice(&1u16.to_le_bytes()); // entries this disk
    out.extend_from_slice(&1u16.to_le_bytes()); // entries total
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len

    out
}
