//! Shortcuts help overlay: a read-only table of every shortcut, rendered
//! from `keymap::SHORTCUTS` so it always matches the dispatch.

use winit::keyboard::{Key, NamedKey};

use crate::input::keymap;
use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{Rgba, Theme};
use crate::ui::overlay::{Overlay, OverlayResult};

const ROW_H: f32 = 29.0;
const SECTION_H: f32 = 22.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 42.0;
const FOOTER_H: f32 = 28.0;
const COLUMN_GAP: f32 = 40.0;
const RADIUS: f32 = 8.0;
const KEYS_SIZE: f32 = 15.0;
const ACTION_SIZE: f32 = 14.0;

#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    center: (f32, f32),
    list_h: f32,
}

#[derive(Default)]
pub struct Help {
    moving: bool,
    grab: (f32, f32),
    offset: (f32, f32),
    geometry: Geometry,
    /// Table scroll offset in pixels, 0 whenever the table fits.
    scroll: f32,
}

/// Height of the full shortcut table: every row plus a caption per
/// section.
fn content_height() -> f32 {
    let rows = keymap::SHORTCUTS;
    let mut sections = 0;
    let mut last = "";
    for row in rows {
        if row.section != last {
            sections += 1;
            last = row.section;
        }
    }
    rows.len() as f32 * ROW_H + sections as f32 * SECTION_H
}

impl Help {
    pub fn new() -> Help {
        Help::default()
    }

    fn scroll_by(&mut self, delta: f32) {
        let max = (content_height() - self.geometry.list_h).max(0.0);
        self.scroll = (self.scroll + delta).clamp(0.0, max);
    }

    #[cfg(test)]
    fn panel_height(&self) -> f32 {
        self.geometry.panel.3
    }

    #[cfg(test)]
    fn scrolled(&self) -> f32 {
        self.scroll
    }
}

impl Overlay for Help {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let rows = keymap::SHORTCUTS;
        let keys_w = rows
            .iter()
            .map(|s| painter.measure(&keymap::display(s.keys), CODE_FAMILY, KEYS_SIZE, 400))
            .fold(0.0, f32::max);
        let action_w = rows
            .iter()
            .map(|s| painter.measure(s.action, BODY_FAMILY, ACTION_SIZE, 400))
            .fold(0.0, f32::max);
        let panel_w = (PAD + keys_w + COLUMN_GAP + action_w + PAD).min(w - 40.0);
        // A window shorter than the table clamps the panel and the
        // table scrolls inside it.
        let content_h = content_height();
        let want_h = HEADER_H + PAD + content_h + PAD + FOOTER_H;
        let panel_h = want_h.min(h - 16.0);
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let px = (center.0 + self.offset.0).clamp(60.0 - panel_w, w - 60.0);
        let py = (center.1 + self.offset.1).clamp(-8.0, h - HEADER_H);
        let list_top = py + HEADER_H + PAD;
        let list_h = panel_h - HEADER_H - FOOTER_H - 2.0 * PAD;
        let list_bottom = list_top + list_h;
        self.scroll = self.scroll.clamp(0.0, (content_h - list_h).max(0.0));
        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            center,
            list_h,
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

