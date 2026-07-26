//! Pages to PDF bytes. Rectangles become paths, runs become glyphs of a
//! subsetted CID font positioned where the layout put them, and every
//! sheet is filled with the theme background before anything else.

use std::collections::HashMap;

use cosmic_text::fontdb::ID as FaceId;
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use subsetter::GlyphRemapper;

use crate::export::paginate::Page;
use crate::export::PageGeometry;
use crate::layout::{DecoRect, LayoutDoc, TextRun};
use crate::style::fonts::FontStore;
use crate::style::theme::{Rgba, Theme};

/// Glyph space is a thousandth of the text size, in PDF as in OpenType.
const GLYPH_UNITS: f32 = 1000.0;

/// Bézier circle constant: the handle length that turns a quarter turn
/// into a circular arc.
const KAPPA: f32 = 0.5523;

struct Alloc(i32);

impl Alloc {
    fn next(&mut self) -> Ref {
        self.0 += 1;
        Ref::new(self.0)
    }
}

/// A face the export needs, the glyphs it must carry, what they say and
/// how wide they are. The widths come from the shaper rather than from
/// the face, so the reader's pen lands where the shaper's did.
struct FaceUse {
    remapper: GlyphRemapper,
    text: HashMap<u16, String>,
    widths: HashMap<u16, f32>,
    id: Ref,
    index: usize,
}

/// One shaped glyph: what to draw, where it sits, how far it advances,
/// and the source text it stands for.
struct Glyph {
    id: u16,
    x: f32,
    width: f32,
    text: String,
}

/// One run of glyphs from a single face, ready to be shown.
struct Segment {
    face: FaceId,
    glyphs: Vec<Glyph>,
    size: f32,
    color: Rgba,
    x: f32,
    baseline: f32,
}

/// Assembles the file one page at a time, so a long document can be
/// emitted across several slices without holding the pass open.
pub struct Builder {
    pdf: Pdf,
    alloc: Alloc,
    catalog_id: Ref,
    tree_id: Ref,
    info_id: Ref,
    faces: HashMap<FaceId, FaceUse>,
    contents: Vec<(Ref, Vec<FaceId>, Vec<u8>)>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        let mut alloc = Alloc(0);
        let catalog_id = alloc.next();
        let tree_id = alloc.next();
        let info_id = alloc.next();
        Builder {
            pdf: Pdf::new(),
            alloc,
            catalog_id,
            tree_id,
            info_id,
            faces: HashMap::new(),
            contents: Vec::new(),
        }
    }

    /// Draws one page and keeps its content stream. Glyph use accumulates
    /// across pages, so the faces can only be written once every page has
    /// been through here.
    pub fn add_page(
        &mut self,
        page: &Page,
        layout: &LayoutDoc,
        theme: &Theme,
        geometry: &PageGeometry,
        fonts: &mut FontStore,
    ) {
        let (data, used, alphas) = draw_page(
            page,
            layout,
            theme,
            geometry,
            fonts,
            &mut self.faces,
            &mut self.alloc,
        );
        let content_id = self.alloc.next();
        self.pdf
            .stream(content_id, &deflate(&data))
            .filter(Filter::FlateDecode);
        self.contents.push((content_id, used, alphas));
    }

    /// Writes the faces, the page objects and the tree, then the bytes.
    pub fn finish(self, geometry: &PageGeometry, fonts: &FontStore, title: &str) -> Vec<u8> {
        finish(self, geometry, fonts, title)
    }

    pub fn pages_written(&self) -> usize {
        self.contents.len()
    }
}

/// Builds a whole file in one call. The pass uses the builder directly;
/// this is the shape the tests and any one-shot caller want.
pub fn build(
    pages: &[Page],
    layout: &LayoutDoc,
    theme: &Theme,
    geometry: &PageGeometry,
    fonts: &mut FontStore,
    title: &str,
) -> Vec<u8> {
    let mut builder = Builder::new();
    for page in pages {
        builder.add_page(page, layout, theme, geometry, fonts);
    }
    builder.finish(geometry, fonts, title)
}

