//! Locked layout metrics. Every value scales with the font size it applies
//! to, so zoom changes the whole rhythm proportionally.

/// Line height as a multiple of font size.
pub const LINE_HEIGHT: f32 = 1.5;

/// Each side margin as a fraction of viewport width.
pub const MARGIN_RATIO: f32 = 0.08;

/// Top and bottom margin as a multiple of the body font size.
pub const VERTICAL_MARGIN_EM: f32 = 2.0;

/// Heading size as a multiple of the body size.
pub fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 2.0,
        2 => 1.6,
        3 => 1.35,
        4 => 1.15,
        5 => 1.0,
        _ => 0.9,
    }
}

/// Space above a block, from its own font size. Doubled for h1 and h2.
pub fn space_above(heading_level: Option<u8>, size: f32) -> f32 {
    match heading_level {
        Some(1) | Some(2) => 1.5 * size,
        _ => 0.75 * size,
    }
}

/// Space below a block, from its own font size.
pub fn space_below(size: f32) -> f32 {
    0.35 * size
}
