//! Find in document: smart-case substring matching over the document's
//! display text. Each match is a `Selection`, so highlight geometry and
//! scroll targets reuse the selection machinery.

use std::borrow::Cow;
use std::ops::Range;

use crate::doc::model::{BlockKind, CodeBody, Document};
use crate::edit::caret::word_char;
use crate::paint::painter::Painter;
use crate::style::fonts::CODE_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::selection::{block_pieces, ModelPos, Piece, Selection};
use crate::ui::textfield::TextField;

/// A character no query can contain, planted between blocks so a match
/// never crosses them.
const BLOCK_SEP: char = '\u{1}';

const BAR_WIDTH: f32 = 344.0;
pub(crate) const BAR_HEIGHT: f32 = 40.0;
pub(crate) const MARGIN: f32 = 16.0;
pub(crate) const PAD: f32 = 16.0;
pub(crate) const RADIUS: f32 = 20.0;
pub(crate) const QUERY_SIZE: f32 = 15.0;
pub(crate) const COUNTER_SIZE: f32 = 13.0;
const TOGGLE_W: f32 = 26.0;
const TOGGLE_H: f32 = 22.0;
const ROW_H: f32 = 36.0;
/// The glyphs sit this much below the row's top, so the text's caps
/// stand centered on the caret and the pill, level with the toggle.
pub(crate) const TEXT_DROP: f32 = 2.0;

/// Live find session: the query as typed, its matches, and the cursor
/// among them. `stale` marks the matches for recomputation against the
/// current layout on the next frame; `rects` caches every match's
/// highlight boxes with their match index, so painting a frame never
/// re-shapes text. The query is a `TextField`, so reopening the bar with
/// the old query selected is a plain `select_all` and the next typed
/// character replaces it.
pub struct SearchState {
    pub query: TextField,
    /// Where the query's text was last drawn, for the mouse.
    pub query_view: FieldView,
    /// The same for the replace field.
    pub replace_view: FieldView,
    /// Regex matching instead of plain text, flipped by the bar's
    /// toggle and kept across close and reopen like the query.
    pub regex: bool,
    /// The pattern failed to compile or exceeded the backtracking
    /// limit; the bar shows the caution border and no counter.
    pub error: bool,
    /// The replace row while find and replace is open; edit mode only,
    /// dropped on leaving the editor. Focus lives inside the row, so a
    /// focused replace field without a row cannot exist.
    pub replace: Option<ReplaceRow>,
    /// A replace happened at this source offset: once the matches
    /// recompute, the current one seats on the first at or after it,
    /// wrapping to the first. Consumed by the recompute, however many
    /// frames the relayout takes to let it run.
    pub seek: Option<usize>,
    /// A replace just edited the document, so Ctrl+Z and Ctrl+Shift+Z
    /// pass through to it and take the replacement back. Any key the
    /// bar claims turns them back to the field. Clicks never set this:
    /// a click off the bar closes it instead.
    pub doc_intent: bool,
    pub matches: Vec<Selection>,
    /// Highlight rects for the matches inside the band window alone;
    /// the counter and navigation use the full match list.
    pub rects: Vec<(usize, (f32, f32, f32, f32))>,
    /// The scroll position `rects` was computed around, so drifting a
    /// viewport past it triggers a refresh.
    pub rects_scroll: f32,
    pub current: usize,
    pub stale: bool,
    /// The current match landed on its recorded block top because its
    /// region was cold; the exact anchor still owes a centering.
    pub settle: bool,
}

/// Where a field's text was last drawn: the left edge of its text and
/// the x of every character boundary from there, the windowed text's
/// characters at their drawn places and the cut ones beyond the box. A
/// click and a drag read it; the bar refreshes it every frame it
/// draws, so it describes the text the pointer sees.
#[derive(Debug, Default, Clone)]
pub struct FieldView {
    pub left: f32,
    pub offsets: Vec<f32>,
    /// The bytes drawn, which the next frame keeps while the caret
    /// stays inside them.
    pub window: Range<usize>,
}

/// The second row of the bar: the replacement text and whether its
/// field takes the typing instead of the query.
pub struct ReplaceRow {
    pub field: TextField,
    pub focused: bool,
}

impl SearchState {
    /// The replace row is shown and takes the typing.
    pub fn replace_focused(&self) -> bool {
        self.replace.as_ref().is_some_and(|row| row.focused)
    }

    /// The field the keyboard feeds.
    pub fn focused(&self) -> &TextField {
        match self.replace.as_ref() {
            Some(row) if row.focused => &row.field,
            _ => &self.query,
        }
    }

    pub fn focused_mut(&mut self) -> &mut TextField {
        match self.replace.as_mut() {
            Some(row) if row.focused => &mut row.field,
            _ => &mut self.query,
        }
    }

    /// The focused field's selected text, when any; the one accessor
    /// copy and cut share so they cannot drift.
    pub fn focused_selection(&self) -> Option<String> {
        let text = self.focused().selected_text();
        (!text.is_empty()).then(|| text.to_string())
    }
}

/// The floating pill over the document's top-right corner: query on the
/// left, match counter on the right, in the theme's overlay colors. The
/// fields' views are refreshed for the mouse. `caret_lit` is the blink's
/// lit half while the bar has the keys.
pub fn draw_bar(
    painter: &mut Painter,
    theme: &Theme,
    state: &mut SearchState,
    width: f32,
    caret_lit: bool,
) {
    let ui = &theme.ui;
    let x = (width - BAR_WIDTH - MARGIN).max(MARGIN);
    let y = MARGIN;
    let height = if state.replace.is_some() {
        BAR_HEIGHT + ROW_H
    } else {
        BAR_HEIGHT
    };
    for (grow, alpha) in [(6.0, 16), (3.0, 28)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            BAR_WIDTH + 2.0 * grow,
            height + 2.0 * grow,
            RADIUS + grow,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: alpha,
            },
        );
    }
    painter.fill(x, y, BAR_WIDTH, height, RADIUS, ui.overlay_bg);
    let border = if state.error {
        theme.alerts.caution
    } else {
        theme.blocks.table_border
    };
    painter.stroke(x, y, BAR_WIDTH, height, RADIUS, 1.0, border);

    // The regex toggle, drawn on the selection background while active.
    let toggle_x = x + BAR_WIDTH - 10.0 - TOGGLE_W;
    let toggle_y = y + (BAR_HEIGHT - TOGGLE_H) / 2.0;
    if state.regex {
        painter.fill(toggle_x, toggle_y, TOGGLE_W, TOGGLE_H, 6.0, ui.selection_bg);
    }
    let label_w = painter.measure(".*", CODE_FAMILY, COUNTER_SIZE, 600);
    let label_color = if state.regex {
        ui.overlay_fg
    } else {
        theme.blocks.frontmatter_fg
    };
    painter.text(
        toggle_x + (TOGGLE_W - label_w) / 2.0,
        toggle_y + 2.0,
        ".*",
        CODE_FAMILY,
        COUNTER_SIZE,
        600,
        label_color,
    );

    let counter = if state.query.is_empty() || state.error {
        String::new()
    } else {
        let shown = if state.matches.is_empty() {
            0
        } else {
            state.current + 1
        };
        format!("{shown}/{}", state.matches.len())
    };
    let counter_w = painter.measure(&counter, CODE_FAMILY, COUNTER_SIZE, 400);
    let counter_color = if !state.query.is_empty() && state.matches.is_empty() {
        theme.alerts.caution
    } else {
        theme.blocks.frontmatter_fg
    };
    // Tops differ by the ascent difference so both texts share a baseline.
    painter.text(
        toggle_x - 10.0 - counter_w,
        y + 10.0 + TEXT_DROP + (QUERY_SIZE - COUNTER_SIZE) * 1.1,
        &counter,
        CODE_FAMILY,
        COUNTER_SIZE,
        400,
        counter_color,
    );

    let avail = toggle_x - 10.0 - counter_w - 12.0 - (x + PAD);
    state.query_view = draw_field(
        painter,
        theme,
        &state.query,
        x + PAD,
        y + 10.0,
        avail,
        "find",
        !state.replace_focused(),
        caret_lit,
        state.query_view.window.start,
    );

    if let Some(row) = state.replace.as_ref() {
        painter.line(
            x + PAD,
            y + BAR_HEIGHT - 1.0,
            x + BAR_WIDTH - PAD,
            y + BAR_HEIGHT - 1.0,
            1.0,
            theme.blocks.table_border,
        );
        // The replace-all hint sits where the counter does above, in
        // the same face and color.
        let hint = "Ctrl+\u{23CE} all";
        let hint_w = painter.measure(hint, CODE_FAMILY, COUNTER_SIZE, 400);
        painter.text(
            x + BAR_WIDTH - PAD - hint_w,
            y + BAR_HEIGHT + 7.0 + TEXT_DROP + (QUERY_SIZE - COUNTER_SIZE) * 1.1,
            hint,
            CODE_FAMILY,
            COUNTER_SIZE,
            400,
            theme.blocks.frontmatter_fg,
        );
        state.replace_view = draw_field(
            painter,
            theme,
            &row.field,
            x + PAD,
            y + BAR_HEIGHT + 7.0,
            BAR_WIDTH - 2.0 * PAD - hint_w - 12.0,
            "replace",
            row.focused,
            caret_lit,
            state.replace_view.window.start,
        );
    }
}

