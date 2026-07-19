//! Performance validation against the budgets. Ignored by default; run
//! release mode locally with: cargo test --release --test perf -- --ignored

use std::path::PathBuf;
use std::time::Instant;

use oryx::doc::images::MediaCache;
use oryx::doc::markdown;
use oryx::layout::{layout, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

#[path = "fixtures/large_gen.rs"]
mod large_gen;

fn measure(bytes: usize) -> (u128, u128, f32) {
    let source = large_gen::generate(bytes);
    let started = Instant::now();
    let doc = markdown::parse(&source);
    let parse_ms = started.elapsed().as_millis();
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let laid = Instant::now();
    let l = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &ViewConfig::default(),
        1200.0,
    );
    (parse_ms, laid.elapsed().as_millis(), l.height)
}

/// The product promise: typical documents open instantly. A 64KB mixed
/// document with dense code blocks is already a long, heavy README.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn typical_document_meets_the_budget() {
    let (parse_ms, layout_ms, height) = measure(64 * 1024);
    println!("typical: parse {parse_ms}ms, layout {layout_ms}ms, height {height:.0}px");
    assert!(height > 10_000.0, "fixture laid out");
    // The measurement includes the one-time syntect grammar and font
    // system warm-up, which real cold start pays too.
    if !cfg!(debug_assertions) {
        assert!(
            parse_ms + layout_ms < 150,
            "budget exceeded: parse {parse_ms}ms + layout {layout_ms}ms"
        );
    }
}

/// Huge files are outside the promise; this records the numbers without
/// asserting. Lazy highlighting is the parked future optimization.
#[test]
#[ignore = "measurement only"]
fn huge_document_measured() {
    let (parse_ms, layout_ms, height) = measure(256 * 1024);
    println!("large: parse {parse_ms}ms, layout {layout_ms}ms, height {height:.0}px");
    let (parse_ms, layout_ms, height) = measure(2 * 1024 * 1024);
    println!("huge: parse {parse_ms}ms, layout {layout_ms}ms, height {height:.0}px");
    assert!(height > 100_000.0, "fixture laid out");
}
