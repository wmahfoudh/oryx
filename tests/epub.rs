//! EPUB package parsing and the plain-chapter pipeline, driven through
//! books assembled in memory by the fixture builder.

#[path = "fixtures/epub_common.rs"]
mod epub_common;

use epub_common::{book, png_bytes, zip_declaring};

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
    let doc = epub::open_book(bytes).unwrap().document;

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
    let doc = epub::open_book(bytes).unwrap().document;

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
    let doc = epub::open_book(bytes).unwrap().document;
    assert_eq!(plain_text(&doc), "Wide chapter.");
}

#[test]
fn a_chapter_image_resolves_and_decodes() {
    let bytes = book()
        .image("images/pic.png", png_bytes(8, 4))
        .chapter(
            "one.xhtml",
            "<html><body><p><img src=\"../images/pic.png\" alt=\"a duchess\"/></p></body></html>",
        )
        .build();
    let opened = epub::open_book(bytes).unwrap();

    assert_eq!(opened.images.len(), 1);
    let (key, img) = &opened.images[0];
    assert_eq!(key, "OEBPS/images/pic.png");
    assert_eq!(img.dimensions(), (8, 4));

    let BlockKind::Paragraph { spans } = &opened.document.blocks[0].kind else {
        panic!(
            "expected a paragraph, got {:?}",
            opened.document.blocks[0].kind
        );
    };
    let span = spans.iter().find(|s| s.image.is_some()).unwrap();
    assert_eq!(span.image.as_ref().unwrap().src, *key);
    assert_eq!(span.text(&opened.document.source), "a duchess");
}

#[test]
fn a_missing_image_keeps_its_span_and_no_pixels() {
    let bytes = book()
        .chapter(
            "one.xhtml",
            "<html><body><p><img src=\"../images/gone.png\" alt=\"lost\"/></p></body></html>",
        )
        .build();
    let opened = epub::open_book(bytes).unwrap();

    assert!(opened.images.is_empty());
    let BlockKind::Paragraph { spans } = &opened.document.blocks[0].kind else {
        panic!(
            "expected a paragraph, got {:?}",
            opened.document.blocks[0].kind
        );
    };
    let span = spans.iter().find(|s| s.image.is_some()).unwrap();
    assert_eq!(span.image.as_ref().unwrap().src, "OEBPS/images/gone.png");
    assert_eq!(span.text(&opened.document.source), "lost");
}

#[test]
fn an_inline_svg_rasterizes() {
    let bytes = book()
        .chapter(
            "one.xhtml",
            "<html><body><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60\" height=\"30\">\
             <rect width=\"60\" height=\"30\" fill=\"#c87137\"/></svg></body></html>",
        )
        .build();
    let opened = epub::open_book(bytes).unwrap();

    assert_eq!(opened.images.len(), 1);
    let (key, img) = &opened.images[0];
    assert!(key.starts_with("svg:"), "synthetic key, got {key:?}");
    assert_eq!(img.dimensions(), (60, 30));
    let px = img.get_pixel(30, 15);
    assert_eq!((px[0], px[1], px[2]), (0xC8, 0x71, 0x37));
}

#[test]
fn an_svg_cover_wrapper_inlines_its_archive_image() {
    let bytes = book()
        .image("images/cover.png", png_bytes(60, 30))
        .chapter(
            "cover.xhtml",
            "<html><body><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60\" height=\"30\">\
             <image href=\"../images/cover.png\" width=\"60\" height=\"30\"/></svg></body></html>",
        )
        .build();
    let opened = epub::open_book(bytes).unwrap();

    let (key, img) = opened
        .images
        .iter()
        .find(|(k, _)| k.starts_with("svg:"))
        .expect("the cover svg should rasterize");
    assert_eq!(img.dimensions(), (60, 30));
    let px = img.get_pixel(30, 15);
    assert_eq!(
        (px[0], px[1], px[2]),
        (10, 20, 30),
        "the archive raster should show through {key}"
    );
}

