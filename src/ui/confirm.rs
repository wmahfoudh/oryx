//! The unsaved-changes confirm: the one modal, guarding quit, reload,
//! open and the new file or note while edits are unsaved. The focus
//! starts on Save; the arrows move it, and Enter or Space runs the
//! focused answer, so a plain Enter saves and proceeds. S saves, D
//! discards and Escape keeps editing wherever the focus is; a click
//! runs an answer and a click outside keeps editing. Every other key is
//! spent, since a modal owns the keyboard.

use winit::keyboard::{Key, NamedKey};

use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{Rgba, Theme};
use crate::ui::overlay::{self, accent_fill, dim, hover_fill, inside};

/// What the guarded action was, carried by the modal until an answer
/// decides. An open remembers whether it reroots the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Quit,
    Reload,
    /// A reload after the document's remote images are dropped from
    /// the fetch cache.
    Refetch,
    Open(std::path::PathBuf, bool),
    New,
    /// A fresh untitled note over the open file's unsaved edits.
    Note,
}

/// The user's decision on the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Save the edits, then run the pending action.
    Save,
    /// Drop the edits, then run the pending action.
    Discard,
    /// Keep editing; the pending action dies.
    Cancel,
    /// The modal holds; the key is spent.
    Hold,
}

/// Resolves one direct key against the modal, whatever the focus:
/// S saves, D discards, Escape keeps editing.
pub fn decide(key: &Key) -> Decision {
    match key {
        Key::Named(NamedKey::Escape) => Decision::Cancel,
        Key::Character(c) if c.eq_ignore_ascii_case("s") => Decision::Save,
        Key::Character(c) if c.eq_ignore_ascii_case("d") => Decision::Discard,
        _ => Decision::Hold,
    }
}

const PANEL_W: f32 = 420.0;
const PAD: f32 = 20.0;
const RADIUS: f32 = 10.0;
const TITLE_SIZE: f32 = 16.0;
const NAME_SIZE: f32 = 13.0;
const LABEL_SIZE: f32 = 15.0;
const KEY_SIZE: f32 = 12.0;
/// The header: the mark and the title, then the file's name.
const HEADER_H: f32 = 56.0;
const ROW_H: f32 = 36.0;
const ROW_GAP: f32 = 4.0;
/// The keycap naming a row's key, a fixed width so the labels align.
const KEY_W: f32 = 52.0;
const KEY_H: f32 = 24.0;
/// How far a row's fill reaches left of the keycap.
const ROW_INSET: f32 = 6.0;

type Rect = (f32, f32, f32, f32);

/// The three answers in row order: the key's name and its decision.
const ANSWERS: [(&str, Decision); 3] = [
    ("S", Decision::Save),
    ("D", Decision::Discard),
    ("Esc", Decision::Cancel),
];

/// The modal's state: what it guards, the file it names, the focused
/// answer, the answer under the mouse, and the rectangles of the last
/// draw for the mouse.
pub struct Confirm {
    pending: Pending,
    name: String,
    focus: usize,
    hover: Option<usize>,
    panel: Rect,
    rows: [Rect; 3],
    keys: [Rect; 3],
}

impl Confirm {
    /// A fresh modal over `pending`, naming the file the edits belong
    /// to; the focus starts on Save.
    pub fn new(pending: Pending, name: String) -> Confirm {
        Confirm {
            pending,
            name,
            focus: 0,
            hover: None,
            panel: (0.0, 0.0, 0.0, 0.0),
            rows: [(0.0, 0.0, 0.0, 0.0); 3],
            keys: [(0.0, 0.0, 0.0, 0.0); 3],
        }
    }