        let action_x = px + PAD + keys_w + COLUMN_GAP;
        let mut ry = list_top - self.scroll;
        let mut section = "";
        for (index, row) in rows.iter().enumerate() {
            if row.section != section {
                section = row.section;
                if ry + SECTION_H > list_top && ry < list_bottom {
                    painter.text(
                        px + PAD,
                        ry + 6.0,
                        section,
                        BODY_FAMILY,
                        12.0,
                        700,
                        theme.blocks.frontmatter_fg,
                    );
                }
                ry += SECTION_H;
            }
            if ry + ROW_H > list_top && ry < list_bottom {
                painter.text(
                    px + PAD,
                    ry + 5.0,
                    &keymap::display(row.keys),
                    CODE_FAMILY,
                    KEYS_SIZE,
                    400,
                    ui.overlay_fg,
                );
                painter.text(
                    action_x,
                    ry + 5.0,
                    row.action,
                    BODY_FAMILY,
                    ACTION_SIZE,
                    400,
                    ui.overlay_fg,
                );
                // Hairlines separate rows inside a section; the caption
                // is the separation at a boundary.
                let boundary = !rows
                    .get(index + 1)
                    .is_some_and(|next| next.section == section);
                if !boundary {
                    painter.line(
                        px + PAD,
                        ry + ROW_H - 1.0,
                        px + panel_w - PAD,
                        ry + ROW_H - 1.0,
                        1.0,
                        theme.blocks.table_border,
                    );
                }
            }
            ry += ROW_H;
        }
        // Rows partially past either end of the table bleed into the
        // header and footer; both bands repaint over them.
        painter.fill(px, py, panel_w, HEADER_H + PAD, RADIUS, ui.overlay_bg);
        painter.fill(
            px,
            list_bottom,
            panel_w,
            panel_h - (list_bottom - py),
            RADIUS,
            ui.overlay_bg,
        );
        let title = "Shortcuts";
        let title_w = painter.measure(title, BODY_FAMILY, 17.0, 700);
        painter.text(
            px + (panel_w - title_w) / 2.0,
            py + 11.0,
            title,
            BODY_FAMILY,
            17.0,
            700,
            ui.overlay_fg,
        );
        let clipped = content_h > list_h;
        if clipped {
            if let Some((thumb_y, thumb_h)) =
                crate::ui::scrollbar::thumb(content_h, list_h, self.scroll, list_h, 1.0)
            {
                painter.fill(
                    px + panel_w - 6.0,
                    list_top + thumb_y,
                    3.0,
                    thumb_h,
                    1.5,
                    ui.scrollbar,
                );
            }
        }
        let hint = if clipped {
            "\u{2191}/\u{2193}: scroll \u{00B7} esc: close"
        } else {
            "esc: close"
        };
        painter.text(
            px + PAD,
            py + panel_h - FOOTER_H + 7.0,
            hint,
            BODY_FAMILY,
            12.0,
            400,
            theme.blocks.frontmatter_fg,
        );
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        let version_w = painter.measure(version, BODY_FAMILY, 12.0, 400);
        painter.text(
            px + panel_w - PAD - version_w,
            py + panel_h - FOOTER_H + 7.0,
            version,
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

    fn key(&mut self, key: &Key, _ctrl: bool, _shift: bool) -> OverlayResult {
        let page = self.geometry.list_h.max(ROW_H);
        match key {
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            Key::Named(NamedKey::ArrowDown) => self.scroll_by(ROW_H),
            Key::Named(NamedKey::ArrowUp) => self.scroll_by(-ROW_H),
            Key::Named(NamedKey::PageDown) => self.scroll_by(page),
            Key::Named(NamedKey::PageUp) => self.scroll_by(-page),
            Key::Named(NamedKey::Home) => self.scroll = 0.0,
            Key::Named(NamedKey::End) => self.scroll_by(f32::MAX),
            _ => {}
        }
        OverlayResult::Open
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

    fn scroll(&mut self, lines: f32) -> OverlayResult {
        self.scroll_by(lines * ROW_H);
        OverlayResult::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::fonts::FontStore;
    use tiny_skia::Pixmap;

    /// One fresh frame at the given window height; the raw pixels come
    /// back for comparison.
    fn frame(help: &mut Help, height: u32) -> Vec<u8> {
        let mut pixmap = Pixmap::new(1100, height).unwrap();
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let mut painter = Painter::new(&mut pixmap, &mut fonts, None, 1.0);
        help.draw(&mut painter, &theme);
        pixmap.data().to_vec()
    }

    #[test]
    fn a_short_window_clamps_the_panel_instead_of_overflowing() {
        let mut help = Help::new();
        frame(&mut help, 400);
        assert!(
            help.panel_height() <= 400.0,
            "panel {} taller than the window",
            help.panel_height()
        );
    }

    #[test]
    fn wheel_scrolls_the_table_and_clamps_at_the_top() {
        let mut help = Help::new();
        let top = frame(&mut help, 400);
        help.scroll(3.0);
        let scrolled = frame(&mut help, 400);
        assert_ne!(top, scrolled, "scrolling repaints different rows");
        help.scroll(-1000.0);
        let back = frame(&mut help, 400);
        assert_eq!(top, back, "the table clamps back at its first row");
    }

    #[test]
    fn arrows_scroll_a_row_and_pages_scroll_a_screen() {
        let mut help = Help::new();
        frame(&mut help, 400);
        help.key(&Key::Named(NamedKey::ArrowDown), false, false);
        assert_eq!(help.scrolled(), ROW_H);
        help.key(&Key::Named(NamedKey::PageDown), false, false);
        assert!(help.scrolled() > ROW_H);
        help.key(&Key::Named(NamedKey::Home), false, false);
        assert_eq!(help.scrolled(), 0.0);
    }

    #[test]
    fn a_tall_window_shows_everything_without_scrolling() {
        let mut help = Help::new();
        frame(&mut help, 1400);
        help.scroll(5.0);
        frame(&mut help, 1400);
        assert_eq!(help.scrolled(), 0.0, "nothing to scroll when it fits");
    }

    #[test]
    fn escape_closes() {
        let mut help = Help::new();
        let result = help.key(&Key::Named(NamedKey::Escape), false, false);
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
            assert!(matches!(help.key(&key, false, false), OverlayResult::Open));
        }
    }

    #[test]
    fn click_outside_closes() {
        let mut help = Help::new();
        let result = help.click(-10.0, -10.0);
        assert!(matches!(result, OverlayResult::Close));
    }
}