#[test]
fn manifest_css_gives_books_their_italics() {
    let bytes = book()
        .stylesheet(".i { font-style: italic }")
        .chapter(
            "one.xhtml",
            "<html><body><p><span class=\"i\">Curiouser and curiouser!</span></p></body></html>",
        )
        .build();
    let doc = epub::open_book(bytes).unwrap().document;
    let BlockKind::Paragraph { spans } = &doc.blocks[0].kind else {
        panic!("expected a paragraph, got {:?}", doc.blocks[0].kind);
    };
    assert!(
        spans[0].italic,
        "the class-styled dialogue should stay italic"
    );
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

type SeenImages = std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>;

fn collecting_sink() -> (oryx::doc::images::ImageSink, SeenImages) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let feed = std::sync::Arc::clone(&seen);
    let sink: oryx::doc::images::ImageSink = std::sync::Arc::new(move |key, image: Option<_>| {
        feed.lock().unwrap().push((key, image.is_some()));
    });
    (sink, seen)
}

fn six_fat_chapters() -> Vec<u8> {
    let para = format!("<p>{}</p>", "word ".repeat(8000));
    let mut b = book();
    for i in 0..6 {
        b = b.chapter(
            &format!("c{i}.xhtml"),
            &format!("<html><body>{para}</body></html>"),
        );
    }
    b.build()
}

#[test]
fn the_prefix_takes_whole_chapters_past_the_target() {
    let (doc, _, job) = epub::open_prefix(six_fat_chapters()).unwrap();
    let job = job.expect("a big book leaves a continuation");
    assert!(job.has_chapters(), "chapters remain for the worker");
    assert!(
        doc.source.len() >= 128 * 1024,
        "the prefix crosses the target, got {}",
        doc.source.len()
    );
    let paragraphs = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .count();
    assert!(
        paragraphs < 6,
        "the prefix must not hold the whole book, got {paragraphs} paragraphs"
    );
}

#[test]
fn the_delivery_extends_the_prefix_bit_for_bit() {
    let (doc, _, job) = epub::open_prefix(six_fat_chapters()).unwrap();
    let job = job.expect("a continuation");
    let (sink, _) = collecting_sink();
    let delivered = epub::run(job, &|| false, sink).expect("an unbailed run delivers");

    let full = delivered.source.expect("a book delivery swaps the source");
    assert!(
        full.as_bytes().starts_with(doc.source.as_bytes()),
        "the delivered source must begin with the prefix bytes"
    );
    match oryx::doc::stream::swap(&doc.blocks, delivered.blocks) {
        oryx::doc::stream::Swap::Splice(tail) => {
            assert!(!tail.is_empty(), "the tail holds the remaining chapters")
        }
        oryx::doc::stream::Swap::Replace(_) => panic!("a book delivery must splice, never replace"),
    }
}

#[test]
fn images_decode_through_the_sink_not_at_open() {
    let bytes = book()
        .image("images/one.png", png_bytes(8, 4))
        .image("images/two.png", png_bytes(6, 3))
        .chapter(
            "one.xhtml",
            "<html><body><p><img src=\"../images/one.png\"/><img src=\"../images/two.png\"/></p></body></html>",
        )
        .build();
    let (_, _, job) = epub::open_prefix(bytes).unwrap();
    let mut job = job.expect("images leave a decode job");
    assert!(
        !job.has_chapters(),
        "a one-chapter book walks whole at open"
    );

    let (sink, seen) = collecting_sink();
    epub::spawn_decodes(job.take_jobs(), sink);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while seen.lock().unwrap().len() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "both images arrive through the sink");
    assert!(seen.iter().all(|(_, decoded)| *decoded));
    assert!(seen.iter().any(|(k, _)| k == "OEBPS/images/one.png"));
}

#[test]
fn a_bailed_run_delivers_nothing() {
    let (_, _, job) = epub::open_prefix(six_fat_chapters()).unwrap();
    let (sink, _) = collecting_sink();
    assert!(epub::run(job.unwrap(), &|| true, sink).is_none());
}

