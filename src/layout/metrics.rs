//! Locked layout metrics. Every value scales with the font size it applies
//! to, so zoom changes the whole rhythm proportionally.

/// Line height as a multiple of font size.
pub const LINE_HEIGHT: f32 = 1.5;

/// Each side margin as a fraction of viewport width.
pub const MARGIN_RATIO: f32 = 0.08;

/// Top and bottom margin as a multiple of the body font size.
pub const VERTICAL_MARGIN_EM: f32 = 2.0;

/// Indent step for quote depth and list nesting, in pixels at zoom 1.
pub const INDENT: f32 = 24.0;

/// Corner radius for panels and table outlines, in pixels at zoom 1.
pub const CORNER_RADIUS: f32 = 6.0;

/// The body size an image's pixels are taken to be drawn for. An image
/// carries a size in pixels and nothing else, so it needs a reading size
/// to be a proportion of; at the default it keeps its natural size, and
/// smaller or larger text scales it to match.
pub const REFERENCE_BODY: f32 = 22.0;

/// Inner padding of a code panel. Pagination needs it: a panel continued
/// on the next page starts one padding above its first line.
pub const CODE_PAD: f32 = 12.0;

/// Inner padding of a missing image's placeholder box. Pagination needs
/// it: the box travels whole with the alt text it holds, one padding
/// above and below.
pub const PLACEHOLDER_PAD: f32 = 12.0;

/// Corner radius for inline code pills, in pixels at zoom 1.
pub const PILL_RADIUS: f32 = 4.0;

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
