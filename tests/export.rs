//! Exported PDFs read back through an independent parser, so the
//! assertions are what a reader sees rather than what we just wrote.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lopdf::Document as Pdf;

use oryx::doc::images::MediaCache;
use oryx::doc::markdown;
use oryx::doc::model::Document;
use oryx::export::paginate::{paginate, Paginator};
use oryx::export::{pdf, ExportPass, ExportSettings, Orientation, PageGeometry, PageSize};
use oryx::layout::{layout, layout_begin, layout_step, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

#[path = "fixtures/epub_common.rs"]
mod epub_common;

fn export_to_bytes(doc: &Document, page: PageSize) -> Vec<u8> {
    export_with(doc, page, false)
}

/// A book export: the same pass with the authored table of contents
/// driving the PDF outline.
fn export_book(book: &oryx::doc::epub::Book) -> Vec<u8> {
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    export_book_with(book, &mut media)
}

fn export_book_with(book: &oryx::doc::epub::Book, media: &mut MediaCache) -> Vec<u8> {
    let mut fonts = FontStore::new();
    let cfg = ViewConfig {
        body_size: 11.0,
        code_size: 9.0,
        zoom: 1.0,
        ..ViewConfig::default()
    };
    let theme = Theme::default_dark();
    let geometry = PageGeometry::new(PageSize::A4, Orientation::Portrait, cfg.body_size);
    let settings = ExportSettings {
        body_size: cfg.body_size,
        code_size: cfg.code_size,
        ..ExportSettings::default()
    };
    let doc = &book.document;
    let laid = layout(doc, &theme, &mut fonts, media, &cfg, geometry.width);
    let pages = paginate(doc, &laid, &geometry);
    let job = pdf::Job {
        doc,
        layout: &laid,
        theme: &theme,
        geometry: &geometry,
        settings: &settings,
        title: "test",
        toc: &book.toc,
    };
    pdf::build(&job, &pages, &mut fonts, media).expect("the export builds")
}

/// A cold book image, adopted as its stored source only, still embeds
/// as pixels: the export warms each image synchronously instead of
/// racing the decode pool.
#[test]
fn a_cold_book_image_exports_as_pixels() {
    let bytes = epub_common::book()
        .image("images/pic.png", epub_common::png_bytes(8, 4))
        .chapter(
            "one.xhtml",
            "<html><body><p><img src=\"../images/pic.png\"/></p></body></html>",
        )
        .build();
    let (_, _, job) = oryx::doc::epub::open_prefix(bytes.clone()).unwrap();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    media.adopt(job.expect("images leave a job").take_sources());
    let book = oryx::doc::epub::open_book(bytes).unwrap();
    let pdf = Pdf::load_mem(&export_book_with(&book, &mut media)).unwrap();
    assert!(
        image_xobjects(&pdf) >= 1,
        "the image embeds, not the placeholder"
    );
}

#[test]
fn book_chapters_start_on_fresh_pages() {
    let bytes = epub_common::book()
        .chapter(
            "one.xhtml",
            "<html><body><p>Alpha chapter text.</p></body></html>",
        )
        .chapter(
            "two.xhtml",
            "<html><body><p>Beta chapter text.</p></body></html>",
        )
        .chapter(
            "three.xhtml",
            "<html><body><p>Gamma chapter text.</p></body></html>",
        )
        .build();
    let book = oryx::doc::epub::open_book(bytes).unwrap();
    let pdf = Pdf::load_mem(&export_to_bytes(&book.document, PageSize::A4)).unwrap();
    assert_eq!(pdf.get_pages().len(), 3, "one page per chapter");
    let first = pdf.extract_text(&[1]).unwrap();
    assert!(
        first.contains("Alpha") && !first.contains("Beta"),
        "{first}"
    );
    let second = pdf.extract_text(&[2]).unwrap();
    assert!(
        second.contains("Beta") && !second.contains("Gamma"),
        "{second}"
    );
    let third = pdf.extract_text(&[3]).unwrap();
    assert!(third.contains("Gamma"), "{third}");
}

#[test]
fn adjacent_and_trailing_chapter_breaks_collapse() {
    use oryx::doc::model::{Block, BlockKind, Span};
    let para = |text: &str| {
        Block::plain(BlockKind::Paragraph {
            spans: vec![Span::plain(text)],
        })
    };
    let chapter_break = |spine| Block::plain(BlockKind::ChapterBreak { spine });
    let doc = Document {
        blocks: vec![
            para("start"),
            chapter_break(1),
            chapter_break(2),
            para("end"),
            chapter_break(3),
        ],
        ..Document::default()
    };
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    assert_eq!(
        pdf.get_pages().len(),
        2,
        "adjacent markers make one break, a trailing one makes none"
    );
}

/// Ledger probe: whole-book export wall time. Run with
/// ORYX_BOOK=<path> cargo test --release --test export book_export_probe -- --ignored --nocapture
#[test]
#[ignore]
fn book_export_probe() {
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let book = oryx::doc::epub::open_book(std::fs::read(&path).unwrap()).unwrap();
    let t = Instant::now();
    let bytes = export_book(&book);
    let pages = Pdf::load_mem(&bytes).unwrap().get_pages().len();
    println!(
        "export: {:?}, {} pages, {} bytes of pdf",
        t.elapsed(),
        pages,
        bytes.len()
    );
}

#[test]
fn the_pdf_outline_follows_the_book_toc() {
    let bytes = epub_common::book()
        .nav_doc(
            "<html><body><nav epub:type=\"toc\"><ol>\
             <li><a href=\"text/one.xhtml\">First Case</a></li>\
             <li><a href=\"text/two.xhtml\">Second Case</a><ol>\
             <li><a href=\"text/two.xhtml#tw\">A Twist</a></li></ol></li>\
             </ol></nav></body></html>",
        )
        .chapter(
            "one.xhtml",
            "<html><body><h1>Heading One</h1><p>First chapter text.</p></body></html>",
        )
        .chapter(
            "two.xhtml",
            "<html><body><h1 id=\"tw\">Heading Two</h1><p>Second chapter text.</p></body></html>",
        )
        .build();
    let book = oryx::doc::epub::open_book(bytes).unwrap();
    let pdf = Pdf::load_mem(&export_book(&book)).unwrap();
    assert_eq!(
        outline_titles(&pdf),
        vec!["First Case", "Second Case", "A Twist"],
        "the authored table of contents, not the heading scan"
    );
}

/// A book without any table of contents falls back to the heading scan;
/// a heading broken over lines with `<br>` bookmarks on one line.
#[test]
fn a_br_heading_bookmarks_on_one_line() {
    let bytes = epub_common::book()
        .chapter(
            "one.xhtml",
            "<html><body><h1>1<br/>&#160;<br/>LE MYST\u{c8}RE</h1><p>Text.</p></body></html>",
        )
        .build();
    let book = oryx::doc::epub::open_book(bytes).unwrap();
    let pdf = Pdf::load_mem(&export_book(&book)).unwrap();
    assert_eq!(outline_titles(&pdf), vec!["1 LE MYST\u{c8}RE"]);
}

fn export_with(doc: &Document, page: PageSize, page_numbers: bool) -> Vec<u8> {
    export_cfg(doc, page, Orientation::Portrait, page_numbers, None)
}

fn export_cfg(
    doc: &Document,
    page: PageSize,
    orientation: Orientation,
    page_numbers: bool,
    body_family: Option<&str>,
) -> Vec<u8> {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let mut cfg = ViewConfig {
        body_size: 11.0,
        code_size: 9.0,
        zoom: 1.0,
        ..ViewConfig::default()
    };
    if let Some(family) = body_family {
        cfg.body_family = family.to_string();
    }
    let theme = Theme::default_dark();
    let geometry = PageGeometry::new(page, orientation, cfg.body_size);
    let settings = ExportSettings {
        body_size: cfg.body_size,
        code_size: cfg.code_size,
        page,
        orientation,
        page_numbers,
        ..ExportSettings::default()
    };
    let laid = layout(doc, &theme, &mut fonts, &mut media, &cfg, geometry.width);
    let pages = paginate(doc, &laid, &geometry);
    let job = pdf::Job {
        doc,
        layout: &laid,
        theme: &theme,
        geometry: &geometry,
        settings: &settings,
        title: "test",
        toc: &[],
    };
    pdf::build(&job, &pages, &mut fonts, &mut media).expect("the export builds")
}

#[test]
fn a_short_document_exports_to_one_readable_page() {
    let doc = markdown::parse("# Title\n\nOne short paragraph.");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).expect("a reader parses the file");
    assert_eq!(pdf.get_pages().len(), 1);
    let text = pdf.extract_text(&[1]).expect("text extracts");
    assert!(text.contains("One short paragraph."), "got {text:?}");
    assert!(text.contains("Title"), "got {text:?}");
}

