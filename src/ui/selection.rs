//! Text selection anchored on the document model: hit testing maps a
//! click through the laid-out runs to a model position, highlight
//! geometry maps the model range back onto whatever runs are placed,
//! and both copies assemble from the model alone, so no operation here
//! needs a complete layout.

use std::borrow::Cow;
use std::cmp::Ordering;

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use crate::doc::model::{BlockKind, Document, Span};
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
        BlockKind::FootnoteDef { label, spans } => {
            out.push(Piece::Label(format!("{label}.\t")));
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
        BlockKind::Table { header, rows } => {
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
        BlockKind::Rule | BlockKind::Image { .. } | BlockKind::ChapterBreak { .. } => {}
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

/// The whole document as a selection, from its first addressable piece
/// to its last. None when nothing is selectable.
pub fn all(doc: &Document) -> Option<Selection> {
    let mut start: Option<ModelPos> = None;
    let mut end: Option<ModelPos> = None;
    for index in 0..doc.blocks.len() {
        for piece in block_pieces(doc, index) {
            if let Piece::Addr { span, text } = piece {
                let here = ModelPos {
                    block: index,
                    span,
                    byte: 0,
                };
                if start.is_none() {
                    start = Some(here);
                }
                end = Some(ModelPos {
                    block: index,
                    span,
                    byte: text.len(),
                });
            }
        }
    }
    Some(Selection {
        start: start?,
        end: end?,
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
        for piece in block_pieces(doc, index) {
            match piece {
                Piece::Sep(sep) => {
                    if !block_text.is_empty() {
                        pending_sep = Some(match pending_sep {
                            Some(prev) => prev + sep,
                            None => sep.to_string(),
                        });
                    }
                }
                Piece::Label(label) => {
                    if block_text.is_empty() {
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
                    if index == b.block {
                        match span.cmp(&b.span) {
                            Ordering::Greater => continue,
                            Ordering::Equal => to = b.byte.min(text.len()),
                            Ordering::Less => {}
                        }
                    }
                    if from >= to {
                        continue;
                    }
                    let slice = &text[floor_boundary(&text, from)..floor_boundary(&text, to)];
                    if let Some(sep) = pending_sep.take() {
                        if !block_text.is_empty() {
                            block_text.push_str(&sep);
                        }
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
/// Character classes word expansion runs over: letters, digits and
/// underscores make words, whitespace makes gaps, anything else stands
/// alone.
fn word_class(c: char) -> u8 {
    if c.is_alphanumeric() || c == '_' {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
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
    let next_c = flat[at..].chars().next();
    let prev_c = flat[..at].chars().next_back();
    let anchor = match (next_c, prev_c) {
        (Some(n), Some(p)) if word_class(n) != 0 && word_class(p) == 0 => p,
        (Some(n), _) => n,
        (None, Some(p)) => p,
        (None, None) => return None,
    };
    let class = word_class(anchor);
    let (mut start, mut end) = (at, at);
    if class == 2 {
        if next_c == Some(anchor) {
            end += anchor.len_utf8();
        } else {
            start -= anchor.len_utf8();
        }
    } else {
        while let Some(c) = flat[..start].chars().next_back() {
            if word_class(c) != class {
                break;
            }
            start -= c.len_utf8();
        }
        while let Some(c) = flat[end..].chars().next() {
            if word_class(c) != class {
                break;
            }
            end += c.len_utf8();
        }
    }
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
pub fn match_tops(lay: &LayoutDoc, matches: &[Selection]) -> Vec<f32> {
    let mut tops = vec![f32::MAX; matches.len()];
    for run in &lay.runs {
        let Some((iv_start, iv_end)) = run_interval(run) else {
            continue;
        };
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
pub fn match_anchor(lay: &LayoutDoc, m: &Selection) -> Option<(f32, f32)> {
    let (a, b) = m.ordered();
    let mut best: Option<(f32, f32)> = None;
    for run in &lay.runs {
        let Some((s, e)) = run_interval(run) else {
            continue;
        };
        if e <= a || b <= s {
            continue;
        }
        if best.map_or(true, |(y, _)| run.y < y) {
            best = Some((run.y, run.size));
        }
    }
    best
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
    let mut out: Vec<(f32, f32, f32, f32)> = Vec::new();
    // The previous box's interval end and line top, kept while that box
    // reached its run's right edge, so a byte-contiguous neighbor on the
    // same line can bridge the gap justification stretched between them.
    let mut prev: Option<(ModelPos, f32)> = None;
    for index in indices {
        let run = &lay.runs[index];
        let Some((iv_start, iv_end)) = run_interval(run) else {
            prev = None;
            continue;
        };
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
        let x0 = if precise && iv_start < a {
            let byte = a.byte.saturating_sub(run_base).min(text.len());
            let ch = text[..floor_boundary(text, byte)].chars().count();
            run.x + prefix_width(cache, fonts, index, run, text, family, ch)
        } else {
            run.x
        };
        let x1 = if precise && b < iv_end {
            let byte = b.byte.saturating_sub(run_base).min(text.len());
            let ch = text[..floor_boundary(text, byte)].chars().count();
            run.x + prefix_width(cache, fonts, index, run, text, family, ch)
        } else {
            run.x + run.width
        };
        if x1 <= x0 {
            prev = None;
            continue;
        }
        let (head, tail) = lay.runs_in(run.y, run.y);
        let height = lay.runs[head]
            .iter()
            .chain(&lay.runs[tail])
            .filter(|r| r.block == run.block && r.y == run.y)
            .map(|r| metrics::LINE_HEIGHT * r.size)
            .fold(metrics::LINE_HEIGHT * run.size, f32::max);
        // Justified lines split words into separate runs with stretched
        // gaps between them; when the selection covers the seam on both
        // sides, this box merges into the previous one, so a selected
        // line highlights as one unbroken box.
        let seam = prev;
        prev = (b >= iv_end).then_some((iv_end, run.y));
        if let Some((prev_end, prev_y)) = seam {
            if prev_y == run.y && prev_end == iv_start && a <= iv_start {
                if let Some(last) = out.last_mut() {
                    if last.1 == run.y {
                        last.2 = (x1 - last.0).max(last.2);
                        continue;
                    }
                }
            }
        }
        out.push((x0, run.y, x1 - x0, height));
    }
    out
}

fn byte_of_char(text: &str, ch: usize) -> usize {
    text.char_indices()
        .nth(ch)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Shapes a run exactly as paint does, single line at its own metrics.
fn shape_run(fonts: &mut FontStore, run: &TextRun, text: &str, family: &str) -> Buffer {
    let line_height = metrics::LINE_HEIGHT * run.size;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(run.size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let mut attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(Weight(run.weight));
    if run.italic {
        attrs = attrs.style(Style::Italic);
    }
    buffer.set_text(
        &mut fonts.font_system,
        text,
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    buffer
}

/// The character boundary nearest to an x offset inside a run, by glyph
/// midpoints.
fn char_index_at(
    fonts: &mut FontStore,
    run: &TextRun,
    text: &str,
    family: &str,
    x_local: f32,
) -> usize {
    if x_local <= 0.0 {
        return 0;
    }
    if x_local >= run.width {
        return text.chars().count();
    }
    let buffer = shape_run(fonts, run, text, family);
    if let Some(line) = buffer.layout_runs().next() {
        for glyph in line.glyphs {
            if x_local < glyph.x + glyph.w / 2.0 {
                return text[..glyph.start].chars().count();
            }
        }
    }
    text.chars().count()
}

/// Advance width of the first `ch` characters of a run, shaping through
/// the cache so a run shapes once per pass however often it is asked.
#[allow(clippy::too_many_arguments)]
fn prefix_width(
    cache: &mut ShapeCache,
    fonts: &mut FontStore,
    index: usize,
    run: &TextRun,
    text: &str,
    family: &str,
    ch: usize,
) -> f32 {
    if ch == 0 {
        return 0.0;
    }
    let byte = byte_of_char(text, ch);
    if byte >= text.len() {
        return run.width;
    }
    let buffer = cache
        .buffers
        .entry(index)
        .or_insert_with(|| shape_run(fonts, run, text, family));
    if let Some(line) = buffer.layout_runs().next() {
        for glyph in line.glyphs {
            if glyph.start >= byte {
                return glyph.x;
            }
        }
    }
    run.width
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::markdown;
    use crate::layout::{layout, ViewConfig};
    use crate::style::theme::Theme;
    use std::path::PathBuf;

    fn lay_doc(source: &str) -> (Document, LayoutDoc, FontStore) {
        let doc = markdown::parse(source);
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
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

    fn select_all(doc: &Document) -> Selection {
        all(doc).expect("document has selectable content")
    }

    #[test]
    fn justified_selection_bridges_word_gaps() {
        let doc = markdown::parse(format!("{}end.\n", "justify word ".repeat(30)));
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
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
        let (doc, lay, mut fonts) =
            lay_doc("the word the word the word here.\n\n".repeat(8).as_str());
        let matches = crate::ui::search::matches(&doc, "word");
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
}
