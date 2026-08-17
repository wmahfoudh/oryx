//! KF8: the skeleton and fragment reassembly, flows, the dual-file
//! boundary, the NCX outline, and the contradiction guards.

#[path = "fixtures/writer.rs"]
mod writer;

use palmbook::{kf8, Book, Error};
use writer::{IndxEntry, Skeleton};

fn two_part_book() -> writer::BookBuilder {
    writer::kf8_book(
        &[
            Skeleton {
                text: "<html><body></body></html>",
                fragments: vec![(12, "<p>one</p>"), (22, "<p>two</p>")],
            },
            Skeleton {
                text: "<html><body></body></html>",
                fragments: vec![(12, "<p>three</p>")],
            },
        ],
        "p { margin: 0 }",
    )
}

#[test]
fn skeletons_and_fragments_stitch_into_parts() {
    let bytes = two_part_book().build();
    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.version(), 8);
    let kf8 = kf8::read(&book).unwrap();

    assert_eq!(kf8.parts.len(), 2);
    assert_eq!(
        kf8.parts[0].body,
        b"<html><body><p>one</p><p>two</p></body></html>"
    );
    assert_eq!(kf8.parts[1].body, b"<html><body><p>three</p></body></html>");
    assert_eq!(kf8.flows.len(), 2);
    assert_eq!(kf8.flows[1], b"p { margin: 0 }");
    assert!(kf8.flows[0].is_empty(), "flow 0 moved into the parts whole");
}

#[test]
fn fragments_report_their_place_in_the_parts() {
    let bytes = two_part_book().build();
    let book = Book::open(&bytes).unwrap();
    let kf8 = kf8::read(&book).unwrap();

    assert_eq!(kf8.fragments.len(), 3);
    let one = &kf8.fragments[0];
    assert_eq!(one.part, 0);
    assert_eq!(
        &kf8.parts[one.part].body[one.offset..one.offset + one.length],
        b"<p>one</p>",
        "the fragment's recorded range holds its own text"
    );
    let two = &kf8.fragments[1];
    assert_eq!(
        &kf8.parts[two.part].body[two.offset..two.offset + two.length],
        b"<p>two</p>"
    );
    let three = &kf8.fragments[2];
    assert_eq!(three.part, 1);
    assert_eq!(
        &kf8.parts[three.part].body[three.offset..three.offset + three.length],
        b"<p>three</p>"
    );
    assert_eq!(one.aid, "aid-0-12");
}

#[test]
fn a_dual_file_finds_the_kf8_payload() {
    let mobi6 = {
        let mut builder = writer::book("The old flow, for old devices.");
        builder.exth = vec![(100, b"A. Author".to_vec())];
        builder
    };
    let mut mobi6_records = mobi6.records();
    let boundary = mobi6_records.len() as u32 + 1;
    // EXTH 121 names the record the KF8 half starts at; rebuild the
    // MOBI6 record 0 with it, then append a boundary marker and the
    // KF8 records.
    let mobi6 = {
        let mut builder = writer::book("The old flow, for old devices.");
        builder.exth = vec![
            (100, b"A. Author".to_vec()),
            (121, boundary.to_be_bytes().to_vec()),
        ];
        builder
    };
    mobi6_records = mobi6.records();
    mobi6_records.push(b"BOUNDARY".to_vec());
    let kf8_records = two_part_book().records();
    mobi6_records.extend(kf8_records);
    let bytes = writer::pdb("dual-test", b"BOOK", b"MOBI", &mobi6_records);

    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.version(), 6);
    let start = book.kf8_boundary().expect("EXTH 121 names the boundary");
    assert_eq!(start, boundary as usize);
    let inner = Book::open_at(&bytes, start).unwrap();
    assert_eq!(inner.version(), 8);
    let kf8 = kf8::read(&inner).unwrap();
    assert_eq!(kf8.parts.len(), 2);
    assert_eq!(kf8.parts[1].body, b"<html><body><p>three</p></body></html>");
}

#[test]
fn the_ncx_index_reads_into_toc_points() {
    let (cncx_record, offsets) = writer::cncx(&["Part One", "Inside One"]);
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
                tags: vec![(3, vec![offsets[1]]), (6, vec![1, 4]), (21, vec![0])],
            },
        ],
        Some(cncx_record),
    );
    let mut builder = two_part_book();
    builder.ncxidx = builder.extra_base() + builder.extra_records.len() as u32;
    builder.extra_records.extend(ncx);
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    let kf8 = kf8::read(&book).unwrap();

    assert_eq!(kf8.toc.len(), 2);
    assert_eq!(kf8.toc[0].label, "Part One");
    assert_eq!(kf8.toc[0].depth, 0);
    assert_eq!(kf8.toc[0].target, Some((0, 0)));
    assert_eq!(kf8.toc[1].label, "Inside One");
    assert_eq!(kf8.toc[1].depth, 1, "a parented point nests");
    assert_eq!(kf8.toc[1].target, Some((1, 4)));
}

