//! Pages to PDF bytes. Rectangles become paths, runs become glyphs of a
//! subsetted CID font positioned where the layout put them, and every
//! sheet is filled with the theme background before anything else.

use std::collections::HashMap;

use cosmic_text::fontdb::ID as FaceId;
use cosmic_text::{
    Attrs, Buffer, CacheKey, CacheKeyFlags, Family, Metrics, Shaping, Style, SwashContent, Weight,
};
use pdf_writer::types::{
    ActionType, AnnotationType, CidFontType, FontFlags, SystemInfo, UnicodeCmap,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use subsetter::GlyphRemapper;

use crate::doc::images::MediaCache;
use crate::doc::model::{BlockKind, Document};
use crate::export::paginate::Page;
use crate::export::{ExportSettings, PageGeometry};
use crate::layout::{DecoRect, LayoutDoc, TextRun};
use crate::style::fonts::FontStore;
use crate::style::theme::{Rgba, Theme};

/// Glyph space is a thousandth of the text size, in PDF as in OpenType.
const GLYPH_UNITS: f32 = 1000.0;

/// Resolution ceiling for an embedded image. A source finer than this at
/// its placed size is downsampled, which keeps a photograph from carrying
/// detail no print can show and no screen will zoom to.
const MAX_DPI: f32 = 300.0;

/// Points per inch, which is what makes a point a point.
const POINTS_PER_INCH: f32 = 72.0;

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

/// Everything an export needs that does not change from page to page.
pub struct Job<'a> {
    pub doc: &'a Document,
    pub layout: &'a LayoutDoc,
    pub theme: &'a Theme,
    pub geometry: &'a PageGeometry,
    pub settings: &'a ExportSettings,
    pub title: &'a str,
}

/// A clickable box and what it points at, before the target is resolved.
/// Internal targets cannot be written until every page has a reference.
struct Link {
    rect: [f32; 4],
    target: String,
}

/// One drawn page, waiting for the objects that reference it.
struct PageContent {
    id: Ref,
    top: f32,
    faces: Vec<FaceId>,
    alphas: Vec<u8>,
    images: Vec<(String, Ref)>,
    links: Vec<Link>,
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
    /// One XObject per source, so a logo repeated through a document is
    /// written once and drawn many times.
    images: HashMap<String, Ref>,
    /// Faces with no outline tables, the color bitmap fonts emoji
    /// resolve through; no subset can carry them, so their glyphs embed
    /// as images.
    bitmap: HashMap<FaceId, bool>,
    /// One image XObject per bitmap glyph and raster size, shared
    /// across pages; None records a glyph that produced no raster.
    glyph_images: HashMap<(FaceId, u16, u32), Option<GlyphImage>>,
    contents: Vec<PageContent>,
}

/// An embedded bitmap glyph: its object, the raster's pixel geometry,
/// and the placement offsets around the baseline origin in raster
/// pixels, as the scaler reported them.
#[derive(Clone, Copy)]
struct GlyphImage {
    id: Ref,
    width: u32,
    height: u32,
    left: i32,
    top: i32,
}