#[test]
fn toc_reads_the_nav_document() {
    let bytes = book()
        .nav_doc(
            "<html xmlns:epub=\"http://www.idpf.org/2007/ops\"><body><nav epub:type=\"toc\"><ol>\
             <li><a href=\"text/one.xhtml\">One</a></li>\
             <li><a href=\"text/two.xhtml\">Two</a><ol>\
             <li><a href=\"text/two.xhtml#deep\">Deep</a></li></ol></li>\
             </ol></nav></body></html>",
        )
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .chapter("two.xhtml", "<html><body><p>Two.</p></body></html>")
        .build();
    let mut archive = Archive::open(bytes).unwrap();
    let package = epub::read_package(&mut archive).unwrap();
    let toc = epub::read_toc(&mut archive, &package);

    let shape: Vec<(&str, u8, &str, Option<&str>)> = toc
        .iter()
        .map(|e| {
            (
                e.label.as_str(),
                e.depth,
                e.path.as_str(),
                e.fragment.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("One", 0, "OEBPS/text/one.xhtml", None),
            ("Two", 0, "OEBPS/text/two.xhtml", None),
            ("Deep", 1, "OEBPS/text/two.xhtml", Some("deep")),
        ]
    );
}

#[test]
fn toc_falls_back_to_the_ncx() {
    let bytes = book()
        .ncx(
            "<?xml version=\"1.0\"?><ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\"><navMap>\
             <navPoint><navLabel><text>Start</text></navLabel><content src=\"text/one.xhtml\"/>\
             <navPoint><navLabel><text>Inner</text></navLabel><content src=\"text/one.xhtml#i\"/></navPoint>\
             </navPoint></navMap></ncx>",
        )
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .build();
    let mut archive = Archive::open(bytes).unwrap();
    let package = epub::read_package(&mut archive).unwrap();
    let toc = epub::read_toc(&mut archive, &package);

    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].label, "Start");
    assert_eq!(toc[0].depth, 0);
    assert_eq!(toc[1].label, "Inner");
    assert_eq!(toc[1].depth, 1);
    assert_eq!(toc[1].fragment.as_deref(), Some("i"));
}

#[test]
fn anchors_cover_chapters_and_ids() {
    let bytes = book()
        .chapter(
            "one.xhtml",
            "<html><body><p>Opening text.</p><p id=\"f1\">Note text here.</p></body></html>",
        )
        .chapter("two.xhtml", "<html><body><p>Second.</p></body></html>")
        .build();
    let doc = epub::open_book(bytes).unwrap().document;

    assert_eq!(doc.anchors.get("OEBPS/text/one.xhtml"), Some(&0));
    let note = *doc
        .anchors
        .get("OEBPS/text/one.xhtml#f1")
        .expect("the id is recorded");
    assert!(
        doc.source[note..].starts_with("Note text here."),
        "the anchor lands on its element's text, got {:?}",
        &doc.source[note..note.saturating_add(20).min(doc.source.len())]
    );
    let second = *doc.anchors.get("OEBPS/text/two.xhtml").unwrap();
    assert!(doc.source[second..].starts_with("Second."));
}

#[test]
fn internal_links_carry_book_targets() {
    let bytes = book()
        .chapter(
            "one.xhtml",
            "<html><body><p><a href=\"two.xhtml#x\">note</a> and <a href=\"gone.xhtml\">dead</a></p></body></html>",
        )
        .chapter("two.xhtml", "<html><body><p id=\"x\">Target.</p></body></html>")
        .build();
    let doc = epub::open_book(bytes).unwrap().document;

    let BlockKind::Paragraph { spans } = &doc.blocks[0].kind else {
        panic!("expected a paragraph, got {:?}", doc.blocks[0].kind);
    };
    let note = spans
        .iter()
        .find(|s| s.text(&doc.source) == "note")
        .unwrap();
    assert_eq!(note.link.as_deref(), Some("book:OEBPS/text/two.xhtml#x"));
    assert!(
        spans
            .iter()
            .all(|s| s.link.is_none() || s.text(&doc.source) == "note"),
        "a link to a file outside the spine stays plain text"
    );
}

