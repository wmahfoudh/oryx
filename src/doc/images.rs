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
    // Enumerating the system fonts costs a noticeable pause and SVGs
    // decode on the layout path, so the database is built exactly once.
    static FONTDB: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    let fontdb = Arc::clone(FONTDB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        // The embedded faces answer the generic CSS families, so an
        // SVG's text renders the same on every machine instead of
        // depending on which platform names happen to exist.
        for bytes in crate::style::fonts::EMBEDDED {
            db.load_font_data(bytes.to_vec());
        }
        db.set_serif_family(crate::style::fonts::BODY_FAMILY);
        db.set_sans_serif_family(crate::style::fonts::BODY_FAMILY);
        db.set_cursive_family(crate::style::fonts::BODY_FAMILY);
        db.set_fantasy_family(crate::style::fonts::BODY_FAMILY);
        db.set_monospace_family(crate::style::fonts::CODE_FAMILY);
        Arc::new(db)
    }));
    let options = resvg::usvg::Options {
        fontdb,
        ..Default::default()
    };
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

/// Where background decodes land: a key and its pixels, None for a
/// failure that should pin a placeholder.
pub type ImageSink = Arc<dyn Fn(String, Option<RgbaImage>) + Send + Sync>;

/// A book image's stored source: bytes still encoded, markup still
/// text, enough to decode on demand.
#[derive(Clone, Debug)]
pub enum BookSource {
    Raster(Vec<u8>),
    Svg(String),
    /// A raw deflate stream around the encoded image, as a zip entry
    /// stores it; it stays compressed until a decode asks, so opening a
    /// deflated comic never inflates the whole book.
    Deflated(Vec<u8>),
}

/// The most a single deflated image may inflate to.
const INFLATE_CEILING: usize = 1 << 28;

/// How much of a deflated image inflates for a header probe; every
/// raster header sits well inside it.
const PROBE_WINDOW: usize = 1 << 18;

/// The head of a raw deflate stream, up to `limit` bytes: whatever
/// inflated cleanly when the stream is short, the filled window when it
/// is not.
fn inflate_head(raw: &[u8], limit: usize) -> Vec<u8> {
    match miniz_oxide::inflate::decompress_to_vec_with_limit(raw, limit) {
        Ok(bytes) => bytes,
        Err(err) => err.output,
    }
}

/// Decodes a stored source into pixels.
pub fn decode_source(source: &BookSource) -> Option<RgbaImage> {
    match source {
        BookSource::Raster(bytes) => decode(bytes),
        BookSource::Svg(markup) => decode(markup.as_bytes()),
        BookSource::Deflated(raw) => {
            let bytes =
                miniz_oxide::inflate::decompress_to_vec_with_limit(raw, INFLATE_CEILING).ok()?;
            decode(&bytes)
        }
    }
}

/// Pixel dimensions from a source's header alone, no pixel decoded:
/// the raster header for image files, the parsed tree for svg. The svg
/// parse takes no font database; the canvas size never depends on text.
pub fn probe_source(source: &BookSource) -> Option<(u32, u32)> {
    fn svg_size(bytes: &[u8]) -> Option<(u32, u32)> {
        let options = resvg::usvg::Options::default();
        let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
        let size = tree.size().to_int_size();
        Some((size.width().max(1), size.height().max(1)))
    }
    fn raster_size(bytes: &[u8]) -> Option<(u32, u32)> {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    }
    match source {
        BookSource::Svg(markup) => svg_size(markup.as_bytes()),
        BookSource::Raster(bytes) if looks_svg(bytes) => svg_size(bytes),
        BookSource::Raster(bytes) => raster_size(bytes),
        // A few KB cover almost every header; the wide window only pays
        // for the rare image whose metadata pushes the header deep.
        BookSource::Deflated(raw) => raster_size(&inflate_head(raw, 1 << 13))
            .or_else(|| raster_size(&inflate_head(raw, PROBE_WINDOW))),
    }
}

