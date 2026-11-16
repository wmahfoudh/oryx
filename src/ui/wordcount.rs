//! The corner word count: a small standing line at the page's bottom
//! right while the setting is on. It draws in the theme's comment color,
//! the one quiet text color every theme keeps readable, on a plate of
//! the page's own color (`paint::paper`), so the text scrolling under it
//! never crosses its letters. It takes no key and no click.

use std::time::Duration;

use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::scrollbar::STRIP_WIDTH;

/// How long a change rests before the file is counted again, so a burst
/// of typing or a selection drag costs one count at its end.
pub const REST: Duration = Duration::from_millis(150);

pub const SIZE: f32 = 12.0;
const PAD: f32 = 8.0;
const HEIGHT: f32 = 22.0;
const RADIUS: f32 = 6.0;
/// Clear of the scrollbar's strip on the right, close to the bottom.
const RIGHT: f32 = STRIP_WIDTH + 4.0;
const BOTTOM: f32 = 8.0;

/// The plate's left, top and width for a text `text_w` wide in a window
/// `width` by `height`. A window narrower than the line keeps the
/// line's start in view.
pub fn place(text_w: f32, width: f32, height: f32) -> (f32, f32, f32) {
    let w = text_w + 2.0 * PAD;
    let x = (width - w - RIGHT).max(0.0);
    (x, height - HEIGHT - BOTTOM, w)
}

/// The busy dot's width and height.
pub const DOT: f32 = 8.0;
/// Between the dot and the count's text.
const DOT_GAP: f32 = 6.0;

/// Where the parts of the corner go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// The plate's left, top and width.
    pub plate: (f32, f32, f32),
    /// The dot's left and top, when it shows.
    pub dot: Option<(f32, f32)>,
    /// The text's left.
    pub text_x: f32,
}

/// The corner for a count `text_w` wide, if one shows, and the busy
/// dot, if it shows: the dot is at the left of the count, on the same
/// plate, and alone on a small plate when there is no count. None when
/// neither shows.
pub fn corner(text_w: Option<f32>, dot: bool, width: f32, height: f32) -> Option<Corner> {
    let before_text = match (dot, text_w) {
        (false, None) => return None,
        (false, Some(_)) => 0.0,
        (true, None) => DOT,
        (true, Some(_)) => DOT + DOT_GAP,
    };
    let plate = place(before_text + text_w.unwrap_or(0.0), width, height);
    let (x, y, _) = plate;
    Some(Corner {
        plate,
        dot: dot.then_some((x + PAD, y + (HEIGHT - DOT) / 2.0)),
        text_x: x + PAD + before_text,
    })
}

/// Draws the corner: the count's text when there is one, the busy dot
/// when `dot` is set. The dot takes the theme's `strike` color.
pub fn draw(
    painter: &mut Painter,
    theme: &Theme,
    text: Option<&str>,
    dot: bool,
    paper: Rgba,
    width: f32,
    height: f32,
) {
    let text_w = text.map(|text| painter.measure(text, BODY_FAMILY, SIZE, 400));
    let Some(corner) = corner(text_w, dot, width, height) else {
        return;
    };
    let (x, y, w) = corner.plate;
    painter.fill(x, y, w, HEIGHT, RADIUS, paper);
    if let Some((dot_x, dot_y)) = corner.dot {
        painter.fill(dot_x, dot_y, DOT, DOT, DOT / 2.0, theme.text.strike);
    }
    if let Some(text) = text {
        painter.text(
            corner.text_x,
            y + 3.0,
            text,
            BODY_FAMILY,
            SIZE,
            400,
            theme.syntax.comment,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_sits_in_the_bottom_right_clear_of_the_scrollbar() {
        let (x, y, w) = place(300.0, 1200.0, 800.0);
        assert_eq!(w, 316.0);
        assert_eq!(x + w, 1200.0 - STRIP_WIDTH - 4.0);
        assert_eq!(y + HEIGHT, 800.0 - BOTTOM);
    }

    #[test]
    fn a_narrow_window_keeps_the_start_of_the_line() {
        let (x, _, _) = place(300.0, 200.0, 800.0);
        assert_eq!(x, 0.0);
    }

    #[test]
    fn the_count_without_a_dot_is_placed_as_before() {
        let corner = corner(Some(300.0), false, 1200.0, 800.0).unwrap();
        assert_eq!(corner.plate, place(300.0, 1200.0, 800.0));
        assert_eq!(corner.dot, None);
        assert_eq!(corner.text_x, corner.plate.0 + PAD);
    }

    #[test]
    fn the_dot_is_at_the_left_of_the_count_on_its_plate() {
        let corner = corner(Some(300.0), true, 1200.0, 800.0).unwrap();
        let (x, y, w) = corner.plate;
        assert_eq!(w, 300.0 + DOT + DOT_GAP + 2.0 * PAD);
        assert_eq!(x + w, 1200.0 - STRIP_WIDTH - 4.0);
        let (dot_x, dot_y) = corner.dot.unwrap();
        assert_eq!(dot_x, x + PAD);
        assert_eq!(dot_y + DOT / 2.0, y + HEIGHT / 2.0);
        assert_eq!(corner.text_x, dot_x + DOT + DOT_GAP);
    }

    #[test]
    fn the_dot_alone_is_in_the_corner() {
        let corner = corner(None, true, 1200.0, 800.0).unwrap();
        let (x, y, w) = corner.plate;
        assert_eq!(w, DOT + 2.0 * PAD);
        assert_eq!(x + w, 1200.0 - STRIP_WIDTH - 4.0);
        assert_eq!(y + HEIGHT, 800.0 - BOTTOM);
        assert_eq!(corner.dot, Some((x + PAD, y + (HEIGHT - DOT) / 2.0)));
    }

    #[test]
    fn a_corner_with_no_count_and_no_dot_is_nothing() {
        assert_eq!(corner(None, false, 1200.0, 800.0), None);
    }
}
