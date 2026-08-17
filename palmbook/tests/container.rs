//! The Palm container: the record walk, both decompressors, the rawml
//! assembly, metadata, refusals, and the no-panic guarantee on hostile
//! input.

#[path = "fixtures/writer.rs"]
mod writer;

use palmbook::{Book, Compression, Error, Pdb, TextEncoding};

#[test]
fn the_pdb_shell_reads_name_creator_and_records() {
    let records = vec![b"alpha".to_vec(), b"beta-record".to_vec(), b"g".to_vec()];
    let bytes = writer::pdb("shell-test", b"BOOK", b"MOBI", &records);
    let pdb = Pdb::open(&bytes).unwrap();
    assert_eq!(pdb.name(), "shell-test");
    assert_eq!(pdb.type_code(), *b"BOOK");
    assert_eq!(pdb.creator(), *b"MOBI");
    assert_eq!(pdb.len(), 3);
    assert_eq!(pdb.record(0).unwrap(), b"alpha");
    assert_eq!(pdb.record(1).unwrap(), b"beta-record");
    assert_eq!(pdb.record(2).unwrap(), b"g");
    assert!(pdb.record(3).is_err(), "a record past the table errors");
}

#[test]
fn an_uncompressed_book_assembles_its_rawml() {
    let text = "A little text that still spans records.".repeat(400);
    let mut builder = writer::book(&text);
    builder.record_size = 512;
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.compression(), Compression::None);
    assert_eq!(book.encoding(), TextEncoding::Utf8);
    assert_eq!(book.title().as_deref(), Some("Test Book"));
    assert_eq!(book.rawml().unwrap(), text.as_bytes());
}

#[test]
fn palmdoc_hand_vectors_decode() {
    // Literals, a run, the space fold, and an overlapping backreference,
    // each byte placed by hand against the format description.
    let mut out = Vec::new();
    palmbook::palmdoc::decompress(b"abc", &mut out).unwrap();
    assert_eq!(out, b"abc");

    let mut out = Vec::new();
    palmbook::palmdoc::decompress(&[0x02, 0xC3, 0x00], &mut out).unwrap();
    assert_eq!(out, [0xC3, 0x00], "a length-2 run carries raw bytes");

    let mut out = Vec::new();
    palmbook::palmdoc::decompress(&[b'a', 0xC2], &mut out).unwrap();
    assert_eq!(out, b"a B", "0xC2 unfolds to space plus B");

    // "abc" then distance 3, length 3: "abcabc".
    let mut out = Vec::new();
    palmbook::palmdoc::decompress(&[b'a', b'b', b'c', 0x80, 0x18], &mut out).unwrap();
    assert_eq!(out, b"abcabc", "distance 3 length 3 copies the run");

    // "ab" then distance 1, length 4: the overlapping self-copy.
    let mut out = Vec::new();
    palmbook::palmdoc::decompress(&[b'a', b'b', 0x80, 0x09], &mut out).unwrap();
    assert_eq!(out, b"abbbbb", "an overlapping copy repeats the last byte");

    let mut out = Vec::new();
    assert!(
        palmbook::palmdoc::decompress(&[0x80, 0x18], &mut out).is_err(),
        "a backreference into nothing is corrupt, not a panic"
    );
}

#[test]
fn a_palmdoc_book_round_trips() {
    let text = "Compressed prose, with spaces And Caps to fold. ".repeat(300);
    let mut builder = writer::book(&text);
    builder.compression = writer::COMPRESSION_PALMDOC;
    builder.record_size = 1024;
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.compression(), Compression::PalmDoc);
    assert_eq!(book.rawml().unwrap(), text.as_bytes());
}

#[test]
fn huffcdic_decodes_the_hand_built_tables() {
    // Phrase 2 is not final: its bytes are themselves codes, and the
    // decoder recurses through them.
    let phrases: Vec<(&[u8], bool)> = vec![
        (b"Hello", true),
        (b", world", true),
        (&[0, 1], false),
        (b"!", true),
    ];
    let tables = writer::huff_records(&phrases);
    let coder = palmbook::huffcdic::HuffCdic::new(&tables[0], &[&tables[1]]).unwrap();
    assert_eq!(coder.unpack(&[0, 1, 3]).unwrap(), b"Hello, world!");
    assert_eq!(
        coder.unpack(&[2, 3]).unwrap(),
        b"Hello, world!",
        "a non-final phrase expands through its own codes"
    );
}

