//! Scroll clamping and the band cache. Scrolling inside the band is a
//! memcpy slice; the band repaints recentered only near its edges.

use std::time::Duration;

use crate::doc::images::MediaCache;
use crate::doc::model::{BlockKind, Document};
use crate::layout::{DecoRect, LayoutDoc};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

pub fn clamp(y: f32, doc_height: f32, viewport_h: f32) -> f32 {
    y.clamp(0.0, (doc_height - viewport_h).max(0.0))
}

/// The source offset of what stands at the top of the view: the start
/// of the block there, and inside a code block whose lines are the
/// source's own, the start of the line there. A code or text file is
/// one such block, so its place is a line, not the top of the file.
pub fn top_offset(lay: &LayoutDoc, doc: &Document, scroll_y: f32) -> usize {
    let mut at = None;
    for index in 0..doc.blocks.len() {
        match lay.approx_top(index, 0) {
            Some(top) if top <= scroll_y + 1.0 => at = Some(index),
            _ => break,
        }
    }
    let Some(index) = at else {
        return 0;
    };
    let block = &doc.blocks[index];
    if let BlockKind::CodeBlock { lines, .. } = &block.kind {
        let line = lay
            .code_line_at(index, lines.len(), scroll_y + 1.0)
            .and_then(|line| lines.line_range(line));
        if let Some(range) = line {
            return range.start;
        }
    }
    block.range.start
}

/// Where a source offset stands on the page, the inverse of
/// `top_offset`: the top of its block, or of its line inside a code
/// block whose lines are the source's own. None before the pass places
/// the block.
pub fn offset_top(lay: &LayoutDoc, doc: &Document, offset: usize) -> Option<f32> {
    let block = doc.block_at_offset(offset)?;
    let line = match &doc.blocks[block].kind {
        BlockKind::CodeBlock { lines, .. } if !lines.is_empty() => lines
            .row_at(&doc.source, offset)
            .map_or(0, |row| row.min(lines.len() - 1)),
        _ => 0,
    };
    lay.approx_top(block, line)
}

/// The line of its block an offset stands on, counted as the editor
/// counts rows: a line's break belongs to the line, and past the final
/// break stands the row the caret opens there. A code block's line
/// table answers by a binary search, where a count of the breaks from
/// the block's start grew with the file on every call; a body with text
/// of its own, which has no source coordinates, still counts them.
pub fn source_row(doc: &Document, block: usize, offset: usize) -> usize {
    let Some(block) = doc.blocks.get(block) else {
        return 0;
    };
    if let BlockKind::CodeBlock { lines, .. } = &block.kind {
        if let Some(row) = lines.row_at(&doc.source, offset) {
            return row;
        }
    }
    let start = block.range.start;
    let end = offset.min(doc.source.len()).max(start);
    doc.source[start..end].matches('\n').count()
}

/// How far under the top of the view a landing stands its block, when
/// `below` was asked for a line inside it that draws no row of its own,
/// an image's or a rule's. A block that would run past the bottom edge
/// is lifted until it shows whole, and one taller than the view stands
/// at the top edge, so the reader sees where it starts. A line of a
/// code block whose lines are the source's own is placed itself, not
/// its block, and keeps `below`.
pub fn fitted_below(
    lay: &LayoutDoc,
    doc: &Document,
    offset: usize,
    below: f32,
    view_h: f32,
) -> f32 {
    let Some(block) = doc.block_at_offset(offset) else {
        return below;
    };
    if matches!(&doc.blocks[block].kind, BlockKind::CodeBlock { lines, .. } if !lines.is_empty()) {
        return below;
    }
    match lay.block_span(block) {
        Some(span) => fit(below, span.end - span.start, view_h),
        None => below,
    }
}

/// Where the row of the source line holding `offset` stands before the
/// page has drawn it: its block's top plus the line's share of the
/// block's height, counted in bytes. Only for a line with text in a
/// block drawn as rows of text, whose exact row the frame finds once
/// the slide draws it (`caret::line_top`); None for a blank line, for
/// the other blocks, and before the pass places the block.
pub fn line_estimate(lay: &LayoutDoc, doc: &Document, offset: usize) -> Option<f32> {
    let source = &doc.source;
    let offset = offset.min(source.len());
    let start = source[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at);
    if source[start..end].trim().is_empty() {
        return None;
    }
    let index = doc.block_at_offset(end)?;
    let block = &doc.blocks[index];
    let rows = matches!(
        block.kind,
        BlockKind::Heading { .. }
            | BlockKind::Paragraph { .. }
            | BlockKind::ListItem { .. }
            | BlockKind::Table { .. }
            | BlockKind::FootnoteDef { .. }
            | BlockKind::Summary { .. }
    );
    if !rows {
        return None;
    }
    let span = lay.block_span(index)?;
    let share = start.saturating_sub(block.range.start) as f32 / block.range.len().max(1) as f32;
    Some(span.start + share.min(1.0) * (span.end - span.start))
}