#[test]
fn the_media_box_matches_the_chosen_page_size() {
    let doc = markdown::parse("Short.");
    let bytes = export_to_bytes(&doc, PageSize::Letter);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let (_, page) = pdf.get_pages().into_iter().next().unwrap();
    let media = pdf
        .get_object(page)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap();
    let sizes: Vec<f32> = media
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(sizes, vec![0.0, 0.0, 612.0, 792.0]);
}

#[test]
fn a_landscape_export_writes_a_turned_media_box() {
    let doc = markdown::parse("Short.");
    let bytes = export_cfg(&doc, PageSize::Letter, Orientation::Landscape, false, None);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let (_, page) = pdf.get_pages().into_iter().next().unwrap();
    let media = pdf
        .get_object(page)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap();
    let sizes: Vec<f32> = media
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(sizes, vec![0.0, 0.0, 792.0, 612.0]);
}

#[test]
fn a_long_document_pages_and_stays_readable() {
    let doc = markdown::parse("A paragraph that says something.\n\n".repeat(200).as_str());
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let count = pdf.get_pages().len();
    assert!(count > 3, "200 paragraphs make more than three pages");
    let last = count as u32;
    assert!(pdf
        .extract_text(&[last])
        .unwrap()
        .contains("says something"));
}

#[test]
fn the_document_carries_its_title_and_producer() {
    let doc = markdown::parse("Short.");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("oryx"), "the producer names the app");
}