#[test]
fn fragment_misses_fall_back_to_the_chapter_start() {
    let bytes = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .chapter("two.xhtml", "<html><body><p>Two.</p></body></html>")
        .build();
    let doc = epub::open_book(bytes).unwrap().document;

    let start = *doc.anchors.get("OEBPS/text/two.xhtml").unwrap();
    assert_eq!(
        epub::resolve_target(&doc, "OEBPS/text/two.xhtml", Some("nope")),
        Some(start)
    );
    assert_eq!(
        epub::resolve_target(&doc, "OEBPS/text/gone.xhtml", None),
        None
    );
}

#[test]
fn book_id_prefers_identifier_and_falls_back_to_the_path() {
    let path = book()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .write_to("oryx_epub_id_test.epub");
    let opened = load::open(&path, None).unwrap();
    assert_eq!(opened.document.book_id.as_deref(), Some("urn:test:1"));

    let path2 = book()
        .no_identifier()
        .chapter("one.xhtml", "<html><body><p>One.</p></body></html>")
        .write_to("oryx_epub_noid_test.epub");
    let opened2 = load::open(&path2, None).unwrap();
    let canonical = path2.canonicalize().unwrap().display().to_string();
    assert_eq!(
        opened2.document.book_id.as_deref(),
        Some(canonical.as_str())
    );
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&path2).ok();
}

#[test]
fn positions_remember_prune_and_move_back() {
    use oryx::platform::config::Positions;
    let mut positions = Positions::default();
    for i in 0..105 {
        positions.remember(&format!("book-{i}"), i * 10);
    }
    assert_eq!(positions.lookup("book-0"), None, "the oldest are pruned");
    assert_eq!(positions.lookup("book-104"), Some(1040));
    positions.remember("book-10", 777);
    assert_eq!(positions.lookup("book-10"), Some(777));

    let path = std::env::temp_dir().join("oryx_positions_test.toml");
    positions.save_to(&path);
    let reloaded = Positions::load_from(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(reloaded.lookup("book-10"), Some(777));
    assert_eq!(reloaded.lookup("book-104"), Some(1040));
}

#[test]
fn the_outline_resolves_toc_entries_as_the_worker_delivers() {
    use oryx::ui::outline::OutlineTree;
    let para = format!("<p>{}</p>", "word ".repeat(8000));
    let mut b = book().nav_doc(
        "<html><body><nav epub:type=\"toc\"><ol>\
         <li><a href=\"text/c0.xhtml\">First</a></li>\
         <li><a href=\"text/c5.xhtml\">Last</a></li>\
         <li><a href=\"text/gone.xhtml\">Absent</a></li>\
         </ol></nav></body></html>",
    );
    for i in 0..6 {
        b = b.chapter(
            &format!("c{i}.xhtml"),
            &format!("<html><body>{para}</body></html>"),
        );
    }
    let bytes = b.build();

    let (doc, _, job) = epub::open_prefix(bytes.clone()).unwrap();
    let mut archive = Archive::open(bytes).unwrap();
    let package = epub::read_package(&mut archive).unwrap();
    let toc = epub::read_toc(&mut archive, &package);

    let mut tree = OutlineTree::from_toc(&toc, &doc);
    assert_eq!(tree.entries().len(), 3);
    assert_ne!(
        tree.entries()[0].block,
        usize::MAX,
        "the first chapter resolves"
    );
    assert_eq!(
        tree.entries()[1].block,
        usize::MAX,
        "the last chapter is not delivered yet"
    );

    let (sink, _) = collecting_sink();
    let delivered = epub::run(job.unwrap(), &|| false, sink).unwrap();
    let full = Document {
        blocks: delivered.blocks,
        source: delivered.source.unwrap(),
        details: delivered.details,
        anchors: delivered.anchors.into_iter().collect(),
        ..Document::default()
    };
    tree.re_resolve(&full);
    assert_ne!(tree.entries()[1].block, usize::MAX, "delivery resolves it");
    assert_eq!(
        tree.entries()[2].block,
        usize::MAX,
        "an absent file stays inert"
    );
}

#[test]
fn the_shipped_sherlock_opens_and_names_its_adventures() {
    let bytes = std::fs::read("examples/sherlock-holmes.epub").expect("the shipped book exists");
    let book = epub::open_book(bytes).unwrap();
    assert_eq!(
        book.document.title.as_deref(),
        Some("The Adventures of Sherlock Holmes")
    );
    let labels: Vec<&str> = book.toc.iter().map(|e| e.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("A Scandal in Bohemia")),
        "{labels:?}"
    );
    assert!(
        labels.len() >= 12,
        "twelve adventures at least, got {}",
        labels.len()
    );
    let breaks = book
        .document
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::ChapterBreak { .. }))
        .count();
    assert!(breaks >= 12, "chapter seams for the page breaks: {breaks}");
}