fn finish(builder: Builder, geometry: &PageGeometry, fonts: &FontStore, title: &str) -> Vec<u8> {
    let Builder {
        mut pdf,
        mut alloc,
        catalog_id,
        tree_id,
        info_id,
        faces,
        contents,
    } = builder;

    let face_ids: Vec<FaceId> = faces.keys().copied().collect();
    for face in &face_ids {
        write_face(&mut pdf, &mut alloc, fonts, &faces[face], *face);
    }

    let page_ids: Vec<Ref> = contents.iter().map(|_| alloc.next()).collect();
    for (index, (content_id, used, alphas)) in contents.iter().enumerate() {
        let mut page = pdf.page(page_ids[index]);
        page.parent(tree_id);
        page.media_box(Rect::new(0.0, 0.0, geometry.width, geometry.height));
        page.contents(*content_id);
        {
            let mut resources = page.resources();
            let mut written = resources.fonts();
            for face in used {
                let entry = &faces[face];
                written.pair(Name(font_name(entry.index).as_bytes()), entry.id);
            }
            written.finish();
            let mut states = resources.ext_g_states();
            for alpha in alphas {
                states
                    .insert(Name(alpha_name(*alpha).as_bytes()))
                    .start::<pdf_writer::writers::ExtGraphicsState>()
                    .non_stroking_alpha(*alpha as f32 / 255.0);
            }
            states.finish();
            resources.finish();
        }
        page.finish();
    }

    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    pdf.catalog(catalog_id).pages(tree_id);
    pdf.document_info(info_id)
        .title(TextStr(title))
        .producer(TextStr(concat!("oryx ", env!("CARGO_PKG_VERSION"))));
    pdf.finish()
}

/// One page's content stream, plus the faces and alpha values it used.
#[allow(clippy::too_many_arguments)]
fn draw_page(
    page: &Page,
    layout: &LayoutDoc,
    theme: &Theme,
    geometry: &PageGeometry,
    fonts: &mut FontStore,
    faces: &mut HashMap<FaceId, FaceUse>,
    alloc: &mut Alloc,
) -> (Vec<u8>, Vec<FaceId>, Vec<u8>) {
    let mut content = Content::new();
    let surface = theme.surface.background;
    content.save_state();
    set_fill(&mut content, surface);
    content.rect(0.0, 0.0, geometry.width, geometry.height);
    content.fill_nonzero();
    content.restore_state();

    let mut alphas: Vec<u8> = Vec::new();
    for rect in &page.rects {
        draw_rect(&mut content, rect, page.top, geometry, &mut alphas);
    }

    let mut used: Vec<FaceId> = Vec::new();
    for run in &layout.runs[page.runs.clone()] {
        for segment in shape(fonts, run) {
            let count = faces.len();
            let entry = faces.entry(segment.face).or_insert_with(|| FaceUse {
                remapper: GlyphRemapper::new(),
                text: HashMap::new(),
                widths: HashMap::new(),
                id: alloc.next(),
                index: count,
            });
            let mut ids: Vec<(u16, f32)> = Vec::new();
            for glyph in &segment.glyphs {
                let cid = entry.remapper.remap(glyph.id);
                entry.text.entry(cid).or_insert_with(|| glyph.text.clone());
                entry
                    .widths
                    .entry(cid)
                    .or_insert(glyph.width * GLYPH_UNITS / segment.size);
                ids.push((cid, glyph.x));
            }
            if !used.contains(&segment.face) {
                used.push(segment.face);
            }
            show(
                &mut content,
                &segment,
                &ids,
                entry.index,
                page.top,
                geometry,
            );
        }
    }
    (content.finish().to_vec(), used, alphas)
}

