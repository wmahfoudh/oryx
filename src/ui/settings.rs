//! Settings overlay: body and code font families and sizes, applied live
//! and persisted by the app. Zoom stepping helpers live here too; zoom is
//! session state and never saved.

use winit::keyboard::{Key, NamedKey};

use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::Theme;
use crate::ui::overlay::{self, Action, Overlay, OverlayResult, PanelDrag};

const ROW_H: f32 = 40.0;
const LIST_ROW_H: f32 = 28.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 44.0;
const FOOTER_H: f32 = 30.0;
const PANEL_W: f32 = 420.0;
const RADIUS: f32 = 8.0;

const ROWS: [&str; 5] = [
    "body font",
    "code font",
    "body size",
    "code size",
    "interface scale",
];

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
    row: usize,
    pick: Option<Pick>,
    drag: PanelDrag,
    geometry: Geometry,
}

impl Settings {
    pub fn new(
        families: Vec<String>,
        body_family: String,
        code_family: String,
        body_size: f32,
        code_size: f32,
        ui_scale: f32,
    ) -> Settings {
        Settings {
            families,
            body_family,
            code_family,
            body_size,
            code_size,
            ui_scale,
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

    fn step_row(&mut self, delta: f32) -> OverlayResult {
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
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
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
            value_x: px + panel_w * 0.42,
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
            let scroll = pick.scroll;
            let index = ((y - self.geometry.list_top + scroll) / LIST_ROW_H).floor() as usize;
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
            "DejaVu Sans".to_string(),
            "Courier Prime".to_string(),
            22.0,
            20.0,
            1.0,
        )
    }

    fn press(s: &mut Settings, key: NamedKey) -> OverlayResult {
        s.key(&Key::Named(key), false, false)
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
