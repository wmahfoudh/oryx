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

/// One styled, positioned run of text on a single visual line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    /// Top of the line box.
    pub y: f32,
    /// Baseline y, used by glyph rasterization at paint.
    pub baseline: f32,
    pub width: f32,
    pub size: f32,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub color: Rgba,
    pub link: Option<String>,
    /// Source position for selection and copy: block and span index.
    pub block: usize,
    pub span: usize,
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
}

#[derive(Debug, Default)]
pub struct LayoutDoc {
    pub height: f32,
    pub runs: Vec<TextRun>,
    pub rects: Vec<DecoRect>,
    /// Placed images, blitted by paint from the media cache.
    pub images: Vec<ImagePlace>,
    /// Heading anchor slugs and their y positions.
    pub anchors: Vec<(String, f32)>,
    /// Per-line records for code blocks, ordered by block then line;
    /// `recolor_code_lines` re-shapes through them.
    pub code_lines: Vec<CodeLine>,
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

impl LayoutDoc {
    /// Link target under a point in document coordinates, if any.
    /// The hit box of a run spans its full line height.
    pub fn link_at(&self, x: f32, y: f32) -> Option<&str> {
        let run_hit = self.runs.iter().find_map(|r| {
            let target = r.link.as_deref()?;
            let inside = x >= r.x
                && x <= r.x + r.width
                && y >= r.y
                && y <= r.y + metrics::LINE_HEIGHT * r.size;
            inside.then_some(target)
        });
        run_hit.or_else(|| {
            self.images.iter().find_map(|i| {
                let target = i.link.as_deref()?;
                let inside = x >= i.x && x <= i.x + i.width && y >= i.y && y <= i.y + i.height;
                inside.then_some(target)
            })
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
}

impl LayoutPass {
    /// True once every block is placed.
    pub fn is_complete(&self) -> bool {
        self.done
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
/// and grows as lines land, which moves no index.
struct OpenCode {
    block: usize,
    frame: Frame,
    /// Panel background rect; the border rect follows it.
    panel: usize,
    y0: f32,
    y: f32,
    pad: f32,
    size: f32,
    line_height: f32,
    wrap_width: f32,
    line: usize,
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
    // selection and copy keep their mapping.
    let (body_order, note_order): (Vec<usize>, Vec<usize>) = (0..doc.blocks.len())
        .partition(|&i| !matches!(doc.blocks[i].kind, BlockKind::FootnoteDef { .. }));
    let notes_start = body_order.len();
    let has_notes = !note_order.is_empty();
    let order: Vec<usize> = body_order.into_iter().chain(note_order).collect();

    let pass = LayoutPass {
        order,
        position: 0,
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
    };
    (LayoutDoc::default(), pass)
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
    while !pass.done {
        if deadline.is_some_and(|at| Instant::now() >= at) {
            return false;
        }
        layout_step(doc, theme, fonts, media, cfg, out, pass);
    }
    true
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
        Some(open) => open.y + open.pad + pass.vertical_margin,
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
        pass.cursor += (1.0 * cfg.zoom).max(1.0);
    }
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
                return;
            }
            cfg.body_size * heading.map(metrics::heading_scale).unwrap_or(1.0) * cfg.zoom
        }
        BlockKind::CodeBlock { .. } => cfg.code_size * cfg.zoom,
        BlockKind::Rule
        | BlockKind::Table { .. }
        | BlockKind::Image { .. }
        | BlockKind::MathBlock { .. }
        | BlockKind::Frontmatter { .. } => cfg.body_size * cfg.zoom,
        BlockKind::FootnoteDef { .. } => 0.85 * cfg.body_size * cfg.zoom,
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

    let quote_indent = block.quote_depth as f32 * metrics::INDENT * cfg.zoom;
    let quote_pad = if block.quote_depth > 0 {
        12.0 * cfg.zoom
    } else {
        0.0
    };
    let x_base = pass.margin + quote_indent + quote_pad;
    let avail = (pass.content_width - quote_indent - 2.0 * quote_pad).max(40.0);
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
    if alert_start {
        let kind = block.alert.expect("alert start has a kind");
        let title = [Span::plain(alert_title(kind))];
        let base = BlockStyle {
            size: cfg.body_size * cfg.zoom,
            color: alert_color(theme, kind),
            bold: true,
            block_index,
        };
        let title_h = shape_block(
            fonts,
            theme,
            cfg,
            &title,
            &base,
            x_base,
            pass.cursor,
            avail,
            out,
        );
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

    let height = match &block.kind {
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
                fonts,
                theme,
                cfg,
                media,
                spans,
                &base,
                x_base,
                pass.cursor,
                avail,
                out,
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
            media,
            marker,
            *depth,
            spans,
            block_index,
            x_base,
            pass.cursor,
            avail,
            out,
        ),
        // A code file is one block, so it is opened and its lines land
        // over as many steps as the slice budget allows.
        BlockKind::CodeBlock { .. } => {
            let open = open_code(theme, cfg, block_index, frame, out, pass);
            place_code_line(doc, theme, fonts, cfg, open, out, pass);
            return;
        }
        BlockKind::Rule => {
            let thickness = (1.0 * cfg.zoom).max(1.0);
            out.rects.push(DecoRect::fill(
                x_base,
                pass.cursor,
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
            header,
            rows,
            block_index,
            x_base,
            pass.cursor,
            avail,
            out,
        ),
        BlockKind::Image { path, alt } => layout_image(
            fonts,
            theme,
            cfg,
            media,
            path,
            alt,
            block_index,
            x_base,
            pass.cursor,
            avail,
            out,
        ),
        BlockKind::Frontmatter { entries } => layout_frontmatter(
            fonts,
            theme,
            cfg,
            entries,
            block_index,
            x_base,
            pass.cursor,
            avail,
            out,
        ),
        BlockKind::MathBlock { tex } => layout_math_block(
            fonts,
            theme,
            cfg,
            tex,
            block_index,
            x_base,
            pass.cursor,
            avail,
            out,
        ),
        BlockKind::FootnoteDef { label, spans } => {
            out.anchors.push((format!("footnote:{label}"), pass.cursor));
            layout_footnote_def(
                fonts,
                theme,
                cfg,
                label,
                spans,
                base_size,
                block_index,
                x_base,
                pass.cursor,
                avail,
                out,
            )
        }
    };

    finish_block(theme, cfg, block, frame, height, out, pass);
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
        let panel_h = pass.cursor + height - top;
        let mut decoration = vec![DecoRect::fill(
            pass.margin,
            top,
            pass.content_width,
            panel_h,
            theme.blocks.quote_bg,
        )];
        for level in 0..block.quote_depth {
            let bar = match block.alert {
                Some(kind) if level == 0 => alert_color(theme, kind),
                _ => theme.blocks.quote_bar,
            };
            decoration.push(DecoRect::fill(
                pass.margin + level as f32 * metrics::INDENT * cfg.zoom,
                top,
                3.0 * cfg.zoom,
                panel_h,
                bar,
            ));
        }
        out.rects
            .splice(frame.marks.rects..frame.marks.rects, decoration);
    }

    pass.cursor += height + metrics::space_below(frame.base_size);
    pass.prev_quote_depth = block.quote_depth;
    pass.prev_alert = block.alert;
    pass.prev_is_list = frame.is_list;
    pass.prev_space_below = metrics::space_below(frame.base_size);
}

/// Opens a code block: the panel and border take their final index with a
/// provisional height, so later lines only grow them.
fn open_code(
    theme: &Theme,
    cfg: &ViewConfig,
    block_index: usize,
    frame: Frame,
    out: &mut LayoutDoc,
    pass: &LayoutPass,
) -> OpenCode {
    let size = cfg.code_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let pad = 12.0 * cfg.zoom;
    // Long lines wrap inside the panel instead of overflowing it, so the
    // panel height follows the shaped lines.
    let wrap_width = (frame.avail - 2.0 * pad).max(40.0);
    let y0 = pass.cursor;
    let panel = out.rects.len();
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let blocks = &theme.blocks;
    let height = 2.0 * pad;
    out.rects.push(
        DecoRect::fill(frame.x_base, y0, frame.avail, height, blocks.code_bg)
            .rounded(radius, radius),
    );
    out.rects.push(
        DecoRect::fill(frame.x_base, y0, frame.avail, height, blocks.code_border)
            .rounded(radius, radius)
            .stroked((1.0 * cfg.zoom).max(1.0)),
    );
    OpenCode {
        block: block_index,
        frame,
        panel,
        y0,
        y: y0 + pad,
        pad,
        size,
        line_height,
        wrap_width,
        line: 0,
    }
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
        finish_block(
            theme,
            cfg,
            block,
            open.frame,
            open.y - open.y0 + open.pad,
            out,
            pass,
        );
        return;
    }

    let line = &lines[open.line];
    if line.is_empty() {
        open.y += open.line_height;
    } else {
        let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
        let segments = highlights.get(open.line).unwrap_or(&empty);
        let x0 = open.frame.x_base + open.pad;
        let run_start = out.runs.len();
        let advance = shape_code_line(
            fonts,
            theme,
            cfg,
            line,
            segments,
            open.block,
            x0,
            open.y,
            open.size,
            open.line_height,
            open.wrap_width,
            out,
        );
        out.code_lines.push(CodeLine {
            block: open.block,
            line: open.line,
            runs: run_start..out.runs.len(),
            x0,
            y0: open.y,
            size: open.size,
            line_height: open.line_height,
            wrap_width: open.wrap_width,
        });
        open.y += advance;
    }
    open.line += 1;

    let height = open.y - open.y0 + open.pad;
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
    color: Rgba,
    link: Option<String>,
    /// Background pill color for inline code.
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
        color,
        link: span.link.clone(),
        pill: code.then_some(theme.text.inline_code_bg),
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
    spans: &[Span],
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    content_width: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let line_height = metrics::LINE_HEIGHT * base.size;
    // Math literals expand into script segments at this point only; the
    // model keeps the raw TeX. Each expanded piece remembers its model
    // span so selection and copy still map to the source.
    let mut shaped: Vec<Span> = Vec::new();
    let mut origins: Vec<usize> = Vec::new();
    let mut styles: Vec<SpanStyle> = Vec::new();
    for (si, span) in spans.iter().enumerate() {
        if span.math && span.text != "\n" {
            for (text, script) in math_scripts(&tex_symbols(&span.text)) {
                let mut piece = span.clone();
                piece.text = text;
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
            }
        } else {
            styles.push(span_style(theme, cfg, base, span));
            shaped.push(span.clone());
            origins.push(si);
        }
    }

    let mut height = 0.0_f32;
    let mut segment: Vec<usize> = Vec::new();
    let mut i = 0;
    while i <= shaped.len() {
        let is_break = i == shaped.len() || shaped[i].text == "\n";
        if is_break {
            if !segment.is_empty() {
                height += shape_segment(
                    fonts,
                    cfg,
                    &shaped,
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
    spans: &[Span],
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
            (spans[si].text.as_str(), attrs)
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

    let mut height = 0.0_f32;
    for run in buffer.layout_runs() {
        height = height.max(run.line_top + line_height);
        let line_text = buffer.lines[run.line_i].text();
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
            out.runs.push(TextRun {
                text: line_text[start_byte..end_byte].to_string(),
                x,
                y,
                baseline,
                width,
                size: st.size,
                family: st.family.clone(),
                weight: st.weight.0,
                italic: st.italic,
                color: st.color,
                link: st.link.clone(),
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
            g = end;
        }
    }
    height
}

/// Lays out one list item: marker (bullet, number, or checkbox) in the
/// gutter, item text indented one step per nesting depth.
#[allow(clippy::too_many_arguments)]
fn layout_list_item(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
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
            let (runs, width) = shape_marker(fonts, cfg, "\u{2022}", size, theme.text.body);
            place_marker(runs, text_x - width - gutter, y0, block_index, out);
        }
        Marker::Number(n) => {
            let text = format!("{n}.");
            let (runs, width) = shape_marker(fonts, cfg, &text, size, theme.text.body);
            place_marker(runs, text_x - width - gutter, y0, block_index, out);
        }
        Marker::Task { checked } => {
            let side = 0.8 * size;
            let bx = text_x - side - gutter;
            let by = y0 + (line_height - side) / 2.0;
            if *checked {
                let radius = 3.0 * cfg.zoom;
                out.rects.push(
                    DecoRect::fill(bx, by, side, side, theme.text.link).rounded(radius, radius),
                );
                let (runs, width) =
                    shape_marker(fonts, cfg, "\u{2713}", 0.7 * size, theme.surface.background);
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
        fonts, theme, cfg, media, spans, &base, text_x, y0, text_w, out,
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
            fonts, theme, cfg, spans, &base, 0.0, 0.0, 100_000.0, &mut tmp,
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
        *w = natural + 2.0 * pad;
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
    let all_rows: Vec<(&[Vec<Span>], bool)> = std::iter::once((header, true))
        .chain(rows.iter().map(|r| (r.as_slice(), false)))
        .collect();
    let mut boundaries = vec![y0];
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
                spans,
                &base,
                0.0,
                0.0,
                (col_width - 2.0 * pad).max(20.0),
                &mut tmp,
            );
            row_h = row_h.max(h);
            shaped.push(tmp);
        }
        let full_h = row_h + 2.0 * vpad;
        let stripe = if *is_header {
            Some(theme.blocks.table_header_bg)
        } else if row_index % 2 == 0 {
            // Header is row 0, so even indices are the 1st, 3rd... body rows.
            Some(theme.blocks.table_row_alt_bg)
        } else {
            None
        };
        if let Some(color) = stripe {
            // Stripes at the table's corners round to match the outline.
            let radius = metrics::CORNER_RADIUS * cfg.zoom;
            let top_r = if *is_header { radius } else { 0.0 };
            let bottom_r = if row_index + 1 == all_rows.len() {
                radius
            } else {
                0.0
            };
            out.rects
                .push(DecoRect::fill(x0, y, table_w, full_h, color).rounded(top_r, bottom_r));
        }
        for (c, tmp) in shaped.into_iter().enumerate() {
            let dx = col_x[c] + pad;
            let dy = y + vpad;
            for mut run in tmp.runs {
                run.x += dx;
                run.y += dy;
                run.baseline += dy;
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

/// Places an image scaled down to fit the available width (never scaled
/// up); an unloadable image becomes a bordered placeholder with alt text.
#[allow(clippy::too_many_arguments)]
fn layout_image(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
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
        let width = (iw as f32).min(avail);
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
    let pad = 12.0 * cfg.zoom;
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let alt_span = [Span {
        text: if alt.is_empty() {
            path.to_string()
        } else {
            alt.to_string()
        },
        italic: true,
        ..Span::default()
    }];
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
        &alt_span,
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
    let rects_mark = out.rects.len();
    let text_h = shape_block(
        fonts,
        theme,
        cfg,
        &spans,
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

/// Block math: the literal centered in a flush panel on the code
/// background, scripts rendered like inline math.
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
    let pad = 12.0 * cfg.zoom;
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    let span = Span {
        text: tex.trim().to_string(),
        math: true,
        ..Span::default()
    };
    let base = BlockStyle {
        size: cfg.body_size * cfg.zoom,
        color: theme.text.math,
        bold: false,
        block_index,
    };
    let runs_mark = out.runs.len();
    let rects_mark = out.rects.len();
    let text_h = shape_block(
        fonts,
        theme,
        cfg,
        &[span],
        &base,
        x0 + pad,
        y0 + pad,
        avail - 2.0 * pad,
        out,
    );
    let min_x = out.runs[runs_mark..]
        .iter()
        .map(|r| r.x)
        .fold(f32::MAX, f32::min);
    let max_x = out.runs[runs_mark..]
        .iter()
        .map(|r| r.x + r.width)
        .fold(0.0, f32::max);
    if max_x > min_x {
        let dx = (x0 + avail / 2.0) - (min_x + max_x) / 2.0;
        for run in &mut out.runs[runs_mark..] {
            run.x += dx;
        }
    }
    let box_h = text_h + 2.0 * pad;
    out.rects.splice(
        rects_mark..rects_mark,
        [DecoRect::fill(x0, y0, avail, box_h, theme.blocks.code_bg).rounded(radius, radius)],
    );
    box_h
}

/// One footnote definition: the label as a small raised marker in link
/// color, the note text indented beside it.
#[allow(clippy::too_many_arguments)]
fn layout_footnote_def(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
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
    shape_block(fonts, theme, cfg, &marker, &marker_base, x0, y0, avail, out);
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
        spans,
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
    media: &mut MediaCache,
    spans: &[Span],
    base: &BlockStyle,
    x0: f32,
    y0: f32,
    avail: f32,
    out: &mut LayoutDoc,
) -> f32 {
    if spans.iter().any(|s| s.image.is_some()) {
        layout_flow(fonts, theme, cfg, media, spans, base, x0, y0, avail, out)
    } else {
        shape_block(fonts, theme, cfg, spans, base, x0, y0, avail, out)
    }
}

/// One line images may join: its top, height, pen x, and whether it is an
/// image row rather than a text line.
struct FlowLine {
    top: f32,
    height: f32,
    end_x: f32,
    row: bool,
}

/// Text with inline images. Text shapes normally; an image joins the last
/// text line when it fits there, otherwise images collect into rows that
/// wrap at the content width. Text after an image starts a new line.
#[allow(clippy::too_many_arguments)]
fn layout_flow(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
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
        if spans[i].image.is_none() {
            // The text chunk up to the next image, masked into a full-length
            // span list so run span indices keep mapping to the model.
            let start = i;
            while i < spans.len() && spans[i].image.is_none() {
                i += 1;
            }
            if spans[start..i].iter().all(|s| s.text.trim().is_empty()) {
                continue;
            }
            if let Some(l) = line.take() {
                if l.row {
                    y = l.top + l.height + 0.25 * base.size;
                }
            }
            let masked: Vec<Span> = spans
                .iter()
                .enumerate()
                .map(|(si, s)| {
                    let mut c = s.clone();
                    if si < start || si >= i || s.image.is_some() {
                        c.text = String::new();
                        c.image = None;
                    }
                    c
                })
                .collect();
            let runs_mark = out.runs.len();
            let h = shape_block(fonts, theme, cfg, &masked, base, x0, y, avail, out);
            let last_top = out.runs[runs_mark..].iter().map(|r| r.y).fold(y, f32::max);
            let end_x = out.runs[runs_mark..]
                .iter()
                .filter(|r| (r.y - last_top).abs() < 1.0)
                .map(|r| r.x + r.width)
                .fold(x0, f32::max);
            y += h;
            line = Some(FlowLine {
                top: last_top,
                height: line_height,
                end_x,
                row: false,
            });
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
    let aw = image.width.map(|v| v as f32 * cfg.zoom);
    let ah = image.height.map(|v| v as f32 * cfg.zoom);
    let (mut w, mut h) = match (aw, ah, natural) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((nw, nh))) => (w, w * nh as f32 / nw as f32),
        (None, Some(h), Some((nw, nh))) => (h * nw as f32 / nh as f32, h),
        (None, None, Some((nw, nh))) => (nw as f32, nh as f32),
        (Some(w), None, None) => (w, w * 0.5),
        (None, Some(h), None) => (h * 2.0, h),
        (None, None, None) => (120.0_f32.min(avail), metrics::LINE_HEIGHT * cfg.body_size),
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
) -> (Vec<TextRun>, f32) {
    let line_height = metrics::LINE_HEIGHT * cfg.body_size * cfg.zoom;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let attrs = Attrs::new().family(Family::Name(&cfg.body_family));
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
        runs.push(TextRun {
            text: text.to_string(),
            x: 0.0,
            y: run.line_top,
            baseline: run.line_y,
            width: last.x + last.w,
            size,
            family: cfg.body_family.clone(),
            weight: Weight::NORMAL.0,
            italic: false,
            color,
            link: None,
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

/// Recolors the laid-out lines `lines` of code block `block` from the
/// document's current highlights, re-shaping only those lines and
/// splicing the runs in place. Geometry is unchanged; run indices after
/// the spliced range shift, so callers must drop selection and search
/// positions exactly as they do after a relayout.
#[allow(clippy::too_many_arguments)]
pub fn recolor_code_lines(
    lay: &mut LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    block: usize,
    lines: Range<usize>,
) {
    let lo = lay
        .code_lines
        .partition_point(|c| (c.block, c.line) < (block, lines.start));
    let hi = lay
        .code_lines
        .partition_point(|c| (c.block, c.line) < (block, lines.end));
    if lo == hi {
        return;
    }
    let Some(BlockKind::CodeBlock {
        lines: source_lines,
        highlights,
        ..
    }) = doc.blocks.get(block).map(|b| &b.kind)
    else {
        return;
    };
    let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
    let run_start = lay.code_lines[lo].runs.start;
    let run_end = lay.code_lines[hi - 1].runs.end;
    let mut scratch = LayoutDoc::default();
    let mut fresh: Vec<Range<usize>> = Vec::with_capacity(hi - lo);
    for record in &lay.code_lines[lo..hi] {
        let Some(line) = source_lines.get(record.line) else {
            return;
        };
        let segments = highlights.get(record.line).unwrap_or(&empty);
        let from = scratch.runs.len();
        shape_code_line(
            fonts,
            theme,
            cfg,
            line,
            segments,
            block,
            record.x0,
            record.y0,
            record.size,
            record.line_height,
            record.wrap_width,
            &mut scratch,
        );
        fresh.push(from..scratch.runs.len());
    }
    let delta = scratch.runs.len() as isize - (run_end - run_start) as isize;
    lay.runs.splice(run_start..run_end, scratch.runs);
    for (record, range) in lay.code_lines[lo..hi].iter_mut().zip(fresh) {
        record.runs = run_start + range.start..run_start + range.end;
    }
    for record in lay.code_lines[hi..].iter_mut() {
        record.runs = record.runs.start.wrapping_add_signed(delta)
            ..record.runs.end.wrapping_add_signed(delta);
    }
}

/// Shapes one code line: one row per source line, wrapping inside the
/// panel, colors from the highlight roles.
#[allow(clippy::too_many_arguments)]
fn shape_code_line(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    line: &str,
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
            out.runs.push(TextRun {
                text: line_text[start_byte..end_byte].to_string(),
                x: x0 + first.x,
                y: y0 + run.line_top,
                baseline: y0 + run.line_y,
                width: last.x + last.w - first.x,
                size,
                family: cfg.code_family.clone(),
                weight: Weight::NORMAL.0,
                italic: false,
                color: role_color(theme, role),
                link: None,
                block: block_index,
                span: segment_index,
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