/// Shapes a run exactly as the band painter shapes it, then splits the
/// glyphs by the face the shaper resolved, so a fallback for an emoji
/// becomes its own segment with no special case.
fn shape(fonts: &mut FontStore, run: &TextRun) -> Vec<Segment> {
    let line_height = crate::layout::metrics::LINE_HEIGHT * run.size;
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

    let mut segments: Vec<Segment> = Vec::new();
    for line in buffer.layout_runs() {
        for glyph in line.glyphs {
            let open = matches!(segments.last(), Some(s) if s.face == glyph.font_id);
            if !open {
                segments.push(Segment {
                    face: glyph.font_id,
                    glyphs: Vec::new(),
                    size: run.size,
                    color: run.color,
                    x: run.x + glyph.x,
                    baseline: run.baseline,
                });
            }
            let segment = segments.last_mut().expect("just opened");
            segment.glyphs.push(Glyph {
                id: glyph.glyph_id,
                x: run.x + glyph.x,
                width: glyph.w,
                text: line.text[glyph.start..glyph.end].to_string(),
            });
        }
    }
    segments
}

/// Shows one segment. The glyphs are positioned by their shaped offsets
/// rather than by the reader's idea of the advances, so a line lands
/// where the layout put it whatever the font does with kerning.
fn show(
    content: &mut Content,
    segment: &Segment,
    ids: &[(u16, f32)],
    index: usize,
    top: f32,
    geometry: &PageGeometry,
) {
    if ids.is_empty() {
        return;
    }
    let name = font_name(index);
    content.begin_text();
    set_fill(content, segment.color);
    content.set_font(Name(name.as_bytes()), segment.size);
    content.set_text_matrix([
        1.0,
        0.0,
        0.0,
        1.0,
        segment.x,
        device_y(segment.baseline, top, geometry),
    ]);
    let mut positioned = content.show_positioned();
    let mut items = positioned.items();
    // The pen walks by the same advances the reader will use, so an
    // adjustment appears only where shaping moved a glyph off them.
    let mut pen = segment.x;
    for ((cid, x), glyph) in ids.iter().zip(&segment.glyphs) {
        if (x - pen).abs() > 0.001 {
            items.adjust(-(x - pen) * GLYPH_UNITS / segment.size);
            pen = *x;
        }
        items.show(Str(&cid.to_be_bytes()));
        pen += glyph.width;
    }
    items.finish();
    positioned.finish();
    content.end_text();
}

/// A rectangle, rounded where the layout asked for it, stroked when it
/// carries an outline width and filled otherwise.
fn draw_rect(
    content: &mut Content,
    rect: &DecoRect,
    top: f32,
    geometry: &PageGeometry,
    alphas: &mut Vec<u8>,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    content.save_state();
    if rect.color.a < 255 {
        if !alphas.contains(&rect.color.a) {
            alphas.push(rect.color.a);
        }
        content.set_parameters(Name(alpha_name(rect.color.a).as_bytes()));
    }
    let x0 = rect.x;
    let x1 = rect.x + rect.width;
    let y1 = device_y(rect.y, top, geometry);
    let y0 = device_y(rect.y + rect.height, top, geometry);
    let rt = rect.radius_top.min(rect.width / 2.0).min(rect.height / 2.0);
    let rb = rect
        .radius_bottom
        .min(rect.width / 2.0)
        .min(rect.height / 2.0);
    content.move_to(x0, y1 - rt);
    if rt > 0.0 {
        content.cubic_to(
            x0,
            y1 - rt + rt * KAPPA,
            x0 + rt - rt * KAPPA,
            y1,
            x0 + rt,
            y1,
        );
    }
    content.line_to(x1 - rt, y1);
    if rt > 0.0 {
        content.cubic_to(
            x1 - rt + rt * KAPPA,
            y1,
            x1,
            y1 - rt + rt * KAPPA,
            x1,
            y1 - rt,
        );
    }
    content.line_to(x1, y0 + rb);
    if rb > 0.0 {
        content.cubic_to(
            x1,
            y0 + rb - rb * KAPPA,
            x1 - rb + rb * KAPPA,
            y0,
            x1 - rb,
            y0,
        );
    }
    content.line_to(x0 + rb, y0);
    if rb > 0.0 {
        content.cubic_to(
            x0 + rb - rb * KAPPA,
            y0,
            x0,
            y0 + rb - rb * KAPPA,
            x0,
            y0 + rb,
        );
    }
    content.close_path();
    if rect.stroke > 0.0 {
        set_stroke(content, rect.color);
        content.set_line_width(rect.stroke);
        content.stroke();
    } else {
        set_fill(content, rect.color);
        content.fill_nonzero();
    }
    content.restore_state();
}

