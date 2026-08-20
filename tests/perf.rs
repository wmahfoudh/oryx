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
use oryx::layout::{layout, layout_begin, layout_more, recolor_batch, window_to, ViewConfig};
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
                ..
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
                ..
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

/// The window costs behind a scroll: the cold band fill a scrollbar
/// jump pays before its first frame, the per-frame slide of a normal
/// scroll, and the return to a region already visited once. Runs over
/// the settled windowed layout, the state the app scrolls in.
#[test]
#[ignore = "measurement only"]
fn window_reentry_measured() {
    let pool = pool();
    for (name, bytes) in &TIERS[2..] {
        for (kind, ext) in [("md", "md"), ("code", "rs")] {
            let source = if kind == "md" {
                large_gen::generate(*bytes)
            } else {
                large_gen::generate_code(*bytes)
            };
            let (_, _, mut doc) = measure_open(&source, ext);
            let mut fonts = FontStore::new();
            let mut media = MediaCache::new(PathBuf::from("."));
            let theme = Theme::default_dark();
            let cfg = ViewConfig::default();
            let (mut lay, mut pass) = layout_begin(&doc, &cfg, WIDTH);
            pass.attach_pool(std::sync::Arc::clone(&pool));
            pass.retain_around(0.0, VIEWPORT_H);
            layout_more(
                &doc, &theme, &mut fonts, &mut media, &cfg, &mut lay, &mut pass, None,
            );
            measure_highlight(&mut doc);
            settle_recolor(&doc, &mut lay, &mut fonts);
            let mut jump = |scroll: f32, lay: &mut oryx::layout::LayoutDoc| {
                let started = Instant::now();
                window_to(
                    &doc,
                    &theme,
                    &mut fonts,
                    &mut media,
                    &cfg,
                    lay,
                    Some(&pool),
                    scroll,
                    VIEWPORT_H,
                    true,
                );
                started.elapsed().as_millis()
            };
            let mid = lay.height / 2.0;
            let cold_ms = jump(mid, &mut lay);
            let slide_ms = jump(mid + VIEWPORT_H, &mut lay);
            let back_ms = jump(0.0, &mut lay);
            println!(
                "reentry {kind:<4} {name:>6}: cold {cold_ms:>4}ms, slide {slide_ms:>4}ms, \
                 back {back_ms:>4}ms, runs {:>6}",
                lay.runs.len()
            );
        }
    }
}

/// Writes the 8MB fixtures to `tests/fixtures/huge.md` and
/// `tests/fixtures/huge.rs` for field testing in the app. Run on
/// demand:
///   cargo test --test perf dump_huge_fixture -- --ignored
#[test]
#[ignore = "writes the fixture on demand"]
fn dump_huge_fixture() {
    let source = large_gen::generate(8 * 1024 * 1024);
    std::fs::write("tests/fixtures/huge.md", source).expect("write huge.md");
    let source = large_gen::generate_code(8 * 1024 * 1024);
    std::fs::write("tests/fixtures/huge.rs", source).expect("write huge.rs");
}