/// The product promise holds for books: the shipped Sherlock opens its
/// prefix inside the startup budget. The whole walk and the decode pool
/// ride the workers and never gate the first frame.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn the_shipped_book_meets_the_open_budget() {
    let bytes = std::fs::read("examples/sherlock-holmes.epub").expect("the shipped book exists");
    let t = std::time::Instant::now();
    let (doc, _, job) = epub::open_prefix(bytes).unwrap();
    let prefix_ms = t.elapsed().as_millis();
    println!(
        "sherlock: prefix {prefix_ms}ms, {} source bytes, worker owed: {}",
        doc.source.len(),
        job.is_some()
    );
    if !cfg!(debug_assertions) {
        assert!(
            prefix_ms < 100,
            "the prefix must open inside the budget: {prefix_ms}ms"
        );
    }
}

/// Temporary stage-timing probe for real books; run with
/// ORYX_BOOK=<path> cargo test --release --test epub timing_probe -- --ignored --nocapture
#[test]
#[ignore]
fn timing_probe() {
    use std::time::Instant;
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let bytes = std::fs::read(&path).unwrap();
    let size = bytes.len();

    let t = Instant::now();
    let mut archive = Archive::open(bytes.clone()).unwrap();
    let t_archive = t.elapsed();

    let t = Instant::now();
    let package = epub::read_package(&mut archive).unwrap();
    let t_package = t.elapsed();

    // Walk only: chapters through the walker, no image extraction.
    let t = Instant::now();
    let mut table = oryx::doc::html::EmphasisTable::default();
    for item in &package.manifest {
        if item.media_type.eq_ignore_ascii_case("text/css") {
            if let Some(css) = archive.read(&epub::resolve(&package.root, &item.href)) {
                table.add_css(&String::from_utf8_lossy(&css));
            }
        }
    }
    let t_css = t.elapsed();

    let t = Instant::now();
    let mut walker = oryx::doc::html::Walker::new();
    walker.set_emphasis(table);
    let mut chapter_bytes = 0usize;
    for &item in &package.spine {
        let href = epub::resolve(&package.root, &package.manifest[item].href);
        if let Some(bytes) = archive.read(&href) {
            chapter_bytes += bytes.len();
            walker.walk_chapter(&String::from_utf8_lossy(&bytes));
        }
    }
    let (blocks, source, _) = walker.finish();
    let t_walk = t.elapsed();

    // The full path, images included.
    let t = Instant::now();
    let book = epub::open_book(bytes.clone()).unwrap();
    let t_full = t.elapsed();

    // What the app now pays before the first frame.
    let t = Instant::now();
    let (prefix_doc, _, job) = epub::open_prefix(bytes).unwrap();
    let t_prefix = t.elapsed();
    let job_note = match &job {
        Some(j) if j.has_chapters() => "chapters remain",
        Some(_) => "decodes only",
        None => "nothing owed",
    };

    println!(
        "file: {size} bytes, {} chapters, {chapter_bytes} bytes of xhtml",
        package.spine.len()
    );
    println!(
        "model: {} blocks, {} bytes of source",
        blocks.len(),
        source.len()
    );
    println!("images: {} decoded", book.images.len());
    println!("archive open: {t_archive:?}");
    println!("package:      {t_package:?}");
    println!("css table:    {t_css:?}");
    println!("walk all:     {t_walk:?}");
    println!("full open_book (walk + images): {t_full:?}");
    println!(
        "open_prefix: {t_prefix:?} for {} source bytes ({job_note})",
        prefix_doc.source.len()
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
