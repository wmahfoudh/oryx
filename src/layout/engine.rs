//! The layout engine: document model in, positioned runs and rects out.
//! Pure with respect to the window: no pixels, fully testable with numbers.

use std::ops::Range;

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use super::metrics;
use crate::doc::model::{BlockKind, Document, Marker, Span};
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Copy)]
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
    /// Heading anchor slugs and their y positions.
    pub anchors: Vec<(String, f32)>,
}

pub fn layout(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    cfg: &ViewConfig,
    viewport_width: f32,
) -> LayoutDoc {
    let margin = metrics::MARGIN_RATIO * viewport_width;
    let vertical_margin = metrics::VERTICAL_MARGIN_EM * cfg.body_size * cfg.zoom;
    let content_width = (viewport_width - 2.0 * margin).max(50.0);
    let mut out = LayoutDoc::default();
    let mut cursor = 0.0_f32;
    let mut first = true;

    let mut prev_quote_depth = 0u8;
    let mut prev_is_list = false;

    for (block_index, block) in doc.blocks.iter().enumerate() {
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
                    continue;
                }
                cfg.body_size * heading.map(metrics::heading_scale).unwrap_or(1.0) * cfg.zoom
            }
            BlockKind::CodeBlock { .. } => cfg.code_size * cfg.zoom,
            BlockKind::Rule | BlockKind::Table { .. } => cfg.body_size * cfg.zoom,
            _ => continue,
        };
        let mut gap = 0.0;
        if first {
            cursor = vertical_margin;
            first = false;
        } else if is_list && prev_is_list {
            gap = 0.25 * base_size;
            cursor += gap;
        } else {
            gap = metrics::space_above(heading, base_size);
            cursor += gap;
        }
        if let BlockKind::Heading { anchor, .. } = &block.kind {
            out.anchors.push((anchor.clone(), cursor));
        }

        let quote_indent = block.quote_depth as f32 * metrics::INDENT * cfg.zoom;
        let quote_pad = if block.quote_depth > 0 {
            12.0 * cfg.zoom
        } else {
            0.0
        };
        let x_base = margin + quote_indent + quote_pad;
        let avail = (content_width - quote_indent - 2.0 * quote_pad).max(40.0);
        let rects_mark = out.rects.len();

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
                shape_block(
                    fonts, theme, cfg, spans, &base, x_base, cursor, avail, &mut out,
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
                marker,
                *depth,
                spans,
                block_index,
                x_base,
                cursor,
                avail,
                &mut out,
            ),
            BlockKind::CodeBlock {
                lines, highlights, ..
            } => layout_code(
                fonts,
                theme,
                cfg,
                lines,
                highlights,
                block_index,
                x_base,
                cursor,
                avail,
                &mut out,
            ),
            BlockKind::Rule => {
                let thickness = (1.0 * cfg.zoom).max(1.0);
                out.rects.push(DecoRect::fill(
                    x_base,
                    cursor,
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
                cursor,
                avail,
                &mut out,
            ),
            _ => 0.0,
        };

        // Quote decoration wraps the block, extending over the gap when the
        // previous block was quoted too, so consecutive quoted blocks read
        // as one region. Inserted at rects_mark to paint under the block's
        // own rects (pills, strikes, panels).
        if block.quote_depth > 0 {
            let continues = prev_quote_depth > 0;
            let top = if continues { cursor - gap } else { cursor };
            let panel_h = cursor + height - top;
            let mut decoration = vec![DecoRect::fill(
                margin,
                top,
                content_width,
                panel_h,
                theme.blocks.quote_bg,
            )];
            for level in 0..block.quote_depth {
                decoration.push(DecoRect::fill(
                    margin + level as f32 * metrics::INDENT * cfg.zoom,
                    top,
                    3.0 * cfg.zoom,
                    panel_h,
                    theme.blocks.quote_bar,
                ));
            }
            out.rects.splice(rects_mark..rects_mark, decoration);
        }

        cursor += height + metrics::space_below(base_size);
        prev_quote_depth = block.quote_depth;
        prev_is_list = is_list;
    }

    if !first {
        out.height = cursor + vertical_margin;
    }
    out
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
}

fn span_style(theme: &Theme, cfg: &ViewConfig, base: &BlockStyle, span: &Span) -> SpanStyle {
    let code = span.code;
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
    SpanStyle {
        family: if code || span.math {
            cfg.code_family.clone()
        } else {
            cfg.body_family.clone()
        },
        size: if code || span.math {
            cfg.code_size * cfg.zoom * (base.size / (cfg.body_size * cfg.zoom))
        } else {
            base.size
        },
        weight: if base.bold || span.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        },
        italic: span.italic,
        strike: span.strike,
        color,
        link: span.link.clone(),
        pill: code.then_some(theme.text.inline_code_bg),
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
    let styles: Vec<SpanStyle> = spans
        .iter()
        .map(|s| span_style(theme, cfg, base, s))
        .collect();

    let mut height = 0.0_f32;
    let mut segment: Vec<usize> = Vec::new();
    let mut i = 0;
    while i <= spans.len() {
        let is_break = i == spans.len() || spans[i].text == "\n";
        if is_break {
            if !segment.is_empty() {
                height += shape_segment(
                    fonts,
                    cfg,
                    spans,
                    &styles,
                    &segment,
                    base,
                    x0,
                    y0 + height,
                    content_width,
                    line_height,
                    out,
                );
                segment.clear();
            } else if i < spans.len() {
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
            let y = y0 + run.line_top;
            let baseline = y0 + run.line_y;
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
                span: span_index,
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
    let height = shape_block(fonts, theme, cfg, spans, &base, text_x, y0, text_w, out);
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

/// Lays out a fenced code block: a bordered panel of monospace lines,
/// one row per source line, no wrapping, colors from highlight roles.
#[allow(clippy::too_many_arguments)]
fn layout_code(
    fonts: &mut FontStore,
    theme: &Theme,
    cfg: &ViewConfig,
    lines: &[String],
    highlights: &[Vec<(Range<usize>, SyntaxRole)>],
    block_index: usize,
    x0: f32,
    y0: f32,
    content_width: f32,
    out: &mut LayoutDoc,
) -> f32 {
    let size = cfg.code_size * cfg.zoom;
    let line_height = metrics::LINE_HEIGHT * size;
    let pad = 12.0 * cfg.zoom;
    let height = lines.len() as f32 * line_height + 2.0 * pad;
    let blocks = &theme.blocks;
    let radius = metrics::CORNER_RADIUS * cfg.zoom;
    out.rects.push(
        DecoRect::fill(x0, y0, content_width, height, blocks.code_bg).rounded(radius, radius),
    );
    out.rects.push(
        DecoRect::fill(x0, y0, content_width, height, blocks.code_border)
            .rounded(radius, radius)
            .stroked((1.0 * cfg.zoom).max(1.0)),
    );
    let empty: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let segments = highlights.get(i).unwrap_or(&empty);
        shape_code_line(
            fonts,
            theme,
            cfg,
            line,
            segments,
            block_index,
            x0 + pad,
            y0 + pad + i as f32 * line_height,
            size,
            line_height,
            out,
        );
    }
    height
}

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
    out: &mut LayoutDoc,
) {
    let whole_line = [(0..line.len(), SyntaxRole::Plain)];
    let segments: &[(Range<usize>, SyntaxRole)] = if segments.is_empty() {
        &whole_line
    } else {
        segments
    };
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
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
    for run in buffer.layout_runs() {
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
