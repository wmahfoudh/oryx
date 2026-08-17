//! Kindle books through the loader: KF8 parts as chapters, MOBI6
//! pagebreak splitting, links and anchors, images by reference, the
//! dual file, encodings, and refusals.

#[path = "../palmbook/tests/fixtures/writer.rs"]
mod writer;

#[path = "fixtures/epub_common.rs"]
mod epub_common;

use oryx::doc::epub;
use oryx::doc::kindle;
use oryx::doc::load::{self, FileKind};
use oryx::doc::model::{BlockKind, Document};
use writer::{IndxEntry, Skeleton};

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

/// Two KF8 parts: a heading and a linking paragraph in the first, the
/// link's landing place in the second.
fn kf8_fixture() -> writer::BookBuilder {
    writer::kf8_book(
        &[
            Skeleton {
                text: "<html><head></head><body></body></html>",
                fragments: vec![
                    (26, "<h1>Chapter One</h1>"),
                    (
                        46,
                        "<p>First text with a \
                         <a href=\"kindle:pos:fid:0002:off:0000000000\">jump</a>.</p>",
                    ),
                ],
            },
            Skeleton {
                text: "<html><head></head><body></body></html>",
                fragments: vec![(26, "<h1>Chapter Two</h1><p>Landing text.</p>")],
            },
        ],
        "h1 { font-style: italic }",
    )
}

#[test]
fn a_kf8_book_opens_with_chapters() {
    let bytes = kf8_fixture().build();
    let opened = kindle::open_book(bytes).unwrap();
    let doc = &opened.document;
    assert_eq!(doc.title.as_deref(), Some("Test Book"));
    let text = plain_text(doc);
    assert!(text.contains("Chapter One"), "{text:?}");
    assert!(text.contains("First text"), "{text:?}");
    assert!(text.contains("Chapter Two"), "{text:?}");
    let headings = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .count();
    assert_eq!(headings, 2, "each part keeps its heading");
}

#[test]
fn a_kindle_pos_link_lands_on_its_fragment() {
    let bytes = kf8_fixture().build();
    let doc = kindle::open_book(bytes).unwrap().document;
    let link = doc
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Paragraph { spans } => spans.iter().find_map(|s| s.link.clone()),
            _ => None,
        })
        .expect("the jump is a link span");
    let target = link.strip_prefix("book:").expect("a book link");
    let (path, fragment) = target.split_once('#').expect("an anchored target");
    assert_eq!(fragment, "fid2");
    let offset = epub::resolve_target(&doc, path, Some(fragment)).expect("the anchor resolves");
    let landing = doc.source.find("Chapter Two").unwrap();
    assert!(
        offset <= landing && landing - offset < 30,
        "the link lands at its fragment: anchor {offset}, text {landing}"
    );
}

#[test]
fn the_kf8_outline_reads_from_the_ncx() {
    let (cncx_record, offsets) = writer::cncx(&["One", "Two"]);
    let ncx = writer::indx_records(
        &[
            (3, 1, 0x01, 0),
            (6, 2, 0x02, 0),
            (21, 1, 0x04, 0),
            (0, 0, 0, 1),
        ],
        &[
            IndxEntry {
                name: b"000".to_vec(),
                tags: vec![(3, vec![offsets[0]]), (6, vec![0, 0])],
            },
            IndxEntry {
                name: b"001".to_vec(),
                tags: vec![(3, vec![offsets[1]]), (6, vec![2, 0])],
            },
        ],
        Some(cncx_record),
    );
    let mut builder = kf8_fixture();
    builder.ncxidx = builder.extra_base() + builder.extra_records.len() as u32;
    builder.extra_records.extend(ncx);
    let opened = kindle::open_book(builder.build()).unwrap();

    assert_eq!(opened.toc.len(), 2);
    assert_eq!(opened.toc[0].label, "One");
    assert_eq!(opened.toc[1].label, "Two");
    let one = epub::resolve_target(
        &opened.document,
        &opened.toc[0].path,
        opened.toc[0].fragment.as_deref(),
    )
    .expect("the first entry resolves");
    let two = epub::resolve_target(
        &opened.document,
        &opened.toc[1].path,
        opened.toc[1].fragment.as_deref(),
    )
    .expect("the second entry resolves");
    assert!(one < two, "outline targets sit in document order");
}

#[test]
fn a_kf8_image_arrives_by_embed_reference() {
    let mut builder = writer::kf8_book(
        &[Skeleton {
            text: "<html><head></head><body></body></html>",
            fragments: vec![(
                26,
                "<p><img src=\"kindle:embed:0001?mime=image/png\" alt=\"a duchess\"/></p>",
            )],
        }],
        "",
    );
    builder.first_image = Some(builder.extra_base() + builder.extra_records.len() as u32);
    builder.extra_records.push(epub_common::png_bytes(8, 4));
    let opened = kindle::open_book(builder.build()).unwrap();

    assert_eq!(opened.images.len(), 1);
    let (key, image) = &opened.images[0];
    assert_eq!(key, "res1");
    assert_eq!(image.dimensions(), (8, 4));
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
    assert_eq!(span.text(&opened.document.source), "a duchess");
}

