//! Settings overlay: body and code font families and sizes, the on/off
//! rows and the autosave pause, applied live and persisted by the app.
//! Zoom stepping helpers live here too; zoom is session state and never
//! saved.

use winit::keyboard::{Key, NamedKey};

use crate::edit::autosave;
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::Theme;
use crate::ui::overlay::{self, Action, Overlay, OverlayResult, PanelDrag};

const ROW_H: f32 = 40.0;
const LIST_ROW_H: f32 = 28.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 44.0;
const FOOTER_H: f32 = 30.0;

// The list's rows are drawn whole and the panel's own color is painted
// back over what reaches past the list; the bands above and below the
// list must be at least a row tall for the cover to be complete.
const _: () = assert!(HEADER_H + PAD / 2.0 >= LIST_ROW_H && FOOTER_H + PAD / 2.0 >= LIST_ROW_H);
const PANEL_W: f32 = 420.0;
/// Where the values start, as a share of the panel's width: past the
/// longest label in bold, "save on focus loss", with air between.
const VALUE_COLUMN: f32 = 0.46;
const RADIUS: f32 = 8.0;

const ROWS: [&str; 9] = [
    "body font",
    "code font",
    "body size",
    "code size",
    "interface scale",
    "line numbers",
    "word count",
    "save on focus loss",
    "save after a pause",
];

/// The on/off rows' places in `ROWS`.
const LINE_NUMBERS_ROW: usize = 5;
const WORD_COUNT_ROW: usize = 6;
const SAVE_FOCUS_ROW: usize = 7;
/// The pause row steps through `autosave::PAUSES`.
const SAVE_PAUSE_ROW: usize = 8;

/// Font size bounds for both families.
pub const SIZE_MIN: f32 = 8.0;
pub const SIZE_MAX: f32 = 32.0;

/// Zoom bounds and step.
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 3.0;
pub const ZOOM_STEP: f32 = 0.1;

/// Manual interface scale bounds and step, a factor over the display's
/// detected baseline of 1.0.
pub const UI_SCALE_MIN: f32 = 0.5;
pub const UI_SCALE_MAX: f32 = 2.0;
pub const UI_SCALE_STEP: f32 = 0.05;

pub fn step_size(size: f32, delta: f32) -> f32 {
    (size + delta).clamp(SIZE_MIN, SIZE_MAX)
}

pub fn step_zoom(zoom: f32, delta: f32) -> f32 {
    (zoom + delta).clamp(ZOOM_MIN, ZOOM_MAX)
}

pub fn step_ui_scale(scale: f32, delta: f32) -> f32 {
    (scale + delta).clamp(UI_SCALE_MIN, UI_SCALE_MAX)
}

/// The scale as a signed percent around the detected baseline, "0" at
/// the baseline itself.
pub fn ui_scale_label(scale: f32) -> String {
    let percent = ((scale - 1.0) * 100.0).round() as i32;
    match percent {
        0 => "0".to_string(),
        p if p > 0 => format!("+{p}"),
        p => p.to_string(),
    }
}

/// Family list open for one of the two font rows.
struct Pick {
    row: usize,
    selected: usize,
    scroll: f32,
}

#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    center: (f32, f32),
    rows_top: f32,
    value_x: f32,
    list_top: f32,
    list_h: f32,
}

pub struct Settings {
    families: Vec<String>,
    body_family: String,
    code_family: String,
    body_size: f32,
    code_size: f32,
    ui_scale: f32,
    line_numbers: bool,
    word_count: bool,
    save_on_focus_loss: bool,
    save_after_pause: u32,
    row: usize,
    pick: Option<Pick>,
    drag: PanelDrag,
    geometry: Geometry,
}

/// What the dialog opens on: the settings as the app holds them.
pub struct Values {
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    pub ui_scale: f32,
    pub line_numbers: bool,
    pub word_count: bool,
    pub save_on_focus_loss: bool,
    pub save_after_pause: u32,
}