#[test]
fn a_contradicting_table_errors_instead_of_mis_stitching() {
    // A skeleton whose declared geometry runs past flow 0.
    let mut builder = two_part_book();
    let skel = writer::indx_records(
        &[(1, 1, 0x01, 0), (6, 2, 0x02, 0), (0, 0, 0, 1)],
        &[IndxEntry {
            name: b"SKEL0000000000".to_vec(),
            tags: vec![(1, vec![0]), (6, vec![0, 100_000])],
        }],
        None,
    );
    let base = builder.extra_base();
    builder.extra_records.splice(0..2, skel);
    builder.skelidx = base;
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    match kf8::read(&book) {
        Err(Error::Corrupt(_)) => {}
        Ok(_) => panic!("a skeleton past the flow must error, not stitch"),
        Err(other) => panic!("expected corrupt, got {other:?}"),
    }

    // A fragment whose insert position lies outside its skeleton.
    let mut builder = two_part_book();
    let frag = writer::indx_records(
        &[
            (2, 1, 0x01, 0),
            (3, 1, 0x02, 0),
            (4, 1, 0x04, 0),
            (6, 2, 0x08, 0),
            (0, 0, 0, 1),
        ],
        &[IndxEntry {
            name: b"90000".to_vec(),
            tags: vec![(2, vec![0]), (3, vec![0]), (4, vec![0]), (6, vec![0, 5])],
        }],
        Some(writer::cncx(&["aid-x"]).0),
    );
    let base = builder.extra_base();
    builder.extra_records.splice(2..5, frag);
    builder.fragidx = base + 2;
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    match kf8::read(&book) {
        Err(Error::Corrupt(_)) => {}
        Ok(_) => panic!("a fragment outside its skeleton must error"),
        Err(other) => panic!("expected corrupt, got {other:?}"),
    }
}

/// Probe for real KF8 books; run with
/// PALMBOOK_FILE=<path> cargo test --release --test kf8 kf8_field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn kf8_field_probe() {
    let path = std::env::var("PALMBOOK_FILE").expect("set PALMBOOK_FILE");
    let bytes = std::fs::read(&path).unwrap();
    let start = std::time::Instant::now();
    let book = Book::open(&bytes).unwrap();
    let book = if book.version() >= 8 {
        book
    } else {
        let boundary = book.kf8_boundary().expect("no KF8 half in this file");
        Book::open_at(&bytes, boundary).unwrap()
    };
    println!(
        "version {}, kf8 header {:?}, rawml {} bytes",
        book.version(),
        book.kf8_header(),
        book.rawml().map(|t| t.len()).unwrap_or(0)
    );
    if let Some(header) = book.kf8_header() {
        if header.fdst != 0xFFFF_FFFF {
            let record = book.record(header.fdst as usize).unwrap();
            let n = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
            print!("fdst {n} flows:");
            for i in 0..n.min(4) {
                let s = u32::from_be_bytes(record[12 + i * 8..16 + i * 8].try_into().unwrap());
                let e = u32::from_be_bytes(record[16 + i * 8..20 + i * 8].try_into().unwrap());
                print!(" {s}..{e}");
            }
            println!();
        }
    }
    let kf8 = kf8::read(&book).unwrap();
    let elapsed = start.elapsed();
    println!(
        "{path}: {} parts, {} flows, {} fragments, {} toc points in {}ms",
        kf8.parts.len(),
        kf8.flows.len(),
        kf8.fragments.len(),
        kf8.toc.len(),
        elapsed.as_millis()
    );
    for part in kf8.parts.iter().take(3) {
        let head = String::from_utf8_lossy(&part.body[..part.body.len().min(90)]);
        println!("  {}: {} bytes, head {head:?}", part.name, part.body.len());
    }
    for point in kf8.toc.iter().take(10) {
        println!(
            "  toc {}{} -> {:?}",
            "  ".repeat(point.depth as usize),
            point.label,
            point.target
        );
    }
    for part in &kf8.parts {
        assert!(
            part.body.first() == Some(&b'<'),
            "part {} does not open as markup",
            part.name
        );
    }
}
