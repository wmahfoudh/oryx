//! Text selection anchored on the document model: hit testing maps a
//! click through the laid-out runs to a model position, highlight
//! geometry maps the model range back onto whatever runs are placed,
//! and both copies assemble from the model alone, so no operation here
//! needs a complete layout.

use std::borrow::Cow;
use std::cmp::Ordering;

use cosmic_text::{Attrs, Buffer, Family, LayoutGlyph, Metrics, Shaping, Style, Weight};

use crate::doc::model::{BlockKind, Document, Span};
use crate::edit::caret;
use crate::layout::{metrics, LayoutDoc, TextRef, TextRun};
use crate::style::fonts::FontStore;

/// Marker runs (bullets, numbers, checkmarks, alert titles) carry this
/// span sentinel and take no part in selection or search.
pub const MARKER_SPAN: usize = usize::MAX;

/// A caret position in the model: block index, the span index inside it
/// (the line index for code blocks, the flattened cell chain for
/// tables), and a byte offset into that span's display text, always on
/// a character boundary. Ordering is document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelPos {
    pub block: usize,
    pub span: usize,
    pub byte: usize,
}

/// A drag selection between two model positions, kept in drag order;
/// `start` may sit after `end` when the drag went upward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: ModelPos,
    pub end: ModelPos,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Endpoints in document order, regardless of drag direction.
    pub fn ordered(&self) -> (ModelPos, ModelPos) {
        if self.end < self.start {
            (self.end, self.start)
        } else {
            (self.start, self.end)
        }
    }
}

/// One piece of a block's display text: addressed pieces carry the span
/// (or line, or cell chain) index their bytes belong to; separator
/// pieces are the display joiners between them, tabs between table
/// cells, newlines between rows and code lines, the footnote label.
pub(crate) enum Piece<'a> {
    Addr { span: usize, text: Cow<'a, str> },
    Sep(&'static str),
    Label(String),
}

/// A block's display text in order. Empty for blocks with none (rules,
/// placed images).
pub(crate) fn block_pieces(doc: &Document, index: usize) -> Vec<Piece<'_>> {
    let source = &*doc.source;
    let mut out = Vec::new();
    match &doc.blocks[index].kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::Summary { spans, .. } => span_pieces(&mut out, spans, 0, source),
        BlockKind::FootnoteDef {
            number,
            continued,
            spans,
            ..
        } => {
            if !continued {
                out.push(Piece::Label(format!("{number}.\t")));
            }
            span_pieces(&mut out, spans, 0, source);
        }
        BlockKind::CodeBlock { lines, .. } => {
            for i in 0..lines.len() {
                if i > 0 {
                    out.push(Piece::Sep("\n"));
                }
                out.push(Piece::Addr {
                    span: i,
                    text: lines.line(source, i).into(),
                });
            }
        }
        BlockKind::Table { header, rows, .. } => {
            let mut chain = 0usize;
            for (r, row) in std::iter::once(header).chain(rows.iter()).enumerate() {
                if r > 0 {
                    out.push(Piece::Sep("\n"));
                }
                for (c, cell) in row.iter().enumerate() {
                    if c > 0 {
                        out.push(Piece::Sep("\t"));
                    }
                    span_pieces(&mut out, cell, chain, source);
                    chain += cell.len();
                }
            }
        }
        BlockKind::MathBlock { tex } => {
            out.push(Piece::Addr {
                span: 0,
                text: crate::layout::math_display(tex).into(),
            });
        }
        BlockKind::Frontmatter { entries } => {
            for (i, (key, value)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(Piece::Sep("\n"));
                }
                out.push(Piece::Addr {
                    span: i,
                    text: format!("{key}: {value}").into(),
                });
            }
        }
        BlockKind::Rule
        | BlockKind::Image { .. }
        | BlockKind::ChapterBreak { .. }
        | BlockKind::PageBreak => {}
    }
    out
}

/// Inline spans as addressed pieces. A footnote reference reads with a
/// space before its label, the separation its raised baseline gives it
/// on screen.
fn span_pieces<'a>(out: &mut Vec<Piece<'a>>, spans: &'a [Span], base: usize, source: &'a str) {
    for (i, span) in spans.iter().enumerate() {
        let footnote = span
            .link
            .as_deref()
            .is_some_and(|l| l.starts_with("footnote:"));
        if footnote {
            out.push(Piece::Sep(" "));
        }
        out.push(Piece::Addr {
            span: base + i,
            text: span.text(source).into(),
        });
    }
}

/// The whole document as a selection: from its first block to its
/// last, so a picture at either end goes with a copy that carries
/// pictures, the end sitting after the last addressable piece. None
/// when nothing is selectable, a document of pictures alone.
pub fn all(doc: &Document) -> Option<Selection> {
    let last = doc.blocks.len().checked_sub(1)?;
    let mut end: Option<ModelPos> = None;
    for index in 0..doc.blocks.len() {
        for piece in block_pieces(doc, index) {
            if let Piece::Addr { span, text } = piece {
                end = Some(ModelPos {
                    block: index,
                    span,
                    byte: text.len(),
                });
            }
        }
    }
    let end = end?;
    Some(Selection {
        start: ModelPos {
            block: 0,
            span: 0,
            byte: 0,
        },
        end: if end.block == last {
            end
        } else {
            ModelPos {
                block: last,
                span: 0,
                byte: 0,
            }
        },
    })
}

/// The selected range as unstyled text, assembled from the model:
/// blocks join with a blank line, and each block's pieces join with the
/// display separators `block_pieces` defines. Only the endpoint blocks
/// slice; everything between comes whole.
pub fn plain_text(sel: &Selection, doc: &Document) -> String {
    if sel.is_empty() || doc.blocks.is_empty() {
        return String::new();
    }
    let (a, b) = sel.ordered();
    let mut out = String::new();
    for index in a.block..=b.block.min(doc.blocks.len() - 1) {
        let mut block_text = String::new();
        let mut pending_sep: Option<String> = None;
        // Set once the walk reaches the selection's first piece, so the
        // separators after it count even while no text was copied yet:
        // a selection made of line breaks alone starts at a piece's end
        // and copies the breaks after it.
        let mut started = index != a.block;
        for piece in block_pieces(doc, index) {
            match piece {
                Piece::Sep(sep) => {
                    if started {
                        pending_sep = Some(match pending_sep {
                            Some(prev) => prev + sep,
                            None => sep.to_string(),
                        });
                    }
                }
                Piece::Label(label) => {
                    if block_text.is_empty() && !started {
                        block_text.push_str(&label);
                    } else {
                        pending_sep = Some(pending_sep.unwrap_or_default() + &label);
                    }
                }
                Piece::Addr { span, text } => {
                    let mut from = 0usize;
                    let mut to = text.len();
                    if index == a.block {
                        match span.cmp(&a.span) {
                            Ordering::Less => continue,
                            Ordering::Equal => from = a.byte.min(text.len()),
                            Ordering::Greater => {}
                        }
                    }
                    started = true;
                    if index == b.block {
                        match span.cmp(&b.span) {
                            Ordering::Greater => continue,
                            Ordering::Equal => to = b.byte.min(text.len()),
                            Ordering::Less => {}
                        }
                    }
                    if from >= to {
                        // The selection's end sits at this piece's
                        // start: the separator before it was crossed
                        // and belongs to the copy, the line break of a
                        // line selected whole.
                        if index == b.block && span == b.span && to == 0 {
                            if let Some(sep) = pending_sep.take() {
                                block_text.push_str(&sep);
                            }
                        }
                        continue;
                    }
                    let slice = &text[floor_boundary(&text, from)..floor_boundary(&text, to)];
                    if let Some(sep) = pending_sep.take() {
                        block_text.push_str(&sep);
                    }
                    block_text.push_str(slice);
                }
            }
        }
        if block_text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&block_text);
    }
    out
}

/// The selected range as markdown: one verbatim slice of the document
/// source between the two endpoints. Endpoints are character-precise
/// inside verbatim spans; a position at a block's edge rounds out to
/// whole source lines so line markers come along, and positions inside
/// code blocks or tables round out to the whole block. Every block
/// strictly inside the selection widens the slice to its source lines,
/// so the notes section laying out last never drops the source tail.
pub fn markdown(sel: &Selection, doc: &Document) -> String {
    if sel.is_empty() || doc.blocks.is_empty() || doc.source.is_empty() {
        return String::new();
    }
    let (a, b) = sel.ordered();
    let mut start = source_edge(doc, &a, Edge::Start);
    let mut end = source_edge(doc, &b, Edge::End);
    for index in a.block..=b.block.min(doc.blocks.len() - 1) {
        if index == a.block || index == b.block {
            continue;
        }
        let range = &doc.blocks[index].range;
        if range.is_empty() {
            continue;
        }
        start = start.min(line_start(&doc.source, range.start));
        end = end.max(line_end(&doc.source, range.end));
    }
    let start = floor_boundary(&doc.source, start);
    let end = floor_boundary(&doc.source, end.max(start));
    doc.source[start..end].to_string()
}

enum Edge {
    Start,
    End,
}

