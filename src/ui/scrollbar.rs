//! Scrollbar geometry and drawing over the presented frame.

use crate::style::theme::Rgba;

/// Total scrollbar strip width at the right window edge.
pub const STRIP_WIDTH: f32 = 12.0;
/// Painted thumb width inside the strip.
pub const THUMB_WIDTH: u32 = 8;
pub const MIN_THUMB: f32 = 24.0;

/// Thumb `(y, height)` in track pixels. None when the document fits.
pub fn thumb(doc_height: f32, viewport_h: f32, scroll_y: f32, track_h: f32) -> Option<(f32, f32)> {
    if doc_height <= viewport_h {
        return None;
    }
    let height = ((viewport_h / doc_height) * track_h)
        .max(MIN_THUMB)
        .min(track_h);
    let max_scroll = (doc_height - viewport_h).max(1.0);
    let y = (scroll_y / max_scroll).clamp(0.0, 1.0) * (track_h - height);
    Some((y, height))
}

/// Maps a thumb y position back to a scroll offset.
pub fn scroll_for_thumb(
    thumb_y: f32,
    thumb_h: f32,
    track_h: f32,
    doc_height: f32,
    viewport_h: f32,
) -> f32 {
    let span = (track_h - thumb_h).max(1.0);
    (thumb_y / span).clamp(0.0, 1.0) * (doc_height - viewport_h).max(0.0)
}

/// Fills the thumb into a 0RGB frame of `width x height`.
pub fn draw(frame: &mut [u32], width: u32, height: u32, thumb: (f32, f32), color: Rgba) {
    let value = ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
    let x0 = width.saturating_sub(THUMB_WIDTH + 2);
    let y0 = (thumb.0.max(0.0) as u32).min(height);
    let y1 = ((thumb.0 + thumb.1).max(0.0) as u32).min(height);
    for y in y0..y1 {
        let row = (y * width) as usize;
        for x in x0..width.saturating_sub(2) {
            frame[row + x as usize] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_thumb_when_document_fits() {
        assert_eq!(thumb(200.0, 300.0, 0.0, 300.0), None);
    }

    #[test]
    fn thumb_is_proportional() {
        let (y, h) = thumb(1200.0, 300.0, 0.0, 300.0).unwrap();
        assert_eq!(y, 0.0);
        assert!((h - 75.0).abs() < 0.5);
    }

    #[test]
    fn thumb_reaches_track_end_at_max_scroll() {
        let (y, h) = thumb(1200.0, 300.0, 900.0, 300.0).unwrap();
        assert!((y + h - 300.0).abs() < 0.5);
    }

    #[test]
    fn thumb_round_trips_to_scroll() {
        let (y, h) = thumb(1200.0, 300.0, 450.0, 300.0).unwrap();
        let back = scroll_for_thumb(y, h, 300.0, 1200.0, 300.0);
        assert!((back - 450.0).abs() < 1.0);
    }
}
