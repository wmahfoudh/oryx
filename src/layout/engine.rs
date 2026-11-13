//! The layout engine: document model in, positioned runs and rects out.
//! Pure with respect to the window: no pixels, fully testable with numbers.

use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use crate::style::fonts::line_width;

use super::metrics;
use crate::doc::images::MediaCache;
use crate::doc::model::{
    AlertKind, Block, BlockKind, ColumnAlign, Document, Marker, Span, SpanImage, SpanScript,
};
use crate::layout::pool::{Job, ShapeCtx, StepKey, Work};
use crate::style::fonts::{FontStore, BODY_FAMILY, CODE_FAMILY};
use crate::style::highlight::{SyntaxRole, LONG_LINE};
use crate::style::theme::{Rgba, Theme};

#[derive(Debug, Clone)]
pub struct ViewConfig {
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    /// Session zoom multiplier, never persisted.
    pub zoom: f32,
    /// Stretch prose to the content width, book typography: paragraphs,
    /// list items and quote text justify, everything else keeps its
    /// natural width. The app sets it from the book preference on a book
    /// and from the markdown preference on other prose, never on a code
    /// or text file (`justify_pref` in the app).
    pub justify: bool,
    /// A comic's display state; layout reads it only when the document
    /// is a comic. The page states carry the viewport height, which the
    /// app refreshes on resize.
    pub comic: ComicFit,
    /// The file's reading direction, cycled by Ctrl+D and remembered
    /// per file.
    pub direction: DirectionMode,
    /// The layout is bound for the page export: marks that only guide a
    /// reader on screen, the page break's dashed line, are left out.
    pub print: bool,
    /// The room the line numbers need left of a file of lines, zero
    /// when they are off. The page's own margin usually holds it and
    /// nothing moves; a wider need steps the lines right by the
    /// difference. A rendered page ignores it.
    pub gutter: f32,
}

impl Default for ViewConfig {
    fn default() -> ViewConfig {
        ViewConfig {
            body_family: BODY_FAMILY.to_string(),
            code_family: CODE_FAMILY.to_string(),
            body_size: 22.0,
            code_size: 20.0,
            zoom: 1.0,
            justify: false,
            comic: ComicFit::Width,
            direction: DirectionMode::Auto,
            print: false,
            gutter: 0.0,
        }
    }
}

/// The per-file reading direction. Automatic reads each text block's
/// first strong character; the forced states override every text block
/// and flip a comic's two-page pairing, for manga, Arabic comics and
/// books whose text Oryx misreads. Code and tables never turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectionMode {
    #[default]
    Auto,
    Rtl,
    Ltr,
}

impl DirectionMode {
    /// The Ctrl+D cycle: automatic, right to left, left to right.
    pub fn step(self) -> DirectionMode {
        match self {
            DirectionMode::Auto => DirectionMode::Rtl,
            DirectionMode::Rtl => DirectionMode::Ltr,
            DirectionMode::Ltr => DirectionMode::Auto,
        }
    }
}

/// How a comic shows its pages, stepped by the zoom keys: the strip
/// scales every page to the window width; the page states fit one page,
/// or the cover and then pairs, entirely inside the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComicFit {
    Width,
    Page { height: f32 },
    Two { height: f32 },
}

/// Where a run's display text lives. A run references text instead of
/// owning it: a model reference slices its span's display text (or its
/// code line's), a side reference slices the layout's own buffer of
/// synthesized text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRef {
    Model { start: u32, len: u32 },
    Side { start: u32, len: u32 },
}

/// One styled, positioned run of text on a single visual line. Text,
/// family and link answer through `LayoutDoc::run_text`, `run_family`
/// and `run_link`; the run itself owns no string.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: TextRef,
    pub x: f32,
    /// Top of the line box.
    pub y: f32,
    /// Baseline y, used by glyph rasterization at paint.
    pub baseline: f32,
    pub width: f32,
    pub size: f32,
    /// Id into `LayoutDoc::families`.
    pub family: u16,
    pub weight: u16,
    pub italic: bool,
    pub color: Rgba,
    /// Source position for selection, copy and links: the block index,
    /// and inside it the span index (the line index for code blocks,
    /// the flattened header-then-rows cell chain index for tables, the
    /// marker sentinel for bullets and checkmarks).
    pub block: usize,
    pub span: usize,
}

/// The model span a run indexes, through the block kind's own span
/// addressing. None for code lines, markers and synthesized spans.
fn model_span(doc: &Document, block: usize, span: usize) -> Option<&Span> {
    match &doc.blocks.get(block)?.kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::FootnoteDef { spans, .. }
        | BlockKind::Summary { spans, .. } => spans.get(span),
        BlockKind::Table { header, rows, .. } => header
            .iter()
            .flatten()
            .chain(rows.iter().flatten().flatten())
            .nth(span),
        _ => None,
    }
}

/// The display text a model reference slices: the span's text, or the
/// code line's for code blocks.
fn model_text(doc: &Document, block: usize, span: usize) -> &str {
    match &doc.blocks[block].kind {
        BlockKind::CodeBlock { lines, .. } => lines.line(&doc.source, span),
        _ => model_span(doc, block, span)
            .map(|s| s.text(&doc.source))
            .unwrap_or(""),
    }
}

/// A decoration rectangle: panels, bars, strike lines, table grid.
/// Zero radii and zero stroke give a plain filled rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecoRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
    /// Radius of the two top corners.
    pub radius_top: f32,
    /// Radius of the two bottom corners.
    pub radius_bottom: f32,
    /// Outline width; 0 fills the rectangle instead.
    pub stroke: f32,
    /// Paints with anti-aliasing. Math rules join anti-aliased glyph ink
    /// at subpixel positions; panel edges stay crisp on the pixel grid.
    pub anti_alias: bool,
}

impl DecoRect {
    pub fn fill(x: f32, y: f32, width: f32, height: f32, color: Rgba) -> DecoRect {
        DecoRect {
            x,
            y,
            width,
            height,
            color,
            radius_top: 0.0,
            radius_bottom: 0.0,
            stroke: 0.0,
            anti_alias: false,
        }
    }

    pub fn rounded(self, radius_top: f32, radius_bottom: f32) -> DecoRect {
        DecoRect {
            radius_top,
            radius_bottom,
            ..self
        }
    }

    pub fn stroked(self, stroke: f32) -> DecoRect {
        DecoRect { stroke, ..self }
    }

    pub fn smooth(self) -> DecoRect {
        DecoRect {
            anti_alias: true,
            ..self
        }
    }
}

#[derive(Debug, Default)]
pub struct LayoutDoc {
    pub height: f32,
    pub runs: Vec<TextRun>,
    pub rects: Vec<DecoRect>,
    /// Placed images, blitted by paint from the media cache.
    pub images: Vec<ImagePlace>,
    /// Typeset math glyphs in the math face, painted from the raster
    /// cache by glyph id; noad positions them, shaping never sees them.
    pub math_glyphs: Vec<MathGlyph>,
    /// Coarse y buckets over runs, rects and images, so the band
    /// painter and hit testing search instead of scanning.
    index: YIndex,
    /// Heading anchor slugs and their y positions.
    pub anchors: Vec<(String, f32)>,
    /// The link each footnote definition's number carries, back to the
    /// first reference: the block index and the `fnref:` target. The
    /// number is a marker run, so it answers here and not in the model.
    pub footnote_backs: Vec<(usize, String)>,
    /// Row bands of every table, in document order. Pagination needs
    /// them because a row is several lines that must not be split.
    pub table_rows: Vec<TableRow>,
    /// Per-line records for code blocks, ordered by block then line;
    /// `recolor_code_lines` re-shapes through them.
    pub code_lines: Vec<CodeLine>,
    /// The resolved families runs shaped with; runs carry ids.
    pub families: Vec<String>,
    /// The code face every code line is set in, family name and size
    /// with the zoom applied: the caret's seat on a line the layout
    /// holds no glyphs for shapes the line's whitespace in it.
    pub code_family: String,
    pub code_size: f32,
    /// Synthesized display text with no model home: markers, alert
    /// titles, frontmatter lines, expanded math, placeholder alts.
    /// Append-only; side references slice it.
    pub side: String,
    /// Every placed block's recorded position, whether or not its
    /// geometry is retained.
    table: BlockTable,
    /// Materialized-window bookkeeping while retention is bounded.
    window: Option<WindowState>,
    /// The checkbox square every placed task item draws, keyed by
    /// block, the click's hit targets. Outside the window's eviction:
    /// positions are exact wherever the window sits, so a recorded box
    /// stays valid until the layout itself is dropped;
    /// re-materialization re-pushes and replaces by key.
    checkboxes: Vec<TaskBox>,
}

/// One drawn checkbox: its square in document coordinates and the
/// model block it flips.
#[derive(Debug, Clone, Copy)]
pub struct TaskBox {
    pub x: f32,
    pub y: f32,
    pub side: f32,
    pub block: u32,
}

/// Recorded placement of one order position: where the block sits and
/// how tall it is, enough to re-shape it at its recorded y or to skip
/// over it without geometry.
#[derive(Debug, Clone)]
struct BlockEntry {
    /// Model block index.
    block: u32,
    /// Region top: where the block's output begins, alert title included.
    y: f32,
    /// Where the block's own emission splices, past any alert title; a
    /// code block's panel top.
    content_y: f32,
    /// Emission height: the shaped kind's, or the code panel's.
    height: f32,
    /// Quote decoration top; NaN when the block paints no quote panel.
    deco_top: f32,
    /// Code blocks: the line height every unwrapped line advances by,
    /// and the panel padding above the first line.
    line_height: f32,
    pad: f32,
    /// Code blocks: where a line's first glyph stands, the block's
    /// left edge past the panel padding.
    x0: f32,
    flags: u8,
}

/// The block emitted nothing: `block_metrics` answered None.
const ENTRY_SILENT: u8 = 1;
const ENTRY_CODE: u8 = 2;
/// The block opened its alert region and carries the bold title line.
const ENTRY_ALERT_TITLE: u8 = 4;

impl BlockEntry {
    /// The bottom of the block's own emission, decorations included.
    fn bottom(&self) -> f32 {
        self.content_y + self.height
    }
}

/// The pass's record of every placed block: y and height per order
/// position, the inverse block map, and the code lines that wrapped
/// taller than their block's shared line height. Heights are shaped,
/// never estimated, so the table stays exact wherever the window sits.
#[derive(Debug, Default)]
pub(crate) struct BlockTable {
    entries: Vec<BlockEntry>,
    /// Order position of each model block; u32::MAX until placed.
    position_of_block: Vec<u32>,
    /// `(position, line, height)` for code lines taller than the shared
    /// line height, in placement order: the exceptions that keep every
    /// line's y exact.
    tall: Vec<(u32, u32, f32)>,
    /// Beside each entry of `tall`, the extra height of its position's
    /// wrapped lines up to and including it: the sum a walk from the
    /// position's first entry would reach, added in the same order, so
    /// a line's top is one lookup and equal to the walk bit for bit.
    tall_extra: Vec<f32>,
    /// The footnote rule's order position and y, once placed.
    notes_rule: Option<(usize, f32)>,
    /// The pass geometry blocks were placed against, for re-shaping.
    margin: f32,
    content_width: f32,
}

impl BlockTable {
    fn push(&mut self, block: usize, entry: BlockEntry) {
        if self.position_of_block.len() <= block {
            self.position_of_block.resize(block + 1, u32::MAX);
        }
        self.position_of_block[block] = self.entries.len() as u32;
        self.entries.push(entry);
    }

    /// Backfills what only the block's tail knows: its shaped height
    /// and, when quoted, its decoration top.
    fn finish(&mut self, height: f32, deco_top: f32) {
        let entry = self.entries.last_mut().expect("a block was placed");
        entry.height = height;
        entry.deco_top = deco_top;
    }

    /// Records a code line taller than its block's line height. The
    /// lines of a position arrive in order.
    fn push_tall(&mut self, position: usize, line: u32, height: f32) {
        let before = match self.tall.last() {
            Some(&(p, _, _)) if p as usize == position => {
                self.tall_extra.last().copied().unwrap_or(0.0)
            }
            _ => 0.0,
        };
        let extra = before + (height - self.entries[position].line_height);
        self.tall.push((position as u32, line, height));
        self.tall_extra.push(extra);
    }

    /// The sums of `tall_extra` again from entry `from` of the list to
    /// the end of that entry's position, after the entries moved.
    fn resum_tall(&mut self, from: usize) {
        let Some(&(position, _, _)) = self.tall.get(from) else {
            return;
        };
        let line_height = self.entries[position as usize].line_height;
        let mut extra = match from.checked_sub(1) {
            Some(prev) if self.tall[prev].0 == position => self.tall_extra[prev],
            _ => 0.0,
        };
        for k in from..self.tall.len() {
            if self.tall[k].0 != position {
                break;
            }
            extra += self.tall[k].2 - line_height;
            self.tall_extra[k] = extra;
        }
    }

    /// Whether each stored sum equals the walk from its position's
    /// first entry, bit for bit, and the lines of a position rise.
    fn tall_sums_hold(&self) -> bool {
        if self.tall.len() != self.tall_extra.len() {
            return false;
        }
        let mut extra = 0.0_f32;
        let mut prev: Option<(u32, u32)> = None;
        for (k, &(position, line, height)) in self.tall.iter().enumerate() {
            match prev {
                Some((p, l)) if p == position => {
                    if l >= line {
                        return false;
                    }
                }
                _ => extra = 0.0,
            }
            extra += height - self.entries[position as usize].line_height;
            if extra.to_bits() != self.tall_extra[k].to_bits() {
                return false;
            }
            prev = Some((position, line));
        }
        true
    }

    /// The extra height of the wrapped lines of `position` above
    /// `line`, and how many of the position's entries they are.
    fn extra_above(&self, position: usize, line: usize) -> (f32, usize) {
        let p = position as u32;
        let lo = self.tall.partition_point(|&(tp, _, _)| tp < p);
        let above = self.tall[lo..].partition_point(|&(tp, l, _)| tp == p && (l as usize) < line);
        let extra = match above {
            0 => 0.0,
            n => self.tall_extra[lo + n - 1],
        };
        (extra, above)
    }

    /// This position's slice of the tall-line exceptions.
    fn tall_of(&self, position: usize) -> &[(u32, u32, f32)] {
        let position = position as u32;
        let lo = self.tall.partition_point(|&(p, _, _)| p < position);
        let hi = self.tall.partition_point(|&(p, _, _)| p <= position);
        &self.tall[lo..hi]
    }

    /// The recorded top of line `line`, reproducing the placement
    /// arithmetic bit for bit: the shared advance in closed form, plus
    /// the wrapped lines' extra heights as the pass accumulated them.
    fn code_line_top(&self, position: usize, line: usize) -> f32 {
        let entry = &self.entries[position];
        let (extra, _) = self.extra_above(position, line);
        code_line_y(entry.content_y + entry.pad, line, entry.line_height, extra)
    }
}

/// A code line's top from the panel's first-line base: one shared
/// expression, so placement, the table and re-entry agree bit for bit.
fn code_line_y(base: f32, line: usize, line_height: f32, extra: f32) -> f32 {
    base + line as f32 * line_height + extra
}

/// Materialized positions while retention is bounded: a contiguous
/// range from `start`, with cumulative element ends per position.
#[derive(Debug, Default)]
struct WindowState {
    start: usize,
    marks: std::collections::VecDeque<PosMarks>,
}

/// One materialized position's cumulative element ends in the layout's
/// vectors and, for a code block, the materialized line range.
#[derive(Debug, Clone)]
struct PosMarks {
    runs: usize,
    rects: usize,
    images: usize,
    math: usize,
    rows: usize,
    code: usize,
    lines: Range<usize>,
}

impl PosMarks {
    fn of(lay: &LayoutDoc, lines: Range<usize>) -> PosMarks {
        PosMarks {
            runs: lay.runs.len(),
            rects: lay.rects.len(),
            images: lay.images.len(),
            math: lay.math_glyphs.len(),
            rows: lay.table_rows.len(),
            code: lay.code_lines.len(),
            lines,
        }
    }

    fn zero() -> PosMarks {
        PosMarks {
            runs: 0,
            rects: 0,
            images: 0,
            math: 0,
            rows: 0,
            code: 0,
            lines: 0..0,
        }
    }
}

impl LayoutDoc {
    /// A run's display text. Every consumer reads through this, so the
    /// run itself is free to reference the model instead of owning a
    /// copy.
    pub fn run_text<'a>(&'a self, doc: &'a Document, run: &'a TextRun) -> &'a str {
        match run.text {
            TextRef::Side { start, len } => &self.side[start as usize..(start + len) as usize],
            TextRef::Model { start, len } => {
                let base = model_text(doc, run.block, run.span);
                &base[start as usize..(start + len) as usize]
            }
        }
    }

    /// The whole display text of the span a model reference slices, the
    /// run's own bytes included: the highlight boxes and the match
    /// anchors read past a run's visible end into the whitespace layout
    /// trimmed there. Empty for a side reference.
    pub fn span_text<'a>(&self, doc: &'a Document, run: &TextRun) -> &'a str {
        match run.text {
            TextRef::Side { .. } => "",
            TextRef::Model { .. } => model_text(doc, run.block, run.span),
        }
    }

    /// The resolved family a run shaped with.
    pub fn run_family<'a>(&'a self, run: &'a TextRun) -> &'a str {
        &self.families[run.family as usize]
    }

    /// A run's link target, resolved through the model span the run
    /// indexes; synthesized runs carry none.
    pub fn run_link<'a>(&'a self, doc: &'a Document, run: &'a TextRun) -> Option<&'a str> {
        if run.span == usize::MAX {
            return self
                .footnote_backs
                .iter()
                .find(|(block, _)| *block == run.block)
                .map(|(_, target)| target.as_str());
        }
        model_span(doc, run.block, run.span)?.link.as_deref()
    }

    /// The expansion of the abbreviation under (`x`, `y`), if a run
    /// standing for one sits there.
    pub fn abbr_at<'a>(&'a self, doc: &'a Document, x: f32, y: f32) -> Option<&'a str> {
        let (head, tail) = self.runs_in(y, y);
        self.runs[head]
            .iter()
            .chain(&self.runs[tail])
            .find_map(|r| {
                let inside = x >= r.x
                    && x <= r.x + r.width
                    && y >= r.y
                    && y <= r.y + metrics::LINE_HEIGHT * r.size;
                if !inside {
                    return None;
                }
                model_span(doc, r.block, r.span)?.abbr(doc)
            })
    }

    /// The id of a family in this layout's table, interned on first use.
    /// The resolved families of a document number a handful, so a scan
    /// beats a map.
    pub(crate) fn family_id(&mut self, name: &str) -> u16 {
        if let Some(index) = self.families.iter().position(|f| f == name) {
            return index as u16;
        }
        self.families.push(name.to_string());
        (self.families.len() - 1) as u16
    }

    /// Appends synthesized text to the side buffer and answers its
    /// reference.
    pub(crate) fn side_ref(&mut self, text: &str) -> TextRef {
        let start = self.side.len() as u32;
        self.side.push_str(text);
        TextRef::Side {
            start,
            len: text.len() as u32,
        }
    }

    /// Drains a step's scratch into the document with every position
    /// dropped by `top` and every carried run index shifted behind the
    /// existing elements. Height stays the caller's: the pass owns the
    /// cursor. The scratch comes back empty for reuse.
    /// Adopts another layout's side buffer and family table, answering
    /// the side offset and the id remap its runs need before they join
    /// this layout's vector.
    pub(crate) fn merge_refs(&mut self, other: &mut LayoutDoc) -> (u32, Vec<u16>) {
        let side_base = self.side.len() as u32;
        let family_map: Vec<u16> = other
            .families
            .iter()
            .map(|name| {
                if let Some(index) = self.families.iter().position(|f| f == name) {
                    index as u16
                } else {
                    self.families.push(name.clone());
                    (self.families.len() - 1) as u16
                }
            })
            .collect();
        self.side.push_str(&other.side);
        other.side.clear();
        other.families.clear();
        (side_base, family_map)
    }

    pub fn splice(&mut self, scratch: &mut LayoutDoc, top: f32) {
        let base_runs = self.runs.len();
        let (side_base, family_map) = self.merge_refs(scratch);
        self.runs.extend(scratch.runs.drain(..).map(|mut run| {
            run.y += top;
            run.baseline += top;
            if let TextRef::Side { start, .. } = &mut run.text {
                *start += side_base;
            }
            run.family = family_map[run.family as usize];
            run
        }));
        self.rects.extend(scratch.rects.drain(..).map(|mut rect| {
            rect.y += top;
            rect
        }));
        self.images
            .extend(scratch.images.drain(..).map(|mut image| {
                image.y += top;
                image
            }));
        self.math_glyphs
            .extend(scratch.math_glyphs.drain(..).map(|mut g| {
                g.y += top;
                g.top += top;
                g.bottom += top;
                g
            }));
        self.anchors
            .extend(scratch.anchors.drain(..).map(|mut anchor| {
                anchor.1 += top;
                anchor
            }));
        for back in scratch.footnote_backs.drain(..) {
            if !self.footnote_backs.iter().any(|(b, _)| *b == back.0) {
                self.footnote_backs.push(back);
            }
        }
        self.table_rows
            .extend(scratch.table_rows.drain(..).map(|mut row| {
                row.top += top;
                row.bottom += top;
                row
            }));
        self.code_lines
            .extend(scratch.code_lines.drain(..).map(|mut line| {
                line.runs = line.runs.start + base_runs..line.runs.end + base_runs;
                line.y0 += top;
                line
            }));
        for mut tb in scratch.checkboxes.drain(..) {
            tb.y += top;
            self.push_checkbox(tb);
        }
        scratch.height = 0.0;
    }

    /// Records a task item's checkbox square, replacing the block's
    /// earlier record when re-materialization shapes it again.
    fn push_checkbox(&mut self, tb: TaskBox) {
        match self.checkboxes.iter_mut().find(|c| c.block == tb.block) {
            Some(slot) => *slot = tb,
            None => self.checkboxes.push(tb),
        }
    }

    /// The task block whose checkbox square holds the point, a small
    /// halo included so the square is easy to hit.
    pub fn checkbox_at(&self, x: f32, y: f32) -> Option<usize> {
        let pad = 2.0;
        self.checkboxes
            .iter()
            .find(|c| {
                x >= c.x - pad
                    && x <= c.x + c.side + pad
                    && y >= c.y - pad
                    && y <= c.y + c.side + pad
            })
            .map(|c| c.block as usize)
    }

    /// Drops the materialized window so the next slide refills the
    /// band from the model at its recorded positions: the cost of a
    /// far scroll jump, paid when the model changed without its
    /// geometry moving, which is the checkbox flip. False when the
    /// layout is not windowed; the caller restarts instead.
    pub fn rematerialize(&mut self) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let start = window.start;
        self.window_clear(start);
        true
    }
}

/// One typeset math glyph: the math face at an absolute position. `y` is
/// the glyph's baseline in document coordinates; paint rasterizes by
/// glyph id through the swash cache. `top` and `bottom` are the whole
/// equation's vertical extent, shared by every glyph of one equation, so
/// pagination can treat the equation as an unbreakable band; `ch` is the
/// character the glyph renders when one exists, which is what the PDF
/// export writes into its ToUnicode map.
#[derive(Debug, Clone, PartialEq)]
pub struct MathGlyph {
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub ch: Option<char>,
    pub top: f32,
    pub bottom: f32,
    /// The left and right edges of the formula's box, as `top` and
    /// `bottom` are its vertical edges: a table's measuring pass and the
    /// alignment of a line read them, a space or a phantom that opens or
    /// ends the formula counted.
    pub left: f32,
    pub right: f32,
    pub color: crate::style::theme::Rgba,
    pub block: usize,
}

impl MathGlyph {
    /// The vertical extent the glyph's ink lies in, for the index and
    /// the paint of a strip: its formula's box, which holds a sized
    /// delimiter and a display operator whole, and a margin around the
    /// baseline for a glyph whose ink passes the box.
    pub fn ink(&self) -> (f32, f32) {
        (
            self.top.min(self.y - 1.2 * self.size),
            self.bottom.max(self.y + 0.6 * self.size),
        )
    }
}

/// One table row's vertical band, stripe and padding included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableRow {
    pub block: usize,
    pub top: f32,
    pub bottom: f32,
}

/// One laid-out code line: the runs it produced and the inputs to
/// re-shape it, so arriving highlights recolor in place without a
/// relayout. Code runs differ only by color across syntax roles, so
/// re-shaping a line never moves anything.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeLine {
    pub block: usize,
    pub line: usize,
    runs: Range<usize>,
    x0: f32,
    y0: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
}

/// Height of one index bucket in layout units.
const BUCKET_H: f32 = 512.0;

/// Coarse y buckets over the layout's element vectors. Filled in append
/// order behind a watermark; queries answer with a conservative indexed
/// range plus the unindexed tail, so a stale index can never miss an
/// element, only cost a longer linear tail.
#[derive(Debug, Default)]
pub struct YIndex {
    runs: Buckets,
    rects: Buckets,
    images: Buckets,
    math: Buckets,
}

/// Per bucket, the first and last element index touching it, and the
/// watermark below which elements are noted.
#[derive(Debug, Default)]
struct Buckets {
    spans: Vec<Option<(usize, usize)>>,
    indexed: usize,
}

impl Buckets {
    fn note(&mut self, index: usize, y0: f32, y1: f32) {
        let b0 = (y0.max(0.0) / BUCKET_H) as usize;
        let b1 = (y1.max(y0).max(0.0) / BUCKET_H) as usize;
        if self.spans.len() <= b1 {
            self.spans.resize(b1 + 1, None);
        }
        for bucket in b0..=b1 {
            let entry = &mut self.spans[bucket];
            *entry = Some(match *entry {
                Some((first, last)) => (first.min(index), last.max(index)),
                None => (index, index),
            });
        }
    }

    fn query(&self, y0: f32, y1: f32, len: usize) -> (Range<usize>, Range<usize>) {
        let tail = self.indexed.min(len)..len;
        if self.spans.is_empty() {
            return (0..0, tail);
        }
        let top = self.spans.len() - 1;
        let b0 = ((y0.max(0.0) / BUCKET_H) as usize).min(top);
        let b1 = ((y1.max(y0).max(0.0) / BUCKET_H) as usize).min(top);
        let (mut first, mut last) = (usize::MAX, 0usize);
        for entry in self.spans[b0..=b1].iter().flatten() {
            first = first.min(entry.0);
            last = last.max(entry.1);
        }
        let head = if first == usize::MAX {
            0..0
        } else {
            first..last + 1
        };
        (head, tail)
    }

    fn clear(&mut self) {
        self.spans.clear();
        self.indexed = 0;
    }
}

impl LayoutDoc {
    /// Extends the index over elements appended since the last call.
    pub fn index_more(&mut self) {
        let index = &mut self.index;
        for (i, run) in self.runs.iter().enumerate().skip(index.runs.indexed) {
            index
                .runs
                .note(i, run.y, run.y + metrics::LINE_HEIGHT * run.size);
        }
        index.runs.indexed = self.runs.len();
        for (i, rect) in self.rects.iter().enumerate().skip(index.rects.indexed) {
            index.rects.note(i, rect.y, rect.y + rect.height);
        }
        index.rects.indexed = self.rects.len();
        for (i, image) in self.images.iter().enumerate().skip(index.images.indexed) {
            index.images.note(i, image.y, image.y + image.height);
        }
        index.images.indexed = self.images.len();
        for (i, g) in self.math_glyphs.iter().enumerate().skip(index.math.indexed) {
            let (top, bottom) = g.ink();
            index.math.note(i, top, bottom);
        }
        index.math.indexed = self.math_glyphs.len();
    }

    /// Index ranges whose union holds every run touching `[y0, y1]`:
    /// the indexed head range and the unindexed tail.
    pub fn runs_in(&self, y0: f32, y1: f32) -> (Range<usize>, Range<usize>) {
        self.index.runs.query(y0, y1, self.runs.len())
    }

    /// As `runs_in`, over the decoration rectangles.
    pub fn rects_in(&self, y0: f32, y1: f32) -> (Range<usize>, Range<usize>) {
        self.index.rects.query(y0, y1, self.rects.len())
    }

    /// As `runs_in`, over the placed images.
    pub fn images_in(&self, y0: f32, y1: f32) -> (Range<usize>, Range<usize>) {
        self.index.images.query(y0, y1, self.images.len())
    }

    /// As `runs_in`, over the typeset math glyphs.
    pub fn math_in(&self, y0: f32, y1: f32) -> (Range<usize>, Range<usize>) {
        self.index.math.query(y0, y1, self.math_glyphs.len())
    }

    /// Link target under a point in document coordinates, if any.
    /// The hit box of a run spans its full line height. The index keeps
    /// this a search; it runs on every mouse move.
    pub fn link_at<'a>(&'a self, doc: &'a Document, x: f32, y: f32) -> Option<&'a str> {
        let (head, tail) = self.runs_in(y, y);
        let run_hit = self.runs[head]
            .iter()
            .chain(&self.runs[tail])
            .find_map(|r| {
                let target = self.run_link(doc, r)?;
                let inside = x >= r.x
                    && x <= r.x + r.width
                    && y >= r.y
                    && y <= r.y + metrics::LINE_HEIGHT * r.size;
                inside.then_some(target)
            });
        run_hit.or_else(|| {
            let (head, tail) = self.images_in(y, y);
            self.images[head]
                .iter()
                .chain(&self.images[tail])
                .find_map(|i| {
                    let target = i.link.as_deref()?;
                    let inside = x >= i.x && x <= i.x + i.width && y >= i.y && y <= i.y + i.height;
                    inside.then_some(target)
                })
        })
    }

    /// The details group of the summary row under `y`, when `x` sits at
    /// or right of the content column's start. The whole row toggles,
    /// the disclosure convention, so only the vertical band is tested.
    pub fn summary_at(&self, doc: &Document, x: f32, y: f32) -> Option<u16> {
        if x < 0.0 {
            return None;
        }
        let (head, tail) = self.runs_in(y, y);
        self.runs[head]
            .iter()
            .chain(&self.runs[tail])
            .find_map(|r| {
                let BlockKind::Summary { group, .. } = &doc.blocks[r.block].kind else {
                    return None;
                };
                let inside = y >= r.y && y <= r.y + metrics::LINE_HEIGHT * r.size;
                inside.then_some(*group)
            })
    }

