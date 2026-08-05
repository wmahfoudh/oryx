//! Measurement plumbing shared by the timing tiers (`tests/perf.rs`)
//! and the memory tiers (`tests/perf_mem.rs`). The two are separate
//! binaries so the memory binary's counting allocator never taxes a
//! timing; sharing the walk here keeps both measuring the same journey.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::markdown;
use oryx::doc::model::{BlockKind, Document};
use oryx::doc::stream::{self, Swap};
use oryx::export::{ExportPass, ExportSettings, PageSize};
use oryx::layout::{
    layout_begin, layout_more, recolor_batch, LayoutDoc, ShapePool, ViewConfig, OPEN_SLICE,
};
use oryx::style::fonts::FontStore;
use oryx::style::highlight;
use oryx::style::theme::Theme;

/// Byte sizes for the measurement tiers, shared by the markdown and the
/// whole-file code fixtures so their rows compare directly.
pub const TIERS: &[(&str, usize)] = &[
    ("small", 64 * 1024),
    ("medium", 256 * 1024),
    ("large", 1024 * 1024),
    ("huge", 8 * 1024 * 1024),
];

pub const WIDTH: f32 = 1200.0;

/// A representative window height, so the open slice can be held to the
/// three viewport heights the first band needs.
pub const VIEWPORT_H: f32 = 800.0;

/// The app's open path: the fixture written to disk, then read, parsed,
/// and highlighted inside the sync budget. A streamed open then pays the
/// worker's full parse and the swap, timed as the parse column; the
/// document handed on is whole either way, so the later columns always
/// measure the full tier.
pub fn measure_open(source: &str, ext: &str) -> (u128, u128, Document) {
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
        let full = markdown::parse(Arc::clone(&doc.source));
        match stream::swap(&doc.blocks, full.blocks) {
            Swap::Splice(tail) => doc.blocks.extend(tail),
            Swap::Replace(blocks) => doc.blocks = blocks,
        }
        parse_ms = started.elapsed().as_millis();
    }
    (open_ms, parse_ms, doc)
}

/// The layout as the app runs it: one budgeted slice before the first
/// frame, then the rest streaming behind it. The run and rect counts
/// size what the pass materializes, which the paint scan walks.
pub struct Laid {
    pub first_ms: u128,
    pub first_height: f32,
    pub ms: u128,
    pub height: f32,
    pub runs: usize,
    pub rects: usize,
}

pub fn pool() -> std::sync::Arc<ShapePool> {
    let width = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .clamp(1, 8);
    std::sync::Arc::new(ShapePool::new(width, &FontStore::new().seed()))
}

pub fn measure_layout(
    doc: &Document,
    pool: Option<&std::sync::Arc<ShapePool>>,
) -> (Laid, LayoutDoc) {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let theme = Theme::default_dark();
    let cfg = ViewConfig::default();
    let started = Instant::now();
    let (mut out, mut pass) = layout_begin(doc, &cfg, WIDTH);
    if let Some(pool) = pool {
        pass.attach_pool(std::sync::Arc::clone(pool));
    }
    // The app's retention: the pass measures everything but keeps only
    // the window around the reading position, here the top.
    pass.retain_around(0.0, VIEWPORT_H);
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
    let laid = Laid {
        first_ms,
        first_height,
        ms: started.elapsed().as_millis(),
        height: out.height,
        runs: out.runs.len(),
        rects: out.rects.len(),
    };
    (laid, out)
}

/// The open slice must leave the first band whole, which is the viewport
/// plus two viewport heights, or place the document outright.
pub fn assert_first_frame_is_whole(laid: &Laid, what: &str) {
    assert!(
        laid.first_height >= 3.0 * VIEWPORT_H || laid.first_height == laid.height,
        "{what}: open slice placed {:.0}px, short of the first band",
        laid.first_height
    );
}

/// The whole export path a Ctrl+P pays after highlighting settles: the
/// streamed pass the app drives, fused layout, pagination and pooled
/// emission flushing to a scratch file.
pub fn measure_export(
    doc: &Document,
    pool: Option<&std::sync::Arc<ShapePool>>,
) -> (u128, usize, usize) {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    let settings = ExportSettings {
        body_size: 11.0,
        code_size: 9.0,
        page: PageSize::A4,
        page_numbers: true,
        ..ExportSettings::default()
    };
    let target = std::env::temp_dir().join(format!("oryx-perf-export-{}.pdf", std::process::id()));
    let started = Instant::now();
    let mut pass = ExportPass::new(&settings, Theme::default_dark(), target.clone());
    while !pass.is_done() {
        pass.step(
            Instant::now() + Duration::from_millis(50),
            doc,
            &mut fonts,
            &mut media,
            false,
            pool,
        );
    }
    let count = pass.finish(doc, &fonts).expect("the export lands");
    let elapsed = started.elapsed().as_millis();
    let bytes = std::fs::metadata(&target)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    std::fs::remove_file(&target).ok();
    (elapsed, count, bytes)
}

/// The syntect cost that lazy highlighting moves off the open path:
/// every code block highlighted in full on warm grammars. The spans fold
/// into the document for the recolor that settles the layout and for the
/// export, which waits for colors in the app. Only the computation is
/// timed.
pub fn measure_highlight(doc: &mut Document) -> u128 {
    // Accumulated as a Duration: many small markdown blocks sit under a
    // millisecond each, and per-block as_millis() truncates them all to
    // zero.
    let mut total = std::time::Duration::ZERO;
    let source = std::sync::Arc::clone(&doc.source);
    for block in &mut doc.blocks {
        if let BlockKind::CodeBlock {
            language,
            lines,
            highlights,
        } = &mut block.kind
        {
            let started = Instant::now();
            let spans = highlight::spans(&source, lines, language.as_deref());
            total += started.elapsed();
            *highlights = spans;
        }
    }
    total.as_millis()
}

/// The wash-in's end state: every folded highlight recolored into the
/// placed layout in one batch, the road the app takes in waves. Untimed;
/// the fold measurements own that cost.
pub fn settle_recolor(doc: &Document, lay: &mut LayoutDoc, fonts: &mut FontStore) {
    let patches: Vec<(usize, std::ops::Range<usize>)> = doc
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| match &b.kind {
            BlockKind::CodeBlock { lines, .. } => Some((i, 0..lines.len())),
            _ => None,
        })
        .collect();
    recolor_batch(
        lay,
        doc,
        &Theme::default_dark(),
        fonts,
        &ViewConfig::default(),
        &patches,
    );
}
