//! Comic reading: page order, header dimensions, the outline,
//! detection, refusals, and the three display states.

#[path = "fixtures/comic_common.rs"]
mod comic_common;

#[path = "../rarball/tests/fixtures/writer.rs"]
mod rar_writer;

use std::path::PathBuf;

use comic_common::{cbz, cbz_deflated, encrypted_cbz, jpeg_bytes, png_bytes};
use oryx::doc::comic;
use oryx::doc::epub;
use oryx::doc::images::{self, BookSource, MediaCache};
use oryx::doc::load::{self, FileKind};
use oryx::doc::markdown;
use oryx::doc::model::{BlockKind, Document};
use oryx::layout::{layout, ComicFit, DirectionMode, LayoutDoc, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

/// The image blocks' keys and alt texts, in document order.
fn pages(doc: &Document) -> Vec<(String, String)> {
    doc.blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Image { path, alt } => Some((path.clone(), alt.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn pages_arrive_in_natural_order_with_dimensions() {
    let bytes = cbz(&[
        ("page10.jpg", &jpeg_bytes(30, 40)),
        ("page2.jpg", &jpeg_bytes(20, 10)),
        ("page1.png", &png_bytes(8, 4)),
    ]);
    let book = comic::open_book(bytes, "Test Comic").unwrap();
    assert_eq!(book.document.title.as_deref(), Some("Test Comic"));
    assert_eq!(
        pages(&book.document),
        [
            ("page1".to_string(), "Page 1".to_string()),
            ("page2".to_string(), "Page 2".to_string()),
            ("page3".to_string(), "Page 3".to_string()),
        ],
        "page2 precedes page10: names sort as a person reads them"
    );
    assert_eq!(
        book.document.blocks.len(),
        3,
        "a comic holds nothing but its pages"
    );
    let dims: Vec<(String, Option<(u32, u32)>)> = book
        .pages
        .iter()
        .map(|(key, _, dims)| (key.clone(), *dims))
        .collect();
    assert_eq!(
        dims,
        [
            ("page1".to_string(), Some((8, 4))),
            ("page2".to_string(), Some((20, 10))),
            ("page3".to_string(), Some((30, 40))),
        ],
        "every source carries its header dimensions in sorted order"
    );
}

#[test]
fn nested_folders_flatten_in_name_order() {
    let bytes = cbz(&[
        ("volume 2/001.jpg", &jpeg_bytes(7, 8)),
        ("volume 1/002.jpg", &jpeg_bytes(5, 6)),
    ]);
    let book = comic::open_book(bytes, "Nested").unwrap();
    let dims: Vec<Option<(u32, u32)>> = book.pages.iter().map(|(_, _, d)| *d).collect();
    assert_eq!(
        dims,
        [Some((5, 6)), Some((7, 8))],
        "full entry names order the pages, directories included"
    );
}

#[test]
fn non_image_entries_are_ignored() {
    let bytes = cbz(&[
        ("ComicInfo.xml", b"<ComicInfo/>".as_slice()),
        ("01.jpg", &jpeg_bytes(10, 12)),
        ("notes.txt", b"scanner notes".as_slice()),
        ("02.jpg", &jpeg_bytes(10, 14)),
    ]);
    let book = comic::open_book(bytes, "Metadata").unwrap();
    assert_eq!(book.pages.len(), 2, "only page images become pages");
    assert_eq!(pages(&book.document).len(), 2);
}

#[test]
fn deflated_pages_stay_compressed_until_a_decode_asks() {
    let bytes = cbz_deflated(&[("p1.png", &png_bytes(9, 7))]);
    let book = comic::open_book(bytes, "Deflated").unwrap();
    assert_eq!(book.pages.len(), 1);
    let (_, source, dims) = &book.pages[0];
    assert_eq!(
        *dims,
        Some((9, 7)),
        "header dimensions probe through the compression"
    );
    assert!(
        matches!(source, BookSource::Deflated(_)),
        "open never inflates a whole page"
    );
    let img = images::decode_source(source).expect("the deflated source decodes");
    assert_eq!(img.dimensions(), (9, 7));
}

#[test]
fn the_outline_lists_every_page_in_order() {
    let bytes = cbz(&[
        ("a.jpg", &jpeg_bytes(4, 4)),
        ("b.jpg", &jpeg_bytes(4, 4)),
        ("c.jpg", &jpeg_bytes(4, 4)),
    ]);
    let book = comic::open_book(bytes, "Outline").unwrap();
    let labels: Vec<(String, u8)> = book
        .toc
        .iter()
        .map(|e| (e.label.clone(), e.depth))
        .collect();
    assert_eq!(
        labels,
        [
            ("Page 1".to_string(), 0),
            ("Page 2".to_string(), 0),
            ("Page 3".to_string(), 0),
        ]
    );
    let offsets: Vec<usize> = book
        .toc
        .iter()
        .map(|e| {
            epub::resolve_target(&book.document, &e.path, e.fragment.as_deref())
                .expect("every page entry resolves")
        })
        .collect();
    assert!(
        offsets.windows(2).all(|w| w[0] < w[1]),
        "outline targets sit in page order: {offsets:?}"
    );
}

#[test]
fn detection_routes_cbz() {
    use std::path::Path;
    assert_eq!(load::detect(Path::new("x/book.cbz")), FileKind::Comic);
    assert_eq!(load::detect(Path::new("x/BOOK.CBZ")), FileKind::Comic);
    assert_eq!(load::detect(Path::new("x/book.cbr")), FileKind::Comic);
    assert_eq!(load::detect(Path::new("x/BOOK.CBR")), FileKind::Comic);
}

/// A RAR comic of the given pages, stored, the wild's pattern.
fn cbr(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let files: Vec<rar_writer::FileSpec> = entries
        .iter()
        .map(|(name, data)| rar_writer::file(name, data))
        .collect();
    rar_writer::rar4(&files, 0)
}

#[test]
fn a_rar_comic_opens_with_pages_in_order() {
    for bytes in [
        cbr(&[
            ("Page 10.jpg", &jpeg_bytes(30, 40)),
            ("Page 2.jpg", &jpeg_bytes(20, 10)),
            ("Page 1.png", &png_bytes(8, 4)),
        ]),
        {
            let files = vec![
                rar_writer::file("Page 10.jpg", &jpeg_bytes(30, 40)),
                rar_writer::file("Page 2.jpg", &jpeg_bytes(20, 10)),
                rar_writer::file("Page 1.png", &png_bytes(8, 4)),
            ];
            rar_writer::rar5(&files, false)
        },
    ] {
        let book = comic::open_book(bytes, "Rar Comic").unwrap();
        assert_eq!(book.document.title.as_deref(), Some("Rar Comic"));
        let dims: Vec<Option<(u32, u32)>> = book.pages.iter().map(|(_, _, d)| *d).collect();
        assert_eq!(
            dims,
            [Some((8, 4)), Some((20, 10)), Some((30, 40))],
            "pages sort naturally and carry header dimensions, both generations"
        );
        assert_eq!(book.toc.len(), 3);
    }
}

#[test]
fn the_container_is_sniffed_not_trusted() {
    // The same open serves both containers whatever the file was
    // named: dispatch reads the signature, mislabels being common.
    let zip_bytes = cbz(&[("p1.jpg", &jpeg_bytes(6, 8))]);
    let rar_bytes = cbr(&[("p1.jpg", &jpeg_bytes(6, 8))]);
    assert_eq!(comic::open_book(zip_bytes, "z").unwrap().pages.len(), 1);
    assert_eq!(comic::open_book(rar_bytes, "r").unwrap().pages.len(), 1);
}

#[test]
fn a_compressed_rar_comic_refuses_plainly() {
    let mut files = vec![rar_writer::file("Page 1.jpg", &jpeg_bytes(6, 8))];
    files[0].method = 3;
    let bytes = rar_writer::rar4(&files, 0);
    let refused = comic::open_book(bytes, "x").map(|_| ()).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "This comic archive uses RAR compression that Oryx cannot read."
    );
}

#[test]
fn an_encrypted_rar_comic_refuses_plainly() {
    let files = vec![rar_writer::file("Page 1.jpg", &jpeg_bytes(6, 8))];
    let whole = rar_writer::rar4(&files, 0x0080);
    let refused = comic::open_book(whole, "x").map(|_| ()).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "This comic archive is encrypted and cannot be opened."
    );
    let mut files = vec![rar_writer::file("Page 1.jpg", &jpeg_bytes(6, 8))];
    files[0].encrypted = true;
    let entry = rar_writer::rar4(&files, 0);
    let refused = comic::open_book(entry, "x").map(|_| ()).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "This comic archive is encrypted and cannot be opened."
    );
}

#[test]
fn a_damaged_rar_comic_says_damaged() {
    let full = cbr(&[("Page 1.jpg", &jpeg_bytes(6, 8))]);
    let cut = full[..full.len() - 20].to_vec();
    let refused = comic::open_book(cut, "x").map(|_| ()).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "This comic archive is damaged and cannot be read."
    );
}

