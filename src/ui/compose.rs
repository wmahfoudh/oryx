//! The box of the composing text: what the system's input method is
//! still composing, drawn at the caret of what has the keys, in the
//! editor and in a text field. Paint only: the composing text is not
//! in the document or in a field's text.

use crate::input::method::Shown;
use crate::paint::painter::Painter;
use crate::style::theme::Rgba;

/// The box is wider than its text by this much on the left and on the
/// right.
const PAD_LEFT: f32 = 2.0;
const PAD_RIGHT: f32 = 3.0;

/// The edge of a box has the text's color at a quarter of its
/// strength: it shows on the box's color in a light theme and in a
/// dark one.
const EDGE_STRENGTH: u8 = 4;

/// Where a composing box is drawn, in the painter's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot<'a> {
    /// The caret's x: the composing text starts there when it fits.
    pub x: f32,
    /// The top and the height of the box.
    pub top: f32,
    pub height: f32,
    /// The top of the text's line, which puts the composing text on
    /// the baseline of the text beside it.
    pub text_top: f32,
    /// The left and the right side of the room the box stays in: the
    /// editor's pane, or a field's text.
    pub room: (f32, f32),
    /// The face of the text under the caret.
    pub family: &'a str,
    pub size: f32,
    pub weight: u16,
    /// The color of the box, and of its text, its lines and its bar.
    pub bg: Rgba,
    pub fg: Rgba,
    /// A thin line around the box, for a box of the color of what is
    /// under it: without the line, the text the box covers looks
    /// deleted.
    pub edged: bool,
}

/// A box placed in its room.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// The left side of the box and its width, inside the room.
    pub left: f32,
    pub width: f32,
    /// The x the text starts at. It is left of the box when the text
    /// is wider than the room and its end shows.
    pub text: f32,
}

/// Places the box of a text `width` wide whose caret is at `caret`,
/// in a room given by its two sides. `bar` is the x of the input
/// method's cursor from the text's start.
///
/// The text starts at the caret, over what follows it. A box that
/// would pass the room's right side is pushed left. A box wider than
/// the room fills it, and the end with the cursor shows: the text
/// starts at the room's left while the bar is in view there, and is
/// moved left until the bar is at the room's right otherwise.
pub fn place(caret: f32, width: f32, bar: f32, room: (f32, f32)) -> Placed {
    let (left, right) = room;
    let space = (right - left).max(0.0);
    let need = width + PAD_LEFT + PAD_RIGHT;
    if need <= space {
        let left = (caret - PAD_LEFT).clamp(left, right - need);
        return Placed {
            left,
            width: need,
            text: left + PAD_LEFT,
        };
    }
    let start = left + PAD_LEFT;
    let last = right - PAD_RIGHT;
    let text = if start + bar > last {
        last - bar
    } else {
        start
    };
    Placed {
        left,
        width: space,
        text,
    }
}

/// The input method's cursor in a composing text, in characters: the
/// boundary the bar is at, and the range under the stronger line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    pub bar: usize,
    pub strong: Option<(usize, usize)>,
}

/// Reads a cursor given in bytes (`Shown::cursor`). With no cursor the
/// bar is at the end of the text. A point is the bar alone. A range
/// has the bar at its end and the stronger line over it: the clause a
/// Japanese input method is converting, or the letters up to the caret
/// under fcitx5.
pub fn marks(text: &str, cursor: Option<(usize, usize)>) -> Marks {
    let chars = |end: usize| text.get(..end).map(|head| head.chars().count());
    let ends = cursor
        .filter(|&(from, to)| from <= to)
        .and_then(|(from, to)| Some((chars(from)?, chars(to)?)));
    match ends {
        Some((from, to)) => Marks {
            bar: to,
            strong: (from < to).then_some((from, to)),
        },
        None => Marks {
            bar: text.chars().count(),
            strong: None,
        },
    }
}

