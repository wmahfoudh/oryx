//! The layout engine: document model in, positioned runs and rects out.
//! Pure with respect to the window: no pixels, fully testable with numbers.

use std::ops::Range;
use std::time::{Duration, Instant};

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use super::metrics;
use crate::doc::images::MediaCache;
use crate::doc::model::{
    AlertKind, Block, BlockKind, Document, Marker, Span, SpanImage, SpanScript,
};
use crate::layout::pool::{Job, ShapeCtx, StepKey, Work};
use crate::style::fonts::{FontStore, BODY_FAMILY, CODE_FAMILY};
use crate::style::highlight::SyntaxRole;
use crate::style::theme::{Rgba, Theme};

#[derive(Debug, Clone)]
pub struct ViewConfig {
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    /// Session zoom multiplier, never persisted.
    pub zoom: f32,
}

impl Default for ViewConfig {
    fn default() -> ViewConfig {
        ViewConfig {
            body_family: BODY_FAMILY.to_string(),
            code_family: CODE_FAMILY.to_string(),
            body_size: 22.0,
            code_size: 20.0,
            zoom: 1.0,
        }
    }
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
        BlockKind::Table { header, rows } => header
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
    /// Row bands of every table, in document order. Pagination needs
    /// them because a row is several lines that must not be split.
    pub table_rows: Vec<TableRow>,
    /// Per-line records for code blocks, ordered by block then line;
    /// `recolor_code_lines` re-shapes through them.
    pub code_lines: Vec<CodeLine>,
    /// The resolved families runs shaped with; runs carry ids.
    pub families: Vec<String>,
    /// Synthesized display text with no model home: markers, alert
    /// titles, frontmatter lines, expanded math, placeholder alts.
    /// Append-only; side references slice it.
    pub side: String,
    /// Every placed block's recorded position, whether or not its
    /// geometry is retained.
    table: BlockTable,
    /// Materialized-window bookkeeping while retention is bounded.
    window: Option<WindowState>,
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

    /// This position's slice of the tall-line exceptions.
    fn tall_of(&self, position: usize) -> &[(u32, u32, f32)] {
        let position = position as u32;
        let lo = self.tall.partition_point(|&(p, _, _)| p < position);
        let hi = self.tall.partition_point(|&(p, _, _)| p <= position);
        &self.tall[lo..hi]
    }

    /// The recorded top of line `line`, reproducing the placement
    /// arithmetic bit for bit: the shared advance in closed form, plus
    /// the wrapped lines' extra heights accumulated in order.
    fn code_line_top(&self, position: usize, line: usize) -> f32 {
        let entry = &self.entries[position];
        let mut extra = 0.0_f32;
        for &(_, l, height) in self.tall_of(position) {
            if l as usize >= line {
                break;
            }
            extra += height - entry.line_height;
        }
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

    /// The resolved family a run shaped with.
    pub fn run_family<'a>(&'a self, run: &'a TextRun) -> &'a str {
        &self.families[run.family as usize]
    }

    /// A run's link target, resolved through the model span the run
    /// indexes; synthesized runs carry none.
    pub fn run_link<'a>(&'a self, doc: &'a Document, run: &'a TextRun) -> Option<&'a str> {
        model_span(doc, run.block, run.span)?.link.as_deref()
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
        scratch.height = 0.0;
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
    pub color: crate::style::theme::Rgba,
    pub block: usize,
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
            // Conservative extent around the baseline; buckets tolerate slack.
            index.math.note(i, g.y - 1.2 * g.size, g.y + 0.6 * g.size);
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

    /// Bounds retention around a scroll position: blocks outside the
    /// band and its margin are measured into the table and dropped.
    pub fn retain_around(&mut self, scroll: f32, viewport_h: f32) {
        self.retain = Some((scroll, viewport_h));
    }