#[test]
fn a_huffcdic_book_round_trips() {
    let text = "Hello, world! And the dictionary carries every phrase.";
    let phrases: Vec<(&[u8], bool)> = vec![
        (b"Hello, world! ", true),
        (b"And the dictionary ", true),
        (b"carries every phrase.", true),
    ];
    let mut builder = writer::book(text);
    builder.compression = writer::COMPRESSION_HUFFCDIC;
    builder.text = vec![0, 1, 2]; // the code stream stands in for the text records
    builder.huff_records = writer::huff_records(&phrases);
    // The writer declared the code stream's length; the reader must see
    // the decoded one.
    let built = patch_text_length(builder.build(), text.len() as u32);
    let book = Book::open(&built).unwrap();
    assert_eq!(book.compression(), Compression::HuffCdic);
    assert_eq!(book.rawml().unwrap(), text.as_bytes());
}

/// Rewrites the declared text length inside record 0.
fn patch_text_length(mut bytes: Vec<u8>, length: u32) -> Vec<u8> {
    let pdb = Pdb::open(&bytes).unwrap();
    let record0 = pdb.record(0).unwrap();
    let start = record0.as_ptr() as usize - bytes.as_ptr() as usize;
    bytes[start + 4..start + 8].copy_from_slice(&length.to_be_bytes());
    bytes
}

#[test]
fn trailing_entries_strip_before_decompression() {
    let text = "Trailing bytes ride every record and never reach the text.".repeat(40);
    let mut builder = writer::book(&text);
    builder.record_size = 512;
    builder.extra_flags = 0x3; // one backward entry plus the multibyte overlap
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.rawml().unwrap(), text.as_bytes());
}

#[test]
fn drm_refuses_before_any_output() {
    let mut builder = writer::book("locked text");
    builder.encryption = 2;
    let bytes = builder.build();
    match Book::open(&bytes).err() {
        Some(Error::Drm) => {}
        other => panic!("a DRM book must refuse as DRM, got {other:?}"),
    }
}

#[test]
fn exth_metadata_reads() {
    let mut builder = writer::book("metadata text");
    builder.exth = vec![
        (100, b"A. Author".to_vec()),
        (503, b"Updated Title".to_vec()),
    ];
    let bytes = builder.build();
    let book = Book::open(&bytes).unwrap();
    assert_eq!(book.exth_string(100).as_deref(), Some("A. Author"));
    assert_eq!(book.exth_string(503).as_deref(), Some("Updated Title"));
    assert_eq!(book.exth_string(999), None);
}

#[test]
fn not_a_palm_file_refuses() {
    match Book::open(b"just some text, no container").err() {
        Some(Error::NotPalm) | Some(Error::Truncated) => {}
        other => panic!("plain text must refuse, got {other:?}"),
    }
    let records = vec![b"x".to_vec()];
    let wrong = writer::pdb("not-a-book", b"TEXt", b"REAd", &records);
    match Book::open(&wrong).err() {
        Some(Error::NotPalm) => {}
        other => panic!("a foreign PDB must refuse as not palm, got {other:?}"),
    }
}

/// Probe for real books; run with
/// PALMBOOK_FILE=<path> cargo test --release --test container field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn field_probe() {
    let path = std::env::var("PALMBOOK_FILE").expect("set PALMBOOK_FILE");
    let bytes = std::fs::read(&path).unwrap();
    let start = std::time::Instant::now();
    let book = match Book::open(&bytes) {
        Ok(book) => book,
        Err(err) => {
            println!("{path}: refused, {err}");
            return;
        }
    };
    let rawml = book.rawml();
    let elapsed = start.elapsed();
    println!(
        "{path}: {} bytes, {:?}, {:?}, title {:?}, {} records, first image {:?}",
        bytes.len(),
        book.compression(),
        book.encoding(),
        book.title(),
        book.record_count(),
        book.first_image(),
    );
    match rawml {
        Ok(text) => {
            let head: String = String::from_utf8_lossy(&text[..text.len().min(120)]).into_owned();
            println!(
                "rawml: {} bytes in {}ms, head {head:?}",
                text.len(),
                elapsed.as_millis()
            );
        }
        Err(err) => println!("rawml failed: {err}"),
    }
}

#[test]
fn every_truncation_errors_and_never_panics() {
    let text = "Truncation fodder, long enough to cross records.".repeat(60);
    let mut builder = writer::book(&text);
    builder.compression = writer::COMPRESSION_PALMDOC;
    builder.record_size = 512;
    builder.exth = vec![(100, b"A. Author".to_vec())];
    let bytes = builder.build();
    assert!(Book::open(&bytes).is_ok(), "the whole file opens");
    for len in 0..bytes.len().min(2048) {
        if let Ok(book) = Book::open(&bytes[..len]) {
            let _ = book.rawml();
        }
    }
    for len in (0..bytes.len()).rev().step_by(101) {
        if let Ok(book) = Book::open(&bytes[..len]) {
            let _ = book.rawml();
        }
    }
}