fn annotation_uris(pdf: &Pdf) -> Vec<String> {
    let mut out = Vec::new();
    for (_, page_id) in pdf.get_pages() {
        let dict = pdf.get_object(page_id).unwrap().as_dict().unwrap();
        let Ok(annots) = dict.get(b"Annots").and_then(|a| a.as_array()) else {
            continue;
        };
        for entry in annots {
            let id = entry.as_reference().unwrap();
            let annot = pdf.get_object(id).unwrap().as_dict().unwrap();
            let Ok(action) = annot.get(b"A").and_then(|a| a.as_dict()) else {
                continue;
            };
            if let Ok(uri) = action.get(b"URI").and_then(|u| u.as_str()) {
                out.push(String::from_utf8_lossy(uri).to_string());
            }
        }
    }
    out
}

/// The page index an internal link points at, one-based, if there is one.
fn internal_destination_page(pdf: &Pdf) -> Option<usize> {
    let pages: Vec<_> = pdf.get_pages().into_iter().collect();
    for (_, page_id) in &pages {
        let dict = pdf.get_object(*page_id).unwrap().as_dict().unwrap();
        let Ok(annots) = dict.get(b"Annots").and_then(|a| a.as_array()) else {
            continue;
        };
        for entry in annots {
            let id = entry.as_reference().unwrap();
            let annot = pdf.get_object(id).unwrap().as_dict().unwrap();
            let Ok(action) = annot.get(b"A").and_then(|a| a.as_dict()) else {
                continue;
            };
            let Ok(dest) = action.get(b"D").and_then(|d| d.as_array()) else {
                continue;
            };
            let target = dest[0].as_reference().unwrap();
            return pages
                .iter()
                .position(|(_, id)| *id == target)
                .map(|at| at + 1);
        }
    }
    None
}

fn decode_title(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

fn outline_titles(pdf: &Pdf) -> Vec<String> {
    fn walk(pdf: &Pdf, id: lopdf::ObjectId, out: &mut Vec<String>) {
        let dict = pdf.get_object(id).unwrap().as_dict().unwrap();
        if let Ok(title) = dict.get(b"Title").and_then(|t| t.as_str()) {
            out.push(decode_title(title));
        }
        if let Ok(first) = dict.get(b"First").and_then(|f| f.as_reference()) {
            walk(pdf, first, out);
        }
        if let Ok(next) = dict.get(b"Next").and_then(|n| n.as_reference()) {
            walk(pdf, next, out);
        }
    }
    let catalog = pdf.catalog().unwrap();
    let Ok(root) = catalog.get(b"Outlines").and_then(|o| o.as_reference()) else {
        return Vec::new();
    };
    let dict = pdf.get_object(root).unwrap().as_dict().unwrap();
    let mut out = Vec::new();
    if let Ok(first) = dict.get(b"First").and_then(|f| f.as_reference()) {
        walk(pdf, first, &mut out);
    }
    out
}

fn image_xobjects(pdf: &Pdf) -> usize {
    pdf.objects
        .values()
        .filter(|object| {
            object.as_stream().is_ok_and(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(|s| s.as_name())
                    .is_ok_and(|name| name == b"Image")
            })
        })
        .count()
}