#[test]
fn refusals_speak_plainly() {
    let garbage = comic::open_book(b"not an archive at all".to_vec(), "x")
        .map(|_| ())
        .unwrap_err();
    assert_eq!(
        garbage.to_string(),
        "This file is not a readable comic book archive."
    );

    let empty = cbz(&[("ComicInfo.xml", b"<ComicInfo/>".as_slice())]);
    let no_pages = comic::open_book(empty, "x").map(|_| ()).unwrap_err();
    assert_eq!(
        no_pages.to_string(),
        "This comic archive holds no page images."
    );

    let encrypted = comic::open_book(encrypted_cbz(), "x")
        .map(|_| ())
        .unwrap_err();
    assert_eq!(
        encrypted.to_string(),
        "This comic archive is encrypted and cannot be opened."
    );
}

/// Lays out a comic built from the given pages under one display state.
fn comic_layout(entries: &[(&str, &[u8])], comic: ComicFit, width: f32) -> LayoutDoc {
    let book = comic::open_book(cbz(entries), "T").unwrap();
    let mut media = MediaCache::new(PathBuf::from("."));
    media.adopt(book.pages);
    let cfg = ViewConfig {
        comic,
        ..ViewConfig::default()
    };
    let mut fonts = FontStore::new();
    layout(
        &book.document,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &cfg,
        width,
    )
}

