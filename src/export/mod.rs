//! PDF export: page geometry, the resumable pass, and the settings that
//! drive them. Pagination and emission live in the two child modules.

pub mod paginate;
pub mod pdf;

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use std::time::Duration;

use crate::doc::images::MediaCache;
use crate::doc::model::Document;
use crate::export::paginate::{Page, Paginator};
use crate::export::pdf::Builder;
use crate::layout::pool::{Job, ShapeCtx, TextJob, Work};
use crate::layout::{
    layout_begin, layout_more, metrics, LayoutDoc, LayoutPass, StepKey, ViewConfig,
};

/// How much layout runs before pagination and emission get their turn
/// inside one fused slice: small enough that pages flow while layout
/// streams, large enough that the pass keeps its pooled throughput.
const LAYOUT_CHUNK: Duration = Duration::from_millis(4);

/// How many queued pages hold seeded shaping at once, enough to keep
/// the workers ahead of the writer without the job queue ever holding
/// the whole document's text.
const SEED_AHEAD: usize = 32;
use crate::platform::config::Config;
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

/// The sheet an export writes onto; `points` is its portrait size and
/// the orientation turns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageSize {
    A4,
    Letter,
    Legal,
    A5,
    #[serde(rename = "6x9")]
    SixByNine,
    #[serde(rename = "5x8")]
    FiveByEight,
}

impl PageSize {
    /// Sheet size in PDF points. One point is one layout unit, so the
    /// engine lays a page out the way it lays a window out. The office
    /// sizes came first; the book trim sizes joined for native-size
    /// book exports, 6 by 9 the common print-on-demand trade format.
    pub fn points(self) -> (f32, f32) {
        match self {
            PageSize::A4 => (595.28, 841.89),
            PageSize::Letter => (612.0, 792.0),
            PageSize::Legal => (612.0, 1008.0),
            PageSize::A5 => (419.53, 595.28),
            PageSize::SixByNine => (432.0, 648.0),
            PageSize::FiveByEight => (360.0, 576.0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::Letter => "Letter",
            PageSize::Legal => "Legal",
            PageSize::A5 => "A5",
            PageSize::SixByNine => "6 x 9 in",
            PageSize::FiveByEight => "5 x 8 in",
        }
    }
}

/// Which way the sheet turns: landscape swaps its axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn label(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }
}

/// What an export renders with, held apart from the appearance settings
/// so a reader on a dark theme exports light without switching themes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub theme: String,
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    pub page: PageSize,
    pub orientation: Orientation,
    pub page_numbers: bool,
    /// Justify book prose in the exported pages; EPUB documents only,
    /// the pass clears it for anything else.
    pub justify: bool,
}

impl ExportSettings {
    /// The values an export starts life with: whatever the reader is
    /// looking at, on portrait A4, numbered.
    pub fn seeded_from(config: &Config) -> ExportSettings {
        ExportSettings {
            theme: config.theme.clone(),
            body_family: config.body_family.clone(),
            code_family: config.code_family.clone(),
            body_size: config.body_size,
            code_size: config.code_size,
            page: PageSize::A4,
            orientation: Orientation::Portrait,
            page_numbers: true,
            justify: true,
        }
    }
}

impl Default for ExportSettings {
    fn default() -> ExportSettings {
        ExportSettings::seeded_from(&Config::default())
    }
}

/// The sheet and the content box inside it, in points. Both margins come
/// from the screen's own rules, so a page carries the proportions the
/// document has in a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub width: f32,
    pub height: f32,
    pub margin_x: f32,
    pub margin_y: f32,
}

impl PageGeometry {
    pub fn new(page: PageSize, orientation: Orientation, body_size: f32) -> PageGeometry {
        let (width, height) = match orientation {
            Orientation::Portrait => page.points(),
            Orientation::Landscape => {
                let (w, h) = page.points();
                (h, w)
            }
        };
        PageGeometry {
            width,
            height,
            margin_x: metrics::MARGIN_RATIO * width,
            margin_y: metrics::VERTICAL_MARGIN_EM * body_size,
        }
    }

