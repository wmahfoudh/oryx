//! Overlay painter: rounded panels and single-line text drawn into a
//! transparent pixmap, composited over the finished frame. Text assumes an
//! opaque panel beneath it; glyphs over transparent pixels are undefined.
//!
//! Callers draw in logical units; the painter multiplies by the display
//! scale, so chrome code never sees the monitor's density. Text is shaped
//! at the physical size, never upscaled from a smaller raster.

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Weight};
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Pixmap, Transform};

use crate::style::fonts::FontStore;
use crate::style::theme::Rgba;

pub struct Painter<'a> {
    pixmap: &'a mut Pixmap,
    fonts: &'a mut FontStore,
    /// Bounds of everything painted, in physical pixels, so composite
    /// touches only those rows.
    dirty: Option<(f32, f32, f32, f32)>,
    /// Physical pixels per logical unit.
    scale: f32,
    /// The clip in physical pixels, `(x0, y0, x1, y1)` with the far
    /// edges exclusive; nothing paints outside it while it is set.
    clip: Option<(i32, i32, i32, i32)>,
    /// The clip as a coverage mask, built by the first shape that
    /// crosses the clip's edge and kept until the clip changes.
    mask: Option<Mask>,
}

/// How much of a shape's bounding box the clip lets through.
enum Reach {
    /// No clip, or the shape lies inside it: painted as it is.
    Whole,
    /// The shape lies outside the clip: nothing to paint.
    None,
    /// The shape crosses the clip's edge: painted through the mask.
    Edge,
}

