//! The layout engine: document model in, positioned runs and rects out.
//! Pure with respect to the window: no pixels, fully testable with numbers.

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};

use super::metrics;
use crate::doc::model::{BlockKind, Document, Span};
use crate::style::fonts::{FontStore, BODY_FAMILY, CODE_FAMILY};
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
            body_size: 16.0,
            code_size: 14.0,
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

/// A filled rectangle: panels, bars, strike lines, table grid.
#[derive(Debug, Clone, Copy)]
pub struct DecoRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
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
    let content_width = (viewport_width - 2.0 * margin).max(50.0);
    let mut out = LayoutDoc::default();
    let mut cursor = 0.0_f32;
    let mut first = true;

    for (block_index, block) in doc.blocks.iter().enumerate() {
        let (spans, heading) = match &block.kind {
            BlockKind::Heading { level, spans, .. } => (spans, Some(*level)),
            BlockKind::Paragraph { spans } => (spans, None),
            _ => continue,
        };
        if spans.is_empty() {
            continue;
        }
        let scale = heading.map(metrics::heading_scale).unwrap_or(1.0);
        let base_size = cfg.body_size * scale * cfg.zoom;
        if first {
            cursor = margin;
            first = false;
        } else {
            cursor += metrics::space_above(heading, base_size);
        }
        if let BlockKind::Heading { anchor, .. } = &block.kind {
            out.anchors.push((anchor.clone(), cursor));
        }
        let base = BlockStyle {
            size: base_size,
            color: match heading {
                Some(level) => heading_color(theme, level),
                None => theme.text.body,
            },
            bold: heading.is_some(),
            block_index,
        };
        let height = shape_block(
            fonts,
            theme,
            cfg,
            spans,
            &base,
            margin,
            cursor,
            content_width,
            &mut out,
        );
        cursor += height + metrics::space_below(base_size);
    }

    if !first {
        out.height = cursor + margin;
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
                out.rects.push(DecoRect {
                    x,
                    y: baseline - 0.3 * st.size,
                    width,
                    height: (0.06 * st.size).max(1.0),
                    color: st.color,
                });
            }
            g = end;
        }
    }
    height
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
