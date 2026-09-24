//! Line numbers in the left margin of a file of lines: a code file, a
//! text file, a markdown file in the editor.
//!
//! The numbers are painted with the band, from the block table's line
//! positions, and are no part of the layout: no run carries them, so
//! selection, copy, the caret and the PDF never see them, and an edit
//! renumbers them by repainting. A wrapped line has one position in the
//! table, its first row, so it gets one number. The layout's only part
//! is the room: `ViewConfig::gutter` widens the left margin when the
//! digits do not fit the margin the page already has.

use cosmic_text::{Attrs, Buffer, Color, Family, Metrics, Shaping};
use tiny_skia::Pixmap;

use crate::doc::model::{BlockKind, Document};
use crate::layout::{code_lines_in, metrics, LayoutDoc, LineSeat};
use crate::paint::band::blend_buffer;
use crate::paint::painter::round_rect;
use crate::style::fonts::FontStore;
use crate::style::theme::Rgba;

/// The numbers' size over the text's: a little smaller, so they read as
/// a margin note and five digits fit the usual margin.
pub const SCALE: f32 = 0.85;
/// Space between the numbers and the text, in ems of the numbers' size.
const GAP: f32 = 1.0;
/// Space between the window's edge and the widest number, in the same ems.
const INSET: f32 = 0.5;
/// The caret's line number stands in a box: its padding either side of
/// the digits, its height, and its corners, in the same ems. The padding
/// stays under the inset and under half the gap, so the box clears the
/// window's edge and the text.
const BOX_PAD: f32 = 0.35;
const BOX_HEIGHT: f32 = 1.25;
const BOX_RADIUS: f32 = 0.25;

/// True for a document whose rows are the lines of its file.
pub fn numbered(doc: &Document) -> bool {
    doc.code_file || doc.plain_file
}

/// The largest number the margin shows for `doc`: its last line's, or
/// one more when the text ends in a line break, where the editor's caret
/// can stand on the row after it and that row reads its own number.
pub fn last_number(doc: &Document) -> usize {
    let lines = match doc.blocks.first().map(|b| &b.kind) {
        Some(BlockKind::CodeBlock { lines, .. }) => lines.len(),
        _ => 0,
    };
    lines + usize::from(doc.source.ends_with('\n'))
}

/// The left margin the numbers of a file of `lines` lines need, the text
/// drawn in `family` at `text_size`: the widest number, the gap to the
/// text and the inset from the edge.
pub fn reserve(fonts: &mut FontStore, family: &str, text_size: f32, lines: usize) -> f32 {
    let size = SCALE * text_size;
    let digits = lines.max(1).to_string().len();
    // Digits share one width in most faces; where they do not, a zero
    // or an eight is the widest.
    let widest = ["0", "8"]
        .into_iter()
        .map(|digit| {
            let buffer = shaped(fonts, &digit.repeat(digits), family, size);
            line_metrics(&buffer).0
        })
        .fold(0.0, f32::max);
    widest + (GAP + INSET) * size
}

/// Paints the number of every line whose first row starts inside the
/// pixmap, which shows the document from `y_top` down.
pub(crate) fn paint(
    pixmap: &mut Pixmap,
    fonts: &mut FontStore,
    layout: &LayoutDoc,
    doc: &Document,
    color: Rgba,
    y_top: f32,
) {
    let bottom = y_top + pixmap.height() as f32;
    let mut baseline = None;
    for (block, lines) in code_lines_in(layout, doc, y_top..bottom) {
        for line in lines {
            let Some(seat) = layout.code_line_seat(block, line) else {
                continue;
            };
            // Every row of a file of lines has one height, so the text's
            // baseline is looked up once for the whole band.
            let baseline =
                *baseline.get_or_insert_with(|| text_baseline(fonts, layout, seat.height));
            number(pixmap, fonts, layout, seat, line, color, baseline, y_top);
        }
    }
}

