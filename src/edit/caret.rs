//! The caret: a source offset walked through the laid-out runs.
//!
//! The caret is a position in rendered text and steps only characters
//! that are on the page: a tab is one step, a multi-byte character is
//! one step, a line ending is crossed in one press. Milestone 1 covers
//! text and code files, where display bytes equal source bytes.

use crate::doc::model::{BlockKind, Document, Span};
use crate::layout::{metrics, LayoutDoc, TextRef, TextRun};
use crate::style::fonts::FontStore;
use crate::ui::selection::{self, ModelPos, Selection, MARKER_SPAN};

/// A caret anchored to a source byte offset. `goal` remembers the
/// preferred x while stepping vertically through shorter lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub offset: usize,
    pub goal: Option<f32>,
}

/// The motion set: the bare keys, plus the document and word jumps
/// every editor puts on the Ctrl chords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    DocStart,
    DocEnd,
    WordLeft,
    WordRight,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The file's final newline is a terminator, not a line: stepping past
/// it would land on a row that does not exist, so the step holds.
fn clamp_final(doc: &Document, from: usize, target: usize) -> usize {
    if target == doc.source.len() && doc.source.ends_with('\n') {
        from
    } else {
        target
    }
}

/// The next word boundary to the right: whitespace is skipped, then one
/// run of word characters or one run of symbols, newlines crossed like
/// any other whitespace.
pub fn word_right(text: &str, at: usize) -> usize {
    let mut pos = at;
    for c in text[at..].chars() {
        if !c.is_whitespace() {
            break;
        }
        pos += c.len_utf8();
    }
    let Some(first) = text[pos..].chars().next() else {
        return pos;
    };
    let class = word_char(first);
    for c in text[pos..].chars() {
        if c.is_whitespace() || word_char(c) != class {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

/// The mirror boundary to the left.
pub fn word_left(text: &str, at: usize) -> usize {
    let mut pos = at;
    for c in text[..at].chars().rev() {
        if !c.is_whitespace() {
            break;
        }
        pos -= c.len_utf8();
    }
    let Some(last) = text[..pos].chars().next_back() else {
        return pos;
    };
    let class = word_char(last);
    for c in text[..pos].chars().rev() {
        if c.is_whitespace() || word_char(c) != class {
            break;
        }
        pos -= c.len_utf8();
    }
    pos
}

/// The caret's bar on the page, in document space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretBox {
    pub x: f32,
    pub y: f32,
    pub h: f32,
}

/// One mappable run on a visual line: laid-out text whose display
/// bytes slice the source directly.
struct MappedRun {
    x: f32,
    width: f32,
    start: usize,
    len: usize,
    /// Index into `lay.runs`, for shaping.
    index: usize,
}

/// A visual line: the mappable runs sharing a y, in x order.
struct Line {
    y: f32,
    h: f32,
    start: usize,
    end: usize,
    runs: Vec<MappedRun>,
}

fn block_spans(kind: &BlockKind) -> Option<&[Span]> {
    match kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::FootnoteDef { spans, .. }
        | BlockKind::Summary { spans, .. } => Some(spans),
        _ => None,
    }
}

/// The source offset a span's display text starts at, when display
/// bytes equal source bytes: a code line's range start, or a sealed
/// span's range start. None for markers and transformed spans.
fn span_base(doc: &Document, block: usize, span: usize) -> Option<usize> {
    if span == MARKER_SPAN {
        return None;
    }
    let block = doc.blocks.get(block)?;
    match &block.kind {
        BlockKind::CodeBlock { lines, .. } => Some(lines.line_range(span)?.start),
        kind => {
            let span = block_spans(kind)?.get(span)?;
            if !span.is_verbatim() || span.range.is_empty() {
                return None;
            }
            Some(span.range.start as usize)
        }
    }
}

/// The source range a run displays; None for synthesized text.
fn run_source(doc: &Document, run: &TextRun) -> Option<(usize, usize)> {
    let TextRef::Model { start, len } = run.text else {
        return None;
    };
    let base = span_base(doc, run.block, run.span)?;
    Some((base + start as usize, len as usize))
}

/// A model position's source byte offset, for verbatim content.
pub fn model_offset(doc: &Document, pos: &ModelPos) -> Option<usize> {
    Some(span_base(doc, pos.block, pos.span)? + pos.byte)
}

/// The model position a source offset stands at, for verbatim content;
/// the inverse of `model_offset`.
pub fn model_pos(doc: &Document, offset: usize) -> Option<ModelPos> {
    for (bi, block) in doc.blocks.iter().enumerate() {
        match &block.kind {
            BlockKind::CodeBlock { lines, .. } => {
                for li in 0..lines.len() {
                    let range = lines.line_range(li)?;
                    if range.start <= offset && offset <= range.end {
                        return Some(ModelPos {
                            block: bi,
                            span: li,
                            byte: offset - range.start,
                        });
                    }
                }
            }
            kind => {
                for (si, span) in block_spans(kind)?.iter().enumerate() {
                    if !span.is_verbatim() || span.range.is_empty() {
                        continue;
                    }
                    let (start, end) = (span.range.start as usize, span.range.end as usize);
                    if start <= offset && offset <= end {
                        return Some(ModelPos {
                            block: bi,
                            span: si,
                            byte: offset - start,
                        });
                    }
                }
            }
        }
    }
    None
}

/// A selection between two source offsets, `caret` as the active end,
/// speaking the model so the rects and both copies work unchanged.
pub fn span_selection(doc: &Document, anchor: usize, caret: usize) -> Option<Selection> {
    Some(Selection {
        start: model_pos(doc, anchor)?,
        end: model_pos(doc, caret)?,
    })
}

/// The ordered source range a selection covers.
pub fn selection_range(doc: &Document, sel: &Selection) -> Option<std::ops::Range<usize>> {
    let (start, end) = sel.ordered();
    let a = model_offset(doc, &start)?;
    let b = model_offset(doc, &end)?;
    Some(a.min(b)..a.max(b))
}

/// The visual lines of every mappable run, in reading order.
fn lines_of(lay: &LayoutDoc, doc: &Document) -> Vec<Line> {
    let mut flat: Vec<(f32, f32, MappedRun)> = Vec::new();
    for (index, run) in lay.runs.iter().enumerate() {
        let Some((start, len)) = run_source(doc, run) else {
            continue;
        };
        flat.push((
            run.y,
            metrics::LINE_HEIGHT * run.size,
            MappedRun {
                x: run.x,
                width: run.width,
                start,
                len,
                index,
            },
        ));
    }
    flat.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.x.total_cmp(&b.2.x)));
    let mut lines: Vec<Line> = Vec::new();
    for (y, h, run) in flat {
        match lines.last_mut() {
            Some(line) if (line.y - y).abs() < 0.5 => {
                line.h = line.h.max(h);
                line.end = line.end.max(run.start + run.len);
                line.runs.push(run);
            }
            _ => lines.push(Line {
                y,
                h,
                start: run.start,
                end: run.start + run.len,
                runs: vec![run],
            }),
        }
    }
    // Layout trims trailing space glyphs, but their bytes are real and
    // the caret must stand after them: each line's end extends over the
    // whitespace up to its ending.
    for line in &mut lines {
        let tail = doc.source[line.end..]
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        line.end += tail;
    }
    lines
}

