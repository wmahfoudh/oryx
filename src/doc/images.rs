//! Image loading and caching. Paths resolve against the document's
//! directory; failures and remote URLs yield None and render as
//! placeholders. Originals and scaled variants are memoized so relayout
//! and repaint never re-decode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::RgbaImage;

pub fn load(doc_dir: &Path, src: &str) -> Option<RgbaImage> {
    if src.starts_with("http://") || src.starts_with("https://") {
        return None;
    }
    let path = if Path::new(src).is_absolute() {
        PathBuf::from(src)
    } else {
        doc_dir.join(src)
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        let bytes = std::fs::read(path).ok()?;
        return load_svg(&bytes);
    }
    image::open(path).ok().map(|dynamic| dynamic.to_rgba8())
}

/// Rasterizes an SVG at its intrinsic size. Uses resvg's own tiny-skia
/// pixmap and demultiplies into straight-alpha RGBA for the blit path.
fn load_svg(bytes: &[u8]) -> Option<RgbaImage> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width().max(1), size.height().max(1));
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut img = RgbaImage::new(width, height);
    for (target, px) in img.chunks_exact_mut(4).zip(pixmap.pixels()) {
        let c = px.demultiply();
        target.copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(img)
}

pub struct MediaCache {
    doc_dir: PathBuf,
    originals: HashMap<String, Option<RgbaImage>>,
    scaled: HashMap<(String, u32, u32), Vec<u8>>,
}

impl MediaCache {
    pub fn new(doc_dir: PathBuf) -> MediaCache {
        MediaCache {
            doc_dir,
            originals: HashMap::new(),
            scaled: HashMap::new(),
        }
    }

    fn original(&mut self, src: &str) -> Option<&RgbaImage> {
        if !self.originals.contains_key(src) {
            let loaded = load(&self.doc_dir, src);
            self.originals.insert(src.to_string(), loaded);
        }
        self.originals.get(src).and_then(|o| o.as_ref())
    }

    /// Natural pixel dimensions, or None when the image cannot load.
    pub fn dimensions(&mut self, src: &str) -> Option<(u32, u32)> {
        self.original(src).map(|img| img.dimensions())
    }

    /// RGBA bytes resized to exactly `width x height`, memoized per size.
    pub fn scaled(&mut self, src: &str, width: u32, height: u32) -> Option<&[u8]> {
        let key = (src.to_string(), width, height);
        if !self.scaled.contains_key(&key) {
            let resized = self.original(src).map(|img| {
                image::imageops::resize(img, width, height, image::imageops::FilterType::Triangle)
                    .into_raw()
            })?;
            self.scaled.insert(key.clone(), resized);
        }
        self.scaled.get(&key).map(|v| v.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(dir: &Path, name: &str, w: u32, h: u32) {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([200, 100, 50, 255]));
        img.save(dir.join(name)).unwrap();
    }

    #[test]
    fn svg_rasterizes_at_intrinsic_size() {
        let dir = temp_dir();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="30">
            <rect width="60" height="30" fill="#c87137"/></svg>"##;
        std::fs::write(dir.join("mark.svg"), svg).unwrap();
        let img = load(&dir, "mark.svg").expect("svg should load");
        assert_eq!(img.dimensions(), (60, 30));
        let px = img.get_pixel(30, 15);
        assert_eq!((px[0], px[1], px[2]), (0xC8, 0x71, 0x37));
    }

    #[test]
    fn loads_relative_to_doc_dir() {
        let dir = temp_dir();
        write_png(&dir, "a.png", 8, 4);
        let img = load(&dir, "a.png").unwrap();
        assert_eq!(img.dimensions(), (8, 4));
    }

    #[test]
    fn missing_and_remote_are_none() {
        let dir = temp_dir();
        assert!(load(&dir, "nope.png").is_none());
        assert!(load(&dir, "https://example.com/x.png").is_none());
    }

    #[test]
    fn cache_dimensions_and_scaling() {
        let dir = temp_dir();
        write_png(&dir, "b.png", 100, 50);
        let mut cache = MediaCache::new(dir);
        assert_eq!(cache.dimensions("b.png"), Some((100, 50)));
        let scaled = cache.scaled("b.png", 40, 20).unwrap();
        assert_eq!(scaled.len(), 40 * 20 * 4);
        assert_eq!(cache.dimensions("missing.png"), None);
        assert!(cache.scaled("missing.png", 10, 10).is_none());
    }
}
