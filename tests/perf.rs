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
use oryx::doc::stream::{self, Swap};
use oryx::export::paginate::paginate;
use oryx::export::{pdf, ExportSettings, PageGeometry, PageSize};
use oryx::layout::{layout, layout_begin, layout_more, ViewConfig, OPEN_SLICE};
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
/// and highlighted inside the sync budget. A streamed open then pays the
/// worker's full parse and the swap, timed as the parse column; the
/// document handed on is whole either way, so the later columns always
/// measure the full tier.
fn measure_open(source: &str, ext: &str) -> (u128, u128, Document) {
    let path = std::env::temp_dir().join(format!("oryx-perf-{}.{ext}", std::process::id()));
    std::fs::write(&path, source).expect("write fixture");
    let started = Instant::now();
    let opened = load::open(&path, Some(Instant::now() + load::OPEN_BUDGET)).expect("open fixture");
    let open_ms = started.elapsed().as_millis();
    std::fs::remove_file(&path).ok();
    let mut doc = opened.document;
    let mut parse_ms = 0;
    if opened.streamed {
        let started = Instant::now();
        let full = markdown::parse(&doc.source);
        match stream::swap(&doc.blocks, full.blocks) {
            Swap::Splice(tail) => doc.blocks.extend(tail),
            Swap::Replace(blocks) => doc.blocks = blocks,
        }
        parse_ms = started.elapsed().as_millis();
    }
    (open_ms, parse_ms, doc)
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

/// The whole export path a Ctrl+E pays after highlighting settles:
/// layout at the page width, pagination, and emission to bytes.
fn measure_export(doc: &Document) -> (u128, usize, usize) {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let theme = Theme::default_dark();
    let cfg = ViewConfig {
        body_size: 11.0,
        code_size: 9.0,
        zoom: 1.0,
        ..ViewConfig::default()
    };
    let geometry = PageGeometry::new(PageSize::A4, 11.0);
    let settings = ExportSettings {
        body_size: 11.0,
        code_size: 9.0,
        page: PageSize::A4,
        page_numbers: true,
        ..ExportSettings::default()
    };
    let started = Instant::now();
    let laid = layout(doc, &theme, &mut fonts, &mut media, &cfg, geometry.width);
    let pages = paginate(doc, &laid, &geometry);
    let count = pages.len();
    let job = pdf::Job {
        doc,
        layout: &laid,
        theme: &theme,
        geometry: &geometry,
        settings: &settings,
        title: "perf",
    };
    let bytes = pdf::build(&job, &pages, &mut fonts, &mut media).expect("the export builds");
    (started.elapsed().as_millis(), count, bytes.len())
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
    let (open_ms, _, doc) = measure_open(&large_gen::generate(64 * 1024), "md");
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
        let (open_ms, parse_ms, doc) = measure_open(&large_gen::generate(*bytes), "md");
        let highlight_ms = measure_highlight(&doc);
        let laid = measure_layout(&doc);
        assert!(laid.height > 0.0, "markdown fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("md {name}"));
        let export = measure_export(&doc);
        print_row("md", name, open_ms, parse_ms, highlight_ms, &laid, export);

        let (open_ms, parse_ms, doc) = measure_open(&large_gen::generate_code(*bytes), "rs");
        let highlight_ms = measure_highlight(&doc);
        let laid = measure_layout(&doc);
        assert!(laid.height > 0.0, "code fixture laid out");
        assert_first_frame_is_whole(&laid, &format!("code {name}"));
        let export = measure_export(&doc);
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
            if lay.link_at(WIDTH / 2.0, *y).is_some() {
                hits += 1;
            }
        }
        let indexed_us = started.elapsed().as_micros();
        let started = Instant::now();
        for y in &probes {
            let linear = lay.runs.iter().find_map(|r| {
                let target = r.link.as_deref()?;
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
    use oryx::layout::recolor_batch;
    for (name, bytes) in &TIERS[2..] {
        let (_, _, mut doc) = measure_open(&large_gen::generate(*bytes), "md");
        for block in &mut doc.blocks {
            if let BlockKind::CodeBlock {
                language,
                lines,
                highlights,
            } = &mut block.kind
            {
                *highlights = highlight::spans(lines, language.as_deref());
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