/// The top of the row an offset stands on. Leaving the editor seats
/// the page here, so a reader returns to the line they were editing
/// rather than to the one they came in from. None when the layout has
/// not placed that far, which the pending target answers instead.
pub fn row_top(lay: &LayoutDoc, doc: &Document, offset: usize) -> Option<f32> {
    let lines = lines_of(lay, doc);
    locate(&lines, offset).map(|i| lines[i].y)
}

/// The line holding an offset. A wrap boundary belongs to the later
/// line; a line's own end position belongs to it.
fn locate(lines: &[Line], offset: usize) -> Option<usize> {
    lines
        .iter()
        .position(|l| offset >= l.start && offset < l.end)
        .or_else(|| {
            lines
                .iter()
                .rposition(|l| offset >= l.start && offset <= l.end)
        })
}

fn run_at(line: &Line, offset: usize) -> Option<&MappedRun> {
    line.runs
        .iter()
        .find(|r| offset >= r.start && offset < r.start + r.len)
        .or_else(|| line.runs.iter().rev().find(|r| offset == r.start + r.len))
        // Past every run's text: the line's trimmed trailing whitespace.
        .or_else(|| line.runs.last())
}

/// Advance x of an offset inside a run, shaped as paint shapes.
fn x_of(
    fonts: &mut FontStore,
    lay: &LayoutDoc,
    doc: &Document,
    run: &MappedRun,
    offset: usize,
) -> f32 {
    let text_run = &lay.runs[run.index];
    let text = lay.run_text(doc, text_run);
    let byte = offset - run.start;
    if byte == 0 {
        return 0.0;
    }
    let family = lay.run_family(text_run);
    if byte > text.len() {
        // Inside the trailing whitespace layout trimmed: shape the
        // source slice so the caret advances past each space.
        if let Some(slice) = doc.source.get(run.start..offset) {
            let buffer = selection::shape_run(fonts, text_run, slice, family);
            if let Some(line) = buffer.layout_runs().next() {
                if let Some(glyph) = line.glyphs.last() {
                    return glyph.x + glyph.w;
                }
            }
        }
        return text_run.width;
    }
    if byte == text.len() {
        return text_run.width;
    }
    let buffer = selection::shape_run(fonts, text_run, text, family);
    if let Some(line) = buffer.layout_runs().next() {
        if let Some(x) = selection::boundary_x(line.glyphs, text, byte) {
            return x;
        }
    }
    text_run.width
}

/// The character boundary nearest an absolute x on a line.
fn offset_at_x(
    fonts: &mut FontStore,
    lay: &LayoutDoc,
    doc: &Document,
    line: &Line,
    x: f32,
) -> usize {
    let Some(run) = line
        .runs
        .iter()
        .find(|r| x < r.x + r.width)
        .or_else(|| line.runs.last())
    else {
        return line.start;
    };
    if x <= run.x {
        return run.start;
    }
    let text_run = &lay.runs[run.index];
    let text = lay.run_text(doc, text_run);
    let family = lay.run_family(text_run);
    let ch = selection::char_index_at(fonts, text_run, text, family, x - run.x);
    run.start + selection::byte_of_char(text, ch)
}

