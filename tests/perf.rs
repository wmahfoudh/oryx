//! Performance validation against the budgets. Ignored by default; run
//! release mode locally with:
//!   cargo test --release --test perf -- --ignored --nocapture --test-threads=1
//! Parallel test threads contend for cores and skew every timing.

use std::path::PathBuf;
use std::time::Instant;

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::model::{BlockKind, Document};
use oryx::layout::{layout_begin, layout_more, ViewConfig, OPEN_SLICE};
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

const WIDTH: f32 = 1200.0;

/// A representative window height, so the open slice can be held to the
/// three viewport heights the first band needs.
const VIEWPORT_H: f32 = 800.0;

/// The layout as the app runs it: one budgeted slice before the first
/// frame, then the rest streaming behind it. The run and rect counts
/// size what the pass materializes, which the paint scan walks.
struct Laid {
    first_ms: u128,
    first_height: f32,
    ms: u128,
    height: f32,
    runs: usize,
    rects: usize,
}

fn measure_layout(doc: &Document) -> Laid {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let theme = Theme::default_dark();
    let cfg = ViewConfig::default();
    let started = Instant::now();
    let (mut out, mut pass) = layout_begin(doc, &cfg, WIDTH);
    let done = layout_more(
        doc,
        &theme,
        &mut fonts,
        &mut media,
        &cfg,
        &mut out,
        &mut pass,
        Some(Instant::now() + OPEN_SLICE),
    );
    let first_ms = started.elapsed().as_millis();
    let first_height = out.height;
    if !done {
        layout_more(
            doc, &theme, &mut fonts, &mut media, &cfg, &mut out, &mut pass, None,
        );
    }
    Laid {
        first_ms,
        first_height,
        ms: started.elapsed().as_millis(),
        height: out.height,
        runs: out.runs.len(),
        rects: out.rects.len(),
    }
}

/// The open slice must leave the first band whole, which is the viewport
/// plus two viewport heights, or place the document outright.
fn assert_first_frame_is_whole(laid: &Laid, what: &str) {
    assert!(
        laid.first_height >= 3.0 * VIEWPORT_H || laid.first_height == laid.height,
        "{what}: open slice placed {:.0}px, short of the first band",
        laid.first_height
    );
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
    let laid = measure_layout(&doc);
    println!(
        "typical: open {open_ms}ms, first slice {}ms, full pass {}ms, height {:.0}px",
        laid.first_ms, laid.ms, laid.height
    );
    assert!(laid.height > 10_000.0, "fixture laid out");
    assert_first_frame_is_whole(&laid, "typical");
    if !cfg!(debug_assertions) {
        assert!(
            open_ms + laid.first_ms < 150,
            "budget exceeded before the first frame: open {open_ms}ms + slice {}ms",
            laid.first_ms
        );
        assert!(
            open_ms + laid.ms < 150,
            "budget exceeded: open {open_ms}ms + layout {}ms",
            laid.ms
        );
    }
}

/// The tier table: budgeted open and full layout pass per tier, with the
/// background highlight cost isolated beside them and the size of what
/// layout materializes. Records numbers without asserting budgets; the
/// first row pays the one-time grammar and font warm-up, as a real cold
/// start does. The huge highlight columns take a minute or two; that
/// work leaving the open path is the point.
#[test]
#[ignore = "measurement only"]
fn tiers_measured() {
    for (name, bytes) in TIERS {
        let (open_ms, doc) = measure_open(&large_gen::generate(*bytes), "md");
        let highlight_ms = measure_highlight(&doc);
        let laid = measure_layout(&doc);
        assert!(laid.height > 0.0, "markdown fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("md {name}"));
        print_row("md", name, open_ms, highlight_ms, &laid);

        let (open_ms, doc) = measure_open(&large_gen::generate_code(*bytes), "rs");
        let highlight_ms = measure_highlight(&doc);
        let laid = measure_layout(&doc);
        assert!(laid.height > 0.0, "code fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("code {name}"));
        print_row("code", name, open_ms, highlight_ms, &laid);
    }
}

fn print_row(kind: &str, tier: &str, open_ms: u128, highlight_ms: u128, laid: &Laid) {
    println!(
        "{kind:<4} {tier:>6}: open {open_ms:>5}ms (highlight {highlight_ms:>5}ms), \
         slice {:>3}ms placing {:>9.0}px, pass {:>5}ms, runs {:>7}, rects {:>7}, \
         height {:>9.0}px",
        laid.first_ms, laid.first_height, laid.ms, laid.runs, laid.rects, laid.height
    );
}