#[test]
fn a_mobi6_book_splits_at_pagebreaks_and_links_by_filepos() {
    let head = "<html><body><p>Alpha text ";
    let link_len = "<a filepos=0000000000>go</a>".len();
    let mid = "</p><mbp:pagebreak/>";
    let target = head.len() + link_len + mid.len();
    let text = format!(
        "{head}<a filepos={target:010}>go</a>{mid}\
         <h1>Beta</h1><p>Beta text with an image \
         <img recindex=\"00001\" alt=\"a grill\"/></p></body></html>"
    );
    let mut builder = writer::book(&text);
    builder.extra_records.push(epub_common::png_bytes(8, 4));
    let opened = kindle::open_book(builder.build()).unwrap();
    let doc = &opened.document;

    let text = plain_text(doc);
    assert!(text.contains("Alpha text"), "{text:?}");
    assert!(text.contains("Beta text"), "{text:?}");
    let link = doc
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Paragraph { spans } => spans.iter().find_map(|s| s.link.clone()),
            _ => None,
        })
        .expect("the filepos link is a link span");
    let target = link.strip_prefix("book:").expect("a book link");
    let (path, fragment) = target.split_once('#').expect("an anchored target");
    let offset = epub::resolve_target(doc, path, Some(fragment)).expect("the anchor resolves");
    let landing = doc.source.find("Beta").unwrap();
    assert!(
        offset <= landing && landing - offset < 30,
        "filepos lands at its tag: anchor {offset}, text {landing}"
    );

    assert_eq!(opened.images.len(), 1);
    assert_eq!(opened.images[0].0, "res1");
    assert_eq!(opened.images[0].1.dimensions(), (8, 4));
}

#[test]
fn a_cp1252_book_decodes_its_punctuation() {
    let mut builder = writer::book("");
    builder.encoding = 1252;
    builder.text = b"<html><body><p>It\x92s a quote.</p></body></html>".to_vec();
    let doc = kindle::open_book(builder.build()).unwrap().document;
    assert!(
        doc.source.contains("It\u{2019}s a quote."),
        "the 1252 apostrophe decodes: {:?}",
        doc.source
    );
}

#[test]
fn a_dual_file_reads_its_kf8_half() {
    let boundary = {
        let builder = writer::book("<html><body><p>Old flow.</p></body></html>");
        builder.records().len() as u32 + 1
    };
    let mut builder = writer::book("<html><body><p>Old flow.</p></body></html>");
    builder.exth = vec![(121, boundary.to_be_bytes().to_vec())];
    let mut records = builder.records();
    records.push(b"BOUNDARY".to_vec());
    records.extend(kf8_fixture().records());
    let bytes = writer::pdb("dual", b"BOOK", b"MOBI", &records);

    let doc = kindle::open_book(bytes).unwrap().document;
    let text = plain_text(&doc);
    assert!(
        text.contains("Chapter One") && !text.contains("Old flow"),
        "the KF8 half wins: {text:?}"
    );
}

#[test]
fn a_drm_kindle_refuses_plainly() {
    let mut builder = writer::book("locked");
    builder.encryption = 2;
    let err = kindle::open_book(builder.build()).unwrap_err();
    assert!(err.to_string().contains("DRM"), "{err}");
}

#[test]
fn a_damaged_kindle_says_so() {
    let bytes = writer::book("a book cut short mid-download").build();
    let err = kindle::open_book(bytes[..bytes.len() - 40].to_vec()).unwrap_err();
    assert!(
        err.to_string().contains("damaged"),
        "a truncated book reads as damaged, not as a foreign file: {err}"
    );
    let err = kindle::open_book(b"just some text".to_vec()).unwrap_err();
    assert!(
        err.to_string().contains("not a readable"),
        "a foreign file stays a foreign file: {err}"
    );
}

#[test]
fn detection_covers_the_kindle_family() {
    use std::path::Path;
    assert_eq!(load::detect(Path::new("b.mobi")), FileKind::Kindle);
    assert_eq!(load::detect(Path::new("B.AZW3")), FileKind::Kindle);
    assert_eq!(load::detect(Path::new("b.azw")), FileKind::Kindle);
    let exts = load::recognized_extensions();
    assert!(exts.contains(&"mobi") && exts.contains(&"azw3") && exts.contains(&"azw"));
}

#[test]
fn a_kindle_file_opens_through_load() {
    let mut builder = kf8_fixture();
    builder.exth = vec![(113, b"B0TEST99".to_vec())];
    let bytes = builder.build();
    let path = std::env::temp_dir().join(format!("oryx-kindle-load-{}.azw3", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let opened = load::open(&path, None).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(opened.document.title.as_deref(), Some("Test Book"));
    assert!(plain_text(&opened.document).contains("Chapter One"));
    assert_eq!(
        opened.document.book_id.as_deref(),
        Some("B0TEST99|azw3"),
        "the position key carries the container, so the MOBI twin of \
         this book keeps its own place"
    );
}

/// Stage probe for real Kindle books; run with
/// ORYX_BOOK=<path> cargo test --release --test kindle field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn field_probe() {
    use std::time::Instant;
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let bytes = std::fs::read(&path).unwrap();
    let size = bytes.len();

    let t = Instant::now();
    let (document, toc, job) = kindle::open_prefix(bytes.clone()).unwrap();
    let t_prefix = t.elapsed();

    let t = Instant::now();
    let book = kindle::open_book(bytes).unwrap();
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
        "whole book: {}ms, {} blocks, {} chars of source, {} images decoded, title {:?}",
        t_whole.as_millis(),
        book.document.blocks.len(),
        book.document.source.len(),
        book.images.len(),
        book.document.title
    );
    for entry in book.toc.iter().take(10) {
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
