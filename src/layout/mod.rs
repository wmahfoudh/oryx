mod engine;
pub mod metrics;
pub mod pool;
mod rows;

/// A vertical position or height in the document's space: a top, a
/// baseline, the scroll, a sum of heights. Past 2^24 px an f32 cannot
/// hold each pixel, and a huge file is that tall far down. What the
/// shaper or a font gives for one line or one glyph stays f32.
pub type DocY = f64;

/// A document height as a distance from `origin`, in the f32 that
/// tiny-skia, cosmic-text and the PDF writer take. The subtraction is
/// made in `DocY` first: far down a huge file both heights are large,
/// while their difference near the screen or the page is small.
pub fn relative_y(y: DocY, origin: DocY) -> f32 {
    (y - origin) as f32
}

/// A vertical length the caller knows is short (a row, a piece of a
/// page, a distance clamped to the view), in f32. The debug build checks
/// that it is under 2^24 px, where an f32 still holds each pixel.
pub fn length_f32(len: DocY) -> f32 {
    debug_assert!(
        len.abs() < 16_777_216.0,
        "a length of {len} px is not short"
    );
    len as f32
}

pub use pool::{ShapeCtx, ShapePool, StepKey};

pub(crate) use engine::{alert_color, alert_title, role_color};

pub use engine::{
    code_framed, code_lines_in, edit_code_lines, layout, layout_begin, layout_extend, layout_more,
    layout_step, line_face, math_display, recolor_batch, recolor_code_lines, window_to, CodeLine,
    ComicFit, DecoRect, DirectionMode, ImagePlace, LayoutDoc, LayoutPass, LineSeat, MathGlyph,
    TableRow, TextRef, TextRun, ViewConfig, KEY_SHAPE_BYTES, OPEN_SLICE, SLICE,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_height_is_taken_from_its_origin() {
        assert_eq!(relative_y(1_000.75, 990.25), 10.5);
        assert_eq!(relative_y(990.25, 1_000.75), -10.5);
    }

    #[test]
    fn a_short_length_goes_back_to_f32_whole() {
        assert_eq!(length_f32(25.5), 25.5);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "is not short")]
    fn a_length_past_two_to_the_24_is_refused_in_the_debug_build() {
        length_f32(20_000_000.0);
    }
}
