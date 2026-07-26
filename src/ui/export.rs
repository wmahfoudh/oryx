//! The export overlay: what the reader watches while a PDF is written,
//! and what it says when the writing stops.

use winit::keyboard::Key;

use crate::export::Progress;
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::overlay::{Overlay, OverlayResult};

const MIN_W: f32 = 380.0;
const PANEL_H: f32 = 96.0;
const PAD: f32 = 24.0;
const RADIUS: f32 = 10.0;
const TITLE_SIZE: f32 = 17.0;
const LINE_SIZE: f32 = 15.0;

/// Trims a line to a width, keeping its end. A path's file name and an
/// error's message both live there, so the head is what gives way.
fn elide(painter: &mut Painter, text: &str, size: f32, weight: u16, max: f32) -> String {
    if painter.measure(text, BODY_FAMILY, size, weight) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for cut in 1..chars.len() {
        let candidate: String = std::iter::once('\u{2026}')
            .chain(chars[cut..].iter().copied())
            .collect();
        if painter.measure(&candidate, BODY_FAMILY, size, weight) <= max {
            return candidate;
        }
    }
    String::from("\u{2026}")
}

/// Either a running export or the line it finished on.
pub enum ExportState {
    Running(Progress),
    Result(String),
}

pub struct ExportProgress {
    state: ExportState,
}

impl ExportProgress {
    pub fn new(progress: Progress) -> ExportProgress {
        ExportProgress {
            state: ExportState::Running(progress),
        }
    }

    /// The line the export ended on. The overlay stays up until a key
    /// dismisses it, because a failure has nowhere else to be reported
    /// and a fast export would otherwise finish with no sign that it ran.
    pub fn settled(line: String) -> ExportProgress {
        ExportProgress {
            state: ExportState::Result(line),
        }
    }

    fn lines(&self) -> (String, String) {
        match &self.state {
            ExportState::Running(progress) => {
                let detail = if progress.total > 0 {
                    format!(
                        "page {} of {}",
                        progress.done.min(progress.total),
                        progress.total
                    )
                } else {
                    String::from("Escape cancels")
                };
                (progress.phase.label().to_string(), detail)
            }
            ExportState::Result(line) => (String::from("Export"), line.clone()),
        }
    }
}

impl Overlay for ExportProgress {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let (title, detail) = self.lines();
        // The panel takes the width its lines need, up to what the window
        // can hold; past that the line gives way rather than the panel.
        let widest = painter
            .measure(&title, BODY_FAMILY, TITLE_SIZE, 700)
            .max(painter.measure(&detail, BODY_FAMILY, LINE_SIZE, 400));
        let panel_w = (widest + 2.0 * PAD).clamp(MIN_W, (w - 40.0).max(MIN_W));
        let detail = elide(painter, &detail, LINE_SIZE, 400, panel_w - 2.0 * PAD);
        let px = ((w - panel_w) / 2.0).floor();
        let py = ((h - PANEL_H) / 2.0).floor();
        for (grow, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
            painter.fill(
                px - grow,
                py - grow + 2.0,
                panel_w + 2.0 * grow,
                PANEL_H + 2.0 * grow,
                RADIUS + grow,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: alpha,
                },
            );
        }
        painter.fill(px, py, panel_w, PANEL_H, RADIUS, ui.overlay_bg);

        let title_w = painter.measure(&title, BODY_FAMILY, TITLE_SIZE, 700);
        painter.text(
            px + (panel_w - title_w) / 2.0,
            py + 22.0,
            &title,
            BODY_FAMILY,
            TITLE_SIZE,
            700,
            ui.overlay_fg,
        );
        let detail_w = painter.measure(&detail, BODY_FAMILY, LINE_SIZE, 400);
        painter.text(
            px + (panel_w - detail_w) / 2.0,
            py + 54.0,
            &detail,
            BODY_FAMILY,
            LINE_SIZE,
            400,
            ui.overlay_fg,
        );
    }

    fn key(&mut self, _key: &Key, _ctrl: bool, _shift: bool) -> OverlayResult {
        OverlayResult::Close
    }

    fn click(&mut self, _x: f32, _y: f32) -> OverlayResult {
        OverlayResult::Open
    }
}
