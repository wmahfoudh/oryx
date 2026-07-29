//! Text selection: caret positions in laid-out runs, hit testing for mouse
//! drags, highlight geometry, and conversion of the selected range back to
//! plain text or markdown.

use std::borrow::Cow;

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use crate::doc::model::{BlockKind, Document, Span};
use crate::layout::{metrics, LayoutDoc, TextRun};
use crate::style::fonts::FontStore;

/// Marker runs (bullets, numbers, checkmarks) carry this span sentinel and
/// take no part in selection or search.
pub const MARKER_SPAN: usize = usize::MAX;

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
pub fn all(lay: &LayoutDoc, doc: &Document) -> Option<Selection> {
    let first = lay.runs.iter().position(|r| r.span != MARKER_SPAN)?;
    let last = lay.runs.iter().rposition(|r| r.span != MARKER_SPAN)?;
    Some(Selection {
        start: RunPos { run: first, ch: 0 },
        end: RunPos {
            run: last,
            ch: lay.run_text(doc, &lay.runs[last]).chars().count(),
        },
    })
}

/// The caret position nearest to a point in document coordinates.
/// Snaps vertically to the closest line and horizontally to the closest
/// character boundary. None only when the document has no runs.
pub fn pos_at(
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    x: f32,
    y: f32,
) -> Option<RunPos> {
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
    let text = lay.run_text(doc, run);
    let family = lay.run_family(run);
    Some(RunPos {
        run: index,
        ch: char_index_at(fonts, run, text, family, x - run.x),
    })
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

/// The selected range as unstyled text. Wrapped lines rejoin with a space,
/// hard breaks and code lines keep their newline, blocks join with a blank
/// line.
pub fn plain_text(sel: &Selection, lay: &LayoutDoc, doc: &Document) -> String {
    let mut out = String::new();
    for (index, text) in fragments(sel, lay, doc) {
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
    let mut start = source_pos(doc, lay, &a, Edge::Start);
    let mut end = source_pos(doc, lay, &b, Edge::End);
    // Layout order is not source order: footnote definitions place at
    // the end as the notes section while living mid-document. The
    // endpoints keep their character precision; every block strictly
    // inside the selection widens the slice to its whole source lines,
    // so a selection over reordered blocks never drops the source tail.
    let lo = a.run.min(lay.runs.len() - 1);
    let hi = b.run.min(lay.runs.len() - 1);
    let edge_blocks = (lay.runs[lo].block, lay.runs[hi].block);
    let mut covered = usize::MAX;
    for run in &lay.runs[lo..=hi] {
        if run.span == MARKER_SPAN || run.block == covered {
            continue;
        }
        covered = run.block;
        if covered == edge_blocks.0 || covered == edge_blocks.1 {
            continue;
        }
        let range = &doc.blocks[covered].range;
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
    let run_text = lay.run_text(doc, run);
    let at_block_edge = match edge {
        Edge::Start => pos.ch == 0 && first_of_block(lay, index),
        Edge::End => pos.ch >= run_text.chars().count() && last_of_block(lay, index),
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
        if span.is_verbatim() {
            let text = span.text(&doc.source);
            if let Some(offset) = text.find(run_text) {
                return span.range.start as usize + offset + byte_of_char(run_text, pos.ch);
            }
        }
        return match edge {
            Edge::Start => span.range.start as usize,
            Edge::End => span.range.end as usize,
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

/// The selected pieces in document order: run index plus the slice of its
/// text inside the selection. Marker runs and empty slices are dropped.
fn fragments<'a>(sel: &Selection, lay: &'a LayoutDoc, doc: &'a Document) -> Vec<(usize, &'a str)> {
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
        let run_text = lay.run_text(doc, run);
        let from = if index == a.run { a.ch } else { 0 };
        let to = if index == b.run {
            b.ch
        } else {
            run_text.chars().count()
        };
        let text = slice_chars(run_text, from, to);
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
                    .any(|s| s.text(&doc.source) == "\n")
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

/// As `rects`, sharing shaped runs through the cache across calls. The
/// line-height lookup rides the y index, since it only needs the runs
/// on the fragment's own line.
pub fn rects_cached(
    sel: &Selection,
    lay: &LayoutDoc,
    doc: &Document,
    fonts: &mut FontStore,
    cache: &mut ShapeCache,
) -> Vec<(f32, f32, f32, f32)> {
    let (a, b) = sel.ordered();
    let mut out = Vec::new();
    for index in a.run..=b.run.min(lay.runs.len().saturating_sub(1)) {
        let run = &lay.runs[index];
        if run.span == MARKER_SPAN {
            continue;
        }
        let text = lay.run_text(doc, run);
        let family = lay.run_family(run);
        let x0 = if index == a.run {
            run.x + prefix_width(cache, fonts, index, run, text, family, a.ch)
        } else {
            run.x
        };
        let x1 = if index == b.run {
            run.x + prefix_width(cache, fonts, index, run, text, family, b.ch)
        } else {
            run.x + run.width
        };
        if x1 <= x0 {
            continue;
        }
        let (head, tail) = lay.runs_in(run.y, run.y);
        let height = lay.runs[head]
            .iter()
            .chain(&lay.runs[tail])
            .filter(|r| r.block == run.block && r.y == run.y)
            .map(|r| metrics::LINE_HEIGHT * r.size)
            .fold(metrics::LINE_HEIGHT * run.size, f32::max);
        out.push((x0, run.y, x1 - x0, height));
    }
    out
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

    #[test]
    fn cached_match_rects_equal_direct_and_share_shapings() {
        let doc = markdown::parse("the word the word the word here.\n\n".repeat(8).as_str());
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(std::path::PathBuf::from("."));
        let lay = layout(
            &doc,
            &crate::style::theme::Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            500.0,
        );
        let matches = crate::ui::search::matches(&lay, &doc, "word");
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

    fn select_all(l: &LayoutDoc, doc: &Document) -> Selection {
        all(l, doc).expect("layout has selectable runs")
    }

    #[test]
    fn markdown_round_trips_styles() {
        let source = "# Title\n\nplain **bold** *italic* ~~gone~~ `code` [link](https://a.tld)";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l, &doc), &l, &doc), source);
    }

    #[test]
    fn plain_text_drops_styles() {
        let source = "# Title\n\nplain **bold** *italic* ~~gone~~ `code` [link](https://a.tld)";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(
            plain_text(&select_all(&l, &doc), &l, &doc),
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
        let sel = all(&l, &doc).unwrap();
        assert_eq!(plain_text(&sel, &l, &doc), "Title\n\nitem with code");
        assert_eq!(markdown(&sel, &l, &doc), "# Title\n\n- item with `code`");
    }

    #[test]
    fn all_of_empty_layout_is_none() {
        assert!(all(&LayoutDoc::default(), &Document::default()).is_none());
    }

    #[test]
    fn markdown_preserves_structure_from_source() {
        let source = "> quoted line\n\n- item one\n- item, with **bold**\n  - nested\n\n1. first\n2. second\n\n- [x] done\n- [ ] todo\n\n---\n\nafter the rule";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l, &doc), &l, &doc), source);
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

    // Footnote definitions lay out at the end as the notes section, so a
    // select-all's last run belongs to a block that sits mid-source; the
    // slice must still cover every selected block.
    #[test]
    fn markdown_select_all_covers_blocks_laid_out_of_source_order() {
        let source =
            "body one.\n\nA claim[^n] made here.\n\n[^n]: The note text.\n\nbody two ends here.\n";
        let (doc, l, _) = lay_doc(source);
        let sel = all(&l, &doc).expect("the document selects");
        let md = markdown(&sel, &l, &doc);
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
        let (doc, l, _) = lay_doc(source);
        // A selection inside the first two body blocks stays precise and
        // never grows over the note or the trailing paragraph.
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
        assert_eq!(markdown(&select_all(&l, &doc), &l, &doc), source);
    }

    #[test]
    fn markdown_fences_unlabeled_code() {
        let source = "```\nplain fence\n```";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(markdown(&select_all(&l, &doc), &l, &doc), source);
    }

    #[test]
    fn blank_code_lines_survive_plain_copy() {
        let source = "```rust\nfn a() {}\n\nfn b() {}\n```";
        let (doc, l, _) = lay_doc(source);
        assert_eq!(
            plain_text(&select_all(&l, &doc), &l, &doc),
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
        let (doc, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let left = pos_at(&l, &doc, &mut fonts, run.x + 0.5, run.y + 1.0).unwrap();
        assert_eq!(left, RunPos { run: 0, ch: 0 });
        let right = pos_at(&l, &doc, &mut fonts, run.x + run.width + 50.0, run.y + 1.0).unwrap();
        assert_eq!(
            right,
            RunPos {
                run: 0,
                ch: l.run_text(&doc, run).chars().count()
            }
        );
    }

    #[test]
    fn rects_cover_fully_selected_run() {
        let (doc, l, mut fonts) = lay_doc("hello world");
        let run = &l.runs[0];
        let sel = select_all(&l, &doc);
        let boxes = rects(&sel, &l, &doc, &mut fonts);
        assert_eq!(boxes.len(), 1);
        let (x, _y, w, h) = boxes[0];
        assert!((x - run.x).abs() < 0.5);
        assert!((w - run.width).abs() < 0.5);
        assert!(h > run.size, "box covers the line height");
    }
}