/// Maps a model position to a byte offset in the document source.
fn source_edge(doc: &Document, pos: &ModelPos, edge: Edge) -> usize {
    let Some(block) = doc.blocks.get(pos.block) else {
        return match edge {
            Edge::Start => 0,
            Edge::End => doc.source.len(),
        };
    };
    let whole = |edge: Edge| match edge {
        Edge::Start => line_start(&doc.source, block.range.start),
        Edge::End => line_end(&doc.source, block.range.end),
    };
    let Some(spans) = kind_spans(&block.kind) else {
        return whole(edge);
    };
    let Some(span) = spans.get(pos.span).filter(|s| !s.range.is_empty()) else {
        return whole(edge);
    };
    let text_len = span.text(&doc.source).len();
    let at_block_edge = match edge {
        Edge::Start => pos.span == 0 && pos.byte == 0,
        Edge::End => pos.span + 1 >= spans.len() && pos.byte >= text_len,
    };
    if at_block_edge {
        return whole(edge);
    }
    // Character precision holds only when the span survived parsing
    // verbatim, so its display bytes are its source bytes.
    if span.is_verbatim() {
        return span.range.start as usize + pos.byte.min(text_len);
    }
    match edge {
        Edge::Start => span.range.start as usize,
        Edge::End => span.range.end as usize,
    }
}

/// Start of the source line containing `byte`.
fn line_start(source: &str, byte: usize) -> usize {
    source[..floor_boundary(source, byte)]
        .rfind('\n')
        .map_or(0, |i| i + 1)
}

/// End of the source line containing `byte`, stepping back over a trailing
/// newline so a range ending in one stays on its own last line.
fn line_end(source: &str, byte: usize) -> usize {
    let byte = floor_boundary(source, byte);
    let byte = if source[..byte].ends_with('\n') {
        byte - 1
    } else {
        byte
    };
    let end = source[byte..].find('\n').map_or(source.len(), |i| byte + i);
    // Sources are normalized at load; the strip guards text that
    // arrived another way.
    if source[..end].ends_with('\r') {
        end - 1
    } else {
        end
    }
}

/// Clamps to length and steps back to a UTF-8 character boundary.
fn floor_boundary(source: &str, byte: usize) -> usize {
    let mut byte = byte.min(source.len());
    while !source.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

fn kind_spans(kind: &BlockKind) -> Option<&[Span]> {
    match kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::FootnoteDef { spans, .. }
        | BlockKind::Summary { spans, .. } => Some(spans),
        _ => None,
    }
}

/// The model interval a run's text covers: exact bytes for a model
/// reference, the whole span for synthesized text (expanded math), and
/// nothing for markers.
pub fn run_interval(run: &TextRun) -> Option<(ModelPos, ModelPos)> {
    if run.span == MARKER_SPAN {
        return None;
    }
    let pos = |byte: usize| ModelPos {
        block: run.block,
        span: run.span,
        byte,
    };
    match run.text {
        TextRef::Model { start, len } => Some((pos(start as usize), pos((start + len) as usize))),
        TextRef::Side { .. } => Some((pos(0), pos(usize::MAX))),
    }
}

/// The model position nearest to a point in document coordinates.
/// Snaps vertically to the closest line and horizontally to the closest
/// character boundary. None only when nothing selectable is placed.
/// Character classes word expansion runs over: the characters of a
/// word (`caret::word_char`) make words, whitespace makes gaps,
/// anything else stands alone.
fn word_class(c: char) -> u8 {
    if crate::edit::caret::word_char(c) {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
}

/// Where a Shift+click extends from: the standing selection's start,
/// the end its drag began at, or with nothing selected the editor's
/// caret. None while reading with nothing selected, where a Shift+click
/// is a plain click.
pub fn shift_anchor(selection: Option<Selection>, caret: Option<ModelPos>) -> Option<ModelPos> {
    selection
        .filter(|s| !s.is_empty())
        .map(|s| s.start)
        .or(caret)
}

/// How many clicks a press reaches: two within the double-click window
/// and slop, three likewise, and a fourth starts a fresh chain.
pub fn click_chain(prev: Option<(u8, f32, f32)>, within: bool, x: f32, y: f32) -> u8 {
    match prev {
        Some((count, px, py))
            if within && count < 3 && (x - px).abs() <= 4.0 && (y - py).abs() <= 4.0 =>
        {
            count + 1
        }
        _ => 1,
    }
}

/// The contiguous run of text pieces around a span, with each piece's
/// span and text length. Separators bound it, so it is one table cell,
/// one soft segment of prose, or one code line.
fn piece_window(doc: &Document, block: usize, span: usize) -> Vec<(usize, String)> {
    let pieces = block_pieces(doc, block);
    let mut window: Vec<(usize, String)> = Vec::new();
    let mut found = false;
    for piece in &pieces {
        match piece {
            Piece::Addr { span: s, text } => {
                if *s == span {
                    found = true;
                }
                window.push((*s, text.to_string()));
            }
            _ => {
                if found {
                    break;
                }
                window.clear();
            }
        }
    }
    if found {
        window
    } else {
        Vec::new()
    }
}

/// The double-click selection: the word around a position, crossing
/// styled span boundaries but never separators (lines, cells, hard
/// breaks). Whitespace expands over its run; any other character stands
/// alone. When the position sits just past a word, the word wins, which
/// is where a snap on a word's last character lands.
pub fn word_at(doc: &Document, pos: ModelPos) -> Option<Selection> {
    let window = piece_window(doc, pos.block, pos.span);
    let mut flat = String::new();
    let mut bases = Vec::new();
    let mut at = None;
    for (span, text) in &window {
        bases.push(flat.len());
        if *span == pos.span {
            at = Some(flat.len() + pos.byte.min(text.len()));
        }
        flat.push_str(text);
    }
    let at = at?;
    // The anchor's class: the character after the click, or the one
    // before it when the one after starts no word and the one before is
    // in one; an attaching character answers its owner's class.
    let next = caret::class_at(&flat, at, word_class);
    let prev = caret::class_before(&flat, at, word_class);
    let (class, forward) = match (next, prev) {
        (Some(n), Some(p)) if n != 0 && p == 0 => (p, false),
        (Some(n), _) => (n, true),
        (None, Some(p)) => (p, false),
        (None, None) => return None,
    };
    let (start, end) = if class == 2 {
        // A symbol stands alone, with the characters attached to it.
        let cluster = caret::cluster(&flat, at, forward);
        (cluster.start, cluster.end)
    } else {
        (
            caret::class_run_start(&flat, at, &class, word_class),
            caret::class_run_end(&flat, at, &class, word_class),
        )
    };
    let locate = |flat_pos: usize| {
        let mut idx = 0;
        for (i, base) in bases.iter().enumerate() {
            if *base <= flat_pos {
                idx = i;
            } else {
                break;
            }
        }
        ModelPos {
            block: pos.block,
            span: window[idx].0,
            byte: flat_pos - bases[idx],
        }
    };
    Some(Selection {
        start: locate(start),
        end: locate(end),
    })
}

/// The triple-click selection: the whole paragraph, or the unit a
/// paragraph is to the block's kind, one code line or one table cell.
pub fn paragraph_at(doc: &Document, pos: ModelPos) -> Option<Selection> {
    match &doc.blocks[pos.block].kind {
        BlockKind::CodeBlock { lines, .. } => {
            if pos.span >= lines.len() {
                return None;
            }
            let len = lines.line(&doc.source, pos.span).len();
            Some(Selection {
                start: ModelPos { byte: 0, ..pos },
                end: ModelPos { byte: len, ..pos },
            })
        }
        BlockKind::Table { .. } => {
            let window = piece_window(doc, pos.block, pos.span);
            let (first, _) = *window.first()?;
            let (last, len) = window.last().map(|(s, t)| (*s, t.len()))?;
            Some(Selection {
                start: ModelPos {
                    block: pos.block,
                    span: first,
                    byte: 0,
                },
                end: ModelPos {
                    block: pos.block,
                    span: last,
                    byte: len,
                },
            })
        }
        _ => {
            let pieces = block_pieces(doc, pos.block);
            let addrs: Vec<(usize, usize)> = pieces
                .iter()
                .filter_map(|p| match p {
                    Piece::Addr { span, text } => Some((*span, text.len())),
                    _ => None,
                })
                .collect();
            let (first, _) = *addrs.first()?;
            let (last, len) = *addrs.last()?;
            Some(Selection {
                start: ModelPos {
                    block: pos.block,
                    span: first,
                    byte: 0,
                },
                end: ModelPos {
                    block: pos.block,
                    span: last,
                    byte: len,
                },
            })
        }
    }
}

pub fn pos_at(
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    x: f32,
    y: f32,
) -> Option<ModelPos> {
    let mut best: Option<(f32, f32, usize)> = None;
    for (i, run) in lay.runs.iter().enumerate() {
        if run.span == MARKER_SPAN {
            continue;
        }
        let bottom = run.y + metrics::LINE_HEIGHT * run.size;
        let dy = if y < run.y {
            run.y - y
        } else if y > bottom {
            y - bottom
        } else {
            0.0
        };
        let dx = if x < run.x {
            run.x - x
        } else if x > run.x + run.width {
            x - (run.x + run.width)
        } else {
            0.0
        };
        let better = match best {
            Some((bdy, bdx, _)) => (dy, dx) < (bdy, bdx),
            None => true,
        };
        if better {
            best = Some((dy, dx, i));
        }
    }
    let (_, _, index) = best?;
    let run = &lay.runs[index];
    let (iv_start, iv_end) = run_interval(run)?;
    match run.text {
        TextRef::Model { start, .. } => {
            let text = lay.run_text(doc, run);
            let family = lay.run_family(run);
            let ch = char_index_at(fonts, run, text, family, x - run.x);
            Some(ModelPos {
                block: run.block,
                span: run.span,
                byte: start as usize + byte_of_char(text, ch),
            })
        }
        // Synthesized text anchors at span granularity: before or after.
        TextRef::Side { .. } => {
            if x < run.x + run.width / 2.0 {
                Some(iv_start)
            } else {
                Some(iv_end)
            }
        }
    }
}

/// Highlight boxes for the selection, one `(x, y, width, height)` per
/// selected run fragment, in document coordinates. Boxes on the same line
/// share the height of the line's tallest run.
pub fn rects(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
) -> Vec<(f32, f32, f32, f32)> {
    rects_cached(sel, lay, doc, fonts, &mut ShapeCache::default())
}

/// Shaped buffers keyed by run index, reused across the matches of one
/// search sync, so a run is shaped once however many matches it holds.
#[derive(Default)]
pub struct ShapeCache {
    buffers: std::collections::HashMap<usize, Buffer>,
}

impl ShapeCache {
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

/// As `rects`, sharing shaped runs through the cache across calls. Runs
/// whose model interval overlaps the selection contribute a box, sliced
/// to character precision where an endpoint lands inside the run.
pub fn rects_cached(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    cache: &mut ShapeCache,
) -> Vec<(f32, f32, f32, f32)> {
    rects_for(sel, lay, doc, fonts, cache, 0..lay.runs.len())
}

/// As `rects_cached`, but only over the runs the y index answers for a
/// vertical window, which is what keeps thousands of matches cheap.
#[allow(clippy::too_many_arguments)]
pub fn rects_window(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    cache: &mut ShapeCache,
    y0: f32,
    y1: f32,
) -> Vec<(f32, f32, f32, f32)> {
    let (head, tail) = lay.runs_in(y0, y1);
    rects_for(sel, lay, doc, fonts, cache, head.chain(tail))
}

/// The y position and size of the first placed run overlapping each
/// match: one walk over the runs with a binary search into the sorted
/// matches. A match with no placed run answers with its recorded block
/// top from the layout's table, so a windowed layout still positions
/// every match; `f32::MAX` only where nothing was ever placed.
pub fn match_tops(lay: &LayoutDoc, doc: &Document, matches: &[Selection]) -> Vec<f32> {
    let mut tops = vec![f32::MAX; matches.len()];
    for (index, run) in lay.runs.iter().enumerate() {
        let Some((iv_start, mut iv_end)) = run_interval(run) else {
            continue;
        };
        iv_end.byte = iv_end
            .byte
            .saturating_add(run_tail(lay, doc, index, run).len());
        let first = matches.partition_point(|m| m.ordered().1 <= iv_start);
        for (i, m) in matches.iter().enumerate().skip(first) {
            let (a, b) = m.ordered();
            if a >= iv_end {
                break;
            }
            if b > iv_start {
                tops[i] = tops[i].min(run.y);
            }
        }
    }
    for (top, m) in tops.iter_mut().zip(matches) {
        if *top == f32::MAX {
            let pos = m.ordered().0;
            if let Some(y) = lay.approx_top(pos.block, pos.span) {
                *top = y;
            }
        }
    }
    tops
}

/// Where the current match sits: the topmost overlapping run's y and
/// text size, for scrolling it into view. None while nothing placed
/// covers it.
pub fn match_anchor(lay: &LayoutDoc, doc: &Document, m: &Selection) -> Option<(f32, f32)> {
    let (a, b) = m.ordered();
    if breaks_only(doc, m).is_some() {
        // No run overlaps a match made of line breaks; the run ending
        // the piece before the first break stands for it.
        return piece_end_run(lay, a, 0..lay.runs.len()).map(|i| (lay.runs[i].y, lay.runs[i].size));
    }
    let mut best: Option<(f32, f32)> = None;
    for (index, run) in lay.runs.iter().enumerate() {
        let Some((s, mut e)) = run_interval(run) else {
            continue;
        };
        e.byte = e.byte.saturating_add(run_tail(lay, doc, index, run).len());
        if e <= a || b <= s {
            continue;
        }
        if best.is_none_or(|(y, _)| run.y < y) {
            best = Some((run.y, run.size));
        }
    }
    best
}

/// The whitespace past a run's visible end that the run owns: the spaces
/// and tabs following its bytes in its span's text, up to the next run of
/// the same span or the span's end. Layout trims the whitespace glyphs at
/// the end of every visual line, a wrap included, so run widths match the
/// visible text; the bytes stay real, the caret already stands in them
/// (`caret::x_of`), and a box over them is measured the same way, by
/// shaping the run's text with this tail appended. Empty for most runs.
fn run_tail<'a>(lay: &LayoutDoc, doc: &'a Document, index: usize, run: &TextRun) -> &'a str {
    let TextRef::Model { start, len } = run.text else {
        return "";
    };
    if run.span == MARKER_SPAN {
        return "";
    }
    let end = (start + len) as usize;
    let after = &lay.span_text(doc, run)[end..];
    let mut tail = after
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count();
    if tail == 0 {
        return "";
    }
    // Whitespace a later visual line of the same span starts with is
    // that line's own. The runs of one span stand together in layout
    // order, so the scan ends at the first run of another span.
    for next in &lay.runs[index + 1..] {
        if next.block != run.block || next.span != run.span {
            break;
        }
        if let TextRef::Model {
            start: next_start, ..
        } = next.text
        {
            let next_start = next_start as usize;
            if next_start >= end {
                tail = tail.min(next_start - end);
                break;
            }
        }
    }
    &after[..tail]
}

