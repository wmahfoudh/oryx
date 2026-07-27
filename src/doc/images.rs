//! Image loading and caching. Paths resolve against the document's
//! directory; failures and remote URLs yield None and render as
//! placeholders. Originals and scaled variants are memoized so relayout
//! and repaint never re-decode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::RgbaImage;

use crate::doc::fetch;

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

/// Wakes the event loop when a background fetch lands.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Decodes fetched bytes: SVG when the head looks like XML, raster
/// otherwise.
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    let head = &bytes[..bytes.len().min(512)];
    let looks_svg = std::str::from_utf8(head).is_ok_and(|t| {
        t.trim_start_matches('\u{feff}')
            .trim_start()
            .starts_with('<')
    });
    if looks_svg {
        load_svg(bytes)
    } else {
        image::load_from_memory(bytes)
            .ok()
            .map(|dynamic| dynamic.to_rgba8())
    }
}

/// A remote source that has no pixels yet.
enum RemoteState {
    Pending,
    Failed,
}

/// Fetch results queued by background threads until the main thread
/// folds them in.
type Arrivals = Arc<Mutex<Vec<(String, Option<RgbaImage>)>>>;

pub struct MediaCache {
    doc_dir: PathBuf,
    cache_dir: Option<PathBuf>,
    originals: HashMap<String, Option<RgbaImage>>,
    scaled: HashMap<(String, u32, u32), Vec<u8>>,
    remote: HashMap<String, RemoteState>,
    arrivals: Arrivals,
    waker: Option<Waker>,
}

impl MediaCache {
    pub fn new(doc_dir: PathBuf) -> MediaCache {
        Self::with_cache_dir(doc_dir, fetch::cache_dir())
    }

    /// The disk cache location is injectable so tests never touch the
    /// user's real cache.
    pub fn with_cache_dir(doc_dir: PathBuf, cache_dir: Option<PathBuf>) -> MediaCache {
        MediaCache {
            doc_dir,
            cache_dir,
            originals: HashMap::new(),
            scaled: HashMap::new(),
            remote: HashMap::new(),
            arrivals: Arc::new(Mutex::new(Vec::new())),
            waker: None,
        }
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// Folds arrived fetches into the cache; true when anything landed.
    pub fn drain_remote(&mut self) -> bool {
        let arrived: Vec<_> = {
            let mut arrivals = self.arrivals.lock().expect("arrivals lock");
            arrivals.drain(..).collect()
        };
        let changed = !arrived.is_empty();
        for (url, image) in arrived {
            match image {
                Some(image) => {
                    self.originals.insert(url.clone(), Some(image));
                    self.remote.remove(&url);
                }
                None => {
                    self.remote.insert(url, RemoteState::Failed);
                }
            }
        }
        changed
    }

    fn is_remote(src: &str) -> bool {
        src.starts_with("http://") || src.starts_with("https://")
    }

    /// Remote lookup: memory, then the disk cache, then a background
    /// fetch. Always returns at once; missing pixels mean a placeholder.
    fn remote_original(&mut self, src: &str) -> Option<&RgbaImage> {
        if !self.originals.contains_key(src) && !self.remote.contains_key(src) {
            match self.cached_bytes(src).as_deref().map(decode) {
                Some(Some(image)) => {
                    self.originals.insert(src.to_string(), Some(image));
                }
                // A truncated write leaves undecodable bytes on disk;
                // pinning them would blank the URL for every session.
                Some(None) => {
                    self.remove_cached(src);
                    self.spawn_fetch(src);
                }
                None => self.spawn_fetch(src),
            }
        }
        self.originals.get(src).and_then(|o| o.as_ref())
    }

    fn remove_cached(&self, src: &str) {
        if let Some(dir) = &self.cache_dir {
            let _ = std::fs::remove_file(dir.join(fetch::key(src)));
        }
    }

    fn cached_bytes(&self, src: &str) -> Option<Vec<u8>> {
        std::fs::read(self.cache_dir.as_ref()?.join(fetch::key(src))).ok()
    }

    fn spawn_fetch(&mut self, src: &str) {
        self.remote.insert(src.to_string(), RemoteState::Pending);
        let url = src.to_string();
        let cache_dir = self.cache_dir.clone();
        let arrivals = Arc::clone(&self.arrivals);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let bytes = fetch::fetch(&url);
            if let (Some(dir), Some(bytes)) = (&cache_dir, &bytes) {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(fetch::key(&url)), bytes);
            }
            let image = bytes.as_deref().and_then(decode);
            arrivals.lock().expect("arrivals lock").push((url, image));
            if let Some(wake) = waker {
                wake();
            }
        });
    }

    fn original(&mut self, src: &str) -> Option<&RgbaImage> {
        if Self::is_remote(src) {
            return self.remote_original(src);
        }
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

    /// A truncated cache write must not pin its URL to a placeholder
    /// forever: the bad entry goes and a fetch replaces it. The address
    /// is TEST-NET, so the background attempt goes nowhere.
    #[test]
    fn a_corrupt_cache_entry_heals_by_refetching() {
        let dir = temp_dir();
        let cache = dir.join("corrupt-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let url = "https://192.0.2.1/badge.png";
        std::fs::write(cache.join(fetch::key(url)), b"not an image").unwrap();
        let mut media = MediaCache::with_cache_dir(dir.clone(), Some(cache.clone()));
        assert!(media.dimensions(url).is_none());
        assert!(
            !cache.join(fetch::key(url)).exists(),
            "the bad entry is removed"
        );
        assert!(media.remote.contains_key(url), "a refetch is under way");
        assert!(
            !media.originals.contains_key(url),
            "no placeholder is pinned"
        );
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

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn decode_sniffs_svg_and_raster() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <rect width="40" height="20" fill="#112233"/></svg>"##;
        assert_eq!(decode(svg.as_bytes()).unwrap().dimensions(), (40, 20));
        assert_eq!(decode(&png_bytes(8, 4)).unwrap().dimensions(), (8, 4));
        assert!(decode(b"not an image at all").is_none());
    }

    #[test]
    fn cached_remote_loads_without_network() {
        let cache = temp_dir().join("cache-hit");
        std::fs::create_dir_all(&cache).unwrap();
        let url = "https://img.example/badge.png";
        std::fs::write(cache.join(crate::doc::fetch::key(url)), png_bytes(8, 4)).unwrap();
        let mut media = MediaCache::with_cache_dir(temp_dir(), Some(cache.clone()));
        assert_eq!(media.dimensions(url), Some((8, 4)));
        std::fs::remove_dir_all(&cache).unwrap();
    }

    #[test]
    fn pending_remote_yields_placeholder_without_blocking() {
        let cache = temp_dir().join("cache-miss");
        let mut media = MediaCache::with_cache_dir(temp_dir(), Some(cache.clone()));
        // Nothing listens on this port; the fetch fails in the background
        // while the placeholder answer returns at once.
        let url = "https://127.0.0.1:1/x.png";
        assert_eq!(media.dimensions(url), None);
        assert_eq!(media.dimensions(url), None);
        let _ = std::fs::remove_dir_all(&cache);
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