/// Raster pixels per point for embedded bitmap glyphs, the same 300 dpi
/// ceiling placed images sample at.
const GLYPH_RASTER: f32 = MAX_DPI / POINTS_PER_INCH;

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
            images: HashMap::new(),
            bitmap: HashMap::new(),
            glyph_images: HashMap::new(),
            contents: Vec::new(),
        }
    }

    /// Draws one page and keeps its content stream. Glyph use accumulates
    /// across pages, so the faces can only be written once every page has
    /// been through here.
    pub fn add_page(
        &mut self,
        job: &Job,
        page: &Page,
        fonts: &mut FontStore,
        media: &mut MediaCache,
    ) {
        let geometry = job.geometry;
        let mut content = Content::new();
        content.save_state();
        set_fill(&mut content, job.theme.surface.background);
        content.rect(0.0, 0.0, geometry.width, geometry.height);
        content.fill_nonzero();
        content.restore_state();

        let mut alphas: Vec<u8> = Vec::new();
        for rect in &page.rects {
            draw_rect(&mut content, rect, page.top, geometry, &mut alphas);
        }

        let mut images: Vec<(String, Ref)> = Vec::new();
        for image in &page.images {
            let Some(id) = self.image_object(&image.src, image.width, media) else {
                continue;
            };
            let name = image_name(images.len());
            content.save_state();
            content.transform([
                image.width,
                0.0,
                0.0,
                image.height,
                image.x,
                device_y(image.y + image.height, page.top, geometry),
            ]);
            content.x_object(Name(name.as_bytes()));
            content.restore_state();
            images.push((name, id));
        }

        let mut used: Vec<FaceId> = Vec::new();
        for run in &job.layout.runs[page.runs.clone()] {
            self.draw_run(
                &mut content,
                run,
                page.top,
                geometry,
                fonts,
                &mut used,
                &mut images,
            );
        }
        if job.settings.page_numbers {
            self.draw_page_number(
                &mut content,
                job,
                self.contents.len() + 1,
                fonts,
                &mut used,
                &mut images,
            );
        }

        let links = collect_links(job.layout, page, geometry);
        let content_id = self.alloc.next();
        self.pdf
            .stream(content_id, &deflate(&content.finish()))
            .filter(Filter::FlateDecode);
        self.contents.push(PageContent {
            id: content_id,
            top: page.top,
            faces: used,
            alphas,
            images,
            links,
        });
    }

    /// One run's glyphs, split by the face the shaper resolved.
    #[allow(clippy::too_many_arguments)]
    fn draw_run(
        &mut self,
        content: &mut Content,
        run: &TextRun,
        top: f32,
        geometry: &PageGeometry,
        fonts: &mut FontStore,
        used: &mut Vec<FaceId>,
        images: &mut Vec<(String, Ref)>,
    ) {
        for segment in shape(fonts, run) {
            if self.is_bitmap_face(fonts, segment.face) {
                self.draw_bitmap_segment(content, &segment, top, geometry, fonts, images);
                continue;
            }
            let ids = self.register(&segment);
            if !used.contains(&segment.face) {
                used.push(segment.face);
            }
            let index = self.faces[&segment.face].index;
            let baseline = device_y(segment.baseline, top, geometry);
            show(content, &segment, &ids, index, baseline);
        }
    }

    /// Whether a face carries no outline tables, which is what emoji
    /// fonts like Noto Color Emoji look like: bitmap strikes only, so
    /// the subsetter cannot embed them.
    fn is_bitmap_face(&mut self, fonts: &mut FontStore, face: FaceId) -> bool {
        if let Some(known) = self.bitmap.get(&face) {
            return *known;
        }
        let bitmap = fonts
            .font_system
            .db()
            .with_face_data(face, |data, index| {
                ttf_parser::Face::parse(data, index)
                    .map(|face| {
                        let tables = face.tables();
                        tables.glyf.is_none() && tables.cff.is_none() && tables.cff2.is_none()
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        self.bitmap.insert(face, bitmap);
        bitmap
    }

    /// Bitmap glyphs draw as images at their shaped positions, sampled
    /// at the 300 dpi ceiling placed images use, so an emoji never fails
    /// the export. Text extraction loses these glyphs; the surrounding
    /// text is untouched.
    fn draw_bitmap_segment(
        &mut self,
        content: &mut Content,
        segment: &Segment,
        top: f32,
        geometry: &PageGeometry,
        fonts: &mut FontStore,
        images: &mut Vec<(String, Ref)>,
    ) {
        let baseline = device_y(segment.baseline, top, geometry);
        for glyph in &segment.glyphs {
            let Some(image) = self.glyph_image(fonts, segment.face, glyph.id, segment.size) else {
                continue;
            };
            let name = image_name(images.len());
            content.save_state();
            content.transform([
                image.width as f32 / GLYPH_RASTER,
                0.0,
                0.0,
                image.height as f32 / GLYPH_RASTER,
                glyph.x + image.left as f32 / GLYPH_RASTER,
                baseline + (image.top - image.height as i32) as f32 / GLYPH_RASTER,
            ]);
            content.x_object(Name(name.as_bytes()));
            content.restore_state();
            images.push((name, image.id));
        }
    }

    /// One rasterized glyph as an image XObject, written once per glyph
    /// and raster size and reused across pages.
    fn glyph_image(
        &mut self,
        fonts: &mut FontStore,
        face: FaceId,
        glyph: u16,
        size: f32,
    ) -> Option<GlyphImage> {
        let raster_size = size * GLYPH_RASTER;
        let key = (face, glyph, raster_size.to_bits());
        if let Some(cached) = self.glyph_images.get(&key) {
            return *cached;
        }
        // Weight only drives synthetic bolding, which bitmap strikes
        // ignore; the shaper already resolved the face.
        let (cache_key, _, _) = CacheKey::new(
            face,
            glyph,
            raster_size,
            (0.0, 0.0),
            Weight::NORMAL,
            CacheKeyFlags::empty(),
        );
        let raster = fonts
            .swash
            .get_image_uncached(&mut fonts.font_system, cache_key)
            .filter(|img| {
                img.placement.width > 0
                    && img.placement.height > 0
                    && img.content == SwashContent::Color
            });
        let built = raster.map(|img| {
            let (width, height) = (img.placement.width, img.placement.height);
            let mut rgb = Vec::with_capacity(img.data.len() / 4 * 3);
            let mut alpha = Vec::with_capacity(img.data.len() / 4);
            for pixel in img.data.chunks_exact(4) {
                rgb.extend_from_slice(&pixel[..3]);
                alpha.push(pixel[3]);
            }
            let id = self.alloc.next();
            let mask_id = self.alloc.next();
            let deflated = deflate(&rgb);
            let mut xobject = self.pdf.image_xobject(id, &deflated);
            xobject.width(width as i32);
            xobject.height(height as i32);
            xobject.color_space().device_rgb();
            xobject.bits_per_component(8);
            xobject.filter(Filter::FlateDecode);
            xobject.s_mask(mask_id);
            xobject.finish();
            let deflated = deflate(&alpha);
            let mut gray = self.pdf.image_xobject(mask_id, &deflated);
            gray.width(width as i32);
            gray.height(height as i32);
            gray.color_space().device_gray();
            gray.bits_per_component(8);
            gray.filter(Filter::FlateDecode);
            gray.finish();
            GlyphImage {
                id,
                width,
                height,
                left: img.placement.left,
                top: img.placement.top,
            }
        });
        self.glyph_images.insert(key, built);
        built
    }

    /// The page number, centred in the bottom margin where nothing else
    /// goes. It is not part of the document, so it is positioned in
    /// device space rather than through a page top.
    #[allow(clippy::too_many_arguments)]
    fn draw_page_number(
        &mut self,
        content: &mut Content,
        job: &Job,
        number: usize,
        fonts: &mut FontStore,
        used: &mut Vec<FaceId>,
        images: &mut Vec<(String, Ref)>,
    ) {
        let run = TextRun {
            text: number.to_string(),
            x: 0.0,
            y: 0.0,
            baseline: 0.0,
            width: 0.0,
            size: job.settings.code_size,
            family: job.settings.body_family.clone(),
            weight: 400,
            italic: false,
            color: job.theme.text.body,
            link: None,
            block: 0,
            span: 0,
        };
        let mut segments = shape(fonts, &run);
        let width = segments
            .iter()
            .flat_map(|segment| segment.glyphs.iter())
            .map(|glyph| glyph.x + glyph.width)
            .fold(0.0, f32::max);
        let offset = (job.geometry.width - width) / 2.0;
        let baseline = job.geometry.margin_y * 0.45;
        for segment in &mut segments {
            segment.x += offset;
            for glyph in &mut segment.glyphs {
                glyph.x += offset;
            }
            if self.is_bitmap_face(fonts, segment.face) {
                // The number's baseline is already in device space;
                // this top makes the segment's device_y answer it.
                let top = baseline - (job.geometry.height - job.geometry.margin_y);
                self.draw_bitmap_segment(content, segment, top, job.geometry, fonts, images);
                continue;
            }
            let ids = self.register(segment);
            if !used.contains(&segment.face) {
                used.push(segment.face);
            }
            let index = self.faces[&segment.face].index;
            show(content, segment, &ids, index, baseline);
        }
    }

    /// Records a segment's glyphs against its face and returns the
    /// subset ids to write, so the content stream and the embedded font
    /// agree on what a code means.
    fn register(&mut self, segment: &Segment) -> Vec<(u16, f32)> {
        let count = self.faces.len();
        let alloc = &mut self.alloc;
        let entry = self.faces.entry(segment.face).or_insert_with(|| FaceUse {
            remapper: GlyphRemapper::new(),
            text: HashMap::new(),
            widths: HashMap::new(),
            id: alloc.next(),
            index: count,
        });
        segment
            .glyphs
            .iter()
            .map(|glyph| {
                let cid = entry.remapper.remap(glyph.id);
                entry.text.entry(cid).or_insert_with(|| glyph.text.clone());
                entry
                    .widths
                    .entry(cid)
                    .or_insert(glyph.width * GLYPH_UNITS / segment.size);
                (cid, glyph.x)
            })
            .collect()
    }

    /// The XObject for a source, written the first time it is asked for.
    /// RGBA splits into colour and an alpha mask, which is what lets a
    /// badge keep its transparent corners.
    fn image_object(&mut self, src: &str, placed: f32, media: &mut MediaCache) -> Option<Ref> {
        if let Some(id) = self.images.get(src) {
            return Some(*id);
        }
        // The samples are the source's own, not the box it is drawn in:
        // a page places an image in points, and a point holds several
        // pixels, so sampling at the placed size prints it soft.
        let (natural_w, natural_h) = media.dimensions(src)?;
        let ceiling = ((placed * MAX_DPI / POINTS_PER_INCH).round() as u32).max(1);
        let width = natural_w.min(ceiling).max(1);
        let height = ((natural_h as f32) * (width as f32) / (natural_w as f32))
            .round()
            .max(1.0) as u32;
        let pixels = media.scaled(src, width, height)?.to_vec();
        let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(pixels.len() / 4);
        for pixel in pixels.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
            alpha.push(pixel[3]);
        }
        let id = self.alloc.next();
        let mask_id = alpha.iter().any(|a| *a != 255).then(|| self.alloc.next());
        let deflated = deflate(&rgb);
        let mut image = self.pdf.image_xobject(id, &deflated);
        image.width(width as i32);
        image.height(height as i32);
        image.color_space().device_rgb();
        image.bits_per_component(8);
        image.filter(Filter::FlateDecode);
        if let Some(mask) = mask_id {
            image.s_mask(mask);
        }
        image.finish();
        if let Some(mask) = mask_id {
            let deflated = deflate(&alpha);
            let mut gray = self.pdf.image_xobject(mask, &deflated);
            gray.width(width as i32);
            gray.height(height as i32);
            gray.color_space().device_gray();
            gray.bits_per_component(8);
            gray.filter(Filter::FlateDecode);
            gray.finish();
        }
        self.images.insert(src.to_string(), id);
        Some(id)
    }

    /// Writes the faces, the page objects and the tree, then the bytes.
    /// A face that cannot be subset fails the export by name, because
    /// embedding the whole font under subset numbering would render
    /// every glyph wrong and still report success.
    pub fn finish(self, job: &Job, fonts: &FontStore) -> Result<Vec<u8>, String> {
        finish(self, job, fonts)
    }
}

/// Every link on a page, as a box in device space and the target it
/// carries. Runs and images both qualify.
fn collect_links(layout: &LayoutDoc, page: &Page, geometry: &PageGeometry) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for run in &layout.runs[page.runs.clone()] {
        let Some(target) = run.link.as_deref() else {
            continue;
        };
        let height = crate::layout::metrics::LINE_HEIGHT * run.size;
        links.push(Link {
            rect: [
                run.x,
                device_y(run.y + height, page.top, geometry),
                run.x + run.width,
                device_y(run.y, page.top, geometry),
            ],
            target: target.to_string(),
        });
    }
    for image in &page.images {
        let Some(target) = image.link.as_deref() else {
            continue;
        };
        links.push(Link {
            rect: [
                image.x,
                device_y(image.y + image.height, page.top, geometry),
                image.x + image.width,
                device_y(image.y, page.top, geometry),
            ],
            target: target.to_string(),
        });
    }
    links
}

/// Builds a whole file in one call. The pass uses the builder directly;
/// this is the shape the tests and any one-shot caller want.
pub fn build(
    job: &Job,
    pages: &[Page],
    fonts: &mut FontStore,
    media: &mut MediaCache,
) -> Result<Vec<u8>, String> {
    let mut builder = Builder::new();
    for page in pages {
        builder.add_page(job, page, fonts, media);
    }
    builder.finish(job, fonts)
}

fn finish(builder: Builder, job: &Job, fonts: &FontStore) -> Result<Vec<u8>, String> {
    let Builder {
        mut pdf,
        mut alloc,
        bitmap: _,
        glyph_images: _,
        catalog_id,
        tree_id,
        info_id,
        faces,
        images: _,
        contents,
    } = builder;
    let geometry = job.geometry;

    let face_ids: Vec<FaceId> = faces.keys().copied().collect();
    for face in &face_ids {
        write_face(&mut pdf, &mut alloc, fonts, &faces[face], *face)?;
    }

    let page_ids: Vec<Ref> = contents.iter().map(|_| alloc.next()).collect();
    let tops: Vec<f32> = contents.iter().map(|content| content.top).collect();

    // Annotations are objects of their own, and an internal target needs
    // the page reference, so they can only be written now.
    let mut annotations: Vec<Vec<Ref>> = Vec::with_capacity(contents.len());
    for content in &contents {
        let mut ids = Vec::with_capacity(content.links.len());
        for link in &content.links {
            let id = alloc.next();
            let mut annotation = pdf.annotation(id);
            annotation.subtype(AnnotationType::Link);
            annotation.rect(Rect::new(
                link.rect[0],
                link.rect[1],
                link.rect[2],
                link.rect[3],
            ));
            annotation.border(0.0, 0.0, 0.0, None);
            match place(job, &tops, &link.target) {
                Some((page, y)) => {
                    annotation
                        .action()
                        .action_type(ActionType::GoTo)
                        .destination()
                        .page(page_ids[page])
                        .xyz(0.0, y, None);
                }
                None if link.target.starts_with("http") => {
                    annotation
                        .action()
                        .action_type(ActionType::Uri)
                        .uri(Str(link.target.as_bytes()));
                }
                None => {}
            }
            annotation.finish();
            ids.push(id);
        }
        annotations.push(ids);
    }

    for (index, content) in contents.iter().enumerate() {
        let mut page = pdf.page(page_ids[index]);
        page.parent(tree_id);
        page.media_box(Rect::new(0.0, 0.0, geometry.width, geometry.height));
        page.contents(content.id);
        if !annotations[index].is_empty() {
            page.annotations(annotations[index].iter().copied());
        }
        {
            let mut resources = page.resources();
            let mut written = resources.fonts();
            for face in &content.faces {
                let entry = &faces[face];
                written.pair(Name(font_name(entry.index).as_bytes()), entry.id);
            }
            written.finish();
            let mut states = resources.ext_g_states();
            for alpha in &content.alphas {
                states
                    .insert(Name(alpha_name(*alpha).as_bytes()))
                    .start::<pdf_writer::writers::ExtGraphicsState>()
                    .non_stroking_alpha(*alpha as f32 / 255.0);
            }
            states.finish();
            let mut objects = resources.x_objects();
            for (name, id) in &content.images {
                objects.pair(Name(name.as_bytes()), *id);
            }
            objects.finish();
            resources.finish();
        }
        page.finish();
    }

    let outline_id = write_outline(&mut pdf, &mut alloc, job, &tops, &page_ids);

    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    let mut catalog = pdf.catalog(catalog_id);
    catalog.pages(tree_id);
    if let Some(outline_id) = outline_id {
        catalog.outlines(outline_id);
    }
    catalog.finish();
    pdf.document_info(info_id)
        .title(TextStr(job.title))
        .producer(TextStr(concat!("oryx ", env!("CARGO_PKG_VERSION"))));
    Ok(pdf.finish())
}

/// Resolves an internal target to a page and a position on it. External
/// targets and anchors the document does not carry answer None.
fn place(job: &Job, tops: &[f32], target: &str) -> Option<(usize, f32)> {
    let anchor = target.strip_prefix('#').unwrap_or(target);
    let y = job
        .layout
        .anchors
        .iter()
        .find_map(|(name, y)| (name == anchor || name == target).then_some(*y))?;
    let page = tops.partition_point(|top| *top <= y).saturating_sub(1);
    Some((page, device_y(y, tops[page], job.geometry)))
}

/// One heading of the outline, with where it points and how it nests.
struct Item {
    title: String,
    level: u8,
    page: usize,
    y: f32,
    id: Ref,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// The heading outline, nested by level. Returns None for a document
/// with no headings, which then carries no outline at all.
fn write_outline(
    pdf: &mut Pdf,
    alloc: &mut Alloc,
    job: &Job,
    tops: &[f32],
    page_ids: &[Ref],
) -> Option<Ref> {
    let mut items: Vec<Item> = Vec::new();
    for block in &job.doc.blocks {
        let BlockKind::Heading {
            level,
            spans,
            anchor,
        } = &block.kind
        else {
            continue;
        };
        let Some((page, y)) = place(job, tops, anchor) else {
            continue;
        };
        let title: String = spans
            .iter()
            .map(|span| span.text(&job.doc.source))
            .collect();
        if title.trim().is_empty() {
            continue;
        }
        items.push(Item {
            title,
            level: *level,
            page,
            y,
            id: alloc.next(),
            parent: None,
            children: Vec::new(),
        });
    }
    if items.is_empty() {
        return None;
    }

    // Nest by level: a heading hangs off the nearest shallower one before
    // it, so a jump from h2 to h4 costs one level rather than failing.
    let mut stack: Vec<usize> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for index in 0..items.len() {
        while let Some(&top) = stack.last() {
            if items[top].level >= items[index].level {
                stack.pop();
            } else {
                break;
            }
        }
        match stack.last() {
            Some(&parent) => {
                items[index].parent = Some(parent);
                items[parent].children.push(index);
            }
            None => roots.push(index),
        }
        stack.push(index);
    }

    let outline_id = alloc.next();
    let mut outline = pdf.outline(outline_id);
    outline.first(items[roots[0]].id);
    outline.last(items[*roots.last().expect("a root")].id);
    outline.count(roots.len() as i32);
    outline.finish();

    for index in 0..items.len() {
        let siblings: &[usize] = match items[index].parent {
            Some(parent) => &items[parent].children,
            None => &roots,
        };
        let at = siblings
            .iter()
            .position(|sibling| *sibling == index)
            .expect("a child of its parent");
        let previous = at.checked_sub(1).map(|before| items[siblings[before]].id);
        let following = siblings.get(at + 1).map(|after| items[*after].id);
        let item = &items[index];
        let mut written = pdf.outline_item(item.id);
        written.title(TextStr(&item.title));
        match item.parent {
            Some(parent) => written.parent(items[parent].id),
            None => written.parent(outline_id),
        };
        if let Some(previous) = previous {
            written.prev(previous);
        }
        if let Some(following) = following {
            written.next(following);
        }
        if let Some(first) = item.children.first() {
            written.first(items[*first].id);
            written.last(items[*item.children.last().expect("a last child")].id);
            written.count(item.children.len() as i32);
        }
        written
            .dest()
            .page(page_ids[item.page])
            .xyz(0.0, item.y, None);
        written.finish();
    }
    Some(outline_id)
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
fn show(content: &mut Content, segment: &Segment, ids: &[(u16, f32)], index: usize, baseline: f32) {
    if ids.is_empty() {
        return;
    }
    let name = font_name(index);
    content.begin_text();
    set_fill(content, segment.color);
    content.set_font(Name(name.as_bytes()), segment.size);
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, segment.x, baseline]);
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

/// Subsets one face and writes the four objects a CID font needs. The
/// embedded flavour follows the subset's own magic: TrueType outlines
/// ride a CIDFontType2 with FontFile2, CFF outlines a CIDFontType0 with
/// FontFile3, since a reader may reject the wrong pairing.
fn write_face(
    pdf: &mut Pdf,
    alloc: &mut Alloc,
    fonts: &FontStore,
    use_: &FaceUse,
    face: FaceId,
) -> Result<(), String> {
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
        None => return Ok(()),
    };
    let subset = subsetter::subset(&data, index, &use_.remapper).map_err(|err| {
        let family = fonts
            .font_system
            .db()
            .face(face)
            .and_then(|info| info.families.first().map(|(name, _)| name.clone()))
            .unwrap_or_else(|| name.clone());
        format!("cannot embed the font {family}: {err}")
    })?;
    let cff = subset.starts_with(b"OTTO");
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
    cid.subtype(if cff {
        CidFontType::Type0
    } else {
        CidFontType::Type2
    })
    .base_font(Name(name.as_bytes()))
    .system_info(system)
    .font_descriptor(descriptor_id);
    // CIDToGIDMap belongs to Type2 alone; a CFF font maps through its
    // own charset, identity here since subset ids are dense from zero.
    if !cff {
        cid.cid_to_gid_map_predefined(Name(b"Identity"));
    }
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
        .stem_v(80.0);
    if cff {
        descriptor.font_file3(data_id);
    } else {
        descriptor.font_file2(data_id);
    }
    if let Some(bbox) = bbox {
        descriptor.bbox(Rect::new(
            bbox.x_min as f32 * scale,
            bbox.y_min as f32 * scale,
            bbox.x_max as f32 * scale,
            bbox.y_max as f32 * scale,
        ));
    }
    descriptor.finish();

    let deflated = deflate(&subset);
    let mut stream = pdf.stream(data_id, &deflated);
    stream.filter(Filter::FlateDecode);
    if cff {
        // FontFile3 carrying a whole OpenType file declares itself.
        stream.pair(Name(b"Subtype"), Name(b"OpenType"));
    }
    stream.finish();

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
    Ok(())
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

fn image_name(index: usize) -> String {
    format!("Im{index}")
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}