impl Settings {
    pub fn new(families: Vec<String>, values: Values) -> Settings {
        let Values {
            body_family,
            code_family,
            body_size,
            code_size,
            ui_scale,
            line_numbers,
            word_count,
            save_on_focus_loss,
            save_after_pause,
        } = values;
        Settings {
            families,
            body_family,
            code_family,
            body_size,
            code_size,
            ui_scale,
            line_numbers,
            word_count,
            save_on_focus_loss,
            save_after_pause,
            row: 0,
            pick: None,
            drag: PanelDrag::default(),
            geometry: Geometry::default(),
        }
    }

    fn view_change(&self) -> OverlayResult {
        OverlayResult::Apply(Action::SetView {
            body_family: self.body_family.clone(),
            code_family: self.code_family.clone(),
            body_size: self.body_size,
            code_size: self.code_size,
            ui_scale: self.ui_scale,
        })
    }

    fn open_pick(&mut self) {
        let current = if self.row == 0 {
            &self.body_family
        } else {
            &self.code_family
        };
        let selected = self.families.iter().position(|f| f == current).unwrap_or(0);
        self.pick = Some(Pick {
            row: self.row,
            selected,
            scroll: selected as f32 * LIST_ROW_H,
        });
    }

    /// Applies the picked family and closes the list.
    fn choose(&mut self, index: usize) -> OverlayResult {
        let Some(pick) = self.pick.take() else {
            return OverlayResult::Open;
        };
        let Some(family) = self.families.get(index) else {
            return OverlayResult::Open;
        };
        if pick.row == 0 {
            self.body_family = family.clone();
        } else {
            self.code_family = family.clone();
        }
        self.view_change()
    }

    /// An on/off row answers either arrow, Space and Enter the same
    /// way: it flips. None for a row that is not one.
    fn flip(&mut self) -> Option<OverlayResult> {
        match self.row {
            LINE_NUMBERS_ROW => {
                self.line_numbers = !self.line_numbers;
                Some(OverlayResult::Apply(Action::SetLineNumbers(
                    self.line_numbers,
                )))
            }
            WORD_COUNT_ROW => {
                self.word_count = !self.word_count;
                Some(OverlayResult::Apply(Action::SetWordCount(self.word_count)))
            }
            SAVE_FOCUS_ROW => {
                self.save_on_focus_loss = !self.save_on_focus_loss;
                Some(OverlayResult::Apply(Action::SetSaveOnFocusLoss(
                    self.save_on_focus_loss,
                )))
            }
            _ => None,
        }
    }

    fn step_row(&mut self, delta: f32) -> OverlayResult {
        if let Some(flipped) = self.flip() {
            return flipped;
        }
        if self.row == SAVE_PAUSE_ROW {
            self.save_after_pause = autosave::step_pause(self.save_after_pause, delta as i32);
            return OverlayResult::Apply(Action::SetSaveAfterPause(self.save_after_pause));
        }
        match self.row {
            2 => self.body_size = step_size(self.body_size, delta),
            3 => self.code_size = step_size(self.code_size, delta),
            4 => self.ui_scale = step_ui_scale(self.ui_scale, delta * UI_SCALE_STEP),
            _ => return OverlayResult::Open,
        }
        self.view_change()
    }

    fn pick_key(&mut self, key: &Key) -> OverlayResult {
        let Some(pick) = self.pick.as_mut() else {
            return OverlayResult::Open;
        };
        match key {
            Key::Named(NamedKey::Escape) => {
                self.pick = None;
            }
            Key::Named(NamedKey::ArrowDown) => {
                pick.selected = (pick.selected + 1).min(self.families.len().saturating_sub(1));
                pick.scroll = overlay::scroll_into_view(
                    pick.selected,
                    LIST_ROW_H,
                    pick.scroll,
                    self.geometry.list_h,
                );
            }
            Key::Named(NamedKey::ArrowUp) => {
                pick.selected = pick.selected.saturating_sub(1);
                pick.scroll = overlay::scroll_into_view(
                    pick.selected,
                    LIST_ROW_H,
                    pick.scroll,
                    self.geometry.list_h,
                );
            }
            Key::Named(NamedKey::Enter) => {
                let index = pick.selected;
                return self.choose(index);
            }
            _ => {}
        }
        OverlayResult::Open
    }