    /// Shifts the indices the pass holds into the layout's vectors
    /// after a consumer drained emitted geometry from their front. The
    /// open code block's panel is never part of what such a consumer
    /// may drop.
    pub(crate) fn rebase(&mut self, rects: usize) {
        if let Some(open) = &mut self.open {
            open.panel -= rects;
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
/// range, meaningful for code blocks.
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
        return;
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
        self.positions.is_empty() && self.extend.as_ref().map_or(true, |(_, l)| l.is_empty())
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
    if lay.window.is_none() || lay.table.entries.is_empty() {
        return false;
    }
    // Elements beyond the marks belong to the pass's open code block;
    // moving them would break the indices the pass holds, so the slide
    // waits for the block to close.
    {
        let window = lay.window.as_ref().expect("windowed layout");
        let ends = window.marks.back().cloned().unwrap_or_else(PosMarks::zero);
        if lay.runs.len() > ends.runs
            || lay.rects.len() > ends.rects
            || lay.images.len() > ends.images
            || lay.math_glyphs.len() > ends.math
            || lay.table_rows.len() > ends.rows
            || lay.code_lines.len() > ends.code
        {
            return false;
        }
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
}

/// What a block derives on entry and needs again when it finishes.
#[derive(Clone, Copy)]
struct Frame {
    marks: Marks,
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
    /// Panel background rect; the border rect follows it.
    panel: usize,
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
    let margin = metrics::MARGIN_RATIO * viewport_width;
    let vertical_margin = metrics::VERTICAL_MARGIN_EM * cfg.body_size * cfg.zoom;
    let content_width = (viewport_width - 2.0 * margin).max(50.0);

    // Footnote definitions collect at the document end under a rule,
    // wherever the source declared them. Model indices stay untouched so
    // selection and copy keep their mapping. Blocks folded inside a
    // closed details group never enter the order; a toggle restarts the
    // pass, so visibility is fixed for its lifetime.
    let (body_order, note_order): (Vec<usize>, Vec<usize>) = (0..doc.blocks.len())
        .filter(|&i| doc.block_visible(i))
        .partition(|&i| !matches!(doc.blocks[i].kind, BlockKind::FootnoteDef { .. }));
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
    };
    let mut out = LayoutDoc::default();
    out.table.margin = margin;
    out.table.content_width = content_width;
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
    let (body, notes): (Vec<usize>, Vec<usize>) = (covered..doc.blocks.len())
        .filter(|&i| doc.block_visible(i))
        .partition(|&i| !matches!(doc.blocks[i].kind, BlockKind::FootnoteDef { .. }));
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
    while pool.backlog() < window && pass.seed_position < pass.order.len() {
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
            // Mirrors open_code: the pad and wrap width the assembler
            // will place the line against.
            let (x_base, avail) = block_geometry(block, pass.margin, pass.content_width, &ctx.cfg);
            let size = ctx.cfg.code_size * ctx.cfg.zoom;
            let pad = metrics::CODE_PAD * ctx.cfg.zoom;
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
                },
            });
        } else {
            let position = pass.seed_position;
            pass.seed_position += 1;
            pass.seed_line = 0;
            if !poolable(block) {
                continue;
            }
            let Some((heading, _, base_size)) = block_metrics(block, &ctx.cfg) else {
                continue;
            };
            let (x_base, avail) = block_geometry(block, pass.margin, pass.content_width, &ctx.cfg);
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
fn block_metrics(block: &Block, cfg: &ViewConfig) -> Option<(Option<u8>, bool, f32)> {
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
        BlockKind::CodeBlock { .. } => cfg.code_size * cfg.zoom,
        BlockKind::Rule
        | BlockKind::Table { .. }
        | BlockKind::Image { .. }
        | BlockKind::MathBlock { .. }
        | BlockKind::Frontmatter { .. } => cfg.body_size * cfg.zoom,
        BlockKind::FootnoteDef { .. } => 0.85 * cfg.body_size * cfg.zoom,
    };
    Some((heading, is_list, base_size))
}