#[test]
fn an_external_link_becomes_a_uri_annotation() {
    let doc = markdown::parse("See [the site](https://oryx.example/docs).");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    assert_eq!(
        annotation_uris(&pdf),
        vec!["https://oryx.example/docs".to_string()]
    );
}

#[test]
fn an_anchor_link_points_at_the_page_holding_its_heading() {
    let doc = markdown::parse(format!(
        "[jump](#target)\n\n{}\n## Target\n\nThe section.\n",
        "Filler paragraph here.\n\n".repeat(140)
    ));
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let page = internal_destination_page(&pdf).expect("an internal destination");
    assert!(page > 1, "the heading is not on the first page, got {page}");
}

#[test]
fn the_outline_carries_every_heading_in_order() {
    let doc = markdown::parse("# One\n\ntext\n\n## Two\n\ntext\n\n# Three\n\ntext\n");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    assert_eq!(outline_titles(&pdf), vec!["One", "Two", "Three"]);
}

#[test]
fn page_numbers_appear_only_when_they_are_on() {
    let doc = markdown::parse("Filler paragraph here.\n\n".repeat(200).as_str());
    let with = Pdf::load_mem(&export_with(&doc, PageSize::A4, true)).unwrap();
    let without = Pdf::load_mem(&export_with(&doc, PageSize::A4, false)).unwrap();
    let numbered = with.extract_text(&[2]).unwrap();
    let plain = without.extract_text(&[2]).unwrap();
    assert!(numbered.contains('2'), "the second page is numbered");
    assert!(!plain.contains('2'), "no number when they are off");
}

#[test]
fn a_repeated_image_is_embedded_once() {
    let doc = markdown::parse("![logo](oryx-test.png)\n\n".repeat(40).as_str());
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let count = image_xobjects(&pdf);
    assert!(count >= 1, "the image is embedded at all");
    assert!(count <= 2, "one source, one image and at most its mask");
}

fn image_sample_widths(pdf: &Pdf) -> Vec<i64> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let stream = object.as_stream().ok()?;
            let subtype = stream.dict.get(b"Subtype").ok()?.as_name().ok()?;
            (subtype == b"Image").then(|| stream.dict.get(b"Width").ok()?.as_i64().ok())?
        })
        .collect()
}

#[test]
fn an_image_is_embedded_at_its_own_resolution() {
    // The fixture is 220 pixels square and lands in a box of about 110
    // points, so sampling at the placed size would halve it.
    let doc = markdown::parse("![logo](oryx-test.png)");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let widths = image_sample_widths(&pdf);
    assert!(!widths.is_empty(), "the image is embedded");
    for width in widths {
        assert_eq!(width, 220, "the source's own pixels, not the placed size");
    }
}

/// Every descendant font in the file: its subtype and which font file
/// entry its descriptor carries.
fn cid_fonts(pdf: &Pdf) -> Vec<(String, bool, bool)> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let dict = object.as_dict().ok()?;
            let subtype = dict.get(b"Subtype").ok()?.as_name().ok()?;
            let subtype = String::from_utf8_lossy(subtype).to_string();
            if subtype != "CIDFontType0" && subtype != "CIDFontType2" {
                return None;
            }
            let descriptor = dict.get(b"FontDescriptor").ok()?.as_reference().ok()?;
            let descriptor = pdf.get_object(descriptor).ok()?.as_dict().ok()?;
            Some((
                subtype,
                descriptor.get(b"FontFile2").is_ok(),
                descriptor.get(b"FontFile3").is_ok(),
            ))
        })
        .collect()
}

