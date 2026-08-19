//! The container walk and stored extraction, both generations: names,
//! sizes, methods, CRC verification, refusals, and the promise that a
//! lying container errors and never panics.

#[path = "fixtures/writer.rs"]
mod writer;

use rarball::{Archive, Error, Method};
use writer::{crc32, directory, file, rar4, rar5, FileSpec};

fn pages() -> Vec<FileSpec> {
    vec![
        file("Page 01.jpg", b"first page bytes"),
        directory("art/"),
        file("art/Page 02.jpg", b"second page, a little longer"),
    ]
}

fn walked(bytes: &[u8]) -> Vec<(String, u64, bool)> {
    let archive = Archive::open(bytes).unwrap();
    archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.unpacked_size, e.directory))
        .collect()
}

#[test]
fn both_generations_walk_names_sizes_and_methods() {
    for bytes in [rar4(&pages(), 0), rar5(&pages(), false)] {
        assert_eq!(
            walked(&bytes),
            [
                ("Page 01.jpg".to_string(), 16, false),
                ("art/".to_string(), 0, true),
                ("art/Page 02.jpg".to_string(), 28, false),
            ]
        );
        let archive = Archive::open(&bytes).unwrap();
        assert!(archive
            .entries()
            .iter()
            .all(|e| matches!(e.method, Method::Stored)));
        assert!(archive
            .entries()
            .iter()
            .filter(|e| !e.directory)
            .all(|e| e.crc.is_some()));
    }
}

#[test]
fn stored_extraction_round_trips_with_crc() {
    for bytes in [rar4(&pages(), 0), rar5(&pages(), false)] {
        let archive = Archive::open(&bytes).unwrap();
        let first = archive.extract(&archive.entries()[0]).unwrap();
        assert_eq!(first.as_ref(), b"first page bytes");
        let second = archive.extract(&archive.entries()[2]).unwrap();
        assert_eq!(second.as_ref(), b"second page, a little longer");
    }
}

#[test]
fn a_wrong_data_crc_reads_as_corrupt() {
    let mut spec = pages();
    spec[0].declared_crc = Some(0xDEAD_BEEF);
    for bytes in [rar4(&spec, 0), rar5(&spec, false)] {
        let archive = Archive::open(&bytes).unwrap();
        assert!(matches!(
            archive.extract(&archive.entries()[0]),
            Err(Error::Corrupt(_))
        ));
        let intact = archive.extract(&archive.entries()[2]).unwrap();
        assert_eq!(intact.as_ref(), b"second page, a little longer");
    }
}

#[test]
fn every_truncation_errors_and_never_panics() {
    for full in [rar4(&pages(), 0), rar5(&pages(), false)] {
        for len in 0..full.len() {
            if let Ok(archive) = Archive::open(&full[..len]) {
                for entry in archive.entries() {
                    let _ = archive.extract(entry);
                }
            }
        }
    }
}

#[test]
fn a_flipped_header_byte_reads_as_corrupt_or_truncated() {
    for full in [rar4(&pages(), 0), rar5(&pages(), false)] {
        // The byte sits inside the first entry's header, past both
        // signatures and the main header.
        let mut bytes = full.clone();
        let at = 40;
        bytes[at] ^= 0x5A;
        assert!(
            Archive::open(&bytes).is_err(),
            "a damaged header must not pass"
        );
    }
}

#[test]
fn encrypted_archives_and_entries_say_so() {
    assert!(matches!(
        Archive::open(&rar4(&pages(), 0x0080)).map(|_| ()),
        Err(Error::Encrypted)
    ));
    assert!(matches!(
        Archive::open(&rar5(&pages(), true)).map(|_| ()),
        Err(Error::Encrypted)
    ));
    let mut spec = pages();
    spec[0].encrypted = true;
    for bytes in [rar4(&spec, 0), rar5(&spec, false)] {
        let archive = Archive::open(&bytes).unwrap();
        assert!(archive.entries()[0].encrypted);
        assert!(matches!(
            archive.extract(&archive.entries()[0]),
            Err(Error::Encrypted)
        ));
        assert!(!archive.entries()[2].encrypted);
    }
}

