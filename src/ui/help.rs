//! Shortcuts help overlay: a read-only table of every shortcut, rendered
//! from `keymap::SHORTCUTS` so it always matches the dispatch.

use winit::keyboard::{Key, NamedKey};

use crate::input::keymap;
use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{Rgba, Theme};
use crate::ui::overlay::{Overlay, OverlayResult};

const ROW_H: f32 = 28.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 44.0;
const FOOTER_H: f32 = 30.0;
const COLUMN_GAP: f32 = 28.0;
const RADIUS: f32 = 8.0;

#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    center: (f32, f32),
}

#[derive(Default)]
pub struct Help {
    moving: bool,
    grab: (f32, f32),
    offset: (f32, f32),
    geometry: Geometry,
}

impl Help {
    pub fn new() -> Help {
        Help::default()
    }
}

impl Overlay for Help {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let rows = keymap::SHORTCUTS;
        let keys_w = rows
            .iter()
            .map(|s| painter.measure(&keymap::display(s.keys), CODE_FAMILY, 13.0, 600))
            .fold(0.0, f32::max);
        let action_w = rows
            .iter()
            .map(|s| painter.measure(s.action, BODY_FAMILY, 14.0, 400))
            .fold(0.0, f32::max);
        let panel_w = (PAD + keys_w + COLUMN_GAP + action_w + PAD).min(w - 40.0);
        let panel_h = HEADER_H + PAD + rows.len() as f32 * ROW_H + PAD + FOOTER_H;
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let px = (center.0 + self.offset.0).clamp(60.0 - panel_w, w - 60.0);
        let py = (center.1 + self.offset.1).clamp(-8.0, h - HEADER_H);
        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            center,
        };

        for (grow, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
            painter.fill(
                px - grow,
                py - grow + 2.0,
                panel_w + 2.0 * grow,
                panel_h + 2.0 * grow,
                RADIUS + grow,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: alpha,
                },
            );
        }
        painter.fill(px, py, panel_w, panel_h, RADIUS, ui.overlay_bg);

        let title = "Shortcuts";
        let title_w = painter.measure(title, BODY_FAMILY, 17.0, 700);
        painter.text(
            px + (panel_w - title_w) / 2.0,
            py + 13.0,
            title,
            BODY_FAMILY,
            17.0,
            700,
            ui.overlay_fg,
        );
        let rows_top = py + HEADER_H + PAD;
        let action_x = px + PAD + keys_w + COLUMN_GAP;
        for (index, row) in rows.iter().enumerate() {
            let ry = rows_top + index as f32 * ROW_H;
            painter.text(
                px + PAD,
                ry + 4.0,
                &keymap::display(row.keys),
                CODE_FAMILY,
                13.0,
                600,
                ui.overlay_fg,
            );
            painter.text(
                action_x,
                ry + 4.0,
                row.action,
                BODY_FAMILY,
                14.0,
                400,
                ui.overlay_fg,
            );
        }
        painter.text(
            px + PAD,
            py + panel_h - FOOTER_H + 6.0,
            "esc: close",
            BODY_FAMILY,
            12.0,
            400,
            theme.blocks.frontmatter_fg,
        );
        painter.stroke(
            px,
            py,
            panel_w,
            panel_h,
            RADIUS,
            1.0,
            theme.blocks.table_border,
        );
    }

    fn key(&mut self, key: &Key, _ctrl: bool) -> OverlayResult {
        match key {
            Key::Named(NamedKey::Escape) => OverlayResult::Close,
            _ => OverlayResult::Open,
        }
    }

    fn click(&mut self, x: f32, y: f32) -> OverlayResult {
        let (px, py, pw, ph) = self.geometry.panel;
        if x < px || x > px + pw || y < py || y > py + ph {
            return OverlayResult::Close;
        }
        if y < py + HEADER_H {
            self.moving = true;
            self.grab = (x - px, y - py);
        }
        OverlayResult::Open
    }

    fn drag(&mut self, x: f32, y: f32) -> OverlayResult {
        if self.moving {
            self.offset = (
                x - self.grab.0 - self.geometry.center.0,
                y - self.grab.1 - self.geometry.center.1,
            );
        }
        OverlayResult::Open
    }

    fn release(&mut self) {
        self.moving = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes() {
        let mut help = Help::new();
        let result = help.key(&Key::Named(NamedKey::Escape), false);
        assert!(matches!(result, OverlayResult::Close));
    }

    #[test]
    fn other_keys_stay_open() {
        let mut help = Help::new();
        for key in [
            Key::Named(NamedKey::Enter),
            Key::Named(NamedKey::ArrowDown),
            Key::Character("a".into()),
        ] {
            assert!(matches!(help.key(&key, false), OverlayResult::Open));
        }
    }

    #[test]
    fn click_outside_closes() {
        let mut help = Help::new();
        let result = help.click(-10.0, -10.0);
        assert!(matches!(result, OverlayResult::Close));
    }
}