impl<'a> Painter<'a> {
    /// Wraps a reused canvas; `stale` is the region the previous frame
    /// painted, in physical pixels, wiped back to transparent before
    /// drawing starts.
    pub fn new(
        pixmap: &'a mut Pixmap,
        fonts: &'a mut FontStore,
        stale: Option<(f32, f32, f32, f32)>,
        scale: f32,
    ) -> Painter<'a> {
        if let Some((x0, y0, x1, y1)) = stale {
            let width = pixmap.width() as usize;
            let height = pixmap.height() as usize;
            let x0 = (x0.floor().max(0.0) as usize).min(width);
            let y0 = (y0.floor().max(0.0) as usize).min(height);
            let x1 = (x1.ceil().max(0.0) as usize).min(width);
            let y1 = (y1.ceil().max(0.0) as usize).min(height);
            let data = pixmap.data_mut();
            for y in y0..y1 {
                let row = (y * width + x0) * 4..(y * width + x1) * 4;
                data[row].fill(0);
            }
        }
        Painter {
            pixmap,
            fonts,
            dirty: None,
            scale,
            clip: None,
            mask: None,
        }
    }

    /// Limits every following paint to `rect`, given in logical units
    /// as origin, width and height, until the next call; `None` lifts
    /// the limit. The edges land on whole physical pixels.
    pub fn clip(&mut self, rect: Option<(f32, f32, f32, f32)>) {
        let s = self.scale;
        let (width, height) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        self.clip = rect.map(|(x, y, w, h)| {
            (
                ((x * s).round() as i32).clamp(0, width),
                ((y * s).round() as i32).clamp(0, height),
                (((x + w) * s).round() as i32).clamp(0, width),
                (((y + h) * s).round() as i32).clamp(0, height),
            )
        });
        self.mask = None;
    }

    /// The painted area in physical pixels, `(x0, y0, x1, y1)`: the
    /// pixmap, cut down to the clip when one is set.
    fn bounds(&self) -> (i32, i32, i32, i32) {
        self.clip.unwrap_or((
            0,
            0,
            self.pixmap.width() as i32,
            self.pixmap.height() as i32,
        ))
    }

    fn reach(&self, x: f32, y: f32, w: f32, h: f32) -> Reach {
        let Some((cx0, cy0, cx1, cy1)) = self.clip else {
            return Reach::Whole;
        };
        let (cx0, cy0, cx1, cy1) = (cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32);
        if x + w <= cx0 || x >= cx1 || y + h <= cy0 || y >= cy1 {
            Reach::None
        } else if x >= cx0 && y >= cy0 && x + w <= cx1 && y + h <= cy1 {
            Reach::Whole
        } else {
            Reach::Edge
        }
    }

    /// Fills `path` with `paint`, through the clip's mask when the
    /// shape's bounding box crosses the clip's edge.
    fn fill_shape(&mut self, path: &Path, paint: &tiny_skia::Paint, reach: Reach) {
        let mask = match reach {
            Reach::Edge => clip_mask(
                &mut self.mask,
                self.clip,
                self.pixmap.width(),
                self.pixmap.height(),
            ),
            _ => None,
        };
        self.pixmap
            .fill_path(path, paint, FillRule::Winding, Transform::identity(), mask);
    }

    fn stroke_shape(
        &mut self,
        path: &Path,
        paint: &tiny_skia::Paint,
        stroke: &tiny_skia::Stroke,
        reach: Reach,
    ) {
        let mask = match reach {
            Reach::Edge => clip_mask(
                &mut self.mask,
                self.clip,
                self.pixmap.width(),
                self.pixmap.height(),
            ),
            _ => None,
        };
        self.pixmap
            .stroke_path(path, paint, stroke, Transform::identity(), mask);
    }

    /// The painted bounds of this frame, to pass back as `stale` next time.
    pub fn dirty(&self) -> Option<(f32, f32, f32, f32)> {
        self.dirty
    }

    pub fn width(&self) -> f32 {
        self.pixmap.width() as f32 / self.scale
    }

    pub fn height(&self) -> f32 {
        self.pixmap.height() as f32 / self.scale
    }

    fn mark(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let mut region = (x, y, x + w, y + h);
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            region = (
                region.0.max(cx0 as f32),
                region.1.max(cy0 as f32),
                region.2.min(cx1 as f32),
                region.3.min(cy1 as f32),
            );
            if region.0 >= region.2 || region.1 >= region.3 {
                return;
            }
        }
        self.dirty = Some(match self.dirty {
            None => region,
            Some((x0, y0, x1, y1)) => (
                x0.min(region.0),
                y0.min(region.1),
                x1.max(region.2),
                y1.max(region.3),
            ),
        });
    }

    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Rgba) {
        let s = self.scale;
        let (x, y, w, h, radius) = (x * s, y * s, w * s, h * s, radius * s);
        let Some(path) = round_rect(x, y, w, h, radius) else {
            return;
        };
        let reach = self.reach(x, y, w, h);
        if matches!(reach, Reach::None) {
            return;
        }
        self.mark(x, y, w, h);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        self.fill_shape(&path, &paint, reach);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, line: f32, color: Rgba) {
        let s = self.scale;
        let (x, y, w, h, radius, line) = (x * s, y * s, w * s, h * s, radius * s, line * s);
        let Some(path) = round_rect(x, y, w, h, radius) else {
            return;
        };
        let reach = self.reach(x - line, y - line, w + 2.0 * line, h + 2.0 * line);
        if matches!(reach, Reach::None) {
            return;
        }
        self.mark(x - line, y - line, w + 2.0 * line, h + 2.0 * line);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        let stroke = tiny_skia::Stroke {
            width: line,
            ..tiny_skia::Stroke::default()
        };
        self.stroke_shape(&path, &paint, &stroke, reach);
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgba) {
        let s = self.scale;
        let (x0, y0, x1, y1, width) = (x0 * s, y0 * s, x1 * s, y1 * s, width * s);
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        let Some(path) = pb.finish() else {
            return;
        };
        let (bx, by) = (x0.min(x1) - width, y0.min(y1) - width);
        let (bw, bh) = ((x1 - x0).abs() + 2.0 * width, (y1 - y0).abs() + 2.0 * width);
        let reach = self.reach(bx, by, bw, bh);
        if matches!(reach, Reach::None) {
            return;
        }
        self.mark(bx, by, bw, bh);
        let mut paint = tiny_skia::Paint {
            anti_alias: true,
            ..tiny_skia::Paint::default()
        };
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        let stroke = tiny_skia::Stroke {
            width,
            line_cap: tiny_skia::LineCap::Round,
            ..tiny_skia::Stroke::default()
        };
        self.stroke_shape(&path, &paint, &stroke, reach);
    }

    /// Fills the triangle through three points, anti-aliased.
    pub fn triangle(&mut self, points: [(f32, f32); 3], color: Rgba) {
        let s = self.scale;
        let p = points.map(|(x, y)| (x * s, y * s));
        let mut pb = PathBuilder::new();
        pb.move_to(p[0].0, p[0].1);
        pb.line_to(p[1].0, p[1].1);
        pb.line_to(p[2].0, p[2].1);
        pb.close();
        let Some(path) = pb.finish() else {
            return;
        };
        let x0 = p.iter().map(|q| q.0).fold(f32::MAX, f32::min);
        let y0 = p.iter().map(|q| q.1).fold(f32::MAX, f32::min);
        let x1 = p.iter().map(|q| q.0).fold(f32::MIN, f32::max);
        let y1 = p.iter().map(|q| q.1).fold(f32::MIN, f32::max);
        let reach = self.reach(x0, y0, x1 - x0, y1 - y0);
        if matches!(reach, Reach::None) {
            return;
        }
        self.mark(x0, y0, x1 - x0, y1 - y0);
        let mut paint = tiny_skia::Paint {
            anti_alias: true,
            ..tiny_skia::Paint::default()
        };
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        self.fill_shape(&path, &paint, reach);
    }

    /// Fills a rectangle from a per-pixel callback over coordinates
    /// normalized to [0, 1]; the result is opaque.
    pub fn shade(&mut self, x: f32, y: f32, w: f32, h: f32, f: impl Fn(f32, f32) -> Rgba) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.mark(x, y, w, h);
        let width = self.pixmap.width() as i32;
        let (bx0, by0, bx1, by1) = self.bounds();
        let data = self.pixmap.data_mut();
        for py in 0..h as i32 {
            for px in 0..w as i32 {
                let tx = x as i32 + px;
                let ty = y as i32 + py;
                if tx < bx0 || ty < by0 || tx >= bx1 || ty >= by1 {
                    continue;
                }
                let u = px as f32 / (w - 1.0).max(1.0);
                let v = py as f32 / (h - 1.0).max(1.0);
                let c = f(u, v);
                let i = ((ty * width + tx) * 4) as usize;
                data[i] = c.r;
                data[i + 1] = c.g;
                data[i + 2] = c.b;
                data[i + 3] = 255;
            }
        }
    }

    pub fn measure(&mut self, text: &str, family: &str, size: f32, weight: u16) -> f32 {
        let buffer = self.shape(text, family, size * self.scale, weight);
        buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.last().map(|g| g.x + g.w))
            .unwrap_or(0.0)
            / self.scale
    }

    /// Draws one line with its top at `y`; returns the advance width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        family: &str,
        size: f32,
        weight: u16,
        color: Rgba,
    ) -> f32 {
        let s = self.scale;
        let (x, y, size) = (x * s, y * s, size * s);
        let buffer = self.shape(text, family, size, weight);
        let width = self.pixmap.width() as i32;
        let (bx0, by0, bx1, by1) = self.bounds();
        let mut advance = 0.0f32;
        if let Some(run) = buffer.layout_runs().next() {
            if let Some(last) = run.glyphs.last() {
                advance = last.x + last.w;
            }
        }
        if matches!(self.reach(x, y, advance, size * 1.4), Reach::None) {
            return advance / s;
        }
        self.mark(x, y, advance, size * 1.4);
        let data = self.pixmap.data_mut();
        buffer.draw(
            &mut self.fonts.font_system,
            &mut self.fonts.swash,
            cosmic_text::Color::rgba(color.r, color.g, color.b, color.a),
            |gx, gy, w, h, c| {
                let alpha = c.a() as u32;
                if alpha == 0 {
                    return;
                }
                for py in 0..h as i32 {
                    for px in 0..w as i32 {
                        let tx = x as i32 + gx + px;
                        let ty = y as i32 + gy + py;
                        if tx < bx0 || ty < by0 || tx >= bx1 || ty >= by1 {
                            continue;
                        }
                        let i = ((ty * width + tx) * 4) as usize;
                        data[i] = blend(c.r(), data[i], alpha);
                        data[i + 1] = blend(c.g(), data[i + 1], alpha);
                        data[i + 2] = blend(c.b(), data[i + 2], alpha);
                        data[i + 3] = 255;
                    }
                }
            },
        );
        advance / s
    }

    /// Blends the painted region onto an opaque 0RGB frame buffer; only
    /// rows inside the dirty bounds are touched.
    pub fn composite(&self, frame: &mut [u32], frame_width: u32) {
        let Some((x0, y0, x1, y1)) = self.dirty else {
            return;
        };
        let data = self.pixmap.data();
        let width = self.pixmap.width() as usize;
        let height = self.pixmap.height() as usize;
        let x0 = (x0.floor().max(0.0)) as usize;
        let y0 = (y0.floor().max(0.0)) as usize;
        let x1 = (x1.ceil().max(0.0) as usize).min(width);
        let y1 = (y1.ceil().max(0.0) as usize).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * width + x) * 4;
                let a = data[i + 3] as u32;
                if a == 0 {
                    continue;
                }
                let di = y * frame_width as usize + x;
                let Some(dst) = frame.get_mut(di) else {
                    continue;
                };
                let (dr, dg, db) = ((*dst >> 16) & 0xFF, (*dst >> 8) & 0xFF, *dst & 0xFF);
                // Pixmap channels are premultiplied: source-over is add.
                let r = data[i] as u32 + dr * (255 - a) / 255;
                let g = data[i + 1] as u32 + dg * (255 - a) / 255;
                let b = data[i + 2] as u32 + db * (255 - a) / 255;
                *dst = (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
            }
        }
    }

    fn shape(&mut self, text: &str, family: &str, size: f32, weight: u16) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts.font_system, Metrics::new(size, size * 1.25));
        buffer.set_size(&mut self.fonts.font_system, None, None);
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(Weight(weight));
        buffer.set_text(
            &mut self.fonts.font_system,
            text,
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.fonts.font_system, false);
        buffer
    }
}