#[test]
fn a_truetype_face_embeds_as_cidfonttype2_with_fontfile2() {
    let doc = markdown::parse("Some body text in the bundled face.");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let fonts = cid_fonts(&pdf);
    assert!(!fonts.is_empty(), "the file embeds a font");
    for (subtype, file2, file3) in fonts {
        assert_eq!(subtype, "CIDFontType2", "the bundled faces are TrueType");
        assert!(file2 && !file3, "TrueType data rides FontFile2");
    }
}

/// A spec-conformant pairing for CFF faces: CIDFontType0 with FontFile3.
/// The bundled defaults are TrueType, so the case only shows with an
/// installed OpenType/CFF family; the test hunts for one and skips with
/// a note when the system has none.
#[test]
fn a_cff_face_embeds_as_cidfonttype0_with_fontfile3() {
    let fonts = FontStore::new();
    let infos: Vec<(cosmic_text::fontdb::ID, Option<String>)> = fonts
        .font_system
        .db()
        .faces()
        .map(|face| (face.id, face.families.first().map(|(name, _)| name.clone())))
        .collect();
    let mut family = None;
    for (id, name) in infos {
        let cff = fonts
            .font_system
            .db()
            .with_face_data(id, |data, _| data.starts_with(b"OTTO"))
            .unwrap_or(false);
        if cff && name.is_some() {
            family = name;
            break;
        }
    }
    let Some(family) = family else {
        eprintln!("skipped: no OpenType/CFF face installed");
        return;
    };
    let doc = markdown::parse("Some body text in a CFF face.");
    let bytes = export_cfg(
        &doc,
        PageSize::A4,
        Orientation::Portrait,
        false,
        Some(&family),
    );
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let fonts = cid_fonts(&pdf);
    assert!(
        fonts
            .iter()
            .any(|(subtype, _, _)| subtype == "CIDFontType0"),
        "the CFF face {family} embeds as CIDFontType0, got {fonts:?}"
    );
    for (subtype, file2, file3) in fonts {
        let cff = subtype == "CIDFontType0";
        assert_eq!(file3, cff, "CFF data rides FontFile3");
        assert_eq!(file2, !cff, "TrueType data rides FontFile2");
    }
}

// The field failure behind this test: a document whose emoji resolve
// through a color bitmap font (Noto Color Emoji has no outlines) failed
// the whole export at the subsetter. Bitmap glyphs embed as images
// instead. On a machine without such a font the emoji resolves to an
// outline face and the test still pins that the export succeeds and the
// surrounding text survives.
#[test]
fn emoji_exports_without_failing_the_document() {
    let doc = markdown::parse("before \u{1F389} after\n");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).expect("a reader parses the file");
    assert_eq!(pdf.get_pages().len(), 1);
    let text = pdf.extract_text(&[1]).expect("text extracts");
    assert!(text.contains("before"), "got {text:?}");
    assert!(text.contains("after"), "got {text:?}");
}

// Task 53: the export stream. Pages flush to the sibling `.part` file
// as they are written, pagination consumes placed blocks behind the
// layout cursor, and the assembled file reads back identical to the
// one-shot builder's.

/// A document that exercises every break rule: headings, split code
/// panels, a table, a quote region, footnotes, across several pages.
fn paged_source() -> String {
    let mut source = String::from("# Title\n\nintro paragraph\n\n");
    source.push_str("```rust\n");
    for i in 0..120 {
        source.push_str(&format!("let value_{i} = compute({i});\n"));
    }
    source.push_str("```\n\n");
    source.push_str("|h1|h2|\n|-|-|\n|a|b|\n|c|d|\n\n");
    source.push_str("> quoted one\n>\n> quoted two\n\n");
    source.push_str("Inline math $a_i^2 + b$ rides its sentence.\n\n");
    source.push_str("```math\n\\sum_{i=1}^{n} \\frac{1}{i^2}\n```\n\n");
    for i in 0..160 {
        source.push_str(&format!("Filler paragraph number {i} with body.\n\n"));
    }
    source.push_str("## Late\n\ntail with a note[^n]\n\n[^n]: the note text\n");
    source
}

fn geometry_and_cfg() -> (PageGeometry, ViewConfig) {
    let cfg = ViewConfig {
        body_size: 11.0,
        code_size: 9.0,
        zoom: 1.0,
        ..ViewConfig::default()
    };
    (
        PageGeometry::new(PageSize::A4, Orientation::Portrait, cfg.body_size),
        cfg,
    )
}

