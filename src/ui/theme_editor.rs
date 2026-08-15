//! Theme editor overlay: all 49 color roles with inline hex entry and a
//! color picker, restyling the document live and saving through
//! `theme::save`. Bundled themes save to a duplicate.

use std::path::{Path, PathBuf};
use std::time::Instant;

use winit::keyboard::{Key, NamedKey};

use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{self, Rgba, Theme};
use crate::ui::overlay::{self, inside, Action, Overlay, OverlayResult, PanelDrag};
use crate::ui::textfield::TextField;
use crate::ui::theme_browser::duplicate_path;

const ROW_H: f32 = 26.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 44.0;
const FOOTER_H: f32 = 30.0;
const PANEL_W: f32 = 660.0;
const PICKER_W: f32 = 200.0;
const STRIP_H: f32 = 16.0;

/// One line in the role list: a group caption or an editable role.
enum Entry {
    Header(&'static str),
    Role(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Sv,
    Hue,
    Alpha,
    /// Dragging the panel by its header.
    Move,
}

#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    /// Centered panel origin before the user offset.
    center: (f32, f32),
    list_top: f32,
    list_h: f32,
    list_w: f32,
    sv: (f32, f32, f32, f32),
    hue: (f32, f32, f32, f32),
    alpha: (f32, f32, f32, f32),
    hex: (f32, f32, f32, f32),
}

pub struct ThemeEditor {
    name: String,
    target: PathBuf,
    /// Working copy; the app previews it live.
    theme: Theme,
    entries: Vec<Entry>,
    selected: usize,
    /// Picker state for the selected role; kept besides the RGB so hue
    /// survives while saturation or value sit at zero.
    hsv: (f32, f32, f32),
    scroll: f32,
    hex_entry: Option<TextField>,
    /// Caret offsets for the open hex field, measured while drawing so a
    /// later click can place the caret without a painter.
    hex_offsets: Vec<f32>,
    drag: Option<Part>,
    /// The header drag, shared panel scaffolding; the picker parts
    /// above carry their own drags.
    panel: PanelDrag,
    dirty: bool,
    /// Created on first copy or paste, kept alive for X11.
    clipboard: Option<arboard::Clipboard>,
    geometry: Geometry,
}

impl ThemeEditor {
    /// None when the file does not load.
    pub fn new(path: &Path) -> Option<ThemeEditor> {
        let theme = theme::load_file(path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let mut entries = Vec::new();
        let mut group = "";
        for (index, (g, _)) in theme::ROLES.iter().enumerate() {
            if *g != group {
                entries.push(Entry::Header(g));
                group = g;
            }
            entries.push(Entry::Role(index));
        }
        let first = theme::role(&theme, 0);
        Some(ThemeEditor {
            name,
            target: save_target(path),
            hsv: {
                let (h, s, v) = rgb_to_hsv(first.r, first.g, first.b);
                (h, s, v)
            },
            theme,
            entries,
            selected: 0,
            scroll: 0.0,
            hex_entry: None,
            hex_offsets: Vec::new(),
            drag: None,
            panel: PanelDrag::default(),
            dirty: false,
            clipboard: None,
            geometry: Geometry::default(),
        })
    }

    /// The theme as currently edited, for the app to preview.
    pub fn current(&self) -> Theme {
        self.theme.clone()
    }

    fn color(&self) -> Rgba {
        theme::role(&self.theme, self.selected)
    }

    /// Adopts the selected role's color into the picker state.
    fn sync_picker(&mut self) {
        let c = self.color();
        self.hsv = rgb_to_hsv(c.r, c.g, c.b);
    }

    fn set_color(&mut self, c: Rgba) {
        *theme::role_mut(&mut self.theme, self.selected) = c;
        self.dirty = true;
    }

    /// Applies the picker state to the selected role, keeping its alpha.
    fn apply_picker(&mut self) -> OverlayResult {
        let (h, s, v) = self.hsv;
        let (r, g, b) = hsv_to_rgb(h, s, v);
        let a = self.color().a;
        self.set_color(Rgba { r, g, b, a });
        OverlayResult::Apply(Action::PreviewTheme(Box::new(self.theme.clone())))
    }

    fn entry_row(&self, role: usize) -> usize {
        self.entries
            .iter()
            .position(|e| matches!(e, Entry::Role(index) if *index == role))
            .unwrap_or(0)
    }

    fn select(&mut self, role: usize) {
        self.selected = role.min(theme::ROLES.len() - 1);
        self.hex_entry = None;
        self.sync_picker();
        self.scroll = overlay::scroll_into_view(
            self.entry_row(self.selected),
            ROW_H,
            self.scroll,
            self.geometry.list_h,
        );
    }

    fn step(&mut self, delta: i32) {
        let next = if delta > 0 {
            (self.selected + 1).min(theme::ROLES.len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
        self.select(next);
    }

    fn max_scroll(&self) -> f32 {
        (self.entries.len() as f32 * ROW_H - self.geometry.list_h).max(0.0)
    }

    fn save(&mut self) -> OverlayResult {
        if let Err(err) = theme::save(&self.target, &self.theme) {
            eprintln!("oryx: cannot save theme: {err}");
            return OverlayResult::Open;
        }
        self.dirty = false;
        OverlayResult::Apply(Action::SetTheme(self.target.clone()))
    }

    fn hex_key(&mut self, key: &Key, ctrl: bool, shift: bool) -> OverlayResult {
        match key {
            Key::Named(NamedKey::Escape) => {
                self.hex_entry = None;
                return OverlayResult::Open;
            }
            Key::Named(NamedKey::Enter) => {
                let buffer = self.hex_text();
                if let Some(c) = theme::parse_hex(&buffer) {
                    self.hex_entry = None;
                    self.set_color(c);
                    self.sync_picker();
                    return OverlayResult::Apply(Action::PreviewTheme(Box::new(
                        self.theme.clone(),
                    )));
                }
                return OverlayResult::Open;
            }
            _ => {}
        }
        if let Some(field) = self.hex_entry.as_mut() {
            field.key(key, ctrl, shift);
        }
        OverlayResult::Open
    }

    /// The hex under edit, empty when the field is closed.
    fn hex_text(&self) -> String {
        self.hex_entry
            .as_ref()
            .map(|f| f.text().to_string())
            .unwrap_or_default()
    }

    /// Opens the hex field on the selected role with everything selected,
    /// so the first keystroke replaces the value.
    fn open_hex(&mut self) {
        let mut field = TextField::new(theme::hex_string(self.color()));
        field.select_all();
        self.hex_entry = Some(field);
    }

    /// Copies the hex under edit, or the selected role's value.
    fn copy_hex(&mut self) {
        let text = match self.hex_entry.as_ref() {
            Some(field) if !field.selected_text().is_empty() => field.selected_text().to_string(),
            Some(field) => field.text().to_string(),
            None => theme::hex_string(self.color()),
        };
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = self.clipboard.as_mut() {
            if let Err(err) = clipboard.set_text(text) {
                eprintln!("oryx: clipboard copy failed: {err}");
            }
        }
    }

    /// Pastes into the hex field when it is open, otherwise applies a
    /// pasted hex color directly. A missing `#` prefix is tolerated.
    fn paste_hex(&mut self) -> OverlayResult {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let Some(text) = self
            .clipboard
            .as_mut()
            .and_then(|clipboard| clipboard.get_text().ok())
        else {
            return OverlayResult::Open;
        };
        let text = text.trim().to_string();
        if let Some(field) = self.hex_entry.as_mut() {
            field.insert(&text);
            return OverlayResult::Open;
        }
        let color = theme::parse_hex(&text).or_else(|| theme::parse_hex(&format!("#{text}")));
        if let Some(c) = color {
            self.set_color(c);
            self.sync_picker();
            return OverlayResult::Apply(Action::PreviewTheme(Box::new(self.theme.clone())));
        }
        OverlayResult::Open
    }

    fn picker_at(&mut self, x: f32, y: f32) -> OverlayResult {
        let Some(part) = self.drag else {
            return OverlayResult::Open;
        };
        match part {
            Part::Move => {
                self.panel.to(x, y, self.geometry.center);
                OverlayResult::Open
            }
            Part::Sv => {
                let (sx, sy, sw, sh) = self.geometry.sv;
                self.hsv.1 = ((x - sx) / sw).clamp(0.0, 1.0);
                self.hsv.2 = 1.0 - ((y - sy) / sh).clamp(0.0, 1.0);
                self.apply_picker()
            }
            Part::Hue => {
                let (hx, _, hw, _) = self.geometry.hue;
                self.hsv.0 = (((x - hx) / hw).clamp(0.0, 1.0) * 360.0).min(359.9);
                self.apply_picker()
            }
            Part::Alpha => {
                let (ax, _, aw, _) = self.geometry.alpha;
                let a = (((x - ax) / aw).clamp(0.0, 1.0) * 255.0).round() as u8;
                let mut c = self.color();
                c.a = a;
                self.set_color(c);
                OverlayResult::Apply(Action::PreviewTheme(Box::new(self.theme.clone())))
            }
        }
    }
}

/// Checkerboard ground for the alpha strip.
fn checker(u: f32, v: f32, w: f32, h: f32) -> u8 {
    let cell = 6.0;
    let cx = (u * w / cell) as u32;
    let cy = (v * h / cell) as u32;
    if (cx + cy) % 2 == 0 {
        200
    } else {
        150
    }
}

impl Overlay for ThemeEditor {
    fn draw(&mut self, painter: &mut Painter, app_theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &app_theme.ui;
        let panel_w = PANEL_W.min(w - 40.0);
        let panel_h = (h * 0.85).max(300.0);
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let (px, py) = self.panel.place(center, panel_w, w, h);
        let list_top = py + HEADER_H + PAD / 2.0;
        let list_h = panel_h - HEADER_H - FOOTER_H - PAD;
        let list_w = panel_w - PICKER_W - 3.0 * PAD;
        let rx = px + panel_w - PICKER_W - PAD;

        let sv = (rx, list_top, PICKER_W, PICKER_W * 0.85);
        let hue = (rx, sv.1 + sv.3 + 10.0, PICKER_W, STRIP_H);
        let alpha = (rx, hue.1 + STRIP_H + 8.0, PICKER_W, STRIP_H);
        let hex = (rx, alpha.1 + STRIP_H + 10.0, PICKER_W, 26.0);
        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            center,
            list_top,
            list_h,
            list_w,
            sv,
            hue,
            alpha,
            hex,
        };
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());

        overlay::panel_shadow(painter, px, py, panel_w, panel_h, 8.0);
        painter.fill(px, py, panel_w, panel_h, 8.0, ui.overlay_bg);

        // Role list.
        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = -(self.scroll - first as f32 * ROW_H);
        let list_bottom = list_top + list_h;
        let mut slot = 0usize;
        loop {
            let at = first + slot;
            let ry = list_top + offset + slot as f32 * ROW_H;
            if at >= self.entries.len() || ry > list_bottom {
                break;
            }
            slot += 1;
            match self.entries[at] {
                Entry::Header(group) => {
                    painter.text(
                        px + PAD,
                        ry + 6.0,
                        group,
                        BODY_FAMILY,
                        12.0,
                        700,
                        app_theme.blocks.frontmatter_fg,
                    );
                }
                Entry::Role(index) => {
                    let c = theme::role(&self.theme, index);
                    if index == self.selected {
                        painter.fill(
                            px + PAD / 2.0,
                            ry,
                            list_w + PAD,
                            ROW_H - 2.0,
                            5.0,
                            ui.overlay_highlight,
                        );
                    }
                    painter.text(
                        px + PAD + 10.0,
                        ry + 4.0,
                        theme::ROLES[index].1,
                        BODY_FAMILY,
                        14.0,
                        if index == self.selected { 700 } else { 400 },
                        ui.overlay_fg,
                    );
                    let sx = px + PAD + list_w - 16.0;
                    painter.fill(sx, ry + 5.0, 14.0, 14.0, 3.0, c);
                    painter.stroke(
                        sx,
                        ry + 5.0,
                        14.0,
                        14.0,
                        3.0,
                        1.0,
                        app_theme.blocks.table_border,
                    );
                    painter.text(
                        sx - 78.0,
                        ry + 6.0,
                        &theme::hex_string(c),
                        CODE_FAMILY,
                        12.0,
                        400,
                        app_theme.blocks.frontmatter_fg,
                    );
                }
            }
        }

        // Masks over list overflow, then header and footer content.
        painter.fill(px, py, panel_w, list_top - py, 8.0, ui.overlay_bg);
        painter.fill(
            px,
            list_bottom,
            panel_w,
            panel_h - (list_bottom - py),
            8.0,
            ui.overlay_bg,
        );
        let title = format!("{}{}", self.name, if self.dirty { " \u{2022}" } else { "" });
        painter.text(
            px + PAD,
            py + 12.0,
            &title,
            BODY_FAMILY,
            16.0,
            700,
            ui.overlay_fg,
        );
        let target_note = format!(
            "saves to {}",
            self.target
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
        );
        let note_w = painter.measure(&target_note, BODY_FAMILY, 12.0, 400);
        painter.text(
            px + panel_w - PAD - note_w,
            py + 16.0,
            &target_note,
            BODY_FAMILY,
            12.0,
            400,
            app_theme.blocks.frontmatter_fg,
        );
        painter.text(
            px + PAD,
            py + panel_h - FOOTER_H + 6.0,
            "enter: hex \u{00B7} drag: pick \u{00B7} ctrl+s: save \u{00B7} esc: close",
            BODY_FAMILY,
            12.0,
            400,
            app_theme.blocks.frontmatter_fg,
        );

        // Picker: saturation-value square under the current hue.
        let hue_now = self.hsv.0;
        painter.shade(sv.0, sv.1, sv.2, sv.3, |u, v| {
            let (r, g, b) = hsv_to_rgb(hue_now, u, 1.0 - v);
            Rgba { r, g, b, a: 255 }
        });
        painter.stroke(
            sv.0,
            sv.1,
            sv.2,
            sv.3,
            0.0,
            1.0,
            app_theme.blocks.table_border,
        );
        let tx = sv.0 + self.hsv.1 * sv.2;
        let ty = sv.1 + (1.0 - self.hsv.2) * sv.3;
        painter.stroke(
            tx - 5.0,
            ty - 5.0,
            10.0,
            10.0,
            5.0,
            2.0,
            Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );
        painter.stroke(
            tx - 6.0,
            ty - 6.0,
            12.0,
            12.0,
            6.0,
            1.0,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        );

        painter.shade(hue.0, hue.1, hue.2, hue.3, |u, _| {
            let (r, g, b) = hsv_to_rgb(u * 360.0, 1.0, 1.0);
            Rgba { r, g, b, a: 255 }
        });
        let hx = hue.0 + (self.hsv.0 / 360.0) * hue.2;
        painter.stroke(
            hx - 2.0,
            hue.1 - 2.0,
            4.0,
            hue.3 + 4.0,
            2.0,
            2.0,
            Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );

        let current = self.color();
        painter.shade(alpha.0, alpha.1, alpha.2, alpha.3, |u, v| {
            let ground = checker(u, v, alpha.2, alpha.3);
            let blend = |c: u8| ((c as f32 * u) + ground as f32 * (1.0 - u)) as u8;
            Rgba {
                r: blend(current.r),
                g: blend(current.g),
                b: blend(current.b),
                a: 255,
            }
        });
        let ax = alpha.0 + (current.a as f32 / 255.0) * alpha.2;
        painter.stroke(
            ax - 2.0,
            alpha.1 - 2.0,
            4.0,
            alpha.3 + 4.0,
            2.0,
            2.0,
            Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );

        // Hex row: swatch plus value, or the entry field while typing.
        painter.fill(hex.0, hex.1 + 3.0, 20.0, 20.0, 4.0, current);
        painter.stroke(
            hex.0,
            hex.1 + 3.0,
            20.0,
            20.0,
            4.0,
            1.0,
            app_theme.blocks.table_border,
        );
        let mut offsets = Vec::new();
        match &self.hex_entry {
            Some(field) => {
                let text = field.text();
                let valid = theme::parse_hex(text).is_some();
                let border = if valid {
                    app_theme.text.link
                } else {
                    app_theme.alerts.caution
                };
                painter.fill(hex.0 + 28.0, hex.1, hex.2 - 28.0, 26.0, 4.0, ui.overlay_bg);
                painter.stroke(hex.0 + 28.0, hex.1, hex.2 - 28.0, 26.0, 4.0, 1.0, border);
                let text_x = hex.0 + 34.0;
                if let Some(range) = field.selection() {
                    let from = painter.measure(&text[..range.start], CODE_FAMILY, 14.0, 400);
                    let to = painter.measure(&text[..range.end], CODE_FAMILY, 14.0, 400);
                    painter.fill(
                        text_x + from,
                        hex.1 + 5.0,
                        to - from,
                        16.0,
                        0.0,
                        ui.selection_bg,
                    );
                }
                painter.text(
                    text_x,
                    hex.1 + 5.0,
                    text,
                    CODE_FAMILY,
                    14.0,
                    400,
                    ui.overlay_fg,
                );
                let caret = field.caret_offset(|s| painter.measure(s, CODE_FAMILY, 14.0, 400));
                offsets = field.offsets(|s| painter.measure(s, CODE_FAMILY, 14.0, 400));
                painter.fill(
                    text_x + caret + 1.0,
                    hex.1 + 5.0,
                    1.5,
                    16.0,
                    0.0,
                    ui.overlay_fg,
                );
            }
            None => {
                painter.text(
                    hex.0 + 34.0,
                    hex.1 + 5.0,
                    &theme::hex_string(current),
                    CODE_FAMILY,
                    14.0,
                    400,
                    ui.overlay_fg,
                );
            }
        }
        self.hex_offsets = offsets;
    }

    fn key(&mut self, key: &Key, ctrl: bool, shift: bool) -> OverlayResult {
        if ctrl {
            if let Key::Character(c) = key {
                match c.as_str() {
                    "s" | "S" => return self.save(),
                    "c" | "C" => self.copy_hex(),
                    "v" | "V" => return self.paste_hex(),
                    "a" | "A" => {
                        if let Some(field) = self.hex_entry.as_mut() {
                            field.select_all();
                        }
                    }
                    _ => {}
                }
            }
            return OverlayResult::Open;
        }
        if self.hex_entry.is_some() {
            return self.hex_key(key, ctrl, shift);
        }
        match key {
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            Key::Named(NamedKey::ArrowDown) => self.step(1),
            Key::Named(NamedKey::ArrowUp) => self.step(-1),
            Key::Named(NamedKey::Enter) => self.open_hex(),
            _ => {}
        }
        OverlayResult::Open
    }

    fn click(&mut self, x: f32, y: f32) -> OverlayResult {
        let (px, py, pw, ph) = self.geometry.panel;
        if x < px || x > px + pw || y < py || y > py + ph {
            return OverlayResult::Close;
        }
        if inside(self.geometry.hex, x, y) {
            if self.hex_entry.is_none() {
                // The first click opens the field with everything selected;
                // later ones place the caret.
                self.open_hex();
                return OverlayResult::Open;
            }
            let text_x = self.geometry.hex.0 + 34.0;
            let offsets = std::mem::take(&mut self.hex_offsets);
            if let Some(field) = self.hex_entry.as_mut() {
                field.click(x - text_x, &offsets, Instant::now());
            }
            self.hex_offsets = offsets;
            return OverlayResult::Open;
        }
        self.hex_entry = None;
        if y < py + HEADER_H {
            self.drag = Some(Part::Move);
            self.panel.press(x, y, px, py);
            return OverlayResult::Open;
        }
        if inside(self.geometry.sv, x, y) {
            self.drag = Some(Part::Sv);
            return self.picker_at(x, y);
        }
        if inside(self.geometry.hue, x, y) {
            self.drag = Some(Part::Hue);
            return self.picker_at(x, y);
        }
        if inside(self.geometry.alpha, x, y) {
            self.drag = Some(Part::Alpha);
            return self.picker_at(x, y);
        }
        let list_left = px;
        let list_right = px + PAD + self.geometry.list_w + PAD;
        if x >= list_left
            && x <= list_right
            && y >= self.geometry.list_top
            && y <= self.geometry.list_top + self.geometry.list_h
        {
            let at = ((y - self.geometry.list_top + self.scroll) / ROW_H).floor() as usize;
            if let Some(Entry::Role(index)) = self.entries.get(at) {
                self.select(*index);
            }
        }
        OverlayResult::Open
    }

    fn drag(&mut self, x: f32, y: f32) -> OverlayResult {
        self.picker_at(x, y)
    }

    fn release(&mut self) {
        self.drag = None;
        self.panel.release();
    }

    fn scroll(&mut self, lines: f32) -> OverlayResult {
        self.scroll = (self.scroll + lines * ROW_H).clamp(0.0, self.max_scroll());
        OverlayResult::Open
    }
}

/// Where edits of this theme file get saved: the file itself, or a fresh
/// duplicate when it belongs to the shipped collection.
pub fn save_target(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if theme::is_bundled(stem) {
        duplicate_path(path)
    } else {
        path.to_path_buf()
    }
}

/// Hue in degrees [0, 360), saturation and value in [0, 1], to RGB bytes.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (to_byte(r + m), to_byte(g + m), to_byte(b + m))
}

fn to_byte(f: f32) -> u8 {
    (f * 255.0).round().clamp(0.0, 255.0) as u8
}

/// RGB bytes to hue in degrees [0, 360), saturation and value in [0, 1].
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == rf {
        60.0 * ((gf - bf) / d)
    } else if max == gf {
        60.0 * ((bf - rf) / d + 2.0)
    } else {
        60.0 * ((rf - gf) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.rem_euclid(360.0), s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_hits_known_corners() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
        assert_eq!(hsv_to_rgb(180.0, 1.0, 0.0), (0, 0, 0));
    }

    #[test]
    fn rgb_hsv_round_trips_within_one() {
        for (r, g, b) in [
            (255u8, 0u8, 0u8),
            (12, 200, 99),
            (240, 240, 240),
            (1, 2, 3),
            (128, 64, 200),
            (99, 179, 164),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!(
                (r as i16 - r2 as i16).abs() <= 1
                    && (g as i16 - g2 as i16).abs() <= 1
                    && (b as i16 - b2 as i16).abs() <= 1,
                "({r},{g},{b}) round-tripped to ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn bundled_theme_saves_to_copy() {
        let bundled = save_target(Path::new("themes/dracula.toml"));
        assert_eq!(bundled.file_name().unwrap(), "dracula-copy.toml");
        let own = save_target(Path::new("themes/my-own-theme.toml"));
        assert_eq!(own.file_name().unwrap(), "my-own-theme.toml");
    }
}
