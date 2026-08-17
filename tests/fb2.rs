//! FB2 reading: the element mapping, the outline, note links, binary
//! images, the windows-1251 encoding, the zip wrappers, and refusals.

#[path = "fixtures/epub_common.rs"]
mod epub_common;

use oryx::doc::epub;
use oryx::doc::fb2;
use oryx::doc::load::{self, FileKind};
use oryx::doc::model::{BlockKind, Document};

/// A minimal FictionBook around the given bodies and binaries.
fn book(bodies: &str, binaries: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
         xmlns:l=\"http://www.w3.org/1999/xlink\">\n\
         <description><title-info><book-title>Test Book</book-title></title-info>\n\
         <document-info><id>fb2-test-1</id></document-info></description>\n\
         {bodies}\n{binaries}</FictionBook>"
    )
    .into_bytes()
}

fn plain_text(doc: &Document) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        let spans = match &block.kind {
            BlockKind::Paragraph { spans } => spans,
            BlockKind::Heading { spans, .. } => spans,
            _ => continue,
        };
        for span in spans {
            out.push_str(span.text(&doc.source));
        }
        out.push('\n');
    }
    out
}

#[test]
fn sections_become_chapters_and_the_outline() {
    let bytes = book(
        "<body><title><p>The Book</p></title>\
         <section><title><p>One</p></title><p>First chapter text.</p>\
         <section id=\"part2\"><title><p>Inside One</p></title><p>Nested text.</p></section>\
         </section>\
         <section><title><p>Two</p></title><p>Second chapter text.</p></section></body>",
        "",
    );
    let opened = fb2::open_book(bytes).unwrap();
    let doc = &opened.document;
    assert_eq!(doc.title.as_deref(), Some("Test Book"));

    let headings: Vec<(u8, String)> = doc
        .blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Heading { level, spans, .. } => {
                Some((*level, spans.iter().map(|s| s.text(&doc.source)).collect()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        headings,
        [
            (1, "The Book".to_string()),
            (1, "One".to_string()),
            (2, "Inside One".to_string()),
            (1, "Two".to_string()),
        ],
        "the body title and each section title become headings by depth"
    );

    let toc: Vec<(String, u8, Option<String>)> = opened
        .toc
        .iter()
        .map(|e| (e.label.clone(), e.depth, e.fragment.clone()))
        .collect();
    assert_eq!(toc.len(), 3, "titled sections make the outline: {toc:?}");
    assert_eq!(toc[0], ("One".to_string(), 0, None));
    assert_eq!(
        toc[1],
        ("Inside One".to_string(), 1, Some("part2".to_string())),
        "a nested section targets its own anchor"
    );
    assert_eq!(toc[2], ("Two".to_string(), 0, None));

    let inside = epub::resolve_target(doc, &opened.toc[1].path, opened.toc[1].fragment.as_deref())
        .expect("the nested entry resolves");
    let two = epub::resolve_target(doc, &opened.toc[2].path, None).expect("chapter two resolves");
    assert!(inside < two, "outline targets sit in document order");
    assert!(inside > 0, "the nested target is past the start");
}

#[test]
fn inline_styles_map() {
    let bytes = book(
        "<body><section><p>plain <emphasis>lean</emphasis> <strong>loud</strong> \
         <strikethrough>gone</strikethrough> <code>mono</code></p></section></body>",
        "",
    );
    let doc = fb2::open_book(bytes).unwrap().document;
    let spans = doc
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Paragraph { spans } => Some(spans),
            _ => None,
        })
        .expect("the paragraph is there");
    let styled = |needle: &str| {
        spans
            .iter()
            .find(|s| s.text(&doc.source).contains(needle))
            .unwrap_or_else(|| panic!("no span holds {needle:?}"))
    };
    assert!(styled("lean").italic, "emphasis reads italic");
    assert!(styled("loud").bold, "strong reads bold");
    assert!(styled("gone").strike, "strikethrough reads struck");
    assert!(styled("mono").code, "code reads in the code face");
    assert!(!styled("plain").italic && !styled("plain").bold);
}

