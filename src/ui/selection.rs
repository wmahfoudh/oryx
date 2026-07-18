//! Text selection: caret positions in laid-out runs, hit testing for mouse
//! drags, highlight geometry, and conversion of the selected range back to
//! plain text or markdown.

use std::borrow::Cow;

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use crate::doc::model::{BlockKind, Document, Span};
use crate::layout::{metrics, LayoutDoc, TextRun};
use crate::style::fonts::FontStore;

/// Marker runs (bullets, numbers, checkmarks) carry this span sentinel and
/// take no part in selection.
const MARKER_SPAN: usize = usize::MAX;

/// A caret position: index into `LayoutDoc::runs` plus a character offset
/// within that run's text, from 0 to the run's character count inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunPos {
    pub run: usize,
    pub ch: usize,
}

/// A drag selection between two caret positions, kept in drag order;
/// `start` may sit after `end` when the drag went upward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: RunPos,
    pub end: RunPos,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Endpoints in document order, regardless of drag direction.
    fn ordered(&self) -> (RunPos, RunPos) {
        if (self.end.run, self.end.ch) < (self.start.run, self.start.ch) {
            (self.end, self.start)
        } else {
            (self.start, self.end)
        }
    }
}

/// The whole document as a selection, from the first selectable run to the
/// last. None when the document has no selectable runs.
pub fn all(lay: &LayoutDoc) -> Option<Selection> {
    let first = lay.runs.iter().position(|r| r.span != MARKER_SPAN)?;
    let last = lay.runs.iter().rposition(|r| r.span != MARKER_SPAN)?;
    Some(Selection {
        start: RunPos { run: first, ch: 0 },
        end: RunPos {
            run: last,
            ch: lay.runs[last].text.chars().count(),
        },
    })
}

/// The caret position nearest to a point in document coordinates.
/// Snaps vertically to the closest line and horizontally to the closest
/// character boundary. None only when the document has no runs.
pub fn pos_at(lay: &LayoutDoc, fonts: &mut FontStore, x: f32, y: f32) -> Option<RunPos> {
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
    Some(RunPos {
        run: index,
        ch: char_index_at(fonts, run, x - run.x),
    })
}

/// Highlight boxes for the selection, one `(x, y, width, height)` per
/// selected run fragment, in document coordinates. Boxes on the same line
/// share the height of the line's tallest run.
pub fn rects(sel: &Selection, lay: &LayoutDoc, fonts: &mut FontStore) -> Vec<(f32, f32, f32, f32)> {
    let (a, b) = sel.ordered();
    let mut out = Vec::new();
    for index in a.run..=b.run.min(lay.runs.len().saturating_sub(1)) {
        let run = &lay.runs[index];
        if run.span == MARKER_SPAN {
            continue;
        }
        let x0 = if index == a.run {
            run.x + prefix_width(fonts, run, a.ch)
        } else {
            run.x
        };
        let x1 = if index == b.run {
            run.x + prefix_width(fonts, run, b.ch)
        } else {
            run.x + run.width
        };
        if x1 <= x0 {
            continue;
        }
        let height = lay
            .runs
            .iter()
            .filter(|r| r.block == run.block && r.y == run.y)
            .map(|r| metrics::LINE_HEIGHT * r.size)
            .fold(metrics::LINE_HEIGHT * run.size, f32::max);
        out.push((x0, run.y, x1 - x0, height));
    }
    out
}

/// The selected range as unstyled text. Wrapped lines rejoin with a space,
/// hard breaks and code lines keep their newline, blocks join with a blank
/// line.
pub fn plain_text(sel: &Selection, lay: &LayoutDoc, doc: &Document) -> String {
    let mut out = String::new();
    for (index, text) in fragments(sel, lay) {
        let run = &lay.runs[index];
        if !out.is_empty() {
            out.push_str(&separator(
                doc,
                &lay.runs[prev_selected(lay, sel, index)],
                run,
            ));
        }
        out.push_str(text);
    }
    out
}

