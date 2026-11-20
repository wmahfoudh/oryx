mod engine;
pub mod metrics;
pub mod pool;
mod rows;

/// A vertical position or height in the document's space: a top, a
/// baseline, the scroll, a sum of heights. What the shaper or a font
/// gives for one line or one glyph stays f32.
pub type DocY = f32;

pub use pool::{ShapeCtx, ShapePool, StepKey};

pub(crate) use engine::{alert_color, alert_title, role_color};

pub use engine::{
    code_framed, code_lines_in, edit_code_lines, layout, layout_begin, layout_extend, layout_more,
    layout_step, line_face, math_display, recolor_batch, recolor_code_lines, window_to, CodeLine,
    ComicFit, DecoRect, DirectionMode, ImagePlace, LayoutDoc, LayoutPass, LineSeat, MathGlyph,
    TableRow, TextRef, TextRun, ViewConfig, OPEN_SLICE, SLICE,
};