    /// Y position of a `#anchor` link target or a `footnote:` reference.
    pub fn anchor_y(&self, target: &str) -> Option<f32> {
        let slug = target.strip_prefix('#').unwrap_or(target);
        self.anchors
            .iter()
            .find(|(s, _)| s == slug)
            .map(|(_, y)| *y)
    }

    /// The y span of materialized geometry, None under full retention.
    pub fn window_span(&self) -> Option<Range<f32>> {
        let window = self.window.as_ref()?;
        if window.marks.is_empty() {
            return Some(0.0..0.0);
        }
        let first = &self.table.entries[window.start];
        let last = &self.table.entries[window.start + window.marks.len() - 1];
        Some(first.y..last.bottom())
    }

    /// The pass's placement frontier: how many order positions carry an
    /// entry, the last possibly still filling.
    pub(crate) fn placed_positions(&self) -> usize {
        self.table.entries.len()
    }

    /// A placed block's order position, None before the pass reaches it.
    pub(crate) fn block_position(&self, block: usize) -> Option<usize> {
        let position = *self.table.position_of_block.get(block)?;
        (position != u32::MAX).then_some(position as usize)
    }

    /// The recorded top of a block, and of a line inside a code block,
    /// from the block table: the position of a cold region without its
    /// geometry. None before the pass places the block.
    pub fn approx_top(&self, block: usize, span: usize) -> Option<f32> {
        let position = *self.table.position_of_block.get(block)?;
        if position == u32::MAX {
            return None;
        }
        let entry = &self.table.entries[position as usize];
        if entry.flags & ENTRY_CODE != 0 {
            return Some(self.table.code_line_top(position as usize, span));
        }
        Some(entry.y)
    }

    /// The model block standing at height `y`, from the block table:
    /// the block of the last placed position whose top is at or above
    /// `y`, the inverse of `approx_top(block, 0)`. The positions are in
    /// the order of the page, not of the source: a footnote definition
    /// stands at the end of the page wherever the source declares it,
    /// and a block folded inside a closed details group has no
    /// position. None above the first block, and before the pass
    /// places one.
    pub fn block_at(&self, y: f32) -> Option<usize> {
        let top = |position: usize| {
            let entry = &self.table.entries[position];
            if entry.flags & ENTRY_CODE != 0 {
                self.table.code_line_top(position, 0)
            } else {
                entry.y
            }
        };
        let after = partition(self.table.entries.len(), |position| top(position) <= y);
        let position = after.checked_sub(1)?;
        Some(self.table.entries[position].block as usize)
    }

    /// The line of code block `block` standing at height `y`, from the
    /// block table: the last of its `lines` whose top is at or above
    /// `y`, the inverse of `approx_top`. None before the pass places
    /// the block, and for a block that is not code.
    pub fn code_line_at(&self, block: usize, lines: usize, y: f32) -> Option<usize> {
        let position = *self.table.position_of_block.get(block)?;
        if position == u32::MAX {
            return None;
        }
        let position = position as usize;
        if self.table.entries[position].flags & ENTRY_CODE == 0 {
            return None;
        }
        let (mut at, mut past) = (0, lines.max(1));
        while at + 1 < past {
            let mid = at + (past - at) / 2;
            if self.table.code_line_top(position, mid) <= y {
                at = mid;
            } else {
                past = mid;
            }
        }
        Some(at)
    }

    /// The recorded top and bottom of a block, from the block table.
    /// None before the pass places the block.
    pub fn block_span(&self, block: usize) -> Option<Range<f32>> {
        let position = *self.table.position_of_block.get(block)?;
        if position == u32::MAX {
            return None;
        }
        let entry = &self.table.entries[position as usize];
        Some(entry.y..entry.bottom().max(entry.y))
    }

    /// Where the first glyph of line `line` of code block `block`
    /// stands, from the block table alone: the caret's seat on a line
    /// the layout holds no glyphs for. None before the pass places the
    /// block, and for a block that is not code.
    pub fn code_line_seat(&self, block: usize, line: usize) -> Option<LineSeat> {
        let position = *self.table.position_of_block.get(block)?;
        if position == u32::MAX {
            return None;
        }
        let entry = &self.table.entries[position as usize];
        if entry.flags & ENTRY_CODE == 0 {
            return None;
        }
        Some(LineSeat {
            x: entry.x0,
            y: self.table.code_line_top(position as usize, line),
            height: entry.line_height,
        })
    }
}

/// A code line's first-glyph position and its row height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineSeat {
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// One image scaled and positioned in the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlace {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Click target when the image is wrapped in a link.
    pub link: Option<String>,
}

/// The first slice at open: the same budget highlighting gets, which
/// places several screens at any document size.
pub const OPEN_SLICE: Duration = Duration::from_millis(40);

/// A wash-in slice, short enough that input latency stays invisible.
pub const SLICE: Duration = Duration::from_millis(16);

/// What a slice boundary carries: the block loop's running values and,
/// when a code block is open, the position inside it. A code file is one
/// block, so stopping between its lines is what bounds a slice.
pub struct LayoutPass {
    order: Vec<usize>,
    position: usize,
    /// Model blocks considered so far; above the order's length when
    /// folded blocks were skipped.
    covered: usize,
    notes_start: usize,
    has_notes: bool,
    cursor: f32,
    first: bool,
    prev_quote_depth: u8,
    prev_alert: Option<AlertKind>,
    prev_is_list: bool,
    prev_space_below: f32,
    /// A plain text file: rows are uniform and blocks meet with no
    /// added gap or trailing space.
    plain: bool,
    margin: f32,
    content_width: f32,
    vertical_margin: f32,
    open: Option<OpenCode>,
    done: bool,
    /// Reused per-step buffer: kind emission shapes into it from zero and
    /// the splice drops it at the cursor, the arithmetic a pooled worker
    /// reproduces.
    scratch: LayoutDoc,
    /// The shaping pool and the generation this pass last claimed. A
    /// mismatch means another pass or a model change superseded the fed
    /// work, and the next slice reclaims and reseeds from the
    /// assembler's own position.
    pool: Option<std::sync::Arc<crate::layout::pool::ShapePool>>,
    pool_generation: u64,
    ctx: Option<std::sync::Arc<crate::layout::pool::ShapeCtx>>,
    /// Where seeding reached: the order position and, inside a code
    /// block, the next line to claim.
    seed_position: usize,
    seed_line: usize,
    /// Retention bound: scroll position and viewport height. None keeps
    /// every placed block, the export's and the tests' full layout.
    retain: Option<(f32, f32)>,
    /// The character widths of the code face, once a long code block's
    /// lines are measured past the kept ones: such a line of plain
    /// ASCII is counted, not shaped (`rows`). None in a face that
    /// cannot be counted and before a block asks.
    counter: Option<super::rows::Widths>,
    /// The order position whose lines are counted from here on, so the
    /// pool is not fed the lines the count answers.
    counting: Option<usize>,
}

/// A code block is counted once this many of its lines are left when
/// the pass moves past the kept ones. A shorter block is shaped whole,
/// as before: reading a face's table costs more than it saves there.
const COUNT_FROM: usize = 2048;

/// Whether a code line's styled ranges all draw in the regular face.
/// A markdown source asks for bold and italic, which the count's table
/// does not hold.
fn regular_face(segments: &[(Range<usize>, SyntaxRole)]) -> bool {
    segments
        .iter()
        .all(|(_, role)| crate::style::highlight::role_face(*role) == (false, false))
}

impl LayoutPass {
    /// True once every block is placed.
    pub fn is_complete(&self) -> bool {
        self.done
    }

    /// Attaches the shaping pool; the next slice claims it and seeds
    /// ahead. Without one the pass shapes every step itself.
    pub fn attach_pool(&mut self, pool: std::sync::Arc<crate::layout::pool::ShapePool>) {
        self.pool = Some(pool);
    }

    /// Marks the pool's fed work stale after a model change, a highlight
    /// fold or a parse swap: the next slice reclaims and reseeds.
    pub fn invalidate_pool(&mut self) {
        if let Some(pool) = &self.pool {
            pool.begin();
        }
    }

    /// Follows a recolor that rebuilt the runs `run_lo..run_hi` into a
    /// span `delta` longer. An open code block holds the run count at
    /// its start across steps, to drop the block whole or to align it
    /// when it closes; runs added before that start move it. A rebuild
    /// reaching into the block's own lines leaves the start at its
    /// first record's. The family table only grows under a recolor, and
    /// the names it added stay, since recolored runs carry their ids.
    fn follow_recolor(&mut self, lay: &LayoutDoc, run_lo: usize, run_hi: usize, delta: isize) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let first_record = lay.code_lines.get(open.counts.code).map(|c| c.runs.start);
        let moved = |start: usize| {
            if start <= run_lo {
                start
            } else if start >= run_hi {
                start.wrapping_add_signed(delta)
            } else {
                first_record.unwrap_or(start)
            }
        };
        open.counts.runs = moved(open.counts.runs);
        open.frame.marks.runs = moved(open.frame.marks.runs);
        open.frame.lines.runs = moved(open.frame.lines.runs);
        open.counts.families = open.counts.families.max(lay.families.len());
    }

    /// True while the pass stands inside a code block it places line by
    /// line: the block's element counts are held across steps.
    pub fn has_open_code(&self) -> bool {
        self.open.is_some()
    }

    /// Bounds retention around a scroll position: blocks outside the
    /// band and its margin are measured into the table and dropped.
    pub fn retain_around(&mut self, scroll: f32, viewport_h: f32) {
        self.retain = Some((scroll, viewport_h));
    }

    /// Shifts the indices the pass holds into the layout's vectors
    /// after a consumer drained emitted geometry from their front: the
    /// open code block's panel, which is never part of what such a
    /// consumer may drop, and the places the block noted at its start
    /// (its marks and its counts), which its end reads again to align
    /// its lines and to insert a quote's decoration. A place the drain
    /// passed is the front of what remains.
    pub(crate) fn rebase(
        &mut self,
        runs: usize,
        rects: usize,
        images: usize,
        math: usize,
        code: usize,
    ) {
        if let Some(open) = &mut self.open {
            if open.framed {
                open.panel -= rects;
            }
            for marks in [&mut open.frame.marks, &mut open.frame.lines] {
                marks.runs = marks.runs.saturating_sub(runs);
                marks.rects = marks.rects.saturating_sub(rects);
                marks.images = marks.images.saturating_sub(images);
                marks.math = marks.math.saturating_sub(math);
            }
            open.counts.runs = open.counts.runs.saturating_sub(runs);
            open.counts.rects = open.counts.rects.saturating_sub(rects);
            open.counts.images = open.counts.images.saturating_sub(images);
            open.counts.math = open.counts.math.saturating_sub(math);
            open.counts.code = open.counts.code.saturating_sub(code);
        }
    }

    /// The attached pool and the generation this pass last claimed, for
    /// emission jobs to ride the same claim.
    pub(crate) fn pool_state(
        &self,
    ) -> Option<(std::sync::Arc<crate::layout::pool::ShapePool>, u64)> {
        self.pool.clone().map(|pool| (pool, self.pool_generation))
    }
}

/// The y range retention keeps materialized around a scroll position:
/// wide enough to cover the paint band wherever its clamping puts it,
/// with a viewport of margin so a small scroll re-shapes nothing.
pub fn retain_range(scroll: f32, viewport_h: f32) -> Range<f32> {
    scroll - 5.0 * viewport_h..scroll + 6.0 * viewport_h
}

/// Element counts at a block's start: the truncation point when the
/// block places outside the retention bound. Truncation only ever cuts
/// what the block just appended, so the anchors and the table survive.
struct ElementCounts {
    runs: usize,
    rects: usize,
    images: usize,
    math: usize,
    rows: usize,
    code: usize,
    side: usize,
    families: usize,
}

impl ElementCounts {
    fn of(out: &LayoutDoc) -> ElementCounts {
        ElementCounts {
            runs: out.runs.len(),
            rects: out.rects.len(),
            images: out.images.len(),
            math: out.math_glyphs.len(),
            rows: out.table_rows.len(),
            code: out.code_lines.len(),
            side: out.side.len(),
            families: out.families.len(),
        }
    }

    fn truncate(&self, out: &mut LayoutDoc) {
        out.runs.truncate(self.runs);
        out.rects.truncate(self.rects);
        out.images.truncate(self.images);
        out.math_glyphs.truncate(self.math);
        out.table_rows.truncate(self.rows);
        out.code_lines.truncate(self.code);
        out.side.truncate(self.side);
        out.families.truncate(self.families);
        // A truncation below an index watermark would leave bucket spans
        // pointing past the vectors; those buckets restart.
        if out.index.runs.indexed > self.runs {
            out.index.runs.clear();
        }
        if out.index.rects.indexed > self.rects {
            out.index.rects.clear();
        }
        if out.index.images.indexed > self.images {
            out.index.images.clear();
        }
        if out.index.math.indexed > self.math {
            out.index.math.clear();
        }
    }
}

/// Settles the block just placed against the retention bound: outside
/// it the geometry drops back to `counts`, inside it the materialized
/// window extends over the position. `lines` is the retained line
/// range, meaningful for code blocks. A window that holds no position
/// yet follows the pass, so the next kept position joins it. When the
/// scroll lies past the end of the page, as after the page got much
/// shorter, the pass keeps no block: its empty window is what lets
/// `window_to` fill the view once the scroll is clamped.
fn settle_retention(
    out: &mut LayoutDoc,
    pass: &LayoutPass,
    counts: &ElementCounts,
    lines: Range<usize>,
) {
    let Some((scroll, viewport_h)) = pass.retain else {
        return;
    };
    let range = retain_range(scroll, viewport_h);
    let position = out.table.entries.len() - 1;
    let entry = &out.table.entries[position];
    let retained = entry.y <= range.end && entry.bottom().max(entry.y) >= range.start;
    if !retained {
        counts.truncate(out);
        match out.window.as_mut() {
            Some(window) if window.marks.is_empty() => window.start = position + 1,
            Some(_) => {}
            None => {
                out.window = Some(WindowState {
                    start: position + 1,
                    marks: std::collections::VecDeque::new(),
                });
            }
        }
        return;
    }
    // The bound moved past the window while the pass ran: the blocks
    // between were placed outside the bound of their slice and dropped.
    // A new window opens on this block; extending the old one would hand
    // its marks to the wrong positions. The old window's blocks may still
    // lie inside the bound of this slice, and the slide re-shapes them.
    let follows = out
        .window
        .as_ref()
        .is_none_or(|window| window.start + window.marks.len() == position);
    if !follows {
        let held = out.window.as_ref().map_or(0, |window| window.marks.len());
        out.window_drop_front(held);
        out.window.as_mut().expect("windowed layout").start = position;
    }
    let marks = PosMarks::of(out, lines);
    match out.window.as_mut() {
        Some(window) => {
            debug_assert_eq!(window.start + window.marks.len(), position);
            window.marks.push_back(marks);
        }
        None => {
            out.window = Some(WindowState {
                start: position,
                marks: [marks].into(),
            });
        }
    }
}

/// The code lines a y range shows, one entry per code block it
/// crosses. The speculation trigger reads it to learn what the viewport
/// wants colored; blocks the pass has not placed answer nothing.
pub fn code_lines_in(
    lay: &LayoutDoc,
    doc: &Document,
    range: Range<f32>,
) -> Vec<(usize, Range<usize>)> {
    let mut out = Vec::new();
    for position in positions_over(&lay.table, &range) {
        let entry = &lay.table.entries[position];
        if entry.flags & ENTRY_CODE == 0 {
            continue;
        }
        let block = entry.block as usize;
        let total = code_line_count(doc, block);
        let lines = lay.table.lines_over(position, total, &range);
        if !lines.is_empty() {
            out.push((block, lines));
        }
    }
    out
}

/// The placed positions whose emission overlaps `range`.
fn positions_over(table: &BlockTable, range: &Range<f32>) -> Range<usize> {
    let start = table.entries.partition_point(|e| e.bottom() < range.start);
    let end = table.entries.partition_point(|e| e.y <= range.end);
    start..end.max(start)
}

/// First index in `0..n` where `pred` turns false; `pred` is monotone.
fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if pred(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

impl BlockTable {
    /// Lines of code position `position` overlapping `range`, out of
    /// `total` lines. A line's bottom is the next line's top, since the
    /// lines tile the panel.
    fn lines_over(&self, position: usize, total: usize, range: &Range<f32>) -> Range<usize> {
        let start = partition(total, |l| self.code_line_top(position, l + 1) < range.start);
        let end = partition(total, |l| self.code_line_top(position, l) <= range.end);
        start..end.max(start)
    }
}

/// A code block's line count; zero for anything else.
fn code_line_count(doc: &Document, block: usize) -> usize {
    match &doc.blocks[block].kind {
        BlockKind::CodeBlock { lines, .. } => lines.len(),
        _ => 0,
    }
}

/// One side of a window fill: whole positions to replay and, at the
/// seam with the surviving window, the line extension of a code block
/// already materialized, shaped without its panel. `fill` is the y
/// range boundary code positions clip their lines to.
#[derive(Debug, Clone)]
struct FillPlan {
    positions: Range<usize>,
    extend: Option<(usize, Range<usize>)>,
    fill: Range<f32>,
}

impl FillPlan {
    fn is_empty(&self) -> bool {
        self.positions.is_empty() && self.extend.as_ref().is_none_or(|(_, l)| l.is_empty())
    }
}

/// One replayed region: elements in document order, absolutely
/// positioned, plus the cumulative marks of each whole position. When
/// the seam extension replays first, `seam` holds its element counts,
/// which the existing seam position's marks absorb.
#[derive(Default)]
struct Assembly {
    doc: LayoutDoc,
    marks: Vec<PosMarks>,
    seam: Option<PosMarks>,
}

impl LayoutDoc {
    /// Whether `window_to` can slide the window. Not before the pass
    /// has settled a first block. Not while elements stand beyond the
    /// marks either: they belong to the pass's open code block, and
    /// moving them would break the indices the pass holds, so the slide
    /// waits for the block to close. A code or text file is one such
    /// block, so its window slides only once the pass has ended.
    pub fn slides(&self) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let ends = window.marks.back().cloned().unwrap_or_else(PosMarks::zero);
        self.runs.len() <= ends.runs
            && self.rects.len() <= ends.rects
            && self.images.len() <= ends.images
            && self.math_glyphs.len() <= ends.math
            && self.table_rows.len() <= ends.rows
            && self.code_lines.len() <= ends.code
    }
}

/// Slides the materialized window to cover the band at `scroll`,
/// evicting what fell behind and re-shaping missing blocks at their
/// recorded positions, through the pool when one is given. With
/// `fill_band` false only the viewport is filled, the interactive
/// path's cheaper frame; eviction still holds the band's bounds.
/// Answers whether the materialized window changed.
#[allow(clippy::too_many_arguments)]
pub fn window_to(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    lay: &mut LayoutDoc,
    pool: Option<&std::sync::Arc<crate::layout::pool::ShapePool>>,
    scroll: f32,
    viewport_h: f32,
    fill_band: bool,
) -> bool {
    if !lay.slides() || lay.table.entries.is_empty() {
        return false;
    }
    let range = retain_range(scroll, viewport_h);
    let fill = if fill_band {
        range.clone()
    } else {
        scroll..scroll + viewport_h
    };
    let keep = positions_over(&lay.table, &range);
    let target = positions_over(&lay.table, &fill);
    let window = lay.window.as_ref().expect("windowed layout");
    let current = window.start..window.start + window.marks.len();

    // What survives eviction. A target beyond the survivors, with a gap
    // of positions between, starts the window over instead of bridging;
    // a far jump inside a boundary code block is the same case at line
    // granularity, since a position's lines stay one contiguous range.
    let kept = current.start.max(keep.start)..current.end.min(keep.end);
    let fresh = kept.is_empty()
        || target.end < kept.start
        || target.start > kept.end
        || line_jump(doc, lay, &kept, &target, &range, &fill);

    if fresh {
        if target.is_empty() && current.is_empty() {
            return false;
        }
        let plan = FillPlan {
            positions: target.clone(),
            extend: None,
            fill,
        };
        let take = seed_fills(doc, theme, cfg, &lay.table, pool, &plan, None);
        lay.window_clear(target.start);
        let assembly = replay_fill(doc, theme, fonts, media, cfg, &lay.table, &plan, &take);
        lay.window_append(assembly, plan.extend.clone());
        lay.compact_side();
        return true;
    }

    // Boundary code lines, planned against the pre-eviction marks: what
    // the range no longer covers goes, what the fill adds comes back.
    let (evict_lines_front, evict_lines_back) = plan_line_evictions(doc, lay, &kept, &range);
    let front = plan_fill_front(doc, lay, &kept, &target, &fill);
    let back = plan_fill_back(doc, lay, &kept, &target, &fill);

    let evict_front = kept.start - current.start;
    let evict_back = current.end - kept.end;
    let changed = evict_front > 0
        || evict_back > 0
        || evict_lines_front.is_some()
        || evict_lines_back.is_some()
        || !front.is_empty()
        || !back.is_empty();
    if !changed {
        return false;
    }

    let take = seed_fills(doc, theme, cfg, &lay.table, pool, &front, Some(&back));
    if evict_front > 0 {
        lay.window_drop_front(evict_front);
    }
    if evict_back > 0 {
        lay.window_drop_back(evict_back);
    }
    if let Some(upto) = evict_lines_front {
        lay.window_evict_code_front(upto);
    }
    if let Some(from) = evict_lines_back {
        lay.window_evict_code_back(from);
    }
    if !front.is_empty() {
        let assembly = replay_fill(doc, theme, fonts, media, cfg, &lay.table, &front, &take);
        lay.window_prepend(assembly, front.positions.start, front.extend.clone());
    }
    if !back.is_empty() {
        let assembly = replay_fill(doc, theme, fonts, media, cfg, &lay.table, &back, &take);
        lay.window_append(assembly, back.extend.clone());
    }
    lay.compact_side();
    true
}

/// Whether the fill needs lines of a kept boundary code position with
/// a gap to what stays materialized there. Extending would shape every
/// line across the gap, so the caller starts the window over instead.
fn line_jump(
    doc: &Document,
    lay: &LayoutDoc,
    kept: &Range<usize>,
    target: &Range<usize>,
    range: &Range<f32>,
    fill: &Range<f32>,
) -> bool {
    let window = lay.window.as_ref().expect("windowed layout");
    let jump = |position: usize| {
        let entry = &lay.table.entries[position];
        if entry.flags & ENTRY_CODE == 0 || !target.contains(&position) {
            return false;
        }
        let total = code_line_count(doc, entry.block as usize);
        let allowed = lay.table.lines_over(position, total, range);
        let lines = &window.marks[position - window.start].lines;
        // The segment line eviction will leave, including its collapse
        // point when the range misses every materialized line.
        let start = allowed.start.clamp(lines.start, lines.end);
        let end = allowed.end.clamp(start, lines.end);
        let need = lay.table.lines_over(position, total, fill);
        need.end < start || need.start > end
    };
    jump(kept.start) || jump(kept.end - 1)
}

/// The planned evictions of boundary code lines outside the retention
/// range: the first kept position's new line start, the last one's new
/// line end, None where nothing goes.
fn plan_line_evictions(
    doc: &Document,
    lay: &LayoutDoc,
    kept: &Range<usize>,
    range: &Range<f32>,
) -> (Option<usize>, Option<usize>) {
    let window = lay.window.as_ref().expect("windowed layout");
    let mark_of = |position: usize| &window.marks[position - window.start];
    let mut front = None;
    let mut back = None;
    let first = &lay.table.entries[kept.start];
    if first.flags & ENTRY_CODE != 0 {
        let total = code_line_count(doc, first.block as usize);
        let allowed = lay.table.lines_over(kept.start, total, range);
        let lines = &mark_of(kept.start).lines;
        if lines.start < allowed.start {
            front = Some(allowed.start.min(lines.end));
        }
    }
    let last_pos = kept.end - 1;
    let last = &lay.table.entries[last_pos];
    if last.flags & ENTRY_CODE != 0 {
        let total = code_line_count(doc, last.block as usize);
        let allowed = lay.table.lines_over(last_pos, total, range);
        let lines = &mark_of(last_pos).lines;
        if lines.end > allowed.end {
            back = Some(allowed.end.max(lines.start));
        }
    }
    (front, back)
}

/// The front fill: positions ahead of the surviving window and, at the
/// seam, the first kept code block's upward line extension.
fn plan_fill_front(
    doc: &Document,
    lay: &LayoutDoc,
    kept: &Range<usize>,
    target: &Range<usize>,
    fill: &Range<f32>,
) -> FillPlan {
    let positions = target.start..kept.start.min(target.end).max(target.start);
    let mut extend = None;
    let first = &lay.table.entries[kept.start];
    if first.flags & ENTRY_CODE != 0 && target.contains(&kept.start) {
        let window = lay.window.as_ref().expect("windowed layout");
        let lines = &window.marks[kept.start - window.start].lines;
        let total = code_line_count(doc, first.block as usize);
        let need = lay.table.lines_over(kept.start, total, fill);
        if need.start < lines.start {
            extend = Some((kept.start, need.start..lines.start));
        }
    }
    FillPlan {
        positions,
        extend,
        fill: fill.clone(),
    }
}

/// The back fill: the last kept code block's downward line extension
/// and the positions behind the surviving window.
fn plan_fill_back(
    doc: &Document,
    lay: &LayoutDoc,
    kept: &Range<usize>,
    target: &Range<usize>,
    fill: &Range<f32>,
) -> FillPlan {
    let positions = kept.end.max(target.start)..target.end.max(kept.end);
    let mut extend = None;
    let last_pos = kept.end - 1;
    let last = &lay.table.entries[last_pos];
    if last.flags & ENTRY_CODE != 0 && target.contains(&last_pos) {
        let window = lay.window.as_ref().expect("windowed layout");
        let lines = &window.marks[last_pos - window.start].lines;
        let total = code_line_count(doc, last.block as usize);
        let need = lay.table.lines_over(last_pos, total, fill);
        if need.end > lines.end {
            extend = Some((last_pos, lines.end..need.end));
        }
    }
    FillPlan {
        positions,
        extend,
        fill: fill.clone(),
    }
}

/// Where a block's output starts, so its decoration splices under its own
/// rects and centering moves only its own runs.
#[derive(Clone, Copy)]
struct Marks {
    rects: usize,
    runs: usize,
    images: usize,
    math: usize,
}

/// What a block derives on entry and needs again when it finishes.
#[derive(Clone, Copy)]
struct Frame {
    marks: Marks,
    /// Where the block's own lines start, after the title of an alert
    /// it opens: the block's alignment moves these and leaves the title.
    lines: Marks,
    x_base: f32,
    avail: f32,
    gap: f32,
    region_top: f32,
    base_size: f32,
    is_list: bool,
}

/// A code block placed over several steps. Its panel is pushed on entry
/// and grows as lines land, which moves no index. Line positions are
/// closed-form over the line index, with wrapped lines' extra heights
/// accumulated in order, the same arithmetic the block table replays.
struct OpenCode {
    block: usize,
    /// The block's order position, the key its line steps pool under.
    position: usize,
    frame: Frame,
    /// Panel background rect; the border rect follows it. Meaningless
    /// while `framed` is false, when no rects were pushed.
    panel: usize,
    /// Whether the block draws its panel; a whole-file code document
    /// does not.
    framed: bool,
    y0: f32,
    pad: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
    line: usize,
    /// Height beyond the shared line height accumulated by wrapped lines.
    extra: f32,
    /// Lines inside the retention bound so far.
    kept: Option<Range<usize>>,
    /// Element state at the block's start, for dropping it whole.
    counts: ElementCounts,
}

impl OpenCode {
    /// The top of the next line to place: the bottom of what is placed.
    fn line_top(&self) -> f32 {
        code_line_y(self.y0 + self.pad, self.line, self.line_height, self.extra)
    }
}

pub fn layout(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    viewport_width: f32,
) -> LayoutDoc {
    let (mut out, mut pass) = layout_begin(doc, cfg, viewport_width);
    layout_more(doc, theme, fonts, media, cfg, &mut out, &mut pass, None);
    out
}

/// Starts a pass: derives the geometry and the block order.
pub fn layout_begin(
    doc: &Document,
    cfg: &ViewConfig,
    viewport_width: f32,
) -> (LayoutDoc, LayoutPass) {
    let right = metrics::MARGIN_RATIO * viewport_width;
    // A file of lines keeps room for its line numbers on the left: the
    // margin itself while the digits fit it, more when they do not.
    let margin = if doc.code_file || doc.plain_file {
        right.max(cfg.gutter)
    } else {
        right
    };
    let vertical_margin = metrics::VERTICAL_MARGIN_EM * cfg.body_size * cfg.zoom;
    let content_width = (viewport_width - margin - right).max(50.0);

    // Footnote definitions collect at the document end under a rule,
    // wherever the source declared them. Model indices stay untouched so
    // selection and copy keep their mapping. Blocks folded inside a
    // closed details group never enter the order; a toggle restarts the
    // pass, so visibility is fixed for its lifetime.
    let (body_order, mut note_order): (Vec<usize>, Vec<usize>) = (0..doc.blocks.len())
        .filter(|&i| doc.block_visible(i))
        .partition(|&i| !matches!(doc.blocks[i].kind, BlockKind::FootnoteDef { .. }));
    note_order.sort_by_key(|&i| footnote_number(&doc.blocks[i]));
    let notes_start = body_order.len();
    let has_notes = !note_order.is_empty();
    let order: Vec<usize> = body_order.into_iter().chain(note_order).collect();

    let pass = LayoutPass {
        order,
        position: 0,
        covered: doc.blocks.len(),
        notes_start,
        has_notes,
        cursor: 0.0,
        first: true,
        prev_quote_depth: 0,
        prev_alert: None,
        prev_is_list: false,
        prev_space_below: 0.0,
        plain: doc.plain_file,
        margin,
        content_width,
        vertical_margin,
        open: None,
        done: false,
        scratch: LayoutDoc::default(),
        pool: None,
        pool_generation: 0,
        ctx: None,
        seed_position: 0,
        seed_line: 0,
        retain: None,
        counter: None,
        counting: None,
    };
    let mut out = LayoutDoc::default();
    out.table.margin = margin;
    out.table.content_width = content_width;
    let (family, size) = code_face(doc.plain_file, cfg);
    out.code_family = family.to_string();
    out.code_size = size * cfg.zoom;
    (out, pass)
}