/// The selected range as markdown: one verbatim slice of the document
/// source between the two endpoints, so every construct survives exactly
/// as written. Endpoints are character-precise inside text blocks; an
/// endpoint at a block edge rounds out to whole source lines so line
/// markers (`#`, `>`, list bullets) come along, and endpoints inside code
/// blocks or tables round out to the whole block.
pub fn markdown(sel: &Selection, lay: &LayoutDoc, doc: &Document) -> String {
    if sel.is_empty() || lay.runs.is_empty() || doc.source.is_empty() {
        return String::new();
    }
    let (a, b) = sel.ordered();
    let start = source_pos(doc, lay, &a, Edge::Start);
    let end = source_pos(doc, lay, &b, Edge::End);
    let start = floor_boundary(&doc.source, start);
    let end = floor_boundary(&doc.source, end.max(start));
    doc.source[start..end].to_string()
}

enum Edge {
    Start,
    End,
}

/// Maps a selection endpoint to a byte offset in the document source.
fn source_pos(doc: &Document, lay: &LayoutDoc, pos: &RunPos, edge: Edge) -> usize {
    let index = pos.run.min(lay.runs.len() - 1);
    let run = &lay.runs[index];
    let block = &doc.blocks[run.block];
    let Some(spans) = kind_spans(&block.kind) else {
        return match edge {
            Edge::Start => line_start(&doc.source, block.range.start),
            Edge::End => line_end(&doc.source, block.range.end),
        };
    };
    let at_block_edge = match edge {
        Edge::Start => pos.ch == 0 && first_of_block(lay, index),
        Edge::End => pos.ch >= run.text.chars().count() && last_of_block(lay, index),
    };
    if at_block_edge {
        return match edge {
            Edge::Start => line_start(&doc.source, block.range.start),
            Edge::End => line_end(&doc.source, block.range.end),
        };
    }
    let span = spans.get(run.span).filter(|s| !s.range.is_empty());
    if let Some(span) = span {
        // Character precision holds only when the span survived parsing
        // verbatim and the run's slice locates uniquely inside it.
        let verbatim = doc
            .source
            .get(span.range.clone())
            .is_some_and(|s| s == span.text);
        if verbatim {
            if let Some(offset) = span.text.find(run.text.as_str()) {
                return span.range.start + offset + byte_of_char(&run.text, pos.ch);
            }
        }
        return match edge {
            Edge::Start => span.range.start,
            Edge::End => span.range.end,
        };
    }
    match edge {
        Edge::Start => line_start(&doc.source, block.range.start),
        Edge::End => line_end(&doc.source, block.range.end),
    }
}

/// True when no earlier non-marker run belongs to the same block.
fn first_of_block(lay: &LayoutDoc, index: usize) -> bool {
    lay.runs[..index]
        .iter()
        .rev()
        .find(|r| r.span != MARKER_SPAN)
        .map_or(true, |r| r.block != lay.runs[index].block)
}

/// True when no later non-marker run belongs to the same block.
fn last_of_block(lay: &LayoutDoc, index: usize) -> bool {
    lay.runs[index + 1..]
        .iter()
        .find(|r| r.span != MARKER_SPAN)
        .map_or(true, |r| r.block != lay.runs[index].block)
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
    source[byte..].find('\n').map_or(source.len(), |i| byte + i)
}

/// Clamps to length and steps back to a UTF-8 character boundary.
fn floor_boundary(source: &str, byte: usize) -> usize {
    let mut byte = byte.min(source.len());
    while !source.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// The selected pieces in document order: run index plus the slice of its
/// text inside the selection. Marker runs and empty slices are dropped.
fn fragments<'a>(sel: &Selection, lay: &'a LayoutDoc) -> Vec<(usize, &'a str)> {
    let (a, b) = sel.ordered();
    if sel.is_empty() || lay.runs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for index in a.run..=b.run.min(lay.runs.len() - 1) {
        let run = &lay.runs[index];
        if run.span == MARKER_SPAN {
            continue;
        }
        let from = if index == a.run { a.ch } else { 0 };
        let to = if index == b.run {
            b.ch
        } else {
            run.text.chars().count()
        };
        let text = slice_chars(&run.text, from, to);
        if !text.is_empty() {
            out.push((index, text));
        }
    }
    out
}

