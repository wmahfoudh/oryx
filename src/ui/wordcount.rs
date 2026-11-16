//! The corner word count: a small standing line at the page's bottom
//! right while the setting is on. It draws in the theme's comment color,
//! the one quiet text color every theme keeps readable, on a plate of
//! the page's own color (`paint::paper`), so the text scrolling under it
//! never crosses its letters. It takes no key and no click.
//!
//! The busy dot is drawn here too, at the right end of the same plate,
//! or on a small plate when there is no count. `ui::busy` says when it
//! shows.

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
/// How far the dot's middle is above the plate's: the middle of the
/// count's digits, measured on a picture.
const DOT_RISE: f32 = 2.0;

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
/// dot, if it shows. The dot is at the right end of the plate, at the
/// same place with or without a count, and a count leaves it its room
/// when it is out, so the text does not move when the dot comes and
/// goes. None when neither shows.
pub fn corner(text_w: Option<f32>, dot: bool, width: f32, height: f32) -> Option<Corner> {
    if text_w.is_none() && !dot {
        return None;
    }
    let text_room = text_w.map_or(0.0, |text_w| text_w + DOT_GAP);
    let plate = place(text_room + DOT, width, height);
    let (x, y, w) = plate;
    Some(Corner {
        plate,
        dot: dot.then_some((x + w - PAD - DOT, y + (HEIGHT - DOT) / 2.0 - DOT_RISE)),
        text_x: x + PAD,
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
    fn the_count_stays_in_place_when_the_dot_comes_and_goes() {
        let idle = corner(Some(300.0), false, 1200.0, 800.0).unwrap();
        let busy = corner(Some(300.0), true, 1200.0, 800.0).unwrap();
        assert_eq!(idle.plate, busy.plate);
        assert_eq!(idle.text_x, busy.text_x);
        assert_eq!(idle.text_x, idle.plate.0 + PAD);
        assert_eq!(idle.dot, None);
        assert!(busy.dot.is_some());
    }

    #[test]
    fn the_dot_is_at_the_right_of_the_count_on_its_plate() {
        let corner = corner(Some(300.0), true, 1200.0, 800.0).unwrap();
        let (x, _, w) = corner.plate;
        assert_eq!(w, 300.0 + DOT_GAP + DOT + 2.0 * PAD);
        assert_eq!(x + w, 1200.0 - STRIP_WIDTH - 4.0);
        let (dot_x, _) = corner.dot.unwrap();
        assert_eq!(dot_x, corner.text_x + 300.0 + DOT_GAP);
        assert_eq!(dot_x + DOT, x + w - PAD);
    }

    #[test]
    fn the_dot_alone_is_where_it_is_beside_a_count() {
        let alone = corner(None, true, 1200.0, 800.0).unwrap();
        let (x, y, w) = alone.plate;
        assert_eq!(w, DOT + 2.0 * PAD);
        assert_eq!(x + w, 1200.0 - STRIP_WIDTH - 4.0);
        assert_eq!(y + HEIGHT, 800.0 - BOTTOM);
        let beside = corner(Some(300.0), true, 1200.0, 800.0).unwrap();
        assert_eq!(alone.dot, beside.dot);
    }

    #[test]
    fn the_dot_is_at_the_height_of_the_digits() {
        let corner = corner(None, true, 1200.0, 800.0).unwrap();
        let (_, y, _) = corner.plate;
        let (_, dot_y) = corner.dot.unwrap();
        assert_eq!(dot_y + DOT / 2.0, y + HEIGHT / 2.0 - DOT_RISE);
        assert_eq!(DOT_RISE, 2.0);
    }

    #[test]
    fn a_corner_with_no_count_and_no_dot_is_nothing() {
        assert_eq!(corner(None, false, 1200.0, 800.0), None);
    }
}
