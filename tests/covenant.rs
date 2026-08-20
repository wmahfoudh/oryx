//! The byte covenant, proven over a corpus: opening a file and saving
//! it with zero edits writes the original bytes back, and any single
//! edit's effect on the written file stays confined to the lines it
//! touched. The corpus is every shipped example plus this crate's own
//! sources, read through the real loader, with synthetic mixed-ending
//! fixtures beside them; files the loader flags lossy are skipped, as
//! the editing door refuses them.

use std::path::PathBuf;
use std::sync::Arc;

use oryx::doc::load;
use oryx::edit::splice::Ledger;

/// Deterministic pseudo-random stream; the covenant needs varied edits,
/// not true randomness, and a fixed seed keeps every failure
/// reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for dir in ["examples", "src", "src/edit"] {
        let Ok(entries) = std::fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "md" | "txt" | "rs" | "toml") {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&path) {
                files.push((path.display().to_string(), bytes));
            }
        }
    }
    files.push((
        "synthetic: crlf".into(),
        b"first\r\nsecond\r\nthird\r\n".to_vec(),
    ));
    files.push((
        "synthetic: mixed endings".into(),
        b"lf line\ncrlf line\r\nlf again\nlast\r\n".to_vec(),
    ));
    files.push(("synthetic: no terminator".into(), b"one\ntwo".to_vec()));
    files.push(("synthetic: blank tail".into(), b"body\n\n\n".to_vec()));
    files.push(("synthetic: empty".into(), Vec::new()));
    files.push((
        "synthetic: byte order mark".into(),
        b"\xEF\xBB\xBF# Title\nbody\n".to_vec(),
    ));
    files.push((
        "synthetic: byte order mark and crlf".into(),
        b"\xEF\xBB\xBFone\r\ntwo\r\n".to_vec(),
    ));
    files.push((
        "synthetic: byte order mark alone".into(),
        b"\xEF\xBB\xBF".to_vec(),
    ));
    assert!(files.len() > 10, "the corpus found the shipped files");
    files
}

/// Opens the bytes through the real loader, as the editor would: the
/// normalized text, the recorded CRLF positions and the byte order
/// mark flag, or None for a lossy read.
fn open_normalized(bytes: &[u8]) -> Option<(Arc<str>, Vec<u32>, bool)> {
    // The two covenant tests walk the corpus concurrently in this
    // process, so the temp path must be unique per call, never derived
    // from the file alone: a shared path lets one test truncate or
    // remove the file between the other's write and read.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("oryx-covenant-{}-{unique}.txt", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let opened = load::open(&path, None);
    std::fs::remove_file(&path).ok();
    let opened = opened.ok()?;
    if opened.lossy {
        return None;
    }
    Some((Arc::clone(&opened.document.source), opened.crlf, opened.bom))
}

fn file_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes.split(|&c| c == b'\n').map(<[u8]>::to_vec).collect()
}

#[test]
fn a_zero_edit_save_is_byte_identical_across_the_corpus() {
    for (name, bytes) in corpus() {
        let Some((source, crlf, bom)) = open_normalized(&bytes) else {
            continue;
        };
        let mut ledger = Ledger::new(source, crlf).with_bom(bom);
        assert_eq!(
            ledger.commit(),
            bytes,
            "zero edits round-trip byte-identical: {name}"
        );
    }
}

#[test]
fn a_single_edit_stays_confined_to_its_lines() {
    let mut rng = Lcg(0x6f727978);
    for (name, bytes) in corpus() {
        let Some((source, crlf, bom)) = open_normalized(&bytes) else {
            continue;
        };
        for _ in 0..8 {
            let mut ledger = Ledger::new(Arc::clone(&source), crlf.clone()).with_bom(bom);
            // One random edit on char boundaries: an insertion, a
            // deletion, or a replacement.
            let pos = {
                let mut p = rng.next(source.len() + 1);
                while !source.is_char_boundary(p) {
                    p -= 1;
                }
                p
            };
            let end = {
                let mut e = (pos + rng.next(24)).min(source.len());
                while !source.is_char_boundary(e) {
                    e -= 1;
                }
                e.max(pos)
            };
            let insert = ["x", "words typed in", "line\nbreak", "\n", ""][rng.next(5)];
            if pos == end && insert.is_empty() {
                continue;
            }
            ledger.edit(pos..end, insert);
            let written = ledger.emit();
            // The normalized text and the file hold the same newline
            // count, so the edit's line span carries over to the file's
            // own lines; everything outside it is written back
            // byte-identical, endings included.
            let start_line = source[..pos].matches('\n').count();
            let end_line = source[..end].matches('\n').count();
            let old_lines = file_lines(&bytes);
            let new_lines = file_lines(&written);
            let removed = end_line - start_line + 1;
            let added = new_lines.len() + removed - old_lines.len();
            assert_eq!(
                old_lines[..start_line],
                new_lines[..start_line],
                "the prefix is untouched: {name} at {pos}..{end}"
            );
            assert_eq!(
                old_lines[end_line + 1..],
                new_lines[start_line + added..],
                "the tail is untouched: {name} at {pos}..{end}"
            );
        }
    }
}