/// Feeds the paginator a layout grown `stride` steps at a time and
/// collects what it finalizes along the way.
fn paginate_in_slices(doc: &Document, stride: usize) -> Vec<oryx::export::paginate::Page> {
    let (geometry, cfg) = geometry_and_cfg();
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let theme = Theme::default_dark();
    let (mut lay, mut pass) = layout_begin(doc, &cfg, geometry.width);
    let mut paginator = Paginator::new();
    let mut pages = Vec::new();
    let mut done = false;
    while !done {
        for _ in 0..stride {
            done = layout_step(
                doc, &theme, &mut fonts, &mut media, &cfg, &mut lay, &mut pass,
            );
            if done {
                break;
            }
        }
        pages.extend(paginator.advance(doc, &lay, &geometry, done));
    }
    pages
}

#[test]
fn incremental_pagination_matches_the_one_shot_pages() {
    let doc = markdown::parse(paged_source().as_str());
    let (geometry, cfg) = geometry_and_cfg();
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let lay = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &cfg,
        geometry.width,
    );
    let whole = paginate(&doc, &lay, &geometry);
    assert!(
        whole.len() > 3,
        "the fixture spans pages, got {}",
        whole.len()
    );
    for stride in [1usize, 7, 1000] {
        let sliced = paginate_in_slices(&doc, stride);
        assert_eq!(sliced.len(), whole.len(), "page count at stride {stride}");
        for (index, (a, b)) in sliced.iter().zip(&whole).enumerate() {
            assert_eq!(a, b, "page {index} at stride {stride}");
        }
    }
}

/// Drives an export pass to completion against a target file.
fn stream_to(
    doc: &Document,
    target: &std::path::Path,
    pool: Option<&std::sync::Arc<oryx::layout::ShapePool>>,
) -> (usize, bool) {
    let settings = ExportSettings {
        body_size: 11.0,
        code_size: 9.0,
        page: PageSize::A4,
        page_numbers: true,
        ..ExportSettings::default()
    };
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let mut pass = ExportPass::new(&settings, Theme::default_dark(), target.to_path_buf());
    let mut part = target.to_path_buf().into_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    let mut part_grew = false;
    let mut last_size = 0u64;
    while !pass.is_done() {
        pass.step(
            Instant::now() + Duration::from_millis(30),
            doc,
            &mut fonts,
            &mut media,
            false,
            pool,
        );
        if let Ok(meta) = std::fs::metadata(&part) {
            if meta.len() > last_size {
                part_grew = true;
                last_size = meta.len();
            }
        }
    }
    let pages = pass.finish(doc, &fonts).expect("the export lands");
    (pages, part_grew)
}

#[test]
fn the_streamed_file_reads_back_like_the_one_shot_build() {
    let source = std::fs::read_to_string("tests/fixtures/tour.md").expect("the tour fixture");
    let doc = markdown::parse(source.as_str());
    let target = std::env::temp_dir().join(format!("oryx-stream-{}.pdf", std::process::id()));

    let (pages, part_grew) = stream_to(&doc, &target, None);
    let streamed = std::fs::read(&target).expect("the streamed file");
    std::fs::remove_file(&target).ok();
    assert!(
        part_grew,
        "the .part file grows while pages are written, not only at finish"
    );

    let reference = export_with(&doc, PageSize::A4, true);
    let streamed = Pdf::load_mem(&streamed).expect("a reader parses the streamed file");
    let reference = Pdf::load_mem(&reference).expect("a reader parses the reference");
    assert_eq!(streamed.get_pages().len(), pages);
    assert_eq!(
        streamed.get_pages().len(),
        reference.get_pages().len(),
        "page count"
    );
    let count = reference.get_pages().len() as u32;
    for page in 1..=count {
        assert_eq!(
            streamed.extract_text(&[page]).expect("streamed text"),
            reference.extract_text(&[page]).expect("reference text"),
            "text of page {page}"
        );
    }
    assert_eq!(
        annotation_uris(&streamed),
        annotation_uris(&reference),
        "link targets"
    );
    assert_eq!(
        internal_destination_page(&streamed),
        internal_destination_page(&reference),
        "internal destinations"
    );
    assert_eq!(
        outline_titles(&streamed),
        outline_titles(&reference),
        "the outline"
    );
}