/// A top the edge already cut keeps its height.
fn fit(below: f32, block_h: f32, view_h: f32) -> f32 {
    if below <= 0.0 {
        return below;
    }
    below.min((view_h - block_h).max(0.0))
}

/// The offset a frame paints the page at: the scroll position floored
/// to a whole device pixel. The position itself keeps its fraction,
/// since a touchpad delivers fractions of a pixel per event and a slow
/// scroll accumulates them; the frame reads this once and hands it to
/// every path, the direct paint, the band, its slice and the overlays,
/// so no two frames of one position land a pixel apart.
pub fn frame_offset(scroll_y: f32) -> f32 {
    scroll_y.floor()
}

/// How long a window size holds still before a deferred relayout runs.
pub const SETTLE: Duration = Duration::from_millis(150);

/// A scroll position held while the pass streams is restorable once the
/// placed document is tall enough to show it, which is exactly when
/// clamping leaves it alone.
pub fn reached(target: f32, doc_height: f32, viewport_h: f32) -> bool {
    clamp(target, doc_height, viewport_h) >= target
}

/// A jump to the bottom of a document the pass is still placing. The
/// placed height is the whole height Oryx knows, so such a jump lands
/// short of the file's end; the intent is held instead of being spent
/// on the spot, and the completing pass seats the view where the
/// reader asked to go. The view holds still while the document grows,
/// since a page moving on its own reads as a fault, and a reader who
/// scrolls away in the meantime releases the hold rather than being
/// pulled to the end later.
#[derive(Debug, Default, Clone, Copy)]
pub struct BottomHold {
    held: bool,
    /// Where the jump left the view. Nothing moves it while the hold
    /// stands, so a different position means the reader took over.
    seated: f32,
}

impl BottomHold {
    /// Records a jump that landed at `seated`. Against a complete pass
    /// the bottom is the document's own and nothing is held.
    pub fn take(&mut self, seated: f32, pass_complete: bool) {
        self.held = !pass_complete;
        self.seated = seated;
    }

    pub fn clear(&mut self) {
        self.held = false;
    }

    /// Whether this slice must seat the view on the completed
    /// document's bottom, which spends the hold. A view that has moved
    /// since the jump belongs to the reader again and releases the
    /// hold unspent.
    pub fn settle(&mut self, scroll_y: f32, pass_complete: bool) -> bool {
        if !self.held {
            return false;
        }
        if scroll_y != self.seated {
            self.held = false;
            return false;
        }
        if !pass_complete {
            return false;
        }
        self.held = false;
        true
    }
}

/// A resize drag delivers a new width per frame. Restarting a pass that
/// cannot finish inside one slice would strand the reader at the top for
/// the whole drag, so the current layout is kept until the size settles.
pub fn defer_relayout(last_pass: Duration, slice: Duration) -> bool {
    last_pass > slice
}

/// Painted pixels for `[y_top, y_top + height)` at full window width,
/// covering the viewport plus two viewport heights above and below.
pub struct BandCache {
    pub pixels: Vec<u32>,
    pub y_top: f32,
    pub width: u32,
    pub height: u32,
    pub doc_height: f32,
}

impl BandCache {
    /// Paints a band recentered on `scroll_y`: five viewport heights,
    /// clamped so it never starts above the document top. `numbers` is
    /// the line numbers' color, None when they are off.
    #[allow(clippy::too_many_arguments)]
    pub fn repaint(
        layout: &LayoutDoc,
        doc: &crate::doc::model::Document,
        theme: &Theme,
        fonts: &mut FontStore,
        media: &mut MediaCache,
        extra: &[DecoRect],
        numbers: Option<crate::style::theme::Rgba>,
        scroll_y: f32,
        width: u32,
        viewport_h: u32,
    ) -> BandCache {
        let height = viewport_h * 5;
        let doc_height = layout.height;
        let max_top = (doc_height - height as f32).max(0.0);
        let y_top = (scroll_y - (2 * viewport_h) as f32)
            .clamp(0.0, max_top)
            .floor();
        let pixels = super::band_numbered(
            layout, doc, theme, fonts, media, extra, numbers, y_top, width, height,
        );
        BandCache {
            pixels,
            y_top,
            width,
            height,
            doc_height,
        }
    }