    /// The guarded action, once an answer lets it run.
    pub fn into_pending(self) -> Pending {
        self.pending
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// The three row rectangles of the last draw, in row order.
    pub fn rows(&self) -> [Rect; 3] {
        self.rows
    }

    /// The panel rectangle of the last draw.
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// The three keycap rectangles of the last draw, in row order.
    pub fn keys(&self) -> [Rect; 3] {
        self.keys
    }

    /// The three labels: the first names what follows the save.
    pub fn labels(&self) -> [&'static str; 3] {
        let save = match self.pending {
            Pending::Quit => "Save and quit",
            Pending::Reload | Pending::Refetch => "Save and reload",
            Pending::Open(..) => "Save and open",
            Pending::New => "Save and start a new file",
            Pending::Note => "Save and start a note",
        };
        [save, "Discard changes", "Keep editing"]
    }

    /// One key: the arrows move the focus, Enter and Space run the
    /// focused answer, and the direct keys answer whatever the focus.
    pub fn key(&mut self, key: &Key) -> Decision {
        match key {
            Key::Named(NamedKey::ArrowDown) => {
                self.focus = (self.focus + 1).min(ANSWERS.len() - 1);
                Decision::Hold
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.focus = self.focus.saturating_sub(1);
                Decision::Hold
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => ANSWERS[self.focus].1,
            _ => decide(key),
        }
    }

    /// Records the answer under the mouse at window coordinates;
    /// reports whether it changed, so the caller redraws only then.
    pub fn hover_at(&mut self, x: f32, y: f32) -> bool {
        let hover = self.rows.iter().position(|r| inside(*r, x, y));
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// A press at window coordinates: an answer's row runs it, the
    /// rest of the panel holds, outside the panel keeps editing.
    pub fn click(&mut self, x: f32, y: f32) -> Decision {
        if let Some(index) = self.rows.iter().position(|r| inside(*r, x, y)) {
            return ANSWERS[index].1;
        }
        if inside(self.panel, x, y) {
            Decision::Hold
        } else {
            Decision::Cancel
        }
    }

    /// The modal over a dimmed page: the header, then the three
    /// answers, the focused one in the accent look, the one under the
    /// mouse lifted.
    pub fn draw(&mut self, painter: &mut Painter, theme: &Theme, width: f32, height: f32) {
        let ui = &theme.ui;
        let accent = ui.sidebar_dir;
        let w = PANEL_W.min(width - 40.0);
        let h = PAD + HEADER_H + 3.0 * ROW_H + 2.0 * ROW_GAP + PAD;
        let x = ((width - w) / 2.0).floor();
        let y = ((height - h) / 2.5).floor();
        self.panel = (x, y, w, h);
        painter.fill(0.0, 0.0, width, height, 0.0, overlay::SCRIM);
        overlay::panel_shadow(painter, x, y, w, h, RADIUS);
        painter.fill(x, y, w, h, RADIUS, ui.overlay_bg);
        painter.stroke(x, y, w, h, RADIUS, 1.0, theme.blocks.table_border);
        draw_mark(painter, x + PAD, y + PAD + 3.0, accent);
        painter.text(
            x + PAD + 20.0,
            y + PAD,
            "Unsaved changes",
            BODY_FAMILY,
            TITLE_SIZE,
            700,
            ui.overlay_fg,
        );
        if !self.name.is_empty() {
            painter.text(
                x + PAD + 20.0,
                y + PAD + 25.0,
                &self.name,
                CODE_FAMILY,
                NAME_SIZE,
                400,
                dim(ui.overlay_fg),
            );
        }
        let labels = self.labels();
        let row_x = x + PAD - ROW_INSET;
        let row_w = w - 2.0 * PAD + 2.0 * ROW_INSET;
        for (index, (key, _)) in ANSWERS.iter().enumerate() {
            let ry = y + PAD + HEADER_H + index as f32 * (ROW_H + ROW_GAP);
            self.rows[index] = (row_x, ry, row_w, ROW_H);
            let focused = index == self.focus;
            if focused {
                painter.fill(row_x, ry, row_w, ROW_H, 6.0, accent_fill(accent));
            } else if self.hover == Some(index) {
                painter.fill(row_x, ry, row_w, ROW_H, 6.0, hover_fill(ui.overlay_fg));
            }
            let (kx, ky) = (x + PAD + 4.0, ry + (ROW_H - KEY_H) / 2.0);
            self.keys[index] = (kx, ky, KEY_W, KEY_H);
            let color = if focused { accent } else { ui.overlay_fg };
            if focused {
                painter.stroke(
                    kx + 0.5,
                    ky + 0.5,
                    KEY_W - 1.0,
                    KEY_H - 1.0,
                    5.0,
                    1.0,
                    accent,
                );
            } else {
                painter.fill(kx, ky, KEY_W, KEY_H, 5.0, ui.overlay_highlight);
            }
            let key_w = painter.measure(key, BODY_FAMILY, KEY_SIZE, 600);
            let key_y = centered_top(painter, ky, KEY_H, KEY_SIZE, 600);
            painter.text(
                kx + (KEY_W - key_w) / 2.0,
                key_y,
                key,
                BODY_FAMILY,
                KEY_SIZE,
                600,
                color,
            );
            let label_y = centered_top(painter, ry, ROW_H, LABEL_SIZE, 400);
            painter.text(
                kx + KEY_W + 14.0,
                label_y,
                labels[index],
                BODY_FAMILY,
                LABEL_SIZE,
                400,
                color,
            );
        }
    }
}

/// The top a line of body text takes so its capitals sit centered in a
/// box of `h` starting at `y`, from the measured ink of a capital.
fn centered_top(painter: &mut Painter, y: f32, h: f32, size: f32, weight: u16) -> f32 {
    let (top, bottom) = painter.cap_bounds(BODY_FAMILY, size, weight);
    y + (h - (bottom - top)) / 2.0 - top
}

/// The document mark in the header: a page outline with two lines, the
/// sidebar's markdown mark in the accent.
fn draw_mark(painter: &mut Painter, x: f32, y: f32, color: Rgba) {
    painter.stroke(x + 0.5, y + 0.5, 9.0, 11.0, 1.5, 1.2, color);
    painter.fill(x + 2.5, y + 4.0, 5.0, 1.2, 0.6, color);
    painter.fill(x + 2.5, y + 7.0, 5.0, 1.2, 0.6, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::painter::Painter;
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use tiny_skia::Pixmap;

    fn confirm() -> Confirm {
        Confirm::new(Pending::Quit, "README.md".to_string())
    }

    /// Draws the dialog on a 700 by 500 canvas and returns the pixels.
    fn drawn(confirm: &mut Confirm, fonts: &mut FontStore, theme: &Theme) -> Pixmap {
        let mut pixmap = Pixmap::new(700, 500).unwrap();
        let mut painter = Painter::new(&mut pixmap, fonts, None, 1.0);
        confirm.draw(&mut painter, theme, 700.0, 500.0);
        pixmap
    }

    fn center(rect: (f32, f32, f32, f32)) -> (f32, f32) {
        (rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0)
    }

    #[test]
    fn the_three_answers_resolve() {
        let mut c = confirm();
        assert_eq!(c.key(&Key::Named(NamedKey::Enter)), Decision::Save);
        assert_eq!(decide(&Key::Character("s".into())), Decision::Save);
        assert_eq!(decide(&Key::Character("S".into())), Decision::Save);
        assert_eq!(decide(&Key::Character("d".into())), Decision::Discard);
        assert_eq!(
            decide(&Key::Character("D".into())),
            Decision::Discard,
            "shift makes no difference"
        );
        assert_eq!(decide(&Key::Named(NamedKey::Escape)), Decision::Cancel);
    }

    #[test]
    fn every_other_key_is_spent_by_the_modal() {
        assert_eq!(decide(&Key::Character("x".into())), Decision::Hold);
        assert_eq!(decide(&Key::Named(NamedKey::Space)), Decision::Hold);
        assert_eq!(decide(&Key::Named(NamedKey::F5)), Decision::Hold);
        let mut c = confirm();
        assert_eq!(c.key(&Key::Character("x".into())), Decision::Hold);
        assert_eq!(c.key(&Key::Named(NamedKey::F5)), Decision::Hold);
    }

    #[test]
    fn the_focus_moves_with_the_arrows_and_space_runs_it() {
        let mut c = confirm();
        assert_eq!(c.focus(), 0, "Save has the focus at first");
        assert_eq!(c.key(&Key::Named(NamedKey::ArrowDown)), Decision::Hold);
        assert_eq!(c.focus(), 1);
        assert_eq!(c.key(&Key::Named(NamedKey::Space)), Decision::Discard);
        c.key(&Key::Named(NamedKey::ArrowDown));
        c.key(&Key::Named(NamedKey::ArrowDown));
        assert_eq!(c.focus(), 2, "the focus stops at the last row");
        assert_eq!(c.key(&Key::Named(NamedKey::Space)), Decision::Cancel);
        for _ in 0..3 {
            c.key(&Key::Named(NamedKey::ArrowUp));
        }
        assert_eq!(c.focus(), 0, "and at the first");
        assert_eq!(c.key(&Key::Named(NamedKey::Space)), Decision::Save);
    }

    /// Enter follows the focus like Space, the way every dialog runs
    /// its highlighted button; D and Escape answer wherever it is.
    #[test]
    fn enter_runs_the_focused_row_and_the_direct_keys_ignore_it() {
        let mut c = confirm();
        c.key(&Key::Named(NamedKey::ArrowDown));
        assert_eq!(c.key(&Key::Named(NamedKey::Enter)), Decision::Discard);
        c.key(&Key::Named(NamedKey::ArrowDown));
        assert_eq!(c.key(&Key::Named(NamedKey::Enter)), Decision::Cancel);
        assert_eq!(c.key(&Key::Character("s".into())), Decision::Save);
        assert_eq!(c.key(&Key::Character("d".into())), Decision::Discard);
        assert_eq!(c.key(&Key::Named(NamedKey::Escape)), Decision::Cancel);
        assert_eq!(
            decide(&Key::Named(NamedKey::Enter)),
            Decision::Hold,
            "Enter is no direct key: it belongs to the focus"
        );
    }

    #[test]
    fn the_first_row_names_what_follows_the_save() {
        for (pending, label) in [
            (Pending::Quit, "Save and quit"),
            (Pending::Reload, "Save and reload"),
            (Pending::Refetch, "Save and reload"),
            (
                Pending::Open(std::path::PathBuf::from("a.md"), false),
                "Save and open",
            ),
            (Pending::New, "Save and start a new file"),
            (Pending::Note, "Save and start a note"),
        ] {
            let c = Confirm::new(pending, String::new());
            assert_eq!(c.labels(), [label, "Discard changes", "Keep editing"]);
        }
    }

    #[test]
    fn a_click_runs_its_row_and_a_click_outside_keeps_editing() {
        let mut c = confirm();
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        drawn(&mut c, &mut fonts, &theme);
        let rows = c.rows();
        let (x, y) = center(rows[1]);
        assert_eq!(c.click(x, y), Decision::Discard);
        let (x, y) = center(rows[2]);
        assert_eq!(c.click(x, y), Decision::Cancel);
        let (x, y) = center(rows[0]);
        assert_eq!(c.click(x, y), Decision::Save);
        assert_eq!(c.click(2.0, 2.0), Decision::Cancel, "outside keeps editing");
        let (px, py, _, _) = c.panel();
        assert_eq!(
            c.click(px + 10.0, py + 10.0),
            Decision::Hold,
            "the header answers nothing"
        );
    }

    #[test]
    fn hover_follows_the_row_under_the_mouse() {
        let mut c = confirm();
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        drawn(&mut c, &mut fonts, &theme);
        let rows = c.rows();
        let (x, y) = center(rows[1]);
        assert!(c.hover_at(x, y));
        assert_eq!(c.hover(), Some(1));
        assert!(!c.hover_at(x + 3.0, y), "the same row changes nothing");
        assert!(c.hover_at(2.0, 2.0));
        assert_eq!(c.hover(), None);
    }

    /// A keycap's letters sit centered in its box: the ink's vertical
    /// middle within a pixel of the box's middle.
    #[test]
    fn the_keycap_text_is_centered_in_its_box() {
        let mut c = confirm();
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let pixmap = drawn(&mut c, &mut fonts, &theme);
        let (kx, ky, kw, kh) = c.keys()[1];
        let ground = pixmap.pixel((kx + 6.0) as u32, (ky + 1.0) as u32).unwrap();
        let mut ink = (f32::MAX, f32::MIN);
        for y in (ky as u32 + 1)..((ky + kh) as u32 - 1) {
            for x in (kx as u32 + 6)..((kx + kw) as u32 - 6) {
                if pixmap.pixel(x, y).unwrap() != ground {
                    ink.0 = ink.0.min(y as f32);
                    ink.1 = ink.1.max(y as f32 + 1.0);
                }
            }
        }
        assert!(ink.0 < ink.1, "the key's letters are drawn");
        let middle = (ink.0 + ink.1) / 2.0;
        assert!(
            (middle - (ky + kh / 2.0)).abs() <= 1.0,
            "ink {ink:?} against the box {ky}..{}",
            ky + kh
        );
    }

    #[test]
    fn the_focused_row_wears_the_accent_and_the_page_is_dimmed() {
        let mut c = confirm();
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let pixmap = drawn(&mut c, &mut fonts, &theme);
        let rows = c.rows();
        let bg = theme.ui.overlay_bg;
        let at = |x: f32, y: f32| {
            let p = pixmap.pixel(x as u32, y as u32).unwrap();
            (p.red(), p.green(), p.blue(), p.alpha())
        };
        let (x, y) = (rows[0].0 + 3.0, rows[0].1 + rows[0].3 / 2.0);
        assert_ne!(
            at(x, y),
            (bg.r, bg.g, bg.b, 255),
            "the focused row is filled"
        );
        let (x, y) = (rows[1].0 + 3.0, rows[1].1 + rows[1].3 / 2.0);
        assert_eq!(
            at(x, y),
            (bg.r, bg.g, bg.b, 255),
            "an unfocused row shows the panel"
        );
        let (_, _, _, a) = at(2.0, 2.0);
        assert!(
            a > 0 && a < 255,
            "the page corner is dimmed, not covered: {a}"
        );
    }
}