#[test]
fn poems_epigraphs_and_subtitles_map() {
    let bytes = book(
        "<body><section><title><p>Verse</p></title>\
         <epigraph><p>An opening thought.</p></epigraph>\
         <subtitle>Part the first</subtitle>\
         <poem><stanza><v>First line of verse</v><v>Second line of verse</v></stanza></poem>\
         <cite><p>A quoted passage.</p></cite>\
         <p>Prose after.</p></section></body>",
        "",
    );
    let doc = fb2::open_book(bytes).unwrap().document;
    let quoted = |needle: &str| {
        doc.blocks
            .iter()
            .find(|b| match &b.kind {
                BlockKind::Paragraph { spans } => {
                    spans.iter().any(|s| s.text(&doc.source).contains(needle))
                }
                _ => false,
            })
            .unwrap_or_else(|| panic!("no paragraph holds {needle:?}"))
    };
    assert!(
        quoted("An opening thought.").quote_depth > 0,
        "an epigraph reads as a quote"
    );
    assert!(
        quoted("First line of verse").quote_depth > 0,
        "a poem reads as a quote"
    );
    assert!(
        quoted("A quoted passage.").quote_depth > 0,
        "a citation reads as a quote"
    );
    assert_eq!(
        quoted("Prose after.").quote_depth,
        0,
        "prose after the quotes is plain"
    );

    let verse = quoted("First line of verse");
    let BlockKind::Paragraph { spans } = &verse.kind else {
        unreachable!()
    };
    let text: String = spans.iter().map(|s| s.text(&doc.source)).collect();
    assert!(
        text.contains("First line of verse") && text.contains("Second line of verse"),
        "the stanza's verses share one paragraph: {text:?}"
    );

    let BlockKind::Paragraph { spans } = &quoted("Part the first").kind else {
        unreachable!()
    };
    assert!(
        spans
            .iter()
            .find(|s| s.text(&doc.source).contains("Part the first"))
            .unwrap()
            .bold,
        "a subtitle reads bold"
    );
}

#[test]
fn a_note_link_round_trips() {
    let bytes = book(
        "<body><section><title><p>One</p></title>\
         <p>A claim<a l:href=\"#n1\" type=\"note\">1</a> in prose.</p></section></body>\
         <body name=\"notes\"><title><p>Notes</p></title>\
         <section id=\"n1\"><title><p>1</p></title><p>The note text.</p></section></body>",
        "",
    );
    let doc = fb2::open_book(bytes).unwrap().document;
    assert!(
        plain_text(&doc).contains("The note text."),
        "the notes body lands in the document"
    );
    let link = doc
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Paragraph { spans } => spans.iter().find_map(|s| s.link.clone()),
            _ => None,
        })
        .expect("the note link is a link span");
    let target = link
        .strip_prefix("book:")
        .expect("a note link targets the book");
    let (path, fragment) = target.split_once('#').expect("the target has a fragment");
    assert_eq!(fragment, "n1");
    let offset =
        epub::resolve_target(&doc, path, Some(fragment)).expect("the note anchor resolves");
    let note_pos = doc.source.find("The note text.").unwrap();
    assert!(
        offset <= note_pos && note_pos - offset < 40,
        "the anchor sits at the note, offset {offset} against text at {note_pos}"
    );
}

#[test]
fn a_binary_image_arrives_with_dimensions() {
    use base64::Engine;
    let png = epub_common::png_bytes(8, 4);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    let bytes = book(
        "<body><section><title><p>One</p></title>\
         <p>Before the picture.</p><image l:href=\"#pic\"/></section></body>",
        &format!("<binary id=\"pic\" content-type=\"image/png\">{encoded}</binary>"),
    );

    let opened = fb2::open_book(bytes.clone()).unwrap();
    assert_eq!(opened.images.len(), 1);
    let (key, img) = &opened.images[0];
    assert_eq!(key, "pic");
    assert_eq!(img.dimensions(), (8, 4));
    let span = opened
        .document
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Paragraph { spans } => spans.iter().find(|s| s.image.is_some()),
            _ => None,
        })
        .expect("the image span is in the document");
    assert_eq!(span.image.as_ref().unwrap().src, *key);

    let (_, _, job) = fb2::open_prefix(bytes).unwrap();
    let mut job = job.expect("a book with images carries a job");
    let sources = job.take_sources();
    let entry = sources.iter().find(|(k, _, _)| k == "pic").unwrap();
    assert_eq!(
        entry.2,
        Some((8, 4)),
        "the header dimensions ride the source entry"
    );
}