/// The clip as a coverage mask the size of the pixmap, built once per
/// clip: full inside the rect, empty outside, with hard edges.
fn clip_mask(
    mask: &mut Option<Mask>,
    clip: Option<(i32, i32, i32, i32)>,
    width: u32,
    height: u32,
) -> Option<&Mask> {
    if mask.is_none() {
        let (x0, y0, x1, y1) = clip?;
        let rect = tiny_skia::Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32)?;
        let mut built = Mask::new(width, height)?;
        built.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
        *mask = Some(built);
    }
    mask.as_ref()
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = radius.min(w / 2.0).min(h / 2.0);
    if r <= 0.0 {
        let rect = tiny_skia::Rect::from_xywh(x, y, w, h)?;
        return Some(PathBuilder::from_rect(rect));
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Source-over blend of one channel against an opaque destination.
fn blend(src: u8, dst: u8, alpha: u32) -> u8 {
    ((src as u32 * alpha + dst as u32 * (255 - alpha)) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::fonts::BODY_FAMILY;

    const RED: Rgba = Rgba {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    fn painted(pixmap: &Pixmap, x: u32, y: u32) -> bool {
        pixmap
            .pixel(x, y)
            .is_some_and(|p| p.alpha() > 0 || p.red() > 0)
    }

    #[test]
    fn scale_reports_the_canvas_in_logical_units() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut fonts = FontStore::new();
        let painter = Painter::new(&mut pixmap, &mut fonts, None, 2.0);
        assert_eq!(painter.width(), 100.0);
        assert_eq!(painter.height(), 50.0);
    }

    #[test]
    fn scale_paints_fills_at_physical_pixels() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 2.0);
        painter.fill(10.0, 10.0, 30.0, 20.0, 0.0, RED);
        assert!(painted(&pixmap, 25, 25), "inside the scaled rect");
        assert!(painted(&pixmap, 75, 55), "still inside near the far corner");
        assert!(!painted(&pixmap, 15, 25), "left of the scaled rect");
        assert!(!painted(&pixmap, 85, 25), "right of the scaled rect");
    }

    #[test]
    fn scale_keeps_dirty_bounds_physical() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 2.0);
        painter.fill(10.0, 10.0, 30.0, 20.0, 0.0, RED);
        let (x0, y0, x1, y1) = painter.dirty().unwrap();
        assert_eq!((x0, y0, x1, y1), (20.0, 20.0, 80.0, 60.0));
    }

    #[test]
    fn scale_leaves_measured_widths_logical() {
        let mut pixmap = Pixmap::new(400, 100).unwrap();
        let mut fonts = FontStore::new();
        let plain = Painter::new(&mut pixmap, &mut fonts, None, 1.0).measure(
            "Shortcuts",
            BODY_FAMILY,
            15.0,
            400,
        );
        let scaled = Painter::new(&mut pixmap, &mut fonts, None, 2.0).measure(
            "Shortcuts",
            BODY_FAMILY,
            15.0,
            400,
        );
        assert!(plain > 10.0, "the sample text has width");
        assert!(
            (plain - scaled).abs() < 0.5,
            "logical width holds under scale: {plain} vs {scaled}"
        );
    }

    fn any_painted(pixmap: &Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        (y0..y1).any(|y| (x0..x1).any(|x| painted(pixmap, x, y)))
    }

    #[test]
    fn a_clip_holds_a_fill_inside_its_rect() {
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        painter.clip(Some((20.0, 20.0, 40.0, 40.0)));
        painter.fill(10.0, 10.0, 60.0, 60.0, 8.0, RED);
        assert_eq!(
            painter.dirty().unwrap(),
            (20.0, 20.0, 60.0, 60.0),
            "the dirty bounds stay inside the clip"
        );
        assert!(painted(&pixmap, 30, 30), "inside the clip");
        assert!(painted(&pixmap, 59, 59), "the clip's far corner is inside");
        assert!(
            !any_painted(&pixmap, 0, 0, 100, 20),
            "nothing above the clip"
        );
        assert!(
            !any_painted(&pixmap, 0, 60, 100, 100),
            "nothing below the clip"
        );
        assert!(
            !any_painted(&pixmap, 0, 0, 20, 100),
            "nothing left of the clip"
        );
        assert!(
            !any_painted(&pixmap, 60, 0, 100, 100),
            "nothing right of the clip"
        );
    }

    #[test]
    fn a_clip_skips_a_shape_outside_it_and_passes_one_inside() {
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        painter.clip(Some((20.0, 20.0, 40.0, 40.0)));
        painter.fill(70.0, 70.0, 20.0, 20.0, 4.0, RED);
        painter.stroke(0.0, 0.0, 100.0, 100.0, 0.0, 4.0, RED);
        painter.fill(25.0, 25.0, 10.0, 10.0, 2.0, RED);
        assert!(painted(&pixmap, 30, 30), "a shape inside the clip paints");
        assert!(
            !any_painted(&pixmap, 0, 0, 100, 20),
            "nothing above the clip"
        );
        assert!(
            !any_painted(&pixmap, 0, 60, 100, 100),
            "nothing below the clip"
        );
        assert!(
            !any_painted(&pixmap, 0, 0, 20, 100),
            "nothing left of the clip"
        );
        assert!(
            !any_painted(&pixmap, 60, 0, 100, 100),
            "nothing right of the clip"
        );
    }

    #[test]
    fn a_clip_cuts_a_line_and_a_stroke_at_its_edges() {
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        painter.clip(Some((20.0, 20.0, 40.0, 40.0)));
        painter.line(0.0, 50.0, 100.0, 50.0, 4.0, RED);
        painter.stroke(25.0, 25.0, 50.0, 50.0, 4.0, 2.0, RED);
        assert!(painted(&pixmap, 30, 50), "the line inside the clip");
        assert!(
            painted(&pixmap, 25, 40),
            "the stroke's left edge inside the clip"
        );
        assert!(
            !any_painted(&pixmap, 0, 0, 20, 100),
            "the line stops at the left edge"
        );
        assert!(
            !any_painted(&pixmap, 60, 0, 100, 100),
            "the line and the stroke stop at the right edge"
        );
    }

    #[test]
    fn a_clip_holds_text_inside_its_rect() {
        let mut pixmap = Pixmap::new(200, 60).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        painter.clip(Some((0.0, 0.0, 40.0, 60.0)));
        painter.text(0.0, 10.0, "MMMMMMMMMMMMMMMM", BODY_FAMILY, 20.0, 400, RED);
        assert!(any_painted(&pixmap, 0, 0, 40, 60), "glyphs inside the clip");
        assert!(
            !any_painted(&pixmap, 40, 0, 200, 60),
            "no glyph past the clip"
        );
    }

    #[test]
    fn clearing_the_clip_paints_everywhere_again() {
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let mut fonts = FontStore::new();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        painter.clip(Some((20.0, 20.0, 40.0, 40.0)));
        painter.fill(70.0, 70.0, 20.0, 20.0, 4.0, RED);
        painter.clip(None);
        painter.fill(70.0, 70.0, 20.0, 20.0, 4.0, RED);
        assert!(painted(&pixmap, 75, 75), "the second fill lands");
    }

    #[test]
    fn a_triangle_fills_its_shape_and_respects_the_clip() {
        let mut fonts = FontStore::new();
        let mut plain = Pixmap::new(100, 100).unwrap();
        let mut painter = Painter::new(&mut plain, &mut fonts, None, 1.0);
        painter.triangle([(10.0, 10.0), (90.0, 50.0), (10.0, 90.0)], RED);
        assert!(painted(&plain, 30, 50), "inside the triangle");
        assert!(painted(&plain, 12, 12), "the first corner");
        assert!(!painted(&plain, 80, 15), "outside, past the slanted edge");
        assert!(!painted(&plain, 80, 85), "outside, past the other edge");
        let mut clipped = Pixmap::new(100, 100).unwrap();
        let mut painter = Painter::new(&mut clipped, &mut fonts, None, 1.0);
        painter.clip(Some((0.0, 0.0, 50.0, 100.0)));
        painter.triangle([(10.0, 10.0), (90.0, 50.0), (10.0, 90.0)], RED);
        assert!(painted(&clipped, 30, 50), "inside the clip");
        assert!(
            !any_painted(&clipped, 50, 0, 100, 100),
            "cut at the clip's edge"
        );
    }
}