/// Design probe for the segmented wash: the hit-rate of the cold-state
/// guess at segment boundaries and the share of lines a cold start
/// colors differently, from one truth pass and one cold pass per
/// corpus. Boundaries are probed every 256 lines; the 512 and 1024
/// hit columns read the same pass at a stride. The pass time covers
/// both parses, so the single-thread wash is about half of it.
/// Corpora: the 8MB code fixture and the repo's own sources
/// concatenated; set ORYX_WASH_PROBE=path to add a real file,
/// language from its extension.
#[test]
#[ignore = "measurement only"]
fn wash_guess_measured() {
    use oryx::doc::model::CodeBody;

    fn rust_sources(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    let mut corpora: Vec<(String, String, String)> = Vec::new();
    corpora.push((
        "code 8MB".into(),
        large_gen::generate_code(8 * 1024 * 1024),
        "rs".into(),
    ));
    let mut files = Vec::new();
    rust_sources(std::path::Path::new("src"), &mut files);
    let repo: String = files
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("read repo source"))
        .collect::<Vec<_>>()
        .join("\n");
    corpora.push(("repo src".into(), repo, "rs".into()));
    if let Ok(path) = std::env::var("ORYX_WASH_PROBE") {
        let path = PathBuf::from(path);
        let language = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_string();
        let text = std::fs::read_to_string(&path).expect("read ORYX_WASH_PROBE");
        corpora.push((path.display().to_string(), text, language));
    }

    for (name, source, language) in &corpora {
        let body = CodeBody::from_text(source);
        let started = Instant::now();
        let probes = highlight::segment_probe("", &body, Some(language), 256);
        let pass_ms = started.elapsed().as_millis();
        print!(
            "wash guess: {name} ({language}): {} lines, probe pass {pass_ms}ms",
            body.len()
        );
        for (segment, stride) in [(256usize, 1usize), (512, 2), (1024, 4)] {
            let sampled: Vec<bool> = probes
                .iter()
                .map(|p| p.state_hit)
                .skip(stride - 1)
                .step_by(stride)
                .collect();
            if sampled.is_empty() {
                continue;
            }
            let hit = sampled.iter().filter(|h| **h).count();
            let mut longest_miss = 0usize;
            let mut run = 0usize;
            for h in &sampled {
                run = if *h { 0 } else { run + 1 };
                longest_miss = longest_miss.max(run);
            }
            print!(
                " | @{segment}: {hit}/{} ({:.1}%), miss run {longest_miss}",
                sampled.len(),
                100.0 * hit as f64 / sampled.len() as f64
            );
        }
        let lines: usize = probes.iter().map(|p| p.lines).sum();
        let drifted: usize = probes.iter().map(|p| p.drifted_lines).sum();
        let worst = probes
            .iter()
            .map(|p| p.drifted_lines as f64 / p.lines.max(1) as f64)
            .fold(0.0f64, f64::max);
        println!(
            " | cold drift {drifted}/{lines} lines ({:.2}%), worst segment {:.1}%",
            100.0 * drifted as f64 / lines.max(1) as f64,
            100.0 * worst
        );
    }
}

/// The rest timer's re-color after an edit, before and after seams.
/// Before: the whole-block sweep from line 0, the churn every typing
/// burst used to pay. After: the sweep resumed from the nearest stored
/// seam, converging against the shifted table. Edits at the top, the
/// middle, and the bottom of the 8MB code tier.
#[test]
#[ignore = "measurement only"]
fn wash_edit_measured() {
    use oryx::doc::model::BlockKind;
    use oryx::edit::{self, splice::Ledger};
    use oryx::style::highlight::{self, Seam, CHUNK_LINES};
    let source = large_gen::generate_code(8 * 1024 * 1024);
    let (_, _, mut doc) = measure_open(&source, "rs");
    let (language, lines) = match &doc.blocks[0].kind {
        BlockKind::CodeBlock {
            language, lines, ..
        } => (language.clone(), lines.clone()),
        _ => panic!("a code file is one code block"),
    };
    let mut table: Vec<(usize, Seam)> = Vec::new();
    let started = Instant::now();
    highlight::spans_chunked(
        &doc.source,
        &lines,
        language.as_deref(),
        CHUNK_LINES,
        None,
        |c| {
            table.push((c.start_line + c.spans.len(), c.seam));
            true
        },
    );
    let full_ms = started.elapsed().as_millis();
    println!(
        "wash edit 8MB: full sweep {full_ms}ms over {} lines, {} seams",
        lines.len(),
        table.len()
    );
    let len = doc.source.len();
    for (name, pos) in [("top", 100), ("middle", len / 2), ("bottom", len - 2)] {
        let mut table = table.clone();
        let mut ledger = Ledger::new(std::sync::Arc::clone(&doc.source), Vec::new());
        let touched = ledger.edit(pos..pos, "x");
        let current = ledger.current();
        let started = Instant::now();
        let (old_eff, new_eff) = edit::splice_document(&mut doc, &current, pos..pos, touched)
            .expect("the fixture edit stays on the fast path");
        highlight::shift_seams(&mut table, old_eff, new_eff.clone());
        // The splice reshaped the block's lines; the sweep reads them
        // as the app would, never the pre-edit clone.
        let lines = match &doc.blocks[0].kind {
            BlockKind::CodeBlock { lines, .. } => lines.clone(),
            _ => unreachable!(),
        };
        let at = table.partition_point(|(line, _)| *line <= new_eff.start);
        let (start_line, seam) = match at.checked_sub(1).and_then(|i| table.get(i)) {
            Some((line, seam)) => (*line, Some(seam.clone())),
            None => (0, None),
        };
        let resume = highlight::Resume {
            start_line,
            seam,
            expected: table[at..].to_vec(),
        };
        let mut delivered = 0usize;
        let mut converged = false;
        let complete = highlight::spans_chunked(
            &doc.source,
            &lines,
            language.as_deref(),
            CHUNK_LINES,
            Some(&resume),
            |c| {
                delivered += c.spans.len();
                converged |= c.converged;
                true
            },
        );
        println!(
            "wash edit 8MB {name}: {}ms, {delivered} lines re-colored, converged {converged}",
            started.elapsed().as_millis()
        );
        assert!(complete, "{name}: the resumed sweep completes");
        // The last chunk has no downstream entry to converge on; every
        // other edit stops without reaching the block's end.
        assert!(
            converged || delivered <= CHUNK_LINES,
            "{name}: the sweep stayed local"
        );
    }
}