/// A selection made of line breaks and nothing else: it starts at the
/// end of a piece, ends at the start of a later piece of the same block,
/// and the pieces between are empty lines or hard breaks. Answers the
/// number of breaks it covers; None for a selection holding addressable
/// text, which the runs box as usual.
fn breaks_only(doc: &Document, sel: &Selection) -> Option<usize> {
    let (a, b) = sel.ordered();
    if a.block != b.block || a.span >= b.span || b.byte != 0 {
        return None;
    }
    let source = &*doc.source;
    match &doc.blocks.get(a.block)?.kind {
        BlockKind::CodeBlock { lines, .. } => {
            if b.span >= lines.len() || a.byte != lines.line(source, a.span).len() {
                return None;
            }
            (a.span + 1..b.span)
                .all(|i| lines.line(source, i).is_empty())
                .then_some(b.span - a.span)
        }
        BlockKind::Frontmatter { entries } => {
            let (key, value) = entries.get(a.span)?;
            let whole = a.byte == format!("{key}: {value}").len();
            (whole && b.span == a.span + 1 && b.span < entries.len()).then_some(1)
        }
        BlockKind::Table { header, rows, .. } => {
            // The spans of every cell chain header first; a break stands
            // only between a row's last span and the next row's first.
            let mut chain = 0;
            for row in std::iter::once(header).chain(rows.iter()) {
                let count: usize = row.iter().map(Vec::len).sum();
                if a.span < chain + count {
                    let last = a.span + 1 == chain + count;
                    let span = row.iter().flatten().nth(a.span - chain)?;
                    let whole = a.byte == span.text(source).len();
                    return (last && whole && b.span == a.span + 1).then_some(1);
                }
                chain += count;
            }
            None
        }
        kind => {
            let spans = kind_spans(kind)?;
            if b.span >= spans.len() || a.byte != spans.get(a.span)?.text(source).len() {
                return None;
            }
            let between = &spans[a.span + 1..b.span];
            (!between.is_empty() && between.iter().all(|s| s.text(source) == "\n"))
                .then_some(between.len())
        }
    }
}

/// The run that ends the piece `pos` stands at the end of: the last of
/// its span in layout order among `indices`, and only when its bytes,
/// tail included, reach the piece's end, so a window holding the piece's
/// earlier visual lines alone answers nothing. A side run stands for its
/// whole piece.
fn piece_end_run(
    lay: &LayoutDoc,
    pos: ModelPos,
    indices: impl Iterator<Item = usize>,
) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None;
    for index in indices {
        let run = &lay.runs[index];
        if run.block != pos.block || run.span != pos.span {
            continue;
        }
        let start = match run.text {
            TextRef::Model { start, .. } => start,
            TextRef::Side { .. } => u32::MAX,
        };
        if best.is_none_or(|(s, _)| start >= s) {
            best = Some((start, index));
        }
    }
    let (_, index) = best?;
    let run = &lay.runs[index];
    match run.text {
        TextRef::Model { start, len } => {
            let reach = (start + len) as usize + run_tail_len(lay, index, run, pos.byte);
            (reach == pos.byte).then_some(index)
        }
        TextRef::Side { .. } => Some(index),
    }
}

/// The tail length of a run at the end of a piece of `len` bytes: the
/// run's whitespace tail, read without the document since the piece's
/// length bounds it.
fn run_tail_len(lay: &LayoutDoc, index: usize, run: &TextRun, len: usize) -> usize {
    let TextRef::Model {
        start,
        len: run_len,
    } = run.text
    else {
        return 0;
    };
    let end = (start + run_len) as usize;
    // A later run of the same span means the run ends a visual line
    // short of the piece; the tail then stops there, and the piece's
    // end is not this run's to reach.
    let next = lay.runs[index + 1..]
        .iter()
        .take_while(|r| r.block == run.block && r.span == run.span)
        .find_map(|r| match r.text {
            TextRef::Model { start: s, .. } if s as usize >= end => Some(s as usize),
            _ => None,
        });
    match next {
        Some(_) => 0,
        None => len.saturating_sub(end),
    }
}

/// The height of the line a run stands on: its tallest run's.
fn line_height_at(lay: &LayoutDoc, run: &TextRun) -> f32 {
    let (head, tail) = lay.runs_in(run.y, run.y);
    lay.runs[head]
        .iter()
        .chain(&lay.runs[tail])
        .filter(|r| r.block == run.block && r.y == run.y)
        .map(|r| metrics::LINE_HEIGHT * r.size)
        .fold(metrics::LINE_HEIGHT * run.size, f32::max)
}

/// The advance of one space in a face.
fn space_width(fonts: &mut FontStore, size: f32, weight: u16, italic: bool, family: &str) -> f32 {
    let buffer = shape_text(fonts, size, weight, italic, " ", family);
    buffer
        .layout_runs()
        .next()
        .and_then(|line| line.glyphs.last().map(|g| g.x + g.w))
        .unwrap_or(size * 0.3)
}

