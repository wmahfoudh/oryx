//! Overlay framework: one modal surface drawn over the document. The app
//! holds at most one active overlay, routes keys, clicks, and wheel to it,
//! and applies the actions it returns.

use std::path::PathBuf;

use winit::keyboard::Key;

use crate::paint::painter::Painter;
use crate::style::theme::Theme;

/// App-level effect requested by an overlay.
pub enum Action {
    /// Load the theme file, apply it, persist the choice.
    SetTheme(PathBuf),
    /// A theme file was renamed; the persisted choice follows when it
    /// pointed there.
    RenamedTheme { from: String, to: String },
    /// Open the theme editor on a file.
    EditTheme(PathBuf),
    /// Restyle with an in-memory theme, without persisting anything.
    PreviewTheme(Box<Theme>),
    /// Persist the export settings, then export with them.
    Export(Box<crate::export::ExportSettings>),
    /// Apply and persist font families, sizes, and the interface scale.
    SetView {
        body_family: String,
        code_family: String,
        body_size: f32,
        code_size: f32,
        ui_scale: f32,
    },
}

/// What the app should do after an overlay handled an event.
pub enum OverlayResult {
    /// Stay open; the overlay may have changed and wants a redraw.
    Open,
    /// Dismiss the overlay.
    Close,
    /// Dismiss is not implied: the overlay stays open while the app
    /// performs the action.
    Apply(Action),
    /// Perform the action, then dismiss: the confirm that also closes.
    ApplyAndClose(Action),
}

/// Whether a point falls inside a rect given as origin, width and height.
pub fn inside(rect: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rect.0 && x <= rect.0 + rect.2 && y >= rect.1 && y <= rect.1 + rect.3
}

pub trait Overlay {
    /// Paints the overlay; geometry may be cached for hit testing.
    fn draw(&mut self, painter: &mut Painter, theme: &Theme);
    fn key(&mut self, key: &Key, ctrl: bool, shift: bool) -> OverlayResult;
    /// Left click at window coordinates.
    fn click(&mut self, x: f32, y: f32) -> OverlayResult;
    /// Cursor movement while the left button is held.
    fn drag(&mut self, _x: f32, _y: f32) -> OverlayResult {
        OverlayResult::Open
    }
    /// Left button released.
    fn release(&mut self) {}
    /// Mouse wheel, positive scrolling down.
    fn scroll(&mut self, _lines: f32) -> OverlayResult {
        OverlayResult::Open
    }
}
