//! Scroll clamping and the band cache. Scrolling inside the band is a
//! memcpy slice; the band repaints recentered only near its edges.

use std::ops::Range;
use std::time::{Duration, Instant};

use crate::doc::images::MediaCache;
use crate::doc::model::{BlockKind, Document};
use crate::layout::{DecoRect, LayoutDoc};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

pub fn clamp(y: f32, doc_height: f32, viewport_h: f32) -> f32 {
    y.clamp(0.0, (doc_height - viewport_h).max(0.0))
}

/// How often the edge scroll of a selection drag advances the view: the
/// frame, as the fling's tick. A move during a drag costs about a
/// millisecond on the window's thread at every tier (Task 347).
pub const EDGE_SCROLL_TICK: Duration = Duration::from_millis(16);

/// The bounds of the three bands of `edge_speed`, in rows past the
/// edge: near, up to a row and a half; middle, up to three; far beyond.
const EDGE_NEAR: f32 = 1.5;
const EDGE_FAR: f32 = 3.0;

/// The edge of the view a held pointer is at or past.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Top,
    Bottom,
}

/// The edge a pointer's y (the window's pixels) is at or past, with the
/// pixels past it: 0 on the edge's own pixel row, which counts so that a
/// maximized window, where the pointer cannot leave, scrolls too. None
/// inside the view.
pub fn edge_of(y: f32, view_h: f32) -> Option<(Edge, f32)> {
    let bottom = view_h - 1.0;
    if y <= 0.0 {
        Some((Edge::Top, -y))
    } else if y >= bottom {
        Some((Edge::Bottom, y - bottom))
    } else {
        None
    }
}

/// The speed of the edge scroll in rows a second, for a pointer
/// `rows_past` the edge on a view of `rows_in_view` rows. VS Code's
/// curve: a view a second on the edge, growing with the distance, ten
/// views a second and more far out; the floors keep a short view
/// moving.
pub fn edge_speed(rows_past: f32, rows_in_view: f32) -> f32 {
    let (views_added, floor) = if rows_past <= EDGE_NEAR {
        (1.0, 30.0)
    } else if rows_past <= EDGE_FAR {
        (2.0, 60.0)
    } else {
        (7.0, 200.0)
    };
    (rows_in_view * (views_added + rows_past)).max(floor)
}

/// The y the selection's end is placed at during a drag: the pointer's
/// y clamped to the view's pixel rows, so the end is never above or
/// below the view.
pub fn edge_clamp(y: f32, view_h: f32) -> f32 {
    y.clamp(0.0, (view_h - 1.0).max(0.0))
}

/// The edge scroll of a selection drag: the edge the pointer is at or
/// past with the pixels past it, and the instant the view last stepped
/// (the pointer's arrival at the edge, before the first tick). Idle
/// while the pointer is inside the view or no drag is live.
#[derive(Debug, Default)]
pub struct EdgeScroll {
    edge: Option<(Edge, f32)>,
    last: Option<Instant>,
}

impl EdgeScroll {
    /// The pointer moved to `y` during a live drag: at or past an edge
    /// the scroll runs, its clock started now when it was idle; inside,
    /// it stops.
    pub fn moved(&mut self, y: f32, view_h: f32, now: Instant) {
        self.edge = edge_of(y, view_h);
        self.last = self.edge.map(|_| self.last.unwrap_or(now));
    }

    /// The drag ended: the scroll stops.
    pub fn stop(&mut self) {
        self.edge = None;
        self.last = None;
    }

    pub fn running(&self) -> bool {
        self.edge.is_some()
    }

    /// A tick: the step in pixels toward the edge, the speed times the
    /// time since the last tick, negative upward. None while idle.
    pub fn tick(&mut self, now: Instant, row_h: f32, view_h: f32) -> Option<f32> {
        let (edge, past) = self.edge?;
        let elapsed = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32()
        });
        self.last = Some(now);
        let step = edge_speed(past / row_h, view_h / row_h) * elapsed * row_h;
        Some(match edge {
            Edge::Top => -step,
            Edge::Bottom => step,
        })
    }
}