/// One line's stretch of the margin, painted apart from the band: the
/// page's color, and the line's number in the page's color on a box of
/// `ink`. The editor lays it over the band on the caret's line, so the
/// caret's number is found at a glance. The editor's ink is the theme's
/// punctuation color: a mid-tone, quieter than the text, whose digits
/// read at least as well as the other numbers in the comment color.
pub struct Strip {
    /// Packed as the band's pixels are.
    pub pixels: Vec<u32>,
    pub width: u32,
    pub height: u32,
    /// The strip's top in document space, a whole pixel.
    pub y: f32,
}

/// The strip of line `line` of code block `block`, None before the pass
/// places the block. Its number lands on the pixels the band gave it:
/// both paint from a whole-pixel top, so the truncation agrees.
pub fn strip(
    fonts: &mut FontStore,
    layout: &LayoutDoc,
    doc: &Document,
    block: usize,
    line: usize,
    paper: Rgba,
    ink: Rgba,
) -> Option<Strip> {
    if !numbered(doc) {
        return None;
    }
    let seat = layout.code_line_seat(block, line)?;
    let y = seat.y.floor();
    // Up to the middle of the gap, clear of the text's first glyph, and
    // the row's own height, clear of the numbers above and below.
    let width = (seat.x - 0.5 * GAP * SCALE * layout.code_size)
        .floor()
        .max(1.0) as u32;
    let height = ((seat.y + seat.height).floor() - y).max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height)?;
    pixmap.fill(tiny_skia::Color::from_rgba8(paper.r, paper.g, paper.b, 255));
    let size = SCALE * layout.code_size;
    let digits = shaped(fonts, &(line + 1).to_string(), &layout.code_family, size);
    let right = seat.x - GAP * size;
    let (pad, box_h) = (BOX_PAD * size, (BOX_HEIGHT * size).min(height as f32));
    let left = right - line_metrics(&digits).0 - pad;
    let top = (seat.y - y) + (seat.height - box_h) / 2.0;
    if let Some(path) = round_rect(left, top, right + pad - left, box_h, BOX_RADIUS * size) {
        let mut fill = tiny_skia::Paint::default();
        fill.set_color_rgba8(ink.r, ink.g, ink.b, 255);
        fill.anti_alias = true;
        pixmap.fill_path(
            &path,
            &fill,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let baseline = text_baseline(fonts, layout, seat.height);
    number(&mut pixmap, fonts, layout, seat, line, paper, baseline, y);
    let pixels = pixmap
        .data()
        .chunks_exact(4)
        .map(|px| ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32)
        .collect();
    Some(Strip {
        pixels,
        width,
        height,
        y,
    })
}

/// Where the engine seats the text's baseline below a row's top: the
/// file's face at the text's size in a row `row_height` tall.
fn text_baseline(fonts: &mut FontStore, layout: &LayoutDoc, row_height: f32) -> f32 {
    let buffer = Buffer::new(
        &mut fonts.font_system,
        Metrics::new(layout.code_size, row_height),
    );
    let buffer = with_text(fonts, buffer, "0", &layout.code_family);
    line_metrics(&buffer).1
}

/// Paints the number of line `line`, seated at `seat`, right-aligned
/// against the gap and on the baseline of the line's first row.
#[allow(clippy::too_many_arguments)]
fn number(
    pixmap: &mut Pixmap,
    fonts: &mut FontStore,
    layout: &LayoutDoc,
    seat: LineSeat,
    line: usize,
    color: Rgba,
    text_baseline: f32,
    y_top: f32,
) {
    let size = SCALE * layout.code_size;
    let mut buffer = shaped(fonts, &(line + 1).to_string(), &layout.code_family, size);
    let (width, own_baseline) = line_metrics(&buffer);
    let right = seat.x - GAP * size;
    blend_buffer(
        pixmap,
        fonts,
        &mut buffer,
        Color::rgba(color.r, color.g, color.b, color.a),
        right - width,
        seat.y + text_baseline - own_baseline - y_top,
    );
}

/// `text` shaped on one line at the numbers' own metrics.
fn shaped(fonts: &mut FontStore, text: &str, family: &str, size: f32) -> Buffer {
    let buffer = Buffer::new(
        &mut fonts.font_system,
        Metrics::new(size, metrics::LINE_HEIGHT * size),
    );
    with_text(fonts, buffer, text, family)
}

fn with_text(fonts: &mut FontStore, mut buffer: Buffer, text: &str, family: &str) -> Buffer {
    buffer.set_size(&mut fonts.font_system, None, None);
    buffer.set_text(
        &mut fonts.font_system,
        text,
        &Attrs::new().family(Family::Name(family)),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    buffer
}

/// A one-line buffer's width and its baseline below the line's top.
fn line_metrics(buffer: &Buffer) -> (f32, f32) {
    buffer
        .layout_runs()
        .next()
        .map_or((0.0, 0.0), |run| (run.line_w, run.line_y))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::load;
    use crate::layout::{layout, ViewConfig};
    use crate::paint::band::{band_numbered, paper};
    use crate::style::theme::Theme;

    const WIDTH: u32 = 1000;
    const HEIGHT: u32 = 400;

    fn packed(c: Rgba) -> u32 {
        ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
    }

    fn lay(doc: &Document, cfg: &ViewConfig, fonts: &mut FontStore) -> LayoutDoc {
        let mut media = MediaCache::new(PathBuf::from("."));
        layout(
            doc,
            &Theme::default_dark(),
            fonts,
            &mut media,
            cfg,
            WIDTH as f32,
        )
    }

    fn painted(doc: &Document, lay: &LayoutDoc, fonts: &mut FontStore, numbers: bool) -> Vec<u32> {
        let theme = Theme::default_dark();
        let mut media = MediaCache::new(PathBuf::from("."));
        band_numbered(
            lay,
            doc,
            &theme,
            fonts,
            &mut media,
            &[],
            numbers.then_some(theme.syntax.comment),
            0.0,
            WIDTH,
            HEIGHT,
        )
    }

    /// Whether any pixel of the margin left of `x`, on the rows from
    /// `top` to `bottom`, differs from the page's color.
    fn inked(pixels: &[u32], paper: u32, x: f32, top: f32, bottom: f32) -> bool {
        (top as usize..bottom as usize).any(|row| {
            let start = row * WIDTH as usize;
            pixels[start..start + x as usize]
                .iter()
                .any(|&p| p != paper)
        })
    }

    #[test]
    fn every_line_gets_its_number_in_the_margin_blank_ones_too() {
        let doc = load::code_document(Some("rust"), "let a = 1;\n\nlet b = 2;\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let paper = packed(paper(&doc, &Theme::default_dark()));
        let plain = painted(&doc, &lay, &mut fonts, false);
        let numbered = painted(&doc, &lay, &mut fonts, true);
        for line in 0..3 {
            let seat = lay.code_line_seat(0, line).expect("placed");
            let (top, bottom) = (seat.y, seat.y + seat.height);
            assert!(
                !inked(&plain, paper, seat.x, top, bottom),
                "line {line}: an empty margin without the setting"
            );
            assert!(
                inked(&numbered, paper, seat.x, top, bottom),
                "line {line}: its number in the margin"
            );
        }
    }

    #[test]
    fn the_text_itself_paints_the_same_with_and_without_numbers() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let plain = painted(&doc, &lay, &mut fonts, false);
        let numbered = painted(&doc, &lay, &mut fonts, true);
        let x0 = lay.code_line_seat(0, 0).unwrap().x as usize;
        for row in 0..HEIGHT as usize {
            let start = row * WIDTH as usize + x0;
            let end = (row + 1) * WIDTH as usize;
            assert_eq!(plain[start..end], numbered[start..end], "row {row}");
        }
    }

    #[test]
    fn a_wrapped_line_gets_one_number_on_its_first_row() {
        let long = "word ".repeat(120);
        let doc = load::text_document(&format!("{long}\nshort\n"));
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let paper = packed(paper(&doc, &Theme::default_dark()));
        let numbered = painted(&doc, &lay, &mut fonts, true);
        let first = lay.code_line_seat(0, 0).unwrap();
        let second = lay.code_line_seat(0, 1).unwrap();
        assert!(
            second.y - first.y > 2.5 * first.height,
            "the long line wraps over several rows"
        );
        assert!(inked(
            &numbered,
            paper,
            first.x,
            first.y,
            first.y + first.height
        ));
        assert!(
            !inked(&numbered, paper, first.x, first.y + first.height, second.y),
            "the rows a line wraps onto carry no number"
        );
    }

    #[test]
    fn a_rendered_page_gets_no_numbers() {
        let doc = crate::doc::markdown::parse("Text.\n\n```rust\nlet a = 1;\n```\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        assert_eq!(
            painted(&doc, &lay, &mut fonts, false),
            painted(&doc, &lay, &mut fonts, true),
            "a fenced block on the page stays as it renders"
        );
    }

    #[test]
    fn the_reserve_grows_with_the_digits() {
        let mut fonts = FontStore::new();
        let family = ViewConfig::default().code_family;
        let two = reserve(&mut fonts, &family, 20.0, 99);
        let three = reserve(&mut fonts, &family, 20.0, 100);
        let six = reserve(&mut fonts, &family, 20.0, 288_026);
        assert!(two > 0.0);
        assert!(three > two && six > three);
        let digit = three - two;
        assert!(
            (six - three - 3.0 * digit).abs() < 0.5,
            "each digit adds its width: {two} {three} {six}"
        );
        assert_eq!(
            reserve(&mut fonts, &family, 20.0, 0),
            reserve(&mut fonts, &family, 20.0, 9),
            "an empty file keeps room for one digit"
        );
    }

    #[test]
    fn a_gutter_the_margin_already_holds_moves_nothing() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\n");
        let mut fonts = FontStore::new();
        let without = lay(&doc, &ViewConfig::default(), &mut fonts);
        let margin = metrics::MARGIN_RATIO * WIDTH as f32;
        let cfg = ViewConfig {
            gutter: margin - 10.0,
            ..ViewConfig::default()
        };
        let with = lay(&doc, &cfg, &mut fonts);
        assert_eq!(without.runs, with.runs);
    }

    #[test]
    fn a_gutter_wider_than_the_margin_steps_the_lines_right() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\n");
        let mut fonts = FontStore::new();
        let without = lay(&doc, &ViewConfig::default(), &mut fonts);
        let margin = metrics::MARGIN_RATIO * WIDTH as f32;
        let cfg = ViewConfig {
            gutter: margin + 40.0,
            ..ViewConfig::default()
        };
        let with = lay(&doc, &cfg, &mut fonts);
        assert_eq!(with.runs.len(), without.runs.len());
        for (a, b) in without.runs.iter().zip(&with.runs) {
            assert!((b.x - a.x - 40.0).abs() < 0.01, "{} then {}", a.x, b.x);
        }
        let seat = with.code_line_seat(0, 0).unwrap();
        assert!((seat.x - without.code_line_seat(0, 0).unwrap().x - 40.0).abs() < 0.01);
    }

    #[test]
    fn a_rendered_page_keeps_its_margin_whatever_the_gutter() {
        let doc = crate::doc::markdown::parse("# Title\n\nText.\n");
        let mut fonts = FontStore::new();
        let without = lay(&doc, &ViewConfig::default(), &mut fonts);
        let cfg = ViewConfig {
            gutter: 300.0,
            ..ViewConfig::default()
        };
        assert_eq!(without.runs, lay(&doc, &cfg, &mut fonts).runs);
    }

    #[test]
    fn the_margin_makes_room_for_the_number_of_the_row_after_the_final_newline() {
        let text = "x\n".repeat(999);
        let doc = load::code_document(None, &text);
        assert_eq!(last_number(&doc), 1000, "the caret's row after line 999");
        let doc = load::code_document(None, text.trim_end());
        assert_eq!(last_number(&doc), 999, "no row after a last line left open");
        let doc = load::text_document("");
        assert_eq!(last_number(&doc), 0);
    }

    #[test]
    fn the_row_after_the_final_newline_gets_a_strip_of_its_own() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let theme = Theme::default_dark();
        let page = paper(&doc, &theme);
        let last = lay.code_line_seat(0, 1).unwrap();
        let strip = strip(&mut fonts, &lay, &doc, 0, 2, page, theme.surface.foreground)
            .expect("the row below the last line");
        assert_eq!(strip.y, (last.y + last.height).floor());
        assert!(
            strip.pixels.iter().any(|&p| p != packed(page)),
            "the strip carries the number 3"
        );
    }

    /// The columns the band inked for a line's number, on the rows of
    /// that line's strip: the number's left and right edges.
    fn band_number_edges(numbered: &[u32], page: u32, strip: &Strip) -> (usize, usize) {
        let mut edges = (usize::MAX, 0);
        for row in 0..strip.height as usize {
            for x in 0..strip.width as usize {
                if numbered[(strip.y as usize + row) * WIDTH as usize + x] != page {
                    edges = (edges.0.min(x), edges.1.max(x));
                }
            }
        }
        edges
    }

    #[test]
    fn the_strip_puts_the_number_on_the_pixels_the_band_gave_it() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\nlet c = 3;\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let theme = Theme::default_dark();
        let (page, ink) = (paper(&doc, &theme), theme.syntax.punctuation);
        let numbered = painted(&doc, &lay, &mut fonts, true);
        let strip = strip(&mut fonts, &lay, &doc, 0, 1, page, ink).expect("the line is placed");
        let seat = lay.code_line_seat(0, 1).unwrap();
        assert_eq!(strip.y, seat.y.floor());
        assert!(
            (strip.width as f32) < seat.x,
            "the strip stays left of the text"
        );
        assert_eq!(strip.pixels.len(), (strip.width * strip.height) as usize);
        // The digits are the page's color cut out of the box: on the rows
        // the band drew them, and within the box's padding, whatever is
        // not the box is a digit, and its columns are the band's.
        let (left, right) = band_number_edges(&numbered, packed(page), &strip);
        let rows = (0..strip.height as usize).filter(|&row| {
            (left..=right)
                .any(|x| numbered[(strip.y as usize + row) * WIDTH as usize + x] != packed(page))
        });
        let mut digits = (usize::MAX, 0);
        for row in rows {
            for x in left - 3..=right + 3 {
                if strip.pixels[row * strip.width as usize + x] != packed(ink) {
                    digits = (digits.0.min(x), digits.1.max(x));
                }
            }
        }
        assert!(
            digits.0.abs_diff(left) <= 1,
            "left edge {digits:?} against {left}"
        );
        assert!(
            digits.1.abs_diff(right) <= 1,
            "right edge {digits:?} against {right}"
        );
    }

    #[test]
    fn the_active_number_stands_in_a_box_of_the_ink() {
        let doc = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\nlet c = 3;\n");
        let mut fonts = FontStore::new();
        let lay = lay(&doc, &ViewConfig::default(), &mut fonts);
        let theme = Theme::default_dark();
        let (page, ink) = (paper(&doc, &theme), theme.syntax.punctuation);
        let numbered = painted(&doc, &lay, &mut fonts, true);
        let strip = strip(&mut fonts, &lay, &doc, 0, 1, page, ink).expect("the line is placed");
        let (left, right) = band_number_edges(&numbered, packed(page), &strip);
        let middle = strip.height as usize / 2;
        let at = |x: usize, row: usize| strip.pixels[row * strip.width as usize + x];
        assert_eq!(at(left - 2, middle), packed(ink), "padded on the left");
        assert_eq!(at(right + 2, middle), packed(ink), "padded on the right");
        assert_eq!(at(0, middle), packed(page), "the box hugs the number");
        assert_eq!(at(right + 2, 0), packed(page), "and stays inside the row");
        assert_eq!(
            at(right + 2, strip.height as usize - 1),
            packed(page),
            "at both ends"
        );
    }
}