/// Index of the selected run preceding `index`, skipping markers.
fn prev_selected(lay: &LayoutDoc, sel: &Selection, index: usize) -> usize {
    let (a, _) = sel.ordered();
    let mut i = index - 1;
    while i > a.run && lay.runs[i].span == MARKER_SPAN {
        i -= 1;
    }
    i
}

/// Text joining two adjacent selected runs: nothing inside a line, a tab at
/// table cell boundaries, a space at soft wraps, a newline at hard breaks
/// and code lines, a blank line between blocks. Blank code lines leave no
/// run, so their count comes from the vertical distance.
fn separator(doc: &Document, prev: &TextRun, cur: &TextRun) -> Cow<'static, str> {
    if cur.block != prev.block {
        return "\n\n".into();
    }
    if cur.y == prev.y {
        return if cur.span <= prev.span { "\t" } else { "" }.into();
    }
    match &doc.blocks[cur.block].kind {
        BlockKind::CodeBlock { .. } => {
            let line_height = metrics::LINE_HEIGHT * cur.size;
            let lines = ((cur.y - prev.y) / line_height).round().max(1.0) as usize;
            "\n".repeat(lines).into()
        }
        BlockKind::Table { .. } => "\n".into(),
        kind => {
            let hard_break = kind_spans(kind).is_some_and(|spans| {
                spans
                    .get(prev.span + 1..cur.span)
                    .unwrap_or(&[])
                    .iter()
                    .any(|s| s.text == "\n")
            });
            if hard_break { "\n" } else { " " }.into()
        }
    }
}

fn kind_spans(kind: &BlockKind) -> Option<&[Span]> {
    match kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::FootnoteDef { spans, .. } => Some(spans),
        _ => None,
    }
}