/// The source offset of what stands at the top of the view: the start
/// of the block there, and inside a code block whose lines are the
/// source's own, the start of the line there. A code or text file is
/// one such block, so its place is a line, not the top of the file.
/// The block is looked up on the page, not in the order of the source:
/// a footnote definition stands at the end of the page, and the blocks
/// of a closed details group are not on it.
pub fn top_offset(lay: &LayoutDoc, doc: &Document, scroll_y: f32) -> usize {
    let at = lay
        .block_at(scroll_y + 1.0)
        .filter(|&index| index < doc.blocks.len());
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
/// block whose lines answer for a source offset. None before the pass
/// places the block.
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

/// The top and the bottom of what `offset_top` answers for: the block
/// of a source offset, or its line inside a code block whose lines
/// answer for a source offset. None before the pass places the block.
pub fn offset_span(lay: &LayoutDoc, doc: &Document, offset: usize) -> Option<Range<f32>> {
    let block = doc.block_at_offset(offset)?;
    if let BlockKind::CodeBlock { lines, .. } = &doc.blocks[block].kind {
        if let Some(row) = lines.row_at(&doc.source, offset) {
            let line = row.min(lines.len().saturating_sub(1));
            // The lines tile the block, so a line ends where the next
            // one starts.
            let top = lay.approx_top(block, line)?;
            let bottom = lay.approx_top(block, line + 1)?;
            return Some(top..bottom.max(top));
        }
    }
    lay.block_span(block)
}

/// What stands at the top of a view, kept so that a new layout of the
/// same text shows it there again: after a zoom step, a new width or a
/// new font, the page above the view has another height, and the
/// scroll as a distance in pixels would show other lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopPlace {
    /// The source offset `top_offset` answers for the view: a block,
    /// or a line of a code or text file.
    pub offset: usize,
    /// How far under the top of that block or line the top edge of the
    /// view stood.
    above: f32,
    /// The height of that block or line in the layout the place was
    /// taken from.
    height: f32,
    /// The heights are those of the whole block, since its line did
    /// not hold the top edge.
    whole: bool,
}

impl TopPlace {
    /// The place at the top of the view at `scroll_y`. None at the top
    /// of the file and inside the margin above the first block, where
    /// the distance itself is the place, and before the pass has placed
    /// a block.
    pub fn of(lay: &LayoutDoc, doc: &Document, scroll_y: f32) -> Option<TopPlace> {
        let offset = top_offset(lay, doc, scroll_y);
        let block = doc.block_at_offset(offset)?;
        let holds = |span: &Range<f32>| span.start <= scroll_y + 1.0 && scroll_y <= span.end;
        let (span, whole) = match offset_span(lay, doc, offset).filter(holds) {
            Some(span) => (span, false),
            None => (lay.block_span(block)?, true),
        };
        if scroll_y + 1.0 < span.start {
            return None;
        }
        Some(TopPlace {
            offset,
            above: (scroll_y - span.start).max(0.0),
            height: span.end - span.start,
            whole,
        })
    }

    /// The scroll that shows the place again on a layout of the same
    /// text: the same share of the block or the line stands above the
    /// top edge. A layout that left the block as it was answers the
    /// scroll the place was taken at. None before the pass places the
    /// block.
    pub fn scroll(&self, lay: &LayoutDoc, doc: &Document) -> Option<f32> {
        let span = if self.whole {
            lay.block_span(doc.block_at_offset(self.offset)?)?
        } else {
            offset_span(lay, doc, self.offset)?
        };
        let height = span.end - span.start;
        let above = if height == self.height || self.height <= 0.0 {
            self.above
        } else {
            self.above * height / self.height
        };
        Some(span.start + above)
    }
}