/// Extends a pass over a document that grew by appending blocks, the
/// parse swap's splice. The placed prefix stays; answers false when the
/// pass cannot extend, because a placed footnote section would put
/// appended body blocks after the notes, and the caller restarts instead.
pub fn layout_extend(doc: &Document, pass: &mut LayoutPass) -> bool {
    if pass.has_notes {
        return false;
    }
    // Folded blocks never enter the order, so coverage is tracked as a
    // model high-water mark rather than the order's length.
    let covered = pass.covered;
    if doc.blocks.len() <= covered {
        return true;
    }
    let (body, mut notes): (Vec<usize>, Vec<usize>) = (covered..doc.blocks.len())
        .filter(|&i| doc.block_visible(i))
        .partition(|&i| !matches!(doc.blocks[i].kind, BlockKind::FootnoteDef { .. }));
    notes.sort_by_key(|&i| footnote_number(&doc.blocks[i]));
    pass.covered = doc.blocks.len();
    pass.order.extend(body);
    pass.notes_start = pass.order.len();
    pass.has_notes = !notes.is_empty();
    pass.order.extend(notes);
    pass.done = false;
    true
}

/// Places steps until the deadline, or to the end when there is none.
/// Returns true when the document is complete.
#[allow(clippy::too_many_arguments)]
pub fn layout_more(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
    deadline: Option<Instant>,
) -> bool {
    // A comic's pass is one loop over its page blocks: geometry comes
    // from stored header dimensions, nothing shapes and nothing decodes,
    // so it always completes inside any slice.
    if doc.comic_file {
        if !pass.done {
            layout_comic(doc, media, cfg, out, pass);
        }
        return true;
    }
    pool_sync(doc, theme, cfg, pass);
    // Seed before the first deadline check, so even an expired slice
    // leaves the workers fed for the next one.
    pool_top_up(doc, pass);
    while !pass.done {
        if deadline.is_some_and(|at| Instant::now() >= at) {
            return false;
        }
        pool_top_up(doc, pass);
        layout_step(doc, theme, fonts, media, cfg, out, pass);
    }
    // A bounded pass dropped most blocks but their side text stayed
    // behind; one compaction at the end settles it.
    if pass.retain.is_some() {
        out.compact_side();
    }
    true
}

/// Places a comic's pages under the active fit. The strip fills the
/// viewport width edge to edge with no gap, so webtoon strips stay
/// unbroken; the page states give every page (or pair row) one slot of
/// exactly the viewport height with the drawn page centered inside, so
/// a snapped scroll shows one whole slot. Pair members share their
/// row's slot top in the block table, which is what the outline jump
/// and the page snap read.
fn layout_comic(
    doc: &Document,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
) {
    let viewport = pass.content_width + 2.0 * pass.margin;
    let mut cursor = 0.0f32;
    // A pair row stays open while its left page waits for the right.
    let mut open_row = false;
    let mut page = 0usize;
    for (block_index, block) in doc.blocks.iter().enumerate() {
        let BlockKind::Image { path, .. } = &block.kind else {
            out.table.push(
                block_index,
                BlockEntry {
                    block: block_index as u32,
                    y: cursor,
                    content_y: cursor,
                    height: 0.0,
                    deco_top: f32::NAN,
                    line_height: 0.0,
                    pad: 0.0,
                    x0: 0.0,
                    flags: ENTRY_SILENT,
                },
            );
            continue;
        };
        // A page whose header would not read keeps a slot in the page
        // ratio; its pixels never arrive and the slot stays background.
        let (iw, ih) = media
            .dimensions(path)
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((3.0, 4.0));
        let (x, y, w, h, slot_top, advance) = match cfg.comic {
            ComicFit::Width => {
                let w = viewport;
                let h = w * ih / iw;
                (0.0, cursor, w, h, cursor, h)
            }
            ComicFit::Page { height } => {
                let slot = height.max(50.0);
                let scale = (viewport / iw).min(slot / ih);
                let (w, h) = (iw * scale, ih * scale);
                let x = (viewport - w) / 2.0;
                (x, cursor + (slot - h) / 2.0, w, h, cursor, slot)
            }
            ComicFit::Two { height } => {
                let slot = height.max(50.0);
                if page == 0 {
                    let scale = (viewport / iw).min(slot / ih);
                    let (w, h) = (iw * scale, ih * scale);
                    let x = (viewport - w) / 2.0;
                    (x, cursor + (slot - h) / 2.0, w, h, cursor, slot)
                } else {
                    let half = viewport / 2.0;
                    let scale = (half / iw).min(slot / ih);
                    let (w, h) = (iw * scale, ih * scale);
                    let first = page % 2 == 1;
                    let left = first != (cfg.direction == DirectionMode::Rtl);
                    let x = if left { half - w } else { half };
                    let y = cursor + (slot - h) / 2.0;
                    // The first page of a spread holds its row open at
                    // the cursor; its pair closes it and the cursor
                    // moves on. Forced RTL seats the first page right.
                    let advance = if first { 0.0 } else { slot };
                    open_row = first;
                    (x, y, w, h, cursor, advance)
                }
            }
        };
        out.images.push(ImagePlace {
            src: path.clone(),
            x,
            y,
            width: w,
            height: h,
            link: None,
        });
        out.table.push(
            block_index,
            BlockEntry {
                block: block_index as u32,
                y: slot_top,
                content_y: slot_top,
                height: match cfg.comic {
                    ComicFit::Width => h,
                    ComicFit::Page { height } | ComicFit::Two { height } => height.max(50.0),
                },
                deco_top: f32::NAN,
                line_height: 0.0,
                pad: 0.0,
                x0: 0.0,
                flags: 0,
            },
        );
        cursor += advance;
        page += 1;
    }
    if open_row {
        // A last odd page left its row open; the row still spans a slot.
        if let ComicFit::Two { height } = cfg.comic {
            cursor += height.max(50.0);
        }
    }
    out.height = cursor;
    pass.cursor = cursor;
    pass.first = false;
    pass.position = pass.order.len();
    pass.done = true;
}

/// Claims the pool when attached and not current: a fresh pass, a model
/// change, or another pass having taken it over. Seeding restarts at the
/// assembler's own position, so nothing stale is ever consumed.
fn pool_sync(doc: &Document, theme: &Theme, cfg: &ViewConfig, pass: &mut LayoutPass) {
    let Some(pool) = pass.pool.clone() else {
        return;
    };
    if pass.ctx.is_some() && pool.generation() == pass.pool_generation {
        return;
    }
    pass.pool_generation = pool.begin();
    pass.ctx = Some(std::sync::Arc::new(ShapeCtx {
        theme: theme.clone(),
        cfg: cfg.clone(),
        source: std::sync::Arc::clone(&doc.source),
    }));
    match &pass.open {
        Some(open) => {
            pass.seed_position = open.position;
            pass.seed_line = open.line;
        }
        None => {
            pass.seed_position = pass.position;
            pass.seed_line = 0;
        }
    }
}

/// Keeps the claim queue ahead of the assembler by a bounded window.
fn pool_top_up(doc: &Document, pass: &mut LayoutPass) {
    let Some(pool) = pass.pool.clone() else {
        return;
    };
    let Some(ctx) = pass.ctx.clone() else {
        return;
    };
    let window = 4 * pool.width();
    // The backlog is read behind two locks: once here and once after
    // each job, not at each line the loop passes without a job.
    let mut backlog = pool.backlog();
    while backlog < window && pass.seed_position < pass.order.len() {
        let index = pass.order[pass.seed_position];
        let block = &doc.blocks[index];
        if let BlockKind::CodeBlock {
            lines, highlights, ..
        } = &block.kind
        {
            if pass.seed_line >= lines.len() {
                pass.seed_position += 1;
                pass.seed_line = 0;
                continue;
            }
            let line_index = pass.seed_line;
            pass.seed_line += 1;
            let line = lines.line(&doc.source, line_index);
            if line.is_empty() {
                continue;
            }
            // A line the assembler counts needs no shaping.
            if pass.counting == Some(pass.seed_position)
                && pass.counter.is_some()
                && super::rows::countable(line)
                && highlights.get(line_index).is_none_or(|s| regular_face(s))
            {
                continue;
            }
            // Mirrors open_code: the pad and wrap width the assembler
            // will place the line against.
            let (x_base, avail) = block_geometry(
                block,
                pass.margin,
                pass.content_width,
                &ctx.cfg,
                &doc.source,
            );
            let size = code_face(doc.plain_file, &ctx.cfg).1 * ctx.cfg.zoom;
            let pad = if code_framed(doc) {
                metrics::CODE_PAD * ctx.cfg.zoom
            } else {
                0.0
            };
            pool.submit(Job {
                generation: pass.pool_generation,
                key: StepKey::step(pass.seed_position, line_index),
                ctx: std::sync::Arc::clone(&ctx),
                work: Work::CodeLine {
                    line: line.to_string(),
                    segments: highlights.get(line_index).cloned().unwrap_or_default(),
                    block_index: index,
                    line_index,
                    x0: x_base + pad,
                    size,
                    line_height: metrics::LINE_HEIGHT * size,
                    wrap_width: (avail - 2.0 * pad).max(40.0),
                    prose: doc.plain_file,
                },
            });
            backlog = pool.backlog();
        } else {
            let position = pass.seed_position;
            pass.seed_position += 1;
            pass.seed_line = 0;
            if !poolable(block) {
                continue;
            }
            let Some((heading, _, base_size)) = block_metrics(block, &ctx.cfg, doc.plain_file)
            else {
                continue;
            };
            let (x_base, avail) = block_geometry(
                block,
                pass.margin,
                pass.content_width,
                &ctx.cfg,
                &doc.source,
            );
            pool.submit(Job {
                generation: pass.pool_generation,
                key: StepKey::step(position, 0),
                ctx: std::sync::Arc::clone(&ctx),
                work: Work::Block {
                    block: block.clone(),
                    block_index: index,
                    heading,
                    base_size,
                    x_base,
                    avail,
                },
            });
            backlog = pool.backlog();
        }
    }
}

/// Whether the pool may shape this block's kind. Image bearers need the
/// media cache the assembler owns; code blocks pool per line instead;
/// summary rows read the fold state only the assembler's document has.
fn poolable(block: &Block) -> bool {
    match &block.kind {
        BlockKind::Image { .. } | BlockKind::CodeBlock { .. } | BlockKind::Summary { .. } => false,
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. } => !spans.iter().any(|s| s.image.is_some()),
        _ => true,
    }
}

/// The block's own metrics: heading level, list flag, base size. None
/// when the block emits nothing, a styled kind with no spans.
fn block_metrics(block: &Block, cfg: &ViewConfig, plain: bool) -> Option<(Option<u8>, bool, f32)> {
    let heading = match &block.kind {
        BlockKind::Heading { level, .. } => Some(*level),
        _ => None,
    };
    let is_list = matches!(block.kind, BlockKind::ListItem { .. });
    let base_size = match &block.kind {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::ListItem { spans, .. } => {
            if spans.is_empty() {
                return None;
            }
            cfg.body_size * heading.map(metrics::heading_scale).unwrap_or(1.0) * cfg.zoom
        }
        // A summary row renders its chevron even with no text.
        BlockKind::Summary { .. } => cfg.body_size * cfg.zoom,
        BlockKind::CodeBlock { .. } => code_face(plain, cfg).1 * cfg.zoom,
        BlockKind::Rule
        | BlockKind::Table { .. }
        | BlockKind::Image { .. }
        | BlockKind::MathBlock { .. }
        | BlockKind::Frontmatter { .. }
        | BlockKind::ChapterBreak { .. }
        | BlockKind::PageBreak => cfg.body_size * cfg.zoom,
        BlockKind::FootnoteDef { .. } => 0.85 * cfg.body_size * cfg.zoom,
    };
    Some((heading, is_list, base_size))
}

/// A block's base direction, the bidi rule: its first strong character
/// decides, unless the file's direction is forced. Only the text-bearing
/// kinds answer; tables keep their column order and code is always LTR.
fn block_rtl(block: &Block, source: &str, mode: DirectionMode) -> bool {
    let spans: &[Span] = match &block.kind {
        BlockKind::Paragraph { spans } => spans,
        BlockKind::Heading { spans, .. } => spans,
        BlockKind::ListItem { spans, .. } => spans,
        _ => return false,
    };
    match mode {
        DirectionMode::Rtl => true,
        DirectionMode::Ltr => false,
        DirectionMode::Auto => spans
            .iter()
            .flat_map(|span| span.text(source).chars())
            .find_map(crate::style::fonts::strong_rtl)
            .unwrap_or(false),
    }
}

/// The x origin and available width a block shapes against, derived from
/// its quote depth and the pass geometry alone, so seeding and assembly
/// agree by construction. An RTL block's quote indents step in from the
/// right, so its column keeps the left edge and loses the right.
fn block_geometry(
    block: &Block,
    margin: f32,
    content_width: f32,
    cfg: &ViewConfig,
    source: &str,
) -> (f32, f32) {
    let quote_indent = block.quote_depth as f32 * metrics::INDENT * cfg.zoom;
    let quote_pad = if block.quote_depth > 0 {
        12.0 * cfg.zoom
    } else {
        0.0
    };
    let x_base = if block.quote_depth > 0 && block_rtl(block, source, cfg.direction) {
        margin + quote_pad
    } else {
        margin + quote_indent + quote_pad
    };
    let avail = (content_width - quote_indent - 2.0 * quote_pad).max(40.0);
    (x_base, avail)
}

/// Places one step: a whole block, or one line of an open code block.
pub fn layout_step(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
) -> bool {
    if pass.done {
        return true;
    }
    match pass.open.take() {
        Some(open) => place_code_line(doc, theme, fonts, cfg, open, out, pass),
        None if pass.position < pass.order.len() => {
            place_block(doc, theme, fonts, media, cfg, out, pass)
        }
        None => {}
    }
    if pass.open.is_none() && pass.position >= pass.order.len() {
        pass.done = true;
    }
    out.height = placed_height(pass);
    pass.done
}

/// Height of what is placed: the running cursor, or the open code block's
/// panel bottom while it fills.
fn placed_height(pass: &LayoutPass) -> f32 {
    if pass.first {
        return 0.0;
    }
    match &pass.open {
        Some(open) => open.line_top() + open.pad + pass.vertical_margin,
        None => pass.cursor + pass.vertical_margin,
    }
}

fn place_block(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
) {
    let position = pass.position;
    let block_index = pass.order[position];
    pass.position += 1;
    let block = &doc.blocks[block_index];
    let counts = ElementCounts::of(out);

    if pass.has_notes && position == pass.notes_start && !pass.first {
        let size = cfg.body_size * cfg.zoom;
        pass.cursor += metrics::space_above(None, size);
        out.rects.push(DecoRect::fill(
            pass.margin,
            pass.cursor,
            pass.content_width,
            (1.0 * cfg.zoom).max(1.0),
            theme.blocks.rule,
        ));
        out.table.notes_rule = Some((position, pass.cursor));
        pass.cursor += (1.0 * cfg.zoom).max(1.0);
    }
    let Some((heading, is_list, base_size)) = block_metrics(block, cfg, doc.plain_file) else {
        out.table.push(
            block_index,
            BlockEntry {
                block: block_index as u32,
                y: pass.cursor,
                content_y: pass.cursor,
                height: 0.0,
                deco_top: f32::NAN,
                line_height: 0.0,
                pad: 0.0,
                x0: 0.0,
                flags: ENTRY_SILENT,
            },
        );
        settle_retention(out, pass, &counts, 0..0);
        return;
    };
    let mut gap = 0.0;
    if pass.first {
        pass.cursor = pass.vertical_margin;
        pass.first = false;
    } else if doc.plain_file {
        // A plain file's rows are uniform: blank lines are real rows
        // inside the blocks, so blocks meet with no added gap.
    } else if is_list && pass.prev_is_list {
        gap = 0.25 * base_size;
        pass.cursor += gap;
    } else {
        gap = metrics::space_above(heading, base_size);
        pass.cursor += gap;
    }
    if let BlockKind::Heading { anchor, .. } = &block.kind {
        out.anchors.push((anchor.clone(), pass.cursor));
    }

    let (x_base, avail) = block_geometry(block, pass.margin, pass.content_width, cfg, &doc.source);
    let marks = Marks {
        rects: out.rects.len(),
        runs: out.runs.len(),
        images: out.images.len(),
        math: out.math_glyphs.len(),
    };

    // The first block of an alert region gets the bold title line; the
    // quote panel later extends up to cover it.
    let alert_start =
        block.alert.is_some() && (pass.prev_quote_depth == 0 || pass.prev_alert != block.alert);
    let region_top = pass.cursor;
    let mut scratch = std::mem::take(&mut pass.scratch);
    if alert_start {
        let kind = block.alert.expect("alert start has a kind");
        let title_h = shape_alert_title(
            fonts,
            theme,
            cfg,
            &doc.source,
            kind,
            block_index,
            x_base,
            avail,
            &mut scratch,
        );
        out.splice(&mut scratch, pass.cursor);
        pass.cursor += title_h + 0.25 * base_size;
    }
    let lines = Marks {
        rects: out.rects.len(),
        runs: out.runs.len(),
        images: out.images.len(),
        math: out.math_glyphs.len(),
    };

    let frame = Frame {
        marks,
        lines,
        x_base,
        avail,
        gap,
        region_top,
        base_size,
        is_list,
    };

    let is_code = matches!(block.kind, BlockKind::CodeBlock { .. });
    let size = code_face(doc.plain_file, cfg).1 * cfg.zoom;
    let pad = if is_code && code_framed(doc) {
        metrics::CODE_PAD * cfg.zoom
    } else {
        0.0
    };
    out.table.push(
        block_index,
        BlockEntry {
            block: block_index as u32,
            y: region_top,
            content_y: pass.cursor,
            height: 0.0,
            deco_top: f32::NAN,
            line_height: if is_code {
                metrics::LINE_HEIGHT * size
            } else {
                0.0
            },
            pad,
            x0: x_base + pad,
            flags: if is_code { ENTRY_CODE } else { 0 }
                | if alert_start { ENTRY_ALERT_TITLE } else { 0 },
        },
    );

    // A code file is one block, so it is opened and its lines land
    // over as many steps as the slice budget allows.
    if is_code {
        pass.scratch = scratch;
        let open = open_code(
            theme,
            cfg,
            block_index,
            frame,
            counts,
            code_framed(doc),
            doc.plain_file,
            out,
            pass,
        );
        place_code_line(doc, theme, fonts, cfg, open, out, pass);
        return;
    }
    if let BlockKind::FootnoteDef {
        label,
        continued: false,
        ..
    } = &block.kind
    {
        out.anchors.push((format!("footnote:{label}"), pass.cursor));
    }
    if let Some(shaped) = pass
        .pool
        .as_ref()
        .and_then(|pool| pool.take(pass.pool_generation, StepKey::step(position, 0)))
    {
        pass.scratch = scratch;
        let mut ready = shaped.scratch;
        out.splice(&mut ready, pass.cursor);
        finish_block(
            theme,
            cfg,
            block,
            frame,
            shaped.height,
            out,
            pass,
            &doc.source,
        );
        settle_retention(out, pass, &counts, 0..0);
        return;
    }
    let summary_open = match &block.kind {
        BlockKind::Summary { group, .. } => doc.details[*group as usize].open,
        _ => false,
    };
    let height = shape_kind(
        fonts,
        theme,
        cfg,
        &doc.source,
        media,
        block,
        block_index,
        heading,
        base_size,
        x_base,
        avail,
        summary_open,
        &mut scratch,
    );
    out.splice(&mut scratch, pass.cursor);
    pass.scratch = scratch;
    finish_block(theme, cfg, block, frame, height, out, pass, &doc.source);
    settle_retention(out, pass, &counts, 0..0);
}

