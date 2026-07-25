//! Performance validation against the budgets. Ignored by default; run
//! release mode locally with:
//!   cargo test --release --test perf -- --ignored --nocapture --test-threads=1
//! Parallel test threads contend for cores and skew every timing.

use std::path::PathBuf;
use std::time::Instant;

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::model::{BlockKind, Document};
use oryx::layout::{layout, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::highlight;
use oryx::style::theme::Theme;

#[path = "fixtures/large_gen.rs"]
mod large_gen;

/// Byte sizes for the measurement tiers, shared by the markdown and the
/// whole-file code fixtures so their rows compare directly.
const TIERS: &[(&str, usize)] = &[
    ("small", 64 * 1024),
    ("medium", 256 * 1024),
    ("large", 1024 * 1024),
    ("huge", 8 * 1024 * 1024),
];

/// The app's open path: the fixture written to disk, then read, parsed,
/// and highlighted inside the sync budget.
fn measure_open(source: &str, ext: &str) -> (u128, Document) {
    let path = std::env::temp_dir().join(format!("oryx-perf-{}.{ext}", std::process::id()));
    std::fs::write(&path, source).expect("write fixture");
    let started = Instant::now();
    let opened = load::open(&path, Some(Instant::now() + load::OPEN_BUDGET)).expect("open fixture");
    let ms = started.elapsed().as_millis();
    std::fs::remove_file(&path).ok();
    (ms, opened.document)
}

fn measure_layout(doc: &Document) -> (u128, f32) {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let started = Instant::now();
    let l = layout(
        doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &ViewConfig::default(),
        1200.0,
    );
    (started.elapsed().as_millis(), l.height)
}

/// The syntect cost that lazy highlighting moves off the open path:
/// every code block highlighted in full on warm grammars.
fn measure_highlight(doc: &Document) -> u128 {
    let started = Instant::now();
    for block in &doc.blocks {
        if let BlockKind::CodeBlock {
            language, lines, ..
        } = &block.kind
        {
            let _ = highlight::spans(lines, language.as_deref());
        }
    }
    started.elapsed().as_millis()
}

/// The product promise: typical documents open instantly. A 64KB mixed
/// document with dense code blocks is already a long, heavy README.
/// Run alone for a true cold number; in a full suite run another test
/// may already have paid the grammar and font warm-up.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn typical_document_meets_the_budget() {
    let (open_ms, doc) = measure_open(&large_gen::generate(64 * 1024), "md");
    let (layout_ms, height) = measure_layout(&doc);
    println!("typical: open {open_ms}ms, layout {layout_ms}ms, height {height:.0}px");
    assert!(height > 10_000.0, "fixture laid out");
    if !cfg!(debug_assertions) {
        assert!(
            open_ms + layout_ms < 150,
            "budget exceeded: open {open_ms}ms + layout {layout_ms}ms"
        );
    }
}

/// The lazy-highlighting tier table: budgeted open and layout per tier,
/// with the full highlight cost that now runs on the background worker
/// isolated beside them. Records numbers without asserting budgets; the
/// first row pays the one-time grammar and font warm-up, as a real cold
/// start does. The huge highlight columns take a minute or two; that
/// work leaving the open path is the point.
#[test]
#[ignore = "measurement only"]
fn tiers_measured() {
    for (name, bytes) in TIERS {
        let (open_ms, doc) = measure_open(&large_gen::generate(*bytes), "md");
        let highlight_ms = measure_highlight(&doc);
        let (layout_ms, height) = measure_layout(&doc);
        assert!(height > 0.0, "markdown fixture laid out");
        println!(
            "md   {name:>6}: open {open_ms:>5}ms (highlight {highlight_ms:>5}ms), \
             layout {layout_ms:>5}ms"
        );

        let (open_ms, doc) = measure_open(&large_gen::generate_code(*bytes), "rs");
        let highlight_ms = measure_highlight(&doc);
        let (layout_ms, height) = measure_layout(&doc);
        assert!(height > 0.0, "code fixture laid out");
        println!(
            "code {name:>6}: open {open_ms:>5}ms (highlight {highlight_ms:>5}ms), \
             layout {layout_ms:>5}ms"
        );
    }
}