/// The line of its block an offset stands on, counted as the editor
/// counts rows: a line's break belongs to the line, and past the final
/// break stands the row the caret opens there. A code block's line
/// table answers by a binary search, where a count of the breaks from
/// the block's start grew with the file on every call; a body with text
/// of its own whose source lines are not recorded still counts them.
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
/// code block whose lines answer for a source offset is placed itself,
/// not its block, and keeps `below`.
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
    if matches!(&doc.blocks[block].kind, BlockKind::CodeBlock { lines, .. } if !lines.is_empty() && lines.has_rows())
    {
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
    // A remembered offset may fall inside a character after an edit.
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
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
    /// Painted while the layout could not slide its window: the pass
    /// was inside a code block, so lines of the band may be missing,
    /// and the line numbers under the top of the block are.
    pub partial: bool,
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
            partial: !layout.slides(),
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

    /// True when the band was painted while the layout could not slide
    /// its window, and the layout can slide it now. The layout then
    /// has the lines and the line numbers that the band lacks, so the
    /// band is painted again, though the view did not move.
    pub fn outdated(&self, layout: &LayoutDoc) -> bool {
        self.partial && layout.slides()
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
        let mut media = MediaCache::offline(PathBuf::from("."));
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

    fn lay_at(doc: &Document, zoom: f32, width: f32) -> LayoutDoc {
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let cfg = ViewConfig {
            zoom,
            ..ViewConfig::default()
        };
        layout(
            doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &cfg,
            width,
        )
    }

    /// Paragraphs of several rows, each naming itself.
    fn paragraphs(count: usize) -> String {
        (1..=count)
            .map(|i| {
                format!(
                    "Paragraph {i}: the reader follows this text across the page, and each \
                     paragraph is long enough to take several rows at the width of the \
                     window, so a change of size moves the breaks of its lines.\n\n"
                )
            })
            .collect()
    }

    #[test]
    fn the_block_at_the_top_of_the_view_stays_there_at_another_text_size() {
        let source = paragraphs(120);
        let doc = crate::doc::markdown::parse(source.as_str());
        let start = source.find("Paragraph 90:").unwrap();
        let before = lay_at(&doc, 1.0, 700.0);
        let span = offset_span(&before, &doc, start).expect("the paragraph is placed");
        // The top edge of the view cuts the paragraph in its middle.
        let scroll = span.start + (span.end - span.start) / 2.0;
        let place = TopPlace::of(&before, &doc, scroll).expect("a block stands at the top");
        assert_eq!(place.offset, start);
        for (zoom, width) in [(1.6, 700.0), (0.7, 700.0), (1.0, 450.0)] {
            let after = lay_at(&doc, zoom, width);
            let kept = place.scroll(&after, &doc).expect("the paragraph is placed");
            assert_eq!(
                top_offset(&after, &doc, kept),
                start,
                "the same paragraph at the top, zoom {zoom}, width {width}"
            );
            let span = offset_span(&after, &doc, start).unwrap();
            let share = (kept - span.start) / (span.end - span.start);
            assert!(
                (share - 0.5).abs() < 0.01,
                "the same share of it above the edge: {share}"
            );
        }
        // What the page did before: the distance times the zoom ratio.
        let after = lay_at(&doc, 1.6, 700.0);
        assert_ne!(
            top_offset(&after, &doc, scroll * 1.6),
            start,
            "the scaled distance shows another paragraph"
        );
    }

    #[test]
    fn the_line_at_the_top_of_the_view_stays_there_in_a_file_of_lines() {
        // Every third line is long enough to wrap at the larger size.
        let source: String = (1..=600)
            .map(|i| {
                let tail = if i % 3 == 0 {
                    "value ".repeat(14)
                } else {
                    String::new()
                };
                format!("let line_{i} = {i}; // {tail}\n")
            })
            .collect();
        let doc = load::code_document(Some("rust"), &source);
        let start = source.find("let line_500 ").unwrap();
        let before = lay_at(&doc, 1.0, 1000.0);
        let top = offset_top(&before, &doc, start).unwrap();
        let place = TopPlace::of(&before, &doc, top + 3.0).expect("a line stands at the top");
        assert_eq!(place.offset, start);
        let after = lay_at(&doc, 1.6, 1000.0);
        let kept = place.scroll(&after, &doc).expect("the line is placed");
        assert_eq!(top_offset(&after, &doc, kept), start, "the same line");
        assert_ne!(
            top_offset(&after, &doc, (top + 3.0) * 1.6),
            start,
            "the scaled distance shows another line"
        );
    }

    #[test]
    fn a_layout_that_did_not_change_brings_the_same_view_back() {
        let source = paragraphs(60);
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_at(&doc, 1.0, 700.0);
        for scroll in [120.0_f32, 1234.5, 3000.25, lay.height - 600.0] {
            let place = TopPlace::of(&lay, &doc, scroll).expect("a block stands at the top");
            assert_eq!(place.scroll(&lay, &doc), Some(scroll));
        }
    }

    #[test]
    fn a_view_at_the_top_of_the_file_names_no_place() {
        let source = paragraphs(10);
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_at(&doc, 1.0, 700.0);
        assert_eq!(
            TopPlace::of(&lay, &doc, 0.0),
            None,
            "the view stays at the top"
        );
    }

    /// The top of the view at paragraph 60 of a page whose first
    /// paragraph is followed by `between`: the place found there, and
    /// brought back at another text size.
    fn top_of_view_under(between: &str) {
        let source = format!(
            "Paragraph 0 cites a note[^1].\n\n{between}{}",
            paragraphs(80)
        );
        let doc = crate::doc::markdown::parse(source.as_str());
        let start = source.find("Paragraph 60:").unwrap();
        let lay = lay_at(&doc, 1.0, 700.0);
        let top = offset_top(&lay, &doc, start).expect("the paragraph is placed");
        assert_eq!(
            top_offset(&lay, &doc, top + 3.0),
            start,
            "the paragraph at the top of the view"
        );
        let place = TopPlace::of(&lay, &doc, top + 3.0).expect("a block stands at the top");
        assert_eq!(place.offset, start);
        let after = lay_at(&doc, 1.6, 700.0);
        let kept = place.scroll(&after, &doc).expect("the paragraph is placed");
        assert_eq!(
            top_offset(&after, &doc, kept),
            start,
            "the same paragraph at another text size"
        );
    }

    #[test]
    fn the_top_of_the_view_is_found_under_a_footnote_definition() {
        // The definition is written under its paragraph, and the page
        // places it at its end, after the last paragraph.
        top_of_view_under("[^1]: The note.\n\n");
    }

    #[test]
    fn the_top_of_the_view_is_found_under_a_closed_details_section() {
        // The blocks of a closed section have no place on the page.
        top_of_view_under("<details>\n<summary>More</summary>\n\nHidden text.\n\n</details>\n\n");
    }

    #[test]
    fn a_view_on_the_notes_at_the_end_of_the_page_names_the_definition() {
        let source = format!(
            "Paragraph 0 cites a note[^1].\n\n[^1]: The note.\n\n{}",
            paragraphs(80)
        );
        let doc = crate::doc::markdown::parse(source.as_str());
        let note = doc
            .blocks
            .iter()
            .find(|block| matches!(block.kind, BlockKind::FootnoteDef { .. }))
            .expect("the definition is a block")
            .range
            .start;
        let last = source.find("Paragraph 80:").unwrap();
        let lay = lay_at(&doc, 1.0, 700.0);
        let top = offset_top(&lay, &doc, note).expect("the note is placed");
        assert!(
            top > offset_top(&lay, &doc, last).unwrap(),
            "the note stands under the last paragraph"
        );
        assert_eq!(top_offset(&lay, &doc, top), note);
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
    fn a_line_of_a_code_body_of_its_own_is_placed_by_itself() {
        let shapes = [
            ("1. Run:\n\n   ```sh\n", "   ", "   ```\n\n"),
            ("Run:\n\n", "    ", "\n"),
            ("> Run:\n>\n> ```sh\n", "> ", "> ```\n\n"),
        ];
        for (open, indent, close) in shapes {
            let mut source = format!("Before the code.\n\n{open}");
            for i in 0..150 {
                source.push_str(&format!("{indent}echo step {i:03}\n"));
            }
            source.push_str(close);
            source.push_str("After.\n");
            let doc = crate::doc::markdown::parse(source.as_str());
            let lay = lay_of(&doc);
            let at = source.find("echo step 100").unwrap();
            let block = doc.block_at_offset(at).unwrap();
            assert!(lay.approx_top(block, 100) > lay.approx_top(block, 0));
            assert_eq!(
                offset_top(&lay, &doc, at),
                lay.approx_top(block, 100),
                "{open:?}: the line answers its own row"
            );
            assert_eq!(
                fitted_below(&lay, &doc, at, 290.0, 600.0),
                290.0,
                "{open:?}: and stands itself in the middle"
            );
        }
    }

    #[test]
    fn a_code_body_with_no_place_in_the_source_stands_from_its_top() {
        let mut source = String::from("Before the code.\n\n<pre>\n");
        for i in 0..150 {
            source.push_str(&format!("echo step {i:03}\n"));
        }
        source.push_str("</pre>\n\nAfter.\n");
        let doc = crate::doc::markdown::parse(source.as_str());
        let lay = lay_of(&doc);
        // The lines of a pre block are a copy with no place in the source.
        let block = doc
            .blocks
            .iter()
            .position(|block| {
                matches!(&block.kind, BlockKind::CodeBlock { lines, .. } if !lines.has_rows())
            })
            .expect("a code block whose lines do not answer");
        let at = doc.blocks[block].range.start;
        assert_eq!(doc.block_at_offset(at), Some(block));
        assert_eq!(offset_top(&lay, &doc, at), lay.approx_top(block, 0));
        assert_eq!(
            fitted_below(&lay, &doc, at, 290.0, 600.0),
            0.0,
            "the block is taller than the view and no line of it answers"
        );
    }

    #[test]
    fn an_offset_inside_a_character_answers_the_estimate_of_its_line() {
        let source = "Before.\n\néé and words\n\nAfter.\n";
        let doc = crate::doc::markdown::parse(source);
        let lay = lay_of(&doc);
        let line = source.find("éé").unwrap();
        let whole = line_estimate(&lay, &doc, line);
        assert!(whole.is_some(), "the line has a row");
        assert_eq!(
            line_estimate(&lay, &doc, line + 1),
            whole,
            "inside the first é"
        );
        assert_eq!(
            line_estimate(&lay, &doc, line + 3),
            whole,
            "inside the second é"
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
            partial: false,
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
            partial: false,
        };
        let view = b.view(103.0, 2);
        assert_eq!(view.len(), 8);
        assert_eq!(view[0], 3);
        assert_eq!(view[7], 4);
    }

    /// The edge a pointer is at or past, and the pixels past it: the
    /// edge's own pixel row counts, so a maximized window scrolls too;
    /// a row inside does not.
    #[test]
    fn a_pointer_at_or_past_an_edge_names_the_edge_and_the_pixels_past_it() {
        assert_eq!(edge_of(599.0, 600.0), Some((Edge::Bottom, 0.0)));
        assert_eq!(edge_of(650.0, 600.0), Some((Edge::Bottom, 51.0)));
        assert_eq!(edge_of(0.0, 600.0), Some((Edge::Top, 0.0)));
        assert_eq!(edge_of(-24.0, 600.0), Some((Edge::Top, 24.0)));
        assert_eq!(edge_of(1.0, 600.0), None);
        assert_eq!(edge_of(598.0, 600.0), None);
        assert_eq!(edge_of(300.0, 600.0), None);
    }

    /// The speed in rows a second: a view a second on the edge, more
    /// with the distance past it, never under the floors.
    #[test]
    fn the_edge_speed_grows_with_the_distance_past_the_edge() {
        assert_eq!(
            edge_speed(0.0, 25.0),
            30.0,
            "25 rows in view: the floor of 30"
        );
        assert_eq!(
            edge_speed(0.0, 40.0),
            40.0,
            "40 rows in view: a view a second"
        );
        assert_eq!(edge_speed(1.0, 25.0), 50.0);
        assert_eq!(edge_speed(1.5, 25.0), 62.5);
        assert_eq!(edge_speed(2.0, 25.0), 100.0);
        assert_eq!(edge_speed(3.0, 25.0), 125.0);
        assert_eq!(edge_speed(4.0, 25.0), 275.0);
        assert_eq!(
            edge_speed(5.0, 10.0),
            200.0,
            "far out on a short view: the floor of 200"
        );
    }

    /// A tick steps the view toward the edge by the speed times the
    /// time since the last tick, in pixels: a pointer on the bottom row
    /// of a 600 px view of 24 px rows (30 rows a second) steps 72 px in
    /// 100 ms, and on the top row the same upward.
    #[test]
    fn a_tick_steps_toward_the_edge_by_the_speed_times_the_elapsed_time() {
        let t0 = Instant::now();
        let mut edge = EdgeScroll::default();
        edge.moved(599.0, 600.0, t0);
        assert!(edge.running());
        let step = edge
            .tick(t0 + Duration::from_millis(100), 24.0, 600.0)
            .expect("a step");
        assert!((step - 72.0).abs() < 0.01, "{step}");
        let mut edge = EdgeScroll::default();
        edge.moved(0.0, 600.0, t0);
        let step = edge
            .tick(t0 + Duration::from_millis(100), 24.0, 600.0)
            .expect("a step");
        assert!((step + 72.0).abs() < 0.01, "{step}");
    }

    /// A tick counts from the last tick, and a return to the edge after
    /// a stay inside starts the clock again at the return.
    #[test]
    fn a_tick_counts_from_the_last_tick_or_from_the_return_to_the_edge() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut edge = EdgeScroll::default();
        edge.moved(599.0, 600.0, t0);
        edge.tick(t0 + ms(100), 24.0, 600.0);
        let step = edge.tick(t0 + ms(200), 24.0, 600.0).expect("a step");
        assert!((step - 72.0).abs() < 0.01, "{step}");
        edge.moved(300.0, 600.0, t0 + ms(250));
        edge.moved(599.0, 600.0, t0 + ms(1000));
        let step = edge.tick(t0 + ms(1100), 24.0, 600.0).expect("a step");
        assert!((step - 72.0).abs() < 0.01, "{step}, not the 800 ms inside");
    }

    /// The scroll stops when the pointer comes back inside and when the
    /// drag ends: a tick then steps nothing.
    #[test]
    fn the_edge_scroll_stops_inside_the_view_and_at_the_drag_s_end() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut edge = EdgeScroll::default();
        edge.moved(650.0, 600.0, t0);
        edge.moved(300.0, 600.0, t0 + ms(50));
        assert!(!edge.running());
        assert_eq!(edge.tick(t0 + ms(100), 24.0, 600.0), None);
        edge.moved(650.0, 600.0, t0 + ms(150));
        edge.stop();
        assert!(!edge.running());
        assert_eq!(edge.tick(t0 + ms(200), 24.0, 600.0), None);
    }

    /// The selection's end is placed on a row of the view: the pointer's
    /// y clamped to the view's pixel rows.
    #[test]
    fn the_selection_s_end_stays_within_the_view() {
        assert_eq!(edge_clamp(-10.0, 600.0), 0.0);
        assert_eq!(edge_clamp(650.0, 600.0), 599.0);
        assert_eq!(edge_clamp(300.0, 600.0), 300.0);
    }
}
