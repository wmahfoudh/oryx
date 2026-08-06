//! EPUB package parsing and the plain-chapter pipeline, driven through
//! books assembled in memory by the fixture builder.

#[path = "fixtures/epub_common.rs"]
mod epub_common;

use epub_common::{book, zip_declaring};

use oryx::doc::epub::{self, Archive};
use oryx::doc::load::{self, FileKind};
use oryx::doc::model::{BlockKind, Document};

/// Paragraph text in block order, one line per block; break markers and
/// non-paragraph blocks contribute nothing.
fn plain_text(doc: &Document) -> String {
    let mut lines = Vec::new();
    for block in &doc.blocks {
        if let BlockKind::Paragraph { spans } = &block.kind {
            let mut line = String::new();
            for span in spans {
                line.push_str(span.text(&doc.source));
            }
            lines.push(line);
        }
    }
    lines.join("\n")
}

#[test]
fn package_reads_metadata_manifest_and_spine() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .chapter("two.xhtml", "<html><body><p>Two.</p></body></html>")
        .font("stix.otf")
        .build();
    let mut archive = Archive::open(bytes).unwrap();
    let package = epub::read_package(&mut archive).unwrap();

    assert_eq!(package.title.as_deref(), Some("Test Book"));
    assert_eq!(package.creator.as_deref(), Some("A. Author"));
    assert_eq!(package.identifier.as_deref(), Some("urn:test:1"));
    assert_eq!(package.spine.len(), 2);
    let spine_types: Vec<&str> = package
        .spine
        .iter()
        .map(|&i| package.manifest[i].media_type.as_str())
        .collect();
    assert_eq!(
        spine_types,
        ["application/xhtml+xml", "application/xhtml+xml"]
    );
    assert!(package
        .manifest
        .iter()
        .any(|item| item.media_type == "application/vnd.ms-opentype"));
    let hrefs: Vec<&str> = package
        .spine
        .iter()
        .map(|&i| package.manifest[i].href.as_str())
        .collect();
    assert_eq!(hrefs, ["text/one.xhtml", "text/two.xhtml"]);
}

#[test]
fn encrypted_content_refuses_as_drm() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .encrypted("text/one.xhtml")
        .build();
    let err = epub::open_book(bytes).unwrap_err();
    assert!(err.to_string().contains("DRM"), "{err}");
}

#[test]
fn font_obfuscation_is_not_drm() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .font("stix.otf")
        .encrypted("fonts/stix.otf")
        .build();
    assert!(epub::open_book(bytes).is_ok());
}

#[test]
fn pre_paginated_refuses() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .pre_paginated()
        .build();
    let err = epub::open_book(bytes).unwrap_err();
    assert!(err.to_string().contains("Fixed-layout"), "{err}");
}

#[test]
fn not_a_zip_refuses() {
    let err = epub::open_book(b"just some prose with an epub extension".to_vec()).unwrap_err();
    assert!(err.to_string().contains("not an EPUB"), "{err}");
}

#[test]
fn declared_size_past_ceiling_refuses() {
    let err = epub::open_book(zip_declaring(2 * 1024 * 1024 * 1024)).unwrap_err();
    assert!(err.to_string().contains("too large"), "{err}");
}

#[test]
fn chapter_text_lands_in_source_with_exact_ranges() {
    let bytes = book()
        .chapter(
            "one.xhtml",
            "<html><body><p>First chapter text.</p><p>Spaced   out\n text.</p></body></html>",
        )
        .build();
    let doc = epub::open_book(bytes).unwrap();

    assert_eq!(plain_text(&doc), "First chapter text.\nSpaced out text.");
    for block in &doc.blocks {
        if let BlockKind::Paragraph { spans } = &block.kind {
            for span in spans {
                assert!(span.is_verbatim(), "span text should slice the source");
            }
        }
    }
}

#[test]
fn chapter_breaks_sit_between_chapters() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .chapter("two.xhtml", "<html><body><p>Two.</p></body></html>")
        .chapter("three.xhtml", "<html><body><p>Three.</p></body></html>")
        .build();
    let doc = epub::open_book(bytes).unwrap();

    let breaks: Vec<usize> = doc
        .blocks
        .iter()
        .filter_map(|b| match b.kind {
            BlockKind::ChapterBreak { spine } => Some(spine),
            _ => None,
        })
        .collect();
    assert_eq!(breaks, [1, 2]);
    assert!(!matches!(
        doc.blocks.first().unwrap().kind,
        BlockKind::ChapterBreak { .. }
    ));
    assert!(!matches!(
        doc.blocks.last().unwrap().kind,
        BlockKind::ChapterBreak { .. }
    ));
}

#[test]
fn utf16_chapter_decodes() {
    let xhtml = "<html><body><p>Wide chapter.</p></body></html>";
    let mut wide = vec![0xFFu8, 0xFE];
    for unit in xhtml.encode_utf16() {
        wide.extend_from_slice(&unit.to_le_bytes());
    }
    let bytes = book().chapter_bytes("one.xhtml", wide).build();
    let doc = epub::open_book(bytes).unwrap();
    assert_eq!(plain_text(&doc), "Wide chapter.");
}

#[test]
fn code_in_an_opened_book_highlights() {
    let path = book()
        .chapter(
            "one.xhtml",
            "<html><body><pre><code class=\"language-rust\">fn main() {}\n</code></pre></body></html>",
        )
        .write_to("oryx_epub_highlight_test.epub");
    let opened = load::open(&path, None).unwrap();
    std::fs::remove_file(&path).ok();

    let code = opened
        .document
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::CodeBlock {
                language,
                highlights,
                ..
            } => Some((language.clone(), highlights.clone())),
            _ => None,
        })
        .expect("the book should hold a code block");
    assert_eq!(code.0.as_deref(), Some("rust"));
    assert!(
        code.1.first().is_some_and(|line| !line.is_empty()),
        "the first line should carry highlight spans"
    );
}

#[test]
fn detect_answers_epub() {
    assert_eq!(
        load::detect(std::path::Path::new("book.epub")),
        FileKind::Epub
    );
    assert_eq!(
        load::detect(std::path::Path::new("BOOK.EPUB")),
        FileKind::Epub
    );
}

#[test]
fn open_yields_both_chapters_in_order() {
    let path = book()
        .chapter(
            "one.xhtml",
            "<html><head><title>ignore me</title></head><body><p>First chapter text.</p></body></html>",
        )
        .chapter("two.xhtml", "<html><body><p>Second chapter text.</p></body></html>")
        .write_to("oryx_epub_open_test.epub");
    let opened = load::open(&path, None).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(
        plain_text(&opened.document),
        "First chapter text.\nSecond chapter text."
    );
    assert_eq!(opened.document.title.as_deref(), Some("Test Book"));
    assert!(!opened.streamed);
}