impl Caret {
    pub fn at(offset: usize) -> Caret {
        Caret { offset, goal: None }
    }

    /// One motion through the runs. `page_h` sizes PageUp and PageDown;
    /// motions past an edge hold still. Horizontal motions clear the
    /// goal column, vertical motions carry it.
    pub fn step(
        self,
        motion: Motion,
        lay: &LayoutDoc,
        doc: &Document,
        fonts: &mut FontStore,
        page_h: f32,
    ) -> Caret {
        // The document and word jumps are pure text rules: they need no
        // layout, and they reach blank lines the layout has no runs for.
        match motion {
            Motion::DocStart => return Caret::at(0),
            Motion::DocEnd => {
                let len = doc.source.len();
                let end = if doc.source.ends_with('\n') {
                    len - 1
                } else {
                    len
                };
                return Caret::at(end);
            }
            Motion::WordLeft => return Caret::at(word_left(&doc.source, self.offset)),
            Motion::WordRight => return Caret::at(word_right(&doc.source, self.offset)),
            _ => {}
        }
        let lines = lines_of(lay, doc);
        let Some(li) = locate(&lines, self.offset) else {
            // Between lines, on a blank row: arrows step by source
            // character so blank lines stay reachable and leavable.
            return match motion {
                Motion::Left if self.offset > 0 => {
                    let prev = doc.source[..self.offset]
                        .chars()
                        .next_back()
                        .map_or(self.offset, |c| self.offset - c.len_utf8());
                    Caret::at(prev)
                }
                Motion::Right if self.offset < doc.source.len() => {
                    let step = doc.source[self.offset..]
                        .chars()
                        .next()
                        .map_or(0, |c| c.len_utf8());
                    Caret::at(clamp_final(doc, self.offset, self.offset + step))
                }
                Motion::Up => {
                    let Some(p) = doc.source[..self.offset].rfind('\n') else {
                        return self;
                    };
                    // The row above ends at p; locating its end lands a
                    // wrapped paragraph on its last visual line.
                    let q = doc.source[..p].rfind('\n').map_or(0, |i| i + 1);
                    let goal = self.goal.unwrap_or(0.0);
                    let offset = match locate(&lines, p) {
                        Some(above) => offset_at_x(fonts, lay, doc, &lines[above], goal),
                        None => q,
                    };
                    Caret {
                        offset,
                        goal: Some(goal),
                    }
                }
                Motion::Down => {
                    let Some(n) = doc.source[self.offset..].find('\n') else {
                        return self;
                    };
                    let r = self.offset + n + 1;
                    if r == doc.source.len() && doc.source.ends_with('\n') {
                        return self;
                    }
                    let goal = self.goal.unwrap_or(0.0);
                    let offset = match locate(&lines, r) {
                        Some(below) => offset_at_x(fonts, lay, doc, &lines[below], goal),
                        None => r,
                    };
                    Caret {
                        offset,
                        goal: Some(goal),
                    }
                }
                _ => self,
            };
        };
        let line = &lines[li];
        match motion {
            // Horizontal steps are source characters: inside the line
            // they walk its glyphs, at its edges they cross the line
            // ending onto the neighboring line or a blank row.
            Motion::Left => {
                if self.offset > 0 {
                    let prev = doc.source[..self.offset]
                        .chars()
                        .next_back()
                        .map_or(self.offset, |c| self.offset - c.len_utf8());
                    Caret::at(prev)
                } else {
                    Caret::at(self.offset)
                }
            }
            Motion::Right => {
                if self.offset < doc.source.len() {
                    let step = doc.source[self.offset..]
                        .chars()
                        .next()
                        .map_or(0, |c| c.len_utf8());
                    Caret::at(clamp_final(doc, self.offset, self.offset + step))
                } else {
                    Caret::at(self.offset)
                }
            }
            Motion::Home => Caret::at(line.start),
            Motion::End => Caret::at(line.end),
            Motion::DocStart | Motion::DocEnd | Motion::WordLeft | Motion::WordRight => {
                unreachable!("returned before the line lookup")
            }
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                let Some(run) = run_at(line, self.offset) else {
                    return self;
                };
                let x = self
                    .goal
                    .unwrap_or_else(|| run.x + x_of(fonts, lay, doc, run, self.offset));
                let target = match motion {
                    // A blank row between this line and its neighbor is
                    // a stop of its own; the goal column rides along.
                    Motion::Up => {
                        if li == 0 {
                            if line.start > 0 {
                                return Caret {
                                    offset: line.start - 1,
                                    goal: Some(x),
                                };
                            }
                            return self;
                        }
                        let prev = &lines[li - 1];
                        let sep = doc.source.get(prev.end.min(line.start)..line.start);
                        if sep.is_some_and(|s| s.matches('\n').count() >= 2) {
                            return Caret {
                                offset: line.start - 1,
                                goal: Some(x),
                            };
                        }
                        li - 1
                    }
                    Motion::Down => {
                        let below = lines.get(li + 1).map_or(doc.source.len(), |l| l.start);
                        let blanks = doc
                            .source
                            .get(line.end..below.max(line.end))
                            .map_or(0, |s| s.matches('\n').count());
                        if blanks >= 2 {
                            return Caret {
                                offset: line.end + 1,
                                goal: Some(x),
                            };
                        }
                        if li + 1 >= lines.len() {
                            return self;
                        }
                        li + 1
                    }
                    _ => {
                        let goal_y = if motion == Motion::PageUp {
                            line.y - page_h
                        } else {
                            line.y + page_h
                        };
                        lines
                            .iter()
                            .enumerate()
                            .min_by(|a, b| {
                                (a.1.y - goal_y).abs().total_cmp(&(b.1.y - goal_y).abs())
                            })
                            .map_or(li, |(i, _)| i)
                    }
                };
                let offset = offset_at_x(fonts, lay, doc, &lines[target], x);
                Caret {
                    offset,
                    goal: Some(x),
                }
            }
        }
    }

    /// Where the caret stands, when its offset is on a placed line.
    pub fn geometry(
        self,
        lay: &LayoutDoc,
        doc: &Document,
        fonts: &mut FontStore,
    ) -> Option<CaretBox> {
        let lines = lines_of(lay, doc);
        if let Some(li) = locate(&lines, self.offset) {
            let line = &lines[li];
            let run = run_at(line, self.offset)?;
            let x = run.x + x_of(fonts, lay, doc, run, self.offset);
            return Some(CaretBox {
                x,
                y: line.y,
                h: line.h,
            });
        }
        // A line with no glyphs, just opened by Enter or blank between
        // paragraphs: the caret stands at the line start, advanced
        // over any whitespace the split carried, anchored to the
        // nearest text line, one advance per blank row between.
        if let Some(li) = lines.iter().rposition(|l| l.end < self.offset) {
            let line = &lines[li];
            let gap = doc.source.get(line.end..self.offset)?.matches('\n').count();
            if gap == 0 {
                return None;
            }
            let x = line.runs.first().map_or(0.0, |r| r.x)
                + line_prefix_advance(fonts, lay, doc, line, self.offset);
            return Some(CaretBox {
                x,
                y: line.y + line.h * gap as f32,
                h: line.h,
            });
        }
        // Nothing above: blank rows at the top of the file anchor to
        // the first text line below instead.
        let below = lines.iter().find(|l| l.start > self.offset)?;
        let gap = doc
            .source
            .get(self.offset..below.start)?
            .matches('\n')
            .count();
        if gap == 0 {
            return None;
        }
        let x = below.runs.first().map_or(0.0, |r| r.x)
            + line_prefix_advance(fonts, lay, doc, below, self.offset);
        Some(CaretBox {
            x,
            y: below.y - below.h * gap as f32,
            h: below.h,
        })
    }
}