/// What a click at logical window coordinates lands on in the bar.
pub enum BarHit {
    Toggle,
    Query,
    Replace,
}

/// The bar element under a point, `None` outside the pill; the geometry
/// mirrors `draw_bar`.
pub fn bar_hit(width: f32, replace_row: bool, px: f32, py: f32) -> Option<BarHit> {
    let x = (width - BAR_WIDTH - MARGIN).max(MARGIN);
    let height = if replace_row {
        BAR_HEIGHT + ROW_H
    } else {
        BAR_HEIGHT
    };
    if !(x..x + BAR_WIDTH).contains(&px) || !(MARGIN..MARGIN + height).contains(&py) {
        return None;
    }
    if toggle_hit(width, px, py) {
        return Some(BarHit::Toggle);
    }
    if replace_row && py >= MARGIN + BAR_HEIGHT {
        return Some(BarHit::Replace);
    }
    Some(BarHit::Query)
}

/// One field row: windowed text with the caret kept visible, the
/// placeholder when empty, the selection drawn only on the field the
/// keyboard feeds, and its caret only on the blink's lit half
/// (`caret_lit`). Answers where the text stands, for the mouse.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_field(
    painter: &mut Painter,
    theme: &Theme,
    field: &TextField,
    left: f32,
    top: f32,
    avail: f32,
    placeholder: &str,
    focused: bool,
    caret_lit: bool,
    previous_start: usize,
) -> FieldView {
    let ui = &theme.ui;
    let text = field.text();
    let (window, cut) = window_fit(painter, text, field.caret(), avail, previous_start);
    let shown = if cut {
        format!("\u{2026}{}", &text[window.clone()])
    } else {
        text[window.clone()].to_string()
    };
    // The boundaries are read on the drawn window's shaped line, so a
    // caret in the middle of a text too long for the pill still lands
    // under its character, and one in right-to-left text at its place.
    let positions = painter.boundaries(&shown, CODE_FAMILY, QUERY_SIZE, 400);
    let x_of = |at: usize| {
        let at = at.clamp(window.start, window.end);
        let index = usize::from(cut) + text[window.start..at].chars().count();
        positions.get(index).copied().unwrap_or(0.0)
    };
    if focused {
        if let Some(range) = field.selection() {
            let (from, to) = (x_of(range.start), x_of(range.end));
            // On right-to-left text the end stands left of the start.
            let (from, to) = (from.min(to), from.max(to));
            painter.fill(
                left + from - 2.0,
                top - 1.0,
                to - from + 4.0,
                BAR_HEIGHT - 18.0,
                4.0,
                ui.selection_bg,
            );
        }
    }
    if text.is_empty() {
        painter.text(
            left + 6.0,
            top + TEXT_DROP,
            placeholder,
            CODE_FAMILY,
            QUERY_SIZE,
            400,
            theme.blocks.frontmatter_fg,
        );
    } else {
        painter.text(
            left,
            top + TEXT_DROP,
            &shown,
            CODE_FAMILY,
            QUERY_SIZE,
            400,
            ui.overlay_fg,
        );
    }
    if focused && caret_lit {
        let caret_x = left + x_of(field.caret()) + 1.0;
        painter.line(caret_x, top + 1.0, caret_x, top + 19.0, 1.0, ui.overlay_fg);
    }
    // Every boundary from the drawn window's start, the hidden ones
    // placed beyond the drawn text on the side where the text goes on:
    // a click at the box's edge reaches them one at a time, and the
    // window follows the caret. A text reading right to left runs from
    // the right, so the characters after the window continue left of
    // it, and the ones before it stand right of it. The direction is
    // the drawn line's own, read off its boundaries, which fall from
    // the right when the shaper laid it right to left: the first strong
    // character says otherwise for Arabic digits (numbers to the
    // shaper), for Thaana or Syriac (right to left to the shaper, not
    // to the face routing) and for a direction mark.
    let rtl = positions
        .first()
        .zip(positions.last())
        .is_some_and(|(first, last)| first > last);
    let side = if rtl { -1.0 } else { 1.0 };
    // A hidden side is shaped once, for the width of each of its
    // characters, and its boundaries go outward from the window's edge
    // one character at a time, each a character's width farther than
    // the one before. The side's own order on a line means nothing to
    // the click, since it is not drawn: read as positions, a side that
    // mixes directions folds back on itself, and a boundary of it can
    // tie a drawn one and take the click.
    let advances = |painter: &mut Painter, slice: &str, from_end: bool| -> Vec<f32> {
        let widths = painter.advances(slice, CODE_FAMILY, QUERY_SIZE, 400);
        let mut sum = 0.0;
        let mut out = Vec::with_capacity(widths.len() + 1);
        out.push(0.0);
        if from_end {
            for width in widths.iter().rev() {
                sum += width;
                out.push(sum);
            }
            out.reverse();
        } else {
            for width in &widths {
                sum += width;
                out.push(sum);
            }
        }
        out
    };
    let after = advances(painter, &text[window.end..], false);
    let before = advances(painter, &text[..window.start], true);
    let end_index = text[..window.end].chars().count();
    let offsets = field
        .boundaries()
        .enumerate()
        .map(|(index, at)| {
            if at > window.end {
                let advance = after.get(index - end_index).copied().unwrap_or(0.0);
                x_of(window.end) + side * advance
            } else if at >= window.start {
                x_of(at)
            } else {
                let advance = before.get(index).copied().unwrap_or(0.0);
                x_of(window.start) - side * advance
            }
        })
        .collect();
    FieldView {
        left,
        offsets,
        window,
    }
}

/// Whether a point in logical window coordinates lands on the regex
/// toggle; the geometry mirrors `draw_bar`.
pub fn toggle_hit(width: f32, px: f32, py: f32) -> bool {
    let x = (width - BAR_WIDTH - MARGIN).max(MARGIN);
    let toggle_x = x + BAR_WIDTH - 10.0 - TOGGLE_W;
    let toggle_y = MARGIN + (BAR_HEIGHT - TOGGLE_H) / 2.0;
    (toggle_x..toggle_x + TOGGLE_W).contains(&px) && (toggle_y..toggle_y + TOGGLE_H).contains(&py)
}

