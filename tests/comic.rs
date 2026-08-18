//! Comic reading: page order, header dimensions, the outline,
//! detection and refusals.

#[path = "fixtures/comic_common.rs"]
mod comic_common;

use comic_common::{cbz, cbz_deflated, encrypted_cbz, jpeg_bytes, png_bytes};
use oryx::doc::comic;
use oryx::doc::epub;
use oryx::doc::images::{self, BookSource};
use oryx::doc::load::{self, FileKind};
use oryx::doc::model::{BlockKind, Document};

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