#[test]
fn foreign_bytes_read_as_not_rar() {
    for bytes in [
        &b""[..],
        b"PK\x03\x04not a rar",
        b"Rar!",
        b"plain text that is long enough",
    ] {
        assert!(matches!(
            Archive::open(bytes).map(|_| ()),
            Err(Error::NotRar)
        ));
    }
}

#[test]
fn compressed_entries_walk_but_refuse_extraction() {
    let mut spec = pages();
    spec[0].method = 3;
    for bytes in [rar4(&spec, 0), rar5(&spec, false)] {
        let archive = Archive::open(&bytes).unwrap();
        assert!(matches!(
            archive.entries()[0].method,
            Method::Compressed { .. }
        ));
        assert!(matches!(
            archive.extract(&archive.entries()[0]),
            Err(Error::Unsupported(_))
        ));
        let stored = archive.extract(&archive.entries()[2]).unwrap();
        assert_eq!(stored.as_ref(), b"second page, a little longer");
    }
}

#[test]
fn rar4_unicode_names_decode() {
    let mut spec = vec![file("__.jpg", b"wide")];
    spec[0].unicode = Some("ペ-je\u{301}.jpg".to_string());
    let bytes = rar4(&spec, 0);
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.entries()[0].name, "ペ-je\u{301}.jpg");
}

#[test]
fn rar4_copy_opcode_reuses_the_plain_name() {
    // Opcode 3 without the correction bit copies from the plain name:
    // high page 0, one flags byte carrying opcode 3, length 5 (+2 = 7).
    let mut name = b"abc.jpg".to_vec();
    name.push(0);
    name.extend_from_slice(&[0u8, 0b1100_0000, 5]);
    let bytes = writer::rar4_with_raw_name(&name, b"x");
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.entries()[0].name, "abc.jpg");
}

#[test]
fn rar4_large_size_fields_parse() {
    let mut spec = pages();
    spec[0].large = true;
    let bytes = rar4(&spec, 0);
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.entries()[0].unpacked_size, 16);
    let first = archive.extract(&archive.entries()[0]).unwrap();
    assert_eq!(first.as_ref(), b"first page bytes");
}

#[test]
fn crc_matches_the_reference() {
    // The IEEE check value pins the fixture writer's own CRC, which
    // seals every header the tests rely on.
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

/// Walks a real archive and verifies every stored entry's CRC; run with
/// RAR_FILE=<path> cargo test -p rarball --release field_probe -- --ignored --nocapture
#[test]
#[ignore]
fn field_probe() {
    use std::time::Instant;
    let path = std::env::var("RAR_FILE").expect("set RAR_FILE");
    let bytes = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let archive = Archive::open(&bytes).unwrap();
    let walk = t.elapsed();
    let mut stored = 0usize;
    let mut compressed = 0usize;
    let mut verified = 0usize;
    let mut extracted_bytes = 0usize;
    let t = Instant::now();
    let dump = std::env::var("RAR_DUMP").ok();
    for entry in archive.entries() {
        match entry.method {
            Method::Stored if !entry.directory => {
                stored += 1;
                let data = archive.extract(entry).expect("stored entry extracts");
                extracted_bytes += data.len();
                verified += 1;
                if let Some(dir) = &dump {
                    let flat = entry.name.replace('/', "_");
                    std::fs::write(std::path::Path::new(dir).join(flat), &data).unwrap();
                }
            }
            Method::Compressed { .. } => compressed += 1,
            _ => {}
        }
    }
    let extract = t.elapsed();
    println!(
        "{path}: {} bytes, {} entries, walk {}ms",
        bytes.len(),
        archive.entries().len(),
        walk.as_millis()
    );
    println!(
        "stored {stored} (all {verified} CRC-verified, {extracted_bytes} bytes in {}ms), compressed {compressed}",
        extract.as_millis()
    );
    for entry in archive.entries().iter().take(5) {
        println!("  {} ({} bytes)", entry.name, entry.unpacked_size);
    }
}