/// The advance of the caret line's own prefix on a line the layout
/// holds no glyphs for: the whitespace Enter carried onto it, shaped
/// in the face of the anchor line's first run so a tab advances as a
/// tab. Zero with nothing before the caret on its line.
fn line_prefix_advance(
    fonts: &mut FontStore,
    lay: &LayoutDoc,
    doc: &Document,
    anchor: &Line,
    offset: usize,
) -> f32 {
    let head = &doc.source[..offset];
    let prefix = &head[head.rfind('\n').map_or(0, |i| i + 1)..];
    if prefix.is_empty() {
        return 0.0;
    }
    let Some(run) = anchor.runs.first() else {
        return 0.0;
    };
    let text_run = &lay.runs[run.index];
    let family = lay.run_family(text_run);
    let buffer = selection::shape_run(fonts, text_run, prefix, family);
    buffer
        .layout_runs()
        .next()
        .and_then(|line| line.glyphs.last())
        .map_or(0.0, |g| g.x + g.w)
}

/// Places the caret from a click, in document coordinates.
pub fn place(
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    x: f32,
    y: f32,
) -> Option<Caret> {
    let lines = lines_of(lay, doc);
    let li = lines
        .iter()
        .position(|l| y >= l.y && y < l.y + l.h)
        .or_else(|| {
            lines
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    (a.1.y + a.1.h / 2.0 - y)
                        .abs()
                        .total_cmp(&(b.1.y + b.1.h / 2.0 - y).abs())
                })
                .map(|(i, _)| i)
        })?;
    Some(Caret::at(offset_at_x(fonts, lay, doc, &lines[li], x)))
}

/// The landing offset on entering edit mode, in precedence order: the
/// selection's start when one exists, else the remembered offset while
/// its line is visible, else the first text position in the viewport.
pub fn landing(
    lay: &LayoutDoc,
    doc: &Document,
    selection: Option<&Selection>,
    remembered: Option<usize>,
    view_top: f32,
    view_h: f32,
) -> usize {
    if let Some(sel) = selection {
        let (start, _) = sel.ordered();
        if let Some(offset) = model_offset(doc, &start) {
            return offset;
        }
    }
    let lines = lines_of(lay, doc);
    let bottom = view_top + view_h;
    if let Some(offset) = remembered {
        if let Some(li) = locate(&lines, offset) {
            let line = &lines[li];
            if line.y >= view_top && line.y + line.h <= bottom {
                return offset;
            }
        }
    }
    lines
        .iter()
        .find(|l| l.y >= view_top && l.y + l.h <= bottom)
        .or_else(|| lines.iter().find(|l| l.y + l.h > view_top && l.y < bottom))
        .or_else(|| lines.first())
        .map_or(0, |l| l.start)
}