    fn max_list_scroll(&self) -> f32 {
        (self.families.len() as f32 * LIST_ROW_H - self.geometry.list_h).max(0.0)
    }
}

impl Overlay for Settings {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme, _caret_lit: bool) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let panel_w = PANEL_W.min(w - 40.0);
        let panel_h = match &self.pick {
            Some(_) => (h * 0.7).max(HEADER_H + 4.0 * LIST_ROW_H + FOOTER_H),
            None => HEADER_H + PAD + ROWS.len() as f32 * ROW_H + PAD + FOOTER_H,
        };
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let (px, py) = self.drag.place(center, panel_w, w, h);
        let rows_top = py + HEADER_H + PAD;
        let list_top = py + HEADER_H + PAD / 2.0;
        let list_h = panel_h - HEADER_H - FOOTER_H - PAD;
        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            center,
            rows_top,
            value_x: px + panel_w * VALUE_COLUMN,
            list_top,
            list_h,
        };

        overlay::panel_shadow(painter, px, py, panel_w, panel_h, RADIUS);
        painter.fill(px, py, panel_w, panel_h, RADIUS, ui.overlay_bg);
        overlay::panel_header(painter, px, py, panel_w, HEADER_H, RADIUS, theme);

        match &self.pick {
            Some(pick) => {
                let scroll = pick.scroll.clamp(0.0, self.max_list_scroll());
                let first = (scroll / LIST_ROW_H).floor() as usize;
                let offset = -(scroll - first as f32 * LIST_ROW_H);
                let list_bottom = list_top + list_h;
                let mut slot = 0usize;
                loop {
                    let index = first + slot;
                    let ry = list_top + offset + slot as f32 * LIST_ROW_H;
                    if index >= self.families.len() || ry > list_bottom {
                        break;
                    }
                    slot += 1;
                    if index == pick.selected {
                        painter.fill(
                            px + PAD / 2.0,
                            ry,
                            panel_w - PAD,
                            LIST_ROW_H - 2.0,
                            5.0,
                            overlay::row_highlight(theme),
                        );
                    }
                    // Each family renders in itself, as its own preview.
                    painter.text(
                        px + PAD,
                        ry + 4.0,
                        &self.families[index],
                        &self.families[index].clone(),
                        15.0,
                        if index == pick.selected { 700 } else { 400 },
                        ui.overlay_fg,
                    );
                }
                painter.fill(px, py, panel_w, list_top - py, RADIUS, ui.overlay_bg);
                painter.fill(
                    px,
                    list_bottom,
                    panel_w,
                    panel_h - (list_bottom - py),
                    RADIUS,
                    ui.overlay_bg,
                );
                overlay::panel_header(painter, px, py, panel_w, HEADER_H, RADIUS, theme);
                let title = ROWS[pick.row];
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
                painter.text(
                    px + PAD,
                    py + panel_h - FOOTER_H + 6.0,
                    "enter: choose \u{00B7} esc: back",
                    BODY_FAMILY,
                    12.0,
                    400,
                    theme.blocks.frontmatter_fg,
                );
            }
            None => {
                let title = "Settings";
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
                for (index, label) in ROWS.iter().enumerate() {
                    let ry = rows_top + index as f32 * ROW_H;
                    if index == self.row {
                        painter.fill(
                            px + PAD / 2.0,
                            ry,
                            panel_w - PAD,
                            ROW_H - 6.0,
                            6.0,
                            overlay::row_highlight(theme),
                        );
                    }
                    painter.text(
                        px + PAD,
                        ry + 8.0,
                        label,
                        BODY_FAMILY,
                        15.0,
                        if index == self.row { 700 } else { 400 },
                        ui.overlay_fg,
                    );
                    let value_x = self.geometry.value_x;
                    match index {
                        0 | 1 => {
                            let family = if index == 0 {
                                &self.body_family
                            } else {
                                &self.code_family
                            };
                            painter.text(
                                value_x,
                                ry + 8.0,
                                &family.clone(),
                                &family.clone(),
                                15.0,
                                400,
                                ui.overlay_fg,
                            );
                        }
                        _ => {
                            let value = match index {
                                2 => format!("{}", self.body_size as i32),
                                3 => format!("{}", self.code_size as i32),
                                LINE_NUMBERS_ROW | WORD_COUNT_ROW | SAVE_FOCUS_ROW => {
                                    let on = match index {
                                        LINE_NUMBERS_ROW => self.line_numbers,
                                        WORD_COUNT_ROW => self.word_count,
                                        _ => self.save_on_focus_loss,
                                    };
                                    if on { "on" } else { "off" }.to_string()
                                }
                                SAVE_PAUSE_ROW => autosave::pause_label(self.save_after_pause),
                                _ => format!("{}%", ui_scale_label(self.ui_scale)),
                            };
                            let text = format!("\u{2039}  {value}  \u{203A}");
                            painter.text(
                                value_x,
                                ry + 8.0,
                                &text,
                                BODY_FAMILY,
                                15.0,
                                400,
                                ui.overlay_fg,
                            );
                        }
                    }
                }
                painter.text(
                    px + PAD,
                    py + panel_h - FOOTER_H + 6.0,
                    "enter: change font \u{00B7} \u{2190}/\u{2192}: adjust \u{00B7} esc: close",
                    BODY_FAMILY,
                    12.0,
                    400,
                    theme.blocks.frontmatter_fg,
                );
            }
        }
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

    fn key(&mut self, key: &Key, ctrl: bool, _shift: bool) -> OverlayResult {
        if ctrl {
            return OverlayResult::Open;
        }
        if self.pick.is_some() {
            return self.pick_key(key);
        }
        match key {
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            Key::Named(NamedKey::ArrowDown) => self.row = (self.row + 1).min(ROWS.len() - 1),
            Key::Named(NamedKey::ArrowUp) => self.row = self.row.saturating_sub(1),
            Key::Named(NamedKey::Enter) if self.row < 2 => self.open_pick(),
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if let Some(flipped) = self.flip() {
                    return flipped;
                }
            }
            Key::Named(NamedKey::ArrowRight) => return self.step_row(1.0),
            Key::Named(NamedKey::ArrowLeft) => return self.step_row(-1.0),
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
            self.drag.press(x, y, px, py);
            return OverlayResult::Open;
        }
        if let Some(pick) = self.pick.as_mut() {
            // The pad above the list and the hint below it are no rows:
            // the rows scrolled out of sight lie under them.
            let list_top = self.geometry.list_top;
            if y < list_top || y >= list_top + self.geometry.list_h {
                return OverlayResult::Open;
            }
            let scroll = pick.scroll;
            let index = ((y - list_top + scroll) / LIST_ROW_H).floor() as usize;
            if index < self.families.len() {
                return self.choose(index);
            }
            return OverlayResult::Open;
        }
        let index = ((y - self.geometry.rows_top) / ROW_H).floor();
        if index < 0.0 || index as usize >= ROWS.len() {
            return OverlayResult::Open;
        }
        self.row = index as usize;
        match self.row {
            0 | 1 => {
                if x >= self.geometry.value_x {
                    self.open_pick();
                }
                OverlayResult::Open
            }
            _ => {
                if x >= self.geometry.value_x {
                    let delta = if x < self.geometry.value_x + 30.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    self.step_row(delta)
                } else {
                    OverlayResult::Open
                }
            }
        }
    }

    fn drag(&mut self, x: f32, y: f32) -> OverlayResult {
        self.drag.to(x, y, self.geometry.center);
        OverlayResult::Open
    }

    fn release(&mut self) {
        self.drag.release();
    }

    fn scroll(&mut self, lines: f32) -> OverlayResult {
        let max = self.max_list_scroll();
        if let Some(pick) = self.pick.as_mut() {
            pick.scroll = (pick.scroll + lines * LIST_ROW_H).clamp(0.0, max);
        }
        OverlayResult::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::overlay::Action;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn zoom_steps_and_clamps() {
        assert!(near(step_zoom(1.0, ZOOM_STEP), 1.1));
        assert!(near(step_zoom(2.95, ZOOM_STEP), 3.0));
        assert!(near(step_zoom(0.55, -ZOOM_STEP), 0.5));
        assert!(near(step_zoom(3.0, ZOOM_STEP), 3.0));
    }

    #[test]
    fn size_steps_and_clamps() {
        assert!(near(step_size(22.0, 1.0), 23.0));
        assert!(near(step_size(32.0, 1.0), 32.0));
        assert!(near(step_size(8.0, -1.0), 8.0));
    }

    fn settings() -> Settings {
        Settings::new(
            vec![
                "Courier Prime".to_string(),
                "DejaVu Sans".to_string(),
                "Other Font".to_string(),
            ],
            Values {
                body_family: "DejaVu Sans".to_string(),
                code_family: "Courier Prime".to_string(),
                body_size: 22.0,
                code_size: 20.0,
                ui_scale: 1.0,
                line_numbers: false,
                word_count: false,
                save_on_focus_loss: false,
                save_after_pause: 0,
            },
        )
    }

    fn press(s: &mut Settings, key: NamedKey) -> OverlayResult {
        s.key(&Key::Named(key), false, false)
    }

    #[test]
    fn a_click_off_the_font_list_chooses_no_font() {
        let families = (0..30).map(|i| format!("Family {i:02}")).collect();
        let mut s = Settings::new(
            families,
            Values {
                body_family: "Family 03".to_string(),
                code_family: "Family 04".to_string(),
                body_size: 22.0,
                code_size: 20.0,
                ui_scale: 1.0,
                line_numbers: false,
                word_count: false,
                save_on_focus_loss: false,
                save_after_pause: 0,
            },
        );
        press(&mut s, NamedKey::Enter);
        assert!(s.pick.is_some(), "the font list is open");
        s.geometry = Geometry {
            panel: (0.0, 0.0, 420.0, 400.0),
            list_top: 51.0,
            list_h: 300.0,
            ..Geometry::default()
        };
        // The hint under the list, and the pad between the header and
        // the first row: neither is a row of the list.
        for y in [380.0, 47.0] {
            assert!(
                matches!(s.click(200.0, y), OverlayResult::Open),
                "a click at y {y} chooses nothing"
            );
            assert!(s.pick.is_some(), "and the list stays open");
        }
        // A row of the list still answers.
        assert!(matches!(
            s.click(200.0, 60.0),
            OverlayResult::Apply(Action::SetView { .. })
        ));
    }

    #[test]
    fn family_pick_emits_view_change() {
        let mut s = settings();
        press(&mut s, NamedKey::Enter);
        press(&mut s, NamedKey::ArrowUp);
        let result = press(&mut s, NamedKey::Enter);
        let OverlayResult::Apply(Action::SetView { body_family, .. }) = result else {
            panic!("expected a view change");
        };
        assert_eq!(body_family, "Courier Prime");
    }

    #[test]
    fn size_row_steps_and_emits_view_change() {
        let mut s = settings();
        press(&mut s, NamedKey::ArrowDown);
        press(&mut s, NamedKey::ArrowDown);
        let result = press(&mut s, NamedKey::ArrowRight);
        let OverlayResult::Apply(Action::SetView { body_size, .. }) = result else {
            panic!("expected a view change");
        };
        assert!(near(body_size, 23.0));
        let result = press(&mut s, NamedKey::ArrowLeft);
        let OverlayResult::Apply(Action::SetView { body_size, .. }) = result else {
            panic!("expected a view change");
        };
        assert!(near(body_size, 22.0));
    }

    #[test]
    fn ui_scale_steps_and_clamps() {
        assert!(near(step_ui_scale(1.0, UI_SCALE_STEP), 1.05));
        assert!(near(
            step_ui_scale(UI_SCALE_MAX, UI_SCALE_STEP),
            UI_SCALE_MAX
        ));
        assert!(near(
            step_ui_scale(UI_SCALE_MIN, -UI_SCALE_STEP),
            UI_SCALE_MIN
        ));
    }

    #[test]
    fn the_line_numbers_row_flips_with_the_arrows_space_and_enter() {
        let mut s = settings();
        for _ in 0..LINE_NUMBERS_ROW {
            press(&mut s, NamedKey::ArrowDown);
        }
        let mut seen = Vec::new();
        for key in [
            NamedKey::ArrowRight,
            NamedKey::ArrowLeft,
            NamedKey::Space,
            NamedKey::Enter,
        ] {
            let OverlayResult::Apply(Action::SetLineNumbers(on)) = press(&mut s, key) else {
                panic!("{key:?} flips the row");
            };
            seen.push(on);
        }
        assert_eq!(seen, [true, false, true, false]);
    }

    #[test]
    fn the_word_count_row_flips_with_the_arrows_space_and_enter() {
        let mut s = settings();
        for _ in 0..WORD_COUNT_ROW {
            press(&mut s, NamedKey::ArrowDown);
        }
        let mut seen = Vec::new();
        for key in [
            NamedKey::ArrowRight,
            NamedKey::ArrowLeft,
            NamedKey::Space,
            NamedKey::Enter,
        ] {
            let OverlayResult::Apply(Action::SetWordCount(on)) = press(&mut s, key) else {
                panic!("{key:?} flips the row");
            };
            seen.push(on);
        }
        assert_eq!(seen, [true, false, true, false]);
    }

    #[test]
    fn the_focus_save_row_flips_with_the_arrows_space_and_enter() {
        let mut s = settings();
        for _ in 0..SAVE_FOCUS_ROW {
            press(&mut s, NamedKey::ArrowDown);
        }
        let mut seen = Vec::new();
        for key in [
            NamedKey::ArrowRight,
            NamedKey::ArrowLeft,
            NamedKey::Space,
            NamedKey::Enter,
        ] {
            let OverlayResult::Apply(Action::SetSaveOnFocusLoss(on)) = press(&mut s, key) else {
                panic!("{key:?} flips the row");
            };
            seen.push(on);
        }
        assert_eq!(seen, [true, false, true, false]);
    }

    #[test]
    fn the_pause_row_steps_through_its_choices_from_off() {
        let mut s = settings();
        for _ in 0..20 {
            press(&mut s, NamedKey::ArrowDown);
        }
        assert_eq!(s.row, SAVE_PAUSE_ROW, "the last row, and Down stops there");
        let mut seen = Vec::new();
        for key in [
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
            NamedKey::ArrowRight,
            NamedKey::ArrowLeft,
        ] {
            let OverlayResult::Apply(Action::SetSaveAfterPause(seconds)) = press(&mut s, key)
            else {
                panic!("{key:?} steps the row");
            };
            seen.push(seconds);
        }
        assert_eq!(seen, [0, 5, 15, 5], "Left at off stays off");
        assert!(
            matches!(press(&mut s, NamedKey::Enter), OverlayResult::Open),
            "a number has nothing to flip"
        );
    }

    #[test]
    fn the_autosave_rows_stand_last() {
        assert_eq!(ROWS.len(), SAVE_PAUSE_ROW + 1);
        assert_eq!(ROWS.get(SAVE_FOCUS_ROW), Some(&"save on focus loss"));
        assert_eq!(ROWS.get(SAVE_PAUSE_ROW), Some(&"save after a pause"));
    }

    #[test]
    fn ui_scale_labels_center_on_zero() {
        assert_eq!(ui_scale_label(1.0), "0");
        assert_eq!(ui_scale_label(1.05), "+5");
        assert_eq!(ui_scale_label(1.3), "+30");
        assert_eq!(ui_scale_label(0.85), "-15");
    }

    #[test]
    fn scale_row_steps_and_emits_view_change() {
        let mut s = settings();
        for _ in 0..4 {
            press(&mut s, NamedKey::ArrowDown);
        }
        let result = press(&mut s, NamedKey::ArrowRight);
        let OverlayResult::Apply(Action::SetView { ui_scale, .. }) = result else {
            panic!("expected a view change");
        };
        assert!(near(ui_scale, 1.05));
        let result = press(&mut s, NamedKey::ArrowLeft);
        let OverlayResult::Apply(Action::SetView { ui_scale, .. }) = result else {
            panic!("expected a view change");
        };
        assert!(near(ui_scale, 1.0));
    }
}
