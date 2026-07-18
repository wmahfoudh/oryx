//! Paints a vertical slice of the laid-out document into a pixel buffer.
//! Rects first, glyphs above. The result is softbuffer's 0RGB format.

use cosmic_text::{Attrs, Buffer, Color, Family, Metrics, Shaping, Style, Weight};
use tiny_skia::{Pixmap, Rect, Transform};

use crate::doc::images::MediaCache;
use crate::layout::{metrics, LayoutDoc, TextRun};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

/// Paints the document slice `[y_top, y_top + height)` at full width.
pub fn band(
    layout: &LayoutDoc,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    y_top: f32,
    width: u32,
    height: u32,
) -> Vec<u32> {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("pixmap allocation");
    let bg = theme.surface.background;
    pixmap.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, 255));
    let band_bottom = y_top + height as f32;

    let mut paint = tiny_skia::Paint {
        anti_alias: false,
        ..tiny_skia::Paint::default()
    };
    for rect in &layout.rects {
        if rect.y + rect.height < y_top || rect.y > band_bottom {
            continue;
        }
        paint.set_color_rgba8(rect.color.r, rect.color.g, rect.color.b, rect.color.a);
        let rounded = rect.radius_top > 0.0 || rect.radius_bottom > 0.0;
        paint.anti_alias = rounded;
        if !rounded && rect.stroke == 0.0 {
            if let Some(r) = Rect::from_xywh(
                rect.x,
                rect.y - y_top,
                rect.width.max(0.5),
                rect.height.max(0.5),
            ) {
                pixmap.fill_rect(r, &paint, Transform::identity(), None);
            }
            continue;
        }
        let Some(path) = rect_path(rect, y_top) else {
            continue;
        };
        if rect.stroke > 0.0 {
            let stroke = tiny_skia::Stroke {
                width: rect.stroke,
                ..tiny_skia::Stroke::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        } else {
            pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    for image in &layout.images {
        if image.y + image.height < y_top || image.y > band_bottom {
            continue;
        }
        blit_image(&mut pixmap, media, image, y_top);
    }

    for run in &layout.runs {
        let line_height = metrics::LINE_HEIGHT * run.size;
        if run.y + line_height < y_top || run.y > band_bottom {
            continue;
        }
        draw_run(&mut pixmap, fonts, run, y_top);
    }

    pixmap
        .data()
        .chunks_exact(4)
        .map(|px| ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32)
        .collect()
}

/// Blends a scaled image from the cache onto the pixmap.
fn blit_image(
    pixmap: &mut Pixmap,
    media: &mut MediaCache,
    image: &crate::layout::ImagePlace,
    y_top: f32,
) {
    let tw = image.width.round().max(1.0) as u32;
    let th = image.height.round().max(1.0) as u32;
    let Some(rgba) = media.scaled(&image.src, tw, th) else {
        return;
    };
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let (ox, oy) = (image.x as i32, (image.y - y_top) as i32);
    let data = pixmap.data_mut();
    for sy in 0..th as i32 {
        let ty = oy + sy;
        if ty < 0 || ty >= ph {
            continue;
        }
        for sx in 0..tw as i32 {
            let tx = ox + sx;
            if tx < 0 || tx >= pw {
                continue;
            }
            let si = ((sy * tw as i32 + sx) * 4) as usize;
            let alpha = rgba[si + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let di = ((ty * pw + tx) * 4) as usize;
            data[di] = blend(rgba[si], data[di], alpha);
            data[di + 1] = blend(rgba[si + 1], data[di + 1], alpha);
            data[di + 2] = blend(rgba[si + 2], data[di + 2], alpha);
            data[di + 3] = 255;
        }
    }
}

/// Builds a rectangle path with independent top and bottom corner radii.
fn rect_path(rect: &crate::layout::DecoRect, y_top: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (rect.x, rect.y - y_top, rect.width, rect.height);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rt = rect.radius_top.min(w / 2.0).min(h / 2.0);
    let rb = rect.radius_bottom.min(w / 2.0).min(h / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + rt, y);
    pb.line_to(x + w - rt, y);
    pb.quad_to(x + w, y, x + w, y + rt);
    pb.line_to(x + w, y + h - rb);
    pb.quad_to(x + w, y + h, x + w - rb, y + h);
    pb.line_to(x + rb, y + h);
    pb.quad_to(x, y + h, x, y + h - rb);
    pb.line_to(x, y + rt);
    pb.quad_to(x, y, x + rt, y);
    pb.close();
    pb.finish()
}

/// Re-shapes one run single-line and blends its glyphs onto the pixmap.
/// Shaping inputs match the layout pass exactly, so positions agree.
fn draw_run(pixmap: &mut Pixmap, fonts: &mut FontStore, run: &TextRun, y_top: f32) {
    let line_height = metrics::LINE_HEIGHT * run.size;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(run.size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let mut attrs = Attrs::new()
        .family(Family::Name(&run.family))
        .weight(Weight(run.weight));
    if run.italic {
        attrs = attrs.style(Style::Italic);
    }
    buffer.set_text(
        &mut fonts.font_system,
        &run.text,
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);

    let color = Color::rgba(run.color.r, run.color.g, run.color.b, run.color.a);
    // Align the paint buffer's own baseline to the run's recorded baseline;
    // aligning tops instead shifts runs whose size differs from the line's.
    let paint_baseline = buffer
        .layout_runs()
        .next()
        .map(|lr| lr.line_y)
        .unwrap_or(run.size);
    let (origin_x, origin_y) = (run.x, run.baseline - paint_baseline - y_top);
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let data = pixmap.data_mut();
    buffer.draw(
        &mut fonts.font_system,
        &mut fonts.swash,
        color,
        |x, y, w, h, c| {
            let alpha = c.a() as u32;
            if alpha == 0 {
                return;
            }
            for py in 0..h as i32 {
                for px in 0..w as i32 {
                    let tx = origin_x as i32 + x + px;
                    let ty = origin_y as i32 + y + py;
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
}

/// Source-over blend of one channel; the destination is always opaque.
fn blend(src: u8, dst: u8, alpha: u32) -> u8 {
    ((src as u32 * alpha + dst as u32 * (255 - alpha)) / 255) as u8
}