/// The boxes of a line-break selection, which no run overlaps: one of a
/// space's width at the end of the line its start piece ends on, after
/// the whitespace layout trimmed there, then one at the left edge of
/// each empty line it covers. An empty start line boxes at its own seat.
#[allow(clippy::too_many_arguments)]
fn break_boxes(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    cache: &mut ShapeCache,
    indices: impl Iterator<Item = usize>,
    breaks: usize,
) -> Vec<(f32, f32, f32, f32)> {
    let (a, _) = sel.ordered();
    let mut out = Vec::new();
    let (first_end, y, height, space, left) = match piece_end_run(lay, a, indices) {
        Some(index) => {
            let run = &lay.runs[index];
            let text = lay.run_text(doc, run);
            let tail = run_tail(lay, doc, index, run);
            let family = lay.run_family(run);
            let shaped: Cow<str> = if tail.is_empty() {
                Cow::Borrowed(text)
            } else {
                Cow::Owned(format!("{text}{tail}"))
            };
            let rtl = run_rtl(cache, fonts, index, run, &shaped, family);
            let end = if tail.is_empty() {
                if rtl {
                    run.x
                } else {
                    run.x + run.width
                }
            } else {
                let ch = shaped.chars().count();
                run.x + prefix_width(cache, fonts, index, run, &shaped, text.len(), family, ch)
            };
            let space = space_width(fonts, run.size, run.weight, run.italic, family);
            let height = line_height_at(lay, run);
            let (head, tail) = lay.runs_in(run.y, run.y);
            let left = lay.runs[head]
                .iter()
                .chain(&lay.runs[tail])
                .filter(|r| r.block == run.block && r.y == run.y)
                .map(|r| r.x)
                .fold(run.x, f32::min);
            let x = if rtl { end - space } else { end };
            (x, run.y, height, space, left)
        }
        // No run reaches the piece's end: an empty code line has its
        // seat; anything else lies outside the window.
        None => match lay.code_line_seat(a.block, a.span).filter(|_| a.byte == 0) {
            Some(seat) => {
                let space = space_width(fonts, lay.code_size, 400, false, &lay.code_family);
                (seat.x, seat.y, seat.height, space, seat.x)
            }
            None => return out,
        },
    };
    out.push((first_end, y, space, height));
    for k in 1..breaks {
        let (x, y) = match lay.code_line_seat(a.block, a.span + k) {
            Some(seat) => (seat.x, seat.y),
            None => (left, y + k as f32 * height),
        };
        out.push((x, y, space, height));
    }
    out
}

fn rects_for(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    cache: &mut ShapeCache,
    indices: impl Iterator<Item = usize>,
) -> Vec<(f32, f32, f32, f32)> {
    let (a, b) = sel.ordered();
    if let Some(breaks) = breaks_only(doc, sel) {
        return break_boxes(sel, lay, doc, fonts, cache, indices, breaks);
    }
    let mut out: Vec<(f32, f32, f32, f32)> = Vec::new();
    // The previous box's interval end and line top, kept while that box
    // reached its run's right edge, so a byte-contiguous neighbor on the
    // same line can bridge the gap justification stretched between them.
    let mut prev: Option<(ModelPos, f32)> = None;
    for index in indices {
        let run = &lay.runs[index];
        let Some((iv_start, mut iv_end)) = run_interval(run) else {
            prev = None;
            continue;
        };
        let tail = run_tail(lay, doc, index, run);
        iv_end.byte = iv_end.byte.saturating_add(tail.len());
        if iv_end <= a || b <= iv_start {
            prev = None;
            continue;
        }
        let text = lay.run_text(doc, run);
        let family = lay.run_family(run);
        let run_base = match run.text {
            TextRef::Model { start, .. } => start as usize,
            TextRef::Side { .. } => 0,
        };
        let precise = matches!(run.text, TextRef::Model { .. });
        // Boundary x of each selection end. A fully covered end sits at
        // the run's logical edge, which is the right edge on an RTL
        // run; the box spans whatever order the two land in. A run with
        // a tail shapes its text with the tail appended, so a boundary
        // inside the tail measures like any other and the logical end
        // moves past the visible text.
        let cut_start = precise && iv_start < a;
        let cut_end = precise && b < iv_end;
        let (x0, x1) = if !cut_start && !cut_end && tail.is_empty() {
            (run.x, run.x + run.width)
        } else {
            let shaped: Cow<str> = if tail.is_empty() {
                Cow::Borrowed(text)
            } else {
                Cow::Owned(format!("{text}{tail}"))
            };
            let shaped = &*shaped;
            let visible = text.len();
            let rtl = run_rtl(cache, fonts, index, run, shaped, family);
            let edge = |cache: &mut ShapeCache, fonts: &mut FontStore, logical_start: bool| {
                if logical_start {
                    if rtl {
                        run.x + run.width
                    } else {
                        run.x
                    }
                } else if tail.is_empty() {
                    if rtl {
                        run.x
                    } else {
                        run.x + run.width
                    }
                } else {
                    let ch = shaped.chars().count();
                    run.x + prefix_width(cache, fonts, index, run, shaped, visible, family, ch)
                }
            };
            let boundary = |cache: &mut ShapeCache, fonts: &mut FontStore, pos: &ModelPos| {
                let byte = pos.byte.saturating_sub(run_base).min(shaped.len());
                let ch = shaped[..floor_boundary(shaped, byte)].chars().count();
                run.x + prefix_width(cache, fonts, index, run, shaped, visible, family, ch)
            };
            let bx_a = if cut_start {
                boundary(cache, fonts, &a)
            } else {
                edge(cache, fonts, true)
            };
            let bx_b = if cut_end {
                boundary(cache, fonts, &b)
            } else {
                edge(cache, fonts, false)
            };
            (bx_a.min(bx_b), bx_a.max(bx_b))
        };
        if x1 <= x0 {
            prev = None;
            continue;
        }
        let height = line_height_at(lay, run);
        // Justified lines split words into separate runs with stretched
        // gaps between them; when the selection covers the seam on both
        // sides, this box merges into the previous one, so a selected
        // line highlights as one unbroken box.
        let seam = prev;
        prev = (b >= iv_end).then_some((iv_end, run.y));
        if let Some((prev_end, prev_y)) = seam {
            if prev_y == run.y && prev_end == iv_start && a <= iv_start {
                if let Some(last) = out.last_mut() {
                    // Bridging is for the stretched space between two
                    // adjacent justified words, so the boxes must also
                    // near-touch: at a direction seam the byte-contiguous
                    // neighbor can sit across the line, with unselected
                    // text between the boxes that must stay unlit.
                    let gap = (last.0.max(x0)) - ((last.0 + last.2).min(x1));
                    if last.1 == run.y && gap <= 1.5 * run.size {
                        // The union of the two boxes: on an RTL line the
                        // byte-contiguous neighbor sits to the left, so
                        // the merge can grow either side.
                        let hi = (last.0 + last.2).max(x1);
                        last.0 = last.0.min(x0);
                        last.2 = hi - last.0;
                        continue;
                    }
                }
            }
        }
        out.push((x0, run.y, x1 - x0, height));
    }
    out
}