    /// Height available to content once both margins are taken.
    pub fn content_height(&self) -> f32 {
        self.height - 2.0 * self.margin_y
    }
}

/// The theme an export renders with: the one it names, or the active one
/// when that file is gone. The flag is what lets the result line say so
/// instead of the reader finding out from the PDF.
pub fn resolve_theme(dirs: &[PathBuf], wanted: &str, active: &Theme) -> (Theme, bool) {
    match crate::style::theme::find(dirs, wanted) {
        Some(theme) => (theme, false),
        None => (active.clone(), true),
    }
}

/// How far a running export has got. Layout, pagination and emission
/// are one fused phase: a page is written the moment layout passes its
/// bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Highlight,
    Emit,
    Faces,
    Done,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Highlight => "Colouring code",
            Phase::Emit => "Writing pages",
            Phase::Faces => "Embedding fonts",
            Phase::Done => "Finishing",
        }
    }
}

/// What the progress overlay draws: the phase, and the page count once
/// pagination has fixed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
}

/// A resumable export. Every phase stops at a deadline, so a document of
/// any size is written without the window losing its frame.
pub struct ExportPass {
    settings: ExportSettings,
    theme: Theme,
    cfg: ViewConfig,
    geometry: PageGeometry,
    target: PathBuf,
    /// The sibling temporary file pages flush to; the finish renames it
    /// onto the target and a dropped pass removes it.
    part: PathBuf,
    title: String,
    phase: Phase,
    layout: LayoutDoc,
    pass: Option<LayoutPass>,
    paginator: Paginator,
    /// Pages closed and awaiting writing: the page, its emission key,
    /// and whether its shaping is seeded.
    queue: std::collections::VecDeque<(Page, usize, bool)>,
    /// Emission keys handed out, ascending across the export.
    seeded: usize,
    /// The context emission jobs ride, built once per export.
    ctx: Option<std::sync::Arc<ShapeCtx>>,
    /// Pages written so far; the total once the pass ends.
    emitted: usize,
    /// Created with the `.part` file when emission starts.
    builder: Option<Builder>,
    failed: Option<String>,
    /// A book's table of contents, driving the PDF outline; empty for
    /// files.
    toc: Vec<crate::doc::epub::TocEntry>,
}

impl ExportPass {
    /// Hands the pass a book's table of contents for the PDF outline.
    pub fn with_toc(mut self, toc: Vec<crate::doc::epub::TocEntry>) -> ExportPass {
        self.toc = toc;
        self
    }

    pub fn new(settings: &ExportSettings, theme: Theme, target: PathBuf) -> ExportPass {
        let geometry = PageGeometry::new(settings.page, settings.orientation, settings.body_size);
        let title = target
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut part = target.clone().into_os_string();
        part.push(".part");
        ExportPass {
            settings: settings.clone(),
            theme,
            cfg: ViewConfig {
                body_family: settings.body_family.clone(),
                code_family: settings.code_family.clone(),
                body_size: settings.body_size,
                code_size: settings.code_size,
                zoom: 1.0,
                justify: settings.justify,
            },
            geometry,
            target,
            part: PathBuf::from(part),
            title,
            phase: Phase::Highlight,
            layout: LayoutDoc::default(),
            pass: None,
            paginator: Paginator::new(),
            queue: std::collections::VecDeque::new(),
            seeded: 0,
            ctx: None,
            emitted: 0,
            builder: None,
            failed: None,
            toc: Vec::new(),
        }
    }