/// The slice of a long query that gets drawn, as a byte range that always
/// contains the caret, and whether characters were cut from the left.
/// The window keeps the start it had at the last frame while the caret
/// stays inside it, as a one-line field does: a click on a drawn letter
/// does not shift the text, and a letter typed in the middle stays in
/// view. It never starts past the tail, the first start that shows the
/// text's end, so a text that shrinks (BackSpace at its end, the undo
/// of a paste) or a field that widens fills the field again. When the
/// caret leaves the window, the window slides by the least: to the
/// caret on the left, to the first start that shows the caret on the
/// right, which for a caret at the end is the tail. A longer slice is
/// wider, so the tail, the end of a window and a slide to the right are
/// binary searches over the character boundaries: a dozen measures each
/// for a query of thousands of characters, where one per character cost
/// the square of the length at every frame. A longer slice can also draw
/// narrower (a ligature, a letter's form), which a binary search does
/// not expect, so the window's end is never short of the caret whatever
/// the searches found.
fn window_fit(
    painter: &mut Painter,
    query: &str,
    caret: usize,
    avail: f32,
    previous_start: usize,
) -> (Range<usize>, bool) {
    let fits = |painter: &mut Painter, start: usize, end: usize| {
        let slice = &query[start..end];
        let width = if start > 0 {
            painter.measure(&format!("\u{2026}{slice}"), CODE_FAMILY, QUERY_SIZE, 400)
        } else {
            painter.measure(slice, CODE_FAMILY, QUERY_SIZE, 400)
        };
        width <= avail
    };
    if fits(painter, 0, query.len()) {
        return (0..query.len(), false);
    }
    let starts: Vec<usize> = query.char_indices().map(|(offset, _)| offset).collect();
    let tail = {
        let (mut lo, mut hi) = (0, starts.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if fits(painter, starts[mid], query.len()) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        starts.get(lo).copied().unwrap_or(query.len())
    };
    // The last frame's start, on the text as it is now.
    let mut start = previous_start.min(query.len());
    while !query.is_char_boundary(start) {
        start -= 1;
    }
    let mut start = start.min(tail);
    if caret < start {
        start = caret;
    }
    // The end a window reaches from its start: the last boundary whose
    // slice fits.
    let end_from = |painter: &mut Painter, start: usize| -> usize {
        let ends: Vec<usize> = query[start..]
            .char_indices()
            .map(|(offset, c)| start + offset + c.len_utf8())
            .collect();
        let (mut lo, mut hi) = (0, ends.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if fits(painter, start, ends[mid]) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            start
        } else {
            ends[lo - 1]
        }
    };
    let mut end = end_from(painter, start);
    if caret > end {
        // The caret right of the window: the first start that shows it.
        let from = starts.partition_point(|s| *s <= start);
        let to = starts.partition_point(|s| *s < caret);
        let slide = &starts[from..to];
        let (mut lo, mut hi) = (0, slide.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if fits(painter, slide[mid], caret) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        start = slide.get(lo).copied().unwrap_or(caret);
        end = end_from(painter, start);
    }
    (start..end.max(caret), start > 0)
}

/// A query with any capital letter matches exactly; an all-lowercase
/// query matches case-insensitively.
pub fn smart_case_sensitive(query: &str) -> bool {
    query.chars().any(|c| c.is_uppercase())
}

/// Lowercase fold for matching. A multi-character expansion keeps only
/// its first character so haystack and needle stay aligned one to one.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// One run of addressable text: `text[range]` maps linearly into the
/// span's text from its start.
struct Seg {
    range: Range<usize>,
    block: usize,
    span: usize,
}

/// The document's display text flattened into one string, walked from
/// the model through the same pieces the copies use, beside the table
/// mapping its addressable bytes back to model positions. Separators,
/// joiners and labels contribute their characters but no segment, so a
/// range covering them alone converts to no selection.
struct Haystack {
    text: String,
    segs: Vec<Seg>,
    /// Where each block separator stands in `text`, in order: the
    /// regex walks the text between two of them as one block.
    seps: Vec<usize>,
}

impl Haystack {
    fn build(doc: &Document) -> Haystack {
        let mut hay = Haystack {
            text: String::new(),
            segs: Vec::new(),
            seps: Vec::new(),
        };
        for index in 0..doc.blocks.len() {
            let mut opened = false;
            for piece in block_pieces(doc, index) {
                match piece {
                    Piece::Sep(text) => hay.push_inert(&mut opened, text),
                    Piece::Label(text) => hay.push_inert(&mut opened, &text),
                    Piece::Addr { span, text } => {
                        // An empty piece (an empty code line, an empty
                        // cell) keeps a zero-length segment, so the
                        // joiners around it stand between two segments
                        // and a match made of them maps exactly.
                        hay.open(&mut opened);
                        hay.segs.push(Seg {
                            range: hay.text.len()..hay.text.len() + text.len(),
                            block: index,
                            span,
                        });
                        hay.text.push_str(&text);
                    }
                }
            }
        }
        hay
    }

    /// Plants the block boundary ahead of a block's first character:
    /// newline, separator, newline, so `^` and `$` treat every block as
    /// its own line, and the regex walk stops each block at its
    /// separator.
    fn open(&mut self, opened: &mut bool) {
        if !*opened {
            if !self.text.is_empty() {
                self.text.push('\n');
                self.seps.push(self.text.len());
                self.text.push(BLOCK_SEP);
                self.text.push('\n');
            }
            *opened = true;
        }
    }

    /// The stretch of `text` each block takes, from just past the
    /// separator before it to the separator after it, in order.
    fn blocks(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let starts =
            std::iter::once(0).chain(self.seps.iter().map(|&at| at + BLOCK_SEP.len_utf8()));
        let ends = self
            .seps
            .iter()
            .copied()
            .chain(std::iter::once(self.text.len()));
        starts.zip(ends).map(|(start, end)| start..end)
    }

    fn push_inert(&mut self, opened: &mut bool, text: &str) {
        if text.is_empty() {
            return;
        }
        self.open(opened);
        self.text.push_str(text);
    }

    /// The selection a byte range of `text` converts to, clipped to the
    /// addressable characters inside it. An empty range, a range covering
    /// the block separator, or one holding neither an addressable byte
    /// nor a line joiner converts to nothing.
    ///
    /// A line joiner is addressed through its neighbors: a range starting
    /// on the `\n` right after a segment starts at that segment's end, and
    /// one ending right after a `\n` that a segment follows ends at that
    /// segment's start. So a match made of line breaks alone selects from
    /// the end of one piece to the start of the next, and never across
    /// blocks, since the `\n` beside the separator has a segment on one
    /// side only and the two ends meet.
    fn selection(&self, range: Range<usize>) -> Option<Selection> {
        if range.is_empty() || self.text[range.clone()].contains(BLOCK_SEP) {
            return None;
        }
        let bytes = self.text.as_bytes();
        let at = |seg: &Seg, byte: usize| ModelPos {
            block: seg.block,
            span: seg.span,
            byte,
        };
        let first = self
            .segs
            .partition_point(|seg| seg.range.end <= range.start);
        let start = match first.checked_sub(1).map(|i| &self.segs[i]) {
            Some(before) if bytes[range.start] == b'\n' && before.range.end == range.start => {
                at(before, before.range.len())
            }
            _ => {
                let head = self.segs.get(first)?;
                at(head, range.start.saturating_sub(head.range.start))
            }
        };
        let last = self.segs.partition_point(|seg| seg.range.start < range.end);
        let end = match self.segs.get(last) {
            Some(after) if bytes[range.end - 1] == b'\n' && after.range.start == range.end => {
                at(after, 0)
            }
            _ => {
                let tail = &self.segs[last.checked_sub(1)?];
                at(tail, range.end.min(tail.range.end) - tail.range.start)
            }
        };
        (start < end).then_some(Selection { start, end })
    }
}

/// Every match in document order, found in the model's display text, so
/// matching needs no layout. Display joiners (tabs between table cells,
/// newlines between rows and code lines) keep a phrase from matching
/// across them, and blocks never join. Empty query, no matches.
pub fn matches(doc: &Document, query: &str) -> Vec<Selection> {
    if query.is_empty() {
        return Vec::new();
    }
    if let Some(lines) = sole_code_lines(doc, query) {
        return if smart_case_sensitive(query) {
            let finder = memchr::memmem::Finder::new(query);
            line_matches(doc, lines, whole_lines(lines), 0, |line, _, out| {
                exact_in_line(line, &finder, out)
            })
        } else {
            let needle: String = query.chars().map(fold).collect();
            line_matches(doc, lines, whole_lines(lines), 0, |line, _, out| {
                folded_in_line(line, query, &needle, out)
            })
        };
    }
    matches_by_haystack(doc, query)
}

/// `matches` through the flattened text of the page.
fn matches_by_haystack(doc: &Document, query: &str) -> Vec<Selection> {
    let hay = Haystack::build(doc);
    let found = if smart_case_sensitive(query) {
        find_exact(&hay.text, query)
    } else {
        find_folded(&hay.text, query)
    };
    found
        .into_iter()
        .filter_map(|range| hay.selection(range))
        .collect()
}

/// Every whole-word occurrence of `word`, exact case, in document
/// order: a match counts when the characters on both sides of it are
/// not word characters, so `count` is found in `count + 1` and not in
/// `recount` or `count_all`.
pub fn word_matches(doc: &Document, word: &str) -> Vec<Selection> {
    if word.is_empty() {
        return Vec::new();
    }
    if let Some(lines) = sole_code_lines(doc, word) {
        return words_in_lines(doc, lines, word, whole_lines(lines));
    }
    word_matches_by_haystack(doc, word)
}

/// The start of a line: the line's index and the count of its first
/// bytes. The lines a layout holds are such starts, a line of megabytes
/// being placed from its first byte on, a chunk at a time.
pub type LineStart = (usize, usize);

/// Each line of a block, whole.
fn whole_lines(lines: &CodeBody) -> impl Iterator<Item = LineStart> {
    (0..lines.len()).map(|line| (line, usize::MAX))
}

/// `word_matches` in the line starts `starts` of a document that is one
/// code block: the occurrences that begin in those bytes, which are the
/// ones a search of the whole file finds there. None in any other
/// document, and none for a word with a line break.
pub fn word_matches_in(doc: &Document, word: &str, starts: &[LineStart]) -> Vec<Selection> {
    match sole_code_lines(doc, word) {
        Some(body) if !word.is_empty() => words_in_lines(doc, body, word, starts.iter().copied()),
        _ => Vec::new(),
    }
}

/// `text_matches` in the line starts `starts` of a document that is one
/// code block. None in any other document, and none for a text with a
/// line break.
pub fn text_matches_in(doc: &Document, text: &str, starts: &[LineStart]) -> Vec<Selection> {
    match sole_code_lines(doc, text) {
        Some(body) if !text.is_empty() => {
            let finder = memchr::memmem::Finder::new(text);
            line_matches(
                doc,
                body,
                starts.iter().copied(),
                text.len() - 1,
                |line, end, out| exact_in_line(&line[..end], &finder, out),
            )
        }
        _ => Vec::new(),
    }
}

/// The whole-word occurrences of `word` in the line starts `within` of
/// a one-block document.
fn words_in_lines(
    doc: &Document,
    lines: &CodeBody,
    word: &str,
    within: impl Iterator<Item = LineStart>,
) -> Vec<Selection> {
    let finder = memchr::memmem::Finder::new(word);
    line_matches(doc, lines, within, word.len() - 1, |line, end, out| {
        let from = out.len();
        exact_in_line(&line[..end], &finder, out);
        // A line's neighbour is a line break, never a word character.
        let mut kept = from;
        for i in from..out.len() {
            let range = out[i].clone();
            let before = line[..range.start].chars().next_back();
            let after = line[range.end..].chars().next();
            if !before.is_some_and(word_char) && !after.is_some_and(word_char) {
                out[kept] = range;
                kept += 1;
            }
        }
        out.truncate(kept);
    })
}

fn word_matches_by_haystack(doc: &Document, word: &str) -> Vec<Selection> {
    let hay = Haystack::build(doc);
    find_exact(&hay.text, word)
        .into_iter()
        .filter(|range| {
            let before = hay.text[..range.start].chars().next_back();
            let after = hay.text[range.end..].chars().next();
            !before.is_some_and(word_char) && !after.is_some_and(word_char)
        })
        .filter_map(|range| hay.selection(range))
        .collect()
}

/// The exact occurrences of a text, wherever it appears, partial words
/// included and the case as written: what a selected text lights.
/// `word_matches` is the whole-word form, what a double-clicked word
/// lights.
pub fn text_matches(doc: &Document, text: &str) -> Vec<Selection> {
    if text.is_empty() {
        return Vec::new();
    }
    if let Some(lines) = sole_code_lines(doc, text) {
        let finder = memchr::memmem::Finder::new(text);
        return line_matches(doc, lines, whole_lines(lines), 0, |line, _, out| {
            exact_in_line(line, &finder, out)
        });
    }
    text_matches_by_haystack(doc, text)
}

fn text_matches_by_haystack(doc: &Document, text: &str) -> Vec<Selection> {
    let hay = Haystack::build(doc);
    find_exact(&hay.text, text)
        .into_iter()
        .filter_map(|range| hay.selection(range))
        .collect()
}

/// The lines of a document that is one code block (a code file, a text
/// file, the source view), when `needle` holds no line break. The
/// flattened text of such a document is its lines joined by line
/// breaks, so a needle without one matches inside a line or not at all,
/// and the lines can be searched where they are: no copy of the file,
/// and a match's line is the line it was found in.
fn sole_code_lines<'a>(doc: &'a Document, needle: &str) -> Option<&'a CodeBody> {
    if needle.contains('\n') {
        return None;
    }
    match &doc.blocks[..] {
        [block] => match &block.kind {
            BlockKind::CodeBlock { lines, .. } => Some(lines),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the document is one code block: a code file, a text file,
/// the source view of the editor.
pub fn sole_code_block(doc: &Document) -> bool {
    sole_code_lines(doc, "").is_some()
}

/// The matches `find` reports in each of the line starts `within` of a
/// one-block document, in document order. `find` is given the line and
/// the byte where its search ends, a character boundary `reach` bytes
/// past the start's count or more, so that a match that begins in the
/// start is found whole; it appends the byte ranges of the matches.
fn line_matches(
    doc: &Document,
    lines: &CodeBody,
    within: impl Iterator<Item = LineStart>,
    reach: usize,
    mut find: impl FnMut(&str, usize, &mut Vec<Range<usize>>),
) -> Vec<Selection> {
    let mut out = Vec::new();
    let mut found: Vec<Range<usize>> = Vec::new();
    for (span, count) in within {
        if span >= lines.len() {
            continue;
        }
        let line = lines.line(&doc.source, span);
        if line.is_empty() {
            continue;
        }
        let mut end = count.saturating_add(reach).min(line.len());
        while !line.is_char_boundary(end) {
            end += 1;
        }
        found.clear();
        find(line, end, &mut found);
        out.extend(found.iter().map(|range| Selection {
            start: ModelPos {
                block: 0,
                span,
                byte: range.start,
            },
            end: ModelPos {
                block: 0,
                span,
                byte: range.end,
            },
        }));
    }
    out
}

/// `find_exact` on one line, appended to `out`: the occurrences in
/// order, the scan resuming at each match's end.
fn exact_in_line(line: &str, query: &memchr::memmem::Finder, out: &mut Vec<Range<usize>>) {
    let len = query.needle().len();
    out.extend(
        query
            .find_iter(line.as_bytes())
            .map(|start| start..start + len),
    );
}

/// `find_folded` on one line, appended to `out`. A line of ASCII folds
/// byte for byte, so it is compared where it is; any other line goes
/// through the fold and its offset table.
fn folded_in_line(line: &str, query: &str, needle: &str, out: &mut Vec<Range<usize>>) {
    if !line.is_ascii() {
        out.extend(find_folded(line, query));
        return;
    }
    // A folded character outside ASCII is in no ASCII line.
    if !needle.is_ascii() {
        return;
    }
    let (hay, needle) = (line.as_bytes(), needle.as_bytes());
    let Some((&first, rest)) = needle.split_first() else {
        return;
    };
    // The folded needle is lowercase: its first byte stands in the
    // line as itself or as its capital.
    let capital = first.to_ascii_uppercase();
    let mut at = 0;
    while at + needle.len() <= hay.len() {
        let Some(step) = memchr::memchr2(first, capital, &hay[at..]) else {
            return;
        };
        at += step;
        if at + needle.len() > hay.len() {
            return;
        }
        if hay[at + 1..at + needle.len()]
            .iter()
            .zip(rest)
            .all(|(h, n)| h.to_ascii_lowercase() == *n)
        {
            out.push(at..at + needle.len());
            at += needle.len();
        } else {
            at += 1;
        }
    }
}

/// Every regex match in document order, or `None` when the pattern does
/// not compile or exceeds the engine's backtracking limit, which the bar
/// reports as an invalid pattern. `^` and `$` match at line starts and
/// ends. Case follows smart case read from the pattern's literal
/// characters. Each block is searched on its own (`walk_blocks`), so a
/// match never crosses blocks.
pub fn regex_matches(doc: &Document, pattern: &str) -> Option<Vec<Selection>> {
    if pattern.is_empty() {
        return Some(Vec::new());
    }
    let regex = compile(pattern)?;
    let hay = Haystack::build(doc);
    let found = walk_blocks(&hay, |text, pos| {
        let found = regex.find_from_pos(text, pos).ok()?;
        Some(found.map(|m| (m.start()..m.end(), ())))
    })?;
    Some(found.into_iter().map(|(selection, ())| selection).collect())
}

/// The regex walk over a page, one block at a time: the pattern runs on
/// the text up to the block's separator, so no match reaches into the
/// next block and the page is read once. The text before the block
/// stays in view for a lookbehind. `find` answers the first match from
/// a position, beside what the caller keeps of it, or `None` when the
/// engine fails. A match that converts to no selection (an empty one,
/// or the line breaks beside a separator alone) resumes one character
/// on, so the real matches it covered are still found.
fn walk_blocks<T>(
    hay: &Haystack,
    mut find: impl FnMut(&str, usize) -> Option<Option<(Range<usize>, T)>>,
) -> Option<Vec<(Selection, T)>> {
    let mut out = Vec::new();
    for block in hay.blocks() {
        let text = &hay.text[..block.end];
        let mut pos = block.start;
        while pos <= block.end {
            let Some((found, kept)) = find(text, pos)? else {
                break;
            };
            match hay.selection(found.clone()) {
                Some(selection) => {
                    out.push((selection, kept));
                    pos = found.end;
                }
                None => pos = next_boundary(text, found.start),
            }
        }
    }
    Some(out)
}

/// The next character boundary past `at`, or past the end so a caller's
/// scan terminates.
fn next_boundary(text: &str, at: usize) -> usize {
    text[at..]
        .chars()
        .next()
        .map_or(text.len() + 1, |c| at + c.len_utf8())
}

/// Every regex match beside its expanded replacement: `$1`, `$0` and
/// `${name}` read the match's own captures and `$$` is a literal dollar.
/// The selections equal `regex_matches` for the same pattern, index for
/// index, since both take the walk of `walk_blocks`, and `None` reports
/// the same failures.
pub fn regex_replacements(
    doc: &Document,
    pattern: &str,
    template: &str,
) -> Option<Vec<(Selection, String)>> {
    if pattern.is_empty() {
        return Some(Vec::new());
    }
    let regex = compile(pattern)?;
    let hay = Haystack::build(doc);
    let template = unescape_template(template);
    let expander = fancy_regex::Expander::default();
    walk_blocks(&hay, |text, pos| {
        let captures = regex.captures_from_pos(text, pos).ok()?;
        Some(captures.map(|captures| {
            let found = captures.get(0).expect("a match always captures group 0");
            (
                found.start()..found.end(),
                expander.expansion(&template, &captures),
            )
        }))
    })
}

/// The replacement with its escapes read, the way VS Code's replace box
/// reads them in regex mode: `\n` a line break, `\t` a tab, `\\` one
/// backslash. Any other backslash sequence stays as typed, and a
/// backslash ending the template keeps itself. The group syntax is
/// dollar-based, so this pass and the expander never meet.
fn unescape_template(template: &str) -> Cow<'_, str> {
    if !template.contains('\\') {
        return Cow::Borrowed(template);
    }
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Cow::Owned(out)
}

/// The pattern with the standing flags: anchors match per line, and
/// smart case adds insensitivity.
fn compile(pattern: &str) -> Option<fancy_regex::Regex> {
    let flags = if regex_case_sensitive(pattern) {
        "(?m)"
    } else {
        "(?mi)"
    };
    fancy_regex::Regex::new(&format!("{flags}{pattern}")).ok()
}

/// Smart case for a pattern: a capital letter typed literally makes it
/// case-sensitive; the letter of an escape like `\W` does not count.
fn regex_case_sensitive(pattern: &str) -> bool {
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

/// Byte ranges of every occurrence in order, non-overlapping: the scan
/// resumes at each match's end. An empty needle finds nothing, which
/// also keeps the scan from standing still.
fn find_exact(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(query) {
        let start = from + at;
        out.push(start..start + query.len());
        from = start + query.len();
    }
    out
}

/// Case-insensitive occurrences: haystack and needle fold to the same
/// alphabet, and each match maps back through the per-character offset
/// table, since folding can change a character's byte length.
fn find_folded(text: &str, query: &str) -> Vec<Range<usize>> {
    let needle: String = query.chars().map(fold).collect();
    let mut folded = String::with_capacity(text.len());
    let mut map: Vec<(usize, usize)> = Vec::new();
    for (at, c) in text.char_indices() {
        map.push((folded.len(), at));
        folded.push(fold(c));
    }
    map.push((folded.len(), text.len()));
    let seat = |at: usize| map[map.partition_point(|&(f, _)| f < at)].1;
    find_exact(&folded, &needle)
        .into_iter()
        .map(|range| seat(range.start)..seat(range.end))
        .collect()
}

/// The neighboring match index with wraparound in either direction.
pub fn step(current: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::markdown;
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use crate::ui::selection::ModelPos;
    use tiny_skia::Pixmap;

    /// The field that has the keys paints its caret on the lit half of
    /// the blink only: the dark half, and a field without the keys, leave
    /// the pixels of the text alone, and the lit half differs from them
    /// by the caret's column only.
    #[test]
    fn a_field_paints_its_caret_on_the_lit_half_only() {
        let field = TextField::new("abc".to_string());
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let mut drawn = |focused: bool, caret_lit: bool| {
            let mut pixmap = Pixmap::new(200, 40).unwrap();
            let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
            draw_field(
                &mut painter,
                &theme,
                &field,
                10.0,
                10.0,
                160.0,
                "",
                focused,
                caret_lit,
                0,
            );
            drop(painter);
            pixmap
        };
        let lit = drawn(true, true);
        let dark = drawn(true, false);
        let unfocused = drawn(false, true);
        let columns: Vec<u32> = (0..200)
            .filter(|&x| (0..40).any(|y| lit.pixel(x, y) != dark.pixel(x, y)))
            .collect();
        assert!(
            !columns.is_empty() && columns.len() <= 3,
            "the caret alone differs: {columns:?}"
        );
        assert!(
            dark.data() == unfocused.data(),
            "a field without the keys paints no caret"
        );
    }

    /// A query longer than the pill, the caret at its start: the drawn
    /// window stops short of the text's end, and the boundaries beyond
    /// it keep growing, so a click right of the pill reaches them.
    #[test]
    fn the_offsets_beyond_the_drawn_window_keep_growing() {
        let query: String = "abcdefghij".repeat(6);
        let mut field = TextField::new(query);
        field.set_caret(0);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            120.0,
            "",
            true,
            true,
            0,
        );
        assert_eq!(view.offsets.len(), 61);
        assert!(
            view.offsets.windows(2).all(|w| w[0] < w[1]),
            "every boundary stands right of the one before: {:?}",
            view.offsets
        );
    }

    /// A long right-to-left query, the caret at its start: the drawn
    /// text runs from the right, and the characters after the window
    /// stand left of it, so a click on a drawn letter reaches that
    /// letter and not a hidden one.
    #[test]
    fn a_click_on_a_drawn_letter_of_a_right_to_left_query_reaches_it() {
        let query = "الباب الثاني في العمران البدوي و الأمم الوحشية و القبائل";
        let mut field = TextField::new(query.to_string());
        field.set_caret(0);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            120.0,
            "",
            true,
            true,
            0,
        );
        let (window, cut) = window_fit(&mut painter, query, 0, 120.0, 0);
        assert!(!cut && window.start == 0 && window.end < query.len());
        let boundaries: Vec<usize> = field.boundaries().collect();
        for (index, at) in boundaries.iter().enumerate() {
            if *at > window.end {
                break;
            }
            let x = view.offsets[index];
            assert_eq!(
                field.caret_at(x, &view.offsets),
                *at,
                "the click at x {x} of byte {at} lands on it"
            );
        }
        let drawn_min = (0..=boundaries.partition_point(|b| *b <= window.end) - 1)
            .map(|i| view.offsets[i])
            .fold(f32::MAX, f32::min);
        for (index, at) in boundaries.iter().enumerate() {
            if *at > window.end {
                assert!(
                    view.offsets[index] < drawn_min,
                    "byte {at} stands left of the drawn text"
                );
            }
        }
    }

    /// A 5,000-character query costs a few shapings a frame, not one
    /// per character: the bound is wide (a debug build), and the cost
    /// of the old way was far over it.
    #[test]
    fn a_long_query_draws_in_a_few_shapings() {
        let query: String = "lorem ipsum dolor sit amet ".repeat(185);
        let mut field = TextField::new(query);
        field.set_caret(0);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        painter.measure("warm", CODE_FAMILY, QUERY_SIZE, 400);
        let started = std::time::Instant::now();
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            300.0,
            "",
            true,
            true,
            0,
        );
        let took = started.elapsed();
        assert!(view.offsets.windows(2).all(|w| w[0] < w[1]));
        assert!(took.as_secs_f32() < 2.0, "one frame took {took:?}");
    }

    /// The drawn window keeps its place while the caret moves inside
    /// it, as a one-line field does: a click on a drawn letter does not
    /// shift the text, and a letter typed in the middle stays in view.
    /// When the caret leaves the window, the window slides by the least.
    #[test]
    fn the_window_keeps_its_place_and_slides_by_the_least() {
        let query: String = "abcdefghij".repeat(6);
        let mut field = TextField::new(query);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        let mut draw = |field: &TextField, previous: usize| {
            draw_field(
                &mut painter,
                &theme,
                field,
                0.0,
                0.0,
                120.0,
                "",
                true,
                true,
                previous,
            )
            .window
        };
        let tail = draw(&field, 0);
        assert!(
            tail.start > 0 && tail.end == 60,
            "the caret at the end: the tail, {tail:?}"
        );
        field.set_caret(tail.start + 5);
        let same = draw(&field, tail.start);
        assert_eq!(same, tail, "a click on a drawn letter keeps the window");
        field.insert("q");
        let typed = draw(&field, same.start);
        assert_eq!(
            typed.start, tail.start,
            "a letter typed inside keeps the window's start"
        );
        assert!(
            typed.start <= field.caret() && field.caret() <= typed.end,
            "the typed letter stays in view: {typed:?}, the caret at {}",
            field.caret()
        );
        field.set_caret(tail.start - 1);
        let left = draw(&field, typed.start);
        assert_eq!(
            left.start,
            tail.start - 1,
            "the caret left of the window: it slides to the caret"
        );
        let beyond = left.end + 1;
        field.set_caret(beyond);
        let right = draw(&field, left.start);
        assert!(
            right.start > left.start && right.start < beyond && right.end >= beyond,
            "the caret right of the window: it slides by the least, {left:?} then {right:?} for {beyond}"
        );
    }

    /// BackSpace at the end of a long query: the window slides left so
    /// the field stays full, as a one-line field does, instead of
    /// keeping its start and leaving the right of the field empty.
    #[test]
    fn backspace_at_the_end_of_a_long_query_keeps_the_field_full() {
        use winit::keyboard::{Key, NamedKey};
        let query: String = "abcdefghij".repeat(6);
        let mut field = TextField::new(query);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        let mut draw = |field: &TextField, previous: usize| {
            draw_field(
                &mut painter,
                &theme,
                field,
                0.0,
                0.0,
                120.0,
                "",
                true,
                true,
                previous,
            )
            .window
        };
        let tail = draw(&field, 0);
        assert!(tail.start > 0 && tail.end == 60, "the tail first: {tail:?}");
        field.key(&Key::Named(NamedKey::Backspace), false, false);
        let after = draw(&field, tail.start);
        let full = draw(&field, 0);
        assert_eq!(
            full.end, 59,
            "a fresh draw shows the tail of the shorter text"
        );
        assert_eq!(
            after, full,
            "after BackSpace the kept window is the tail of the shorter text"
        );
    }

    /// The field's font draws some longer slices narrower (the probe of
    /// 19/10/2026 on this title: the first four characters are 17 wide,
    /// the first three 24), and the binary searches of the window
    /// assume a slice grows with each character. Whatever they answer,
    /// the window holds the caret: for every caret of the title, at
    /// five widths of the field (a sweep of every width in steps of
    /// five found the caret outside at 23 of them before the fix),
    /// with the window kept from the frame before and from a fresh
    /// draw. Shaping Arabic is slow in a debug build, hence the few
    /// widths.
    #[test]
    fn the_window_contains_the_caret_where_a_longer_slice_is_narrower() {
        let query = "الباب الثاني في العمران البدوي و الأمم الوحشية و القبائل";
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let ends: Vec<usize> = query
            .char_indices()
            .map(|(at, c)| at + c.len_utf8())
            .collect();
        let widths: Vec<f32> = ends
            .iter()
            .map(|&end| painter.measure(&query[..end], CODE_FAMILY, QUERY_SIZE, 400))
            .collect();
        assert!(
            widths.windows(2).any(|w| w[1] < w[0]),
            "the premise: a longer slice draws narrower somewhere: {widths:?}"
        );
        let carets: Vec<usize> = std::iter::once(0).chain(ends.iter().copied()).collect();
        let mut outside = Vec::new();
        for avail in (80..=240).step_by(40) {
            let mut previous = 0;
            for &caret in &carets {
                let (kept, _) = window_fit(&mut painter, query, caret, avail as f32, previous);
                if !(kept.start <= caret && caret <= kept.end) {
                    outside.push((avail, caret, "kept", kept.clone()));
                }
                previous = kept.start;
                let (fresh, _) = window_fit(&mut painter, query, caret, avail as f32, 0);
                if !(fresh.start <= caret && caret <= fresh.end) {
                    outside.push((avail, caret, "fresh", fresh));
                }
            }
        }
        assert!(
            outside.is_empty(),
            "the caret outside the window: {outside:?}"
        );
    }

    /// Arabic-Indic digits belong to the Arabic block, but the shaper
    /// reads them as numbers, not as a strong script, and draws a query
    /// of them left to right. The hidden boundaries follow the drawn
    /// line: they stand beyond its right end, not inside it.
    #[test]
    fn a_query_of_arabic_digits_places_its_hidden_boundaries_beyond_the_drawn_text() {
        let query: String = "٠١٢٣٤٥٦٧٨٩".repeat(6);
        let mut field = TextField::new(query.clone());
        field.set_caret(0);
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            120.0,
            "",
            true,
            true,
            0,
        );
        let (window, cut) = window_fit(&mut painter, &query, 0, 120.0, 0);
        assert!(!cut && window.start == 0 && window.end < query.len());
        let boundaries: Vec<usize> = field.boundaries().collect();
        let drawn = boundaries.partition_point(|b| *b <= window.end);
        assert!(
            view.offsets[..drawn].windows(2).all(|w| w[0] < w[1]),
            "the premise: the digits are drawn left to right: {:?}",
            &view.offsets[..drawn]
        );
        let drawn_max = view.offsets[..drawn]
            .iter()
            .fold(f32::MIN, |m, x| m.max(*x));
        for (index, at) in boundaries.iter().enumerate().skip(drawn) {
            assert!(
                view.offsets[index] > drawn_max,
                "byte {at} stands right of the drawn text: {} against {drawn_max}",
                view.offsets[index]
            );
        }
    }

    /// A hidden side that mixes directions is not drawn, so its own
    /// order on a line means nothing to the click: its boundaries go
    /// outward from the window's edge one character at a time, each
    /// farther than the one before, on the side after the window and
    /// on the side before it.
    #[test]
    fn the_hidden_boundaries_of_a_mixed_side_grow_with_each_character() {
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        let theme = Theme::default_dark();
        // The side after the window: the caret at the start.
        let query = format!("{}abc مقدمة def ghi", "abcdefghij".repeat(3));
        let mut field = TextField::new(query.clone());
        field.set_caret(0);
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            120.0,
            "",
            true,
            true,
            0,
        );
        let (window, _) = window_fit(&mut painter, &query, 0, 120.0, 0);
        assert!(
            window.end < 30,
            "the window stops in the Latin head: {window:?}"
        );
        let boundaries: Vec<usize> = field.boundaries().collect();
        let drawn = boundaries.partition_point(|b| *b <= window.end);
        let after = &view.offsets[drawn - 1..];
        assert!(
            after.windows(2).all(|w| w[0] < w[1]),
            "each boundary after the window stands right of the one before: {after:?}"
        );
        // The side before the window: the caret at the end.
        let query = format!("abc مقدمة def {}", "abcdefghij".repeat(4));
        let field = TextField::new(query.clone());
        let view = draw_field(
            &mut painter,
            &theme,
            &field,
            0.0,
            0.0,
            120.0,
            "",
            true,
            true,
            0,
        );
        let (window, _) = window_fit(&mut painter, &query, query.len(), 120.0, 0);
        assert!(
            window.start > 20,
            "the window is the Latin tail: {window:?}"
        );
        let boundaries: Vec<usize> = field.boundaries().collect();
        let first_drawn = boundaries.partition_point(|b| *b < window.start);
        let before = &view.offsets[..=first_drawn];
        assert!(
            before.windows(2).all(|w| w[0] < w[1]),
            "each boundary before the window stands left of the one after it: {before:?}"
        );
    }

    #[test]
    fn matches_reach_inside_closed_details() {
        let doc = markdown::parse(
            "<details>\n<summary>S</summary>\n\nthe needle hides here\n\n</details>",
        );
        assert!(!doc.block_visible(1), "the paragraph is folded away");
        assert_eq!(matches(&doc, "needle").len(), 1);
    }

    #[test]
    fn word_matches_are_whole_words_in_exact_case() {
        let doc = markdown::parse("cat catalog Cat cat_ (cat) concat cat.\n\n- a cat\n\n`cat`");
        let found = word_matches(&doc, "cat");
        let bytes: Vec<(usize, usize)> = found
            .iter()
            .map(|m| (m.start.block, m.start.byte))
            .collect();
        assert_eq!(found.len(), 5, "{bytes:?}");
        assert_eq!(bytes[..3], [(0, 0), (0, 22), (0, 34)]);
        assert!(word_matches(&doc, "").is_empty());
    }

    #[test]
    fn word_matches_read_letters_of_any_script_as_word_characters() {
        let doc = markdown::parse("été répété été, l'été");
        assert_eq!(word_matches(&doc, "été").len(), 3);
    }

    #[test]
    fn lowercase_query_matches_any_case() {
        assert!(!smart_case_sensitive("panel"));
        assert!(!smart_case_sensitive("3/17"));
        let doc = markdown::parse("Panel PANEL panel");
        assert_eq!(matches(&doc, "panel").len(), 3);
    }

    #[test]
    fn capital_query_matches_exactly() {
        assert!(smart_case_sensitive("Panel"));
        let doc = markdown::parse("Panel PANEL panel");
        let found = matches(&doc, "Panel");
        assert_eq!(
            found,
            vec![Selection {
                start: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 0
                },
                end: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 5
                },
            }]
        );
    }

    #[test]
    fn match_crosses_a_style_boundary() {
        let doc = markdown::parse("**pan**el rest");
        let found = matches(&doc, "panel");
        assert_eq!(
            found,
            vec![Selection {
                start: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 0
                },
                end: ModelPos {
                    block: 0,
                    span: 1,
                    byte: 2
                },
            }]
        );
    }

    #[test]
    fn no_match_across_blocks() {
        let doc = markdown::parse("one\n\ntwo");
        assert!(matches(&doc, "onetwo").is_empty());
        assert!(matches(&doc, "netw").is_empty());
    }

    #[test]
    fn no_match_across_table_cells() {
        let doc = markdown::parse("| ab | cd |\n|---|---|\n| ef | gh |");
        assert!(matches(&doc, "bc").is_empty());
        assert!(matches(&doc, "fg").is_empty());
        assert_eq!(matches(&doc, "ef").len(), 1);
    }

    #[test]
    fn no_match_across_a_hard_break() {
        let doc = markdown::parse("one two\\\nthree");
        assert!(matches(&doc, "two three").is_empty());
        assert_eq!(matches(&doc, "three").len(), 1);
    }

    #[test]
    fn markers_stay_out_of_the_text() {
        let doc = markdown::parse("- item one");
        let found = matches(&doc, "item");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].start,
            ModelPos {
                block: 0,
                span: 0,
                byte: 0
            },
            "the bullet is not part of the text"
        );
    }

    #[test]
    fn matches_span_code_lines_but_not_line_breaks() {
        let doc = markdown::parse("```rust\nlet alpha = 1;\nlet beta = 2;\n```");
        assert_eq!(matches(&doc, "alpha").len(), 1);
        assert!(matches(&doc, "1;let").is_empty(), "lines never join");
    }

    #[test]
    fn successive_matches_do_not_overlap() {
        let doc = markdown::parse("aaaa");
        assert_eq!(matches(&doc, "aa").len(), 2);
    }

    #[test]
    fn empty_query_finds_nothing() {
        let doc = markdown::parse("anything");
        assert!(matches(&doc, "").is_empty());
    }

    #[test]
    fn step_wraps_both_ways() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 1, true), 0);
    }

    #[test]
    fn regex_finds_a_pattern() {
        let doc = markdown::parse("alpha beta");
        let found = regex_matches(&doc, r"b\w+").expect("valid pattern");
        assert_eq!(
            found,
            vec![Selection {
                start: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 6
                },
                end: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 10
                },
            }]
        );
    }

    /// A regex runs on each block on its own: a pattern that would run
    /// through the whole page matches each paragraph whole, and the
    /// replacements pair with the same selections.
    #[test]
    fn a_regex_matches_each_block_on_its_own() {
        let doc = markdown::parse("alpha one.\n\nbeta two.\n\ngamma three.\n");
        let found = regex_matches(&doc, r"[\s\S]+").expect("valid pattern");
        let spans: Vec<(usize, usize, usize)> = found
            .iter()
            .map(|s| (s.start.block, s.start.byte, s.end.byte))
            .collect();
        assert_eq!(spans, vec![(0, 0, 10), (1, 0, 9), (2, 0, 12)]);
        let paired: Vec<Selection> = regex_replacements(&doc, r"[\s\S]+", "x")
            .expect("valid pattern")
            .into_iter()
            .map(|(selection, _)| selection)
            .collect();
        assert_eq!(paired, found);
    }

    /// The walk is one pass over the page: a pattern that would run
    /// through every paragraph costs one match per paragraph, not a scan
    /// to the end of the page from each of its characters.
    #[test]
    fn a_regex_over_many_blocks_costs_one_pass() {
        const PARAGRAPHS: usize = 500;
        let page: String = (0..PARAGRAPHS)
            .map(|n| format!("Paragraph number {n} of a long page of text.\n\n"))
            .collect();
        let doc = markdown::parse(page.as_str());
        let started = std::time::Instant::now();
        let found = regex_matches(&doc, r"[\s\S]+").expect("valid pattern");
        let took = started.elapsed();
        assert!(
            found.len() == PARAGRAPHS && took < std::time::Duration::from_secs(1),
            "{} matches in {took:?}",
            found.len()
        );
    }

    #[test]
    fn regex_smart_case_reads_literals_only() {
        let doc = markdown::parse("Panel PANEL panel");
        assert_eq!(regex_matches(&doc, "panel").expect("valid").len(), 3);
        assert_eq!(regex_matches(&doc, "Panel").expect("valid").len(), 1);
        let doc = markdown::parse("A B");
        assert_eq!(regex_matches(&doc, r"a\Wb").expect("valid").len(), 1);
    }

    #[test]
    fn regex_zero_width_terminates_and_finds_nothing() {
        let doc = markdown::parse("abc");
        assert!(regex_matches(&doc, "x*").expect("valid").is_empty());
        assert!(regex_matches(&doc, r"\b").expect("valid").is_empty());
    }

    #[test]
    fn regex_never_crosses_blocks() {
        let doc = markdown::parse("one\n\ntwo");
        assert!(regex_matches(&doc, "one.two").expect("valid").is_empty());
    }

    #[test]
    fn regex_crosses_a_joiner_only_when_named() {
        let doc = markdown::parse("```rust\nlet alpha = 1;\nlet beta = 2;\n```");
        assert_eq!(regex_matches(&doc, "1;\nlet").expect("valid").len(), 1);
        assert!(regex_matches(&doc, "1;.let").expect("valid").is_empty());
    }

    #[test]
    fn regex_anchors_match_line_starts() {
        let doc = markdown::parse("```rust\nlet alpha = 1;\nlet beta = 2;\n```");
        assert_eq!(regex_matches(&doc, "^let").expect("valid").len(), 2);
    }

    #[test]
    fn regex_replacements_expand_captures() {
        let doc = markdown::parse("ab cd");
        let pairs = regex_replacements(&doc, r"(\w)(\w)", "$2$1").expect("valid");
        let texts: Vec<&str> = pairs.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(texts, ["ba", "dc"]);
        let sels: Vec<Selection> = pairs.iter().map(|(sel, _)| *sel).collect();
        assert_eq!(
            sels,
            regex_matches(&doc, r"(\w)(\w)").expect("valid"),
            "the pairs mirror the match list index for index"
        );
    }

    #[test]
    fn regex_replacement_dollar_forms() {
        let doc = markdown::parse("cat");
        let pairs = regex_replacements(&doc, "c(a)t", "[$0|$1|$$]").expect("valid");
        assert_eq!(pairs[0].1, "[cat|a|$]");
    }

    #[test]
    fn regex_anchors_match_block_edges() {
        let doc = markdown::parse("off\n\nbeta f\n\ngamma");
        let found = regex_matches(&doc, "^.*f.*$").expect("valid");
        assert_eq!(found.len(), 2, "every block holding an f is a line");
        assert_eq!(found[0].start.block, 0);
        assert_eq!(found[1].start.block, 1);
    }

    #[test]
    fn a_discarded_greedy_match_hides_nothing_behind_it() {
        // The greedy candidate from block one spans the separator and is
        // discarded; the scan must resume inside it, not after it.
        let doc = markdown::parse("bar\n\nfoo");
        assert_eq!(regex_matches(&doc, "[^q]*foo").expect("valid").len(), 1);
        assert_eq!(regex_matches(&doc, "(?s).*foo").expect("valid").len(), 1);
        let pairs = regex_replacements(&doc, "[^q]*foo", "$0").expect("valid");
        assert_eq!(
            pairs.len(),
            1,
            "the pairs walk with the same recovery as the matches"
        );
    }

    #[test]
    fn find_exact_refuses_an_empty_needle() {
        assert!(find_exact("abc", "").is_empty());
    }

    #[test]
    fn regex_invalid_pattern_reports_as_invalid() {
        let doc = markdown::parse("anything");
        assert!(regex_matches(&doc, "foo(").is_none());
    }

    #[test]
    fn regex_backtrack_limit_reports_as_invalid() {
        let doc = markdown::parse("x".repeat(40));
        assert!(regex_matches(&doc, r"(?:(x+)\1)+y").is_none());
    }

    // ---- Line breaks as matches: the joiners between pieces ----

    fn code_doc(text: &str) -> Document {
        crate::doc::load::code_document(None, text)
    }

    /// A code file with what a fold can trip on: capitals, letters
    /// whose lowercase is longer or shorter in bytes, empty lines,
    /// matches at both ends of a line and matches that touch.
    fn mixed_code_text(lines: usize) -> String {
        let mut text = String::new();
        for i in 0..lines {
            match i % 9 {
                0 => text.push_str(&format!("let Count_{i} = count + recount; // COUNT count\n")),
                1 => text.push('\n'),
                2 => text.push_str("caf\u{e9} CAF\u{c9} \u{130}stanbul stra\u{df}e \u{212a}elvin \u{3a3}\u{3c3}\u{3c2} count\n"),
                3 => text.push_str("aaaa AAAA aAaA countcount\n"),
                4 => text.push_str("count\n"),
                5 => text.push_str(&format!("  http://example.org:80/Series/{i}/x-count.mkv  \n")),
                6 => text.push_str("\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} count \u{5e2}\u{5d1}\u{5e8}\u{5d9}\u{5ea}\n"),
                _ => text.push_str(&format!("fn f{i}() {{ call({i}); }}\n")),
            }
        }
        text
    }

    const QUERIES: [&str; 16] = [
        "count",
        "Count",
        "COUNT",
        "aa",
        "a",
        "e",
        " ",
        "caf\u{e9}",
        "\u{e9}",
        "i",
        "k",
        "ss",
        "\u{3c3}",
        "://",
        "x-count.mkv  ",
        "\u{639}\u{631}",
    ];

    #[test]
    fn a_code_file_searched_line_by_line_finds_what_the_flattened_text_finds() {
        for text in [
            mixed_code_text(400),
            "count".to_string(),
            "\n\ncount\n\n".to_string(),
            String::new(),
        ] {
            let doc = code_doc(&text);
            for query in QUERIES {
                assert_eq!(
                    matches(&doc, query),
                    matches_by_haystack(&doc, query),
                    "{query:?}"
                );
                assert_eq!(
                    word_matches(&doc, query),
                    word_matches_by_haystack(&doc, query),
                    "word {query:?}"
                );
                assert_eq!(
                    text_matches(&doc, query),
                    text_matches_by_haystack(&doc, query),
                    "text {query:?}"
                );
            }
        }
    }

    #[test]
    fn a_query_with_a_line_break_goes_through_the_flattened_text() {
        let doc = code_doc("count\ncount\n");
        assert!(sole_code_lines(&doc, "count").is_some());
        assert!(sole_code_lines(&doc, "t\nc").is_none());
        assert_eq!(matches(&doc, "t\nc"), matches_by_haystack(&doc, "t\nc"));
        assert_eq!(matches(&doc, "t\nc").len(), 1);
    }

    fn sel(a: (usize, usize, usize), b: (usize, usize, usize)) -> Selection {
        let pos = |(block, span, byte)| ModelPos { block, span, byte };
        Selection {
            start: pos(a),
            end: pos(b),
        }
    }

    /// The source after every pair is spliced, the way replace all does.
    fn replaced_all(doc: &Document, pattern: &str, template: &str) -> String {
        let pairs = regex_replacements(doc, pattern, template).expect("valid");
        let edits: Vec<(Range<usize>, String)> = pairs
            .into_iter()
            .filter_map(|(sel, text)| {
                crate::edit::caret::selection_range(doc, &sel).map(|range| (range, text))
            })
            .collect();
        let (region, text) = crate::edit::splice::combine(&doc.source, &edits).expect("edits");
        let mut out = doc.source.to_string();
        out.replace_range(region, &text);
        out
    }

    #[test]
    fn a_line_break_matches_between_code_lines() {
        let doc = code_doc("one\ntwo\nthree");
        assert_eq!(
            regex_matches(&doc, "\\n").expect("valid"),
            vec![sel((0, 0, 3), (0, 1, 0)), sel((0, 1, 3), (0, 2, 0))]
        );
    }

    #[test]
    fn line_breaks_around_an_empty_line_match_exactly() {
        let doc = code_doc("one\n\ntwo");
        assert_eq!(
            regex_matches(&doc, "\\n\\n").expect("valid"),
            vec![sel((0, 0, 3), (0, 2, 0))]
        );
        assert_eq!(
            regex_matches(&doc, "\\n").expect("valid"),
            vec![sel((0, 0, 3), (0, 1, 0)), sel((0, 1, 0), (0, 2, 0))]
        );
    }

    #[test]
    fn a_line_break_never_joins_blocks() {
        let doc = markdown::parse("one\n\ntwo");
        assert!(regex_matches(&doc, "\\n").expect("valid").is_empty());
        let doc = markdown::parse("one\n\n```\na\nb\n```\n\ntwo");
        assert_eq!(
            regex_matches(&doc, "\\n").expect("valid"),
            vec![sel((1, 0, 1), (1, 1, 0))],
            "only the break inside the code block"
        );
    }

    #[test]
    fn the_final_line_break_is_not_a_joiner() {
        let doc = code_doc("one\ntwo\n");
        assert_eq!(regex_matches(&doc, "\\n").expect("valid").len(), 1);
    }

    #[test]
    fn line_breaks_join_table_rows_and_tabs_join_nothing() {
        let doc = markdown::parse("| a | b |\n|---|---|\n| c | d |\n| e | f |");
        assert_eq!(regex_matches(&doc, "\\n").expect("valid").len(), 2);
        assert!(regex_matches(&doc, "\\t").expect("valid").is_empty());
        assert!(matches(&doc, "\t").is_empty());
    }

    #[test]
    fn a_hard_break_matches_as_a_line_break() {
        let doc = markdown::parse("one two\\\nthree");
        assert_eq!(regex_matches(&doc, "\\n").expect("valid").len(), 1);
        assert!(matches(&doc, "two three").is_empty());
    }

    #[test]
    fn a_named_line_end_and_a_line_break_agree() {
        let doc = code_doc("go\nno\nyes\nso");
        assert_eq!(regex_matches(&doc, "o$").expect("valid").len(), 3);
        let found = regex_matches(&doc, "o\\n").expect("valid");
        assert_eq!(found.len(), 2, "the last line has no break after it");
        assert_eq!(
            found[0],
            sel((0, 0, 1), (0, 1, 0)),
            "the break itself is inside the match"
        );
    }

    #[test]
    fn a_pasted_newline_finds_the_joiners_too() {
        let doc = code_doc("one\ntwo\nthree");
        assert_eq!(matches(&doc, "\n").len(), 2);
    }

    #[test]
    fn line_break_replacements_mirror_the_matches() {
        let doc = code_doc("one\ntwo\nthree");
        let pairs = regex_replacements(&doc, "\\n", " ").expect("valid");
        let sels: Vec<Selection> = pairs.iter().map(|(s, _)| *s).collect();
        assert_eq!(sels, regex_matches(&doc, "\\n").expect("valid"));
        assert!(pairs.iter().all(|(_, t)| t == " "));
    }

    #[test]
    fn replacing_line_breaks_joins_lines() {
        let doc = code_doc("one\ntwo\nthree\n");
        assert_eq!(
            replaced_all(&doc, "\\n", " "),
            "one two three\n",
            "the terminator stays"
        );
    }

    #[test]
    fn replacing_double_blank_lines_collapses_them() {
        let doc = code_doc("one\n\n\ntwo\n");
        assert_eq!(replaced_all(&doc, "\\n\\n\\n", "\n\n"), "one\n\ntwo\n");
    }

    #[test]
    fn replacing_a_line_break_with_two_doubles_it() {
        let doc = code_doc("one\ntwo\n");
        assert_eq!(replaced_all(&doc, "\\n", "\n\n"), "one\n\ntwo\n");
    }

    #[test]
    fn replacing_a_line_break_keeps_the_other_crlf_endings() {
        // The load normalizes CRLF to LF and remembers where; the ledger
        // puts every untouched ending back on save.
        let doc = code_doc("one\ntwo\nthree\n");
        let first = regex_matches(&doc, "\\n").expect("valid")[0];
        let range = crate::edit::caret::selection_range(&doc, &first).expect("a source range");
        let mut led = crate::edit::splice::Ledger::new(doc.source.clone(), vec![3, 7, 13]);
        led.edit(range, " ");
        assert_eq!(led.emit(), b"one two\r\nthree\r\n");
    }

    // ---- Escapes in the replacement ----

    fn replacement_texts(doc: &Document, pattern: &str, template: &str) -> Vec<String> {
        regex_replacements(doc, pattern, template)
            .expect("valid")
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }

    #[test]
    fn a_tab_and_a_newline_escape_in_the_replacement() {
        let doc = markdown::parse("a b");
        assert_eq!(replacement_texts(&doc, " ", "\\t"), ["\t"]);
        assert_eq!(replacement_texts(&doc, " ", "\\n"), ["\n"]);
        assert_eq!(replacement_texts(&doc, " ", "x\\ty\\nz"), ["x\ty\nz"]);
    }

    #[test]
    fn a_doubled_backslash_is_one_backslash() {
        let doc = markdown::parse("a b");
        assert_eq!(replacement_texts(&doc, " ", "\\\\"), ["\\"]);
        assert_eq!(
            replacement_texts(&doc, " ", "\\\\t"),
            ["\\t"],
            "the escaped backslash does not join the t"
        );
    }

    #[test]
    fn other_escapes_stay_as_typed() {
        let doc = markdown::parse("a b");
        assert_eq!(replacement_texts(&doc, " ", "\\s"), ["\\s"]);
        assert_eq!(replacement_texts(&doc, " ", "\\d"), ["\\d"]);
        assert_eq!(
            replacement_texts(&doc, " ", "end\\"),
            ["end\\"],
            "a lone backslash at the end keeps itself"
        );
    }

    #[test]
    fn a_group_and_an_escape_combine() {
        let doc = markdown::parse("ab cd");
        assert_eq!(
            replacement_texts(&doc, r"(\w)(\w)", "$2\\t$1"),
            ["b\ta", "d\tc"]
        );
        assert_eq!(
            replacement_texts(&doc, r"(\w)(\w)", "$$\\n"),
            ["$\n", "$\n"]
        );
    }
}