pub(crate) fn byte_of_char(text: &str, ch: usize) -> usize {
    text.char_indices()
        .nth(ch)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Shapes a run exactly as paint does, single line at its own metrics.
pub(crate) fn shape_run(fonts: &mut FontStore, run: &TextRun, text: &str, family: &str) -> Buffer {
    shape_text(fonts, run.size, run.weight, run.italic, text, family)
}

/// Shapes `text` in a face named outright, where no run stands to
/// take the attributes from.
pub(crate) fn shape_text(
    fonts: &mut FontStore,
    size: f32,
    weight: u16,
    italic: bool,
    text: &str,
    family: &str,
) -> Buffer {
    let line_height = metrics::LINE_HEIGHT * size;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let mut attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(Weight(weight));
    if italic {
        attrs = attrs.style(Style::Italic);
    }
    buffer.set_text(
        &mut fonts.font_system,
        &crate::style::fonts::shapable(text),
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    buffer
}

/// The character boundary nearest to an x offset inside a run, measured
/// with the same boundary math the highlight boxes use, so a click and
/// its highlight always agree. Works in either direction: every
/// character boundary is a candidate and the nearest one wins, which
/// also answers clicks outside the run with the correct logical end.
pub(crate) fn char_index_at(
    fonts: &mut FontStore,
    run: &TextRun,
    text: &str,
    family: &str,
    x_local: f32,
) -> usize {
    let buffer = shape_run(fonts, run, text, family);
    let Some(line) = buffer.layout_runs().next() else {
        return 0;
    };
    let anchor = if line.rtl {
        run.width - line.line_w
    } else {
        0.0
    };
    let x = x_local - anchor;
    let end_x = if line.rtl { 0.0 } else { line.line_w };
    let mut chars = 0;
    let mut best = (f32::MAX, 0usize);
    for (i, (byte, _)) in text.char_indices().enumerate() {
        if let Some(bx) = boundary_x(line.glyphs, text, byte) {
            let d = (x - bx).abs();
            if d < best.0 {
                best = (d, i);
            }
        }
        chars = i + 1;
    }
    if (x - end_x).abs() < best.0 {
        return chars;
    }
    best.1
}

/// X offset (from the run's x) of the boundary before character `ch` of
/// `text`, shaping through the cache so a run shapes once per pass
/// however often it is asked. `text` is the run's own text, `visible`
/// bytes of it, with its whitespace tail appended when it has one. The
/// offset carries the paint anchor, so an RTL fragment's boundaries land
/// where paint draws its glyphs.
#[allow(clippy::too_many_arguments)]
fn prefix_width(
    cache: &mut ShapeCache,
    fonts: &mut FontStore,
    index: usize,
    run: &TextRun,
    text: &str,
    visible: usize,
    family: &str,
    ch: usize,
) -> f32 {
    let buffer = cache
        .buffers
        .entry(index)
        .or_insert_with(|| shape_run(fonts, run, text, family));
    let Some(line) = buffer.layout_runs().next() else {
        return 0.0;
    };
    let rtl = line.rtl;
    let tailed = visible < text.len();
    // Where the fragment's x = 0 lands against the run's x. Shaped alone,
    // an LTR fragment starts at the run's left edge and an RTL one ends
    // at its right edge, the difference to its own width being a
    // justified line's stretch. With the tail along, the run's own
    // glyphs map onto the run's box and the tail falls past its logical
    // end: right of an LTR run, left of an RTL one.
    let anchor = if !tailed {
        if rtl {
            run.width - line.line_w
        } else {
            0.0
        }
    } else {
        let own_left = line
            .glyphs
            .iter()
            .filter(|g| g.start < visible)
            .map(|g| g.x)
            .fold(f32::MAX, f32::min);
        if own_left == f32::MAX {
            0.0
        } else {
            -own_left
        }
    };
    let byte = byte_of_char(text, ch);
    if byte < text.len() {
        if let Some(x) = boundary_x(line.glyphs, text, byte) {
            return anchor + x;
        }
    }
    // The boundary after the last character: the line's logical end.
    if tailed {
        let far = if rtl {
            line.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min)
        } else {
            line.glyphs
                .iter()
                .map(|g| g.x + g.w)
                .fold(f32::MIN, f32::max)
        };
        return if far.is_finite() {
            anchor + far
        } else {
            anchor
        };
    }
    if rtl {
        anchor
    } else {
        run.width
    }
}

/// Whether a run's shaped line reads right to left, from the cache.
fn run_rtl(
    cache: &mut ShapeCache,
    fonts: &mut FontStore,
    index: usize,
    run: &TextRun,
    text: &str,
    family: &str,
) -> bool {
    let buffer = cache
        .buffers
        .entry(index)
        .or_insert_with(|| shape_run(fonts, run, text, family));
    buffer.layout_runs().next().is_some_and(|line| line.rtl)
}

/// X offset of a byte boundary among shaped glyphs. An LTR glyph's
/// boundary sits at its left edge; an RTL glyph's at its right, since
/// the character before it draws to its right. The glyph that starts at
/// the byte answers first, wherever the line puts it: a line of mixed
/// directions (a field's whole text) comes in visual order, where the
/// first glyph past the byte can belong to another word. A cluster drawn
/// as several glyphs (a letter and its mark) takes the outermost edge. A
/// boundary inside a glyph's cluster (a ligature such as fi is one glyph
/// over two characters) splits the glyph's width evenly per character,
/// mirrored for RTL, so carets and highlights land between the
/// characters and always agree.
pub(crate) fn boundary_x(glyphs: &[LayoutGlyph], text: &str, byte: usize) -> Option<f32> {
    let mut starting: Option<f32> = None;
    for glyph in glyphs.iter().filter(|g| g.start == byte) {
        let rtl = glyph.level.is_rtl();
        let edge = if rtl { glyph.x + glyph.w } else { glyph.x };
        starting = Some(match starting {
            Some(seen) if rtl => seen.max(edge),
            Some(seen) => seen.min(edge),
            None => edge,
        });
    }
    if starting.is_some() {
        return starting;
    }
    for glyph in glyphs {
        let rtl = glyph.level.is_rtl();
        if glyph.start >= byte {
            return Some(if rtl { glyph.x + glyph.w } else { glyph.x });
        }
        if byte < glyph.end {
            let within = text[glyph.start..byte].chars().count() as f32;
            let total = text[glyph.start..glyph.end].chars().count() as f32;
            let frac = within / total.max(1.0);
            return Some(if rtl {
                glyph.x + glyph.w * (1.0 - frac)
            } else {
                glyph.x + glyph.w * frac
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::markdown;
    use crate::layout::{layout, ViewConfig};
    use crate::style::theme::Theme;
    use std::path::PathBuf;

    fn lay_doc_at(source: &str, width: f32) -> (Document, LayoutDoc, FontStore) {
        let doc = markdown::parse(source);
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            width,
        );
        (doc, l, fonts)
    }

    fn lay_doc(source: &str) -> (Document, LayoutDoc, FontStore) {
        lay_doc_at(source, 2000.0)
    }

    fn select_all(doc: &Document) -> Selection {
        all(doc).expect("document has selectable content")
    }

    #[test]
    fn a_line_selected_with_its_break_copies_the_break() {
        let doc = markdown::parse("```\nalpha\nbeta\n```\n");
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 0,
            },
            end: ModelPos {
                block: 0,
                span: 1,
                byte: 0,
            },
        };
        assert_eq!(
            plain_text(&sel, &doc),
            "alpha\n",
            "a selection reaching the next line's start covers the break"
        );
    }

    #[test]
    fn justified_selection_bridges_word_gaps() {
        let doc = markdown::parse(format!("{}end.\n", "justify word ".repeat(30)));
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig {
                justify: true,
                ..ViewConfig::default()
            },
            700.0,
        );
        let sel = select_all(&doc);
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        let mut ys: Vec<i32> = boxes.iter().map(|b| b.1.round() as i32).collect();
        ys.dedup();
        assert_eq!(
            boxes.len(),
            ys.len(),
            "a fully selected justified line is one unbroken box"
        );
    }

    #[test]
    fn cached_match_rects_equal_direct_and_share_shapings() {
        // Trailing spaces on every line: the boxes past a run's visible
        // end go through the cache like the rest.
        let (doc, lay, mut fonts) = lay_doc(&format!(
            "```\n{}```\n",
            "the word the word the word here.   \n".repeat(8)
        ));
        let matches = crate::ui::search::matches(&doc, " ");
        assert!(matches.len() >= 16, "the fixture is match-dense");
        let mut cache = ShapeCache::default();
        for m in &matches {
            let direct = rects(m, &lay, &doc, &mut fonts);
            let cached = rects_cached(m, &lay, &doc, &mut fonts, &mut cache);
            assert_eq!(direct, cached, "the cache changes nothing visible");
        }
        assert!(!cache.is_empty(), "the cache actually holds shaped runs");
        assert!(
            cache.len() < matches.len(),
            "{} runs shaped for {} matches: once per run, not per match",
            cache.len(),
            matches.len()
        );
    }

    #[test]
    fn line_end_steps_back_over_a_carriage_return() {
        let src = "alpha\r\nbeta\r\n";
        assert_eq!(line_end(src, 2), 5, "the carriage return stays out");
        assert_eq!(
            line_end(src, 9),
            11,
            "the second line ends before its return"
        );
        assert_eq!(line_end("plain\nnext", 1), 5, "clean sources are untouched");
    }

    #[test]
    fn markdown_round_trips_styles() {
        let source = "# Title\n\nplain **bold** *italic* ~~gone~~ `code` [link](https://a.tld)";
        let (doc, _, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&doc), &doc), source);
    }

    #[test]
    fn double_click_selects_the_word_across_styles() {
        let doc = markdown::parse("make it **fas**ter now");
        let pos = ModelPos {
            block: 0,
            span: 1,
            byte: 1,
        };
        let sel = word_at(&doc, pos).expect("a word under the cursor");
        assert_eq!(plain_text(&sel, &doc), "faster", "styling splits no word");
    }

    #[test]
    fn a_double_click_takes_a_joined_emoji_whole() {
        let doc = markdown::parse("see \u{1f469}\u{200d}\u{1f4bb} now");
        let pos = ModelPos {
            block: 0,
            span: 0,
            byte: 4,
        };
        let sel = word_at(&doc, pos).expect("the emoji under the cursor");
        assert_eq!(plain_text(&sel, &doc), "\u{1f469}\u{200d}\u{1f4bb}");
        let inside = ModelPos {
            block: 0,
            span: 0,
            byte: 11,
        };
        let sel = word_at(&doc, inside).expect("from inside the sequence");
        assert_eq!(plain_text(&sel, &doc), "\u{1f469}\u{200d}\u{1f4bb}");
    }

    #[test]
    fn double_click_on_space_punctuation_and_word_ends() {
        let doc = markdown::parse("one   two , three");
        let sel = word_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: 4,
            },
        )
        .expect("the gap");
        assert_eq!(
            plain_text(&sel, &doc),
            "   ",
            "whitespace runs select whole"
        );
        let sel = word_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: 10,
            },
        )
        .expect("the comma");
        assert_eq!(plain_text(&sel, &doc), ",", "punctuation stands alone");
        let sel = word_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: 9,
            },
        )
        .expect("just past a word");
        assert_eq!(plain_text(&sel, &doc), "two", "the word wins at its end");
    }

    #[test]
    fn a_word_never_crosses_a_cell_boundary() {
        let doc = markdown::parse("|a|b|\n|-|-|\n|end|start|");
        let cells: Vec<ModelPos> = (0..4)
            .map(|span| ModelPos {
                block: 0,
                span,
                byte: 0,
            })
            .collect();
        let sel = word_at(&doc, cells[2]).expect("the first body cell");
        assert_eq!(plain_text(&sel, &doc), "end", "the tab boundary holds");
    }

    #[test]
    fn triple_click_selects_paragraph_line_or_cell() {
        let doc = markdown::parse("A first sentence. A second one.\n\nAnother paragraph.");
        let sel = paragraph_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: 3,
            },
        )
        .expect("the paragraph");
        assert_eq!(plain_text(&sel, &doc), "A first sentence. A second one.");

        let code = markdown::parse("```\nfirst line\nsecond line\n```");
        let sel = paragraph_at(
            &code,
            ModelPos {
                block: 0,
                span: 1,
                byte: 2,
            },
        )
        .expect("the line");
        assert_eq!(
            plain_text(&sel, &code),
            "second line",
            "code answers one line"
        );

        let table = markdown::parse("|a|b|\n|-|-|\n|one two|three|");
        let sel = paragraph_at(
            &table,
            ModelPos {
                block: 0,
                span: 2,
                byte: 0,
            },
        )
        .expect("the cell");
        assert_eq!(
            plain_text(&sel, &table),
            "one two",
            "a table answers the cell"
        );
    }

    #[test]
    fn a_shift_click_extends_from_the_selection_or_the_caret() {
        let at = |byte: usize| ModelPos {
            block: 0,
            span: 0,
            byte,
        };
        let standing = Selection {
            start: at(9),
            end: at(3),
        };
        assert_eq!(
            shift_anchor(Some(standing), Some(at(3))),
            Some(at(9)),
            "the selection's own start, where its drag began, whichever way it ran"
        );
        let empty = Selection {
            start: at(5),
            end: at(5),
        };
        assert_eq!(
            shift_anchor(Some(empty), Some(at(7))),
            Some(at(7)),
            "an empty selection is none: the editor's caret"
        );
        assert_eq!(shift_anchor(None, Some(at(7))), Some(at(7)));
        assert_eq!(
            shift_anchor(None, None),
            None,
            "reading with nothing selected: a plain click"
        );
    }

    #[test]
    fn click_chain_counts_and_cycles() {
        assert_eq!(click_chain(None, true, 10.0, 10.0), 1);
        assert_eq!(click_chain(Some((1, 10.0, 10.0)), true, 12.0, 11.0), 2);
        assert_eq!(click_chain(Some((2, 10.0, 10.0)), true, 10.0, 10.0), 3);
        assert_eq!(
            click_chain(Some((3, 10.0, 10.0)), true, 10.0, 10.0),
            1,
            "a fourth click starts over"
        );
        assert_eq!(
            click_chain(Some((1, 10.0, 10.0)), false, 10.0, 10.0),
            1,
            "the window closed"
        );
        assert_eq!(
            click_chain(Some((1, 10.0, 10.0)), true, 40.0, 10.0),
            1,
            "moved too far"
        );
    }

    #[test]
    fn copies_cover_closed_details_content() {
        let doc = markdown::parse(
            "Before.\n\n<details>\n<summary>S</summary>\n\nthe needle hides here\n\n</details>\n\nAfter.",
        );
        let sel = all(&doc).expect("a selection over the document");
        let text = plain_text(&sel, &doc);
        assert!(text.contains("needle"), "fold state never truncates a copy");
        assert!(text.contains("Before.") && text.contains("After."));
    }

    #[test]
    fn plain_text_drops_styles() {
        let (doc, _, _) = lay_doc("plain **bold** `code` [link](https://a.tld)");
        assert_eq!(plain_text(&select_all(&doc), &doc), "plain bold code link");
    }

    #[test]
    fn partial_selection_joins_paragraphs_with_blank_line() {
        let (doc, _, _) = lay_doc("alpha one\n\nsecond beta");
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 6,
            },
            end: ModelPos {
                block: 1,
                span: 0,
                byte: 6,
            },
        };
        assert_eq!(plain_text(&sel, &doc), "one\n\nsecond");
    }

    #[test]
    fn all_selects_every_run() {
        let (doc, _, _) = lay_doc("# Title\n\n- item with `code`");
        let sel = all(&doc).unwrap();
        assert_eq!(plain_text(&sel, &doc), "Title\n\nitem with code");
        assert_eq!(markdown(&sel, &doc), "# Title\n\n- item with `code`");
    }

    #[test]
    fn all_of_empty_document_is_none() {
        assert!(all(&Document::default()).is_none());
    }

    #[test]
    fn all_spans_the_pictures_at_both_ends() {
        let source = "![a](a.png)\n\ntext\n\n![b](b.png)";
        let (doc, _, _) = lay_doc(source);
        let sel = all(&doc).unwrap();
        assert_eq!((sel.start.block, sel.end.block), (0, 2));
        assert_eq!(plain_text(&sel, &doc), "text");
        assert_eq!(markdown(&sel, &doc), source);
    }

    #[test]
    fn all_of_pictures_alone_is_none() {
        let (doc, _, _) = lay_doc("![a](a.png)\n\n![b](b.png)");
        assert!(all(&doc).is_none());
    }

    #[test]
    fn markdown_preserves_structure_from_source() {
        let source = "> quoted line\n\n- item one\n- item, with **bold**\n  - nested\n\n1. first\n2. second\n\n- [x] done\n- [ ] todo\n\n---\n\nafter the rule";
        let (doc, _, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&doc), &doc), source);
    }

    #[test]
    fn markdown_partial_selection_slices_characters() {
        let source = "alpha one\n\nsecond beta";
        let (doc, _, _) = lay_doc(source);
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 6,
            },
            end: ModelPos {
                block: 1,
                span: 0,
                byte: 6,
            },
        };
        assert_eq!(markdown(&sel, &doc), "one\n\nsecond");
    }

    // Footnote definitions lay out at the end as the notes section, but
    // the model interval is source-ordered; a select-all must cover the
    // source tail.
    #[test]
    fn markdown_select_all_covers_blocks_laid_out_of_source_order() {
        let source =
            "body one.\n\nA claim[^n] made here.\n\n[^n]: The note text.\n\nbody two ends here.\n";
        let (doc, _, _) = lay_doc(source);
        let md = markdown(&select_all(&doc), &doc);
        assert!(
            md.contains("body two ends here."),
            "the copy covered the source tail, got {md:?}"
        );
        assert!(md.starts_with("body one."), "got {md:?}");
        assert!(md.contains("[^n]: The note text."));
    }

    #[test]
    fn markdown_partial_precision_survives_the_coverage_walk() {
        let source = "alpha one\n\nsecond beta\n\n[^x]: a note\n\nafter[^x] text\n";
        let (doc, _, _) = lay_doc(source);
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 6,
            },
            end: ModelPos {
                block: 1,
                span: 0,
                byte: 6,
            },
        };
        assert_eq!(markdown(&sel, &doc), "one\n\nsecond");
    }

    #[test]
    fn markdown_fences_code_blocks() {
        let source = "intro\n\n```rust\nfn a() {}\n\nfn b() {}\n```\n\noutro";
        let (doc, _, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&doc), &doc), source);
    }

    #[test]
    fn markdown_fences_unlabeled_code() {
        let source = "```\nplain text\n```";
        let (doc, _, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&doc), &doc), source);
    }

    #[test]
    fn blank_code_lines_survive_plain_copy() {
        let source = "```rust\nfn a() {}\n\nfn b() {}\n```";
        let (doc, _, _) = lay_doc(source);
        assert_eq!(
            plain_text(&select_all(&doc), &doc),
            "fn a() {}\n\nfn b() {}"
        );
    }

    #[test]
    fn upward_drag_normalizes() {
        let (doc, _, _) = lay_doc("alpha one\n\nsecond beta");
        let sel = Selection {
            start: ModelPos {
                block: 1,
                span: 0,
                byte: 6,
            },
            end: ModelPos {
                block: 0,
                span: 0,
                byte: 6,
            },
        };
        assert_eq!(plain_text(&sel, &doc), "one\n\nsecond");
    }

    #[test]
    fn pos_at_snaps_to_character_boundaries() {
        let (doc, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let left = pos_at(&l, &doc, &mut fonts, run.x + 0.5, run.y + 1.0).unwrap();
        assert_eq!(
            left,
            ModelPos {
                block: 0,
                span: 0,
                byte: 0
            }
        );
        let right = pos_at(&l, &doc, &mut fonts, run.x + run.width + 50.0, run.y + 1.0).unwrap();
        assert_eq!(
            right,
            ModelPos {
                block: 0,
                span: 0,
                byte: l.run_text(&doc, run).len()
            }
        );
    }

    #[test]
    fn a_highlight_splits_a_ligature() {
        let (doc, l, mut fonts) = lay_doc("field");
        let pos = |byte| ModelPos {
            block: 0,
            span: 0,
            byte,
        };
        let f_only = Selection {
            start: pos(0),
            end: pos(1),
        };
        let fi = Selection {
            start: pos(0),
            end: pos(2),
        };
        let w_f = rects(&f_only, &l, &doc, &mut fonts)[0].2;
        let w_fi = rects(&fi, &l, &doc, &mut fonts)[0].2;
        assert!(w_f > 0.0);
        assert!(
            w_f < w_fi,
            "the f alone ({w_f}) is narrower than the fi ligature ({w_fi})"
        );
    }

    #[test]
    fn rects_cover_fully_selected_run() {
        let (doc, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let sel = select_all(&doc);
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        assert_eq!(boxes.len(), 1);
        let (x, _y, w, h) = boxes[0];
        assert!((x - run.x).abs() < 0.5);
        assert!((w - run.width).abs() < 0.5);
        assert!(h > run.size, "box covers the line height");
    }

    // ---- Whitespace past a line's visible end: the tail a run owns ----

    /// The model runs of one code line, in byte order.
    fn line_runs(l: &LayoutDoc, block: usize, line: usize) -> Vec<&TextRun> {
        let start_of = |r: &TextRun| match r.text {
            TextRef::Model { start, .. } => start,
            TextRef::Side { .. } => u32::MAX,
        };
        let mut runs: Vec<&TextRun> = l
            .runs
            .iter()
            .filter(|r| r.block == block && r.span == line && start_of(r) != u32::MAX)
            .collect();
        runs.sort_by_key(|r| start_of(r));
        runs
    }

    fn one_box(
        m: &Selection,
        l: &LayoutDoc,
        doc: &Document,
        fonts: &mut FontStore,
    ) -> (f32, f32, f32, f32) {
        let boxes = rects(m, l, doc, fonts);
        assert_eq!(boxes.len(), 1, "one box for {m:?}: {boxes:?}");
        boxes[0]
    }

    fn close(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.6
            && (a.1 - b.1).abs() < 0.6
            && (a.2 - b.2).abs() < 0.6
            && (a.3 - b.3).abs() < 0.6
    }

    #[test]
    fn trailing_spaces_on_a_code_line_get_their_boxes() {
        let (doc, l, mut fonts) = lay_doc("```\nalpha beta   \n```\n");
        let spaces = crate::ui::search::matches(&doc, " ");
        assert_eq!(spaces.len(), 4, "one between the words, three trailing");
        let run = line_runs(&l, 0, 0)[0];
        let (_, _, space_w, _) = one_box(&spaces[0], &l, &doc, &mut fonts);
        for (i, m) in spaces[1..].iter().enumerate() {
            let (x, y, w, _) = one_box(m, &l, &doc, &mut fonts);
            let expected = run.x + run.width + i as f32 * space_w;
            assert_eq!(y, run.y);
            assert!(
                (x - expected).abs() < 0.6,
                "trailing space {i} starts at {x}, expected {expected}"
            );
            assert!(
                (w - space_w).abs() < 0.6,
                "trailing space {i} is {w} wide, a space is {space_w}"
            );
        }
    }

    #[test]
    fn tabs_at_a_wrap_box_the_end_of_the_first_visual_line() {
        let doc = markdown::parse("```\nword word word\t\tmore words after the tabs\n```\n");
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        // Narrow the layout until the line wraps at the two tabs: the
        // first visual line ends before them, the second starts after.
        let mut found = None;
        let mut width = 900.0;
        while width > 100.0 && found.is_none() {
            let l = layout(
                &doc,
                &Theme::default_dark(),
                &mut fonts,
                &mut media,
                &ViewConfig::default(),
                width,
            );
            let runs = line_runs(&l, 0, 0);
            let ends_before_tabs = runs
                .first()
                .is_some_and(|r| matches!(r.text, TextRef::Model { start: 0, len: 14 }));
            let resumes_after = runs
                .iter()
                .any(|r| matches!(r.text, TextRef::Model { start: 16, .. }));
            if ends_before_tabs && resumes_after {
                found = Some(l);
            }
            width -= 4.0;
        }
        let l = found.expect("a width wraps the line at the tabs");
        let tabs = crate::ui::search::regex_matches(&doc, "\\t").expect("a valid pattern");
        assert_eq!(tabs.len(), 2);
        let first = line_runs(&l, 0, 0)[0];
        let (x0, y0, w0, _) = one_box(&tabs[0], &l, &doc, &mut fonts);
        let (x1, y1, w1, _) = one_box(&tabs[1], &l, &doc, &mut fonts);
        assert_eq!(y0, first.y, "the first tab sits on the first visual line");
        assert_eq!(y1, first.y, "so does the second");
        assert!(
            x0 >= first.x + first.width - 0.5,
            "the first tab starts past the visible text: {x0} against {}",
            first.x + first.width
        );
        assert!(
            (x1 - (x0 + w0)).abs() < 0.6,
            "the second tab follows the first: {x1} against {}",
            x0 + w0
        );
        assert!(w0 > 0.0 && w1 > 0.0, "{w0} and {w1}");
    }

    #[test]
    fn a_tab_and_a_space_mid_line_keep_their_boxes() {
        let (doc, l, mut fonts) = lay_doc("```\ngamma\tdelta beta\n```\n");
        let tab = crate::ui::search::regex_matches(&doc, "\\t").expect("a valid pattern");
        let space = crate::ui::search::matches(&doc, " ");
        assert_eq!((tab.len(), space.len()), (1, 1));
        let got = [
            one_box(&tab[0], &l, &doc, &mut fonts),
            one_box(&space[0], &l, &doc, &mut fonts),
        ];
        // The boxes as they were before a run learned its whitespace
        // tail, in the embedded code face.
        let recorded = [(231.96, 56.0, 35.98, 30.0), (327.9, 56.0, 11.99, 30.0)];
        assert!(
            got.iter().zip(recorded).all(|(g, w)| close(*g, w)),
            "{got:?} against the recorded {recorded:?}"
        );
    }

    #[test]
    fn a_space_at_a_paragraph_wrap_gets_a_box() {
        let (doc, l, mut fonts) = lay_doc_at(&"word ".repeat(40), 400.0);
        let spaces = crate::ui::search::matches(&doc, " ");
        assert_eq!(spaces.len(), 39, "the paragraph's last space is stripped");
        let mut ys: Vec<i32> = l
            .runs
            .iter()
            .filter(|r| r.block == 0)
            .map(|r| r.y.round() as i32)
            .collect();
        ys.dedup();
        assert!(ys.len() >= 3, "the paragraph wraps: {} lines", ys.len());
        // Every space has one box; the one at a wrap sits past the
        // visible end of its line.
        let mut at_wraps = 0;
        for m in &spaces {
            let (x, y, w, _) = one_box(m, &l, &doc, &mut fonts);
            assert!(w > 0.0);
            let right = l
                .runs
                .iter()
                .filter(|r| r.block == 0 && r.y == y)
                .map(|r| r.x + r.width)
                .fold(f32::MIN, f32::max);
            if x >= right - 0.5 {
                at_wraps += 1;
            }
        }
        assert_eq!(
            at_wraps,
            ys.len() - 1,
            "one boxed space at the end of every wrapped line"
        );
    }

    #[test]
    fn a_selection_to_the_line_end_covers_the_trailing_spaces() {
        let (doc, l, mut fonts) = lay_doc("```\nalpha beta   \n```\n");
        let run = line_runs(&l, 0, 0)[0];
        let spaces = crate::ui::search::matches(&doc, " ");
        let (_, _, space_w, _) = one_box(&spaces[0], &l, &doc, &mut fonts);
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 0,
            },
            end: ModelPos {
                block: 0,
                span: 0,
                byte: 13,
            },
        };
        let (x, _, w, _) = one_box(&sel, &l, &doc, &mut fonts);
        assert!((x - run.x).abs() < 0.5);
        assert!(
            (w - (run.width + 3.0 * space_w)).abs() < 1.0,
            "the box runs over the three trailing spaces: {w} against {}",
            run.width + 3.0 * space_w
        );
    }

    #[test]
    fn the_current_match_anchors_on_a_trailing_space() {
        let (doc, l, _fonts) = lay_doc("```\nalpha beta   \ngamma\n```\n");
        let spaces = crate::ui::search::matches(&doc, " ");
        let last = spaces.last().expect("four spaces");
        let first_line = line_runs(&l, 0, 0)[0];
        let (top, size) =
            match_anchor(&l, &doc, last).expect("the trailing space anchors on its line");
        assert_eq!(top, first_line.y);
        assert_eq!(size, first_line.size);
        let tops = match_tops(&l, &doc, &spaces);
        assert!(tops.iter().all(|t| *t == first_line.y), "{tops:?}");
    }

    #[test]
    fn an_rtl_wrap_boxes_the_space_left_of_the_line() {
        let (doc, l, mut fonts) = lay_doc_at(&format!("{RTL_LINE} {RTL_LINE} {RTL_LINE}"), 500.0);
        let spaces = crate::ui::search::matches(&doc, " ");
        let mut ys: Vec<i32> = l
            .runs
            .iter()
            .filter(|r| r.block == 0)
            .map(|r| r.y.round() as i32)
            .collect();
        ys.dedup();
        assert!(ys.len() >= 2, "the text wraps: {} lines", ys.len());
        let mut at_wraps = 0;
        for m in &spaces {
            let (x, y, w, _) = one_box(m, &l, &doc, &mut fonts);
            assert!(w > 0.0 && x.is_finite(), "{x} {w}");
            let left = l
                .runs
                .iter()
                .filter(|r| r.block == 0 && r.y == y)
                .map(|r| r.x)
                .fold(f32::MAX, f32::min);
            if x + w <= left + 0.5 {
                at_wraps += 1;
            }
        }
        assert_eq!(
            at_wraps,
            ys.len() - 1,
            "the space trimmed at each wrap boxes left of its line, the logical end"
        );
    }

    #[test]
    fn a_justified_rtl_wrap_boxes_the_space_left_of_the_line_too() {
        // The gate's claim of 15/09/2026: a tailed run on a justified
        // RTL line would miss the stretch. The last group of a line
        // carries no stretched space, so the tail's anchor is exact.
        let doc = markdown::parse(format!("{RTL_LINE} {RTL_LINE} {RTL_LINE} {RTL_LINE}"));
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig {
                justify: true,
                ..ViewConfig::default()
            },
            500.0,
        );
        let spaces = crate::ui::search::matches(&doc, " ");
        let mut ys: Vec<i32> = l
            .runs
            .iter()
            .filter(|r| r.block == 0)
            .map(|r| r.y.round() as i32)
            .collect();
        ys.dedup();
        assert!(
            ys.len() >= 3,
            "the text wraps and justifies: {} lines",
            ys.len()
        );
        let mut at_wraps = 0;
        for m in &spaces {
            let (x, y, w, _) = one_box(m, &l, &doc, &mut fonts);
            assert!(w > 0.0 && x.is_finite(), "{x} {w}");
            let left = l
                .runs
                .iter()
                .filter(|r| r.block == 0 && r.y == y)
                .map(|r| r.x)
                .fold(f32::MAX, f32::min);
            if x + w <= left + 0.5 {
                at_wraps += 1;
            }
        }
        assert_eq!(at_wraps, ys.len() - 1, "one box left of each wrapped line");
    }

    // ---- Line-break matches: a box at the end of each line broken ----

    fn lay_code(text: &str) -> (Document, LayoutDoc, FontStore) {
        let doc = crate::doc::load::code_document(None, text);
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            2000.0,
        );
        (doc, l, fonts)
    }

    #[test]
    fn a_line_break_match_boxes_the_end_of_its_line() {
        let (doc, l, mut fonts) = lay_code("alpha beta   \ngamma\n");
        let breaks = crate::ui::search::regex_matches(&doc, "\\n").expect("valid");
        assert_eq!(breaks.len(), 1);
        let spaces = crate::ui::search::matches(&doc, " ");
        let (sx, _, sw, _) = one_box(&spaces[3], &l, &doc, &mut fonts);
        let run = line_runs(&l, 0, 0)[0];
        let (x, y, w, _) = one_box(&breaks[0], &l, &doc, &mut fonts);
        assert_eq!(y, run.y);
        assert!(
            (x - (sx + sw)).abs() < 0.6,
            "after the trailing whitespace: {x} against {}",
            sx + sw
        );
        assert!((w - sw).abs() < 0.6, "a space wide: {w} against {sw}");
    }

    #[test]
    fn a_double_line_break_boxes_the_empty_line_too() {
        let (doc, l, mut fonts) = lay_code("one\n\ntwo\n");
        let breaks = crate::ui::search::regex_matches(&doc, "\\n\\n").expect("valid");
        assert_eq!(breaks.len(), 1);
        let run = line_runs(&l, 0, 0)[0];
        let boxes = rects(&breaks[0], &l, &doc, &mut fonts);
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        let (x0, y0, w0, h0) = boxes[0];
        let (x1, y1, w1, _) = boxes[1];
        assert_eq!(y0, run.y);
        assert!(
            (x0 - (run.x + run.width)).abs() < 0.6,
            "{x0} against {}",
            run.x + run.width
        );
        let seat = l.code_line_seat(0, 1).expect("the empty line has a seat");
        assert!(
            (x1 - seat.x).abs() < 0.6,
            "the empty line's box at its left edge"
        );
        assert!(
            (y1 - (y0 + h0)).abs() < 0.6,
            "on the row below: {y1} against {}",
            y0 + h0
        );
        assert!((w1 - w0).abs() < 0.6);
    }

    #[test]
    fn text_selections_over_three_lines_keep_their_boxes() {
        let (doc, l, mut fonts) = lay_code("one\ntwo\nthree\n");
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 1,
            },
            end: ModelPos {
                block: 0,
                span: 2,
                byte: 2,
            },
        };
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        // The boxes as they were before line-break matches drew any.
        let recorded = [
            (171.99, 44.0, 23.98, 30.0),
            (160.0, 74.0, 35.98, 30.0),
            (160.0, 104.0, 23.98, 30.0),
        ];
        assert!(
            boxes.len() == 3 && boxes.iter().zip(recorded).all(|(g, w)| close(*g, w)),
            "{boxes:?} against the recorded {recorded:?}"
        );
    }

    #[test]
    fn copying_a_line_break_selection_gives_one_line_break() {
        let doc = crate::doc::load::code_document(None, "one\ntwo\n");
        let breaks = crate::ui::search::regex_matches(&doc, "\\n").expect("valid");
        assert_eq!(plain_text(&breaks[0], &doc), "\n");
        let doc = crate::doc::load::code_document(None, "one\n\ntwo\n");
        let breaks = crate::ui::search::regex_matches(&doc, "\\n\\n").expect("valid");
        assert_eq!(plain_text(&breaks[0], &doc), "\n\n");
    }

    // ---- RTL lines: hit testing, boxes, the justified merge ----

    const RTL_LINE: &str = "اعلم أن فن التاريخ فن عزيز المذهب";

    #[test]
    fn rtl_edges_map_to_the_logical_ends() {
        let (doc, l, mut fonts) = lay_doc(RTL_LINE);
        let run = l.runs.iter().find(|r| r.width > 100.0).expect("the run");
        let (x, y) = (run.x, run.y + 2.0);
        let right = pos_at(&l, &doc, &mut fonts, x + run.width + 10.0, y).expect("a position");
        let left = pos_at(&l, &doc, &mut fonts, x - 10.0, y).expect("a position");
        assert_eq!(right.byte, 0, "past the right edge is the logical start");
        assert_eq!(
            left.byte,
            RTL_LINE.len(),
            "past the left edge is the logical end"
        );
    }

    #[test]
    fn a_drag_across_an_rtl_run_selects_the_logical_middle() {
        let (doc, l, mut fonts) = lay_doc(RTL_LINE);
        let run = l.runs.iter().find(|r| r.width > 100.0).expect("the run");
        let y = run.y + 2.0;
        let from_right = pos_at(&l, &doc, &mut fonts, run.x + run.width - 2.0, y).expect("a hit");
        let from_left = pos_at(&l, &doc, &mut fonts, run.x + 2.0, y).expect("a hit");
        assert!(
            from_right.byte < from_left.byte,
            "the right edge reads before the left: {} vs {}",
            from_right.byte,
            from_left.byte
        );
        let sel = Selection {
            start: from_right,
            end: from_left,
        };
        let text = plain_text(&sel, &doc);
        assert!(
            RTL_LINE.contains(text.trim()),
            "the drag copies logical text: {text:?}"
        );
        assert!(
            text.chars().count() > 20,
            "the drag covered most of the line: {text:?}"
        );
    }

    #[test]
    fn a_logical_prefix_highlights_at_the_right_edge() {
        let (doc, l, mut fonts) = lay_doc(RTL_LINE);
        let run = l.runs.iter().find(|r| r.width > 100.0).expect("the run");
        let (iv_start, _) = run_interval(run).expect("the interval");
        let word = "اعلم";
        let sel = Selection {
            start: iv_start,
            end: ModelPos {
                byte: iv_start.byte + word.len(),
                ..iv_start
            },
        };
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        assert_eq!(boxes.len(), 1, "one word, one box");
        let (x, _, w, _) = boxes[0];
        assert!(w > 5.0, "the box has the word's width, got {w}");
        assert!(
            (run.x + run.width) - (x + w) < 1.0,
            "the first word's box hugs the right edge: box right {}, run right {}",
            x + w,
            run.x + run.width
        );
        assert!(
            x > run.x + 5.0,
            "the box leaves the rest of the line unlit: box x {}, run x {}",
            x,
            run.x
        );
    }

    #[test]
    fn a_cross_script_selection_boxes_each_side_inside_its_run() {
        let source = "قبل history بعد";
        let (doc, l, mut fonts) = lay_doc(source);
        let arabic = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r).contains("قبل"))
            .expect("the arabic run");
        let (arabic_start, _) = run_interval(arabic).expect("the interval");
        let sel = Selection {
            start: ModelPos {
                byte: arabic_start.byte + 2,
                ..arabic_start
            },
            end: ModelPos {
                byte: arabic_start.byte + source.find("story").expect("the latin word"),
                ..arabic_start
            },
        };
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        assert!(boxes.len() >= 2, "one box per script side, got {boxes:?}");
        for (x, y, w, _) in &boxes {
            assert!(*w > 0.0, "every box spans left to right, got {boxes:?}");
            assert!(
                l.runs.iter().any(|r| (r.y - y).abs() < 0.5
                    && *x >= r.x - 1.0
                    && x + w <= r.x + r.width + 1.0),
                "a box stays inside its run: [{x}..{}]",
                x + w
            );
        }
        let cut = boxes
            .iter()
            .find(|(x, _, w, _)| x + w < arabic.x + arabic.width - 2.0 && *x < arabic.x + 1.0)
            .is_some();
        assert!(
            cut,
            "the arabic box starts at the run's left and stops short of its right edge, \
             the selected middle of an RTL run: {boxes:?}, run [{}..{}]",
            arabic.x,
            arabic.x + arabic.width
        );
    }

    #[test]
    fn a_selected_justified_rtl_line_is_one_full_box() {
        let source = format!("{}الغاية.\n", "اعلم أن فن التاريخ فن عزيز ".repeat(8));
        let doc = markdown::parse(source);
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig {
                justify: true,
                ..ViewConfig::default()
            },
            600.0,
        );
        let sel = select_all(&doc);
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        let mut ys: Vec<i32> = boxes.iter().map(|b| b.1.round() as i32).collect();
        ys.dedup();
        assert_eq!(
            boxes.len(),
            ys.len(),
            "a fully selected justified RTL line is one unbroken box"
        );
        for (x, y, w, _) in &boxes {
            let (left, right) = l
                .runs
                .iter()
                .filter(|r| (r.y - y).abs() < 0.5)
                .fold((f32::MAX, f32::MIN), |(lo, hi), r| {
                    (lo.min(r.x), hi.max(r.x + r.width))
                });
            assert!(
                *x <= left + 1.0 && x + w >= right - 1.0,
                "the box covers its line: box [{x}..{}], line [{left}..{right}]",
                x + w
            );
        }
    }

    #[test]
    fn a_double_click_selects_the_arabic_word() {
        let (doc, _, _) = lay_doc(RTL_LINE);
        let inside = RTL_LINE.find("أن").expect("the word") + 2;
        let sel = word_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: inside,
            },
        )
        .expect("a word");
        assert_eq!(plain_text(&sel, &doc).trim(), "أن");
    }
}