    /// Seeds pooled shaping for the queue's front pages, a bounded
    /// window so the job queue never holds the whole document's text;
    /// without a pool the writer shapes serially.
    fn seed_window(&mut self, doc: &Document) {
        let Some((pool, generation)) = self.pass.as_ref().and_then(|pass| pass.pool_state()) else {
            return;
        };
        let Some(ctx) = self.ctx.clone() else {
            return;
        };
        for index in 0..self.queue.len().min(SEED_AHEAD) {
            if self.queue[index].2 {
                continue;
            }
            let (page, key) = (&self.queue[index].0, self.queue[index].1);
            let runs = self.layout.runs[page.runs.clone()]
                .iter()
                .map(|run| TextJob {
                    text: self.layout.run_text(doc, run).to_string(),
                    family: self.layout.run_family(run).to_string(),
                    weight: run.weight,
                    italic: run.italic,
                    size: run.size,
                    x: run.x,
                })
                .collect();
            pool.submit(Job {
                generation,
                key: StepKey::text(key),
                ctx: std::sync::Arc::clone(&ctx),
                work: Work::Text { runs },
            });
            self.queue[index].2 = true;
        }
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Advances until the deadline and reports where it stopped. The
    /// highlight phase idles while the worker still has blocks to colour,
    /// since a PDF cannot wash in after it is written. Past it, layout,
    /// pagination and emission run fused: layout advances a small chunk,
    /// every page it finalized is written and its geometry dropped, and
    /// the loop repeats until the deadline.
    pub fn step(
        &mut self,
        deadline: Instant,
        doc: &Document,
        fonts: &mut FontStore,
        media: &mut MediaCache,
        highlighting: bool,
        pool: Option<&std::sync::Arc<crate::layout::ShapePool>>,
    ) -> Progress {
        // Justification is book typography: whatever the setting says,
        // anything that is not a book exports at natural width.
        self.cfg.justify = self.settings.justify && doc.book_id.is_some();
        if self.phase == Phase::Highlight {
            if highlighting {
                return self.progress();
            }
            self.phase = Phase::Emit;
        }
        if self.phase == Phase::Emit {
            if self.builder.is_none() {
                match Builder::to_file(&self.part) {
                    Ok(builder) => self.builder = Some(builder),
                    Err(error) => {
                        self.failed = Some(error.to_string());
                        self.phase = Phase::Done;
                        return self.progress();
                    }
                }
            }
            if self.pass.is_none() {
                let (out, mut pass) = layout_begin(doc, &self.cfg, self.geometry.width);
                if let Some(pool) = pool {
                    pass.attach_pool(std::sync::Arc::clone(pool));
                }
                self.layout = out;
                self.pass = Some(pass);
                self.ctx = Some(std::sync::Arc::new(ShapeCtx {
                    theme: self.theme.clone(),
                    cfg: self.cfg.clone(),
                    source: std::sync::Arc::clone(&doc.source),
                }));
            }
            loop {
                let pass = self.pass.as_mut().expect("emission has a pass");
                let chunk = Instant::now() + LAYOUT_CHUNK;
                let complete = layout_more(
                    doc,
                    &self.theme,
                    fonts,
                    media,
                    &self.cfg,
                    &mut self.layout,
                    pass,
                    Some(chunk.min(deadline)),
                );
                // Close what the grown layout decides, keep a bounded
                // window of their shaping seeded, and write what the
                // workers already got to; the deadline can interrupt
                // the writing but never a page.
                let pages = self
                    .paginator
                    .advance(doc, &self.layout, &self.geometry, complete);
                for page in pages {
                    let key = self.seeded;
                    self.seeded += 1;
                    self.queue.push_back((page, key, false));
                }
                self.seed_window(doc);
                let mut out_of_time = false;
                while let Some((page, key, seeded)) = self.queue.pop_front() {
                    let shaped = seeded
                        .then(|| {
                            self.pass
                                .as_ref()
                                .and_then(|pass| pass.pool_state())
                                .and_then(|(pool, generation)| {
                                    pool.take(generation, StepKey::text(key))
                                })
                                .map(|shaped| shaped.text)
                        })
                        .flatten();
                    let job = crate::export::pdf::Job {
                        doc,
                        layout: &self.layout,
                        theme: &self.theme,
                        geometry: &self.geometry,
                        settings: &self.settings,
                        title: &self.title,
                        toc: &self.toc,
                    };
                    let builder = self.builder.as_mut().expect("emission has a builder");
                    builder.add_page_shaped(&job, &page, fonts, media, shaped);
                    self.emitted += 1;
                    self.seed_window(doc);
                    if Instant::now() >= deadline && !self.queue.is_empty() {
                        out_of_time = true;
                        break;
                    }
                }
                // The written pages' geometry goes once nothing queued
                // still reads it.
                if self.queue.is_empty() {
                    let consumed = self.paginator.consume();
                    if consumed.runs > 0
                        || consumed.rects > 0
                        || consumed.images > 0
                        || consumed.math > 0
                    {
                        self.layout.drain_front(
                            consumed.runs,
                            consumed.rects,
                            consumed.images,
                            consumed.math,
                        );
                        self.pass
                            .as_mut()
                            .expect("emission has a pass")
                            .rebase(consumed.rects);
                    }
                }
                if complete && self.queue.is_empty() {
                    self.phase = Phase::Faces;
                    break;
                }
                if out_of_time || Instant::now() >= deadline {
                    return self.progress();
                }
            }
        }
        if self.phase == Phase::Faces {
            let builder = self.builder.as_mut().expect("the pass emitted");
            loop {
                match builder.write_face_step(fonts) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.phase = Phase::Done;
                        break;
                    }
                    Err(error) => {
                        self.failed = Some(error);
                        self.phase = Phase::Done;
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    return self.progress();
                }
            }
        }
        self.progress()
    }