/// The speculative band: what a view resting past the exact sweep
/// waits for, at the 8MB code tier. The band is the viewport padded a
/// screen each way, taken at the end of the file, the worst place the
/// old linear wash reached last. The drift column is what the exact
/// sweep later corrects on screen.
#[test]
#[ignore = "measurement only"]
fn wash_band_measured() {
    use oryx::doc::model::BlockKind;
    use oryx::layout::metrics;
    use oryx::style::highlight;
    let source = large_gen::generate_code(8 * 1024 * 1024);
    let (_, _, doc) = measure_open(&source, "rs");
    let (language, lines) = match &doc.blocks[0].kind {
        BlockKind::CodeBlock {
            language, lines, ..
        } => (language.clone(), lines.clone()),
        _ => panic!("a code file is one code block"),
    };
    // Three screens of code lines: the viewport padded a screen each
    // way, at the reference body size.
    let line_h = metrics::REFERENCE_BODY * metrics::LINE_HEIGHT;
    let band = (3.0 * VIEWPORT_H / line_h).ceil() as usize;
    let start = lines.len() - band;
    let truth = highlight::spans(&doc.source, &lines, language.as_deref());
    let started = Instant::now();
    let guessed =
        highlight::spans_band(&doc.source, &lines, language.as_deref(), start..lines.len());
    let ms = started.elapsed().as_millis();
    let drifted = guessed
        .iter()
        .zip(&truth[start..])
        .filter(|(a, b)| a != b)
        .count();
    println!(
        "wash band 8MB: {band} lines at the file's end in {ms}ms, {drifted} lines corrected later ({:.2}%)",
        100.0 * drifted as f64 / band as f64
    );
}

/// One keystroke's display path in edit mode, both pipes. The fast
/// path is the app's: the ledger splice, the current-text rebuild, the
/// in-place model and layout splices, then the window refill the next
/// frame performs. The fallback rows measure the correctness-first
/// pipe the app keeps for a still-measuring pass: the whole-document
/// reparse with the highlight carry and the restarted streaming pass.
/// The keystroke lands at the end of the file, the worst case for
/// every scan; the code tier folds its highlights first so the carry
/// copies real spans.
#[test]
#[ignore = "measurement only"]
fn keystroke_measured() {
    use oryx::doc::load;
    use oryx::edit::{self, splice::Ledger};
    use oryx::layout::edit_code_lines;
    use oryx::ui::outline::OutlineTree;
    use std::path::Path;
    let pool = pool();
    let theme = Theme::default_dark();
    let cfg = ViewConfig::default();
    for (name, bytes) in &TIERS[2..] {
        for (kind, ext) in [("code", "rs"), ("text", "txt")] {
            let source = large_gen::generate_code(*bytes);
            let (_, _, mut doc) = measure_open(&source, ext);
            if *kind == *"code" {
                measure_highlight(&mut doc);
            }
            let mut fonts = FontStore::new();
            let mut media = MediaCache::new(PathBuf::from("."));
            let (mut lay, mut pass) = layout_begin(&doc, &cfg, WIDTH);
            pass.attach_pool(std::sync::Arc::clone(&pool));
            pass.retain_around(0.0, VIEWPORT_H);
            layout_more(
                &doc, &theme, &mut fonts, &mut media, &cfg, &mut lay, &mut pass, None,
            );
            let mut ledger = Ledger::new(std::sync::Arc::clone(&doc.source), Vec::new());
            let at = doc.source.len() - 1;
            let started = Instant::now();
            let touched = ledger.edit(at..at, "x");
            let splice_us = started.elapsed().as_micros();
            let started = Instant::now();
            let current = ledger.current();
            let current_us = started.elapsed().as_micros();
            let started = Instant::now();
            let (old_lines, new_lines) =
                edit::splice_document(&mut doc, &current, at..at, touched.clone())
                    .expect("a file document splices");
            let model_us = started.elapsed().as_micros();
            let started = Instant::now();
            assert!(edit_code_lines(
                &mut lay, &doc, &theme, &mut fonts, &cfg, 0, old_lines, new_lines,
            ));
            let layout_us = started.elapsed().as_micros();
            let started = Instant::now();
            window_to(
                &doc,
                &theme,
                &mut fonts,
                &mut media,
                &cfg,
                &mut lay,
                Some(&pool),
                0.0,
                VIEWPORT_H,
                true,
            );
            lay.index_more();
            let refill_us = started.elapsed().as_micros();
            let started = Instant::now();
            let _outline = OutlineTree::build(&doc);
            let outline_us = started.elapsed().as_micros();
            println!(
                "{kind:<4} {name:>6} fast: splice {:>5.1}ms, current {:>5.1}ms, \
                 model {:>5.2}ms, layout {:>5.2}ms, refill {:>5.1}ms, outline {:>5.2}ms",
                splice_us as f64 / 1000.0,
                current_us as f64 / 1000.0,
                model_us as f64 / 1000.0,
                layout_us as f64 / 1000.0,
                refill_us as f64 / 1000.0,
                outline_us as f64 / 1000.0,
            );

            // The fallback pipe over the same edited state.
            let file_kind = load::detect(Path::new(&format!("fixture.{ext}")));
            let at = doc.source.len() - 1;
            let touched = ledger.edit(at..at, "y");
            let current = ledger.current();
            let started = Instant::now();
            doc = edit::reparse(file_kind, &current, &doc, touched.clone(), touched);
            let reparse_ms = started.elapsed().as_millis();
            let (laid, _resident) = measure_layout(&doc, Some(&pool));
            println!(
                "{kind:<4} {name:>6} slow: reparse {reparse_ms:>4}ms, \
                 first slice {:>4}ms, full pass {:>5}ms",
                laid.first_ms, laid.ms
            );
        }
    }
}