/// The scroll that keeps the caret in view: unchanged while the caret
/// is visible, otherwise the minimal move that reveals it.
pub fn snap(scroll_y: f32, view_h: f32, caret: CaretBox) -> f32 {
    if caret.y < scroll_y {
        caret.y
    } else if caret.y + caret.h > scroll_y + view_h {
        caret.y + caret.h - view_h
    } else {
        scroll_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::load;
    use crate::layout::{layout, TextRun, ViewConfig};
    use crate::style::theme::Theme;
    use crate::ui::selection::pos_at;
    use std::path::PathBuf;

    fn lay_of(doc: &Document) -> (LayoutDoc, FontStore) {
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        let l = layout(
            doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            2000.0,
        );
        (l, fonts)
    }

    fn text_doc(source: &str) -> Document {
        load::text_document(source)
    }

    fn md_doc(source: &str) -> Document {
        crate::doc::markdown::parse(source)
    }

    /// A rendered page with enough blocks to scroll through.
    fn page() -> Document {
        let mut src = String::new();
        for i in 0..40 {
            src.push_str(&format!("## Section {i}\n\nA paragraph of prose for section {i}, long enough to wrap once or twice across the width the tests lay out at.\n\n"));
        }
        md_doc(&src)
    }

    #[test]
    fn entering_lands_on_the_first_visible_line() {
        let doc = page();
        let (l, _) = lay_of(&doc);
        let lines = lines_of(&l, &doc);
        for probe in [0usize, 7, 23, 61] {
            let view_top = lines[probe].y;
            let offset = landing(&l, &doc, None, None, view_top, 400.0);
            assert_eq!(
                offset, lines[probe].start,
                "a view resting on row {probe} enters at that row's first character"
            );
        }
    }

    #[test]
    fn the_crossing_returns_to_the_row_it_left() {
        let doc = page();
        let (l, _) = lay_of(&doc);
        let lines = lines_of(&l, &doc);
        for probe in [0usize, 7, 23, 61] {
            let view_top = lines[probe].y;
            let offset = landing(&l, &doc, None, None, view_top, 400.0);
            assert_eq!(
                row_top(&l, &doc, offset),
                Some(view_top),
                "leaving from row {probe} seats the page back on it"
            );
        }
    }

    #[test]
    fn a_caret_moved_while_editing_seats_its_own_row() {
        let doc = page();
        let (l, _) = lay_of(&doc);
        let lines = lines_of(&l, &doc);
        let moved = lines[30].start + 2;
        assert_eq!(
            row_top(&l, &doc, moved),
            Some(lines[30].y),
            "the row the caret ended on, not the one the crossing began at"
        );
    }

    /// Code lines shape in the monospace face, so columns align exactly
    /// across lines and the goal-column assertions hold to the pixel.
    fn code_doc(source: &str) -> Document {
        load::code_document(Some("rust"), source)
    }

    fn run<'l>(l: &'l LayoutDoc, doc: &Document, text: &str) -> &'l TextRun {
        l.runs
            .iter()
            .find(|r| l.run_text(doc, r) == text)
            .unwrap_or_else(|| panic!("no run shows {text:?}"))
    }

    fn at(doc: &Document, needle: &str) -> usize {
        doc.source
            .find(needle)
            .unwrap_or_else(|| panic!("source lacks {needle:?}"))
    }

    fn lines(n: usize) -> String {
        (0..n).map(|i| format!("line {i:02}\n")).collect()
    }

    fn step(c: Caret, m: Motion, l: &LayoutDoc, doc: &Document, fonts: &mut FontStore) -> Caret {
        c.step(m, l, doc, fonts, 0.0)
    }

    #[test]
    fn the_caret_steps_by_character_and_crosses_line_ends() {
        let doc = code_doc("alpha alpha\nx\nbeta beta\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = step(Caret::at(0), Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 1);
        let end = at(&doc, "alpha alpha") + "alpha alpha".len();
        let c = step(Caret::at(end), Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(
            c.offset,
            at(&doc, "x"),
            "right at a line end lands on the next line start"
        );
        let c = step(Caret::at(at(&doc, "x")), Motion::Left, &l, &doc, &mut fonts);
        assert_eq!(
            c.offset, end,
            "left at a line start lands on the previous line end"
        );
    }

    #[test]
    fn motions_clamp_at_the_document_edges() {
        let doc = code_doc("alpha alpha\nx\nbeta beta\n");
        let (l, mut fonts) = lay_of(&doc);
        assert_eq!(
            step(Caret::at(0), Motion::Left, &l, &doc, &mut fonts).offset,
            0
        );
        let end = at(&doc, "beta beta") + "beta beta".len();
        assert_eq!(
            step(Caret::at(end), Motion::Right, &l, &doc, &mut fonts).offset,
            end
        );
        assert_eq!(
            step(Caret::at(5), Motion::Up, &l, &doc, &mut fonts).offset,
            5,
            "up on the first line holds still"
        );
        assert_eq!(
            step(Caret::at(end - 2), Motion::Down, &l, &doc, &mut fonts).offset,
            end - 2,
            "down on the last line holds still"
        );
    }

    #[test]
    fn a_tab_and_a_two_byte_character_are_one_step_each() {
        let doc = code_doc("\tcafé x\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = step(Caret::at(0), Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 1, "the tab is one step");
        let e = at(&doc, "é");
        let c = step(Caret::at(e), Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(c.offset, e + "é".len(), "a two-byte character is one step");
        let c = step(Caret::at(e + "é".len()), Motion::Left, &l, &doc, &mut fonts);
        assert_eq!(c.offset, e);
    }

    #[test]
    fn up_and_down_keep_the_goal_column_through_a_short_line() {
        let doc = code_doc("alpha alpha\nx\nbeta beta\n");
        let (l, mut fonts) = lay_of(&doc);
        let short_end = at(&doc, "x") + 1;
        let c = step(Caret::at(8), Motion::Down, &l, &doc, &mut fonts);
        assert_eq!(c.offset, short_end, "the short line clamps to its end");
        let c = step(c, Motion::Down, &l, &doc, &mut fonts);
        assert_eq!(
            c.offset,
            at(&doc, "beta beta") + 8,
            "the goal column returns past the short line"
        );
        let c = step(c, Motion::Up, &l, &doc, &mut fonts);
        assert_eq!(c.offset, short_end);
        let c = step(c, Motion::Up, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 8, "the goal column survives the round trip");
    }

    #[test]
    fn the_caret_stands_on_a_just_opened_empty_line() {
        let doc = code_doc("a\n\nb\n");
        let (l, mut fonts) = lay_of(&doc);
        let a = run(&l, &doc, "a");
        let b = run(&l, &doc, "b");
        let void = Caret::at(2)
            .geometry(&l, &doc, &mut fonts)
            .expect("an empty line still seats the caret");
        assert_eq!(void.x, a.x, "the empty line starts where lines start");
        let advance = b.y - a.y;
        assert_eq!(
            void.y,
            a.y + advance / 2.0,
            "the empty line sits between its neighbors"
        );
        let doc = text_doc("alpha\n\n");
        let (l, mut fonts) = lay_of(&doc);
        let alpha = run(&l, &doc, "alpha");
        let tail = Caret::at(6)
            .geometry(&l, &doc, &mut fonts)
            .expect("the line opened past the text seats the caret");
        assert_eq!(tail.x, alpha.x);
        assert!(tail.y > alpha.y, "the new line stands below the last text");
    }

    // The line Enter just opened holds nothing but the carried
    // indentation; the layout trims it to no glyphs at all, and the
    // caret must still stand at its column, not at the line start.
    #[test]
    fn the_caret_advances_over_a_whitespace_only_line() {
        let doc = code_doc("    deep\n    \nnext\n");
        let (l, mut fonts) = lay_of(&doc);
        let column = Caret::at(at(&doc, "deep"))
            .geometry(&l, &doc, &mut fonts)
            .expect("the indented text seats the caret");
        let held = Caret::at(13)
            .geometry(&l, &doc, &mut fonts)
            .expect("the whitespace line seats the caret");
        assert!(
            (held.x - column.x).abs() < 0.5,
            "four carried spaces stand the caret under the line above: {} vs {}",
            held.x,
            column.x
        );

        let doc = code_doc("\tdeep\n\t\nnext\n");
        let (l, mut fonts) = lay_of(&doc);
        let column = Caret::at(at(&doc, "deep"))
            .geometry(&l, &doc, &mut fonts)
            .expect("the tabbed text seats the caret");
        let held = Caret::at(7)
            .geometry(&l, &doc, &mut fonts)
            .expect("the tab-only line seats the caret");
        assert!(
            (held.x - column.x).abs() < 0.5,
            "a carried tab advances the caret: {} vs {}",
            held.x,
            column.x
        );
    }

    #[test]
    fn offsets_round_trip_through_model_positions() {
        let doc = text_doc("alpha\n\nbeta é x\n");
        for offset in [0, 3, 5, 6, 7, 12, 16] {
            let pos = model_pos(&doc, offset)
                .unwrap_or_else(|| panic!("offset {offset} has a model position"));
            assert_eq!(
                model_offset(&doc, &pos),
                Some(offset),
                "offset {offset} survives the round trip"
            );
        }
    }

    #[test]
    fn a_selection_spans_source_offsets_in_either_direction() {
        let doc = text_doc("alpha\nbeta\n");
        let back = span_selection(&doc, 8, 2).expect("a selection");
        assert_eq!(selection_range(&doc, &back), Some(2..8));
        assert_eq!(
            model_offset(&doc, &back.end),
            Some(2),
            "the caret side stays the active end"
        );
        let fwd = span_selection(&doc, 2, 8).expect("a selection");
        assert_eq!(selection_range(&doc, &fwd), Some(2..8));
        assert_eq!(
            selection::plain_text(&fwd, &doc),
            "pha\nbe",
            "the existing copy machinery reads it character-exact"
        );
    }

    #[test]
    fn select_all_covers_the_text_and_keeps_the_terminator() {
        let doc = text_doc("alpha\nbeta\n");
        let sel = selection::all(&doc).expect("selectable content");
        assert_eq!(selection_range(&doc, &sel), Some(0..10));
    }

    #[test]
    fn the_caret_stands_after_a_trailing_space() {
        for doc in [code_doc("word \nnext\n"), text_doc("word \nnext\n")] {
            let (l, mut fonts) = lay_of(&doc);
            let before = Caret::at(4)
                .geometry(&l, &doc, &mut fonts)
                .expect("the word end seats the caret");
            let after = Caret::at(5)
                .geometry(&l, &doc, &mut fonts)
                .expect("the position after a trailing space seats the caret");
            assert!(
                after.x > before.x,
                "the space advances the caret: {} then {}",
                before.x,
                after.x
            );
        }
    }

    #[test]
    fn the_caret_shows_on_a_blank_first_line() {
        let doc = text_doc("\nalpha\n");
        let (l, mut fonts) = lay_of(&doc);
        let alpha = run(&l, &doc, "alpha");
        let first = Caret::at(0)
            .geometry(&l, &doc, &mut fonts)
            .expect("a blank first line seats the caret");
        assert_eq!(first.x, alpha.x);
        assert!(first.y < alpha.y, "the blank line stands above the text");
    }

    #[test]
    fn the_document_jumps_include_leading_and_trailing_blanks() {
        let doc = text_doc("\nalpha\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = Caret::at(3).step(Motion::DocStart, &l, &doc, &mut fonts, 0.0);
        assert_eq!(c.offset, 0, "the jump reaches the blank first line");
        let doc = text_doc("alpha\n\n\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = Caret::at(2).step(Motion::DocEnd, &l, &doc, &mut fonts, 0.0);
        assert_eq!(c.offset, 7, "the jump reaches the last blank line");
    }

    #[test]
    fn up_and_down_work_on_a_blank_line() {
        let doc = code_doc("a\n\nb\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = step(Caret::at(2), Motion::Up, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 0, "up from the blank line reaches the line above");
        let c = step(Caret::at(2), Motion::Down, &l, &doc, &mut fonts);
        assert_eq!(
            c.offset, 3,
            "down from the blank line reaches the line below"
        );
    }

    #[test]
    fn vertical_motion_stops_at_blank_lines_and_keeps_the_goal() {
        let doc = code_doc("alpha\n\nbeta\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = step(Caret::at(2), Motion::Down, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 6, "down lands on the blank row, not past it");
        let c = step(c, Motion::Down, &l, &doc, &mut fonts);
        assert_eq!(
            c.offset,
            at(&doc, "beta") + 2,
            "the goal column survives the blank row"
        );
        let c = step(c, Motion::Up, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 6);
        let c = step(c, Motion::Up, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 2, "the round trip lands home");
    }

    #[test]
    fn arrows_walk_through_blank_lines() {
        let doc = code_doc("a\n\nb\n");
        let (l, mut fonts) = lay_of(&doc);
        let c = step(Caret::at(1), Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 2, "right from a line end enters the blank line");
        let c = step(c, Motion::Right, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 3, "right from the blank line reaches the next");
        let c = step(c, Motion::Left, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 2);
        let c = step(c, Motion::Left, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 1);
    }

    #[test]
    fn word_jumps_hop_words_symbols_and_lines() {
        let text = "let x = a1_b;\n";
        assert_eq!(word_right(text, 0), 3, "a word run is one hop");
        assert_eq!(word_right(text, 3), 5, "the space before x is skipped");
        assert_eq!(word_right(text, 5), 7, "a symbol run is one hop");
        assert_eq!(word_right(text, 7), 12);
        assert_eq!(word_right(text, 12), 13);
        assert_eq!(word_right(text, 13), 14, "the jump clamps at the end");
        assert_eq!(word_left(text, 12), 8, "back to the word start");
        assert_eq!(word_left(text, 8), 6, "back over the symbol");
        assert_eq!(word_left(text, 4), 0);
        assert_eq!(word_left(text, 0), 0);
    }

    #[test]
    fn the_document_jumps_reach_both_ends() {
        let doc = code_doc("alpha alpha\nx\nbeta beta\n");
        let (l, mut fonts) = lay_of(&doc);
        let mid = at(&doc, "x");
        let c = step(Caret::at(mid), Motion::DocStart, &l, &doc, &mut fonts);
        assert_eq!(c.offset, 0, "the jump lands on the first text position");
        let end = at(&doc, "beta beta") + "beta beta".len();
        let c = step(Caret::at(mid), Motion::DocEnd, &l, &doc, &mut fonts);
        assert_eq!(c.offset, end, "the jump lands past the last character");
    }

    #[test]
    fn home_and_end_bound_the_line() {
        let doc = code_doc("alpha alpha\nx\nbeta beta\n");
        let (l, mut fonts) = lay_of(&doc);
        let start = at(&doc, "beta beta");
        let c = step(Caret::at(start + 5), Motion::Home, &l, &doc, &mut fonts);
        assert_eq!(c.offset, start);
        let c = step(Caret::at(start + 5), Motion::End, &l, &doc, &mut fonts);
        assert_eq!(c.offset, start + "beta beta".len());
    }

    #[test]
    fn page_motions_stride_the_viewport_and_clamp() {
        let source = lines(30);
        let doc = code_doc(&source);
        let (l, mut fonts) = lay_of(&doc);
        let advance = run(&l, &doc, "line 01").y - run(&l, &doc, "line 00").y;
        let page = advance * 10.0;
        let c =
            Caret::at(at(&doc, "line 02") + 3).step(Motion::PageDown, &l, &doc, &mut fonts, page);
        assert_eq!(c.offset, at(&doc, "line 12") + 3);
        let c = Caret::at(at(&doc, "line 12") + 3).step(Motion::PageUp, &l, &doc, &mut fonts, page);
        assert_eq!(c.offset, at(&doc, "line 02") + 3);
        let c =
            Caret::at(at(&doc, "line 25") + 3).step(Motion::PageDown, &l, &doc, &mut fonts, page);
        assert_eq!(
            c.offset,
            at(&doc, "line 29") + 3,
            "a page past the end clamps to the last line"
        );
    }

    #[test]
    fn a_click_places_the_caret_at_the_character() {
        let source = lines(8);
        let doc = text_doc(&source);
        let (l, mut fonts) = lay_of(&doc);
        let r = run(&l, &doc, "line 05");
        let c = place(&l, &doc, &mut fonts, r.x + 0.5, r.y + 1.0).expect("a line answers a click");
        assert_eq!(c.offset, at(&doc, "line 05"));
        let c = place(&l, &doc, &mut fonts, r.x + r.width + 40.0, r.y + 1.0).unwrap();
        assert_eq!(
            c.offset,
            at(&doc, "line 05") + "line 05".len(),
            "past the line's end is the line end"
        );
    }

    #[test]
    fn the_caret_stands_where_its_line_starts() {
        let source = lines(8);
        let doc = text_doc(&source);
        let (l, mut fonts) = lay_of(&doc);
        let r = run(&l, &doc, "line 05");
        let b = Caret::at(at(&doc, "line 05"))
            .geometry(&l, &doc, &mut fonts)
            .expect("a placed offset has a box");
        assert_eq!(b.x, r.x);
        assert_eq!(b.y, r.y);
        assert!(b.h > 0.0);
        let mid = Caret::at(at(&doc, "line 05") + 3)
            .geometry(&l, &doc, &mut fonts)
            .unwrap();
        assert!(mid.x > r.x, "a mid-line caret stands past the line start");
    }

    #[test]
    fn landing_prefers_the_selection_start() {
        let source = lines(30);
        let doc = text_doc(&source);
        let (l, mut fonts) = lay_of(&doc);
        let r = run(&l, &doc, "line 11");
        let start = pos_at(&l, &doc, &mut fonts, r.x + 0.5, r.y + 1.0).unwrap();
        let end = pos_at(&l, &doc, &mut fonts, r.x + r.width + 40.0, r.y + 1.0).unwrap();
        let sel = Selection { start, end };
        let view_top = run(&l, &doc, "line 20").y;
        let got = landing(
            &l,
            &doc,
            Some(&sel),
            Some(at(&doc, "line 22")),
            view_top,
            2000.0,
        );
        assert_eq!(
            got,
            at(&doc, "line 11"),
            "the selection start wins even off screen"
        );
    }

    #[test]
    fn landing_recalls_the_remembered_offset_while_visible() {
        let source = lines(30);
        let doc = text_doc(&source);
        let (l, _fonts) = lay_of(&doc);
        let advance = run(&l, &doc, "line 01").y - run(&l, &doc, "line 00").y;
        let view_top = run(&l, &doc, "line 10").y - 0.4 * advance;
        let got = landing(
            &l,
            &doc,
            None,
            Some(at(&doc, "line 12")),
            view_top,
            advance * 6.0,
        );
        assert_eq!(got, at(&doc, "line 12"));
    }

    #[test]
    fn an_off_screen_memory_falls_to_the_first_visible_position() {
        let source = lines(30);
        let doc = text_doc(&source);
        let (l, _fonts) = lay_of(&doc);
        let advance = run(&l, &doc, "line 01").y - run(&l, &doc, "line 00").y;
        let view_top = run(&l, &doc, "line 10").y - 0.4 * advance;
        let got = landing(
            &l,
            &doc,
            None,
            Some(at(&doc, "line 25")),
            view_top,
            advance * 6.0,
        );
        assert_eq!(
            got,
            at(&doc, "line 10"),
            "the first visible line answers when the memory is off screen"
        );
    }

    #[test]
    fn a_blank_page_lands_at_the_origin() {
        let doc = text_doc("");
        let (l, _fonts) = lay_of(&doc);
        assert_eq!(landing(&l, &doc, None, None, 0.0, 500.0), 0);
    }

    #[test]
    fn a_visible_caret_leaves_the_scroll_alone() {
        let caret = CaretBox {
            x: 0.0,
            y: 300.0,
            h: 20.0,
        };
        assert_eq!(snap(100.0, 500.0, caret), 100.0);
    }

    #[test]
    fn an_off_screen_caret_snaps_the_view_minimally() {
        let above = CaretBox {
            x: 0.0,
            y: 50.0,
            h: 20.0,
        };
        assert_eq!(
            snap(100.0, 500.0, above),
            50.0,
            "above the view, the caret line tops it"
        );
        let below = CaretBox {
            x: 0.0,
            y: 620.0,
            h: 20.0,
        };
        assert_eq!(
            snap(100.0, 500.0, below),
            140.0,
            "below the view, the caret line closes it"
        );
    }
}