#[test]
fn windows_1251_decodes() {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(
        b"<?xml version=\"1.0\" encoding=\"windows-1251\"?>\n\
          <FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">\n\
          <description><title-info><book-title>",
    );
    // "Привет" in windows-1251.
    bytes.extend_from_slice(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
    bytes.extend_from_slice(b"</book-title></title-info></description><body><section><p>");
    bytes.extend_from_slice(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
    bytes.extend_from_slice(b", world.</p></section></body></FictionBook>");

    let doc = fb2::open_book(bytes).unwrap().document;
    assert_eq!(doc.title.as_deref(), Some("Привет"));
    assert!(
        doc.source.contains("Привет, world."),
        "the 1251 body decodes: {:?}",
        &doc.source
    );
}

#[test]
fn an_fb2_zip_unwraps() {
    use std::io::Write;
    let inner = book(
        "<body><section><title><p>One</p></title><p>Wrapped text.</p></section></body>",
        "",
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("book.fb2", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&inner).unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    let doc = fb2::open_book(bytes).unwrap().document;
    assert_eq!(doc.title.as_deref(), Some("Test Book"));
    assert!(plain_text(&doc).contains("Wrapped text."));
}

#[test]
fn refusals_speak_plainly() {
    let malformed = fb2::open_book(b"<FictionBook><body>".to_vec()).unwrap_err();
    assert!(malformed.to_string().contains("FB2"), "{malformed}");

    let not_fb2 =
        fb2::open_book(b"<?xml version=\"1.0\"?><html><body/></html>".to_vec()).unwrap_err();
    assert!(not_fb2.to_string().contains("FB2"), "{not_fb2}");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, b"nothing here").unwrap();
    let empty_zip = fb2::open_book(zip.finish().unwrap().into_inner()).unwrap_err();
    assert!(empty_zip.to_string().contains("FB2"), "{empty_zip}");
}

#[test]
fn detection_covers_the_fb2_family() {
    use std::path::Path;
    assert_eq!(load::detect(Path::new("book.fb2")), FileKind::Fb2);
    assert_eq!(load::detect(Path::new("BOOK.FB2")), FileKind::Fb2);
    assert_eq!(load::detect(Path::new("book.fbz")), FileKind::Fb2);
    assert_eq!(load::detect(Path::new("book.fb2.zip")), FileKind::Fb2);
    assert_eq!(load::detect(Path::new("book.zip")), FileKind::Unknown);
    let exts = load::recognized_extensions();
    assert!(exts.contains(&"fb2") && exts.contains(&"fbz"));
}

/// Stage-timing probe for real books; run with
/// ORYX_BOOK=<path> cargo test --release --test fb2 field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn field_probe() {
    use std::time::Instant;
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let bytes = std::fs::read(&path).unwrap();
    let size = bytes.len();

    let t = Instant::now();
    let (document, toc, job) = fb2::open_prefix(bytes.clone()).unwrap();
    let t_prefix = t.elapsed();

    let t = Instant::now();
    let book = fb2::open_book(bytes).unwrap();
    let t_whole = t.elapsed();

    println!("{path}: {size} bytes");
    println!(
        "prefix: {}ms, {} blocks, {} toc entries, job {}",
        t_prefix.as_millis(),
        document.blocks.len(),
        toc.len(),
        if job.is_some() { "carried" } else { "none" }
    );
    println!(
        "whole book: {}ms, {} blocks, {} chars of source, {} images decoded",
        t_whole.as_millis(),
        book.document.blocks.len(),
        book.document.source.len(),
        book.images.len()
    );
    for (key, img) in &book.images {
        println!("  image {key}: {}x{}", img.dimensions().0, img.dimensions().1);
    }
    for entry in book.toc.iter().take(12) {
        println!(
            "  toc {}{} -> {}{}",
            "  ".repeat(entry.depth as usize),
            entry.label,
            entry.path,
            entry
                .fragment
                .as_deref()
                .map(|f| format!("#{f}"))
                .unwrap_or_default()
        );
    }
}

#[test]
fn an_fb2_file_opens_through_load() {
    let bytes = book(
        "<body><section><title><p>One</p></title><p>Loaded text.</p></section></body>",
        "",
    );
    let path = std::env::temp_dir().join(format!("oryx-fb2-load-{}.fb2", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let opened = load::open(&path, None).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(opened.document.title.as_deref(), Some("Test Book"));
    assert_eq!(opened.toc.len(), 1);
    assert_eq!(
        opened.document.book_id.as_deref(),
        Some("fb2-test-1"),
        "the document id keys position memory"
    );
    assert!(plain_text(&opened.document).contains("Loaded text."));
}
