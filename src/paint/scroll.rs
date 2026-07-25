//! Scroll clamping and the band cache. Scrolling inside the band is a
//! memcpy slice; the band repaints recentered only near its edges.

use std::time::Duration;

use crate::doc::images::MediaCache;
use crate::layout::{DecoRect, LayoutDoc};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

pub fn clamp(y: f32, doc_height: f32, viewport_h: f32) -> f32 {
    y.clamp(0.0, (doc_height - viewport_h).max(0.0))
}

/// How long a window size holds still before a deferred relayout runs.
pub const SETTLE: Duration = Duration::from_millis(150);

/// A scroll position held while the pass streams is restorable once the
/// placed document is tall enough to show it, which is exactly when
/// clamping leaves it alone.
pub fn reached(target: f32, doc_height: f32, viewport_h: f32) -> bool {
    clamp(target, doc_height, viewport_h) >= target
}

/// A resize drag delivers a new width per frame. Restarting a pass that
/// cannot finish inside one slice would strand the reader at the top for
/// the whole drag, so the current layout is kept until the size settles.
pub fn defer_relayout(last_pass: Duration, slice: Duration) -> bool {
    last_pass > slice
}

/// Painted pixels for `[y_top, y_top + height)` at full window width,
/// covering the viewport plus two viewport heights above and below.
pub struct BandCache {
    pub pixels: Vec<u32>,
    pub y_top: f32,
    pub width: u32,
    pub height: u32,
    pub doc_height: f32,
}

impl BandCache {
    /// Paints a band recentered on `scroll_y`: five viewport heights,
    /// clamped so it never starts above the document top.
    #[allow(clippy::too_many_arguments)]
    pub fn repaint(
        layout: &LayoutDoc,
        theme: &Theme,
        fonts: &mut FontStore,
        media: &mut MediaCache,
        extra: &[DecoRect],
        scroll_y: f32,
        width: u32,
        viewport_h: u32,
    ) -> BandCache {
        let height = viewport_h * 5;
        let doc_height = layout.height;
        let max_top = (doc_height - height as f32).max(0.0);
        let y_top = (scroll_y - (2 * viewport_h) as f32)
            .clamp(0.0, max_top)
            .floor();
        let pixels = super::band(layout, theme, fonts, media, extra, y_top, width, height);
        BandCache {
            pixels,
            y_top,
            width,
            height,
            doc_height,
        }
    }

    /// True when the viewport nears a band edge that is not a document edge.
    pub fn needs_repaint(&self, scroll_y: f32, viewport_h: f32) -> bool {
        let bottom = self.y_top + self.height as f32;
        let view_bottom = scroll_y + viewport_h;
        if scroll_y < self.y_top || view_bottom > bottom {
            return true;
        }
        let margin = viewport_h * 0.5;
        (scroll_y - self.y_top < margin && self.y_top > 0.0)
            || (bottom - view_bottom < margin && bottom < self.doc_height)
    }

    /// The viewport slice at `scroll_y`, without repainting.
    pub fn view(&self, scroll_y: f32, viewport_h: u32) -> &[u32] {
        let offset_rows =
            (((scroll_y - self.y_top).max(0.0)) as u32).min(self.height.saturating_sub(viewport_h));
        let start = (offset_rows * self.width) as usize;
        let end = start + (viewport_h * self.width) as usize;
        &self.pixels[start..end.min(self.pixels.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_bounds() {
        assert_eq!(clamp(-10.0, 1000.0, 300.0), 0.0);
        assert_eq!(clamp(2000.0, 1000.0, 300.0), 700.0);
        assert_eq!(clamp(100.0, 200.0, 300.0), 0.0);
    }

    #[test]
    fn a_target_is_reached_once_the_placed_document_shows_it() {
        // 700px of placed document under a 300px viewport scrolls to 400.
        assert!(!reached(500.0, 700.0, 300.0));
        assert!(reached(400.0, 700.0, 300.0));
        assert!(reached(500.0, 800.0, 300.0));
        // The top is always reachable, including in an empty document.
        assert!(reached(0.0, 0.0, 300.0));
    }

    #[test]
    fn relayout_defers_only_when_a_pass_outlasts_a_slice() {
        let slice = Duration::from_millis(16);
        assert!(!defer_relayout(Duration::from_millis(5), slice));
        assert!(!defer_relayout(slice, slice));
        assert!(defer_relayout(Duration::from_millis(17), slice));
        assert!(defer_relayout(Duration::from_secs(5), slice));
    }

    fn band(y_top: f32, height: u32, doc_height: f32) -> BandCache {
        BandCache {
            pixels: Vec::new(),
            y_top,
            width: 1,
            height,
            doc_height,
        }
    }

    #[test]
    fn no_repaint_at_band_center() {
        let b = band(1000.0, 1500, 10000.0);
        assert!(!b.needs_repaint(1600.0, 300.0));
    }

    #[test]
    fn repaint_near_inner_edges() {
        let b = band(1000.0, 1500, 10000.0);
        assert!(b.needs_repaint(1100.0, 300.0));
        assert!(b.needs_repaint(2150.0, 300.0));
    }

    #[test]
    fn no_repaint_at_document_edges() {
        let top = band(0.0, 1500, 10000.0);
        assert!(!top.needs_repaint(0.0, 300.0));
        let bottom = band(8500.0, 1500, 10000.0);
        assert!(!bottom.needs_repaint(9700.0, 300.0));
    }

    #[test]
    fn view_slices_exact_rows() {
        let width = 4u32;
        let rows = 10u32;
        let mut pixels = Vec::new();
        for row in 0..rows {
            pixels.extend(std::iter::repeat_n(row, width as usize));
        }
        let b = BandCache {
            pixels,
            y_top: 100.0,
            width,
            height: rows,
            doc_height: 1000.0,
        };
        let view = b.view(103.0, 2);
        assert_eq!(view.len(), 8);
        assert_eq!(view[0], 3);
        assert_eq!(view[7], 4);
    }
}