    pub fn progress(&self) -> Progress {
        Progress {
            phase: self.phase,
            done: self.emitted,
            total: if self.phase == Phase::Done || self.phase == Phase::Faces {
                self.emitted
            } else {
                0
            },
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Closes the `.part` file and renames it onto the target, so a
    /// full disk or a refused permission leaves the target as it was.
    /// Reports the page count.
    pub fn finish(mut self, doc: &Document, fonts: &FontStore) -> std::io::Result<usize> {
        if let Some(failed) = self.failed.take() {
            return Err(std::io::Error::other(failed));
        }
        let pages = self.emitted;
        let builder = match self.builder.take() {
            Some(builder) => builder,
            // Nothing ever emitted: an empty document still lands a file.
            None => Builder::to_file(&self.part)?,
        };
        let job = crate::export::pdf::Job {
            doc,
            layout: &self.layout,
            theme: &self.theme,
            geometry: &self.geometry,
            settings: &self.settings,
            title: &self.title,
            toc: &self.toc,
        };
        builder.finish(&job, fonts).map_err(std::io::Error::other)?;
        std::fs::rename(&self.part, &self.target)?;
        Ok(pages)
    }
}

impl Drop for ExportPass {
    /// A dropped pass is a cancelled or finished one either way: the
    /// `.part` file has been renamed away or must not survive.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.part);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_uses_the_same_margin_rules_as_the_screen() {
        let g = PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0);
        assert_eq!((g.width, g.height), (595.28, 841.89));
        assert!((g.margin_x - 0.08 * 595.28).abs() < 0.01, "8 percent sides");
        assert_eq!(g.margin_y, 22.0, "2em of the body size");
        assert!((g.content_height() - (841.89 - 44.0)).abs() < 0.01);
    }

    #[test]
    fn landscape_swaps_the_sheets_axes() {
        let g = PageGeometry::new(PageSize::A4, Orientation::Landscape, 11.0);
        assert_eq!((g.width, g.height), (841.89, 595.28));
        assert!(
            (g.margin_x - 0.08 * 841.89).abs() < 0.01,
            "margins follow the turned sheet"
        );
        assert_eq!(g.margin_y, 22.0, "vertical margin still 2em");
    }

    #[test]
    fn every_page_size_is_taller_than_it_is_wide() {
        for page in [PageSize::A4, PageSize::Letter, PageSize::Legal] {
            let (w, h) = page.points();
            assert!(h > w, "{} is portrait", page.label());
        }
    }
}