    /// True when the viewport nears a band edge that is not a document edge.
    pub fn needs_repaint(&self, scroll_y: f32, viewport_h: f32) -> bool {
        let bottom = self.y_top + self.height as f32;
        let view_bottom = scroll_y + viewport_h;
        if scroll_y < self.y_top || view_bottom > bottom {
            return true;
        }
        let margin = viewport_h * 0.5;
        (scroll_y - self.y_top < margin && self.y_top > 0.0)
            || (bottom - view_bottom < margin && bottom < self.doc_height)
    }

    /// The viewport slice at `scroll_y`, without repainting.
    pub fn view(&self, scroll_y: f32, viewport_h: u32) -> &[u32] {
        let offset_rows =
            (((scroll_y - self.y_top).max(0.0)) as u32).min(self.height.saturating_sub(viewport_h));
        let start = (offset_rows * self.width) as usize;
        let end = start + (viewport_h * self.width) as usize;
        &self.pixels[start..end.min(self.pixels.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;
    use crate::layout::{layout, ViewConfig};
    use std::path::PathBuf;

    fn lay_of(doc: &Document) -> LayoutDoc {
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        layout(
            doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            2000.0,
        )
    }

    fn code_lines(count: usize) -> String {
        (1..=count)
            .map(|i| format!("let line_{i} = {i};\n"))
            .collect()
    }

    #[test]
    fn a_code_file_keeps_its_place_by_the_line() {
        let source = code_lines(400);
        let doc = load::code_document(Some("rust"), &source);
        let lay = lay_of(&doc);
        for line in [0usize, 1, 64, 399] {
            let start = source
                .match_indices('\n')
                .nth(line.wrapping_sub(1))
                .map_or(0, |(at, _)| at + 1);
            let start = if line == 0 { 0 } else { start };
            let y = offset_top(&lay, &doc, start).expect("the block is placed");
            assert_eq!(
                top_offset(&lay, &doc, y),
                start,
                "line {line} at the top of the view answers with its own start"
            );
            assert_eq!(
                top_offset(&lay, &doc, y + 3.0),
                start,
                "and still does a few pixels into the line"
            );
        }
        let first = offset_top(&lay, &doc, 0).unwrap();
        let later = offset_top(&lay, &doc, source.find("line_65").unwrap()).unwrap();
        assert!(
            later > first + 600.0,
            "line 65 stands far under line 1: {first} {later}"
        );
    }

    #[test]
    fn a_text_file_keeps_its_place_by_the_line() {
        let source: String = (1..=300).map(|i| format!("text line {i}\n")).collect();
        let doc = load::text_document(&source);
        let lay = lay_of(&doc);
        let start = source.find("text line 200").unwrap();
        let y = offset_top(&lay, &doc, start).unwrap();
        assert!(y > 1000.0);
        assert_eq!(top_offset(&lay, &doc, y), start);
    }

    #[test]
    fn a_page_keeps_its_place_by_the_block() {
        let mut source = String::new();
        for i in 0..40 {
            source.push_str(&format!(
                "## Section {i}\n\nA paragraph for section {i}.\n\n"
            ));
        }
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_of(&doc);
        let block = 21;
        let start = doc.blocks[block].range.start;
        let y = offset_top(&lay, &doc, start).unwrap();
        assert_eq!(y, lay.approx_top(block, 0).unwrap());
        assert_eq!(top_offset(&lay, &doc, y), start);
    }

    #[test]
    fn a_code_block_inside_a_page_keeps_its_line() {
        let mut source = String::from("# Title\n\n```rust\n");
        source.push_str(&code_lines(200));
        source.push_str("```\n\nThe end.\n");
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_of(&doc);
        let start = source.find("let line_120 ").unwrap();
        let y = offset_top(&lay, &doc, start).unwrap();
        assert_eq!(top_offset(&lay, &doc, y), start);
        let end = source.find("The end").unwrap();
        assert_eq!(
            top_offset(&lay, &doc, offset_top(&lay, &doc, end).unwrap()),
            end
        );
    }

    #[test]
    fn a_landing_block_shows_whole_or_from_its_top() {
        let view_h = 600.0;
        assert_eq!(
            fit(290.0, 40.0, view_h),
            290.0,
            "a short block keeps the middle"
        );
        assert_eq!(
            fit(290.0, 450.0, view_h),
            150.0,
            "a block that fits is lifted until its bottom shows"
        );
        assert_eq!(
            fit(290.0, 2000.0, view_h),
            0.0,
            "a taller one stands at the top"
        );
        assert_eq!(fit(0.0, 2000.0, view_h), 0.0);
        assert_eq!(
            fit(-12.0, 450.0, view_h),
            -12.0,
            "a top the edge cut keeps its cut"
        );
    }

    #[test]
    fn a_tall_block_stands_from_its_top_and_a_code_line_by_itself() {
        let mut source = String::from("Before the table.\n\n| a | b |\n|---|---|\n");
        for i in 0..80 {
            source.push_str(&format!("| row {i} | cell {i} |\n"));
        }
        source.push_str("\nA short paragraph.\n\n```rust\n");
        source.push_str(&code_lines(200));
        source.push_str("```\n");
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_of(&doc);
        let view_h = 600.0;
        let row = source.find("| row 40 ").unwrap();
        assert_eq!(fitted_below(&lay, &doc, row, 290.0, view_h), 0.0);
        let short = source.find("A short").unwrap();
        assert_eq!(fitted_below(&lay, &doc, short, 290.0, view_h), 290.0);
        let code = source.find("let line_120 ").unwrap();
        assert_eq!(
            fitted_below(&lay, &doc, code, 290.0, view_h),
            290.0,
            "a code line stands itself in the middle, not its block"
        );
    }

    #[test]
    fn a_line_not_drawn_yet_stands_near_its_row() {
        let mut source = String::from("Before the table.\n\n| a | b |\n|---|---|\n");
        for i in 0..80 {
            source.push_str(&format!("| row {i} | cell {i} |\n"));
        }
        source.push_str("\n---\n\n```rust\n");
        source.push_str(&code_lines(20));
        source.push_str("```\n");
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_of(&doc);
        for row in [5, 40, 74] {
            let at = source.find(&format!("| row {row} ")).unwrap();
            let exact = crate::edit::caret::line_top(&lay, &doc, at).unwrap();
            let guess = line_estimate(&lay, &doc, at).unwrap();
            assert!(
                (guess - exact).abs() < 90.0,
                "row {row}: the guess {guess} stands within two rows of {exact}"
            );
        }
        assert_eq!(
            line_estimate(&lay, &doc, source.find("---\n\n```").unwrap()),
            None
        );
        let blank = source.find("\n\n---").unwrap() + 1;
        assert_eq!(
            line_estimate(&lay, &doc, blank),
            None,
            "a blank line draws no row"
        );
        assert_eq!(
            line_estimate(&lay, &doc, source.find("let line_5 ").unwrap()),
            None
        );
    }

    #[test]
    fn a_line_counts_by_the_table_as_by_the_breaks() {
        let sources = [
            code_lines(30) + "\n\nlast",
            code_lines(12),
            "\n\n\n".to_string(),
            "one line".to_string(),
        ];
        for source in &sources {
            for doc in [
                load::code_document(Some("rust"), source),
                load::text_document(source),
            ] {
                if let Some(BlockKind::CodeBlock { lines, .. }) =
                    doc.blocks.first().map(|b| &b.kind)
                {
                    assert!(lines.row_at(&doc.source, 0).is_some(), "the table answers");
                }
                for offset in 0..=source.len() {
                    let Some(block) = doc.block_at_offset(offset) else {
                        continue;
                    };
                    let start = doc.blocks[block].range.start;
                    let counted = source[start..offset.max(start)].matches('\n').count();
                    assert_eq!(
                        source_row(&doc, block, offset),
                        counted,
                        "offset {offset} of {source:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn clamp_bounds() {
        assert_eq!(clamp(-10.0, 1000.0, 300.0), 0.0);
        assert_eq!(clamp(2000.0, 1000.0, 300.0), 700.0);
        assert_eq!(clamp(100.0, 200.0, 300.0), 0.0);
    }

    #[test]
    fn the_frame_offset_floors_and_the_state_keeps_its_fraction() {
        assert_eq!(frame_offset(50.6), 50.0);
        assert_eq!(frame_offset(50.0), 50.0);
        assert_eq!(
            frame_offset(clamp(1000.0, 700.5, 300.0)),
            400.0,
            "a clamp to a fractional maximum floors under it"
        );
        let mut y = 0.0;
        for _ in 0..4 {
            y = clamp(y + 0.3, 1000.0, 300.0);
        }
        assert!((y - 1.2).abs() < 1e-6, "four touchpad steps add up: {y}");
        assert_eq!(frame_offset(y), 1.0);
    }

    #[test]
    fn a_target_is_reached_once_the_placed_document_shows_it() {
        // 700px of placed document under a 300px viewport scrolls to 400.
        assert!(!reached(500.0, 700.0, 300.0));
        assert!(reached(400.0, 700.0, 300.0));
        assert!(reached(500.0, 800.0, 300.0));
        // The top is always reachable, including in an empty document.
        assert!(reached(0.0, 0.0, 300.0));
    }

    #[test]
    fn a_bottom_jump_lands_again_when_the_pass_completes() {
        let (viewport, mut placed) = (300.0, 1000.0);
        let mut hold = BottomHold::default();
        let mut at = clamp(placed, placed, viewport);
        assert_eq!(at, 700.0);
        hold.take(at, false);
        // Slices land under the reader; the view holds still until the
        // document is whole.
        for grown in [4000.0, 9000.0] {
            assert!(
                !hold.settle(at, false),
                "a growing document never moves the view under the reader"
            );
            placed = grown;
        }
        assert!(hold.settle(at, true));
        at = clamp(placed, placed, viewport);
        assert_eq!(at, 8700.0, "the completed pass seats the view at the end");
        assert!(!hold.settle(at, true), "the hold is spent");
    }

    #[test]
    fn reading_elsewhere_releases_the_bottom_jump() {
        let mut hold = BottomHold::default();
        hold.take(700.0, false);
        // The reader scrolls back up before the pass completes; the
        // document must not jump out from under them later.
        assert!(!hold.settle(120.0, false));
        assert!(!hold.settle(120.0, true));
    }

    #[test]
    fn a_jump_against_a_complete_pass_holds_nothing() {
        let mut hold = BottomHold::default();
        hold.take(700.0, true);
        assert!(!hold.settle(700.0, true));
    }

    #[test]
    fn relayout_defers_only_when_a_pass_outlasts_a_slice() {
        let slice = Duration::from_millis(16);
        assert!(!defer_relayout(Duration::from_millis(5), slice));
        assert!(!defer_relayout(slice, slice));
        assert!(defer_relayout(Duration::from_millis(17), slice));
        assert!(defer_relayout(Duration::from_secs(5), slice));
    }

    fn band(y_top: f32, height: u32, doc_height: f32) -> BandCache {
        BandCache {
            pixels: Vec::new(),
            y_top,
            width: 1,
            height,
            doc_height,
        }
    }

    #[test]
    fn no_repaint_at_band_center() {
        let b = band(1000.0, 1500, 10000.0);
        assert!(!b.needs_repaint(1600.0, 300.0));
    }

    #[test]
    fn repaint_near_inner_edges() {
        let b = band(1000.0, 1500, 10000.0);
        assert!(b.needs_repaint(1100.0, 300.0));
        assert!(b.needs_repaint(2150.0, 300.0));
    }

    #[test]
    fn no_repaint_at_document_edges() {
        let top = band(0.0, 1500, 10000.0);
        assert!(!top.needs_repaint(0.0, 300.0));
        let bottom = band(8500.0, 1500, 10000.0);
        assert!(!bottom.needs_repaint(9700.0, 300.0));
    }

    #[test]
    fn view_slices_exact_rows() {
        let width = 4u32;
        let rows = 10u32;
        let mut pixels = Vec::new();
        for row in 0..rows {
            pixels.extend(std::iter::repeat_n(row, width as usize));
        }
        let b = BandCache {
            pixels,
            y_top: 100.0,
            width,
            height: rows,
            doc_height: 1000.0,
        };
        let view = b.view(103.0, 2);
        assert_eq!(view.len(), 8);
        assert_eq!(view[0], 3);
        assert_eq!(view[7], 4);
    }
}
