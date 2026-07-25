//! Performance validation against the budgets. Ignored by default; run
//! release mode locally with:
//!   cargo test --release --test perf -- --ignored --nocapture --test-threads=1
//! Parallel test threads contend for cores and skew every timing.

use std::path::PathBuf;
use std::time::Instant;

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::markdown;
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

/// The syntect share of a parsed document: every code block highlighted
/// again on warm grammars, isolated from the rest of parsing.
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

fn measure(bytes: usize) -> (u128, u128, f32) {
    let source = large_gen::generate(bytes);
    let started = Instant::now();
    let doc = markdown::parse(&source);
    let parse_ms = started.elapsed().as_millis();
    let (layout_ms, height) = measure_layout(&doc);
    (parse_ms, layout_ms, height)
}

/// The product promise: typical documents open instantly. A 64KB mixed
/// document with dense code blocks is already a long, heavy README.
/// Run alone for a true cold number; in a full suite run another test
/// may already have paid the grammar and font warm-up.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn typical_document_meets_the_budget() {
    let (parse_ms, layout_ms, height) = measure(64 * 1024);
    println!("typical: parse {parse_ms}ms, layout {layout_ms}ms, height {height:.0}px");
    assert!(height > 10_000.0, "fixture laid out");
    if !cfg!(debug_assertions) {
        assert!(
            parse_ms + layout_ms < 150,
            "budget exceeded: parse {parse_ms}ms + layout {layout_ms}ms"
        );
    }
}

/// The lazy-highlighting before/after table: markdown parse with its
/// syntect share isolated, whole-file code open, and layout, per tier.
/// Records numbers without asserting budgets; the first markdown row
/// pays the one-time grammar and font warm-up, as a real cold start
/// does. The huge tier takes a minute or two; that stall is the point.
#[test]
#[ignore = "measurement only"]
fn tiers_measured() {
    for (name, bytes) in TIERS {
        let source = large_gen::generate(*bytes);
        let started = Instant::now();
        let doc = markdown::parse(&source);
        let parse_ms = started.elapsed().as_millis();
        let highlight_ms = measure_highlight(&doc);
        let (layout_ms, height) = measure_layout(&doc);
        assert!(height > 0.0, "markdown fixture laid out");
        println!(
            "md   {name:>6}: parse {parse_ms:>5}ms (highlight {highlight_ms:>5}ms), \
             layout {layout_ms:>5}ms"
        );

        let source = large_gen::generate_code(*bytes);
        let path = std::env::temp_dir().join(format!("oryx-perf-{}.rs", std::process::id()));
        std::fs::write(&path, &source).expect("write code fixture");
        let started = Instant::now();
        let doc = load::open(&path).expect("open code fixture");
        let open_ms = started.elapsed().as_millis();
        std::fs::remove_file(&path).ok();
        let (layout_ms, height) = measure_layout(&doc);
        assert!(height > 0.0, "code fixture laid out");
        println!("code {name:>6}: open  {open_ms:>5}ms, layout {layout_ms:>5}ms");
    }
}