/// The editing crossing's three costs at the markdown tiers: entering
/// edit mode (the source view built from the file's bytes, then its
/// layout), returning with no edit (the parked page and layout move
/// back whole, the outline held), and returning after a one-byte edit
/// (the buffer reparsed as a page, its outline rebuilt, then a fresh
/// layout). The clean return is what the parked render exists for and
/// must not scale with the file; its first cut rebuilt the outline and
/// paid 273ms at the 8MB tier, which moved the rebuild to the changed
/// path. The edited return's parse and outline columns are also the
/// save cost, a save rebuilding the parked page the same way.
#[test]
#[ignore = "measurement only"]
fn crossing_measured() {
    use oryx::doc::load;
    use oryx::edit::{self, splice::Ledger};
    use oryx::ui::outline::OutlineTree;
    use std::path::Path;
    let pool = pool();
    let kind = load::detect(Path::new("fixture.md"));
    for (name, bytes) in TIERS.iter().filter(|(n, _)| *n != "medium") {
        let source = large_gen::generate(*bytes);
        let (_, _, doc) = measure_open(&source, "md");
        // The reading state the crossing leaves: the page fully placed.
        let (_, page_lay) = measure_layout(&doc, Some(&pool));

        let text = std::sync::Arc::clone(&doc.source);
        let started = Instant::now();
        let source_doc = edit::source_document(kind, &text).expect("markdown swaps");
        let build_us = started.elapsed().as_micros();
        let (laid, _resident) = measure_layout(&source_doc, Some(&pool));
        assert!(laid.height > 0.0, "source view laid out");
        println!(
            "crossing {name:>6} enter: source build {:>5.1}ms, \
             first slice {:>4}ms, full pass {:>5}ms",
            build_us as f64 / 1000.0,
            laid.first_ms,
            laid.ms
        );

        let started = Instant::now();
        let restored = doc;
        let _restored_lay = page_lay;
        let return_us = started.elapsed().as_micros();
        drop(restored);
        println!(
            "crossing {name:>6} clean return: {:>5.2}ms",
            return_us as f64 / 1000.0
        );

        let mut ledger = Ledger::new(std::sync::Arc::clone(&source_doc.source), Vec::new());
        let at = source_doc.source.len() - 1;
        ledger.edit(at..at, "x");
        let current = ledger.current();
        let started = Instant::now();
        let page = edit::rendered_document(kind, &current);
        let parse_ms = started.elapsed().as_millis();
        let started = Instant::now();
        let _outline = OutlineTree::build(&page);
        let outline_us = started.elapsed().as_micros();
        let (laid, _resident) = measure_layout(&page, Some(&pool));
        assert!(laid.height > 0.0, "edited page laid out");
        println!(
            "crossing {name:>6} edited return: parse {parse_ms:>4}ms, \
             outline {:>5.2}ms, first slice {:>4}ms, full pass {:>5}ms",
            outline_us as f64 / 1000.0,
            laid.first_ms,
            laid.ms
        );
    }
}

