//! Overlay painter: rounded panels and single-line text drawn into a
//! transparent pixmap, composited over the finished frame. Text assumes an
//! opaque panel beneath it; glyphs over transparent pixels are undefined.

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Weight};
use tiny_skia::{PathBuilder, Pixmap, Transform};

use crate::style::fonts::FontStore;
use crate::style::theme::Rgba;

pub struct Painter<'a> {
    pixmap: Pixmap,
    fonts: &'a mut FontStore,
}

impl<'a> Painter<'a> {
    pub fn new(width: u32, height: u32, fonts: &'a mut FontStore) -> Painter<'a> {
        Painter {
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("pixmap allocation"),
            fonts,
        }
    }

    pub fn width(&self) -> f32 {
        self.pixmap.width() as f32
    }

    pub fn height(&self) -> f32 {
        self.pixmap.height() as f32
    }

    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Rgba) {
        let Some(path) = round_rect(x, y, w, h, radius) else {
            return;
        };
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        self.pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, line: f32, color: Rgba) {
        let Some(path) = round_rect(x, y, w, h, radius) else {
            return;
        };
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        let stroke = tiny_skia::Stroke {
            width: line,
            ..tiny_skia::Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    pub fn measure(&mut self, text: &str, family: &str, size: f32, weight: u16) -> f32 {
        let buffer = self.shape(text, family, size, weight);
        buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.last().map(|g| g.x + g.w))
            .unwrap_or(0.0)
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
        let buffer = self.shape(text, family, size, weight);
        let width = self.pixmap.width() as i32;
        let height = self.pixmap.height() as i32;
        let mut advance = 0.0f32;
        if let Some(run) = buffer.layout_runs().next() {
            if let Some(last) = run.glyphs.last() {
                advance = last.x + last.w;
            }
        }
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
                        if tx < 0 || ty < 0 || tx >= width || ty >= height {
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
        advance
    }

    /// Blends every painted pixel onto an opaque 0RGB frame buffer.
    pub fn composite(&self, frame: &mut [u32], frame_width: u32) {
        let data = self.pixmap.data();
        let width = self.pixmap.width() as usize;
        for (i, px) in data.chunks_exact(4).enumerate() {
            let a = px[3] as u32;
            if a == 0 {
                continue;
            }
            let x = i % width;
            let y = i / width;
            let di = y * frame_width as usize + x;
            let Some(dst) = frame.get_mut(di) else {
                continue;
            };
            let (dr, dg, db) = ((*dst >> 16) & 0xFF, (*dst >> 8) & 0xFF, *dst & 0xFF);
            // Pixmap channels are premultiplied: source-over is add.
            let r = px[0] as u32 + dr * (255 - a) / 255;
            let g = px[1] as u32 + dg * (255 - a) / 255;
            let b = px[2] as u32 + db * (255 - a) / 255;
            *dst = (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
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