/// Draws the composing text in its box at `spot`: the text underlined,
/// the input method's cursor as a bar, a stronger line under its
/// range, and a thin line around the box where the spot asks for one.
/// The box covers what is under it, the text after the caret
/// included. The painter's clip is lifted at the end.
pub fn draw(painter: &mut Painter, shown: Shown, spot: Spot) {
    let width = painter.measure(shown.text, spot.family, spot.size, spot.weight);
    let positions = painter.boundaries(shown.text, spot.family, spot.size, spot.weight);
    let at = |boundary: usize| positions.get(boundary).copied().unwrap_or(width);
    let marks = marks(shown.text, shown.cursor);
    let placed = place(spot.x, width, at(marks.bar), spot.room);
    if placed.width <= 0.0 || spot.height <= 0.0 {
        return;
    }
    painter.fill(
        placed.left,
        spot.top,
        placed.width,
        spot.height,
        0.0,
        spot.bg,
    );
    painter.clip(Some((placed.left, spot.top, placed.width, spot.height)));
    painter.text(
        placed.text,
        spot.text_top,
        shown.text,
        spot.family,
        spot.size,
        spot.weight,
        spot.fg,
    );
    let under = spot.top + spot.height - 2.5;
    painter.line(placed.text, under, placed.text + width, under, 1.0, spot.fg);
    if let Some((from, to)) = marks.strong {
        // On a text read right to left the end is left of the start.
        let (x0, x1) = (at(from).min(at(to)), at(from).max(at(to)));
        painter.line(
            placed.text + x0,
            under - 0.5,
            placed.text + x1,
            under - 0.5,
            3.0,
            spot.fg,
        );
    }
    painter.fill(
        placed.text + at(marks.bar),
        spot.top + 3.0,
        1.5,
        (spot.height - 8.0).max(1.0),
        0.0,
        spot.fg,
    );
    if spot.edged && placed.width > 1.0 && spot.height > 1.0 {
        // The line is inside the rectangle, so the box is no larger.
        let edge = Rgba {
            a: spot.fg.a / EDGE_STRENGTH,
            ..spot.fg
        };
        painter.stroke(
            placed.left + 0.5,
            spot.top + 0.5,
            placed.width - 1.0,
            spot.height - 1.0,
            0.0,
            1.0,
            edge,
        );
    }
    painter.clip(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::fonts::{FontStore, CODE_FAMILY};
    use tiny_skia::Pixmap;

    const ROOM: (f32, f32) = (10.0, 500.0);

    #[test]
    fn a_box_that_fits_starts_at_the_caret() {
        let placed = place(100.0, 40.0, 40.0, ROOM);
        assert_eq!(placed.text, 100.0, "the text starts at the caret");
        assert_eq!(placed.left, 100.0 - PAD_LEFT);
        assert_eq!(placed.width, 40.0 + PAD_LEFT + PAD_RIGHT);
    }

    #[test]
    fn a_box_at_the_right_side_is_pushed_left() {
        let placed = place(480.0, 40.0, 40.0, ROOM);
        assert_eq!(
            placed.left + placed.width,
            500.0,
            "its right side on the room's"
        );
        assert_eq!(placed.text, placed.left + PAD_LEFT);
        assert_eq!(placed.width, 45.0);
    }

    #[test]
    fn a_box_at_the_left_side_stays_in_the_room() {
        let placed = place(10.0, 40.0, 40.0, ROOM);
        assert_eq!(placed.left, 10.0);
        assert_eq!(placed.text, 12.0);
    }

    #[test]
    fn a_box_wider_than_its_room_fills_it_and_shows_the_end_with_the_cursor() {
        // The bar at the end of the text: the end shows.
        let end = place(100.0, 600.0, 600.0, ROOM);
        assert_eq!((end.left, end.width), (10.0, 490.0));
        assert_eq!(
            end.text + 600.0,
            500.0 - PAD_RIGHT,
            "the bar at the room's right"
        );
        // The bar near the start: the start shows.
        let start = place(100.0, 600.0, 20.0, ROOM);
        assert_eq!((start.left, start.width), (10.0, 490.0));
        assert_eq!(start.text, 10.0 + PAD_LEFT);
        // The bar in the part that would be cut: moved into view.
        let middle = place(100.0, 600.0, 550.0, ROOM);
        assert_eq!(middle.text + 550.0, 500.0 - PAD_RIGHT);
    }

    #[test]
    fn a_room_with_no_width_gets_no_box() {
        assert_eq!(place(100.0, 40.0, 40.0, (300.0, 300.0)).width, 0.0);
        assert_eq!(place(100.0, 40.0, 40.0, (300.0, 200.0)).width, 0.0);
    }

    /// For each caret, width and cursor: the box is inside its room,
    /// and the bar is inside the box.
    #[test]
    fn the_box_and_its_bar_stay_in_the_room() {
        for caret in [-50.0, 10.0, 11.0, 250.0, 470.0, 499.0, 500.0, 900.0] {
            for width in [0.0, 1.0, 30.0, 484.0, 485.0, 486.0, 490.0, 2000.0] {
                for part in [0.0, 0.25, 0.5, 1.0] {
                    let bar = width * part;
                    let placed = place(caret, width, bar, ROOM);
                    let state = format!("caret {caret}, width {width}, bar {bar}: {placed:?}");
                    assert!(placed.left >= ROOM.0, "{state}");
                    assert!(placed.left + placed.width <= ROOM.1, "{state}");
                    let x = placed.text + bar;
                    assert!(
                        x >= placed.left && x <= placed.left + placed.width,
                        "{state}"
                    );
                }
            }
        }
    }

    #[test]
    fn with_no_cursor_the_bar_is_at_the_end_of_the_text() {
        let m = marks("你好", None);
        assert_eq!((m.bar, m.strong), (2, None));
    }

    #[test]
    fn a_point_is_the_bar_alone() {
        assert_eq!(
            marks("ni hao", Some((2, 2))),
            Marks {
                bar: 2,
                strong: None
            }
        );
        assert_eq!(
            marks("ni hao", Some((0, 0))),
            Marks {
                bar: 0,
                strong: None
            }
        );
        // The cursor is in bytes and the bar in characters.
        assert_eq!(
            marks("你好", Some((3, 3))),
            Marks {
                bar: 1,
                strong: None
            }
        );
        assert_eq!(
            marks("你好", Some((6, 6))),
            Marks {
                bar: 2,
                strong: None
            }
        );
    }

    #[test]
    fn a_range_has_the_bar_at_its_end_and_the_stronger_line_over_it() {
        // fcitx5 on Wayland: from the start to the caret.
        assert_eq!(
            marks("ni h", Some((0, 4))),
            Marks {
                bar: 4,
                strong: Some((0, 4))
            }
        );
        // The clause a Japanese input method converts.
        let text = "今日はいい天気";
        assert_eq!(
            marks(text, Some((9, 15))),
            Marks {
                bar: 5,
                strong: Some((3, 5))
            }
        );
    }

    /// `Shown` gives ends that are in order and on boundaries. A cursor
    /// that is not reads as no cursor, and nothing panics.
    #[test]
    fn a_cursor_off_the_text_reads_as_no_cursor() {
        let end = Marks {
            bar: 2,
            strong: None,
        };
        assert_eq!(marks("你好", Some((1, 1))), end, "inside a character");
        assert_eq!(marks("你好", Some((0, 9))), end, "past the end");
        assert_eq!(marks("你好", Some((6, 3))), end, "reversed");
    }

    fn spot(x: f32, room: (f32, f32)) -> Spot<'static> {
        Spot {
            x,
            top: 10.0,
            height: 24.0,
            text_top: 12.0,
            room,
            family: CODE_FAMILY,
            size: 15.0,
            weight: 400,
            bg: Rgba {
                r: 26,
                g: 32,
                b: 48,
                a: 255,
            },
            fg: Rgba {
                r: 201,
                g: 209,
                b: 227,
                a: 255,
            },
            edged: false,
        }
    }

    fn painted(shown: Shown, spot: Spot) -> Pixmap {
        let mut fonts = FontStore::new();
        let mut pixmap = Pixmap::new(300, 50).unwrap();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        draw(&mut painter, shown, spot);
        drop(painter);
        pixmap
    }

    /// The columns and the rows that hold a painted pixel.
    fn extent(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                if pixmap.pixel(x, y).is_some_and(|p| p.alpha() > 0) {
                    let (x0, y0, x1, y1) = found.unwrap_or((x, y, x, y));
                    found = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
                }
            }
        }
        found
    }

    #[test]
    fn the_box_is_painted_at_the_caret_over_its_height() {
        let shown = Shown {
            text: "ni",
            cursor: None,
        };
        let (x0, y0, x1, y1) = extent(&painted(shown, spot(100.0, (10.0, 290.0)))).unwrap();
        assert_eq!(
            (x0, y0, y1),
            (98, 10, 33),
            "from two pixels left of the caret, 24 high"
        );
        assert!(x1 > 110 && x1 < 130, "two letters wide: {x1}");
    }

    #[test]
    fn a_long_composing_text_is_painted_inside_its_room_only() {
        let text = "nihao".repeat(40);
        let shown = Shown {
            text: &text,
            cursor: None,
        };
        let (x0, _, x1, _) = extent(&painted(shown, spot(100.0, (60.0, 200.0)))).unwrap();
        assert_eq!((x0, x1), (60, 199), "the room and nothing beside it");
    }

    /// The edge is one pixel inside the box's rectangle, in the text's
    /// color made faint: the box is no larger with it, and its inside
    /// has the box's color.
    #[test]
    fn an_edged_box_has_a_faint_line_around_it_inside_its_rectangle() {
        let shown = Shown {
            text: "ni",
            cursor: None,
        };
        let plain = painted(shown, spot(100.0, (10.0, 290.0)));
        let edged = painted(
            shown,
            Spot {
                edged: true,
                ..spot(100.0, (10.0, 290.0))
            },
        );
        let (x0, y0, x1, y1) = extent(&plain).unwrap();
        assert_eq!(extent(&edged), Some((x0, y0, x1, y1)), "no larger");
        let color = |pixmap: &Pixmap, x: u32, y: u32| {
            let p = pixmap.pixel(x, y).unwrap();
            (p.red(), p.green(), p.blue())
        };
        let (bg, fg) = ((26, 32, 48), (201, 209, 227));
        let middle = (y0 + y1) / 2;
        for (x, y, side) in [
            (x0, middle, "left"),
            (x1, middle, "right"),
            (x0 + 1, y0, "top"),
            (x0 + 1, y1, "bottom"),
        ] {
            assert_eq!(color(&plain, x, y), bg, "{side}, no edge");
            let (r, g, b) = color(&edged, x, y);
            assert!(
                r > bg.0 + 20 && r < fg.0 - 60 && g > bg.1 + 20 && b > bg.2 + 20,
                "{side}: a faint line, ({r}, {g}, {b})"
            );
        }
        assert_eq!(color(&edged, x0 + 1, y0 + 1), bg, "the inside");
    }

    /// The bar is a column of the text's color over the box's height,
    /// at the end with no cursor and at the cursor with one.
    #[test]
    fn the_bar_is_painted_where_the_cursor_is() {
        let column = |pixmap: &Pixmap, x: u32| {
            (14..24).all(|y| pixmap.pixel(x, y).is_some_and(|p| p.red() > 150))
        };
        let end = painted(
            Shown {
                text: "nnnn",
                cursor: None,
            },
            spot(100.0, (10.0, 290.0)),
        );
        let start = painted(
            Shown {
                text: "nnnn",
                cursor: Some((0, 0)),
            },
            spot(100.0, (10.0, 290.0)),
        );
        assert!(
            column(&start, 100),
            "the bar at the caret's x with the cursor at 0"
        );
        assert!(!column(&end, 100), "and not there with no cursor");
        let (_, _, x1, _) = extent(&end).unwrap();
        assert!(
            (x1 - 6..=x1).any(|x| column(&end, x)),
            "the bar at the end of the text with no cursor"
        );
    }

    #[test]
    fn a_range_is_painted_with_a_stronger_line() {
        let lit = |pixmap: &Pixmap| {
            let mut n = 0;
            for y in 29..34 {
                for x in 0..pixmap.width() {
                    n += u32::from(pixmap.pixel(x, y).is_some_and(|p| p.red() > 150));
                }
            }
            n
        };
        let point = painted(
            Shown {
                text: "nnnn",
                cursor: Some((4, 4)),
            },
            spot(100.0, (10.0, 290.0)),
        );
        let range = painted(
            Shown {
                text: "nnnn",
                cursor: Some((0, 4)),
            },
            spot(100.0, (10.0, 290.0)),
        );
        assert!(
            lit(&range) > lit(&point) + 20,
            "{} against {}",
            lit(&range),
            lit(&point)
        );
    }

    #[test]
    fn a_room_with_no_width_is_painted_with_nothing() {
        let shown = Shown {
            text: "ni",
            cursor: None,
        };
        assert_eq!(extent(&painted(shown, spot(100.0, (100.0, 100.0)))), None);
    }
}
