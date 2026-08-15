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

/// The header band every panel drags by.
pub const HEADER_H: f32 = 44.0;

/// The drag state every panel shares: the offset a header drag
/// accumulates, applied to the panel's centred seat at draw.
#[derive(Default)]
pub struct PanelDrag {
    offset: (f32, f32),
    grab: (f32, f32),
    moving: bool,
}

impl PanelDrag {
    /// Arms the drag from a press at `(x, y)` on a panel at `(px, py)`.
    pub fn press(&mut self, x: f32, y: f32, px: f32, py: f32) {
        self.moving = true;
        self.grab = (x - px, y - py);
    }

    /// Follows the cursor while armed; the offset is read back at draw
    /// through `place`.
    pub fn to(&mut self, x: f32, y: f32, center: (f32, f32)) {
        if self.moving {
            self.offset = (x - self.grab.0 - center.0, y - self.grab.1 - center.1);
        }
    }

    pub fn release(&mut self) {
        self.moving = false;
    }

    /// The accumulated offset, read by tests pinning drag behavior.
    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    /// The panel's top-left from its centred seat and the accumulated
    /// offset, clamped so a dragged panel always keeps a grabbable
    /// edge on screen.
    pub fn place(&self, center: (f32, f32), panel_w: f32, w: f32, h: f32) -> (f32, f32) {
        (
            (center.0 + self.offset.0).clamp(60.0 - panel_w, w - 60.0),
            (center.1 + self.offset.1).clamp(-8.0, h - HEADER_H),
        )
    }
}

/// Soft shadow lifting a panel off the document: three fills growing
/// outward, drawn before the panel's own fill, following its corner
/// radius. The confirm dialog and the search bar keep their own
/// lighter pair, small chrome deliberately floating lower.
pub fn panel_shadow(painter: &mut Painter, x: f32, y: f32, w: f32, h: f32, radius: f32) {
    for (grow, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
        painter.fill(
            x - grow,
            y - grow + 2.0,
            w + 2.0 * grow,
            h + 2.0 * grow,
            radius + grow,
            crate::style::theme::Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: alpha,
            },
        );
    }
}

/// Keeps row `index` visible in a list: the scroll that moves just
/// enough in the row's direction, or the standing one when the row
/// already shows.
pub fn scroll_into_view(index: usize, row_h: f32, scroll: f32, list_h: f32) -> f32 {
    let top = index as f32 * row_h;
    let list_h = list_h.max(row_h);
    if top < scroll {
        top
    } else if top + row_h > scroll + list_h {
        top + row_h - list_h
    } else {
        scroll
    }
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
