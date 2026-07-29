//! Performance validation against the budgets. Ignored by default; run
//! release mode locally with:
//!   cargo test --release --test perf -- --ignored --nocapture --test-threads=1
//! Parallel test threads contend for cores and skew every timing.
//! The memory tiers live in tests/perf_mem.rs, a separate binary, so
//! its counting allocator never taxes a timing measured here.

use std::path::PathBuf;
use std::time::Instant;

use oryx::doc::images::MediaCache;
use oryx::doc::model::BlockKind;
use oryx::layout::{layout, layout_begin, layout_more, recolor_batch, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::highlight;
use oryx::style::theme::Theme;

#[path = "fixtures/large_gen.rs"]
mod large_gen;

#[path = "fixtures/perf_common.rs"]
mod perf_common;

use perf_common::{
    assert_first_frame_is_whole, measure_export, measure_highlight, measure_layout, measure_open,
    pool, settle_recolor, Laid, TIERS, VIEWPORT_H, WIDTH,
};

/// The product promise: typical documents open instantly. A 64KB mixed
/// document with dense code blocks is already a long, heavy README.
/// Run alone for a true cold number; in a full suite run another test
/// may already have paid the grammar and font warm-up.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn typical_document_meets_the_budget() {
    let (open_ms, _, doc) = measure_open(&large_gen::generate(64 * 1024), "md");
    let (laid, _resident) = measure_layout(&doc, Some(&pool()));
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
///
/// The timing columns measure the open-path layout of the uncolored
/// document, exactly the app's open, so they compare across entries.
/// The document then folds its highlights and the layout recolors in
/// one batch, the state the app settles into after the wash-in, so the
/// run and rect counts report the settled layout and the export covers
/// the colored document, the state the app's export waits for.
#[test]
#[ignore = "measurement only"]
fn tiers_measured() {
    let pool = pool();
    for (name, bytes) in TIERS {
        let source = large_gen::generate(*bytes);
        let mut fonts = FontStore::new();
        let (open_ms, parse_ms, mut doc) = measure_open(&source, "md");
        let (mut laid, mut resident) = measure_layout(&doc, Some(&pool));
        assert!(laid.height > 0.0, "markdown fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("md {name}"));
        let highlight_ms = measure_highlight(&mut doc);
        settle_recolor(&doc, &mut resident, &mut fonts);
        laid.runs = resident.runs.len();
        laid.rects = resident.rects.len();
        drop(resident);
        let export = measure_export(&doc, Some(&pool));
        print_row("md", name, open_ms, parse_ms, highlight_ms, &laid, export);

        let source = large_gen::generate_code(*bytes);
        let mut fonts = FontStore::new();
        let (open_ms, parse_ms, mut doc) = measure_open(&source, "rs");
        let (mut laid, mut resident) = measure_layout(&doc, Some(&pool));
        assert!(laid.height > 0.0, "code fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("code {name}"));
        let highlight_ms = measure_highlight(&mut doc);
        settle_recolor(&doc, &mut resident, &mut fonts);
        laid.runs = resident.runs.len();
        laid.rects = resident.rects.len();
        drop(resident);
        let export = measure_export(&doc, Some(&pool));
        print_row("code", name, open_ms, parse_ms, highlight_ms, &laid, export);
    }
}

/// The interactive paths the y index converted from scans to searches:
/// link hit-testing (every mouse move) and the direct band paint (every
/// frame of a drag or an open search). The linear hover column re-runs
/// the pre-index scan for comparison; the band's own before figure is
/// the 4.7ms full-scan share recorded in the backlog.
#[test]
#[ignore = "measurement only"]
fn interactive_paths_measured() {
    use oryx::layout::metrics;
    for (name, bytes) in &TIERS[2..] {
        let (_, _, doc) = measure_open(&large_gen::generate(*bytes), "md");
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        let theme = Theme::default_dark();
        let cfg = ViewConfig::default();
        let (mut lay, mut pass) = layout_begin(&doc, &cfg, WIDTH);
        layout_more(
            &doc, &theme, &mut fonts, &mut media, &cfg, &mut lay, &mut pass, None,
        );
        lay.index_more();
        // A fixed sample keeps the linear column bounded; per-probe cost
        // is what matters and 2000 probes settle it.
        let count = 2000usize;
        let step = lay.height / count as f32;
        let probes: Vec<f32> = (0..count).map(|i| i as f32 * step).collect();
        let mut hits = 0usize;
        let started = Instant::now();
        for y in &probes {
            if lay.link_at(&doc, WIDTH / 2.0, *y).is_some() {
                hits += 1;
            }
        }
        let indexed_us = started.elapsed().as_micros();
        let started = Instant::now();
        for y in &probes {
            let linear = lay.runs.iter().find_map(|r| {
                let target = lay.run_link(&doc, r)?;
                let inside = WIDTH / 2.0 >= r.x
                    && WIDTH / 2.0 <= r.x + r.width
                    && *y >= r.y
                    && *y <= r.y + metrics::LINE_HEIGHT * r.size;
                inside.then_some(target)
            });
            if linear.is_some() {
                hits += 1;
            }
        }
        let linear_us = started.elapsed().as_micros();
        let bands = 5u32;
        let started = Instant::now();
        for i in 0..bands {
            let y = lay.height / (bands + 1) as f32 * (i + 1) as f32;
            let _ = oryx::paint::band(
                &lay,
                &doc,
                &theme,
                &mut fonts,
                &mut media,
                &[],
                y,
                WIDTH as u32,
                VIEWPORT_H as u32,
            );
        }
        let band_ms = started.elapsed().as_millis() / u128::from(bands);
        println!(
            "hover {name}: {} probes, indexed {indexed_us}us, linear {linear_us}us, \
             {hits} hits; direct band {band_ms}ms per frame",
            probes.len()
        );
    }
}

fn print_row(
    kind: &str,
    tier: &str,
    open_ms: u128,
    parse_ms: u128,
    highlight_ms: u128,
    laid: &Laid,
    export: (u128, usize, usize),
) {
    let (export_ms, pages, pdf_bytes) = export;
    println!(
        "{kind:<4} {tier:>6}: open {open_ms:>5}ms (parse {parse_ms:>4}ms, \
         highlight {highlight_ms:>5}ms), \
         slice {:>3}ms placing {:>9.0}px, pass {:>5}ms, runs {:>7}, rects {:>7}, \
         height {:>9.0}px, pdf {export_ms:>6}ms for {pages:>5} pages ({:.1}MB)",
        laid.first_ms,
        laid.first_height,
        laid.ms,
        laid.runs,
        laid.rects,
        laid.height,
        pdf_bytes as f32 / (1024.0 * 1024.0)
    );
}

/// The field scenario behind the batched fold: a full arrival backlog
/// recolored against a completely placed layout. The pre-batch cost was
/// one tail-shifting splice per arrival, 43 seconds in the field on the
/// huge tier; the batch pays one rebuild.
#[test]
#[ignore = "measurement only"]
fn fold_backlog_measured() {
    for (name, bytes) in &TIERS[2..] {
        let (_, _, mut doc) = measure_open(&large_gen::generate(*bytes), "md");
        let source = std::sync::Arc::clone(&doc.source);
        for block in &mut doc.blocks {
            if let BlockKind::CodeBlock {
                language,
                lines,
                highlights,
            } = &mut block.kind
            {
                *highlights = highlight::spans(&source, lines, language.as_deref());
            }
        }
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        let theme = Theme::default_dark();
        let cfg = ViewConfig::default();
        let mut lay = layout(&doc, &theme, &mut fonts, &mut media, &cfg, WIDTH);
        let patches: Vec<(usize, std::ops::Range<usize>)> = doc
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match &b.kind {
                BlockKind::CodeBlock { lines, .. } => Some((i, 0..lines.len())),
                _ => None,
            })
            .collect();
        let started = Instant::now();
        recolor_batch(&mut lay, &doc, &theme, &mut fonts, &cfg, &patches);
        println!(
            "fold {name}: {} blocks over {} runs in {}ms",
            patches.len(),
            lay.runs.len(),
            started.elapsed().as_millis()
        );
    }
}

/// The field scenario refined: arrivals trickling in over many drains
/// against a fully placed layout, each drain a recolor_batch call. The
/// full-vector rebuild paid per drain is what froze deselection for 11
/// seconds in the field; the ranged rebuild pays for the touched span.
#[test]
#[ignore = "measurement only"]
fn fold_trickle_measured() {
    for (name, bytes) in &TIERS[2..] {
        let (_, _, mut doc) = measure_open(&large_gen::generate(*bytes), "md");
        let source = std::sync::Arc::clone(&doc.source);
        for block in &mut doc.blocks {
            if let BlockKind::CodeBlock {
                language,
                lines,
                highlights,
            } = &mut block.kind
            {
                *highlights = highlight::spans(&source, lines, language.as_deref());
            }
        }
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        let theme = Theme::default_dark();
        let cfg = ViewConfig::default();
        let patches: Vec<(usize, std::ops::Range<usize>)> = doc
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match &b.kind {
                BlockKind::CodeBlock { lines, .. } => Some((i, 0..lines.len())),
                _ => None,
            })
            .collect();
        for chunk in [200usize, 2000] {
            let mut lay = layout(&doc, &theme, &mut fonts, &mut media, &cfg, WIDTH);
            let drains: Vec<&[(usize, std::ops::Range<usize>)]> = patches.chunks(chunk).collect();
            let count = drains.len();
            let started = Instant::now();
            for drain in drains {
                recolor_batch(&mut lay, &doc, &theme, &mut fonts, &cfg, drain);
            }
            println!(
                "trickle {name}: {} drains of {} blocks over {} runs in {}ms",
                count,
                chunk,
                lay.runs.len(),
                started.elapsed().as_millis()
            );
        }
    }
}