/// Subsets one face and writes the four objects a CID font needs.
fn write_face(pdf: &mut Pdf, alloc: &mut Alloc, fonts: &FontStore, use_: &FaceUse, face: FaceId) {
    let cid_id = alloc.next();
    let descriptor_id = alloc.next();
    let data_id = alloc.next();
    let cmap_id = alloc.next();

    let system = SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    };
    let name = font_name(use_.index);
    let gids: Vec<u16> = use_.remapper.remapped_gids().collect();

    let source = fonts
        .font_system
        .db()
        .with_face_data(face, |data, index| (data.to_vec(), index));
    let (data, index) = match source {
        Some(pair) => pair,
        None => return,
    };
    let subset = subsetter::subset(&data, index, &use_.remapper).unwrap_or_else(|_| data.clone());
    let parsed = ttf_parser::Face::parse(&data, index).ok();
    let scale = parsed
        .as_ref()
        .map(|f| GLYPH_UNITS / f.units_per_em() as f32)
        .unwrap_or(1.0);
    // Widths as the shaper measured them, so the pen the content stream
    // walks and the pen the reader walks are the same pen.
    let widths: Vec<f32> = (0..gids.len() as u16)
        .map(|cid| use_.widths.get(&cid).copied().unwrap_or(0.0))
        .collect();

    pdf.type0_font(use_.id)
        .base_font(Name(name.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid_id)
        .to_unicode(cmap_id);

    let mut cid = pdf.cid_font(cid_id);
    cid.subtype(CidFontType::Type2)
        .base_font(Name(name.as_bytes()))
        .system_info(system)
        .font_descriptor(descriptor_id)
        .cid_to_gid_map_predefined(Name(b"Identity"));
    cid.widths().consecutive(0, widths.iter().copied());
    cid.finish();

    let bbox = parsed.as_ref().map(|f| f.global_bounding_box());
    let mut descriptor = pdf.font_descriptor(descriptor_id);
    descriptor
        .name(Name(name.as_bytes()))
        .flags(FontFlags::SYMBOLIC)
        .italic_angle(parsed.as_ref().map(|f| f.italic_angle()).unwrap_or(0.0))
        .ascent(
            parsed
                .as_ref()
                .map(|f| f.ascender() as f32 * scale)
                .unwrap_or(800.0),
        )
        .descent(
            parsed
                .as_ref()
                .map(|f| f.descender() as f32 * scale)
                .unwrap_or(-200.0),
        )
        .stem_v(80.0)
        .font_file2(data_id);
    if let Some(bbox) = bbox {
        descriptor.bbox(Rect::new(
            bbox.x_min as f32 * scale,
            bbox.y_min as f32 * scale,
            bbox.x_max as f32 * scale,
            bbox.y_max as f32 * scale,
        ));
    }
    descriptor.finish();

    pdf.stream(data_id, &deflate(&subset))
        .filter(Filter::FlateDecode);

    let mut cmap = UnicodeCmap::new(Name(b"Custom"), system);
    for (cid, text) in &use_.text {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(one), None) => cmap.pair(*cid, one),
            (Some(_), Some(_)) => cmap.pair_with_multiple(*cid, text.chars()),
            _ => {}
        }
    }
    pdf.stream(cmap_id, &deflate(&cmap.finish()))
        .filter(Filter::FlateDecode);
}

fn device_y(y: f32, top: f32, geometry: &PageGeometry) -> f32 {
    geometry.height - geometry.margin_y - (y - top)
}

fn set_fill(content: &mut Content, color: Rgba) {
    content.set_fill_rgb(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    );
}

fn set_stroke(content: &mut Content, color: Rgba) {
    content.set_stroke_rgb(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    );
}

fn font_name(index: usize) -> String {
    format!("F{index}")
}

fn alpha_name(alpha: u8) -> String {
    format!("GS{alpha}")
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}