fn rect(place: &oryx::layout::ImagePlace) -> (f32, f32, f32, f32) {
    (place.x, place.y, place.width, place.height)
}

#[test]
fn the_strip_fills_the_window_width() {
    let l = comic_layout(
        &[
            ("1.png", &png_bytes(100, 50)),
            ("2.png", &png_bytes(40, 80)),
        ],
        ComicFit::Width,
        600.0,
    );
    assert_eq!(l.images.len(), 2);
    assert_eq!(
        rect(&l.images[0]),
        (0.0, 0.0, 600.0, 300.0),
        "a page fills the width edge to edge, upscaling freely"
    );
    assert_eq!(
        rect(&l.images[1]),
        (0.0, 300.0, 600.0, 1200.0),
        "pages meet with no gap, the webtoon strip unbroken"
    );
    assert_eq!(l.height, 1500.0);
}

#[test]
fn full_page_fits_both_dimensions_and_centers() {
    let l = comic_layout(
        &[
            ("1.png", &png_bytes(100, 50)),
            ("2.png", &png_bytes(40, 80)),
        ],
        ComicFit::Page { height: 400.0 },
        600.0,
    );
    assert_eq!(
        rect(&l.images[0]),
        (0.0, 50.0, 600.0, 300.0),
        "a wide page fills the width and centers vertically in its slot"
    );
    assert_eq!(
        rect(&l.images[1]),
        (200.0, 400.0, 200.0, 400.0),
        "a tall page fills the height and centers horizontally"
    );
    assert_eq!(l.height, 800.0, "every page occupies one viewport slot");
}

#[test]
fn two_pages_pair_after_the_cover() {
    let pages: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|n| (format!("{n}.png"), png_bytes(100, 100)))
        .collect();
    let entries: Vec<(&str, &[u8])> = pages
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let l = comic_layout(&entries, ComicFit::Two { height: 400.0 }, 600.0);
    assert_eq!(
        rect(&l.images[0]),
        (100.0, 0.0, 400.0, 400.0),
        "the cover stands alone, centered"
    );
    assert_eq!(
        rect(&l.images[1]),
        (0.0, 450.0, 300.0, 300.0),
        "the left page sits against the gutter"
    );
    assert_eq!(
        rect(&l.images[2]),
        (300.0, 450.0, 300.0, 300.0),
        "the right page continues the spread"
    );
    assert_eq!(
        rect(&l.images[3]),
        (0.0, 850.0, 300.0, 300.0),
        "a last odd page takes the left of its own row"
    );
    assert_eq!(l.height, 1200.0, "the cover and two rows");
}