/// Shapes one block's own emission into the scratch from zero: the kind
/// dispatch the serial pass and a pool worker share. Code blocks never
/// come here, since they open and step per line, and a worker never sees
/// a block whose spans carry images, so its media cache stays untouched.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_kind(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    block: &Block,
    block_index: usize,
    heading: Option<u8>,
    base_size: f32,
    x_base: f32,
    avail: f32,
    summary_open: bool,
    scratch: &mut LayoutDoc,
) -> f32 {
    match &block.kind {
        BlockKind::Summary { spans, .. } => layout_summary(
            fonts,
            theme,
            cfg,
            source,
            media,
            spans,
            summary_open,
            block_index,
            x_base,
            avail,
            scratch,
        ),
        BlockKind::Heading { spans, .. } | BlockKind::Paragraph { spans } => {
            let base = BlockStyle {
                size: base_size,
                color: match heading {
                    Some(level) => heading_color(theme, level),
                    None => theme.text.body,
                },
                bold: heading.is_some(),
                block_index,
                justify: heading.is_none() && cfg.justify,
                align: None,
            };
            flow_or_shape(
                fonts, theme, cfg, source, media, spans, &base, x_base, 0.0, avail, scratch,
            )
        }
        BlockKind::ListItem {
            marker,
            depth,
            spans,
        } => layout_list_item(
            fonts,
            theme,
            cfg,
            source,
            media,
            marker,
            *depth,
            spans,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
        BlockKind::CodeBlock { .. } => 0.0,
        // The chapter seam: one blank body line, nothing drawn; the
        // block spacing on both sides completes the larger gap.
        BlockKind::ChapterBreak { .. } => metrics::LINE_HEIGHT * base_size,
        // A page break takes the seam's blank line. On screen a dashed
        // hairline in the rule color shows where the page ends; the
        // export draws nothing, the break itself is the paginator's.
        BlockKind::PageBreak => {
            let height = metrics::LINE_HEIGHT * base_size;
            if !cfg.print {
                let thickness = (1.0 * cfg.zoom).max(1.0);
                let dash = 8.0 * cfg.zoom;
                let gap = 6.0 * cfg.zoom;
                let y = ((height - thickness) / 2.0).round();
                let end = x_base + avail;
                let mut x = x_base;
                while x < end {
                    let width = dash.min(end - x);
                    scratch
                        .rects
                        .push(DecoRect::fill(x, y, width, thickness, theme.blocks.rule));
                    x += dash + gap;
                }
            }
            height
        }
        BlockKind::Rule => {
            let thickness = (1.0 * cfg.zoom).max(1.0);
            scratch.rects.push(DecoRect::fill(
                x_base,
                0.0,
                avail,
                thickness,
                theme.blocks.rule,
            ));
            thickness
        }
        BlockKind::Table {
            header,
            rows,
            aligns,
        } => layout_table(
            fonts,
            theme,
            cfg,
            source,
            media,
            header,
            rows,
            aligns,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
        BlockKind::Image { path, alt } => layout_image(
            fonts,
            theme,
            cfg,
            source,
            media,
            path,
            alt,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
        BlockKind::Frontmatter { entries } => layout_frontmatter(
            fonts,
            theme,
            cfg,
            source,
            entries,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
        BlockKind::MathBlock { tex } => layout_math_block(
            fonts,
            theme,
            cfg,
            tex,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
        BlockKind::FootnoteDef {
            label,
            number,
            continued,
            spans,
        } => layout_footnote_def(
            fonts,
            theme,
            cfg,
            source,
            media,
            label,
            *number,
            *continued,
            spans,
            base_size,
            block_index,
            x_base,
            0.0,
            avail,
            scratch,
        ),
    }
}

impl LayoutDoc {
    /// Empties every element vector; the window starts over at `start`.
    fn window_clear(&mut self, start: usize) {
        self.runs.clear();
        self.rects.clear();
        self.images.clear();
        self.math_glyphs.clear();
        self.table_rows.clear();
        self.code_lines.clear();
        let window = self.window.as_mut().expect("windowed layout");
        window.marks.clear();
        window.start = start;
        self.index = YIndex::default();
    }

    /// Drops the first `count` materialized positions' elements.
    fn window_drop_front(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let window = self.window.as_mut().expect("windowed layout");
        let base = window.marks[count - 1].clone();
        window.marks.drain(..count);
        for mark in window.marks.iter_mut() {
            mark.runs -= base.runs;
            mark.rects -= base.rects;
            mark.images -= base.images;
            mark.math -= base.math;
            mark.rows -= base.rows;
            mark.code -= base.code;
        }
        window.start += count;
        self.runs.drain(..base.runs);
        self.rects.drain(..base.rects);
        self.images.drain(..base.images);
        self.math_glyphs.drain(..base.math);
        self.table_rows.drain(..base.rows);
        self.code_lines.drain(..base.code);
        for record in &mut self.code_lines {
            record.runs = record.runs.start - base.runs..record.runs.end - base.runs;
        }
        self.index = YIndex::default();
    }

    /// Whether the code line records, the run vector and the window's
    /// marks agree: every record's runs lie inside the vector and rise
    /// with the records, the marks rise and stay inside both, and each
    /// position's records lie inside the position's own runs. The error
    /// names the first thing out of step. Every operation that moves
    /// runs has to leave this true; the tests hold them to it.
    pub fn records_consistent(&self) -> Result<(), String> {
        let runs = self.runs.len();
        let mut floor = 0;
        for (i, record) in self.code_lines.iter().enumerate() {
            let range = &record.runs;
            if range.start > range.end || range.end > runs || range.start < floor {
                return Err(format!(
                    "record {i} (block {}, line {}) holds runs {range:?} after {floor}, of {runs}",
                    record.block, record.line
                ));
            }
            floor = range.end;
        }
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let (mut lo_runs, mut lo_code) = (0, 0);
        for (i, mark) in window.marks.iter().enumerate() {
            if mark.runs < lo_runs
                || mark.code < lo_code
                || mark.runs > runs
                || mark.code > self.code_lines.len()
            {
                return Err(format!(
                    "mark {i} ends at run {} and record {} after {lo_runs} and {lo_code}, of {runs} and {}",
                    mark.runs,
                    mark.code,
                    self.code_lines.len()
                ));
            }
            for record in &self.code_lines[lo_code..mark.code] {
                if record.runs.start < lo_runs || record.runs.end > mark.runs {
                    return Err(format!(
                        "position {i} spans runs {lo_runs}..{} and its record (block {}, line {}) holds {:?}",
                        mark.runs, record.block, record.line, record.runs
                    ));
                }
            }
            (lo_runs, lo_code) = (mark.runs, mark.code);
        }
        Ok(())
    }

    /// Drops the last `count` materialized positions' elements.
    fn window_drop_back(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let window = self.window.as_mut().expect("windowed layout");
        let len = window.marks.len();
        let keep = if len > count {
            window.marks[len - count - 1].clone()
        } else {
            PosMarks::zero()
        };
        window.marks.truncate(len - count);
        self.runs.truncate(keep.runs);
        self.rects.truncate(keep.rects);
        self.images.truncate(keep.images);
        self.math_glyphs.truncate(keep.math);
        self.table_rows.truncate(keep.rows);
        self.code_lines.truncate(keep.code);
        self.index = YIndex::default();
    }

    /// Drops the first position's code lines before `upto`. Records of
    /// the first position are the first `marks[0].code` records, and a
    /// code position's runs are exactly its line runs, so the drained
    /// spans come straight from the records.
    fn window_evict_code_front(&mut self, upto: usize) {
        let window = self.window.as_mut().expect("windowed layout");
        let mark0 = window.marks.front().expect("a first position").clone();
        let upto = upto.clamp(mark0.lines.start, mark0.lines.end);
        let cut = self.code_lines[..mark0.code].partition_point(|c| c.line < upto);
        if cut > 0 {
            let lo = self.code_lines[0].runs.start;
            let hi = self.code_lines[cut - 1].runs.end;
            self.runs.drain(lo..hi);
            self.code_lines.drain(..cut);
            for record in &mut self.code_lines {
                if record.runs.start >= hi {
                    record.runs = record.runs.start - (hi - lo)..record.runs.end - (hi - lo);
                }
            }
            for mark in window.marks.iter_mut() {
                mark.runs -= hi - lo;
                mark.code -= cut;
            }
        }
        window
            .marks
            .front_mut()
            .expect("a first position")
            .lines
            .start = upto;
        self.index = YIndex::default();
    }

    /// Drops the last position's code lines from `from` on.
    fn window_evict_code_back(&mut self, from: usize) {
        let window = self.window.as_mut().expect("windowed layout");
        let len = window.marks.len();
        let last = window.marks.back().expect("a last position").clone();
        let from = from.clamp(last.lines.start, last.lines.end);
        let prev_code = if len >= 2 {
            window.marks[len - 2].code
        } else {
            0
        };
        let records = &self.code_lines[prev_code..last.code];
        let cut = prev_code + records.partition_point(|c| c.line < from);
        if cut < last.code {
            let lo = self.code_lines[cut].runs.start;
            self.runs.truncate(lo);
            self.code_lines.truncate(cut);
            let mark = window.marks.back_mut().expect("a last position");
            mark.runs = lo;
            mark.code = cut;
        }
        window.marks.back_mut().expect("a last position").lines.end = from;
        self.index = YIndex::default();
    }

    /// Splices a replayed assembly in front of the window: whole
    /// positions from `new_start`, then the seam extension of the first
    /// surviving position.
    fn window_prepend(
        &mut self,
        mut assembly: Assembly,
        new_start: usize,
        extend: Option<(usize, Range<usize>)>,
    ) {
        let (side_base, family_map) = self.merge_refs(&mut assembly.doc);
        for run in &mut assembly.doc.runs {
            if let TextRef::Side { start, .. } = &mut run.text {
                *start += side_base;
            }
            run.family = family_map[run.family as usize];
        }
        let added = PosMarks::of(&assembly.doc, 0..0);
        for record in &mut self.code_lines {
            record.runs = record.runs.start + added.runs..record.runs.end + added.runs;
        }
        let window = self.window.as_mut().expect("windowed layout");
        for mark in window.marks.iter_mut() {
            mark.runs += added.runs;
            mark.rects += added.rects;
            mark.images += added.images;
            mark.math += added.math;
            mark.rows += added.rows;
            mark.code += added.code;
        }
        if let Some((_, lines)) = extend {
            let mark0 = window.marks.front_mut().expect("a seam position");
            mark0.lines.start = lines.start;
        }
        for mark in assembly.marks.drain(..).rev() {
            window.marks.push_front(mark);
        }
        window.start = new_start;
        self.runs.splice(0..0, assembly.doc.runs.drain(..));
        self.rects.splice(0..0, assembly.doc.rects.drain(..));
        self.images.splice(0..0, assembly.doc.images.drain(..));
        self.math_glyphs
            .splice(0..0, assembly.doc.math_glyphs.drain(..));
        self.table_rows
            .splice(0..0, assembly.doc.table_rows.drain(..));
        self.code_lines
            .splice(0..0, assembly.doc.code_lines.drain(..));
        for tb in assembly.doc.checkboxes.drain(..) {
            self.push_checkbox(tb);
        }
        self.index = YIndex::default();
    }

    /// Appends a replayed assembly behind the window: the seam
    /// extension of the last surviving position, then whole positions.
    fn window_append(&mut self, mut assembly: Assembly, extend: Option<(usize, Range<usize>)>) {
        let (side_base, family_map) = self.merge_refs(&mut assembly.doc);
        for run in &mut assembly.doc.runs {
            if let TextRef::Side { start, .. } = &mut run.text {
                *start += side_base;
            }
            run.family = family_map[run.family as usize];
        }
        let base = PosMarks::of(self, 0..0);
        for record in &mut assembly.doc.code_lines {
            record.runs = record.runs.start + base.runs..record.runs.end + base.runs;
        }
        let window = self.window.as_mut().expect("windowed layout");
        if let Some(seam) = assembly.seam.take() {
            let last = window.marks.back_mut().expect("a seam position");
            last.runs += seam.runs;
            last.rects += seam.rects;
            last.images += seam.images;
            last.math += seam.math;
            last.rows += seam.rows;
            last.code += seam.code;
            if let Some((_, lines)) = &extend {
                last.lines.end = lines.end;
            }
        }
        for mark in assembly.marks.drain(..) {
            window.marks.push_back(PosMarks {
                runs: base.runs + mark.runs,
                rects: base.rects + mark.rects,
                images: base.images + mark.images,
                math: base.math + mark.math,
                rows: base.rows + mark.rows,
                code: base.code + mark.code,
                lines: mark.lines,
            });
        }
        self.runs.append(&mut assembly.doc.runs);
        self.rects.append(&mut assembly.doc.rects);
        self.images.append(&mut assembly.doc.images);
        self.math_glyphs.append(&mut assembly.doc.math_glyphs);
        self.table_rows.append(&mut assembly.doc.table_rows);
        self.code_lines.append(&mut assembly.doc.code_lines);
        for tb in assembly.doc.checkboxes.drain(..) {
            self.push_checkbox(tb);
        }
    }

    /// Drops emitted geometry from the vectors' front: the fused
    /// export's drain behind its pagination cursor. Code line records
    /// fully behind the run frontier go with it, the rest shift. The
    /// caller rebases everything of its own that indexes the vectors;
    /// the answer is the count of code line records that went.
    pub(crate) fn drain_front(
        &mut self,
        runs: usize,
        rects: usize,
        images: usize,
        math: usize,
    ) -> usize {
        debug_assert!(self.window.is_none(), "the drain and the window exclude");
        let code = self.code_lines.partition_point(|c| c.runs.end <= runs);
        self.runs.drain(..runs);
        self.rects.drain(..rects);
        self.images.drain(..images);
        self.math_glyphs.drain(..math);
        self.code_lines.drain(..code);
        // The frontier can fall between the rows of one wrapped line:
        // its record stays, with the runs that remain.
        for record in &mut self.code_lines {
            record.runs = record.runs.start.saturating_sub(runs)..record.runs.end - runs;
        }
        self.index = YIndex::default();
        code
    }

    /// Rebuilds the side buffer over the live references once evictions
    /// have left it mostly garbage. The threshold keeps the walk off
    /// every frame.
    fn compact_side(&mut self) {
        let live: usize = self
            .runs
            .iter()
            .filter_map(|r| match r.text {
                TextRef::Side { len, .. } => Some(len as usize),
                TextRef::Model { .. } => None,
            })
            .sum();
        if self.side.len() < 64 * 1024 || self.side.len() < live * 2 {
            return;
        }
        let mut side = String::with_capacity(live);
        for run in &mut self.runs {
            if let TextRef::Side { start, len } = &mut run.text {
                let text = &self.side[*start as usize..(*start + *len) as usize];
                *start = side.len() as u32;
                side.push_str(text);
            }
        }
        self.side = side;
    }
}

/// Seeds every fill's shaping through the pool in ascending key order
/// and answers the taker the assemblies consume with. Without a pool
/// the taker answers nothing and the replay shapes serially.
fn seed_fills(
    doc: &Document,
    theme: &Theme,
    cfg: &ViewConfig,
    table: &BlockTable,
    pool: Option<&std::sync::Arc<crate::layout::pool::ShapePool>>,
    first: &FillPlan,
    second: Option<&FillPlan>,
) -> impl Fn(StepKey) -> Option<crate::layout::pool::Shaped> {
    let claimed = pool.map(|pool| {
        let generation = pool.begin();
        let ctx = std::sync::Arc::new(ShapeCtx {
            theme: theme.clone(),
            cfg: cfg.clone(),
            source: std::sync::Arc::clone(&doc.source),
        });
        seed_plan(doc, table, pool, generation, &ctx, first);
        if let Some(second) = second {
            seed_plan(doc, table, pool, generation, &ctx, second);
        }
        (std::sync::Arc::clone(pool), generation)
    });
    move |key| {
        claimed
            .as_ref()
            .and_then(|(pool, generation)| pool.take(*generation, key))
    }
}

fn seed_plan(
    doc: &Document,
    table: &BlockTable,
    pool: &crate::layout::pool::ShapePool,
    generation: u64,
    ctx: &std::sync::Arc<ShapeCtx>,
    plan: &FillPlan,
) {
    let seed_lines = |position: usize, lines: Range<usize>| {
        let entry = &table.entries[position];
        let block = &doc.blocks[entry.block as usize];
        let BlockKind::CodeBlock {
            lines: source_lines,
            highlights,
            ..
        } = &block.kind
        else {
            return;
        };
        let (x_base, avail) = block_geometry(
            block,
            table.margin,
            table.content_width,
            &ctx.cfg,
            &doc.source,
        );
        for line in lines {
            let text = source_lines.line(&doc.source, line);
            if text.is_empty() {
                continue;
            }
            pool.submit(Job {
                generation,
                key: StepKey::step(position, line),
                ctx: std::sync::Arc::clone(ctx),
                work: Work::CodeLine {
                    line: text.to_string(),
                    segments: highlights.get(line).cloned().unwrap_or_default(),
                    block_index: entry.block as usize,
                    line_index: line,
                    x0: x_base + entry.pad,
                    size: code_face(doc.plain_file, &ctx.cfg).1 * ctx.cfg.zoom,
                    line_height: entry.line_height,
                    wrap_width: (avail - 2.0 * entry.pad).max(40.0),
                    prose: doc.plain_file,
                },
            });
        }
    };
    let seam_first = plan
        .extend
        .as_ref()
        .is_some_and(|(p, _)| *p < plan.positions.start);
    if seam_first {
        if let Some((position, lines)) = &plan.extend {
            seed_lines(*position, lines.clone());
        }
    }
    for position in plan.positions.clone() {
        let entry = &table.entries[position];
        if entry.flags & ENTRY_SILENT != 0 {
            continue;
        }
        let block = &doc.blocks[entry.block as usize];
        if entry.flags & ENTRY_CODE != 0 {
            let total = code_line_count(doc, entry.block as usize);
            seed_lines(position, table.lines_over(position, total, &plan.fill));
            continue;
        }
        if !poolable(block) {
            continue;
        }
        let Some((heading, _, base_size)) = block_metrics(block, &ctx.cfg, doc.plain_file) else {
            continue;
        };
        let (x_base, avail) = block_geometry(
            block,
            table.margin,
            table.content_width,
            &ctx.cfg,
            &doc.source,
        );
        pool.submit(Job {
            generation,
            key: StepKey::step(position, 0),
            ctx: std::sync::Arc::clone(ctx),
            work: Work::Block {
                block: block.clone(),
                block_index: entry.block as usize,
                heading,
                base_size,
                x_base,
                avail,
            },
        });
    }
    if !seam_first {
        if let Some((position, lines)) = &plan.extend {
            seed_lines(*position, lines.clone());
        }
    }
}

/// Replays a fill plan into an assembly, whole positions and the seam
/// extension in document order.
#[allow(clippy::too_many_arguments)]
fn replay_fill(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    table: &BlockTable,
    plan: &FillPlan,
    take: &impl Fn(StepKey) -> Option<crate::layout::pool::Shaped>,
) -> Assembly {
    let mut assembly = Assembly::default();
    let seam_first = plan
        .extend
        .as_ref()
        .is_some_and(|(p, _)| *p < plan.positions.start);
    if seam_first {
        if let Some((position, lines)) = &plan.extend {
            replay_code_lines(
                doc,
                theme,
                fonts,
                cfg,
                table,
                *position,
                lines.clone(),
                take,
                &mut assembly.doc,
            );
            assembly.seam = Some(PosMarks::of(&assembly.doc, 0..0));
        }
    }
    for position in plan.positions.clone() {
        let lines = replay_position(
            doc,
            theme,
            fonts,
            media,
            cfg,
            table,
            position,
            &plan.fill,
            take,
            &mut assembly.doc,
        );
        assembly.marks.push(PosMarks::of(&assembly.doc, lines));
    }
    if !seam_first {
        if let Some((position, lines)) = &plan.extend {
            replay_code_lines(
                doc,
                theme,
                fonts,
                cfg,
                table,
                *position,
                lines.clone(),
                take,
                &mut assembly.doc,
            );
        }
    }
    assembly
}

/// Re-emits one recorded position at its recorded y: the notes rule,
/// the alert title, the kind emission, centering and the quote
/// decoration, exactly as the pass placed them. Answers the line range
/// materialized for a code block.
#[allow(clippy::too_many_arguments)]
fn replay_position(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    table: &BlockTable,
    position: usize,
    fill: &Range<f32>,
    take: &impl Fn(StepKey) -> Option<crate::layout::pool::Shaped>,
    out: &mut LayoutDoc,
) -> Range<usize> {
    let entry = &table.entries[position];
    if let Some((rule_position, y)) = table.notes_rule {
        if rule_position == position {
            out.rects.push(DecoRect::fill(
                table.margin,
                y,
                table.content_width,
                (1.0 * cfg.zoom).max(1.0),
                theme.blocks.rule,
            ));
        }
    }
    if entry.flags & ENTRY_SILENT != 0 {
        return 0..0;
    }
    let block = &doc.blocks[entry.block as usize];
    let (heading, _, base_size) =
        block_metrics(block, cfg, doc.plain_file).expect("a non-silent entry has metrics");
    let (x_base, avail) =
        block_geometry(block, table.margin, table.content_width, cfg, &doc.source);
    let rect_mark = out.rects.len();
    let mut scratch = LayoutDoc::default();
    if entry.flags & ENTRY_ALERT_TITLE != 0 {
        let kind = block.alert.expect("an alert title has a kind");
        shape_alert_title(
            fonts,
            theme,
            cfg,
            &doc.source,
            kind,
            entry.block as usize,
            x_base,
            avail,
            &mut scratch,
        );
        out.splice(&mut scratch, entry.y);
    }
    // The block's own lines start here, after the title of an alert it
    // opens: the alignment moves them and leaves the title.
    let lines_mark = Marks {
        rects: out.rects.len(),
        runs: out.runs.len(),
        images: out.images.len(),
        math: out.math_glyphs.len(),
    };
    let mut lines = 0..0;
    if entry.flags & ENTRY_CODE != 0 {
        if code_framed(doc) {
            for rect in code_panel_rects(theme, cfg, x_base, avail, entry.content_y, entry.height) {
                out.rects.push(rect);
            }
        }
        let total = code_line_count(doc, entry.block as usize);
        lines = table.lines_over(position, total, fill);
        replay_code_lines(
            doc,
            theme,
            fonts,
            cfg,
            table,
            position,
            lines.clone(),
            take,
            out,
        );
    } else {
        match take(StepKey::step(position, 0)) {
            Some(shaped) => {
                let mut ready = shaped.scratch;
                out.splice(&mut ready, entry.content_y);
            }
            None => {
                let summary_open = match &block.kind {
                    BlockKind::Summary { group, .. } => doc.details[*group as usize].open,
                    _ => false,
                };
                shape_kind(
                    fonts,
                    theme,
                    cfg,
                    &doc.source,
                    media,
                    block,
                    entry.block as usize,
                    heading,
                    base_size,
                    x_base,
                    avail,
                    summary_open,
                    &mut scratch,
                );
                out.splice(&mut scratch, entry.content_y);
            }
        }
    }
    if let Some(factor) = block.align_factor() {
        align_lines(
            out,
            lines_mark.runs,
            lines_mark.rects,
            lines_mark.images,
            lines_mark.math,
            x_base,
            avail,
            factor,
        );
    }
    if entry.deco_top.is_finite() {
        let decoration = quote_decoration(
            theme,
            cfg,
            block,
            table.margin,
            table.content_width,
            entry.deco_top,
            entry.bottom(),
            &doc.source,
        );
        out.rects.splice(rect_mark..rect_mark, decoration);
    }
    lines
}

/// Re-shapes a recorded code block's lines at their recorded tops.
#[allow(clippy::too_many_arguments)]
fn replay_code_lines(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    table: &BlockTable,
    position: usize,
    lines: Range<usize>,
    take: &impl Fn(StepKey) -> Option<crate::layout::pool::Shaped>,
    out: &mut LayoutDoc,
) {
    if lines.is_empty() {
        return;
    }
    let entry = &table.entries[position];
    let block = &doc.blocks[entry.block as usize];
    let BlockKind::CodeBlock {
        lines: source_lines,
        highlights,
        ..
    } = &block.kind
    else {
        return;
    };
    let (x_base, avail) =
        block_geometry(block, table.margin, table.content_width, cfg, &doc.source);
    let x0 = x_base + entry.pad;
    let wrap_width = (avail - 2.0 * entry.pad).max(40.0);
    let size = code_face(doc.plain_file, cfg).1 * cfg.zoom;
    let base = entry.content_y + entry.pad;
    let tall = table.tall_of(position);
    let (mut extra, mut next_tall) = table.extra_above(position, lines.start);
    let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
    let mut scratch = LayoutDoc::default();
    for line in lines {
        let top = code_line_y(base, line, entry.line_height, extra);
        let text = source_lines.line(&doc.source, line);
        if !text.is_empty() {
            match take(StepKey::step(position, line)) {
                Some(shaped) => {
                    let mut ready = shaped.scratch;
                    out.splice(&mut ready, top);
                }
                None => {
                    let segments = highlights.get(line).unwrap_or(&empty);
                    shape_code_line_step(
                        fonts,
                        theme,
                        cfg,
                        text,
                        segments,
                        entry.block as usize,
                        line,
                        x0,
                        size,
                        entry.line_height,
                        wrap_width,
                        doc.plain_file,
                        &mut scratch,
                    );
                    out.splice(&mut scratch, top);
                }
            }
        }
        if next_tall < tall.len() && tall[next_tall].1 as usize == line {
            extra += tall[next_tall].2 - entry.line_height;
            next_tall += 1;
        }
    }
}

/// The tail every block runs once its height is known: centering, the
/// quote decoration, and the advance of the carried state.
#[allow(clippy::too_many_arguments)]
fn finish_block(
    theme: &Theme,
    cfg: &ViewConfig,
    block: &Block,
    frame: Frame,
    height: f32,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
    source: &str,
) {
    if let Some(factor) = block.align_factor() {
        align_lines(
            out,
            frame.lines.runs,
            frame.lines.rects,
            frame.lines.images,
            frame.lines.math,
            frame.x_base,
            frame.avail,
            factor,
        );
    }

    // Quote decoration wraps the block, extending over the gap when the
    // previous block continues the same region (quote or alert), so
    // consecutive quoted blocks read as one. Inserted at the block's rect
    // mark to paint under the block's own rects (pills, strikes, panels).
    let mut deco_top = f32::NAN;
    if block.quote_depth > 0 {
        let continues = pass.prev_quote_depth > 0 && pass.prev_alert == block.alert;
        // The previous block's trailing space belongs to the region
        // too, plus one pixel of overlap into its panel: rasterization
        // rounds abutting edges independently and a shared fractional
        // edge can otherwise leave an uncovered row.
        let top = if continues {
            pass.cursor - frame.gap - pass.prev_space_below - 1.0
        } else {
            frame.region_top
        };
        deco_top = top;
        let decoration = quote_decoration(
            theme,
            cfg,
            block,
            pass.margin,
            pass.content_width,
            top,
            pass.cursor + height,
            source,
        );
        out.rects
            .splice(frame.marks.rects..frame.marks.rects, decoration);
    }
    out.table.finish(height, deco_top);

    let below = if pass.plain {
        0.0
    } else {
        metrics::space_below(frame.base_size)
    };
    pass.cursor += height + below;
    pass.prev_quote_depth = block.quote_depth;
    pass.prev_alert = block.alert;
    pass.prev_is_list = frame.is_list;
    pass.prev_space_below = below;
}

/// Shapes an alert region's bold title line into the scratch; the runs
/// carry the marker span, decoration outside selection and search.
/// Shared by the pass and the window replay.
#[allow(clippy::too_many_arguments)]
fn shape_alert_title(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    kind: AlertKind,
    block_index: usize,
    x_base: f32,
    avail: f32,
    scratch: &mut LayoutDoc,
) -> f32 {
    let title = [Span::plain(alert_title(kind))];
    let base = BlockStyle {
        size: cfg.body_size * cfg.zoom,
        color: alert_color(theme, kind),
        bold: true,
        block_index,
        justify: false,
        align: None,
    };
    let title_h = shape_block(
        fonts, theme, cfg, source, &title, false, &base, x_base, 0.0, avail, scratch,
    );
    // The title is a decoration with no model home; it takes no part in
    // selection or search.
    for run in &mut scratch.runs {
        run.span = usize::MAX;
    }
    title_h
}

/// The quote region's panel and bars wrapping a block from `top` to
/// `bottom`, in paint order: shared by the pass's tail and the window
/// replay so both produce the same rectangles.
#[allow(clippy::too_many_arguments)]
fn quote_decoration(
    theme: &Theme,
    cfg: &ViewConfig,
    block: &Block,
    margin: f32,
    content_width: f32,
    top: f32,
    bottom: f32,
    source: &str,
) -> Vec<DecoRect> {
    let panel_h = bottom - top;
    let rtl = block_rtl(block, source, cfg.direction);
    let mut decoration = vec![DecoRect::fill(
        margin,
        top,
        content_width,
        panel_h,
        theme.blocks.quote_bg,
    )];
    for level in 0..block.quote_depth {
        let bar = match block.alert {
            Some(kind) if level == 0 => alert_color(theme, kind),
            _ => theme.blocks.quote_bar,
        };
        let bar_w = 3.0 * cfg.zoom;
        let step = level as f32 * metrics::INDENT * cfg.zoom;
        let bar_x = if rtl {
            margin + content_width - bar_w - step
        } else {
            margin + step
        };
        decoration.push(DecoRect::fill(bar_x, top, bar_w, panel_h, bar));
    }
    decoration
}

/// Whether code blocks in this document draw the panel frame. A code
/// or text file is a page that is its lines, not a page containing
/// code: the lines draw on the page itself, no panel, no border, no
/// inner pad, the full width to wrap in. A markdown fence always keeps
/// the panel, even when the fence is the whole file.
pub fn code_framed(doc: &Document) -> bool {
    !(doc.code_file || doc.plain_file)
}

/// The face code lines draw in: a plain text file's lines are prose
/// and keep the body face and color; every other code line is code.
/// The face and the zoomed size a file of lines draws its text in, what
/// the line numbers size themselves from before a layout exists.
pub fn line_face<'a>(doc: &Document, cfg: &'a ViewConfig) -> (&'a str, f32) {
    let (family, size) = code_face(doc.plain_file, cfg);
    (family, size * cfg.zoom)
}

fn code_face(plain: bool, cfg: &ViewConfig) -> (&str, f32) {
    if plain {
        (&cfg.body_family, cfg.body_size)
    } else {
        (&cfg.code_family, cfg.code_size)
    }
}

/// Opens a code block: the panel and border take their final index with a
/// provisional height, so later lines only grow them. A frameless block
/// pushes no rects and pads nothing.
#[allow(clippy::too_many_arguments)]
fn open_code(
    theme: &Theme,
    cfg: &ViewConfig,
    block_index: usize,
    frame: Frame,
    counts: ElementCounts,
    framed: bool,
    plain: bool,
    out: &mut LayoutDoc,
    pass: &LayoutPass,
) -> OpenCode {
    let size = code_face(plain, cfg).1 * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let pad = if framed {
        metrics::CODE_PAD * cfg.zoom
    } else {
        0.0
    };
    // Long lines wrap inside the panel instead of overflowing it, so the
    // panel height follows the shaped lines.
    let wrap_width = (frame.avail - 2.0 * pad).max(40.0);
    let y0 = pass.cursor;
    let panel = out.rects.len();
    let height = 2.0 * pad;
    if framed {
        for rect in code_panel_rects(theme, cfg, frame.x_base, frame.avail, y0, height) {
            out.rects.push(rect);
        }
    }
    OpenCode {
        block: block_index,
        // place_block advanced past this block one line above.
        position: pass.position - 1,
        frame,
        panel,
        framed,
        y0,
        pad,
        size,
        line_height,
        wrap_width,
        line: 0,
        extra: 0.0,
        kept: None,
        counts,
    }
}

/// A code panel's background and border, shared by the pass's open and
/// the window replay.
fn code_panel_rects(
    theme: &Theme,
    cfg: &ViewConfig,
    x_base: f32,
    avail: f32,
    y0: f32,
    height: f32,
) -> [DecoRect; 2] {
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let blocks = &theme.blocks;
    [
        DecoRect::fill(x_base, y0, avail, height, blocks.code_bg).rounded(radius, radius),
        DecoRect::fill(x_base, y0, avail, height, blocks.code_border)
            .rounded(radius, radius)
            .stroked((1.0 * cfg.zoom).max(1.0)),
    ]
}

/// Places one line of the open code block and grows its panel, or closes
/// the block once its last line is placed.
fn place_code_line(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    mut open: OpenCode,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
) {
    let block = &doc.blocks[open.block];
    let BlockKind::CodeBlock {
        lines, highlights, ..
    } = &block.kind
    else {
        return;
    };
    if open.line >= lines.len() {
        let height = open.line_top() - open.y0 + open.pad;
        finish_block(
            theme,
            cfg,
            block,
            open.frame,
            height,
            out,
            pass,
            &doc.source,
        );
        let kept = open.kept.unwrap_or(0..0);
        settle_retention(out, pass, &open.counts, kept);
        return;
    }

    let top = open.line_top();
    let line = lines.line(&doc.source, open.line);
    let runs_mark = out.runs.len();
    let code_mark = out.code_lines.len();
    let mut advance = open.line_height;
    // A line of plain ASCII past the kept lines of a long block: its
    // height by the count, with no shaping.
    let counted = match (&pass.counter, pass.retain) {
        (Some(widths), Some((scroll, viewport_h)))
            if pass.counting == Some(open.position)
                && !line.is_empty()
                && widths.is_for(open.size)
                && super::rows::countable(line)
                && highlights.get(open.line).is_none_or(|s| regular_face(s)) =>
        {
            let height = widths.height(line, open.wrap_width, open.line_height);
            let range = retain_range(scroll, viewport_h);
            let inside = top <= range.end && top + height >= range.start;
            let contiguous = open.kept.as_ref().is_none_or(|kept| kept.end == open.line);
            (!(inside && contiguous)).then_some(height)
        }
        _ => None,
    };
    if let Some(height) = counted {
        #[cfg(debug_assertions)]
        {
            let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
            let segments = highlights.get(open.line).unwrap_or(&empty);
            let mut scratch = LayoutDoc::default();
            let shaped = shape_code_line_step(
                fonts,
                theme,
                cfg,
                line,
                segments,
                open.block,
                open.line,
                open.frame.x_base + open.pad,
                open.size,
                open.line_height,
                open.wrap_width,
                doc.plain_file,
                &mut scratch,
            );
            debug_assert_eq!(
                height.to_bits(),
                shaped.to_bits(),
                "line {} counted {height}, shaped {shaped}",
                open.line
            );
        }
        advance = height;
        if advance != open.line_height {
            out.table
                .push_tall(open.position, open.line as u32, advance);
        }
    } else if !line.is_empty() {
        let key = StepKey::step(open.position, open.line);
        let pooled = pass
            .pool
            .as_ref()
            .and_then(|pool| pool.take(pass.pool_generation, key));
        let (shaped, mut scratch) = match pooled {
            Some(shaped) => (shaped.height, shaped.scratch),
            None => {
                let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
                let segments = highlights.get(open.line).unwrap_or(&empty);
                let x0 = open.frame.x_base + open.pad;
                let mut scratch = std::mem::take(&mut pass.scratch);
                let advance = shape_code_line_step(
                    fonts,
                    theme,
                    cfg,
                    line,
                    segments,
                    open.block,
                    open.line,
                    x0,
                    open.size,
                    open.line_height,
                    open.wrap_width,
                    doc.plain_file,
                    &mut scratch,
                );
                (advance, scratch)
            }
        };
        out.splice(&mut scratch, top);
        pass.scratch = scratch;
        advance = shaped;
        if advance != open.line_height {
            out.table
                .push_tall(open.position, open.line as u32, advance);
        }
    }
    // A line outside the retention bound was measured and drops; one
    // that would leave a gap in the kept segment drops too, and the
    // slide materializes it later.
    if let Some((scroll, viewport_h)) = pass.retain {
        let range = retain_range(scroll, viewport_h);
        let inside = top <= range.end && top + advance >= range.start;
        let contiguous = open.kept.as_ref().is_none_or(|kept| kept.end == open.line);
        if inside && contiguous {
            match &mut open.kept {
                Some(kept) => kept.end = open.line + 1,
                None => open.kept = Some(open.line..open.line + 1),
            }
        } else {
            out.runs.truncate(runs_mark);
            out.code_lines.truncate(code_mark);
            if out.index.runs.indexed > runs_mark {
                out.index.runs.clear();
            }
            // A line of a long block was measured and dropped: from
            // here the block's lines in the code face are counted where
            // a line allows it.
            if pass.counting != Some(open.position)
                && !doc.plain_file
                && lines.len() - open.line >= COUNT_FROM
            {
                if !pass.counter.as_ref().is_some_and(|w| w.is_for(open.size)) {
                    let face = code_face(doc.plain_file, cfg).0;
                    pass.counter = super::rows::Widths::of(fonts, face, open.size);
                }
                if pass.counter.is_some() {
                    pass.counting = Some(open.position);
                }
            }
        }
    }
    if advance != open.line_height {
        open.extra += advance - open.line_height;
    }
    open.line += 1;

    if open.framed {
        let height = open.line_top() - open.y0 + open.pad;
        out.rects[open.panel].height = height;
        out.rects[open.panel + 1].height = height;
    }
    pass.open = Some(open);
}

struct BlockStyle {
    size: f32,
    color: Rgba,
    bold: bool,
    block_index: usize,
    /// Justify this block's segments; runs then split per word so paint
    /// can re-shape each at its stretched position.
    justify: bool,
    /// The alignment of the block's lines, a table cell's from its
    /// column; `None` leaves each line its natural one.
    align: Option<Align>,
}

/// A column's mark as the alignment of its cells' lines; a column
/// without a mark leaves them to their direction.
fn cell_align(column: ColumnAlign) -> Option<Align> {
    match column {
        ColumnAlign::Auto => None,
        ColumnAlign::Left => Some(Align::Left),
        ColumnAlign::Center => Some(Align::Center),
        ColumnAlign::Right => Some(Align::Right),
    }
}

fn heading_color(theme: &Theme, level: u8) -> Rgba {
    match level {
        1 => theme.headings.h1,
        2 => theme.headings.h2,
        3 => theme.headings.h3,
        4 => theme.headings.h4,
        5 => theme.headings.h5,
        _ => theme.headings.h6,
    }
}

/// Resolved visual properties of one model span.
struct SpanStyle {
    family: String,
    size: f32,
    weight: Weight,
    italic: bool,
    strike: bool,
    underline: bool,
    /// Dotted underline; an abbreviation with an expansion to show.
    abbr: bool,
    color: Rgba,
    /// Background pill color for inline code and mark highlights.
    pill: Option<Rgba>,
    /// Baseline shift: positive raises (superscripts), negative lowers.
    rise: f32,
}

fn span_style(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    base: &BlockStyle,
    span: &Span,
) -> SpanStyle {
    let code = span.code;
    let footnote = span
        .link
        .as_deref()
        .is_some_and(|l| l.starts_with("footnote:"));
    let color = if span.link.is_some() {
        theme.text.link
    } else if span.math {
        theme.text.math
    } else if code {
        theme.text.inline_code
    } else if span.strike {
        theme.text.strike
    } else if base.bold {
        base.color
    } else if span.bold {
        theme.text.bold
    } else if span.italic {
        theme.text.italic
    } else {
        base.color
    };
    let mut size = if code || span.math {
        cfg.code_size * cfg.zoom * (base.size / (cfg.body_size * cfg.zoom))
    } else {
        base.size
    };
    let mut rise = 0.0;
    if footnote {
        size *= 0.7;
        rise = 0.3 * base.size;
    }
    match span.script {
        SpanScript::Sup => {
            size *= 0.7;
            rise = 0.3 * base.size;
        }
        SpanScript::Sub => {
            size *= 0.7;
            rise = -0.12 * base.size;
        }
        SpanScript::Small => size *= 0.85,
        SpanScript::None => {}
    }
    let family = if code || span.math {
        cfg.code_family.clone()
    } else {
        cfg.body_family.clone()
    };
    let weight = if base.bold || span.bold {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    SpanStyle {
        weight: fonts.weight_for(&family, weight),
        family,
        size,
        italic: span.italic || span.math,
        strike: span.strike,
        underline: span.underline,
        abbr: span.abbr.is_some(),
        color,
        // A mark highlight outranks the code pill when both apply.
        pill: if span.mark {
            Some(theme.ui.search_match_bg)
        } else {
            code.then_some(theme.text.inline_code_bg)
        },
        rise,
    }
}

/// Shapes one block's spans into positioned runs. Hard-break spans ("\n")
/// split the block into segments whose lines sit flush. Returns the height.
#[allow(clippy::too_many_arguments)]
fn shape_block(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    spans: &[Span],
    model_spans: bool,
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    content_width: f32,
    out: &mut LayoutDoc,
) -> f32 {
    shape_block_lines(
        fonts,
        theme,
        cfg,
        source,
        spans,
        model_spans,
        base,
        x0,
        y0,
        content_width,
        false,
        out,
    )
}

/// As `shape_block`; with `first_line_only`, the first visual line is
/// shaped and emitted and the rest of the block is left, for a chunk
/// whose first line joins an open line of the flow.
#[allow(clippy::too_many_arguments)]
fn shape_block_lines(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    spans: &[Span],
    model_spans: bool,
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    content_width: f32,
    first_line_only: bool,
    out: &mut LayoutDoc,
) -> f32 {
    let line_height = metrics::LINE_HEIGHT * base.size;
    // Spans expand into pieces at this point only. Math literals expand
    // into sup/sub script segments with synthesized text (the model
    // keeps the raw TeX, so the pieces are non-model and land in the
    // side buffer). Arabic and Hebrew stretches split into pieces
    // routed to their designated faces; those pieces keep the span's
    // exact text, so they stay model spans and selection maps through
    // the origin. Code spans never route: code is always LTR in the
    // code face.
    let mut shaped: Vec<Span> = Vec::new();
    let mut origins: Vec<usize> = Vec::new();
    let mut styles: Vec<SpanStyle> = Vec::new();
    let mut model: Vec<bool> = Vec::new();
    for (si, span) in spans.iter().enumerate() {
        if span.math && span.text(source) != "\n" {
            for (text, script) in math_scripts(span.text(source)) {
                let mut piece = span.clone();
                piece.set_text(text);
                let mut style = span_style(fonts, theme, cfg, base, &piece);
                if script != Script::Normal {
                    style.rise = match script {
                        Script::Sup => 0.3 * style.size,
                        _ => -0.12 * style.size,
                    };
                    style.size *= 0.7;
                }
                shaped.push(piece);
                origins.push(si);
                styles.push(style);
                model.push(false);
            }
        } else {
            let text = span.text(source);
            let segments = if span.code {
                vec![(0..text.len(), None)]
            } else {
                crate::style::fonts::script_segments(text)
            };
            if let [(_, family)] = segments.as_slice() {
                let mut style = span_style(fonts, theme, cfg, base, span);
                if let Some(face) = family {
                    style.family = face.to_string();
                    style.italic = false;
                }
                styles.push(style);
                shaped.push(span.clone());
                origins.push(si);
                model.push(model_spans);
            } else {
                for (range, family) in segments {
                    let mut piece = span.clone();
                    piece.set_text(text[range].to_string());
                    let mut style = span_style(fonts, theme, cfg, base, &piece);
                    if let Some(face) = family {
                        style.family = face.to_string();
                        style.italic = false;
                    }
                    shaped.push(piece);
                    origins.push(si);
                    styles.push(style);
                    model.push(model_spans);
                }
            }
        }
    }

    let mut height = 0.0_f32;
    let mut segment: Vec<usize> = Vec::new();
    let mut i = 0;
    while i <= shaped.len() {
        let is_break = i == shaped.len() || shaped[i].text(source) == "\n";
        if is_break {
            if !segment.is_empty() {
                height += shape_segment(
                    fonts,
                    cfg,
                    source,
                    &shaped,
                    &model,
                    &styles,
                    &origins,
                    &segment,
                    base,
                    x0,
                    y0 + height,
                    content_width,
                    line_height,
                    first_line_only,
                    out,
                );
                segment.clear();
                if first_line_only && height > 0.0 {
                    break;
                }
            } else if i < shaped.len() {
                height += line_height;
            }
        } else if !shaped[i].text(source).is_empty() {
            // A piece with no text (a span the flow masked out) joins no
            // segment: alone after a break it would make an empty line.
            segment.push(i);
        }
        i += 1;
    }
    height
}

/// A prose segment with more pieces than this, or more bytes, shapes
/// in chunks cut at piece boundaries, each chunk its own buffer
/// stacked under the previous one. cosmic-text's span list grows
/// quadratic in the piece count: a paragraph of 32,000 inline spans
/// took 6.5s. Bytes alone shape in linear time, so the byte cap sits
/// far above the code line's, since a cut inside a book paragraph
/// shows as a ragged line.
const LONG_SEGMENT_PIECES: usize = 1000;
const LONG_SEGMENT_BYTES: usize = 8 * LONG_LINE;

/// Shapes one hard-break segment of a block: the pieces `segment`
/// indexes, as one buffer or as stacked chunks. Returns the height.
#[allow(clippy::too_many_arguments)]
fn shape_segment(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    source: &str,
    spans: &[Span],
    model: &[bool],
    styles: &[SpanStyle],
    origins: &[usize],
    segment: &[usize],
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    content_width: f32,
    line_height: f32,
    first_line_only: bool,
    out: &mut LayoutDoc,
) -> f32 {
    // Almost every segment is small and shapes whole; it pays one
    // length pass here and builds its own piece table inside the chunk
    // shaper, nothing more.
    let total: usize = segment.iter().map(|&si| spans[si].text(source).len()).sum();
    if segment.len() <= LONG_SEGMENT_PIECES && total <= LONG_SEGMENT_BYTES {
        return shape_segment_chunk(
            fonts,
            cfg,
            source,
            spans,
            model,
            styles,
            origins,
            segment,
            0,
            None,
            base,
            x0,
            y0,
            content_width,
            line_height,
            first_line_only,
            out,
        );
    }
    // Byte offset of each piece inside the whole segment, and of the
    // first piece of each origin span: the pieces of a split span
    // concatenate to the span's text, so a model reference counts from
    // where the origin starts, not where the piece does. Only a
    // segment big enough to shape in chunks needs the tables over the
    // whole of it.
    let mut offsets: Vec<u32> = Vec::with_capacity(segment.len());
    let mut origin_starts: HashMap<usize, u32> = HashMap::new();
    let mut acc = 0u32;
    for &si in segment {
        offsets.push(acc);
        origin_starts.entry(origins[si]).or_insert(acc);
        acc += spans[si].text(source).len() as u32;
    }
    let mut height = 0.0_f32;
    let mut start = 0usize;
    while start < segment.len() {
        let mut end = start + 1;
        while end < segment.len()
            && end - start < LONG_SEGMENT_PIECES
            && (offsets[end] - offsets[start]) as usize + spans[segment[end]].text(source).len()
                <= LONG_SEGMENT_BYTES
        {
            end += 1;
        }
        height += shape_segment_chunk(
            fonts,
            cfg,
            source,
            spans,
            model,
            styles,
            origins,
            &segment[start..end],
            offsets[start],
            Some(&origin_starts),
            base,
            x0,
            y0 + height,
            content_width,
            line_height,
            first_line_only,
            out,
        );
        start = end;
        if first_line_only && height > 0.0 {
            break;
        }
    }
    height
}

/// Shapes the pieces `chunk` indexes as one buffer. `chunk_base` is
/// the chunk's byte offset inside its segment and `origin_starts` the
/// segment offset of each origin span's first piece, both over the
/// whole segment, so a model reference lands on the span's own bytes
/// whichever chunk shaped it. An unchunked segment passes None and the
/// piece table is built here, over the whole segment it was handed.
#[allow(clippy::too_many_arguments)]
fn shape_segment_chunk(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    source: &str,
    spans: &[Span],
    model: &[bool],
    styles: &[SpanStyle],
    origins: &[usize],
    segment: &[usize],
    chunk_base: u32,
    origin_starts: Option<&HashMap<usize, u32>>,
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    content_width: f32,
    line_height: f32,
    first_line_only: bool,
    out: &mut LayoutDoc,
) -> f32 {
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(base.size, line_height));
    buffer.set_size(&mut fonts.font_system, Some(content_width), None);
    let texts: Vec<std::borrow::Cow<str>> = segment
        .iter()
        .map(|&si| crate::style::fonts::shapable(spans[si].text(source)))
        .collect();
    let rich: Vec<(&str, Attrs)> = segment
        .iter()
        .zip(&texts)
        .map(|(&si, text)| {
            let st = &styles[si];
            let mut attrs = Attrs::new()
                .family(Family::Name(&st.family))
                .weight(st.weight)
                .metadata(si);
            if st.italic {
                attrs = attrs.style(Style::Italic);
            }
            if (st.size - base.size).abs() > f32::EPSILON {
                attrs = attrs.metrics(Metrics::new(st.size, line_height));
            }
            (&**text, attrs)
        })
        .collect();
    let default_attrs = Attrs::new().family(Family::Name(&cfg.body_family));
    buffer.set_rich_text(
        &mut fonts.font_system,
        rich,
        &default_attrs,
        Shaping::Advanced,
        None,
    );
    // Book typography: spaces stretch to the content width, the last
    // visual line of each paragraph stays natural.
    if base.justify {
        for line in buffer.lines.iter_mut() {
            line.set_align(Some(Align::Justified));
        }
    } else if let Some(align) = base.align {
        for line in buffer.lines.iter_mut() {
            line.set_align(Some(align));
        }
    } else if cfg.direction != DirectionMode::Auto {
        // A forced direction aligns every line to the forced side; a
        // line already reading that way keeps its natural alignment.
        let forced_rtl = cfg.direction == DirectionMode::Rtl;
        for line in buffer.lines.iter_mut() {
            let natural = line
                .text()
                .chars()
                .find_map(crate::style::fonts::strong_rtl)
                .unwrap_or(false);
            if natural != forced_rtl {
                line.set_align(Some(if forced_rtl {
                    Align::Right
                } else {
                    Align::Left
                }));
            }
        }
    }
    buffer.shape_until_scroll(&mut fonts.font_system, false);

    // A span text may carry embedded newlines; the buffer splits them
    // into separate lines whose glyph offsets are line-local. Each
    // line's base offset inside the chunk counts the stripped newline
    // back in, one byte, so a model reference lands on the span's own
    // bytes. The buffer splits at every paragraph separator, and
    // `shapable` turns the others (the carriage return, U+0085, U+2029)
    // into blanks, so the newline is the only one it drops here.
    // The unchunked path's piece table: each piece with its offset in
    // the segment, the first of an origin span being where a model
    // reference counts from. A handful of pieces, so a scan answers.
    let local_starts: Vec<(usize, u32)> = if origin_starts.is_none() {
        let mut acc = chunk_base;
        segment
            .iter()
            .map(|&si| {
                let at = (si, acc);
                acc += spans[si].text(source).len() as u32;
                at
            })
            .collect()
    } else {
        Vec::new()
    };
    let origin_start_of = |origin: usize| -> u32 {
        match origin_starts {
            Some(map) => map.get(&origin).copied().unwrap_or(0),
            None => local_starts
                .iter()
                .find(|(si, _)| origins[*si] == origin)
                .map_or(0, |(_, at)| *at),
        }
    };
    let mut line_offsets: Vec<u32> = Vec::with_capacity(buffer.lines.len());
    let mut line_acc = chunk_base;
    for line in &buffer.lines {
        line_offsets.push(line_acc);
        line_acc += line.text().len() as u32 + 1;
    }

    // Inline code is drawn as written, left to right: its group keeps
    // the gaps the bidi rules open in its bytes, while the span is one
    // group. Justification cuts a span at its spaces, and a kept gap
    // in one of its words would span the bytes of the others.
    let as_written: Vec<bool> = spans
        .iter()
        .map(|s| s.code && (!base.justify || !s.text(source).contains([' ', '\u{a0}'])))
        .collect();
    let mut height = 0.0_f32;
    for (visual_line, run) in buffer.layout_runs().enumerate() {
        if first_line_only && visual_line > 0 {
            break;
        }
        height = height.max(run.line_top + line_height);
        let line_text = buffer.lines[run.line_i].text();
        let line_base = line_offsets[run.line_i];
        let glyphs = trim_trailing_spaces(run.glyphs, line_text);
        // The pill open under the previous group of this line, so the
        // words of one phrase share a pill instead of overlapping at
        // their spaces, which doubles a translucent color.
        let mut open_pill: Option<(usize, Rgba)> = None;
        for group in glyph_groups(glyphs, line_text, base.justify, &as_written) {
            let span_index = group.meta;
            let (start_byte, end_byte) = (group.bytes.start, group.bytes.end);
            let st = &styles[span_index];
            let x = x0 + group.x;
            let width = group.width;
            let y = y0 + run.line_top - st.rise;
            let baseline = y0 + run.line_y - st.rise;
            match (st.pill, open_pill) {
                (Some(pill), Some((at, color))) if color == pill => {
                    // The groups of a right-to-left line come from the
                    // right, and the bidi rules can put a group left of
                    // the first one: the pill grows on both sides.
                    let rect = &mut out.rects[at];
                    let left = rect.x.min(x - 3.0);
                    let right = (rect.x + rect.width).max(x + width + 3.0);
                    rect.x = left;
                    rect.width = right - left;
                }
                (Some(pill), _) => {
                    let radius = metrics::PILL_RADIUS * cfg.zoom;
                    out.rects.push(
                        DecoRect::fill(
                            x - 3.0,
                            y + 0.1 * line_height,
                            width + 6.0,
                            0.8 * line_height,
                            pill,
                        )
                        .rounded(radius, radius),
                    );
                    open_pill = Some((out.rects.len() - 1, pill));
                }
                (None, _) => open_pill = None,
            }
            if let Some(label) = spans[span_index]
                .link
                .as_deref()
                .and_then(|l| l.strip_prefix("footnote:"))
            {
                // The way back from the foot: the first reference wins
                // the lookup, later ones only add entries.
                out.anchors.push((format!("fnref:{label}"), y));
            }
            let text = if model[span_index] {
                let origin_start = origin_start_of(origins[span_index]);
                TextRef::Model {
                    start: line_base + start_byte as u32 - origin_start,
                    len: (end_byte - start_byte) as u32,
                }
            } else {
                out.side_ref(&line_text[start_byte..end_byte])
            };
            let family = out.family_id(&st.family);
            out.runs.push(TextRun {
                text,
                x,
                y,
                baseline,
                width,
                size: st.size,
                family,
                weight: st.weight.0,
                italic: st.italic,
                color: st.color,
                block: base.block_index,
                span: origins[span_index],
            });
            if st.strike {
                out.rects.push(DecoRect::fill(
                    x,
                    baseline - 0.3 * st.size,
                    width,
                    (0.06 * st.size).max(1.0),
                    st.color,
                ));
            }
            if st.underline {
                out.rects.push(DecoRect::fill(
                    x,
                    baseline + 0.1 * st.size,
                    width,
                    (0.06 * st.size).max(1.0),
                    st.color,
                ));
            }
            if st.abbr {
                // Square dots a dot apart, the run's own color.
                let dot = (0.08 * st.size).max(1.0);
                let mut dx = x;
                while dx + dot <= x + width {
                    out.rects.push(DecoRect::fill(
                        dx,
                        baseline + 0.1 * st.size,
                        dot,
                        dot,
                        st.color,
                    ));
                    dx += 2.0 * dot;
                }
            }
        }
    }
    height
}

/// Lays out a summary row: the fold chevron in the gutter, text
/// indented one step, the list item pattern at depth zero. The chevron
/// points right closed and down open.
#[allow(clippy::too_many_arguments)]
fn layout_summary(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    spans: &[Span],
    open: bool,
    block_index: usize,
    x0: f32,
    avail: f32,
    scratch: &mut LayoutDoc,
) -> f32 {
    let size = cfg.body_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let indent = metrics::INDENT * cfg.zoom;
    let text_x = x0 + indent;
    let text_w = (avail - indent).max(40.0);
    let gutter = 10.0 * cfg.zoom;
    let glyph = if open { "\u{25BE}" } else { "\u{25B8}" };
    let (runs, width) = shape_marker(fonts, cfg, glyph, size, theme.text.body, scratch);
    place_marker(runs, text_x - width - gutter, 0.0, block_index, scratch);
    let base = BlockStyle {
        size,
        color: theme.text.body,
        bold: false,
        block_index,
        justify: false,
        align: None,
    };
    let h = flow_or_shape(
        fonts, theme, cfg, source, media, spans, &base, text_x, 0.0, text_w, scratch,
    );
    h.max(line_height)
}

/// Lays out one list item: marker (bullet, number, or checkbox) in the
/// gutter, item text indented one step per nesting depth.
#[allow(clippy::too_many_arguments)]
fn layout_list_item(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    marker: &Marker,
    depth: u8,
    spans: &[Span],
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let size = cfg.body_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let indent = metrics::INDENT * cfg.zoom * (depth as f32 + 1.0);
    // An RTL item mirrors: the text column keeps the left edge and
    // steps its right edge in, and the marker sits in the right gutter.
    let rtl = match cfg.direction {
        DirectionMode::Rtl => true,
        DirectionMode::Ltr => false,
        DirectionMode::Auto => spans
            .iter()
            .flat_map(|span| span.text(source).chars())
            .find_map(crate::style::fonts::strong_rtl)
            .unwrap_or(false),
    };
    let text_x = if rtl { x0 } else { x0 + indent };
    let text_w = (avail - indent).max(40.0);
    let gutter = 10.0 * cfg.zoom;

    match marker {
        Marker::Bullet => {
            let (runs, width) = shape_marker(fonts, cfg, "\u{2022}", size, theme.text.body, out);
            let x = if rtl {
                text_x + text_w + gutter
            } else {
                text_x - width - gutter
            };
            place_marker(runs, x, y0, block_index, out);
        }
        Marker::Number(n) => {
            let text = format!("{n}.");
            let (runs, width) = shape_marker(fonts, cfg, &text, size, theme.text.body, out);
            let x = if rtl {
                text_x + text_w + gutter
            } else {
                text_x - width - gutter
            };
            place_marker(runs, x, y0, block_index, out);
        }
        Marker::None => {}
        Marker::Task { checked, .. } => {
            // Above the web's native 0.81, which reads small at 1x; the
            // mark at 0.8 fills the box instead of floating in it.
            let side = 0.9 * size;
            let bx = if rtl {
                text_x + text_w + gutter
            } else {
                text_x - side - gutter
            };
            let by = y0 + (line_height - side) / 2.0;
            // The square is the click's hit target, recorded with the
            // same values it is drawn from.
            out.push_checkbox(TaskBox {
                x: bx,
                y: by,
                side,
                block: block_index as u32,
            });
            if *checked {
                let radius = 3.0 * cfg.zoom;
                out.rects.push(
                    DecoRect::fill(bx, by, side, side, theme.text.link).rounded(radius, radius),
                );
                let (runs, width) = shape_marker(
                    fonts,
                    cfg,
                    "\u{2713}",
                    0.8 * size,
                    theme.surface.background,
                    out,
                );
                // A logical pixel down from the text's own line: the glyph
                // sits high in its em box and reads off-center otherwise.
                let lift = cfg.zoom;
                place_marker(runs, bx + (side - width) / 2.0, y0 + lift, block_index, out);
            } else {
                let t = (1.0 * cfg.zoom).max(1.0);
                let radius = 3.0 * cfg.zoom;
                out.rects.push(
                    DecoRect::fill(bx, by, side, side, theme.blocks.rule)
                        .rounded(radius, radius)
                        .stroked(t),
                );
            }
        }
    }

    let base = BlockStyle {
        size,
        color: theme.text.body,
        bold: false,
        block_index,
        justify: cfg.justify,
        align: None,
    };
    let height = flow_or_shape(
        fonts, theme, cfg, source, media, spans, &base, text_x, y0, text_w, out,
    );
    height.max(line_height)
}

/// Lays out a table: columns sized to their widest cell, capped at
/// 1.5 shares of the available width, header on its own background in
/// bold, alternating body row shading, and a 1px border grid.
#[allow(clippy::too_many_arguments)]
fn layout_table(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    aligns: &[ColumnAlign],
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    // Cells keep their own natural direction whatever the file forces:
    // tables hold their column order, and forcing a cell's alignment
    // would tear the grid.
    let cell_cfg = ViewConfig {
        direction: DirectionMode::Auto,
        ..cfg.clone()
    };
    let cfg = &cell_cfg;
    let size = cfg.body_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let pad = 8.0 * cfg.zoom;
    let vpad = 6.0 * cfg.zoom;
    let ncols = header
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if ncols == 0 {
        return 0.0;
    }

    // A cell with a formula goes through the paragraph's flow, which
    // typesets the formula and sets it on the text's line or on a row of
    // its own; a cell without one keeps the plain shaper.
    let has_math = |spans: &[Span]| spans.iter().any(|s| s.math);
    let measure =
        |fonts: &mut FontStore, media: &mut MediaCache, spans: &[Span], bold: bool| -> f32 {
            let mut tmp = LayoutDoc::default();
            let base = BlockStyle {
                size,
                color: theme.text.body,
                bold,
                block_index,
                justify: false,
                align: None,
            };
            if has_math(spans) {
                let spans = cell_without_images(spans);
                layout_flow(
                    fonts, theme, cfg, source, media, &spans, &base, 0.0, 0.0, 100_000.0, &mut tmp,
                );
            } else {
                shape_block(
                    fonts, theme, cfg, source, spans, true, &base, 0.0, 0.0, 100_000.0, &mut tmp,
                );
            }
            cell_right_edge(&tmp)
        };

    let empty_cell: Vec<Span> = Vec::new();
    let mut widths = vec![0.0f32; ncols];
    for (c, w) in widths.iter_mut().enumerate() {
        let mut natural = measure(fonts, media, header.get(c).unwrap_or(&empty_cell), true);
        for row in rows {
            natural = natural.max(measure(
                fonts,
                media,
                row.get(c).unwrap_or(&empty_cell),
                false,
            ));
        }
        // One pixel of slack: the measuring pass shapes the cell without a
        // wrap width and the layout pass shapes it inside the column, and
        // font fallback can resolve differently between the two. Granting
        // exactly the measured width makes a cell wrap on that difference
        // alone, in a table with room to spare.
        *w = natural.ceil() + 2.0 * pad + 1.0;
    }
    let naturals = widths.clone();
    let cap = avail / ncols as f32 * 1.5;
    for w in widths.iter_mut() {
        *w = w.min(cap).max(3.0 * pad);
    }
    let total: f32 = widths.iter().sum();
    if total > avail {
        let scale = avail / total;
        for w in widths.iter_mut() {
            *w *= scale;
        }
    } else {
        // Columns clipped by the cap grow back into the unused width,
        // proportionally to their deficit. Compact tables stay compact;
        // a dominant text column uses the room the table actually has.
        let deficits: Vec<f32> = naturals
            .iter()
            .zip(&widths)
            .map(|(n, w)| (n - w).max(0.0))
            .collect();
        let deficit_sum: f32 = deficits.iter().sum();
        let leftover = avail - total;
        if deficit_sum > 0.0 && leftover > 0.0 {
            let grow = leftover.min(deficit_sum);
            for (w, d) in widths.iter_mut().zip(&deficits) {
                *w += grow * (d / deficit_sum);
            }
        }
    }
    let table_w: f32 = widths.iter().sum();

    let mut col_x = vec![x0];
    for w in &widths {
        col_x.push(col_x.last().unwrap() + w);
    }

    let mut y = y0;
    // An HTML table without a header row is headerless: no band, body
    // rows from the top.
    let header_rows = usize::from(!header.is_empty());
    let all_rows: Vec<(&[Vec<Span>], bool)> = std::iter::once((header, true))
        .take(header_rows)
        .chain(rows.iter().map(|r| (r.as_slice(), false)))
        .collect();
    let mut boundaries = vec![y0];
    // Runs address table spans through the flattened header-then-rows
    // cell chain, which this loop visits in exactly that order.
    let mut span_base = 0usize;
    for (row_index, (cells, is_header)) in all_rows.iter().enumerate() {
        let mut shaped: Vec<LayoutDoc> = Vec::new();
        let mut row_h = line_height;
        for (c, col_width) in widths.iter().enumerate() {
            let spans = cells.get(c).unwrap_or(&empty_cell);
            let mut tmp = LayoutDoc::default();
            let align = aligns.get(c).copied().and_then(cell_align);
            let math = has_math(spans);
            let rtl = math && cell_reads_rtl(spans, source);
            // The flow sets a formula after its text: a centered or
            // right-aligned cell with a formula is laid out at the left
            // and moved as a block afterwards, so the formula moves with
            // its text. A right-to-left cell keeps its mark instead: its
            // text sits at the right by nature, and the flow puts the
            // formula's row at the marked side, the right when unmarked.
            let base = BlockStyle {
                size,
                color: theme.text.body,
                bold: *is_header,
                block_index,
                justify: false,
                align: match align {
                    Some(Align::Center) | Some(Align::Right) if math && !rtl => None,
                    None if rtl => Some(Align::Right),
                    a => a,
                },
            };
            let content_w = (col_width - 2.0 * pad).max(20.0);
            let h = if math {
                let spans = cell_without_images(spans);
                let h = layout_flow(
                    fonts, theme, cfg, source, media, &spans, &base, 0.0, 0.0, content_w, &mut tmp,
                );
                let factor = if rtl { 0.0 } else { align_factor(align) };
                // The room is the column's own, not the content floor's,
                // which a very narrow column falls under: the cell never
                // moves past its column.
                let room = (col_width - 2.0 * pad) - cell_right_edge(&tmp);
                if factor > 0.0 && room > 0.0 {
                    shift_cell_right(&mut tmp, room * factor);
                }
                h
            } else {
                shape_block(
                    fonts, theme, cfg, source, spans, true, &base, 0.0, 0.0, content_w, &mut tmp,
                )
            };
            for run in &mut tmp.runs {
                if run.span != usize::MAX {
                    run.span += span_base;
                }
            }
            span_base += spans.len();
            row_h = row_h.max(h);
            shaped.push(tmp);
        }
        let full_h = row_h + 2.0 * vpad;
        out.table_rows.push(TableRow {
            block: block_index,
            top: y,
            bottom: y + full_h,
        });
        let stripe = if *is_header {
            Some(theme.blocks.table_header_bg)
        } else if (row_index + 1 - header_rows) % 2 == 0 {
            // Even-numbered body rows stripe, counted from the first body
            // row, wherever the header row leaves it.
            Some(theme.blocks.table_row_alt_bg)
        } else {
            None
        };
        if let Some(color) = stripe {
            // Stripes at the table's corners round to match the outline.
            let radius = metrics::CORNER_RADIUS * cfg.zoom;
            let top_r = if row_index == 0 { radius } else { 0.0 };
            let bottom_r = if row_index + 1 == all_rows.len() {
                radius
            } else {
                0.0
            };
            out.rects
                .push(DecoRect::fill(x0, y, table_w, full_h, color).rounded(top_r, bottom_r));
        }
        for (c, mut tmp) in shaped.into_iter().enumerate() {
            let dx = col_x[c] + pad;
            let dy = y + vpad;
            let (side_base, family_map) = out.merge_refs(&mut tmp);
            for mut run in tmp.runs {
                run.x += dx;
                run.y += dy;
                run.baseline += dy;
                if let TextRef::Side { start, .. } = &mut run.text {
                    *start += side_base;
                }
                run.family = family_map[run.family as usize];
                out.runs.push(run);
            }
            for mut rect in tmp.rects {
                rect.x += dx;
                rect.y += dy;
                out.rects.push(rect);
            }
            for mut g in tmp.math_glyphs {
                g.x += dx;
                g.left += dx;
                g.right += dx;
                g.y += dy;
                g.top += dy;
                g.bottom += dy;
                out.math_glyphs.push(g);
            }
            for mut image in tmp.images {
                image.x += dx;
                image.y += dy;
                out.images.push(image);
            }
            for (name, ay) in tmp.anchors {
                out.anchors.push((name, ay + dy));
            }
        }
        y += full_h;
        boundaries.push(y);
    }

    let border = theme.blocks.table_border;
    let t = (1.0 * cfg.zoom).max(1.0);
    for by in &boundaries[1..boundaries.len().saturating_sub(1)] {
        out.rects.push(DecoRect::fill(x0, *by, table_w, t, border));
    }
    for bx in &col_x[1..col_x.len().saturating_sub(1)] {
        out.rects.push(DecoRect::fill(*bx, y0, t, y - y0, border));
    }
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    out.rects.push(
        DecoRect::fill(x0, y0, table_w, y - y0, border)
            .rounded(radius, radius)
            .stroked(t),
    );
    y - y0
}

/// A cell's spans for the paragraph's flow: an image span becomes its
/// text, as every cell shows an image, and the pool's worker, which
/// shapes tables, has no media cache to look one up in.
fn cell_without_images(spans: &[Span]) -> std::borrow::Cow<'_, [Span]> {
    if spans.iter().all(|s| s.image.is_none()) {
        return std::borrow::Cow::Borrowed(spans);
    }
    std::borrow::Cow::Owned(
        spans
            .iter()
            .map(|s| {
                let mut c = s.clone();
                c.image = None;
                c
            })
            .collect(),
    )
}

/// Whether a cell's text reads right to left: its first strong character,
/// the formulas' TeX left out.
fn cell_reads_rtl(spans: &[Span], source: &str) -> bool {
    spans
        .iter()
        .filter(|s| !s.math)
        .flat_map(|s| s.text(source).chars())
        .find_map(crate::style::fonts::strong_rtl)
        .unwrap_or(false)
}

/// The right edge of a cell laid out alone: its runs, its formulas' boxes
/// and its images.
fn cell_right_edge(cell: &LayoutDoc) -> f32 {
    let runs = cell.runs.iter().map(|r| r.x + r.width);
    let math = cell.math_glyphs.iter().map(|g| g.right);
    let images = cell.images.iter().map(|i| i.x + i.width);
    runs.chain(math).chain(images).fold(0.0, f32::max)
}

/// Moves a cell laid out alone to the right, as a block.
fn shift_cell_right(cell: &mut LayoutDoc, dx: f32) {
    for run in &mut cell.runs {
        run.x += dx;
    }
    for rect in &mut cell.rects {
        rect.x += dx;
    }
    for g in &mut cell.math_glyphs {
        g.x += dx;
        g.left += dx;
        g.right += dx;
    }
    for image in &mut cell.images {
        image.x += dx;
    }
}

/// How much of its natural size an image is drawn at. Pixels are all an
/// image carries, so they are read as a size for the reference body; a
/// document set smaller scales its images down with its text rather than
/// letting them take a share of the page that the text no longer has.
fn image_scale(cfg: &ViewConfig) -> f32 {
    cfg.body_size * cfg.zoom / metrics::REFERENCE_BODY
}

/// Places an image scaled down to fit the available width (never scaled
/// up); an unloadable image becomes a bordered placeholder with alt text.
#[allow(clippy::too_many_arguments)]
fn layout_image(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    path: &str,
    alt: &str,
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    if let Some((iw, ih)) = media.dimensions(path) {
        let width = (iw as f32 * image_scale(cfg)).min(avail);
        let height = ih as f32 * width / iw as f32;
        out.images.push(ImagePlace {
            src: path.to_string(),
            x: x0,
            y: y0,
            width,
            height,
            link: None,
        });
        return height;
    }
    let pad = metrics::PLACEHOLDER_PAD * cfg.zoom;
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let alt_span = {
        let mut span = Span::plain(if alt.is_empty() { path } else { alt });
        span.italic = true;
        [span]
    };
    let base = BlockStyle {
        size: cfg.body_size * cfg.zoom,
        color: theme.blocks.frontmatter_fg,
        bold: false,
        block_index,
        justify: false,
        align: None,
    };
    let rects_mark = out.rects.len();
    let text_h = shape_block(
        fonts,
        theme,
        cfg,
        source,
        &alt_span,
        false,
        &base,
        x0 + pad,
        y0 + pad,
        avail - 2.0 * pad,
        out,
    );
    let box_h = text_h + 2.0 * pad;
    out.rects.splice(
        rects_mark..rects_mark,
        [
            DecoRect::fill(x0, y0, avail, box_h, theme.blocks.frontmatter_bg)
                .rounded(radius, radius),
            DecoRect::fill(x0, y0, avail, box_h, theme.blocks.code_border)
                .rounded(radius, radius)
                .stroked((1.0 * cfg.zoom).max(1.0)),
        ],
    );
    box_h
}

/// Vertical role of one piece of a math literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Normal,
    Sup,
    Sub,
}

/// TeX command to Unicode symbol, sorted by name for binary search.
static TEX_SYMBOLS: &[(&str, &str)] = &[
    ("Delta", "\u{0394}"),
    ("Gamma", "\u{0393}"),
    ("Lambda", "\u{039B}"),
    ("Leftarrow", "\u{21D0}"),
    ("Omega", "\u{03A9}"),
    ("Phi", "\u{03A6}"),
    ("Pi", "\u{03A0}"),
    ("Psi", "\u{03A8}"),
    ("Rightarrow", "\u{21D2}"),
    ("Sigma", "\u{03A3}"),
    ("Theta", "\u{0398}"),
    ("Xi", "\u{039E}"),
    ("alpha", "\u{03B1}"),
    ("approx", "\u{2248}"),
    ("beta", "\u{03B2}"),
    ("cap", "\u{2229}"),
    ("cdot", "\u{22C5}"),
    ("cdots", "\u{22EF}"),
    ("chi", "\u{03C7}"),
    ("circ", "\u{2218}"),
    ("cup", "\u{222A}"),
    ("delta", "\u{03B4}"),
    ("div", "\u{00F7}"),
    ("emptyset", "\u{2205}"),
    ("epsilon", "\u{03B5}"),
    ("equiv", "\u{2261}"),
    ("eta", "\u{03B7}"),
    ("exists", "\u{2203}"),
    ("forall", "\u{2200}"),
    ("gamma", "\u{03B3}"),
    ("geq", "\u{2265}"),
    ("in", "\u{2208}"),
    ("infty", "\u{221E}"),
    ("int", "\u{222B}"),
    ("iota", "\u{03B9}"),
    ("kappa", "\u{03BA}"),
    ("lambda", "\u{03BB}"),
    ("langle", "\u{27E8}"),
    ("ldots", "\u{2026}"),
    ("leftarrow", "\u{2190}"),
    ("leftrightarrow", "\u{2194}"),
    ("leq", "\u{2264}"),
    ("mp", "\u{2213}"),
    ("mu", "\u{03BC}"),
    ("nabla", "\u{2207}"),
    ("neg", "\u{00AC}"),
    ("neq", "\u{2260}"),
    ("notin", "\u{2209}"),
    ("nu", "\u{03BD}"),
    ("oint", "\u{222E}"),
    ("omega", "\u{03C9}"),
    ("oplus", "\u{2295}"),
    ("otimes", "\u{2297}"),
    ("partial", "\u{2202}"),
    ("phi", "\u{03C6}"),
    ("pi", "\u{03C0}"),
    ("pm", "\u{00B1}"),
    ("prod", "\u{220F}"),
    ("propto", "\u{221D}"),
    ("psi", "\u{03C8}"),
    ("rangle", "\u{27E9}"),
    ("rho", "\u{03C1}"),
    ("rightarrow", "\u{2192}"),
    ("sigma", "\u{03C3}"),
    ("sim", "\u{223C}"),
    ("sqrt", "\u{221A}"),
    ("subset", "\u{2282}"),
    ("subseteq", "\u{2286}"),
    ("sum", "\u{2211}"),
    ("supset", "\u{2283}"),
    ("supseteq", "\u{2287}"),
    ("tau", "\u{03C4}"),
    ("theta", "\u{03B8}"),
    ("times", "\u{00D7}"),
    ("to", "\u{2192}"),
    ("upsilon", "\u{03C5}"),
    ("vee", "\u{2228}"),
    ("wedge", "\u{2227}"),
    ("xi", "\u{03BE}"),
    ("zeta", "\u{03B6}"),
];

/// The reserved characters of TeX, which show themselves when escaped
/// (`\%`), as noad reads them.
const TEX_RESERVED: &str = "#$%&_{}";

/// The text a TeX literal shows where noad does not typeset it (a table
/// cell, the copy, the search): a command with a known symbol becomes
/// the symbol, an escaped reserved character its character, and the
/// rest stays literal.
fn tex_symbols(tex: &str) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = String::with_capacity(tex.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let mut end = i + 1;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > i + 1 {
                let name: String = chars[i + 1..end].iter().collect();
                if let Ok(hit) = TEX_SYMBOLS.binary_search_by_key(&name.as_str(), |(n, _)| n) {
                    out.push_str(TEX_SYMBOLS[hit].1);
                    i = end;
                    continue;
                }
            } else if let Some(&c) = chars.get(i + 1).filter(|c| TEX_RESERVED.contains(**c)) {
                out.push(c);
                i += 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The display text a math literal renders as, scripts flattened: the
/// text the screen shows, which model copy and search read.
pub fn math_display(tex: &str) -> String {
    math_scripts(tex.trim())
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// Splits a TeX literal into script segments: `^` and `_` bind the next
/// character, a control sequence (`\pi`, `\%`) or a braced group, as in
/// TeX, and a backslash escapes the character after it, so the `_` of
/// `x\_1` is a character. Anything unmatched stays literal. Each
/// segment's text goes through `tex_symbols`.
fn math_scripts(tex: &str) -> Vec<(String, Script)> {
    fn flush(normal: &mut String, out: &mut Vec<(String, Script)>) {
        if !normal.is_empty() {
            out.push((tex_symbols(&std::mem::take(normal)), Script::Normal));
        }
    }
    let chars: Vec<char> = tex.chars().collect();
    let mut out = Vec::new();
    let mut normal = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            normal.push(c);
            if let Some(&next) = chars.get(i + 1) {
                normal.push(next);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '^' || c == '_' {
            let script = if c == '^' { Script::Sup } else { Script::Sub };
            match chars.get(i + 1) {
                Some('{') => {
                    if let Some(close) = chars[i + 2..].iter().position(|&ch| ch == '}') {
                        flush(&mut normal, &mut out);
                        let group: String = chars[i + 2..i + 2 + close].iter().collect();
                        out.push((tex_symbols(&group), script));
                        i += close + 3;
                        continue;
                    }
                }
                Some('\\') => {
                    // A control sequence is one token, as in TeX: the
                    // backslash with its letters (`\pi`, `\infty`), or
                    // with the one character after it (`\%`). A
                    // backslash that ends the text binds nothing.
                    let letters = chars[i + 2..]
                        .iter()
                        .take_while(|ch| ch.is_ascii_alphabetic())
                        .count();
                    let end = if letters > 0 { i + 2 + letters } else { i + 3 };
                    if end <= chars.len() {
                        flush(&mut normal, &mut out);
                        let word: String = chars[i + 1..end].iter().collect();
                        out.push((tex_symbols(&word), script));
                        i = end;
                        continue;
                    }
                }
                Some(&arg) => {
                    flush(&mut normal, &mut out);
                    out.push((arg.to_string(), script));
                    i += 2;
                    continue;
                }
                None => {}
            }
        }
        normal.push(c);
        i += 1;
    }
    flush(&mut normal, &mut out);
    out
}

pub(crate) fn alert_title(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "Note",
        AlertKind::Tip => "Tip",
        AlertKind::Important => "Important",
        AlertKind::Warning => "Warning",
        AlertKind::Caution => "Caution",
    }
}

pub(crate) fn alert_color(theme: &Theme, kind: AlertKind) -> Rgba {
    match kind {
        AlertKind::Note => theme.alerts.note,
        AlertKind::Tip => theme.alerts.tip,
        AlertKind::Important => theme.alerts.important,
        AlertKind::Warning => theme.alerts.warning,
        AlertKind::Caution => theme.alerts.caution,
    }
}

/// Frontmatter panel: dim key-value lines on `frontmatter_bg`.
#[allow(clippy::too_many_arguments)]
fn layout_frontmatter(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    entries: &[(String, String)],
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let pad = 12.0 * cfg.zoom;
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let mut spans = Vec::new();
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push(Span::plain("\n"));
        }
        spans.push(Span::plain(format!("{key}: {value}")));
    }
    let base = BlockStyle {
        size: 0.85 * cfg.body_size * cfg.zoom,
        color: theme.blocks.frontmatter_fg,
        bold: false,
        block_index,
        justify: false,
        align: None,
    };
    let runs_mark = out.runs.len();
    let rects_mark = out.rects.len();
    let text_h = shape_block(
        fonts,
        theme,
        cfg,
        source,
        &spans,
        false,
        &base,
        x0 + pad,
        y0 + pad,
        avail - 2.0 * pad,
        out,
    );
    // Selection addresses frontmatter by entry; the synthesized list
    // interleaves newline spans, so the entry is half the span index.
    for run in &mut out.runs[runs_mark..] {
        run.span /= 2;
    }
    let box_h = text_h + 2.0 * pad;
    out.rects.splice(
        rects_mark..rects_mark,
        [
            DecoRect::fill(x0, y0, avail, box_h, theme.blocks.frontmatter_bg)
                .rounded(radius, radius),
            DecoRect::fill(x0, y0, avail, box_h, theme.blocks.code_border)
                .rounded(radius, radius)
                .stroked((1.0 * cfg.zoom).max(1.0)),
        ],
    );
    box_h
}

/// Block math: the literal centered in a flush panel on the code
/// background, scripts rendered like inline math.
#[allow(clippy::too_many_arguments)]
/// noad's font handle: the embedded STIX face answers every metric and
/// glyph question, and the code face's fixed advance measures the literal
/// boxes the engine renders itself as fallback runs.
struct OryxMathFont {
    stix: noad::font::TtfMathFont<'static>,
    /// The code face's advance as a fraction of its em; Courier Prime is
    /// monospace, so one number measures any literal.
    literal_em: f32,
}

impl OryxMathFont {
    fn new() -> OryxMathFont {
        let stix = noad::font::TtfMathFont::from_bytes(crate::style::fonts::MATH_FONT)
            .expect("the embedded math face parses and carries MATH");
        let face = ttf_parser::Face::parse(crate::style::fonts::CODE_REGULAR, 0)
            .expect("the embedded code face parses");
        let advance = face
            .glyph_index('x')
            .and_then(|g| face.glyph_hor_advance(g))
            .unwrap_or(600);
        OryxMathFont {
            stix,
            literal_em: f32::from(advance) / f32::from(face.units_per_em()),
        }
    }
}

impl noad::MathFont for OryxMathFont {
    fn units_per_em(&self) -> f32 {
        self.stix.units_per_em()
    }
    fn glyph(&self, c: char) -> Option<noad::font::GlyphId> {
        self.stix.glyph(c)
    }
    fn advance(&self, glyph: noad::font::GlyphId) -> f32 {
        self.stix.advance(glyph)
    }
    fn bounds(&self, glyph: noad::font::GlyphId) -> noad::font::Bounds {
        self.stix.bounds(glyph)
    }
    fn italic_correction(&self, glyph: noad::font::GlyphId) -> f32 {
        self.stix.italic_correction(glyph)
    }
    fn top_accent(&self, glyph: noad::font::GlyphId) -> Option<f32> {
        self.stix.top_accent(glyph)
    }
    fn constants(&self) -> noad::font::MathConstants {
        self.stix.constants()
    }
    fn vertical_variants(&self, glyph: noad::font::GlyphId) -> Vec<noad::font::Variant> {
        self.stix.vertical_variants(glyph)
    }
    fn horizontal_variants(&self, glyph: noad::font::GlyphId) -> Vec<noad::font::Variant> {
        self.stix.horizontal_variants(glyph)
    }
    fn vertical_assembly(&self, glyph: noad::font::GlyphId) -> Option<noad::font::Assembly> {
        self.stix.vertical_assembly(glyph)
    }
    fn horizontal_assembly(&self, glyph: noad::font::GlyphId) -> Option<noad::font::Assembly> {
        self.stix.horizontal_assembly(glyph)
    }
    fn kern(&self, glyph: noad::font::GlyphId, corner: noad::font::Corner, height: f32) -> f32 {
        self.stix.kern(glyph, corner, height)
    }
    fn measure_literal(&self, text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * self.literal_em * size
    }
}

/// Emits a laid-out equation into the document at `x` with its baseline at
/// `baseline`: glyphs into the math vector in their color or the foreground ink, rules as
/// filled rects, and literal fallbacks as code-family runs in the theme's
/// math color, which is that role's job under rendered math.
#[allow(clippy::too_many_arguments)]
fn emit_math_layout(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    m: &noad::layout::MathLayout,
    x: f32,
    baseline: f32,
    block_index: usize,
    out: &mut LayoutDoc,
) {
    let ink = theme.surface.foreground;
    // A color the formula set (`\color`, `\textcolor`) over the theme's ink.
    let color = |c: Option<[u8; 3]>| {
        c.map_or(ink, |[r, g, b]| crate::style::theme::Rgba {
            r,
            g,
            b,
            a: 255,
        })
    };
    let top = baseline - m.ascent;
    let bottom = baseline + m.descent;
    let right = x + m.width;
    for g in &m.glyphs {
        out.math_glyphs.push(MathGlyph {
            glyph: g.glyph.0,
            x: x + g.x,
            y: baseline + g.y,
            size: g.size,
            ch: g.ch,
            top,
            bottom,
            left: x,
            right,
            color: color(g.color),
            block: block_index,
        });
    }
    for r in &m.rules {
        out.rects.push(
            DecoRect::fill(x + r.x, baseline + r.y, r.width, r.height, color(r.color)).smooth(),
        );
    }
    for lit in &m.literals {
        let (runs, _) = shape_side_text(
            fonts,
            cfg,
            &cfg.code_family.clone(),
            &lit.text,
            lit.size,
            theme.text.math,
            out,
        );
        for mut run in runs {
            let dy = (baseline + lit.y) - run.baseline;
            run.x += x + lit.x;
            run.y += dy;
            run.baseline += dy;
            run.block = block_index;
            out.runs.push(run);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn layout_math_block(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    tex: &str,
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let font = OryxMathFont::new();
    let size = cfg.body_size * cfg.zoom;
    let mut m = noad::layout::layout(tex.trim(), noad::layout::MathStyle::Display, size, &font);
    // A display equation wider than the column shrinks uniformly to fit,
    // down to the half-size floor; past the floor it overflows and the
    // surface edge clips it.
    if m.width > avail {
        let s = (avail / m.width).max(0.5);
        scale_math_layout(&mut m, s);
    }
    let x = x0 + ((avail - m.width) / 2.0).max(0.0);
    let baseline = y0 + m.ascent;
    emit_math_layout(fonts, theme, cfg, &m, x, baseline, block_index, out);
    (m.ascent + m.descent).max(metrics::LINE_HEIGHT * size)
}

/// Scales flat math geometry uniformly. Layout is linear in the em size,
/// so scaling the output equals laying out at the scaled size.
fn scale_math_layout(m: &mut noad::layout::MathLayout, s: f32) {
    for g in &mut m.glyphs {
        g.x *= s;
        g.y *= s;
        g.size *= s;
    }
    for r in &mut m.rules {
        r.x *= s;
        r.y *= s;
        r.width *= s;
        r.height *= s;
    }
    for lit in &mut m.literals {
        lit.x *= s;
        lit.y *= s;
        lit.size *= s;
        lit.width *= s;
    }
    m.width *= s;
    m.ascent *= s;
    m.descent *= s;
}

/// One footnote definition: the label as a small raised marker in link
/// color, the note text indented beside it.
/// The number a footnote block sorts by at the foot; the body's blocks
/// never ask.
fn footnote_number(block: &Block) -> u32 {
    match &block.kind {
        BlockKind::FootnoteDef { number, .. } => *number,
        _ => 0,
    }
}

/// Lays out one block of a footnote definition: the number in the link
/// color, linking back to the first reference, the text indented past
/// it. A continued block measures the number to take the same indent
/// and draws none.
#[allow(clippy::too_many_arguments)]
fn layout_footnote_def(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    label: &str,
    number: u32,
    continued: bool,
    spans: &[Span],
    base_size: f32,
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    if !continued && !out.footnote_backs.iter().any(|(b, _)| *b == block_index) {
        out.footnote_backs
            .push((block_index, format!("fnref:{label}")));
    }
    let marker = [Span::plain(format!("{number}."))];
    let marker_base = BlockStyle {
        size: 0.7 * base_size,
        color: theme.text.link,
        bold: true,
        block_index,
        justify: false,
        align: None,
    };
    let runs_mark = out.runs.len();
    shape_block(
        fonts,
        theme,
        cfg,
        source,
        &marker,
        false,
        &marker_base,
        x0,
        y0,
        avail,
        out,
    );
    let marker_w = out.runs[runs_mark..]
        .iter()
        .map(|r| r.x + r.width)
        .fold(x0, f32::max)
        - x0;
    if continued {
        out.runs.truncate(runs_mark);
    }
    // The number is a marker run: outside the selection, its link
    // answered by `footnote_backs`.
    for run in &mut out.runs[runs_mark..] {
        run.span = usize::MAX;
    }
    let indent = (marker_w + 8.0 * cfg.zoom).max(metrics::INDENT * cfg.zoom);
    let base = BlockStyle {
        size: base_size,
        color: theme.text.body,
        bold: false,
        block_index,
        justify: false,
        align: None,
    };
    let width = (avail - indent).max(40.0);
    // A definition with a formula flows as a paragraph does, its
    // pictures as their text, as in a table cell.
    let text_h = if spans.iter().any(|s| s.math) {
        let spans = cell_without_images(spans);
        layout_flow(
            fonts,
            theme,
            cfg,
            source,
            media,
            &spans,
            &base,
            x0 + indent,
            y0,
            width,
            out,
        )
    } else {
        shape_block(
            fonts,
            theme,
            cfg,
            source,
            spans,
            true,
            &base,
            x0 + indent,
            y0,
            width,
            out,
        )
    };
    text_h.max(metrics::LINE_HEIGHT * base_size)
}

/// Chooses between plain shaping and mixed text-and-image flow.
#[allow(clippy::too_many_arguments)]
fn flow_or_shape(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    spans: &[Span],
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    if spans.iter().any(|s| s.image.is_some() || s.math) {
        layout_flow(
            fonts, theme, cfg, source, media, spans, base, x0, y0, avail, out,
        )
    } else {
        shape_block(
            fonts, theme, cfg, source, spans, true, base, x0, y0, avail, out,
        )
    }
}

/// Where a flow line's content starts in the layout vectors: everything
/// appended past these marks belongs to the open line, which is what
/// lets a tall joining equation shift the whole line down.
struct LineMarks {
    runs: usize,
    rects: usize,
    images: usize,
    math: usize,
}

/// One line images and equations may join: its top, height, pen x, the
/// text baseline when it carries one, and whether it is a standalone row
/// rather than a text line.
struct FlowLine {
    top: f32,
    height: f32,
    end_x: f32,
    /// Present on text lines and equation rows; image rows have none.
    baseline: Option<f32>,
    row: bool,
    marks: LineMarks,
}

/// Moves an open line's content down, TeX's baselineskip stretching to
/// admit taller ink.
fn shift_line_down(out: &mut LayoutDoc, marks: &LineMarks, dy: f32) {
    for run in &mut out.runs[marks.runs..] {
        run.y += dy;
        run.baseline += dy;
    }
    for rect in &mut out.rects[marks.rects..] {
        rect.y += dy;
    }
    for image in &mut out.images[marks.images..] {
        image.y += dy;
    }
    for g in &mut out.math_glyphs[marks.math..] {
        g.y += dy;
        g.top += dy;
        g.bottom += dy;
    }
}

/// A shaped chunk's last visual line: its top, its right edge, and its
/// baseline, read off the chunk's runs.
fn chunk_metrics(runs: &[TextRun], x0: f32, y: f32) -> (f32, f32, f32) {
    let last_top = runs.iter().map(|r| r.y).fold(y, f32::max);
    let end_x = runs
        .iter()
        .filter(|r| (r.y - last_top).abs() < 1.0)
        .map(|r| r.x + r.width)
        .fold(x0, f32::max);
    let last_base = runs
        .iter()
        .filter(|r| (r.y - last_top).abs() < 1.0)
        .map(|r| r.baseline)
        .fold(0.0, f32::max);
    (last_top, end_x, last_base)
}

/// Lays out the text chunk after an equation on an open line. The chunk
/// is shaped in the room left, first visual line only, and that line
/// joins the open line as shaped, so a justified paragraph stretches it
/// to the margin. The remainder is shaped again at the full width, from
/// the first word after the cut, the model offsets of the cut span
/// shifted by the bytes skipped, so selection and copy stay exact. The
/// chunk starts a new line whole, from its first word, when no word
/// fits, when the first line would end inside a word, or when the chunk
/// reads right to left: shaped alone, such a line is laid out right to
/// left and lands over the text before the equation. Answers the
/// remainder's marks and height; marks at the end of the runs mean the
/// whole chunk joined the line.
#[allow(clippy::too_many_arguments)]
fn split_after_equation(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    masked: &[Span],
    start: usize,
    end: usize,
    line: &mut FlowLine,
    base: &BlockStyle,
    x0: f32,
    y: f32,
    avail: f32,
    runs_mark: usize,
    rects_mark: usize,
    out: &mut LayoutDoc,
) -> (usize, usize, f32) {
    let lb = line.baseline.expect("an open line with a baseline");
    let room = x0 + avail - line.end_x;
    out.runs.truncate(runs_mark);
    out.rects.truncate(rects_mark);
    let rtl = match cfg.direction {
        DirectionMode::Rtl => true,
        DirectionMode::Ltr => false,
        DirectionMode::Auto => masked[start..end]
            .iter()
            .flat_map(|s| s.text(source).chars())
            .find_map(crate::style::fonts::strong_rtl)
            .unwrap_or(false),
    };
    // The cut: where the first visual line of the chunk, shaped in the
    // room left, ends. No cut when nothing fits (a first line of a blank
    // alone, which shaping trims, or wider than the room, a word that
    // cannot break) or when the line would end inside a word.
    let mut cut: Option<(usize, usize)> = None;
    if room > 0.0 && !rtl {
        shape_block_lines(
            fonts, theme, cfg, source, masked, true, base, x0, y, room, true, out,
        );
        let runs = &out.runs[runs_mark..];
        let left = runs.iter().map(|r| r.x).fold(f32::MAX, f32::min);
        let right = runs.iter().map(|r| r.x + r.width).fold(x0, f32::max);
        let last = runs
            .iter()
            .map(|r| {
                let len = masked[r.span].text(source).len();
                match r.text {
                    TextRef::Model { start: s, len: l } => (r.span, ((s + l) as usize).min(len)),
                    TextRef::Side { .. } => (r.span, len),
                }
            })
            .max();
        if let Some(c) = last {
            let fits = left >= x0 - 0.5 && right - left <= room + 0.5;
            if fits && cut_at_a_break(masked, source, c, end) {
                cut = Some(c);
            }
        }
    }
    let (mut span, mut byte) = match cut {
        Some(c) => {
            // The first line joins the open line. Its baseline is the
            // line box's, which any run gives whatever its own raise.
            let runs = &out.runs[runs_mark..];
            let pb = runs.first().map_or(y, |r| y + r.baseline - r.y);
            let pw = runs.iter().map(|r| r.x + r.width).fold(x0, f32::max) - x0;
            let dx = line.end_x - x0;
            let dy = lb - pb;
            for run in &mut out.runs[runs_mark..] {
                run.x += dx;
                run.y += dy;
                run.baseline += dy;
            }
            for rect in &mut out.rects[rects_mark..] {
                rect.x += dx;
                rect.y += dy;
            }
            line.end_x += pw;
            c
        }
        None => {
            out.runs.truncate(runs_mark);
            out.rects.truncate(rects_mark);
            (start, 0)
        }
    };
    // The remainder starts at the first word after the cut.
    loop {
        let text = masked[span].text(source);
        byte += text[byte..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        if byte < text.len() || span + 1 >= end {
            break;
        }
        span += 1;
        byte = 0;
    }
    let rmark_runs = out.runs.len();
    let rmark_rects = out.rects.len();
    if byte >= masked[span].text(source).len() {
        return (rmark_runs, rmark_rects, 0.0);
    }
    let mut rest_spans = masked.to_vec();
    for s in &mut rest_spans[start..span] {
        s.clear_text();
    }
    let tail = masked[span].text(source)[byte..].to_string();
    rest_spans[span].set_text(tail);
    let h = shape_block(
        fonts,
        theme,
        cfg,
        source,
        &rest_spans,
        true,
        base,
        x0,
        y,
        avail,
        out,
    );
    for run in &mut out.runs[rmark_runs..] {
        if run.span == span {
            if let TextRef::Model { start: s, .. } = &mut run.text {
                *s += byte as u32;
            }
        }
    }
    (rmark_runs, rmark_rects, h)
}

/// Whether a cut of the chunk at `(span, byte)` is a place where a line
/// may break: whitespace follows it, the chunk ends there, or the Unicode
/// line-breaking rules allow a break there in the span's text (after a
/// hyphen or a dash, between Chinese characters), the rules the shaper
/// breaks by. A cut anywhere else is inside a word wider than the room,
/// which shaping breaks between glyphs.
fn cut_at_a_break(
    masked: &[Span],
    source: &str,
    (mut span, mut byte): (usize, usize),
    end: usize,
) -> bool {
    loop {
        let text = masked[span].text(source);
        if byte < text.len() {
            return text[byte..].starts_with(char::is_whitespace)
                || unicode_linebreak::linebreaks(text).any(|(i, _)| i == byte);
        }
        span += 1;
        if span >= end {
            return true;
        }
        byte = 0;
    }
}

/// Closes the flow's open line: a row of its own, whose height the flow
/// books when the line closes, moves the pen below it.
fn close_line(line: &mut Option<FlowLine>, y: &mut f32, size: f32) {
    if let Some(l) = line.take() {
        if l.row {
            *y = l.top + l.height + 0.25 * size;
        }
    }
}

/// A hard break in the flow: it closes the open line, and with no line
/// open after another break it leaves a blank line.
fn hard_break(
    line: &mut Option<FlowLine>,
    y: &mut f32,
    size: f32,
    line_height: f32,
    broke: &mut bool,
) {
    if line.is_some() {
        close_line(line, y, size);
    } else if *broke {
        *y += line_height;
    }
    *broke = true;
}

/// Where a box alone on its row sits in the room left: at the left, in
/// the middle or at the right, by the block's alignment.
fn align_factor(align: Option<Align>) -> f32 {
    match align {
        Some(Align::Center) => 0.5,
        Some(Align::Right) => 1.0,
        _ => 0.0,
    }
}

/// Text with inline images or equations. Text shapes normally; an image
/// or equation joins the last text line when it fits there, otherwise it
/// opens its own row. A one-line text chunk that follows an equation on an
/// open line merges back onto that line, so a sentence flows around its
/// math; longer continuations start below.
#[allow(clippy::too_many_arguments)]
fn layout_flow(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    media: &mut MediaCache,
    spans: &[Span],
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let line_height = metrics::LINE_HEIGHT * base.size;
    let gap = 8.0 * cfg.zoom;
    let mut y = y0;
    let mut line: Option<FlowLine> = None;
    // The last thing the flow met was a hard break: the next one in a
    // row is a blank line. The paragraph's start counts as one, so a
    // break that opens the paragraph leaves a blank line, as it does in
    // a paragraph of text alone.
    let mut broke = true;
    let is_break = |s: &Span| s.image.is_none() && !s.math && s.text(source) == "\n";

    let mut i = 0;
    while i < spans.len() {
        if is_break(&spans[i]) {
            // A hard break that opens a piece of text is the flow's own:
            // it closes the open line, so the text after it starts a line
            // below at the margin and is no part of a formula's line.
            hard_break(&mut line, &mut y, base.size, line_height, &mut broke);
            i += 1;
        } else if spans[i].image.is_none() && !spans[i].math {
            // The text chunk up to the next image or equation, masked into
            // a full-length span list so run span indices keep mapping to
            // the model.
            let start = i;
            while i < spans.len() && spans[i].image.is_none() && !spans[i].math {
                i += 1;
            }
            // A hard break closes the open line: the next box starts a
            // line of its own below it, as the text after the break does.
            let ends_with_break = spans[i - 1].text(source) == "\n";
            if spans[start..i]
                .iter()
                .all(|s| s.text(source).trim().is_empty())
            {
                let breaks = spans[start..i].iter().filter(|s| is_break(s)).count();
                for _ in 0..breaks {
                    hard_break(&mut line, &mut y, base.size, line_height, &mut broke);
                }
                if breaks > 0 {
                } else if let Some(l) = line.as_mut() {
                    // Whitespace between two placed boxes keeps its word gap.
                    if l.baseline.is_some() {
                        l.end_x += 0.25 * base.size;
                    }
                }
                continue;
            }
            let masked: Vec<Span> = spans
                .iter()
                .enumerate()
                .map(|(si, s)| {
                    let mut c = s.clone();
                    if si < start || si >= i || s.image.is_some() || s.math {
                        c.clear_text();
                        c.image = None;
                        c.math = false;
                    }
                    c
                })
                .collect();
            let runs_mark = out.runs.len();
            let rects_mark = out.rects.len();
            // After an equation on an open text line, the split lays the
            // chunk out: the words that fit join the line, and the rest
            // flows below from its first word.
            let after_equation = start > 0
                && spans[start - 1].math
                && line.as_ref().is_some_and(|l| l.baseline.is_some());
            let (h, runs_mark, rects_mark, merged) = if after_equation {
                let l = line.as_mut().expect("an open line with a baseline");
                let (rm, rcm, rh) = split_after_equation(
                    fonts, theme, cfg, source, &masked, start, i, l, base, x0, y, avail, runs_mark,
                    rects_mark, out,
                );
                if out.runs.len() == rm {
                    // The whole chunk joined the line, which stays open
                    // unless the chunk ended with a hard break.
                    broke = false;
                    if ends_with_break {
                        hard_break(&mut line, &mut y, base.size, line_height, &mut broke);
                    }
                    continue;
                }
                (rh, rm, rcm, false)
            } else {
                let h = shape_block(
                    fonts, theme, cfg, source, &masked, true, base, x0, y, avail, out,
                );
                let (_, end_x, last_base) = chunk_metrics(&out.runs[runs_mark..], x0, y);
                let single_line = h <= 1.5 * line_height;
                let merged = if let Some(l) = line.as_mut() {
                    let fits = l.end_x + (end_x - x0) <= x0 + avail;
                    match l.baseline {
                        Some(lb) if single_line && fits => {
                            let dx = l.end_x - x0;
                            let dy = lb - last_base;
                            for run in &mut out.runs[runs_mark..] {
                                run.x += dx;
                                run.y += dy;
                                run.baseline += dy;
                            }
                            for rect in &mut out.rects[rects_mark..] {
                                rect.x += dx;
                                rect.y += dy;
                            }
                            l.end_x += end_x - x0;
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                };
                (h, runs_mark, rects_mark, merged)
            };
            let (mut last_top, end_x, mut last_base) = chunk_metrics(&out.runs[runs_mark..], x0, y);
            if !merged {
                if let Some(l) = line.take() {
                    if l.row {
                        let below = l.top + l.height + 0.25 * base.size;
                        if below > y {
                            // The chunk shaped over the open row; it moves
                            // below, and the line metrics move with it so
                            // the next box joins the real line, not a stale
                            // baseline over the row.
                            let dy = below - y;
                            for run in &mut out.runs[runs_mark..] {
                                run.y += dy;
                                run.baseline += dy;
                            }
                            for rect in &mut out.rects[rects_mark..] {
                                rect.y += dy;
                            }
                            last_top += dy;
                            last_base += dy;
                            y = below;
                        }
                    }
                }
                y += h;
                // The open line is the chunk's last visual line; raised
                // marks like footnote references sit within half a size
                // of its top.
                let line_runs = (runs_mark..out.runs.len())
                    .find(|&r| out.runs[r].y >= last_top - 0.5 * base.size)
                    .unwrap_or(runs_mark);
                let line_rects = (rects_mark..out.rects.len())
                    .find(|&r| out.rects[r].y >= last_top - 0.5 * base.size)
                    .unwrap_or(out.rects.len());
                line = Some(FlowLine {
                    top: last_top,
                    height: line_height,
                    end_x,
                    baseline: Some(last_base),
                    row: false,
                    marks: LineMarks {
                        runs: line_runs,
                        rects: line_rects,
                        images: out.images.len(),
                        math: out.math_glyphs.len(),
                    },
                });
            }
            broke = false;
            if ends_with_break {
                hard_break(&mut line, &mut y, base.size, line_height, &mut broke);
            }
        } else if spans[i].math {
            broke = false;
            let span = &spans[i];
            let font = OryxMathFont::new();
            let style = if span.display {
                noad::layout::MathStyle::Display
            } else {
                noad::layout::MathStyle::Text
            };
            let mut m = noad::layout::layout(span.text(source).trim(), style, base.size, &font);
            // A formula wider than the width at hand (a table cell, a
            // narrow column) shrinks into it, down to the half-size floor,
            // as a display block wider than the column does; past the
            // floor it overflows.
            if m.width > avail {
                let s = (avail / m.width).max(0.5);
                scale_math_layout(&mut m, s);
            }
            let w = m.width.max(1.0);
            // A space the author typed before the equation is the trailing
            // byte of the preceding text chunk, and shaping drops trailing
            // whitespace at the chunk's line end; the pen restores it.
            let spaced = (span.range.start as usize)
                .checked_sub(1)
                .and_then(|i| source.as_bytes().get(i))
                .is_some_and(|b| b.is_ascii_whitespace());
            let space = if spaced { 0.3 * base.size } else { 0.0 };
            let joins = line
                .as_ref()
                .is_some_and(|l| l.baseline.is_some() && l.end_x + space + w <= x0 + avail);
            if joins {
                let l = line.as_mut().expect("open line");
                let lb = l.baseline.expect("checked");
                // Ink taller than the line's ascent shifts the whole line
                // down, TeX's baselineskip stretching, so nothing reaches
                // the line above.
                let dy = (m.ascent - (lb - l.top)).max(0.0);
                if dy > 0.0 {
                    shift_line_down(out, &l.marks, dy);
                    l.baseline = Some(lb + dy);
                    l.height += dy;
                }
                let lb = lb + dy;
                l.end_x += space;
                emit_math_layout(fonts, theme, cfg, &m, l.end_x, lb, base.block_index, out);
                l.end_x += w;
                // Ink deeper than the line box pushes what follows below.
                if l.row {
                    l.height = l
                        .height
                        .max((lb - l.top) + m.descent.max(0.25 * line_height));
                } else {
                    y = y.max(lb + m.descent + 0.25 * base.size);
                }
            } else {
                if let Some(l) = line.take() {
                    if l.row {
                        y = l.top + l.height + 0.25 * base.size;
                    }
                }
                let marks = LineMarks {
                    runs: out.runs.len(),
                    rects: out.rects.len(),
                    images: out.images.len(),
                    math: out.math_glyphs.len(),
                };
                let baseline = y + m.ascent.max(0.75 * line_height);
                // A row of its own follows the block's alignment, as the
                // text lines do: centered or at the right under a mark.
                let x = x0 + (avail - w).max(0.0) * align_factor(base.align);
                emit_math_layout(fonts, theme, cfg, &m, x, baseline, base.block_index, out);
                let height = (baseline - y) + m.descent.max(0.25 * line_height);
                line = Some(FlowLine {
                    top: y,
                    height,
                    end_x: x + w,
                    baseline: Some(baseline),
                    row: true,
                    marks,
                });
            }
            i += 1;
        } else {
            broke = false;
            let span = &spans[i];
            let image = span.image.as_ref().expect("image span");
            let (w, h, loaded) = image_size(fonts, media, span, image, cfg, source, avail);
            let joins = line
                .as_ref()
                .is_some_and(|l| l.end_x + gap + w <= x0 + avail && (l.row || h <= l.height + 2.0));
            if joins {
                let l = line.as_mut().expect("open line");
                let iy = if l.row {
                    l.top
                } else {
                    l.top + 0.85 * l.height - h
                };
                place_image(
                    fonts,
                    cfg,
                    source,
                    out,
                    theme,
                    span,
                    image,
                    base.block_index,
                    l.end_x + gap,
                    iy,
                    w,
                    h,
                    loaded,
                );
                l.end_x += gap + w;
                if l.row && h > l.height {
                    l.height = h;
                }
            } else {
                if let Some(l) = line.take() {
                    if l.row {
                        y = l.top + l.height + 0.25 * base.size;
                    }
                }
                let marks = LineMarks {
                    runs: out.runs.len(),
                    rects: out.rects.len(),
                    images: out.images.len(),
                    math: out.math_glyphs.len(),
                };
                place_image(
                    fonts,
                    cfg,
                    source,
                    out,
                    theme,
                    span,
                    image,
                    base.block_index,
                    x0,
                    y,
                    w,
                    h,
                    loaded,
                );
                line = Some(FlowLine {
                    top: y,
                    height: h,
                    end_x: x0 + w,
                    baseline: None,
                    row: true,
                    marks,
                });
            }
            i += 1;
        }
    }
    if let Some(l) = line {
        if l.row {
            y = l.top + l.height;
        }
    }
    (y - y0).max(line_height)
}

/// The text a placeholder shows for an inline image: the span's text,
/// which is the alt, or the file's name when the alt is empty.
fn image_label<'a>(span: &'a Span, image: &'a SpanImage, source: &'a str) -> &'a str {
    let alt = span.text(source).trim();
    if !alt.is_empty() {
        return alt;
    }
    image
        .src
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(&image.src)
}

/// Display size of an inline image: attribute pixels win, the natural size
/// fills in the rest, everything capped to the content width. The flag is
/// false when no pixels are available yet. With neither attributes nor
/// pixels the box is one text line sized to the label it will show.
#[allow(clippy::too_many_arguments)]
fn image_size(
    fonts: &mut FontStore,
    media: &mut MediaCache,
    span: &Span,
    image: &SpanImage,
    cfg: &ViewConfig,
    source: &str,
    avail: f32,
) -> (f32, f32, bool) {
    let natural = media.dimensions(&image.src);
    let scale = image_scale(cfg);
    let aw = image.width.map(|v| v as f32 * scale);
    let ah = image.height.map(|v| v as f32 * scale);
    let (mut w, mut h) = match (aw, ah, natural) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((nw, nh))) => (w, w * nh as f32 / nw as f32),
        (None, Some(h), Some((nw, nh))) => (h * nw as f32 / nh as f32, h),
        (None, None, Some((nw, nh))) => (nw as f32 * scale, nh as f32 * scale),
        (Some(w), None, None) => (w, w * 0.5),
        (None, Some(h), None) => (h * 2.0, h),
        (None, None, None) => {
            let size = cfg.body_size * cfg.zoom;
            let label = image_label(span, image, source);
            let text_w = side_text_width(fonts, cfg, label, size);
            let pad = metrics::PLACEHOLDER_PAD * cfg.zoom;
            (
                (text_w + 2.0 * pad).max(120.0 * scale).min(avail),
                metrics::LINE_HEIGHT * size,
            )
        }
    };
    if w > avail {
        h *= avail / w;
        w = avail;
    }
    (w, h, natural.is_some())
}

/// Places one inline image; without pixels yet a placeholder box holds
/// its spot, in the block placeholder's style, with the alt text on one
/// line inside it: the span's text, or the file's name when the alt is
/// empty, shortened to fit. The box keeps its size either way, so an
/// arrival moves nothing.
#[allow(clippy::too_many_arguments)]
fn place_image(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    source: &str,
    out: &mut LayoutDoc,
    theme: &Theme,
    span: &Span,
    image: &SpanImage,
    block_index: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    loaded: bool,
) {
    out.images.push(ImagePlace {
        src: image.src.clone(),
        x,
        y,
        width: w,
        height: h,
        link: span.link.clone(),
    });
    if loaded {
        return;
    }
    out.rects
        .push(DecoRect::fill(x, y, w, h, theme.blocks.frontmatter_bg));
    out.rects
        .push(DecoRect::fill(x, y, w, h, theme.blocks.code_border).stroked(1.0));
    let label = image_label(span, image, source);
    // The text takes the body size when the box has the height for it,
    // and shrinks to the box otherwise; below a legible size the box
    // stays bare.
    let size = (cfg.body_size * cfg.zoom).min(0.6 * h);
    if size < 6.0 {
        return;
    }
    let pad = (metrics::PLACEHOLDER_PAD * cfg.zoom)
        .min(0.25 * h)
        .min(0.1 * w);
    let inner = w - 2.0 * pad;
    let Some((mut runs, width)) =
        fit_side_text(fonts, cfg, label, size, theme.text.italic, inner, out)
    else {
        return;
    };
    for run in &mut runs {
        run.italic = true;
    }
    let line_height = metrics::LINE_HEIGHT * cfg.body_size * cfg.zoom;
    let text_x = x + pad + (inner - width).max(0.0) / 2.0;
    let text_y = y + (h - line_height) / 2.0;
    place_marker(runs, text_x, text_y, block_index, out);
}

/// Side text shaped to fit `avail` on one line: whole when it fits,
/// else the longest head that fits with an ellipsis after it. None
/// when not even one character fits beside the ellipsis.
fn fit_side_text(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    text: &str,
    size: f32,
    color: Rgba,
    avail: f32,
    out: &mut LayoutDoc,
) -> Option<(Vec<TextRun>, f32)> {
    let (runs, width) = shape_marker(fonts, cfg, text, size, color, out);
    if width <= avail {
        return Some((runs, width));
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    let mut best = None;
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let head: String = chars[..mid]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
            + "\u{2026}";
        let (runs, width) = shape_marker(fonts, cfg, &head, size, color, out);
        if width <= avail {
            best = Some((runs, width));
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    best
}

/// Shifts every element of an aligned block so each visual line sits at
/// `factor` of the room left in the content width: 0.0 puts it against
/// the left edge, 0.5 centers it, 1.0 puts it against the right edge.
/// A line may move either way, since right-to-left text starts on the
/// right; a line wider than the content stays where it is. Lines are
/// clustered by vertical overlap.
#[allow(clippy::too_many_arguments)]
fn align_lines(
    out: &mut LayoutDoc,
    runs_mark: usize,
    rects_mark: usize,
    images_mark: usize,
    math_mark: usize,
    x0: f32,
    avail: f32,
    factor: f32,
) {
    // (top, bottom, kind, index) per element; kinds: 0 runs, 1 rects,
    // 2 images, 3 math glyphs.
    let mut items: Vec<(f32, f32, u8, usize)> = Vec::new();
    for (i, r) in out.runs.iter().enumerate().skip(runs_mark) {
        items.push((r.y, r.y + metrics::LINE_HEIGHT * r.size, 0, i));
    }
    for (i, r) in out.rects.iter().enumerate().skip(rects_mark) {
        items.push((r.y, r.y + r.height, 1, i));
    }
    for (i, im) in out.images.iter().enumerate().skip(images_mark) {
        items.push((im.y, im.y + im.height, 2, i));
    }
    for (i, g) in out.math_glyphs.iter().enumerate().skip(math_mark) {
        items.push((g.top, g.bottom, 3, i));
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut start = 0;
    while start < items.len() {
        let mut end = start + 1;
        let mut bottom = items[start].1;
        while end < items.len() && items[end].0 < bottom {
            bottom = bottom.max(items[end].1);
            end += 1;
        }
        let group = &items[start..end];
        let span_x = |item: &(f32, f32, u8, usize)| -> (f32, f32) {
            match item.2 {
                0 => (out.runs[item.3].x, out.runs[item.3].width),
                1 => (out.rects[item.3].x, out.rects[item.3].width),
                2 => (out.images[item.3].x, out.images[item.3].width),
                _ => {
                    let g = &out.math_glyphs[item.3];
                    (g.left, g.right - g.left)
                }
            }
        };
        let min_x = group.iter().map(|it| span_x(it).0).fold(f32::MAX, f32::min);
        let max_x = group
            .iter()
            .map(|it| {
                let (x, w) = span_x(it);
                x + w
            })
            .fold(0.0, f32::max);
        let dx = x0 + (avail - (max_x - min_x)) * factor - min_x;
        if dx.abs() > 0.5 && min_x + dx >= x0 - 0.5 {
            for item in group {
                match item.2 {
                    0 => out.runs[item.3].x += dx,
                    1 => out.rects[item.3].x += dx,
                    2 => out.images[item.3].x += dx,
                    _ => {
                        out.math_glyphs[item.3].x += dx;
                        out.math_glyphs[item.3].left += dx;
                        out.math_glyphs[item.3].right += dx;
                    }
                }
            }
        }
        start = end;
    }
}

/// Shapes marker text at origin; the caller places it. Returns total width.
fn shape_marker(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    text: &str,
    size: f32,
    color: Rgba,
    out: &mut LayoutDoc,
) -> (Vec<TextRun>, f32) {
    let family = cfg.body_family.clone();
    shape_side_text(fonts, cfg, &family, text, size, color, out)
}

/// The width side text takes in the body family at `size`, with no run
/// made: the measure behind a box sized to its label.
fn side_text_width(fonts: &mut FontStore, cfg: &ViewConfig, text: &str, size: f32) -> f32 {
    let line_height = metrics::LINE_HEIGHT * cfg.body_size * cfg.zoom;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let attrs = Attrs::new().family(Family::Name(&cfg.body_family));
    buffer.set_text(
        &mut fonts.font_system,
        &crate::style::fonts::shapable(text),
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    buffer
        .layout_runs()
        .map(|run| line_width(run.glyphs))
        .fold(0.0, f32::max)
}

/// Shapes synthesized side text in a given family at origin; the caller
/// places the runs. Returns them with the total width.
fn shape_side_text(
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    family_name: &str,
    text: &str,
    size: f32,
    color: Rgba,
    out: &mut LayoutDoc,
) -> (Vec<TextRun>, f32) {
    let line_height = metrics::LINE_HEIGHT * cfg.body_size * cfg.zoom;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let attrs = Attrs::new().family(Family::Name(family_name));
    buffer.set_text(
        &mut fonts.font_system,
        &crate::style::fonts::shapable(text),
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    let mut runs = Vec::new();
    let mut width = 0.0f32;
    for run in buffer.layout_runs() {
        if run.glyphs.is_empty() {
            continue;
        }
        let run_width = line_width(run.glyphs);
        width = width.max(run_width);
        let text = out.side_ref(text);
        let family = out.family_id(family_name);
        runs.push(TextRun {
            text,
            x: 0.0,
            y: run.line_top,
            baseline: run.line_y,
            width: run_width,
            size,
            family,
            weight: Weight::NORMAL.0,
            italic: false,
            color,
            block: 0,
            span: usize::MAX,
        });
    }
    (runs, width)
}

/// Marker runs use span `usize::MAX`: synthetic, excluded from selection.
fn place_marker(runs: Vec<TextRun>, x: f32, y: f32, block_index: usize, out: &mut LayoutDoc) {
    for mut run in runs {
        run.x += x;
        run.y += y;
        run.baseline += y;
        run.block = block_index;
        out.runs.push(run);
    }
}

/// Recolors every patched code line by rebuilding only the touched span
/// of the run vector: untouched stretches inside it move through, patched
/// lines re-shape in place, and everything outside the span moves once,
/// however many drains a wash-in takes. Records shift by the running
/// delta. Geometry is unchanged; run indices shift, so callers drop
/// selection and search positions exactly as they do after a relayout. A
/// record whose model line vanished keeps its old runs; arrivals are
/// trusted only as far as the document reaches. Answers the splice it
/// made, (first run, old end run, length delta), so callers can remap
/// positions they hold instead of dropping them; None means no run
/// moved. `pass` is the pass still placing this layout, when there is
/// one: the run counts it holds for an open code block follow the
/// splice, or the block's drop would cut into the lines above it.
pub fn recolor_batch(
    lay: &mut LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    patches: &[(usize, Range<usize>)],
    pass: Option<&mut LayoutPass>,
) -> Option<(usize, usize, isize)> {
    // Records sort by (block, line) and their run ranges rise with it,
    // so the affected list visits the run vector strictly left to right.
    let mut affected: Vec<usize> = Vec::new();
    for (block, lines) in patches {
        let lo = lay
            .code_lines
            .partition_point(|c| (c.block, c.line) < (*block, lines.start));
        let hi = lay
            .code_lines
            .partition_point(|c| (c.block, c.line) < (*block, lines.end));
        affected.extend(lo..hi);
    }
    if affected.is_empty() {
        return None;
    }
    affected.sort_unstable();
    affected.dedup();

    let first = affected[0];
    let last = *affected.last().expect("affected is not empty");
    let run_lo = lay.code_lines[first].runs.start;
    let run_hi = lay.code_lines[last].runs.end;
    let mut rebuilt: Vec<TextRun> = Vec::with_capacity(run_hi - run_lo);
    let mut copied = run_lo;
    let mut scratch = LayoutDoc::default();
    let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
    let mut next = affected.iter().peekable();
    for index in first..lay.code_lines.len() {
        if next.peek() != Some(&&index) {
            let delta = (run_lo + rebuilt.len()) as isize - copied as isize;
            let record = &mut lay.code_lines[index];
            record.runs = record.runs.start.wrapping_add_signed(delta)
                ..record.runs.end.wrapping_add_signed(delta);
            continue;
        }
        next.next();
        let record = lay.code_lines[index].clone();
        rebuilt.extend_from_slice(&lay.runs[copied..record.runs.start]);
        copied = record.runs.start;
        let from = run_lo + rebuilt.len();
        let source = doc.blocks.get(record.block).and_then(|b| match &b.kind {
            BlockKind::CodeBlock {
                lines, highlights, ..
            } => Some((lines, highlights)),
            _ => None,
        });
        let reshaped = source.and_then(|(source_lines, highlights)| {
            if record.line >= source_lines.len() {
                return None;
            }
            let line = source_lines.line(&doc.source, record.line);
            let segments = highlights.get(record.line).unwrap_or(&empty);
            shape_code_line(
                fonts,
                theme,
                cfg,
                line,
                record.line,
                segments,
                record.block,
                record.x0,
                record.y0,
                record.size,
                record.line_height,
                record.wrap_width,
                doc.plain_file,
                &mut scratch,
            );
            Some(())
        });
        if reshaped.is_some() {
            // The scratch interned its families locally; the rebuilt
            // runs join the layout's table before they join its vector.
            let family_map: Vec<u16> = scratch
                .families
                .iter()
                .map(|name| lay.family_id(name))
                .collect();
            for run in &mut scratch.runs {
                run.family = family_map[run.family as usize];
            }
            scratch.families.clear();
            rebuilt.append(&mut scratch.runs);
        } else {
            rebuilt.extend_from_slice(&lay.runs[copied..record.runs.end]);
        }
        copied = record.runs.end;
        lay.code_lines[index].runs = from..run_lo + rebuilt.len();
    }
    rebuilt.extend_from_slice(&lay.runs[copied..run_hi]);
    let delta = (run_lo + rebuilt.len()) as isize - run_hi as isize;
    // One splice, one tail move, whatever the drain holds.
    lay.runs.splice(run_lo..run_hi, rebuilt);
    // The rebuild moves every later run index, which the bucket spans
    // hold; queries fall back to the linear tail until reindexed.
    lay.index.runs.clear();
    // Window marks follow the moved run ends. A mark inside the rebuilt
    // span belongs to a code position whose records were just updated;
    // its end is its last record's.
    if let Some(window) = lay.window.as_mut() {
        for mark in window.marks.iter_mut() {
            if mark.runs <= run_lo {
                continue;
            }
            if mark.runs >= run_hi {
                mark.runs = mark.runs.wrapping_add_signed(delta);
            } else {
                mark.runs = lay.code_lines[mark.code - 1].runs.end;
            }
        }
    }
    if let Some(pass) = pass {
        pass.follow_recolor(lay, run_lo, run_hi, delta);
    }
    Some((run_lo, run_hi, delta))
}

/// Splices an edit into a placed code block, the keystroke fast path:
/// the touched lines shape once to learn their heights, the block's
/// tall exceptions and every height shift arithmetically, and the
/// window clears so the standing per-frame refill re-materializes the
/// band from the corrected table. Nothing else in the document
/// re-measures. The document must already carry the edit. Answers
/// false when the layout is not windowed or the block is not placed as
/// a code entry, the caller's cue to restart the pass instead.
#[allow(clippy::too_many_arguments)]
pub fn edit_code_lines(
    lay: &mut LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    block: usize,
    old_lines: Range<usize>,
    new_lines: Range<usize>,
) -> bool {
    if lay.window.is_none() {
        return false;
    }
    let position = match lay.table.position_of_block.get(block) {
        Some(&p) if p != u32::MAX => p as usize,
        _ => return false,
    };
    let entry = &lay.table.entries[position];
    if entry.flags & ENTRY_CODE == 0 {
        return false;
    }
    let (line_height, pad) = (entry.line_height, entry.pad);
    let Some(BlockKind::CodeBlock {
        lines, highlights, ..
    }) = doc.blocks.get(block).map(|b| &b.kind)
    else {
        return false;
    };

    // What the touched lines measured before the edit: the shared
    // advance plus their tall exceptions.
    let mut old_h = old_lines.len() as f32 * line_height;
    let tall = lay.table.tall_of(position);
    let first = tall.partition_point(|&(_, l, _)| (l as usize) < old_lines.start);
    for &(_, l, h) in &tall[first..] {
        if l as usize >= old_lines.end {
            break;
        }
        old_h += h - line_height;
    }

    // Shape the new touched lines once, for their heights alone; the
    // window refill re-shapes the band from the corrected table.
    let (x_base, avail) = block_geometry(
        &doc.blocks[block],
        lay.table.margin,
        lay.table.content_width,
        cfg,
        &doc.source,
    );
    let x0 = x_base + pad;
    let wrap_width = (avail - 2.0 * pad).max(40.0);
    let size = code_face(doc.plain_file, cfg).1 * cfg.zoom;
    let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
    let mut scratch = LayoutDoc::default();
    let mut new_h = 0.0_f32;
    let mut new_tall: Vec<(u32, u32, f32)> = Vec::new();
    for line in new_lines.clone() {
        let text = lines.line(&doc.source, line);
        let advance = if text.is_empty() {
            line_height
        } else {
            scratch.runs.clear();
            scratch.code_lines.clear();
            let segments = highlights.get(line).unwrap_or(&empty);
            shape_code_line_step(
                fonts,
                theme,
                cfg,
                text,
                segments,
                block,
                line,
                x0,
                size,
                line_height,
                wrap_width,
                doc.plain_file,
                &mut scratch,
            )
        };
        if advance != line_height {
            new_tall.push((position as u32, line as u32, advance));
        }
        new_h += advance;
    }
    let dy = new_h - old_h;
    let line_delta = new_lines.len() as isize - old_lines.len() as isize;

    // The tall exceptions: the touched range's entries swap for the
    // measured ones, the position's later lines re-index.
    let tall = &mut lay.table.tall;
    let p = position as u32;
    let lo =
        tall.partition_point(|&(tp, tl, _)| tp < p || (tp == p && (tl as usize) < old_lines.start));
    let hi =
        tall.partition_point(|&(tp, tl, _)| tp < p || (tp == p && (tl as usize) < old_lines.end));
    let pos_end = tall.partition_point(|&(tp, _, _)| tp <= p);
    for t in &mut tall[hi..pos_end] {
        t.1 = t.1.wrapping_add_signed(line_delta as i32);
    }
    let added = new_tall.len();
    tall.splice(lo..hi, new_tall);
    lay.table
        .tall_extra
        .splice(lo..hi, std::iter::repeat_n(0.0, added));
    lay.table.resum_tall(lo);
    debug_assert!(lay.table.tall_sums_hold());

    // The heights follow arithmetically: this entry grows, everything
    // recorded below it slides.
    lay.table.entries[position].height += dy;
    for entry in &mut lay.table.entries[position + 1..] {
        entry.y += dy;
        entry.content_y += dy;
        if !entry.deco_top.is_nan() {
            entry.deco_top += dy;
        }
    }
    if let Some((rule_pos, y)) = &mut lay.table.notes_rule {
        if *rule_pos > position {
            *y += dy;
        }
    }
    lay.height += dy;
    lay.window_clear(position);
    true
}

/// Recolors the laid-out lines `lines` of code block `block`: the
/// one-patch batch.
#[allow(clippy::too_many_arguments)]
pub fn recolor_code_lines(
    lay: &mut LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    block: usize,
    lines: Range<usize>,
) -> Option<(usize, usize, isize)> {
    recolor_batch(lay, doc, theme, fonts, cfg, &[(block, lines)], None)
}

/// Shapes one code line into the scratch from zero, record included:
/// the unit place_code_line and a pool worker share.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_code_line_step(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    line: &str,
    segments: &[(Range<usize>, SyntaxRole)],
    block_index: usize,
    line_index: usize,
    x0: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
    prose: bool,
    scratch: &mut LayoutDoc,
) -> f32 {
    let advance = shape_code_line(
        fonts,
        theme,
        cfg,
        line,
        line_index,
        segments,
        block_index,
        x0,
        0.0,
        size,
        line_height,
        wrap_width,
        prose,
        scratch,
    );
    scratch.code_lines.push(CodeLine {
        block: block_index,
        line: line_index,
        runs: 0..scratch.runs.len(),
        x0,
        y0: 0.0,
        size,
        line_height,
        wrap_width,
    });
    advance
}

/// Shapes one code line: one row per source line, wrapping inside the
/// panel, colors from the highlight roles. A line longer than
/// `LONG_LINE` shapes in chunks, each its own buffer stacked under the
/// previous one, so the cost stays linear in the line's length; the
/// runs keep their byte offsets into the whole line, so selection,
/// copy, search and the editor see one line.
#[allow(clippy::too_many_arguments)]
fn shape_code_line(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    line: &str,
    line_index: usize,
    segments: &[(Range<usize>, SyntaxRole)],
    block_index: usize,
    x0: f32,
    y0: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
    prose: bool,
    out: &mut LayoutDoc,
) -> f32 {
    let whole_line = [(0..line.len(), SyntaxRole::Plain)];
    let segments: &[(Range<usize>, SyntaxRole)] = if segments.is_empty() {
        &whole_line
    } else {
        segments
    };
    if line.len() <= LONG_LINE {
        let height = shape_code_chunk(
            fonts,
            theme,
            cfg,
            line,
            0,
            line_index,
            segments,
            block_index,
            x0,
            y0,
            size,
            line_height,
            wrap_width,
            prose,
            out,
        );
        return height.max(line_height);
    }
    let mut height = 0.0_f32;
    for chunk in long_line_chunks(line) {
        let local = segments_within(segments, &chunk);
        height += shape_code_chunk(
            fonts,
            theme,
            cfg,
            &line[chunk.clone()],
            chunk.start,
            line_index,
            &local,
            block_index,
            x0,
            y0 + height,
            size,
            line_height,
            wrap_width,
            prose,
            out,
        );
    }
    height.max(line_height)
}

/// Cuts a long line into chunks of at most `LONG_LINE` bytes, each
/// ending on a character boundary and, when one sits in the last few
/// hundred bytes, after a space, so the cut reads as a wrap. A line
/// within the threshold is one chunk.
fn long_line_chunks(line: &str) -> Vec<Range<usize>> {
    const LOOK_BACK: usize = 256;
    let bytes = line.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0;
    while line.len() - start > LONG_LINE {
        let target = start + LONG_LINE;
        let floor = target - LOOK_BACK;
        let mut end = match bytes[floor..target].iter().rposition(|&b| b == b' ') {
            Some(space) => floor + space + 1,
            None => target,
        };
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(start..end);
        start = end;
    }
    chunks.push(start..line.len());
    chunks
}

/// The styled ranges overlapping `chunk`, clipped to it and rebased to
/// its start. The ranges arrive sorted and disjoint, as the
/// highlighter emits them.
fn segments_within(
    segments: &[(Range<usize>, SyntaxRole)],
    chunk: &Range<usize>,
) -> Vec<(Range<usize>, SyntaxRole)> {
    let first = segments.partition_point(|(range, _)| range.end <= chunk.start);
    segments[first..]
        .iter()
        .take_while(|(range, _)| range.start < chunk.end)
        .map(|(range, role)| {
            let lo = range.start.max(chunk.start) - chunk.start;
            let hi = range.end.min(chunk.end) - chunk.start;
            (lo..hi, *role)
        })
        .collect()
}

/// Shapes one chunk of a code line, `text` starting `base` bytes into
/// the line, its styled ranges relative to `text`; the runs' model
/// references count from the line's start.
#[allow(clippy::too_many_arguments)]
fn shape_code_chunk(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    text: &str,
    base: usize,
    line_index: usize,
    segments: &[(Range<usize>, SyntaxRole)],
    block_index: usize,
    x0: f32,
    y0: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
    prose: bool,
    out: &mut LayoutDoc,
) -> f32 {
    let face = code_face(prose, cfg).0;
    let bold_weight = fonts.weight_for(face, Weight::BOLD);
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, Some(wrap_width), None);
    let shaped_text = crate::style::fonts::shapable(text);
    let text = &*shaped_text;
    let rich: Vec<(&str, Attrs)> = segments
        .iter()
        .enumerate()
        .map(|(index, (range, role))| {
            let mut attrs = Attrs::new().family(Family::Name(face)).metadata(index);
            // Only a markdown source asks for a face: a monospace bold
            // and italic keep the regular advance, so the grid holds.
            let (bold, italic) = crate::style::highlight::role_face(*role);
            if bold {
                attrs = attrs.weight(bold_weight);
            }
            if italic {
                attrs = attrs.style(Style::Italic);
            }
            (&text[range.clone()], attrs)
        })
        .collect();
    let default_attrs = Attrs::new().family(Family::Name(face));
    buffer.set_rich_text(
        &mut fonts.font_system,
        rich,
        &default_attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    let mut height = 0.0_f32;
    for run in buffer.layout_runs() {
        height = height.max(run.line_top + line_height);
        let line_text = buffer.lines[run.line_i].text();
        let glyphs = trim_trailing_spaces(run.glyphs, line_text);
        for group in glyph_groups(glyphs, line_text, false, &[]) {
            let segment_index = group.meta;
            let (start_byte, end_byte) = (group.bytes.start, group.bytes.end);
            let role = segments[segment_index].1;
            // The rich pieces cover the chunk contiguously, so an offset
            // into their concatenation is an offset into the chunk.
            let family = out.family_id(face);
            // The face the run was shaped with has to travel with it:
            // paint rasterizes from these fields, not from the buffer.
            let (bold, slanted) = crate::style::highlight::role_face(role);
            out.runs.push(TextRun {
                text: TextRef::Model {
                    start: (base + start_byte) as u32,
                    len: (end_byte - start_byte) as u32,
                },
                x: x0 + group.x,
                y: y0 + run.line_top,
                baseline: y0 + run.line_y,
                width: group.width,
                size,
                family,
                weight: if bold {
                    bold_weight.0
                } else {
                    Weight::NORMAL.0
                },
                italic: slanted,
                color: if prose {
                    theme.text.body
                } else {
                    role_color(theme, role)
                },
                block: block_index,
                span: line_index,
            });
        }
    }
    height
}

/// The height the shaper gives a code line drawn in one plain range,
/// for the tests of the count.
#[cfg(test)]
pub(crate) fn shaped_code_line_height(
    fonts: &mut FontStore,
    line: &str,
    size: f32,
    line_height: f32,
    wrap_width: f32,
) -> f32 {
    let mut scratch = LayoutDoc::default();
    shape_code_line_step(
        fonts,
        &Theme::default_dark(),
        &ViewConfig::default(),
        line,
        &[],
        0,
        0,
        0.0,
        size,
        line_height,
        wrap_width,
        false,
        &mut scratch,
    )
}

pub(crate) fn role_color(theme: &Theme, role: SyntaxRole) -> Rgba {
    let s = &theme.syntax;
    match role {
        SyntaxRole::Keyword => s.keyword,
        SyntaxRole::String => s.string,
        SyntaxRole::Number => s.number,
        SyntaxRole::Function => s.function,
        SyntaxRole::Type => s.type_,
        SyntaxRole::Comment => s.comment,
        SyntaxRole::Operator => s.operator,
        SyntaxRole::Variable => s.variable,
        SyntaxRole::Punctuation => s.punctuation,
        SyntaxRole::Plain => theme.surface.foreground,
        // A diff is read by its green and red. The themes name no such
        // colors among the code ones, but every theme has a green, a
        // red and a blue chosen to read as text on the page: the tip,
        // the caution and the note alerts' own.
        SyntaxRole::Added => theme.alerts.tip,
        SyntaxRole::Removed => theme.alerts.caution,
        SyntaxRole::Range => theme.alerts.note,
        // A markdown source is drawn in the colors its own rendering
        // uses, so the file on screen and the page it becomes agree.
        // The rule's dashes and the quote's `>` are the exception: their
        // rendering is a line in a color chosen for a line, too faint
        // for text, so they take the punctuation color instead.
        SyntaxRole::Heading(level) => {
            let h = &theme.headings;
            match level {
                1 => h.h1,
                2 => h.h2,
                3 => h.h3,
                4 => h.h4,
                5 => h.h5,
                _ => h.h6,
            }
        }
        SyntaxRole::Bold => theme.text.bold,
        SyntaxRole::Italic => theme.text.italic,
        SyntaxRole::InlineCode => theme.text.inline_code,
        SyntaxRole::Link => theme.text.link,
        SyntaxRole::Quote | SyntaxRole::Rule => theme.syntax.punctuation,
    }
}

/// One run of consecutive glyphs sharing a metadata index on a shaped
/// line: the segment they answer to, their byte span inside the line,
/// and their x extent.
struct GlyphGroup {
    meta: usize,
    bytes: std::ops::Range<usize>,
    x: f32,
    width: f32,
}

/// Walks a shaped line grouping consecutive glyphs by metadata, the
/// one walk both the prose and the code paths consume. A group also
/// ends where the next glyph's bytes do not touch the previous one's
/// (`glyphs_touch`), so that its byte span, the smallest and the largest
/// byte of its glyphs, holds its glyphs and nothing else. A span marked
/// in `as_written` (inline code that stays one group) keeps such a gap,
/// so that paint draws its text as written, left to right, whatever the
/// line's direction; the caller leaves the mark off a span that the
/// space cut below splits, since a kept gap in one of its words would
/// span the bytes of the others.
/// With `cut_spaces`, a group also ends after each space run, since a
/// justified line must hold no stretched space inside a group: paint
/// re-shapes runs naturally, so every word paints at its own recorded
/// x, and the space stays with the word before it to keep byte ranges
/// contiguous.
fn glyph_groups<'a>(
    glyphs: &'a [cosmic_text::LayoutGlyph],
    line_text: &'a str,
    cut_spaces: bool,
    as_written: &'a [bool],
) -> impl Iterator<Item = GlyphGroup> + 'a {
    let mut g = 0;
    std::iter::from_fn(move || {
        if g >= glyphs.len() {
            return None;
        }
        let meta = glyphs[g].metadata;
        let keep_gaps = as_written.get(meta).copied().unwrap_or(false);
        let mut end = g + 1;
        while end < glyphs.len()
            && glyphs[end].metadata == meta
            && (keep_gaps || glyphs_touch(&glyphs[end - 1], &glyphs[end]))
            && !(cut_spaces
                && glyph_is_space(line_text, &glyphs[end - 1])
                && !glyph_is_space(line_text, &glyphs[end]))
        {
            end += 1;
        }
        let start = glyphs[g..end]
            .iter()
            .map(|gl| gl.start)
            .min()
            .expect("a glyph");
        let stop = glyphs[g..end]
            .iter()
            .map(|gl| gl.end)
            .max()
            .expect("a glyph");
        // Glyphs arrive in logical order; on an RTL line their x runs
        // right to left, so the extent comes from min and max, never
        // from the first and last of the walk.
        let left = glyphs[g..end]
            .iter()
            .map(|gl| gl.x)
            .fold(f32::MAX, f32::min);
        let right = glyphs[g..end]
            .iter()
            .map(|gl| gl.x + gl.w)
            .fold(f32::MIN, f32::max);
        let group = GlyphGroup {
            meta,
            bytes: start..stop,
            x: left,
            width: right - left,
        };
        g = end;
        Some(group)
    })
}

/// Whether two glyphs next to each other on the line come from bytes
/// that touch or overlap, in either direction, since the glyphs of a
/// right-to-left line arrive from the right. A gap means the bidi rules
/// placed one of them away from its neighbors in the text, as the final
/// period of a Latin run, which lands at the run's left end in a
/// right-to-left line, next to the run's first word.
fn glyphs_touch(a: &cosmic_text::LayoutGlyph, b: &cosmic_text::LayoutGlyph) -> bool {
    b.start <= a.end && b.end >= a.start
}

/// Whether a glyph draws only a stretchable space, U+0020 or U+00A0,
/// the characters justification widens.
fn glyph_is_space(line_text: &str, glyph: &cosmic_text::LayoutGlyph) -> bool {
    matches!(&line_text[glyph.start..glyph.end], " " | "\u{a0}")
}

/// Drops line-trailing whitespace glyphs so run widths match visible text.
fn trim_trailing_spaces<'a>(
    glyphs: &'a [cosmic_text::LayoutGlyph],
    line_text: &str,
) -> &'a [cosmic_text::LayoutGlyph] {
    let mut end = glyphs.len();
    while end > 0 {
        let g = &glyphs[end - 1];
        if line_text[g.start..g.end].trim().is_empty() {
            end -= 1;
        } else {
            break;
        }
    }
    &glyphs[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::{load, markdown};
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use std::path::PathBuf;

    /// A code position of `lines` lines where every other line wrapped
    /// into a row more, as a playlist of long lines does.
    fn table_of_wrapped_lines(lines: u32) -> BlockTable {
        let mut table = BlockTable::default();
        table.push(
            0,
            BlockEntry {
                block: 0,
                y: 0.0,
                content_y: 12.5,
                height: 0.0,
                deco_top: f32::NAN,
                line_height: 30.3,
                pad: 14.0,
                x0: 0.0,
                flags: ENTRY_CODE,
            },
        );
        for line in (0..lines).step_by(2) {
            let rows = 2 + (line / 2) % 3;
            table.push_tall(0, line, rows as f32 * 30.3);
        }
        table
    }

    /// The top of a line as the pass adds it up: the extra height of
    /// each wrapped line above, one by one from the first.
    fn top_by_walk(table: &BlockTable, line: usize) -> f32 {
        let entry = &table.entries[0];
        let mut extra = 0.0_f32;
        for &(_, l, height) in table.tall_of(0) {
            if l as usize >= line {
                break;
            }
            extra += height - entry.line_height;
        }
        code_line_y(entry.content_y + entry.pad, line, entry.line_height, extra)
    }

    #[test]
    fn a_line_top_is_the_walk_of_the_wrapped_lines_to_the_bit() {
        let table = table_of_wrapped_lines(20_000);
        for line in (0..20_001)
            .step_by(7)
            .chain([0, 1, 2, 19_998, 19_999, 20_000])
        {
            assert_eq!(
                table.code_line_top(0, line).to_bits(),
                top_by_walk(&table, line).to_bits(),
                "line {line}"
            );
        }
        assert!(table.tall_sums_hold());
    }

    #[test]
    fn a_line_top_far_down_does_not_walk_the_wrapped_lines_above() {
        let lines = 1_400_000;
        let table = table_of_wrapped_lines(lines);
        let start = std::time::Instant::now();
        let mut sum = 0.0_f64;
        for k in 0..2_000 {
            sum += table.code_line_top(0, lines as usize - 1 - k) as f64;
        }
        let spent = start.elapsed();
        assert!(sum > 0.0);
        // 2,000 lookups at the end of 700,000 wrapped lines: a walk is
        // seconds, a lookup microseconds.
        assert!(spent.as_millis() < 100, "2,000 tops took {spent:?}");
    }

    fn seg(text: &str, script: Script) -> (String, Script) {
        (text.to_string(), script)
    }

    /// The glyphs of a right-to-left line come in reading order, placed
    /// from the right: the width of a label is its whole line, not its
    /// last letter.
    #[test]
    fn a_right_to_left_label_measures_its_whole_width() {
        let mut fonts = FontStore::new();
        let cfg = ViewConfig::default();
        let word = side_text_width(&mut fonts, &cfg, "مقدمة", 16.0);
        let line = side_text_width(&mut fonts, &cfg, "مقدمة ابن خلدون", 16.0);
        assert!(
            line > 2.0 * word,
            "three words measure more than twice the first: {line} against {word}"
        );
        let mut out = LayoutDoc::default();
        let family = cfg.body_family.clone();
        let white = Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        let (runs, shaped) = shape_side_text(
            &mut fonts,
            &cfg,
            &family,
            "مقدمة ابن خلدون",
            16.0,
            white,
            &mut out,
        );
        assert_eq!(shaped, line, "the shaped runs give the same width");
        assert_eq!(runs[0].width, line);
    }

    /// A paragraph separator inside a paragraph's text splits the
    /// shaped line into two bidi paragraphs, whose directions the text
    /// library asserts equal: the page shapes through `shapable`.
    #[test]
    fn a_paragraph_separator_in_the_text_lays_out() {
        let doc = markdown::parse("abc\u{2029}مق and on\n");
        let lay = lay_of(&doc);
        assert!(lay.height > 0.0);
    }

    /// A next line character (U+0085) is a paragraph separator too, and
    /// the shaper drops it between two of its lines, which puts every
    /// offset after it one byte off: the page shapes through
    /// `shapable`, so each run's text is still readable.
    #[test]
    fn a_next_line_character_in_the_text_lays_out() {
        let doc = markdown::parse("abc\u{85}مق and on\n");
        let lay = lay_of(&doc);
        assert!(lay.height > 0.0);
        for run in &lay.runs {
            assert!(!lay.run_text(&doc, run).is_empty());
        }
    }

    /// The label of an image shown without its picture is shaped like
    /// the text: a separator between two directions does not end the
    /// layout, and a control character measures as the space it is
    /// shown as, not as a glyph searched through every font.
    #[test]
    fn an_image_label_is_shaped_through_shapable() {
        for label in ["abc\u{2029}مق", "abc\u{85}مق"] {
            let doc = markdown::parse(format!("See ![{label}](nothing.png) here.\n"));
            let lay = lay_of(&doc);
            assert!(lay.height > 0.0);
        }
        let mut fonts = FontStore::new();
        let cfg = ViewConfig::default();
        let with_escape = side_text_width(&mut fonts, &cfg, "x\u{1b}y", 15.0);
        let with_space = side_text_width(&mut fonts, &cfg, "x y", 15.0);
        assert!(
            (with_escape - with_space).abs() < 0.01,
            "the escape measures as a space: {with_escape} against {with_space}"
        );
    }

    fn lay_of(doc: &Document) -> LayoutDoc {
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        layout(
            doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            800.0,
        )
    }

    fn run_y(l: &LayoutDoc, doc: &Document, text: &str) -> f32 {
        l.runs
            .iter()
            .find(|r| l.run_text(doc, r) == text)
            .unwrap_or_else(|| panic!("no run shows {text:?}"))
            .y
    }

    #[test]
    fn an_abbreviation_draws_dots_and_answers_at_the_cursor() {
        let doc = markdown::parse("The HTML spec.\n\n*[HTML]: Hyper Text Markup Language\n");
        let l = lay_of(&doc);
        let word = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == "HTML")
            .expect("the word placed");
        let dots: Vec<&DecoRect> = l
            .rects
            .iter()
            .filter(|r| {
                r.x >= word.x - 0.5
                    && r.x + r.width <= word.x + word.width + 0.5
                    && r.y > word.y
                    && (r.width - r.height).abs() < 0.01
                    && r.color == word.color
            })
            .collect();
        assert!(
            dots.len() >= 4,
            "a dotted line under the word, {} dots",
            dots.len()
        );
        let gap = dots[1].x - dots[0].x;
        assert!(gap > dots[0].width, "dots stand apart");
        let mid_y = word.y + metrics::LINE_HEIGHT * word.size / 2.0;
        assert_eq!(
            l.abbr_at(&doc, word.x + word.width / 2.0, mid_y),
            Some("Hyper Text Markup Language")
        );
        let plain = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r).starts_with("The"))
            .expect("the text placed");
        assert_eq!(l.abbr_at(&doc, plain.x + 2.0, mid_y), None);
        assert!(
            l.rects.iter().all(|r| r.width > 0.01 || r.height > 0.01),
            "no empty rect"
        );
    }

    #[test]
    fn the_foot_orders_notes_by_number_and_continues_under_one_marker() {
        let doc = markdown::parse(
            "A[^b] B[^a]\n\n[^a]: first defined.\n\n[^b]: second defined.\n\n    more of b.\n",
        );
        let l = lay_of(&doc);
        let first = run_y(&l, &doc, "first defined.");
        let second = run_y(&l, &doc, "second defined.");
        assert!(second < first, "b is number 1 and comes first at the foot");
        let markers: Vec<&TextRun> = l
            .runs
            .iter()
            .filter(|r| matches!(l.run_text(&doc, r), "1." | "2."))
            .collect();
        assert_eq!(
            markers.len(),
            2,
            "one marker per note, none on a continued block"
        );
        let head = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == "second defined.")
            .unwrap();
        let more = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == "more of b.")
            .unwrap();
        assert!(
            (head.x - more.x).abs() < 0.01,
            "the continued block takes the same indent"
        );
        assert!(more.y > head.y);
        assert_eq!(
            l.anchors.iter().filter(|(a, _)| a == "footnote:b").count(),
            1,
            "one anchor per note"
        );
    }

    #[test]
    fn a_highlight_over_several_words_is_one_pill() {
        let doc =
            markdown::parse("Some ==highlighted with equal signs== here and more words to fill.\n");
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let theme = Theme::default_dark();
        for justify in [false, true] {
            let cfg = ViewConfig {
                justify,
                ..ViewConfig::default()
            };
            let l = layout(&doc, &theme, &mut fonts, &mut media, &cfg, 800.0);
            let pills = l
                .rects
                .iter()
                .filter(|r| r.color == theme.ui.search_match_bg)
                .count();
            assert_eq!(pills, 1, "one pill under the phrase, justify {justify}");
        }
    }

    #[test]
    fn a_footnote_number_at_the_foot_links_back_to_its_reference() {
        let doc = markdown::parse("Alpha[^n] beta.\n\nGamma.\n\n[^n]: The note.\n");
        let l = lay_of(&doc);
        let reference = l
            .runs
            .iter()
            .find(|r| l.run_link(&doc, r) == Some("footnote:n"))
            .expect("the reference run");
        let back = l.anchor_y("fnref:n").expect("an anchor at the reference");
        assert!(
            (back - reference.y).abs() < reference.size,
            "the anchor sits on the reference's line: {back} against {}",
            reference.y
        );
        let marker = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == "1.")
            .expect("the marker run");
        assert_eq!(l.run_link(&doc, marker), Some("fnref:n"));
    }

    /// A reference inside a table cell shapes into the cell's own
    /// scratch layout; its back anchor must reach the page's list with
    /// the cell's offset, like the cell's runs do.
    #[test]
    fn a_footnote_cited_in_a_table_cell_has_its_back_anchor() {
        let doc = markdown::parse("| a | b |\n|---|---|\n| one[^n] | two |\n\n[^n]: The note.\n");
        let l = lay_of(&doc);
        let reference = l
            .runs
            .iter()
            .find(|r| l.run_link(&doc, r) == Some("footnote:n"))
            .expect("the reference run in the cell");
        let back = l
            .anchor_y("fnref:n")
            .expect("an anchor at the cell's reference");
        assert!(
            (back - reference.y).abs() < reference.size,
            "the anchor sits on the cell's line: {back} against {}",
            reference.y
        );
    }

    #[test]
    fn a_plain_file_shows_blank_lines_as_rows() {
        let doc = load::plain_document("alpha\nbeta\n");
        let l = lay_of(&doc);
        let one = run_y(&l, &doc, "beta") - run_y(&l, &doc, "alpha");
        let doc = load::plain_document("alpha\n\n\nbeta\n");
        let l = lay_of(&doc);
        let three = run_y(&l, &doc, "beta") - run_y(&l, &doc, "alpha");
        assert!(
            (three - 3.0 * one).abs() < 0.5,
            "two blank lines are two rows: single advance {one}, with blanks {three}"
        );
    }

    #[test]
    fn a_text_file_draws_prose_lines_in_the_body_face() {
        let doc = load::text_document("alpha\n\nbeta\n");
        let l = lay_of(&doc);
        let cfg = ViewConfig::default();
        let alpha = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == "alpha")
            .expect("alpha placed");
        assert_eq!(
            l.run_family(alpha),
            cfg.body_family,
            "prose lines keep the body face"
        );
        assert_eq!(alpha.size, cfg.body_size, "prose lines keep the body size");
        let blocks = &Theme::default_dark().blocks;
        assert!(
            l.rects
                .iter()
                .all(|r| r.color != blocks.code_bg && r.color != blocks.code_border),
            "a text file carries no panel"
        );
        let advance = run_y(&l, &doc, "beta") - run_y(&l, &doc, "alpha");
        let line = metrics::LINE_HEIGHT * cfg.body_size;
        assert!(
            (advance - 2.0 * line).abs() < 0.5,
            "the blank line is one row: {advance} vs {line}"
        );
    }

    #[test]
    fn a_whole_file_code_document_draws_frameless_at_the_page_margin() {
        let doc = load::code_document(Some("rust"), "fn main() {}\nlet x = 1;\n");
        let lay = lay_of(&doc);
        let blocks = &Theme::default_dark().blocks;
        assert!(
            lay.rects
                .iter()
                .all(|r| r.color != blocks.code_bg && r.color != blocks.code_border),
            "a page that is code carries no panel"
        );
        let plain = load::plain_document("hello\n");
        let plain_lay = lay_of(&plain);
        assert_eq!(
            lay.runs[0].x, plain_lay.runs[0].x,
            "code lines start where body text starts"
        );
    }

    #[test]
    fn fenced_code_inside_markdown_keeps_its_panel() {
        let doc = markdown::parse("text\n\n```rust\nlet x = 1;\n```\n");
        let lay = lay_of(&doc);
        let blocks = &Theme::default_dark().blocks;
        assert!(
            lay.rects.iter().any(|r| r.color == blocks.code_bg),
            "embedded code keeps the panel"
        );
    }

    #[test]
    fn math_scripts_split_sup_and_sub() {
        assert_eq!(
            math_scripts("x^2"),
            [seg("x", Script::Normal), seg("2", Script::Sup)]
        );
        assert_eq!(
            math_scripts("a_i"),
            [seg("a", Script::Normal), seg("i", Script::Sub)]
        );
        assert_eq!(
            math_scripts("x^{10}+y"),
            [
                seg("x", Script::Normal),
                seg("10", Script::Sup),
                seg("+y", Script::Normal)
            ]
        );
        assert_eq!(
            math_scripts("E=mc^2"),
            [seg("E=mc", Script::Normal), seg("2", Script::Sup)]
        );
    }

    #[test]
    fn tex_symbols_replace_known_commands() {
        assert_eq!(tex_symbols(r"\sum_{i=1}^{n}"), "\u{2211}_{i=1}^{n}");
        assert_eq!(tex_symbols(r"\alpha + \beta"), "\u{03B1} + \u{03B2}");
        assert_eq!(tex_symbols(r"\pi r^2"), "\u{03C0} r^2");
        assert_eq!(tex_symbols(r"x \to \infty"), "x \u{2192} \u{221E}");
        assert_eq!(tex_symbols(r"a \leq b \neq c"), "a \u{2264} b \u{2260} c");
    }

    #[test]
    fn tex_symbols_keep_unknown_commands_literal() {
        assert_eq!(tex_symbols(r"\foo{x}"), r"\foo{x}");
        assert_eq!(tex_symbols("no commands"), "no commands");
        assert_eq!(tex_symbols(r"trailing \"), r"trailing \");
    }

    /// An escaped reserved character shows its character in the text
    /// fallback, as noad shows it on the page (Task 280): the `_` of
    /// `x\_1` is a character, not a subscript.
    #[test]
    fn escaped_reserved_characters_show_their_character() {
        assert_eq!(tex_symbols(r"15\%"), "15%");
        assert_eq!(tex_symbols(r"a \# b \& c \$5 \{x\}"), "a # b & c $5 {x}");
        assert_eq!(math_display(r"x\_1"), "x_1");
        assert_eq!(math_display(r"15\%"), "15%");
        assert_eq!(math_scripts(r"x\_1"), [seg("x_1", Script::Normal)]);
        assert_eq!(
            math_scripts(r"a_\%"),
            [seg("a", Script::Normal), seg("%", Script::Sub)],
            "an escaped character as a script's argument"
        );
        assert_eq!(
            math_scripts(r"\sum_{i=1}^{n}"),
            [
                seg("\u{2211}", Script::Normal),
                seg("i=1", Script::Sub),
                seg("n", Script::Sup)
            ]
        );
    }

    /// A control word as a bare script's argument (`e^\pi`,
    /// `\int_0^\infty`) is the whole word, as in TeX, where a control
    /// sequence is one token (claim 10 of the hunt of Task 328). Before,
    /// the script took two characters and cut the word after its first
    /// letter.
    #[test]
    fn a_control_word_as_a_script_is_taken_whole() {
        assert_eq!(
            math_scripts(r"x^\alpha"),
            [seg("x", Script::Normal), seg("\u{03B1}", Script::Sup)]
        );
        assert_eq!(math_display(r"e^\pi"), "e\u{03C0}");
        assert_eq!(
            math_scripts(r"\int_0^\infty e^{-x}"),
            [
                seg("\u{222B}", Script::Normal),
                seg("0", Script::Sub),
                seg("\u{221E}", Script::Sup),
                seg(" e", Script::Normal),
                seg("-x", Script::Sup)
            ]
        );
        assert_eq!(
            math_scripts(r"e^\pi r"),
            [
                seg("e", Script::Normal),
                seg("\u{03C0}", Script::Sup),
                seg(" r", Script::Normal)
            ],
            "the word ends at the first character that is not a letter"
        );
        assert_eq!(
            math_scripts(r"x^\foo+1"),
            [
                seg("x", Script::Normal),
                seg(r"\foo", Script::Sup),
                seg("+1", Script::Normal)
            ],
            "an unknown word stays literal, whole"
        );
        assert_eq!(
            math_scripts(r"a^\"),
            [seg(r"a^\", Script::Normal)],
            "a backslash with nothing after it stays literal"
        );
    }

    #[test]
    fn math_scripts_keep_unmatched_literal() {
        assert_eq!(math_scripts("plain"), [seg("plain", Script::Normal)]);
        assert_eq!(math_scripts("a^"), [seg("a^", Script::Normal)]);
        assert_eq!(math_scripts("a^{open"), [seg("a^{open", Script::Normal)]);
        assert_eq!(math_scripts(""), Vec::<(String, Script)>::new());
    }

    #[test]
    fn a_heading_and_a_bold_word_in_a_one_face_body_font_keep_it_at_its_own_weight() {
        use crate::style::fonts::MATH_FAMILY;
        let doc = markdown::parse("# Title\n\nSome **bold** text.\n");
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let cfg = ViewConfig {
            body_family: MATH_FAMILY.to_string(),
            ..ViewConfig::default()
        };
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &cfg,
            800.0,
        );
        for text in ["Title", "bold"] {
            let run = l
                .runs
                .iter()
                .find(|r| l.run_text(&doc, r) == text)
                .unwrap_or_else(|| panic!("no run shows {text:?}"));
            assert_eq!(l.run_family(run), MATH_FAMILY);
            assert_eq!(
                run.weight,
                Weight::NORMAL.0,
                "{text} asks for the weight the family has"
            );
        }
    }

    #[test]
    fn rule_and_quote_markers_take_the_punctuation_color() {
        let theme = Theme::default_dark();
        assert_eq!(
            role_color(&theme, SyntaxRole::Rule),
            theme.syntax.punctuation
        );
        assert_eq!(
            role_color(&theme, SyntaxRole::Quote),
            theme.syntax.punctuation
        );
    }

    #[test]
    fn a_page_break_draws_a_dashed_line_on_screen_only() {
        let doc = markdown::parse("one\n\n\\newpage\n\ntwo\n");
        let l = lay_of(&doc);
        assert!(
            l.rects.len() >= 8,
            "a dashed line is a row of short rects, got {}",
            l.rects.len()
        );
        let y = l.rects[0].y;
        assert!(l.rects.iter().all(|r| (r.y - y).abs() < 0.01), "one row");
        assert!(
            l.rects.iter().all(|r| r.height >= 1.0 && r.width < 20.0),
            "short dashes"
        );
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let cfg = ViewConfig {
            print: true,
            ..ViewConfig::default()
        };
        let printed = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &cfg,
            800.0,
        );
        assert!(printed.rects.is_empty(), "the export draws no dashes");
        assert_eq!(printed.runs.len(), l.runs.len(), "the text is the same");
    }
}