/// Resident memory from the kernel's view, in kilobytes.
fn vm_rss_kb() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

/// The container-parity bench: one real book, opened as the app opens
/// it, timed to the first frame and through the whole walk. The bar it
/// serves: a book must not open slower because it arrived in a
/// different container. Run once per container of the same title, each
/// in its own process so memory reads clean:
/// ORYX_BOOK=<path> cargo test --release --test perf book_bench -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn book_bench() {
    use oryx::doc::load;
    use oryx::doc::model::Document;
    let path = std::env::var("ORYX_BOOK").expect("set ORYX_BOOK");
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let t = Instant::now();
    let opened = load::open(
        std::path::Path::new(&path),
        Some(Instant::now() + load::OPEN_BUDGET),
    )
    .expect("the book opens");
    let open_ms = t.elapsed().as_millis();
    let (prefix_laid, _) = measure_layout(&opened.document, Some(&pool()));

    let t = Instant::now();
    let mut walk_ms = 0;
    let mut images = 0usize;
    let full_doc = match opened.book {
        Some(job) => {
            let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let seen = std::sync::Arc::clone(&counter);
            let sink: oryx::doc::images::SourceSink = std::sync::Arc::new(move |sources| {
                seen.fetch_add(sources.len(), std::sync::atomic::Ordering::Relaxed);
            });
            let delivered = job.run(&|| false, sink);
            walk_ms = t.elapsed().as_millis();
            images = counter.load(std::sync::atomic::Ordering::Relaxed);
            match delivered {
                Some(delivered) => Document {
                    blocks: delivered.blocks,
                    source: delivered
                        .source
                        .unwrap_or_else(|| std::sync::Arc::clone(&opened.document.source)),
                    details: delivered.details,
                    title: opened.document.title.clone(),
                    anchors: delivered.anchors.into_iter().collect(),
                    book_id: opened.document.book_id.clone(),
                    code_file: false,
                    plain_file: false,
                    comic_file: false,
                },
                None => opened.document,
            }
        }
        None => opened.document,
    };
    let (full_laid, _) = measure_layout(&full_doc, Some(&pool()));
    println!(
        "{path}\n  {size} bytes, open {open_ms}ms, first slice {}ms, prefix pass {}ms\n  \
         walk {walk_ms}ms, {} blocks, {} source chars, {images} image sources\n  \
         full pass {}ms, height {:.0}px, rss {}MB",
        prefix_laid.first_ms,
        prefix_laid.ms,
        full_doc.blocks.len(),
        full_doc.source.len(),
        full_laid.ms,
        full_laid.height,
        vm_rss_kb() / 1024
    );
}

/// Long lines: a 250KB minified JSON on one line, and a paragraph of
/// 4,000 inline spans. Before the chunked shaping the JSON took 106.8s
/// to lay out, quadratic in the highlight segment count inside the
/// span list, and the paragraph 6.5s.
#[test]
#[ignore = "timing asserts only hold in release mode"]
fn long_lines_meet_the_budget() {
    let mut json = String::from("{");
    let mut i = 0;
    while json.len() < 250 * 1024 {
        json.push_str(&format!(
            "\"key{i}\":{{\"id\":{i},\"ok\":true,\"tags\":[\"a\",\"b\"]}},"
        ));
        i += 1;
    }
    json.push_str("\"end\":0}");
    let (open_ms, _, doc) = measure_open(&json, "json");
    let (laid, _) = measure_layout(&doc, Some(&pool()));
    println!(
        "json line: open {open_ms}ms, first slice {}ms, full pass {}ms, {} runs",
        laid.first_ms, laid.ms, laid.runs
    );
    assert!(laid.height > 0.0, "the json line laid out");
    if !cfg!(debug_assertions) {
        assert!(
            open_ms + laid.ms < 2000,
            "budget exceeded: open {open_ms}ms + layout {}ms",
            laid.ms
        );
    }

    let paragraph: String = (0..2000).map(|i| format!("w{i} *e{i}* ")).collect();
    let (open_ms, _, doc) = measure_open(&paragraph, "md");
    let (laid, _) = measure_layout(&doc, Some(&pool()));
    println!(
        "span paragraph: open {open_ms}ms, full pass {}ms, {} runs",
        laid.ms, laid.runs
    );
    if !cfg!(debug_assertions) {
        assert!(laid.ms < 1000, "budget exceeded: layout {}ms", laid.ms);
    }
}
