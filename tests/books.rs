//! The bar for the new book formats: generated fixtures held to the
//! open budget at every gate, and the corpus sweep that opens a whole
//! library and reports what refused.

#[path = "../palmbook/tests/fixtures/writer.rs"]
mod writer;

use std::path::{Path, PathBuf};

use oryx::doc::load::{self, FileKind};
use oryx::doc::{fb2, kindle};

/// A generated FB2 in the class of the library's real ones: hundreds of
/// titled chapters of styled prose, no images.
fn generated_fb2() -> Vec<u8> {
    let mut bodies = String::new();
    for chapter in 0..300 {
        bodies.push_str(&format!("<section><title><p>Chapter {chapter}</p></title>"));
        for paragraph in 0..18 {
            bodies.push_str(&format!(
                "<p>Paragraph {paragraph} of chapter {chapter}: prose with \
                 <emphasis>leaning</emphasis> words and <strong>louder</strong> ones, \
                 written long enough that a line wraps on the page the way a real \
                 book's does, and the layout pass earns its keep measuring it, \
                 rather than skimming through a toy fixture of short lines.</p>"
            ));
        }
        bodies.push_str("</section>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
         xmlns:l=\"http://www.w3.org/1999/xlink\">\n\
         <description><title-info><book-title>Generated FB2</book-title></title-info>\n\
         <document-info><id>oryx-gen-fb2</id></document-info></description>\n\
         <body><title><p>Generated FB2</p></title>{bodies}</body></FictionBook>"
    )
    .into_bytes()
}

/// A generated MOBI6 in the class of the library's real ones: PalmDOC
/// compression, pagebreak chapters, filepos links back to the start.
fn generated_mobi() -> Vec<u8> {
    let mut text = String::from("<html><body>");
    for chapter in 0..300 {
        if chapter > 0 {
            text.push_str("<mbp:pagebreak/>");
        }
        text.push_str(&format!("<h1>Chapter {chapter}</h1>"));
        for paragraph in 0..18 {
            text.push_str(&format!(
                "<p>Paragraph {paragraph} of chapter {chapter}: prose with <i>leaning</i> \
                 words and <b>louder</b> ones, written long enough that a line wraps on \
                 the page the way a real book's does, and the decompressor earns its \
                 keep on it, rather than skimming through a toy fixture, with a \
                 <a filepos=0000000000>return link</a> the anchor machinery resolves.</p>"
            ));
        }
    }
    text.push_str("</body></html>");
    let mut builder = writer::book(&text);
    builder.compression = writer::COMPRESSION_PALMDOC;
    builder.build()
}

/// The product promise holds for FB2: a real-sized book opens its
/// prefix inside the startup budget.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn a_generated_fb2_meets_the_open_budget() {
    let bytes = generated_fb2();
    let size = bytes.len();
    let t = std::time::Instant::now();
    let (doc, toc, job) = fb2::open_prefix(bytes).unwrap();
    let prefix_ms = t.elapsed().as_millis();
    println!(
        "generated fb2: {size} bytes, prefix {prefix_ms}ms, {} source bytes, \
         {} toc entries, worker owed: {}",
        doc.source.len(),
        toc.len(),
        job.is_some()
    );
    assert_eq!(toc.len(), 300, "every chapter outlines");
    if !cfg!(debug_assertions) {
        assert!(
            prefix_ms < 100,
            "the prefix must open inside the budget: {prefix_ms}ms"
        );
    }
}

/// The product promise holds for Kindle books: a real-sized MOBI opens
/// its prefix inside the startup budget.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn a_generated_mobi_meets_the_open_budget() {
    let bytes = generated_mobi();
    let size = bytes.len();
    let t = std::time::Instant::now();
    let (doc, _, job) = kindle::open_prefix(bytes).unwrap();
    let prefix_ms = t.elapsed().as_millis();
    println!(
        "generated mobi: {size} bytes, prefix {prefix_ms}ms, {} source bytes, \
         worker owed: {}",
        doc.source.len(),
        job.is_some()
    );
    assert!(doc.source.len() > 10_000, "the prefix holds real text");
    if !cfg!(debug_assertions) {
        assert!(
            prefix_ms < 100,
            "the prefix must open inside the budget: {prefix_ms}ms"
        );
    }
}

fn collect_books(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_books(&path, out);
        } else if matches!(load::detect(&path), FileKind::Fb2 | FileKind::Kindle) {
            out.push(path);
        }
    }
}

/// The corpus sweep: every FB2, MOBI and AZW3 under a directory opens
/// and walks whole, headless. Clean opens, refusals with their reasons
/// and the slowest books are reported; a panic anywhere fails the
/// sweep. Run with
/// ORYX_CORPUS=<dir> cargo test --release --test books corpus_sweep -- --ignored --nocapture
#[test]
#[ignore]
fn corpus_sweep() {
    let root = std::env::var("ORYX_CORPUS").expect("set ORYX_CORPUS");
    let mut files = Vec::new();
    collect_books(Path::new(&root), &mut files);
    files.sort();
    println!("sweeping {} books under {root}", files.len());

    let mut clean = 0usize;
    let mut refused: Vec<(PathBuf, String)> = Vec::new();
    let mut panicked: Vec<PathBuf> = Vec::new();
    let mut timings: Vec<(u128, usize, PathBuf)> = Vec::new();
    for path in &files {
        let t = std::time::Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let opened = load::open(path, None)?;
            let mut blocks = opened.document.blocks.len();
            if let Some(job) = opened.book {
                let sink: oryx::doc::images::SourceSink = std::sync::Arc::new(|_| {});
                if let Some(delivered) = job.run(&|| false, sink) {
                    blocks = delivered.blocks.len();
                }
            }
            Ok::<usize, anyhow::Error>(blocks)
        }));
        let ms = t.elapsed().as_millis();
        match outcome {
            Ok(Ok(blocks)) => {
                clean += 1;
                timings.push((ms, blocks, path.clone()));
            }
            Ok(Err(err)) => refused.push((path.clone(), err.to_string())),
            Err(_) => panicked.push(path.clone()),
        }
    }

    timings.sort_by_key(|&(ms, _, _)| std::cmp::Reverse(ms));
    println!(
        "\n{clean} of {} opened and walked clean, {} refused, {} panicked",
        files.len(),
        refused.len(),
        panicked.len()
    );
    for (path, reason) in &refused {
        println!("refused: {} ({reason})", path.display());
    }
    for (ms, blocks, path) in timings.iter().take(10) {
        println!("slow: {ms}ms, {blocks} blocks, {}", path.display());
    }
    assert!(panicked.is_empty(), "panics on: {panicked:#?}");
}