#[test]
fn a_text_document_ignores_the_comic_fit() {
    let doc = markdown::parse("# Title\n\nSome prose under it.");
    let mut media = MediaCache::new(PathBuf::from("."));
    let mut fonts = FontStore::new();
    let plain = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &ViewConfig::default(),
        600.0,
    );
    let cfg = ViewConfig {
        comic: ComicFit::Page { height: 400.0 },
        ..ViewConfig::default()
    };
    let paged = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &cfg,
        600.0,
    );
    assert_eq!(plain.height, paged.height);
    assert_eq!(plain.runs.len(), paged.runs.len());
}

/// Stage-timing probe for real comics; run with
/// ORYX_BOOK=<path> cargo test --release --test comic field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn field_probe() {
    use std::time::Instant;
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let bytes = std::fs::read(&path).unwrap();
    let size = bytes.len();
    let stem = std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Comic")
        .to_string();

    let t = Instant::now();
    let book = comic::open_book(bytes, &stem).unwrap();
    let t_open = t.elapsed();

    let probed = book.pages.iter().filter(|(_, _, d)| d.is_some()).count();
    println!("{path}: {size} bytes");
    println!(
        "open: {}ms, {} pages, {} with header dimensions, {} toc entries",
        t_open.as_millis(),
        book.pages.len(),
        probed,
        book.toc.len()
    );
    for (key, _, dims) in book.pages.iter().take(5) {
        match dims {
            Some((w, h)) => println!("  {key}: {w}x{h}"),
            None => println!("  {key}: no header dimensions"),
        }
    }
}

#[test]
fn a_comic_opens_through_the_load_path() {
    let bytes = cbz(&[
        ("p1.jpg", &jpeg_bytes(10, 12)),
        ("p2.jpg", &jpeg_bytes(10, 14)),
    ]);
    let path = std::env::temp_dir().join("oryx-comic-load.cbz");
    std::fs::write(&path, &bytes).unwrap();
    let opened = load::open(&path, None).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(!opened.streamed, "a comic owes nothing after open");
    let mut book = opened
        .book
        .expect("a comic hands its sources through the job");
    assert!(!book.has_chapters(), "a comic has nothing to stream");
    let sources = book.take_sources();
    assert_eq!(sources.len(), 2);
    assert!(
        sources.iter().all(|(_, _, dims)| dims.is_some()),
        "sizes reach layout ahead of any pixel"
    );
    assert_eq!(opened.toc.len(), 2);
    assert_eq!(
        opened.document.title.as_deref(),
        Some("oryx-comic-load"),
        "the file stem names a book with no metadata"
    );
    assert!(
        opened.document.book_id.is_some(),
        "positions need a memory key"
    );
}

#[test]
fn rtl_pairing_puts_the_right_page_first() {
    let pages: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|n| (format!("{n}.png"), png_bytes(100, 100)))
        .collect();
    let entries: Vec<(&str, &[u8])> = pages
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let book = comic::open_book(cbz(&entries), "T").unwrap();
    let mut media = MediaCache::new(PathBuf::from("."));
    media.adopt(book.pages);
    let cfg = ViewConfig {
        comic: ComicFit::Two { height: 400.0 },
        direction: DirectionMode::Rtl,
        ..ViewConfig::default()
    };
    let mut fonts = FontStore::new();
    let l = layout(
        &book.document,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &cfg,
        600.0,
    );
    assert_eq!(
        rect(&l.images[0]),
        (100.0, 0.0, 400.0, 400.0),
        "the cover stands alone, centered, whatever the direction"
    );
    assert_eq!(
        rect(&l.images[1]),
        (300.0, 450.0, 300.0, 300.0),
        "the first page of a spread sits right"
    );
    assert_eq!(
        rect(&l.images[2]),
        (0.0, 450.0, 300.0, 300.0),
        "its pair continues on the left"
    );
    assert_eq!(
        rect(&l.images[3]),
        (300.0, 850.0, 300.0, 300.0),
        "a last odd page takes the right of its own row"
    );
}