#[test]
fn a_pool_of_one_emits_the_same_bytes_as_a_pool_of_many() {
    let doc = markdown::parse(paged_source().as_str());
    let fonts = FontStore::new();
    let one = std::sync::Arc::new(oryx::layout::ShapePool::new(1, &fonts.seed()));
    let many = std::sync::Arc::new(oryx::layout::ShapePool::new(4, &fonts.seed()));
    // The same file name in two directories, since the name becomes the
    // document title and the bytes must compare whole.
    let dir_a = std::env::temp_dir().join(format!("oryx-pool1-{}", std::process::id()));
    let dir_b = std::env::temp_dir().join(format!("oryx-pool4-{}", std::process::id()));
    std::fs::create_dir_all(&dir_a).expect("a scratch directory");
    std::fs::create_dir_all(&dir_b).expect("a scratch directory");
    let target_a = dir_a.join("streamed.pdf");
    let target_b = dir_b.join("streamed.pdf");
    stream_to(&doc, &target_a, Some(&one));
    stream_to(&doc, &target_b, Some(&many));
    let a = std::fs::read(&target_a).expect("pool-of-one file");
    let b = std::fs::read(&target_b).expect("pool-of-many file");
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
    assert!(a == b, "emission is deterministic across pool widths");
}

#[test]
fn a_cancelled_export_leaves_the_target_and_no_part_behind() {
    let doc = markdown::parse(paged_source().as_str());
    let target = std::env::temp_dir().join(format!("oryx-cancel-{}.pdf", std::process::id()));
    std::fs::write(&target, b"previous export").expect("the previous file");
    let mut part = target.clone().into_os_string();
    part.push(".part");
    let part = PathBuf::from(part);

    let settings = ExportSettings {
        body_size: 11.0,
        code_size: 9.0,
        page: PageSize::A4,
        page_numbers: true,
        ..ExportSettings::default()
    };
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let mut pass = ExportPass::new(&settings, Theme::default_dark(), target.clone());
    for _ in 0..50 {
        if pass.is_done() {
            break;
        }
        pass.step(
            Instant::now() + Duration::from_millis(10),
            &doc,
            &mut fonts,
            &mut media,
            false,
            None,
        );
    }
    assert!(part.exists(), "the run reached the disk before the cancel");
    drop(pass);
    assert!(!part.exists(), "the cancel removes the .part file");
    assert_eq!(
        std::fs::read(&target).expect("the target survives"),
        b"previous export",
        "the target is untouched"
    );
    std::fs::remove_file(&target).ok();
}

// Task 61: math in the PDF export. Equations reach the emitter as STIX
// glyphs; rules and literal fallbacks already travel as rects and runs.

#[test]
fn an_equation_exports_typeset_and_extracts_its_characters() {
    let doc = markdown::parse("Before.\n\n```math\n\\frac{12}{34}\n```\n\nInline $a+b=c$ after.\n");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).expect("a reader parses the file");
    let text = pdf.extract_text(&[1]).expect("text extracts");
    assert!(
        !text.contains("frac"),
        "the equation typesets rather than printing its TeX, got {text:?}"
    );
    assert!(
        text.contains("12") && text.contains("34"),
        "the fraction's digits extract, got {text:?}"
    );
    for ch in ['\u{1D44E}', '\u{1D44F}', '\u{1D450}', '+', '='] {
        assert!(text.contains(ch), "{ch:?} extracts, got {text:?}");
    }
    let fonts = cid_fonts(&pdf);
    assert!(
        fonts
            .iter()
            .any(|(subtype, _, file3)| subtype == "CIDFontType0" && *file3),
        "STIX embeds as a CFF CID font, got {fonts:?}"
    );
}

#[test]
fn every_display_equation_lands_whole_on_one_page() {
    let mut source = String::new();
    for i in 0..60 {
        source.push_str(&format!("Filler paragraph number {i} with body.\n\n"));
        source.push_str("```math\n\\frac{a+b}{c+d}\n```\n\n");
    }
    let doc = markdown::parse(source.as_str());
    let (geometry, cfg) = geometry_and_cfg();
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let lay = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &cfg,
        geometry.width,
    );
    let pages = paginate(&doc, &lay, &geometry);
    assert!(pages.len() > 3, "the fixture spans pages");
    let mut seen: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for (index, page) in pages.iter().enumerate() {
        for glyph in &page.math {
            total += 1;
            assert!(
                glyph.y >= page.top - 0.01 && glyph.y <= page.bottom + 0.01,
                "glyph baseline {} inside page {} [{}, {}]",
                glyph.y,
                index,
                page.top,
                page.bottom
            );
            let entry = seen.entry(glyph.block).or_insert(index);
            assert_eq!(
                *entry, index,
                "equation block {} splits across pages {} and {}",
                glyph.block, entry, index
            );
        }
    }
    assert!(total > 0, "pages carry math glyphs at all");
    assert_eq!(seen.len(), 60, "every equation lands");
}

