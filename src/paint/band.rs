//! Paints a vertical slice of the laid-out document into a pixel buffer.
//! Rects first, glyphs above. The result is softbuffer's 0RGB format.

use cosmic_text::{Attrs, Buffer, Color, Family, Metrics, Shaping, Style, Weight};
use tiny_skia::{Pixmap, Rect, Transform};

use crate::doc::images::MediaCache;
use crate::doc::model::Document;
use crate::layout::{metrics, DecoRect, LayoutDoc, MathGlyph, TextRun};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

/// Paints the document slice `[y_top, y_top + height)` at full width.
/// `extra` rects (the selection highlight) paint above the document's own
/// rects and below images and glyphs.
#[allow(clippy::too_many_arguments)]
pub fn band(
    layout: &LayoutDoc,
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    extra: &[DecoRect],
    y_top: f32,
    width: u32,
    height: u32,
) -> Vec<u32> {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("pixmap allocation");
    // A code file's page is code: the theme's code background becomes
    // the paper, edge to edge, where the dropped panel used to carry it.
    let bg = if doc.code_file {
        theme.blocks.code_bg
    } else {
        theme.surface.background
    };
    pixmap.fill(tiny_skia::Color::from_rgba8(bg.r, bg.g, bg.b, 255));
    let band_bottom = y_top + height as f32;

    let mut paint = tiny_skia::Paint {
        anti_alias: false,
        ..tiny_skia::Paint::default()
    };
    let (rect_head, rect_tail) = layout.rects_in(y_top, band_bottom);
    for rect in layout.rects[rect_head]
        .iter()
        .chain(&layout.rects[rect_tail])
        .chain(extra)
    {
        if rect.y + rect.height < y_top || rect.y > band_bottom {
            continue;
        }
        paint.set_color_rgba8(rect.color.r, rect.color.g, rect.color.b, rect.color.a);
        let rounded = rect.radius_top > 0.0 || rect.radius_bottom > 0.0;
        let smooth = rounded || rect.anti_alias;
        paint.anti_alias = smooth;
        if !smooth && rect.stroke == 0.0 {
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

    let (image_head, image_tail) = layout.images_in(y_top, band_bottom);
    for image in layout.images[image_head]
        .iter()
        .chain(&layout.images[image_tail])
    {
        if image.y + image.height < y_top || image.y > band_bottom {
            continue;
        }
        blit_image(&mut pixmap, media, image, y_top);
    }

    let (run_head, run_tail) = layout.runs_in(y_top, band_bottom);
    for run in layout.runs[run_head].iter().chain(&layout.runs[run_tail]) {
        let line_height = metrics::LINE_HEIGHT * run.size;
        if run.y + line_height < y_top || run.y > band_bottom {
            continue;
        }
        draw_run(
            &mut pixmap,
            fonts,
            run,
            layout.run_text(doc, run),
            layout.run_family(run),
            y_top,
        );
    }

    let (math_head, math_tail) = layout.math_in(y_top, band_bottom);
    for g in layout.math_glyphs[math_head]
        .iter()
        .chain(&layout.math_glyphs[math_tail])
    {
        if g.y + 0.6 * g.size < y_top || g.y - 1.2 * g.size > band_bottom {
            continue;
        }
        draw_math_glyph(&mut pixmap, fonts, g, y_top);
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
fn draw_run(
    pixmap: &mut Pixmap,
    fonts: &mut FontStore,
    run: &TextRun,
    text: &str,
    family: &str,
    y_top: f32,
) {
    let line_height = metrics::LINE_HEIGHT * run.size;
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(run.size, line_height));
    buffer.set_size(&mut fonts.font_system, None, None);
    let mut attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(Weight(run.weight));
    if run.italic {
        attrs = attrs.style(Style::Italic);
    }
    buffer.set_text(
        &mut fonts.font_system,
        text,
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

/// Rasterizes one typeset math glyph from the swash cache by glyph id in
/// the math face; no shaping is involved, noad already positioned it. The
/// glyph's `y` is its baseline; the cache answers placement offsets from
/// that origin.
fn draw_math_glyph(pixmap: &mut Pixmap, fonts: &mut FontStore, g: &MathGlyph, y_top: f32) {
    let (key, gx, gy) = cosmic_text::CacheKey::new(
        fonts.math_face,
        g.glyph,
        g.size,
        (g.x, g.y - y_top),
        cosmic_text::fontdb::Weight::NORMAL,
        cosmic_text::CacheKeyFlags::empty(),
    );
    let FontStore {
        font_system, swash, ..
    } = fonts;
    let Some(image) = swash.get_image(font_system, key).as_ref() else {
        return;
    };
    let left = gx + image.placement.left;
    let top = gy - image.placement.top;
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let data = pixmap.data_mut();
    let (pw, ph) = (image.placement.width as i32, image.placement.height as i32);
    match image.content {
        cosmic_text::SwashContent::Mask => {
            for py in 0..ph {
                for px in 0..pw {
                    let alpha = image.data[(py * pw + px) as usize] as u32;
                    if alpha == 0 {
                        continue;
                    }
                    let (tx, ty) = (left + px, top + py);
                    if tx < 0 || ty < 0 || tx >= width || ty >= height {
                        continue;
                    }
                    let i = ((ty * width + tx) * 4) as usize;
                    data[i] = blend(g.color.r, data[i], alpha);
                    data[i + 1] = blend(g.color.g, data[i + 1], alpha);
                    data[i + 2] = blend(g.color.b, data[i + 2], alpha);
                    data[i + 3] = 255;
                }
            }
        }
        cosmic_text::SwashContent::Color => {
            for py in 0..ph {
                for px in 0..pw {
                    let s = ((py * pw + px) * 4) as usize;
                    let alpha = image.data[s + 3] as u32;
                    if alpha == 0 {
                        continue;
                    }
                    let (tx, ty) = (left + px, top + py);
                    if tx < 0 || ty < 0 || tx >= width || ty >= height {
                        continue;
                    }
                    let i = ((ty * width + tx) * 4) as usize;
                    data[i] = blend(image.data[s], data[i], alpha);
                    data[i + 1] = blend(image.data[s + 1], data[i + 1], alpha);
                    data[i + 2] = blend(image.data[s + 2], data[i + 2], alpha);
                    data[i + 3] = 255;
                }
            }
        }
        cosmic_text::SwashContent::SubpixelMask => {}
    }
}

/// Source-over blend of one channel; the destination is always opaque.
fn blend(src: u8, dst: u8, alpha: u32) -> u8 {
    ((src as u32 * alpha + dst as u32 * (255 - alpha)) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;
    use crate::layout::{layout, ViewConfig};
    use crate::style::theme::Rgba;
    use std::path::PathBuf;

    fn packed(c: Rgba) -> u32 {
        ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
    }

    fn corner_pixel(doc: &Document) -> u32 {
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        let lay = layout(
            doc,
            &theme,
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            400.0,
        );
        band(&lay, doc, &theme, &mut fonts, &mut media, &[], 0.0, 40, 40)[0]
    }

    #[test]
    fn a_code_file_paints_the_page_in_the_code_background() {
        let theme = Theme::default_dark();
        let doc = load::code_document(Some("rust"), "let x = 1;\n");
        assert_eq!(
            corner_pixel(&doc),
            packed(theme.blocks.code_bg),
            "a page that is code keeps the theme's code background"
        );
        let prose = load::plain_document("hello\n");
        assert_eq!(
            corner_pixel(&prose),
            packed(theme.surface.background),
            "prose keeps the page background"
        );
    }
}