/// Byte index of a character offset, clamped to the text end.
fn byte_of_char(text: &str, ch: usize) -> usize {
    text.char_indices()
        .nth(ch)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Slice by character offsets, clamped.
fn slice_chars(text: &str, from: usize, to: usize) -> &str {
    let start = byte_of_char(text, from);
    let end = byte_of_char(text, to.max(from));
    &text[start..end]
}

/// Shapes a run exactly as paint does, single line at its own metrics.
fn shape_run(fonts: &mut FontStore, run: &TextRun) -> Buffer {
    let line_height = metrics::LINE_HEIGHT * run.size;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(run.size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let mut attrs = Attrs::new()
        .family(Family::Name(&run.family))
        .weight(Weight(run.weight));
    if run.italic {
        attrs = attrs.style(Style::Italic);
    }
    buffer.set_text(
        &mut fonts.font_system,
        &run.text,
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    buffer
}

/// The character boundary nearest to an x offset inside a run, by glyph
/// midpoints.
fn char_index_at(fonts: &mut FontStore, run: &TextRun, x_local: f32) -> usize {
    if x_local <= 0.0 {
        return 0;
    }
    if x_local >= run.width {
        return run.text.chars().count();
    }
    let buffer = shape_run(fonts, run);
    if let Some(line) = buffer.layout_runs().next() {
        for glyph in line.glyphs {
            if x_local < glyph.x + glyph.w / 2.0 {
                return run.text[..glyph.start].chars().count();
            }
        }
    }
    run.text.chars().count()
}

/// Advance width of the first `ch` characters of a run.
fn prefix_width(fonts: &mut FontStore, run: &TextRun, ch: usize) -> f32 {
    if ch == 0 {
        return 0.0;
    }
    let byte = byte_of_char(&run.text, ch);
    if byte >= run.text.len() {
        return run.width;
    }
    let buffer = shape_run(fonts, run);
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

    fn select_all(l: &LayoutDoc) -> Selection {
        all(l).expect("layout has selectable runs")
    }

    #[test]
    fn markdown_round_trips_styles() {
        let source = "# Title\n\nplain **bold** *italic* ~~gone~~ `code` [link](https://a.tld)";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l), &l, &doc), source);
    }

    #[test]
    fn plain_text_drops_styles() {
        let source = "# Title\n\nplain **bold** *italic* ~~gone~~ `code` [link](https://a.tld)";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(
            plain_text(&select_all(&l), &l, &doc),
            "Title\n\nplain bold italic gone code link"
        );
    }

    #[test]
    fn partial_selection_joins_paragraphs_with_blank_line() {
        let source = "alpha one\n\nsecond beta";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(l.runs.len(), 2, "expected one run per paragraph");
        let sel = Selection {
            start: RunPos { run: 0, ch: 6 },
            end: RunPos { run: 1, ch: 6 },
        };
        assert_eq!(plain_text(&sel, &l, &doc), "one\n\nsecond");
    }

    #[test]
    fn all_selects_every_run() {
        let (doc, l, _) = lay_doc("# Title\n\n- item with `code`");
        let sel = all(&l).unwrap();
        assert_eq!(plain_text(&sel, &l, &doc), "Title\n\nitem with code");
        assert_eq!(markdown(&sel, &l, &doc), "# Title\n\n- item with `code`");
    }

    #[test]
    fn all_of_empty_layout_is_none() {
        assert!(all(&LayoutDoc::default()).is_none());
    }

    #[test]
    fn markdown_preserves_structure_from_source() {
        let source = "> quoted line\n\n- item one\n- item, with **bold**\n  - nested\n\n1. first\n2. second\n\n- [x] done\n- [ ] todo\n\n---\n\nafter the rule";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l), &l, &doc), source);
    }

    #[test]
    fn markdown_partial_selection_slices_characters() {
        let source = "alpha one\n\nsecond beta";
        let (doc, l, _) = lay_doc(source);
        let sel = Selection {
            start: RunPos { run: 0, ch: 6 },
            end: RunPos { run: 1, ch: 6 },
        };
        assert_eq!(markdown(&sel, &l, &doc), "one\n\nsecond");
    }

    #[test]
    fn markdown_fences_code_blocks() {
        let source = "intro\n\n```rust\nfn a() {}\n\nfn b() {}\n```\n\noutro";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l), &l, &doc), source);
    }

    #[test]
    fn markdown_fences_unlabeled_code() {
        let source = "```\nplain fence\n```";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l), &l, &doc), source);
    }

    #[test]
    fn blank_code_lines_survive_plain_copy() {
        let source = "```rust\nfn a() {}\n\nfn b() {}\n```";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(
            plain_text(&select_all(&l), &l, &doc),
            "fn a() {}\n\nfn b() {}"
        );
    }

    #[test]
    fn upward_drag_normalizes() {
        let source = "alpha one\n\nsecond beta";
        let (doc, l, _) = lay_doc(source);
        let sel = Selection {
            start: RunPos { run: 1, ch: 6 },
            end: RunPos { run: 0, ch: 6 },
        };
        assert_eq!(plain_text(&sel, &l, &doc), "one\n\nsecond");
    }

    #[test]
    fn pos_at_snaps_to_character_boundaries() {
        let (_, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let left = pos_at(&l, &mut fonts, run.x + 0.5, run.y + 1.0).unwrap();
        assert_eq!(left, RunPos { run: 0, ch: 0 });
        let right = pos_at(&l, &mut fonts, run.x + run.width + 50.0, run.y + 1.0).unwrap();
        assert_eq!(
            right,
            RunPos {
                run: 0,
                ch: run.text.chars().count()
            }
        );
    }

    #[test]
    fn rects_cover_fully_selected_run() {
        let (_, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let sel = select_all(&l);
        let boxes = rects(&sel, &l, &mut fonts);
        assert_eq!(boxes.len(), 1);
        let (x, _y, w, h) = boxes[0];
        assert!((x - run.x).abs() < 0.5);
        assert!((w - run.width).abs() < 0.5);
        assert!(h > run.size, "box covers the line height");
    }
}