/// The showcase collection is the field's export scenario: emoji through
/// a bitmap font where one is installed, footnotes, tables, math. The
/// export must build and keep the text a reader extracts.
#[test]
fn the_showcase_collection_exports() {
    let mut names: Vec<_> = std::fs::read_dir("tests/showcase")
        .expect("showcase directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    names.sort();
    let mut source = String::new();
    for path in names {
        source.push_str(&std::fs::read_to_string(path).expect("showcase file"));
    }
    let doc = markdown::parse(source.as_str());
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).expect("a reader parses the file");
    let pages: Vec<u32> = pdf.get_pages().keys().copied().collect();
    assert!(pages.len() > 1, "the collection spans pages");
    let text = pdf.extract_text(&pages).expect("text extracts");
    assert!(
        text.contains("pushed around"),
        "the footnote text survives, got tail {:?}",
        &text[text.len().saturating_sub(200)..]
    );
}

#[test]
fn a_book_export_justifies_when_asked() {
    let body = format!(
        "<html><body><p>{}end.</p></body></html>",
        "justify word ".repeat(60)
    );
    let bytes = epub_common::book().chapter("one.xhtml", &body).build();
    let book = oryx::doc::epub::open_book(bytes).unwrap();
    let pdf_for = |justify: bool| {
        let settings = ExportSettings {
            justify,
            body_size: 11.0,
            code_size: 9.0,
            ..ExportSettings::default()
        };
        let target =
            std::env::temp_dir().join(format!("oryx-justify-{justify}-{}.pdf", std::process::id()));
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let mut pass = ExportPass::new(&settings, Theme::default_dark(), target.clone());
        while !pass.is_done() {
            pass.step(
                Instant::now() + Duration::from_millis(30),
                &book.document,
                &mut fonts,
                &mut media,
                false,
                None,
            );
        }
        pass.finish(&book.document, &fonts)
            .expect("the export lands");
        let out = std::fs::read(&target).expect("the exported file");
        std::fs::remove_file(&target).ok();
        out
    };
    let just = pdf_for(true);
    let plain = pdf_for(false);
    let content = |bytes: &[u8]| {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let page = *pdf.get_pages().get(&1).unwrap();
        pdf.get_page_content(page)
    };
    assert_ne!(
        content(&just),
        content(&plain),
        "justification moves the page's text positions"
    );
    let text = Pdf::load_mem(&just).unwrap().extract_text(&[1]).unwrap();
    assert!(
        text.contains("justify") && text.contains("word"),
        "the words survive: {text}"
    );
}

#[test]
fn a_markdown_export_never_justifies() {
    let doc = markdown::parse(format!("{}end.\n", "justify word ".repeat(40)));
    let pdf_for = |justify: bool| {
        let settings = ExportSettings {
            justify,
            body_size: 11.0,
            code_size: 9.0,
            ..ExportSettings::default()
        };
        let target =
            std::env::temp_dir().join(format!("oryx-md-just-{justify}-{}.pdf", std::process::id()));
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let mut pass = ExportPass::new(&settings, Theme::default_dark(), target.clone());
        while !pass.is_done() {
            pass.step(
                Instant::now() + Duration::from_millis(30),
                &doc,
                &mut fonts,
                &mut media,
                false,
                None,
            );
        }
        pass.finish(&doc, &fonts).expect("the export lands");
        let out = std::fs::read(&target).expect("the exported file");
        std::fs::remove_file(&target).ok();
        out
    };
    let content = |bytes: &[u8]| {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let page = *pdf.get_pages().get(&1).unwrap();
        pdf.get_page_content(page)
    };
    assert_eq!(
        content(&pdf_for(true)),
        content(&pdf_for(false)),
        "the setting has no effect outside books"
    );
}