/// What a drained batch of arrivals asks of the caller; a batch takes
/// the strongest answer among its arrivals.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Folded {
    Nothing,
    /// Pixels for sizes already known: the band repaints.
    Repaint,
    /// A size that was unknown until now: layout runs again.
    Relayout,
}

/// One stored book image: its encoded source and header dimensions.
struct BookImage {
    source: BookSource,
    dims: (u32, u32),
}

/// The budget for decoded book originals, about twenty screenshots:
/// enough that the visible region and zoom stay warm, small enough that
/// a book never holds every original at once.
const BOOK_BUDGET: usize = 64 << 20;

fn rgba_bytes(image: &RgbaImage) -> usize {
    image.width() as usize * image.height() as usize * 4
}

/// A handful of decode threads behind one queue. Dropping the pool
/// closes the queue; workers drain what remains and exit on their own.
struct DecodePool {
    sender: std::sync::mpsc::Sender<(String, BookSource)>,
}

impl DecodePool {
    fn spawn(sink: ImageSink) -> DecodePool {
        let (sender, receiver) = std::sync::mpsc::channel::<(String, BookSource)>();
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        for _ in 0..workers {
            let receiver = Arc::clone(&receiver);
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || loop {
                let job = receiver.lock().expect("decode queue").recv();
                match job {
                    Ok((key, source)) => {
                        let image = decode_source(&source);
                        sink(key, image);
                    }
                    Err(_) => break,
                }
            });
        }
        DecodePool { sender }
    }

    fn send(&self, jobs: Vec<(String, BookSource)>) {
        for job in jobs {
            let _ = self.sender.send(job);
        }
    }
}

/// Whether bytes read as markup rather than a raster header.
fn looks_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    std::str::from_utf8(head).is_ok_and(|t| {
        t.trim_start_matches('\u{feff}')
            .trim_start()
            .starts_with('<')
    })
}

