//! PDF export: page geometry, the resumable pass, and the settings that
//! drive them. Pagination and emission live in the two child modules.

pub mod paginate;
pub mod pdf;

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::doc::images::MediaCache;
use crate::doc::model::Document;
use crate::export::paginate::{paginate, Page};
use crate::export::pdf::Builder;
use crate::layout::{layout_begin, layout_more, metrics, LayoutDoc, LayoutPass, ViewConfig};
use crate::platform::config::Config;
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

/// The sheet an export writes onto, portrait only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageSize {
    A4,
    Letter,
    Legal,
}

impl PageSize {
    /// Sheet size in PDF points. One point is one layout unit, so the
    /// engine lays a page out the way it lays a window out.
    pub fn points(self) -> (f32, f32) {
        match self {
            PageSize::A4 => (595.28, 841.89),
            PageSize::Letter => (612.0, 792.0),
            PageSize::Legal => (612.0, 1008.0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::Letter => "Letter",
            PageSize::Legal => "Legal",
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
    pub page_numbers: bool,
}

impl ExportSettings {
    /// The values an export starts life with: whatever the reader is
    /// looking at, on A4, numbered.
    pub fn seeded_from(config: &Config) -> ExportSettings {
        ExportSettings {
            theme: config.theme.clone(),
            body_family: config.body_family.clone(),
            code_family: config.code_family.clone(),
            body_size: config.body_size,
            code_size: config.code_size,
            page: PageSize::A4,
            page_numbers: true,
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
    pub fn new(page: PageSize, body_size: f32) -> PageGeometry {
        let (width, height) = page.points();
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

/// How far a running export has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Highlight,
    Layout,
    Paginate,
    Emit,
    Done,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Highlight => "Colouring code",
            Phase::Layout => "Laying out",
            Phase::Paginate => "Making pages",
            Phase::Emit => "Writing pages",
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
    title: String,
    phase: Phase,
    layout: LayoutDoc,
    pass: Option<LayoutPass>,
    pages: Vec<Page>,
    next: usize,
    builder: Builder,
}

impl ExportPass {
    pub fn new(settings: &ExportSettings, theme: Theme, target: PathBuf) -> ExportPass {
        let geometry = PageGeometry::new(settings.page, settings.body_size);
        let title = target
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        ExportPass {
            settings: settings.clone(),
            theme,
            cfg: ViewConfig {
                body_family: settings.body_family.clone(),
                code_family: settings.code_family.clone(),
                body_size: settings.body_size,
                code_size: settings.code_size,
                zoom: 1.0,
            },
            geometry,
            target,
            title,
            phase: Phase::Highlight,
            layout: LayoutDoc::default(),
            pass: None,
            pages: Vec::new(),
            next: 0,
            builder: Builder::new(),
        }
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Advances until the deadline and reports where it stopped. The
    /// highlight phase idles while the worker still has blocks to colour,
    /// since a PDF cannot wash in after it is written.
    pub fn step(
        &mut self,
        deadline: Instant,
        doc: &Document,
        fonts: &mut FontStore,
        media: &mut MediaCache,
        highlighting: bool,
        pool: Option<&std::sync::Arc<crate::layout::ShapePool>>,
    ) -> Progress {
        if self.phase == Phase::Highlight {
            if highlighting {
                return self.progress();
            }
            self.phase = Phase::Layout;
        }
        if self.phase == Phase::Layout {
            let pass = self.pass.get_or_insert_with(|| {
                let (out, mut pass) = layout_begin(doc, &self.cfg, self.geometry.width);
                if let Some(pool) = pool {
                    pass.attach_pool(std::sync::Arc::clone(pool));
                }
                self.layout = out;
                pass
            });
            let complete = layout_more(
                doc,
                &self.theme,
                fonts,
                media,
                &self.cfg,
                &mut self.layout,
                pass,
                Some(deadline),
            );
            if !complete {
                return self.progress();
            }
            self.phase = Phase::Paginate;
        }
        if self.phase == Phase::Paginate {
            self.pages = paginate(doc, &self.layout, &self.geometry);
            self.phase = Phase::Emit;
        }
        if self.phase == Phase::Emit {
            while self.next < self.pages.len() {
                let job = crate::export::pdf::Job {
                    doc,
                    layout: &self.layout,
                    theme: &self.theme,
                    geometry: &self.geometry,
                    settings: &self.settings,
                    title: &self.title,
                };
                self.builder
                    .add_page(&job, &self.pages[self.next], fonts, media);
                self.next += 1;
                if Instant::now() >= deadline {
                    return self.progress();
                }
            }
            self.phase = Phase::Done;
        }
        self.progress()
    }

    pub fn progress(&self) -> Progress {
        Progress {
            phase: self.phase,
            done: self.next,
            total: self.pages.len(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Writes the assembled bytes through a sibling temporary file, so a
    /// full disk or a refused permission leaves the target as it was.
    /// Reports the page count.
    pub fn finish(self, doc: &Document, fonts: &FontStore) -> std::io::Result<usize> {
        let pages = self.pages.len();
        let job = crate::export::pdf::Job {
            doc,
            layout: &self.layout,
            theme: &self.theme,
            geometry: &self.geometry,
            settings: &self.settings,
            title: &self.title,
        };
        let bytes = self
            .builder
            .finish(&job, fonts)
            .map_err(std::io::Error::other)?;
        let mut partial = self.target.clone().into_os_string();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, &self.target)?;
        Ok(pages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_uses_the_same_margin_rules_as_the_screen() {
        let g = PageGeometry::new(PageSize::A4, 11.0);
        assert_eq!((g.width, g.height), (595.28, 841.89));
        assert!((g.margin_x - 0.08 * 595.28).abs() < 0.01, "8 percent sides");
        assert_eq!(g.margin_y, 22.0, "2em of the body size");
        assert!((g.content_height() - (841.89 - 44.0)).abs() < 0.01);
    }

    #[test]
    fn every_page_size_is_taller_than_it_is_wide() {
        for page in [PageSize::A4, PageSize::Letter, PageSize::Legal] {
            let (w, h) = page.points();
            assert!(h > w, "{} is portrait", page.label());
        }
    }
}
