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

/// The motion set. Bare keys only; every chord keeps its app meaning.
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

/// The source range a run displays, when display bytes equal source
/// bytes; None for markers, synthesized text, and transformed spans.
fn run_source(doc: &Document, run: &TextRun) -> Option<(usize, usize)> {
    let TextRef::Model { start, len } = run.text else {
        return None;
    };
    if run.span == MARKER_SPAN {
        return None;
    }
    let block = doc.blocks.get(run.block)?;
    let base = match &block.kind {
        BlockKind::CodeBlock { lines, .. } => lines.line_range(run.span)?.start,
        kind => {
            let span = block_spans(kind)?.get(run.span)?;
            if !span.is_verbatim() || span.range.is_empty() {
                return None;
            }
            span.range.start as usize
        }
    };
    Some((base + start as usize, len as usize))
}

/// A model position's source byte offset, for verbatim content.
fn model_offset(doc: &Document, pos: &ModelPos) -> Option<usize> {
    if pos.span == MARKER_SPAN {
        return None;
    }
    let block = doc.blocks.get(pos.block)?;
    match &block.kind {
        BlockKind::CodeBlock { lines, .. } => Some(lines.line_range(pos.span)?.start + pos.byte),
        kind => {
            let span = block_spans(kind)?.get(pos.span)?;
            if !span.is_verbatim() || span.range.is_empty() {
                return None;
            }
            Some(span.range.start as usize + pos.byte)
        }
    }
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
    lines
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
        .or_else(|| line.runs.first())
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
    if byte >= text.len() {
        return text_run.width;
    }
    let family = lay.run_family(text_run);
    let buffer = selection::shape_run(fonts, text_run, text, family);
    if let Some(line) = buffer.layout_runs().next() {
        for glyph in line.glyphs {
            if glyph.start >= byte {
                return glyph.x;
            }
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
        let lines = lines_of(lay, doc);
        let Some(li) = locate(&lines, self.offset) else {
            return self;
        };
        let line = &lines[li];
        match motion {
            Motion::Left => {
                if self.offset > line.start {
                    let text = &doc.source[line.start..line.end];
                    let local = self.offset - line.start;
                    let prev = text[..local]
                        .chars()
                        .next_back()
                        .map(|c| local - c.len_utf8())
                        .unwrap_or(0);
                    Caret::at(line.start + prev)
                } else if li > 0 {
                    Caret::at(lines[li - 1].end)
                } else {
                    Caret::at(self.offset)
                }
            }
            Motion::Right => {
                if self.offset < line.end {
                    let text = &doc.source[line.start..line.end];
                    let local = self.offset - line.start;
                    let step = text[local..].chars().next().map_or(0, |c| c.len_utf8());
                    Caret::at(line.start + local + step)
                } else if li + 1 < lines.len() {
                    Caret::at(lines[li + 1].start)
                } else {
                    Caret::at(self.offset)
                }
            }
            Motion::Home => Caret::at(line.start),
            Motion::End => Caret::at(line.end),
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                let Some(run) = run_at(line, self.offset) else {
                    return self;
                };
                let x = self
                    .goal
                    .unwrap_or_else(|| run.x + x_of(fonts, lay, doc, run, self.offset));
                let target = match motion {
                    Motion::Up if li == 0 => return self,
                    Motion::Down if li + 1 >= lines.len() => return self,
                    Motion::Up => li - 1,
                    Motion::Down => li + 1,
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
        let li = locate(&lines, self.offset)?;
        let line = &lines[li];
        let run = run_at(line, self.offset)?;
        let x = run.x + x_of(fonts, lay, doc, run, self.offset);
        Some(CaretBox {
            x,
            y: line.y,
            h: line.h,
        })
    }
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
        load::plain_document(source)
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