/// Decodes fetched bytes: SVG when the head looks like XML, raster
/// otherwise.
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    if looks_svg(bytes) {
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

/// A book image source with its key and header dimensions, as the
/// walker hands them over.
pub type SourceEntry = (String, BookSource, Option<(u32, u32)>);

/// Where the walker's image sources land, ahead of any pixel.
pub type SourceSink = Arc<dyn Fn(Vec<SourceEntry>) + Send + Sync>;

/// One queued arrival from a background thread, folded in by the main
/// thread in queue order, so a source always lands before its pixels.
enum Arrival {
    Pixels(String, Option<RgbaImage>),
    Sources(Vec<SourceEntry>),
}

/// Results queued by background threads until the main thread folds
/// them in.
type Arrivals = Arc<Mutex<Vec<Arrival>>>;

pub struct MediaCache {
    doc_dir: PathBuf,
    cache_dir: Option<PathBuf>,
    originals: HashMap<String, Option<RgbaImage>>,
    scaled: HashMap<(String, u32, u32), Vec<u8>>,
    remote: HashMap<String, RemoteState>,
    /// Book image sources by key: sizes answer from here, pixels decode
    /// on demand.
    book: HashMap<String, BookImage>,
    /// Book keys with a decode in flight, so a repaint asks only once.
    decoding: std::collections::HashSet<String>,
    /// Decoded book originals by recency, oldest first, and their byte
    /// total against the budget.
    lru: Vec<String>,
    lru_bytes: usize,
    book_budget: usize,
    /// The on-demand decode pool, spawned on first use and living with
    /// the cache; dropping the cache closes its queue.
    pool: Option<DecodePool>,
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
            book: HashMap::new(),
            decoding: std::collections::HashSet::new(),
            lru: Vec::new(),
            lru_bytes: 0,
            book_budget: BOOK_BUDGET,
            pool: None,
            arrivals: Arc::new(Mutex::new(Vec::new())),
            waker: None,
        }
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// Adopts book image sources: bytes and header dimensions per key.
    /// A source whose header did not read pins the placeholder.
    pub fn adopt(&mut self, sources: Vec<SourceEntry>) {
        for (key, source, dims) in sources {
            match dims {
                Some(dims) => {
                    self.book.insert(key, BookImage { source, dims });
                }
                None => {
                    self.originals.insert(key, None);
                }
            }
        }
    }

    /// Lands decoded book pixels under the budget: the newest original
    /// joins the recency list and the oldest leave until the total
    /// fits. A None pins the placeholder and costs nothing.
    fn adopt_pixels(&mut self, key: String, image: Option<RgbaImage>) {
        self.drop_book_original(&key);
        if let Some(image) = &image {
            self.lru_bytes += rgba_bytes(image);
            self.lru.push(key.clone());
        }
        self.originals.insert(key, image);
        while self.lru_bytes > self.book_budget && self.lru.len() > 1 {
            let oldest = self.lru[0].clone();
            self.drop_book_original(&oldest);
        }
    }

    fn drop_book_original(&mut self, key: &str) {
        let Some(position) = self.lru.iter().position(|k| k == key) else {
            return;
        };
        self.lru.remove(position);
        if let Some(Some(image)) = self.originals.remove(key) {
            self.lru_bytes -= rgba_bytes(&image);
        }
    }

    /// Marks a decoded book original as just used.
    fn touch(&mut self, key: &str) {
        if let Some(position) = self.lru.iter().position(|k| k == key) {
            let key = self.lru.remove(position);
            self.lru.push(key);
        }
    }

    /// Decodes a book image now, for the export pass that reads pixels
    /// synchronously; a warm or non-book key is untouched.
    pub fn warm(&mut self, src: &str) {
        if self.originals.contains_key(src) || !self.book.contains_key(src) {
            return;
        }
        let image = decode_source(&self.book[src].source);
        self.adopt_pixels(src.to_string(), image);
    }

    /// Queues a stored source on the pool once; the arrival folds in
    /// through the queue like any decoded image.
    fn queue_decode(&mut self, src: &str) {
        if self.decoding.contains(src) {
            return;
        }
        let Some(entry) = self.book.get(src) else {
            return;
        };
        let job = (src.to_string(), entry.source.clone());
        self.decoding.insert(src.to_string());
        let sink = self.feeder();
        self.pool
            .get_or_insert_with(|| DecodePool::spawn(sink))
            .send(vec![job]);
    }

    /// Folds arrived results into the cache, answering what changed. A
    /// book image's size was known from its header, so its pixels only
    /// repaint; a remote fetch's size lands with it, so layout reruns.
    /// A sources batch registers sizes for blocks not yet laid out and
    /// asks nothing by itself.
    pub fn drain_remote(&mut self) -> Folded {
        let arrived: Vec<_> = {
            let mut arrivals = self.arrivals.lock().expect("arrivals lock");
            arrivals.drain(..).collect()
        };
        let mut folded = Folded::Nothing;
        for arrival in arrived {
            let (url, image) = match arrival {
                Arrival::Sources(sources) => {
                    self.adopt(sources);
                    continue;
                }
                Arrival::Pixels(url, image) => (url, image),
            };
            self.decoding.remove(&url);
            let book = self.book.contains_key(&url);
            folded = folded.max(if book {
                Folded::Repaint
            } else {
                Folded::Relayout
            });
            match image {
                _ if book => self.adopt_pixels(url, image),
                Some(image) => {
                    self.originals.insert(url.clone(), Some(image));
                    self.remote.remove(&url);
                }
                None => {
                    self.remote.insert(url, RemoteState::Failed);
                }
            }
        }
        folded
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
            arrivals
                .lock()
                .expect("arrivals lock")
                .push(Arrival::Pixels(url, image));
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
            // A book image decodes on demand: queue it and answer the
            // placeholder until the pixels fold in.
            if self.book.contains_key(src) {
                self.queue_decode(src);
                return None;
            }
            let loaded = load(&self.doc_dir, src);
            self.originals.insert(src.to_string(), loaded);
        }
        self.touch(src);
        self.originals.get(src).and_then(|o| o.as_ref())
    }

    /// A sink feeding this cache's arrivals queue and waking the loop,
    /// for the book decode pool. Arrivals fold in through
    /// `drain_remote` like any fetched image; a sink outliving the
    /// cache feeds a dead queue harmlessly.
    pub fn feeder(&self) -> ImageSink {
        let arrivals = Arc::clone(&self.arrivals);
        let waker = self.waker.clone();
        Arc::new(move |key, image| {
            arrivals
                .lock()
                .expect("arrivals lock")
                .push(Arrival::Pixels(key, image));
            if let Some(wake) = &waker {
                wake();
            }
        })
    }

    /// The sources counterpart of `feeder`: the walker hands each
    /// chapter's image sources over ahead of any decode of them.
    pub fn source_sink(&self) -> SourceSink {
        let arrivals = Arc::clone(&self.arrivals);
        let waker = self.waker.clone();
        Arc::new(move |sources| {
            if sources.is_empty() {
                return;
            }
            arrivals
                .lock()
                .expect("arrivals lock")
                .push(Arrival::Sources(sources));
            if let Some(wake) = &waker {
                wake();
            }
        })
    }

    /// Natural pixel dimensions, or None when the image cannot load. A
    /// book image answers from its stored header without any decode.
    pub fn dimensions(&mut self, src: &str) -> Option<(u32, u32)> {
        if let Some(entry) = self.book.get(src) {
            return Some(entry.dims);
        }
        self.original(src).map(|img| img.dimensions())
    }

    /// Drops the resized buffers. A new layout pass changes the placed
    /// sizes, so every entry is dead weight the moment one begins.
    pub fn clear_scaled(&mut self) {
        self.scaled.clear();
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

    /// Generic CSS families resolve to the faces Oryx embeds, so an SVG
    /// badge's label renders the same on every machine instead of
    /// vanishing when the platform lacks the database's default names.
    #[test]
    fn svg_text_renders_through_the_embedded_faces() {
        let dir = temp_dir();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40">
            <rect width="120" height="40" fill="#000"/>
            <text x="10" y="28" font-family="sans-serif" font-size="20" fill="#fff">Ab</text>
        </svg>"##;
        std::fs::write(dir.join("text.svg"), svg).unwrap();
        let img = load(&dir, "text.svg").expect("svg loads");
        let lit = img.pixels().filter(|p| p[0] > 128 && p[3] > 128).count();
        assert!(lit > 20, "the glyphs left {lit} lit pixels");
    }

    #[test]
    fn a_new_pass_drops_the_scaled_buffers() {
        let dir = temp_dir();
        write_png(&dir, "evict.png", 40, 20);
        let mut media = MediaCache::new(dir);
        assert!(media.scaled("evict.png", 20, 10).is_some());
        assert!(!media.scaled.is_empty());
        media.clear_scaled();
        assert!(media.scaled.is_empty(), "a new pass starts clean");
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
    fn a_stored_book_source_answers_dimensions_without_pixels() {
        let mut media = MediaCache::new(temp_dir());
        let source = BookSource::Raster(png_bytes(8, 4));
        let dims = probe_source(&source);
        assert_eq!(dims, Some((8, 4)), "the header answers the size");
        media.adopt(vec![("book/pic.png".to_string(), source, dims)]);
        assert_eq!(media.dimensions("book/pic.png"), Some((8, 4)));
        assert!(media.originals.is_empty(), "no pixel decoded");
    }

    /// An svg answers from its parsed attributes, both as inline markup
    /// and as raster bytes that turn out to be svg, the `<img
    /// src="x.svg">` case.
    #[test]
    fn an_svg_source_answers_its_intrinsic_size() {
        let markup = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="30">
            <rect width="60" height="30" fill="#c87137"/></svg>"##;
        assert_eq!(
            probe_source(&BookSource::Svg(markup.to_string())),
            Some((60, 30))
        );
        assert_eq!(
            probe_source(&BookSource::Raster(markup.as_bytes().to_vec())),
            Some((60, 30))
        );
    }

    /// A source whose header does not read pins the placeholder the way
    /// a failed load always has.
    #[test]
    fn an_unreadable_source_pins_the_placeholder() {
        let mut media = MediaCache::new(temp_dir());
        let source = BookSource::Raster(b"not an image".to_vec());
        media.adopt(vec![("book/bad.bin".to_string(), source, None)]);
        assert_eq!(media.dimensions("book/bad.bin"), None);
        assert!(
            matches!(media.originals.get("book/bad.bin"), Some(None)),
            "the placeholder is pinned"
        );
    }

    /// Paint's ask for a cold book image draws the placeholder, queues
    /// exactly one decode, and finds the pixels after the arrival folds.
    #[test]
    fn a_cold_book_image_decodes_on_demand_once() {
        let mut media = MediaCache::new(temp_dir());
        let source = BookSource::Raster(png_bytes(8, 4));
        media.adopt(vec![("book/pic.png".to_string(), source, Some((8, 4)))]);
        assert!(
            media.scaled("book/pic.png", 4, 2).is_none(),
            "a cold ask answers the placeholder"
        );
        assert!(media.scaled("book/pic.png", 4, 2).is_none());
        assert_eq!(media.decoding.len(), 1, "one decode in flight, not two");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if media.drain_remote() == Folded::Repaint {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the decode never landed"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(media.scaled("book/pic.png", 4, 2).is_some());
        assert!(media.decoding.is_empty());
    }

    /// Decoded book originals hold a byte budget: past it the least
    /// recently touched leave, and a return decodes again.
    #[test]
    fn book_originals_evict_past_the_budget_oldest_first() {
        let mut media = MediaCache::new(temp_dir());
        // Two 8x4 rgba originals fit, a third does not.
        media.book_budget = 300;
        for name in ["a", "b", "c"] {
            media.adopt(vec![(
                format!("book/{name}.png"),
                BookSource::Raster(png_bytes(8, 4)),
                Some((8, 4)),
            )]);
        }
        media.warm("book/a.png");
        media.warm("book/b.png");
        media.warm("book/c.png");
        assert!(
            !media.originals.contains_key("book/a.png"),
            "the oldest left"
        );
        assert!(media.originals.contains_key("book/b.png"));
        assert!(media.originals.contains_key("book/c.png"));
        media.warm("book/a.png");
        assert!(
            media.originals.contains_key("book/a.png"),
            "a return decodes again"
        );
        assert!(
            !media.originals.contains_key("book/b.png"),
            "now the oldest leaves"
        );
    }

    /// A book image's size is known from its header, so its pixels only
    /// repaint; a remote fetch's size is unknown until it lands, so it
    /// still relayouts.
    #[test]
    fn a_book_arrival_repaints_a_remote_one_relayouts() {
        let mut media = MediaCache::new(temp_dir());
        let source = BookSource::Raster(png_bytes(8, 4));
        media.adopt(vec![("book/pic.png".to_string(), source, Some((8, 4)))]);
        let sink = media.feeder();
        sink("book/pic.png".to_string(), Some(RgbaImage::new(8, 4)));
        assert_eq!(media.drain_remote(), Folded::Repaint);
        sink("https://x/y.png".to_string(), Some(RgbaImage::new(2, 2)));
        assert_eq!(media.drain_remote(), Folded::Relayout);
        assert_eq!(media.drain_remote(), Folded::Nothing);
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
