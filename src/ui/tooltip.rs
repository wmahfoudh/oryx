//! The tooltip: a small pill beside the cursor carrying the expansion
//! of the abbreviation under it. It follows the cursor, takes no key,
//! and leaves when the cursor does.

use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::Theme;

pub const SIZE: f32 = 13.0;
const PAD: f32 = 10.0;
const HEIGHT: f32 = 28.0;
const RADIUS: f32 = 6.0;
/// Distance from the cursor's point to the pill's corner, clear of the
/// arrow's own shape.
const OFFSET: f32 = 16.0;
const MARGIN: f32 = 8.0;

/// Where the pill sits for a text `text_w` wide with the cursor at
/// (`cx`, `cy`) in a window `width` by `height`: below and right of the
/// cursor, pulled left or up when the window's edge is near. Returns
/// the pill's left, top and width.
pub fn place(text_w: f32, cx: f32, cy: f32, width: f32, height: f32) -> (f32, f32, f32) {
    let w = text_w + 2.0 * PAD;
    let x = (cx + OFFSET).min(width - w - MARGIN).max(MARGIN);
    let y = if cy + OFFSET + HEIGHT + MARGIN <= height {
        cy + OFFSET
    } else {
        (cy - OFFSET - HEIGHT).max(MARGIN)
    };
    (x, y, w)
}

/// Draws the pill for `text` beside the cursor at (`cx`, `cy`), in the
/// theme's overlay colors.
pub fn draw(
    painter: &mut Painter,
    theme: &Theme,
    text: &str,
    cx: f32,
    cy: f32,
    width: f32,
    height: f32,
) {
    let text_w = painter.measure(text, BODY_FAMILY, SIZE, 400);
    let (x, y, w) = place(text_w, cx, cy, width, height);
    painter.fill(x, y, w, HEIGHT, RADIUS, theme.ui.overlay_bg);
    painter.stroke(x, y, w, HEIGHT, RADIUS, 1.0, theme.blocks.table_border);
    painter.text(
        x + PAD,
        y + 6.0,
        text,
        BODY_FAMILY,
        SIZE,
        400,
        theme.ui.overlay_fg,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_sits_below_right_of_the_cursor_and_stays_inside() {
        let (x, y, w) = place(100.0, 200.0, 100.0, 800.0, 600.0);
        assert_eq!((x, y, w), (216.0, 116.0, 120.0));
        let (x, _, _) = place(100.0, 780.0, 100.0, 800.0, 600.0);
        assert_eq!(x + w + MARGIN, 800.0, "pulled back from the right edge");
        let (_, y, _) = place(100.0, 200.0, 590.0, 800.0, 600.0);
        assert_eq!(
            y + HEIGHT + OFFSET,
            590.0,
            "flipped above the cursor at the bottom"
        );
    }
}