/// The x origin and available width a block shapes against, derived from
/// its quote depth and the pass geometry alone, so seeding and assembly
/// agree by construction.
fn block_geometry(block: &Block, margin: f32, content_width: f32, cfg: &ViewConfig) -> (f32, f32) {
    let quote_indent = block.quote_depth as f32 * metrics::INDENT * cfg.zoom;
    let quote_pad = if block.quote_depth > 0 {
        12.0 * cfg.zoom
    } else {
        0.0
    };
    let x_base = margin + quote_indent + quote_pad;
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
    let Some((heading, is_list, base_size)) = block_metrics(block, cfg) else {
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

    let (x_base, avail) = block_geometry(block, pass.margin, pass.content_width, cfg);
    let marks = Marks {
        rects: out.rects.len(),
        runs: out.runs.len(),
        images: out.images.len(),
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

    let frame = Frame {
        marks,
        x_base,
        avail,
        gap,
        region_top,
        base_size,
        is_list,
    };

    let is_code = matches!(block.kind, BlockKind::CodeBlock { .. });
    let size = cfg.code_size * cfg.zoom;
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
            pad: if is_code {
                metrics::CODE_PAD * cfg.zoom
            } else {
                0.0
            },
            flags: if is_code { ENTRY_CODE } else { 0 }
                | if alert_start { ENTRY_ALERT_TITLE } else { 0 },
        },
    );

    // A code file is one block, so it is opened and its lines land
    // over as many steps as the slice budget allows.
    if is_code {
        pass.scratch = scratch;
        let open = open_code(theme, cfg, block_index, frame, counts, out, pass);
        place_code_line(doc, theme, fonts, cfg, open, out, pass);
        return;
    }
    if let BlockKind::FootnoteDef { label, .. } = &block.kind {
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
        finish_block(theme, cfg, block, frame, shaped.height, out, pass);
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
    finish_block(theme, cfg, block, frame, height, out, pass);
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
        BlockKind::Table { header, rows } => layout_table(
            fonts,
            theme,
            cfg,
            source,
            header,
            rows,
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
        BlockKind::FootnoteDef { label, spans } => layout_footnote_def(
            fonts,
            theme,
            cfg,
            source,
            label,
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
    }

    /// Drops emitted geometry from the vectors' front: the fused
    /// export's drain behind its pagination cursor. Code line records
    /// fully behind the run frontier go with it, the rest shift. The
    /// caller rebases everything of its own that indexes the vectors.
    pub(crate) fn drain_front(&mut self, runs: usize, rects: usize, images: usize, math: usize) {
        debug_assert!(self.window.is_none(), "the drain and the window exclude");
        let code = self.code_lines.partition_point(|c| c.runs.end <= runs);
        self.runs.drain(..runs);
        self.rects.drain(..rects);
        self.images.drain(..images);
        self.math_glyphs.drain(..math);
        self.code_lines.drain(..code);
        for record in &mut self.code_lines {
            record.runs = record.runs.start - runs..record.runs.end - runs;
        }
        self.index = YIndex::default();
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
        let (x_base, avail) = block_geometry(block, table.margin, table.content_width, &ctx.cfg);
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
                    size: ctx.cfg.code_size * ctx.cfg.zoom,
                    line_height: entry.line_height,
                    wrap_width: (avail - 2.0 * entry.pad).max(40.0),
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
        let Some((heading, _, base_size)) = block_metrics(block, &ctx.cfg) else {
            continue;
        };
        let (x_base, avail) = block_geometry(block, table.margin, table.content_width, &ctx.cfg);
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
        block_metrics(block, cfg).expect("a non-silent entry has metrics");
    let (x_base, avail) = block_geometry(block, table.margin, table.content_width, cfg);
    let run_mark = out.runs.len();
    let rect_mark = out.rects.len();
    let image_mark = out.images.len();
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
    let mut lines = 0..0;
    if entry.flags & ENTRY_CODE != 0 {
        for rect in code_panel_rects(theme, cfg, x_base, avail, entry.content_y, entry.height) {
            out.rects.push(rect);
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
    if block.centered {
        center_lines(out, run_mark, rect_mark, image_mark, x_base, avail);
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
    let (x_base, avail) = block_geometry(block, table.margin, table.content_width, cfg);
    let x0 = x_base + entry.pad;
    let wrap_width = (avail - 2.0 * entry.pad).max(40.0);
    let size = cfg.code_size * cfg.zoom;
    let base = entry.content_y + entry.pad;
    let tall = table.tall_of(position);
    let mut next_tall = 0;
    let mut extra = 0.0_f32;
    while next_tall < tall.len() && (tall[next_tall].1 as usize) < lines.start {
        extra += tall[next_tall].2 - entry.line_height;
        next_tall += 1;
    }
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
fn finish_block(
    theme: &Theme,
    cfg: &ViewConfig,
    block: &Block,
    frame: Frame,
    height: f32,
    out: &mut LayoutDoc,
    pass: &mut LayoutPass,
) {
    if block.centered {
        center_lines(
            out,
            frame.marks.runs,
            frame.marks.rects,
            frame.marks.images,
            frame.x_base,
            frame.avail,
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
        );
        out.rects
            .splice(frame.marks.rects..frame.marks.rects, decoration);
    }
    out.table.finish(height, deco_top);

    pass.cursor += height + metrics::space_below(frame.base_size);
    pass.prev_quote_depth = block.quote_depth;
    pass.prev_alert = block.alert;
    pass.prev_is_list = frame.is_list;
    pass.prev_space_below = metrics::space_below(frame.base_size);
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
fn quote_decoration(
    theme: &Theme,
    cfg: &ViewConfig,
    block: &Block,
    margin: f32,
    content_width: f32,
    top: f32,
    bottom: f32,
) -> Vec<DecoRect> {
    let panel_h = bottom - top;
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
        decoration.push(DecoRect::fill(
            margin + level as f32 * metrics::INDENT * cfg.zoom,
            top,
            3.0 * cfg.zoom,
            panel_h,
            bar,
        ));
    }
    decoration
}

/// Opens a code block: the panel and border take their final index with a
/// provisional height, so later lines only grow them.
fn open_code(
    theme: &Theme,
    cfg: &ViewConfig,
    block_index: usize,
    frame: Frame,
    counts: ElementCounts,
    out: &mut LayoutDoc,
    pass: &LayoutPass,
) -> OpenCode {
    let size = cfg.code_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let pad = metrics::CODE_PAD * cfg.zoom;
    // Long lines wrap inside the panel instead of overflowing it, so the
    // panel height follows the shaped lines.
    let wrap_width = (frame.avail - 2.0 * pad).max(40.0);
    let y0 = pass.cursor;
    let panel = out.rects.len();
    let height = 2.0 * pad;
    for rect in code_panel_rects(theme, cfg, frame.x_base, frame.avail, y0, height) {
        out.rects.push(rect);
    }
    OpenCode {
        block: block_index,
        // place_block advanced past this block one line above.
        position: pass.position - 1,
        frame,
        panel,
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
        finish_block(theme, cfg, block, open.frame, height, out, pass);
        let kept = open.kept.unwrap_or(0..0);
        settle_retention(out, pass, &open.counts, kept);
        return;
    }

    let top = open.line_top();
    let line = lines.line(&doc.source, open.line);
    let runs_mark = out.runs.len();
    let code_mark = out.code_lines.len();
    let mut advance = open.line_height;
    if !line.is_empty() {
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
                .tall
                .push((open.position as u32, open.line as u32, advance));
        }
    }
    // A line outside the retention bound was measured and drops; one
    // that would leave a gap in the kept segment drops too, and the
    // slide materializes it later.
    if let Some((scroll, viewport_h)) = pass.retain {
        let range = retain_range(scroll, viewport_h);
        let inside = top <= range.end && top + advance >= range.start;
        let contiguous = open
            .kept
            .as_ref()
            .map_or(true, |kept| kept.end == open.line);
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
        }
    }
    if advance != open.line_height {
        open.extra += advance - open.line_height;
    }
    open.line += 1;

    let height = open.line_top() - open.y0 + open.pad;
    out.rects[open.panel].height = height;
    out.rects[open.panel + 1].height = height;
    pass.open = Some(open);
}

struct BlockStyle {
    size: f32,
    color: Rgba,
    bold: bool,
    block_index: usize,
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
    color: Rgba,
    /// Background pill color for inline code and mark highlights.
    pill: Option<Rgba>,
    /// Baseline shift: positive raises (superscripts), negative lowers.
    rise: f32,
}

fn span_style(theme: &Theme, cfg: &ViewConfig, base: &BlockStyle, span: &Span) -> SpanStyle {
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
    SpanStyle {
        family: if code || span.math {
            cfg.code_family.clone()
        } else {
            cfg.body_family.clone()
        },
        size,
        weight: if base.bold || span.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        },
        italic: span.italic || span.math,
        strike: span.strike,
        underline: span.underline,
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
    let line_height = metrics::LINE_HEIGHT * base.size;
    // Math literals expand into script segments at this point only; the
    // model keeps the raw TeX. Each expanded piece remembers its model
    // span so selection and copy still map to the source, but its text
    // is synthesized and lands in the side buffer.
    let mut shaped: Vec<Span> = Vec::new();
    let mut origins: Vec<usize> = Vec::new();
    let mut styles: Vec<SpanStyle> = Vec::new();
    let mut model: Vec<bool> = Vec::new();
    for (si, span) in spans.iter().enumerate() {
        if span.math && span.text(source) != "\n" {
            for (text, script) in math_scripts(&tex_symbols(span.text(source))) {
                let mut piece = span.clone();
                piece.set_text(text);
                let mut style = span_style(theme, cfg, base, &piece);
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
            styles.push(span_style(theme, cfg, base, span));
            shaped.push(span.clone());
            origins.push(si);
            model.push(model_spans);
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
                    out,
                );
                segment.clear();
            } else if i < shaped.len() {
                height += line_height;
            }
        } else {
            segment.push(i);
        }
        i += 1;
    }
    height
}

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
    out: &mut LayoutDoc,
) -> f32 {
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(base.size, line_height));
    buffer.set_size(&mut fonts.font_system, Some(content_width), None);
    let rich: Vec<(&str, Attrs)> = segment
        .iter()
        .map(|&si| {
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
            (spans[si].text(source), attrs)
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
    buffer.shape_until_scroll(&mut fonts.font_system, false);

    // Byte offset of each segment member inside the shaped line, so a
    // model reference lands in its own span's display text.
    let mut prefixes: Vec<(usize, u32)> = Vec::with_capacity(segment.len());
    let mut acc = 0u32;
    for &si in segment {
        prefixes.push((si, acc));
        acc += spans[si].text(source).len() as u32;
    }

    // A span text may carry embedded newlines; the buffer splits them
    // into separate lines whose glyph offsets are line-local. Each
    // line's base offset inside the segment counts the stripped newline
    // back in, so a model reference lands on the span's own bytes.
    let mut line_offsets: Vec<u32> = Vec::with_capacity(buffer.lines.len());
    let mut line_acc = 0u32;
    for line in &buffer.lines {
        line_offsets.push(line_acc);
        line_acc += line.text().len() as u32 + 1;
    }

    let mut height = 0.0_f32;
    for run in buffer.layout_runs() {
        height = height.max(run.line_top + line_height);
        let line_text = buffer.lines[run.line_i].text();
        let line_base = line_offsets[run.line_i];
        let glyphs = trim_trailing_spaces(run.glyphs, line_text);
        let mut g = 0;
        while g < glyphs.len() {
            let span_index = glyphs[g].metadata;
            let mut end = g + 1;
            while end < glyphs.len() && glyphs[end].metadata == span_index {
                end += 1;
            }
            let first = &glyphs[g];
            let last = &glyphs[end - 1];
            let start_byte = glyphs[g..end].iter().map(|gl| gl.start).min().unwrap();
            let end_byte = glyphs[g..end].iter().map(|gl| gl.end).max().unwrap();
            let st = &styles[span_index];
            let x = x0 + first.x;
            let width = last.x + last.w - first.x;
            let y = y0 + run.line_top - st.rise;
            let baseline = y0 + run.line_y - st.rise;
            if let Some(pill) = st.pill {
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
            }
            let text = if model[span_index] {
                let prefix = prefixes
                    .iter()
                    .find(|(si, _)| *si == span_index)
                    .map(|(_, offset)| *offset)
                    .unwrap_or(0);
                TextRef::Model {
                    start: line_base + start_byte as u32 - prefix,
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
            g = end;
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
    let text_x = x0 + indent;
    let text_w = (avail - indent).max(40.0);
    let gutter = 10.0 * cfg.zoom;

    match marker {
        Marker::Bullet => {
            let (runs, width) = shape_marker(fonts, cfg, "\u{2022}", size, theme.text.body, out);
            place_marker(runs, text_x - width - gutter, y0, block_index, out);
        }
        Marker::Number(n) => {
            let text = format!("{n}.");
            let (runs, width) = shape_marker(fonts, cfg, &text, size, theme.text.body, out);
            place_marker(runs, text_x - width - gutter, y0, block_index, out);
        }
        Marker::None => {}
        Marker::Task { checked } => {
            let side = 0.8 * size;
            let bx = text_x - side - gutter;
            let by = y0 + (line_height - side) / 2.0;
            if *checked {
                let radius = 3.0 * cfg.zoom;
                out.rects.push(
                    DecoRect::fill(bx, by, side, side, theme.text.link).rounded(radius, radius),
                );
                let (runs, width) = shape_marker(
                    fonts,
                    cfg,
                    "\u{2713}",
                    0.7 * size,
                    theme.surface.background,
                    out,
                );
                place_marker(runs, bx + (side - width) / 2.0, y0, block_index, out);
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
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
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

    let measure = |fonts: &mut FontStore, spans: &[Span], bold: bool| -> f32 {
        let mut tmp = LayoutDoc::default();
        let base = BlockStyle {
            size,
            color: theme.text.body,
            bold,
            block_index,
        };
        shape_block(
            fonts, theme, cfg, source, spans, true, &base, 0.0, 0.0, 100_000.0, &mut tmp,
        );
        tmp.runs.iter().map(|r| r.x + r.width).fold(0.0, f32::max)
    };

    let empty_cell: Vec<Span> = Vec::new();
    let mut widths = vec![0.0f32; ncols];
    for (c, w) in widths.iter_mut().enumerate() {
        let mut natural = measure(fonts, header.get(c).unwrap_or(&empty_cell), true);
        for row in rows {
            natural = natural.max(measure(fonts, row.get(c).unwrap_or(&empty_cell), false));
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
            let base = BlockStyle {
                size,
                color: theme.text.body,
                bold: *is_header,
                block_index,
            };
            let h = shape_block(
                fonts,
                theme,
                cfg,
                source,
                spans,
                true,
                &base,
                0.0,
                0.0,
                (col_width - 2.0 * pad).max(20.0),
                &mut tmp,
            );
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

/// Replaces common TeX commands with their Unicode symbols; unknown
/// commands stay literal.
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
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Splits a TeX literal into script segments: `^` and `_` bind the next
/// character or braced group, as in TeX. Anything unmatched stays literal.
/// The display text a math literal renders as, scripts flattened: the
/// text the screen shows, which model copy and search read.
pub fn math_display(tex: &str) -> String {
    math_scripts(&tex_symbols(tex.trim()))
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn math_scripts(tex: &str) -> Vec<(String, Script)> {
    fn flush(normal: &mut String, out: &mut Vec<(String, Script)>) {
        if !normal.is_empty() {
            out.push((std::mem::take(normal), Script::Normal));
        }
    }
    let chars: Vec<char> = tex.chars().collect();
    let mut out = Vec::new();
    let mut normal = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '^' || c == '_' {
            let script = if c == '^' { Script::Sup } else { Script::Sub };
            match chars.get(i + 1) {
                Some('{') => {
                    if let Some(close) = chars[i + 2..].iter().position(|&ch| ch == '}') {
                        flush(&mut normal, &mut out);
                        out.push((chars[i + 2..i + 2 + close].iter().collect(), script));
                        i += close + 3;
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

fn alert_title(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "Note",
        AlertKind::Tip => "Tip",
        AlertKind::Important => "Important",
        AlertKind::Warning => "Warning",
        AlertKind::Caution => "Caution",
    }
}

fn alert_color(theme: &Theme, kind: AlertKind) -> Rgba {
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
/// `baseline`: glyphs into the math vector in the foreground ink, rules as
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
    let top = baseline - m.ascent;
    let bottom = baseline + m.descent;
    for g in &m.glyphs {
        out.math_glyphs.push(MathGlyph {
            glyph: g.glyph.0,
            x: x + g.x,
            y: baseline + g.y,
            size: g.size,
            ch: g.ch,
            top,
            bottom,
            color: ink,
            block: block_index,
        });
    }
    for r in &m.rules {
        out.rects
            .push(DecoRect::fill(x + r.x, baseline + r.y, r.width, r.height, ink).smooth());
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
    let m = noad::layout::layout(tex.trim(), noad::layout::MathStyle::Display, size, &font);
    let x = x0 + ((avail - m.width) / 2.0).max(0.0);
    let baseline = y0 + m.ascent;
    emit_math_layout(fonts, theme, cfg, &m, x, baseline, block_index, out);
    (m.ascent + m.descent).max(metrics::LINE_HEIGHT * size)
}

/// One footnote definition: the label as a small raised marker in link
/// color, the note text indented beside it.
#[allow(clippy::too_many_arguments)]
fn layout_footnote_def(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    source: &str,
    label: &str,
    spans: &[Span],
    base_size: f32,
    block_index: usize,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let marker = [Span::plain(format!("{label}."))];
    let marker_base = BlockStyle {
        size: 0.7 * base_size,
        color: theme.text.link,
        bold: true,
        block_index,
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
    let indent = (marker_w + 8.0 * cfg.zoom).max(metrics::INDENT * cfg.zoom);
    let base = BlockStyle {
        size: base_size,
        color: theme.text.body,
        bold: false,
        block_index,
    };
    let text_h = shape_block(
        fonts,
        theme,
        cfg,
        source,
        spans,
        true,
        &base,
        x0 + indent,
        y0,
        (avail - indent).max(40.0),
        out,
    );
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
}

/// Text with inline images or equations. Text shapes normally; an image
/// or equation joins the last text line when it fits there, otherwise it
/// opens its own row. A one-line text chunk that follows an equation on an
/// open line merges back onto that line, so a sentence flows around its
/// math; longer continuations start below, a recorded limitation.
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

    let mut i = 0;
    while i < spans.len() {
        if spans[i].image.is_none() && !spans[i].math {
            // The text chunk up to the next image or equation, masked into
            // a full-length span list so run span indices keep mapping to
            // the model.
            let start = i;
            while i < spans.len() && spans[i].image.is_none() && !spans[i].math {
                i += 1;
            }
            if spans[start..i]
                .iter()
                .all(|s| s.text(source).trim().is_empty())
            {
                // Whitespace between two placed boxes keeps its word gap.
                if let Some(l) = line.as_mut() {
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
            let h = shape_block(
                fonts, theme, cfg, source, &masked, true, base, x0, y, avail, out,
            );
            let mut last_top = out.runs[runs_mark..].iter().map(|r| r.y).fold(y, f32::max);
            let end_x = out.runs[runs_mark..]
                .iter()
                .filter(|r| (r.y - last_top).abs() < 1.0)
                .map(|r| r.x + r.width)
                .fold(x0, f32::max);
            let mut last_base = out.runs[runs_mark..]
                .iter()
                .filter(|r| (r.y - last_top).abs() < 1.0)
                .map(|r| r.baseline)
                .fold(0.0, f32::max);
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
                        l.end_x += end_x - x0;
                        true
                    }
                    _ => false,
                }
            } else {
                false
            };
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
                            last_top += dy;
                            last_base += dy;
                            y = below;
                        }
                    }
                }
                y += h;
                line = Some(FlowLine {
                    top: last_top,
                    height: line_height,
                    end_x,
                    baseline: Some(last_base),
                    row: false,
                });
            }
        } else if spans[i].math {
            let span = &spans[i];
            let font = OryxMathFont::new();
            let m = noad::layout::layout(
                span.text(source).trim(),
                noad::layout::MathStyle::Text,
                base.size,
                &font,
            );
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
                l.end_x += space;
                emit_math_layout(fonts, theme, cfg, &m, l.end_x, lb, base.block_index, out);
                l.end_x += w;
            } else {
                if let Some(l) = line.take() {
                    if l.row {
                        y = l.top + l.height + 0.25 * base.size;
                    }
                }
                let baseline = y + m.ascent.max(0.75 * line_height);
                emit_math_layout(fonts, theme, cfg, &m, x0, baseline, base.block_index, out);
                let height = (baseline - y) + m.descent.max(0.25 * line_height);
                line = Some(FlowLine {
                    top: y,
                    height,
                    end_x: x0 + w,
                    baseline: Some(baseline),
                    row: true,
                });
            }
            i += 1;
        } else {
            let span = &spans[i];
            let image = span.image.as_ref().expect("image span");
            let (w, h, loaded) = image_size(media, image, cfg, avail);
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
                place_image(out, theme, span, image, l.end_x + gap, iy, w, h, loaded);
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
                place_image(out, theme, span, image, x0, y, w, h, loaded);
                line = Some(FlowLine {
                    top: y,
                    height: h,
                    end_x: x0 + w,
                    baseline: None,
                    row: true,
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

/// Display size of an inline image: attribute pixels win, the natural size
/// fills in the rest, everything capped to the content width. The flag is
/// false when no pixels are available yet.
fn image_size(
    media: &mut MediaCache,
    image: &SpanImage,
    cfg: &ViewConfig,
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
        (None, None, None) => (
            (120.0 * scale).min(avail),
            metrics::LINE_HEIGHT * cfg.body_size,
        ),
    };
    if w > avail {
        h *= avail / w;
        w = avail;
    }
    (w, h, natural.is_some())
}

/// Places one inline image; without pixels yet a bordered placeholder box
/// holds its spot until the fetch lands.
#[allow(clippy::too_many_arguments)]
fn place_image(
    out: &mut LayoutDoc,
    theme: &Theme,
    span: &Span,
    image: &SpanImage,
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
    if !loaded {
        out.rects
            .push(DecoRect::fill(x, y, w, h, theme.blocks.code_border).stroked(1.0));
    }
}

/// Shifts every element of a centered block so each visual line sits in the
/// middle of the content width. Lines are clustered by vertical overlap.
fn center_lines(
    out: &mut LayoutDoc,
    runs_mark: usize,
    rects_mark: usize,
    images_mark: usize,
    x0: f32,
    avail: f32,
) {
    // (top, bottom, kind, index) per element; kinds: 0 runs, 1 rects, 2 images.
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
                _ => (out.images[item.3].x, out.images[item.3].width),
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
        let dx = x0 + (avail - (max_x - min_x)) / 2.0 - min_x;
        if dx > 0.5 {
            for item in group {
                match item.2 {
                    0 => out.runs[item.3].x += dx,
                    1 => out.rects[item.3].x += dx,
                    _ => out.images[item.3].x += dx,
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
        text,
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    let mut runs = Vec::new();
    let mut width = 0.0f32;
    for run in buffer.layout_runs() {
        let Some(last) = run.glyphs.last() else {
            continue;
        };
        width = width.max(last.x + last.w);
        let text = out.side_ref(text);
        let family = out.family_id(family_name);
        runs.push(TextRun {
            text,
            x: 0.0,
            y: run.line_top,
            baseline: run.line_y,
            width: last.x + last.w,
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
/// moved.
pub fn recolor_batch(
    lay: &mut LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    patches: &[(usize, Range<usize>)],
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
    Some((run_lo, run_hi, delta))
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
    recolor_batch(lay, doc, theme, fonts, cfg, &[(block, lines)])
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
/// panel, colors from the highlight roles.
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
    out: &mut LayoutDoc,
) -> f32 {
    let whole_line = [(0..line.len(), SyntaxRole::Plain)];
    let segments: &[(Range<usize>, SyntaxRole)] = if segments.is_empty() {
        &whole_line
    } else {
        segments
    };
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, Some(wrap_width), None);
    let rich: Vec<(&str, Attrs)> = segments
        .iter()
        .enumerate()
        .map(|(index, (range, _))| {
            let attrs = Attrs::new()
                .family(Family::Name(&cfg.code_family))
                .metadata(index);
            (&line[range.clone()], attrs)
        })
        .collect();
    let default_attrs = Attrs::new().family(Family::Name(&cfg.code_family));
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
        let mut g = 0;
        while g < glyphs.len() {
            let segment_index = glyphs[g].metadata;
            let mut end = g + 1;
            while end < glyphs.len() && glyphs[end].metadata == segment_index {
                end += 1;
            }
            let first = &glyphs[g];
            let last = &glyphs[end - 1];
            let start_byte = glyphs[g..end].iter().map(|gl| gl.start).min().unwrap();
            let end_byte = glyphs[g..end].iter().map(|gl| gl.end).max().unwrap();
            let role = segments[segment_index].1;
            // The rich pieces cover the line contiguously, so an offset
            // into their concatenation is an offset into the line.
            let family = out.family_id(&cfg.code_family);
            out.runs.push(TextRun {
                text: TextRef::Model {
                    start: start_byte as u32,
                    len: (end_byte - start_byte) as u32,
                },
                x: x0 + first.x,
                y: y0 + run.line_top,
                baseline: y0 + run.line_y,
                width: last.x + last.w - first.x,
                size,
                family,
                weight: Weight::NORMAL.0,
                italic: false,
                color: role_color(theme, role),
                block: block_index,
                span: line_index,
            });
            g = end;
        }
    }
    height.max(line_height)
}

fn role_color(theme: &Theme, role: SyntaxRole) -> Rgba {
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
    }
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

    fn seg(text: &str, script: Script) -> (String, Script) {
        (text.to_string(), script)
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

    #[test]
    fn math_scripts_keep_unmatched_literal() {
        assert_eq!(math_scripts("plain"), [seg("plain", Script::Normal)]);
        assert_eq!(math_scripts("a^"), [seg("a^", Script::Normal)]);
        assert_eq!(math_scripts("a^{open"), [seg("a^{open", Script::Normal)]);
        assert_eq!(math_scripts(""), Vec::<(String, Script)>::new());
    }
}
