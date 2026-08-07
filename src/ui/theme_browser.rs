//! Theme browser overlay: the scanned collection with preview swatches,
//! keyboard navigation, pixel-smooth scrolling, and per-row duplicate,
//! delete, and rename actions.

use std::path::{Path, PathBuf};
use std::time::Instant;

use winit::keyboard::{Key, NamedKey};

use crate::input::DOUBLE_CLICK;
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{self, Rgba, Theme};
use crate::ui::overlay::{inside, Action, Overlay, OverlayResult};
use crate::ui::textfield::TextField;

/// A drawn rename field: the caret offsets of its text and its box.
type RenameBox = (Vec<f32>, (f32, f32, f32, f32));

const ROW_H: f32 = 36.0;
const PAD: f32 = 12.0;
const HEADER_H: f32 = 46.0;
const PANEL_W: f32 = 400.0;
const SWATCH: f32 = 16.0;
const ICON_BOX: f32 = 22.0;
const RADIUS: f32 = 8.0;

struct Row {
    name: String,
    path: PathBuf,
    swatches: Option<(Rgba, Rgba)>,
}

/// Panel geometry cached at draw time for click hit testing.
#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    center: (f32, f32),
    list_top: f32,
    list_h: f32,
    name_x: f32,
    delete_x: f32,
    duplicate_x: f32,
    edit_x: f32,
}

enum Commit {
    Renamed(String, String),
    Unchanged,
    Invalid,
}

pub struct ThemeBrowser {
    dirs: Vec<PathBuf>,
    rows: Vec<Row>,
    selected: usize,
    /// List scroll offset in pixels.
    scroll: f32,
    pending_delete: Option<usize>,
    /// Inline rename in progress: row index and the edited name.
    renaming: Option<(usize, TextField)>,
    /// Caret offsets and box of the open rename field, measured while
    /// drawing so a later click can place the caret.
    rename_offsets: Vec<f32>,
    rename_rect: (f32, f32, f32, f32),
    clipboard: Option<arboard::Clipboard>,
    last_name_click: Option<(usize, Instant)>,
    moving: bool,
    grab: (f32, f32),
    offset: (f32, f32),
    geometry: Geometry,
}

impl ThemeBrowser {
    pub fn new(dirs: Vec<PathBuf>, active: &str) -> ThemeBrowser {
        let mut browser = ThemeBrowser {
            dirs,
            rows: Vec::new(),
            selected: 0,
            scroll: 0.0,
            pending_delete: None,
            renaming: None,
            rename_offsets: Vec::new(),
            rename_rect: (0.0, 0.0, 0.0, 0.0),
            clipboard: None,
            last_name_click: None,
            moving: false,
            grab: (0.0, 0.0),
            offset: (0.0, 0.0),
            geometry: Geometry::default(),
        };
        browser.rescan();
        if let Some(index) = browser.rows.iter().position(|r| r.name == active) {
            browser.selected = index;
        }
        browser
    }

    /// Rebuilds the row list from the theme directories; the first
    /// directory wins on duplicate names.
    fn rescan(&mut self) {
        let mut rows: Vec<Row> = Vec::new();
        for dir in &self.dirs {
            for entry in theme::scan(dir) {
                if rows.iter().any(|r| r.name == entry.name) {
                    continue;
                }
                rows.push(Row {
                    swatches: theme::preview(&entry.path),
                    name: entry.name,
                    path: entry.path,
                });
            }
        }
        // Light themes first, dark after, names inside each group, so
        // the two halves of the collection read as two shelves.
        rows.sort_by(|a, b| {
            (theme::dark_rank(&a.swatches), &a.name).cmp(&(theme::dark_rank(&b.swatches), &b.name))
        });
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
        self.pending_delete = None;
    }

    fn max_scroll(&self) -> f32 {
        (self.rows.len() as f32 * ROW_H - self.geometry.list_h).max(0.0)
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows.len().saturating_sub(1));
        self.pending_delete = None;
        let row_top = self.selected as f32 * ROW_H;
        let list_h = self.geometry.list_h.max(ROW_H);
        if row_top < self.scroll {
            self.scroll = row_top;
        } else if row_top + ROW_H > self.scroll + list_h {
            self.scroll = row_top + ROW_H - list_h;
        }
    }

    fn select_by_name(&mut self, name: &str) {
        if let Some(index) = self.rows.iter().position(|r| r.name == name) {
            self.select(index);
        }
    }

    /// The highlighted theme as a live preview. Nothing persists: the app
    /// puts the confirmed theme back when the browser closes unconfirmed.
    fn preview(&self) -> OverlayResult {
        match self
            .rows
            .get(self.selected)
            .and_then(|row| theme::load_file(&row.path))
        {
            Some(theme) => OverlayResult::Apply(Action::PreviewTheme(Box::new(theme))),
            None => OverlayResult::Open,
        }
    }

    /// Copies the row's file and immediately opens the copy for renaming.
    fn duplicate(&mut self, index: usize) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let target = duplicate_path(&row.path);
        let name = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if let Err(err) = std::fs::copy(&row.path, &target) {
            eprintln!("oryx: cannot duplicate theme: {err}");
            return;
        }
        self.rescan();
        self.select_by_name(&name);
        let index = self.selected;
        self.start_rename(index, name);
    }

    fn delete(&mut self, index: usize) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        if let Err(err) = std::fs::remove_file(&row.path) {
            eprintln!("oryx: cannot delete theme: {err}");
        }
        let selected = self.selected;
        self.rescan();
        self.select(selected.min(self.rows.len().saturating_sub(1)));
    }

    /// Opens the inline rename with the whole name selected, so the first
    /// keystroke replaces it.
    fn start_rename(&mut self, index: usize, name: String) {
        let mut field = TextField::new(name);
        field.select_all();
        self.renaming = Some((index, field));
    }

    /// Attempts to finish the rename in progress.
    fn commit_rename(&mut self) -> Commit {
        let Some((index, buffer)) = self
            .renaming
            .as_ref()
            .map(|(index, field)| (*index, field.text().to_string()))
        else {
            return Commit::Unchanged;
        };
        let Some(row) = self.rows.get(index) else {
            self.renaming = None;
            return Commit::Unchanged;
        };
        let from = row.name.clone();
        if buffer.trim() == from {
            self.renaming = None;
            return Commit::Unchanged;
        }
        let Some(dir) = row.path.parent() else {
            self.renaming = None;
            return Commit::Unchanged;
        };
        let Some(target) = rename_target(dir, &buffer) else {
            return Commit::Invalid;
        };
        let to = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if let Err(err) = std::fs::rename(&row.path, &target) {
            eprintln!("oryx: cannot rename theme: {err}");
            self.renaming = None;
            return Commit::Unchanged;
        }
        self.renaming = None;
        self.rescan();
        self.select_by_name(&to);
        Commit::Renamed(from, to)
    }

    fn rename_key(&mut self, key: &Key, ctrl: bool, shift: bool) -> OverlayResult {
        match key {
            Key::Named(NamedKey::Escape) => {
                self.renaming = None;
                return OverlayResult::Open;
            }
            Key::Named(NamedKey::Enter) => {
                if let Commit::Renamed(from, to) = self.commit_rename() {
                    return OverlayResult::Apply(Action::RenamedTheme { from, to });
                }
                return OverlayResult::Open;
            }
            _ => {}
        }
        if ctrl {
            if let Key::Character(c) = key {
                match c.as_str() {
                    "c" | "C" => return self.copy_name(),
                    "x" | "X" => return self.copy_name(),
                    "v" | "V" => return self.paste_name(),
                    _ => {}
                }
            }
        }
        if let Some((_, field)) = self.renaming.as_mut() {
            field.key(key, ctrl, shift);
        }
        OverlayResult::Open
    }

    /// Copies the selected part of the name under edit, or all of it.
    fn copy_name(&mut self) -> OverlayResult {
        let Some((_, field)) = self.renaming.as_ref() else {
            return OverlayResult::Open;
        };
        let text = if field.selected_text().is_empty() {
            field.text().to_string()
        } else {
            field.selected_text().to_string()
        };
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = self.clipboard.as_mut() {
            if let Err(err) = clipboard.set_text(text) {
                eprintln!("oryx: clipboard copy failed: {err}");
            }
        }
        OverlayResult::Open
    }

    fn paste_name(&mut self) -> OverlayResult {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let text = self
            .clipboard
            .as_mut()
            .and_then(|clipboard| clipboard.get_text().ok());
        if let (Some(text), Some((_, field))) = (text, self.renaming.as_mut()) {
            field.insert(text.trim());
        }
        OverlayResult::Open
    }
}

impl Overlay for ThemeBrowser {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let mut rename_box: Option<RenameBox> = None;
        let (w, h) = (painter.width(), painter.height());
        let panel_w = PANEL_W.min(w - 40.0);
        let max_h = (h * 0.8).max(ROW_H + HEADER_H + 2.0 * PAD);
        let want_h = HEADER_H + PAD + self.rows.len() as f32 * ROW_H + PAD;
        let panel_h = want_h.min(max_h);
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let px = (center.0 + self.offset.0).clamp(60.0 - panel_w, w - 60.0);
        let py = (center.1 + self.offset.1).clamp(-8.0, h - HEADER_H);
        let list_top = py + HEADER_H + PAD;
        let list_h = panel_h - HEADER_H - 2.0 * PAD;
        let list_bottom = list_top + list_h;

        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            center,
            list_top,
            list_h,
            name_x: px + PAD + 2.0 * SWATCH + 16.0,
            delete_x: px + panel_w - PAD - ICON_BOX,
            duplicate_x: px + panel_w - PAD - 2.0 * ICON_BOX - 6.0,
            edit_x: px + panel_w - PAD - 3.0 * ICON_BOX - 12.0,
        };
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());

        // Soft shadow lifts the panel off the untouched document.
        for (offset, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
            painter.fill(
                px - offset,
                py - offset + 2.0,
                panel_w + 2.0 * offset,
                panel_h + 2.0 * offset,
                RADIUS + offset,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: alpha,
                },
            );
        }
        painter.fill(px, py, panel_w, panel_h, RADIUS, theme.ui.overlay_bg);

        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = -(self.scroll - first as f32 * ROW_H);
        let mut slot = 0usize;
        loop {
            let index = first + slot;
            let ry = list_top + offset + slot as f32 * ROW_H;
            if index >= self.rows.len() || ry > list_bottom {
                break;
            }
            slot += 1;
            let row = &self.rows[index];
            let row_bg = if index == self.selected {
                painter.fill(
                    px + PAD / 2.0,
                    ry,
                    panel_w - PAD,
                    ROW_H - 4.0,
                    6.0,
                    theme.ui.overlay_highlight,
                );
                theme.ui.overlay_highlight
            } else {
                theme.ui.overlay_bg
            };

            if let Some((bg, h1)) = row.swatches {
                let sy = ry + (ROW_H - 4.0 - SWATCH) / 2.0;
                painter.fill(px + PAD, sy, SWATCH, SWATCH, 4.0, bg);
                painter.stroke(
                    px + PAD,
                    sy,
                    SWATCH,
                    SWATCH,
                    4.0,
                    1.0,
                    theme.blocks.table_border,
                );
                painter.fill(px + PAD + SWATCH + 6.0, sy, SWATCH, SWATCH, 4.0, h1);
            }

            match &self.renaming {
                Some((rename_index, field)) if *rename_index == index => {
                    let buffer = field.text();
                    let field_w = self.geometry.edit_x - self.geometry.name_x - 10.0;
                    let valid = row
                        .path
                        .parent()
                        .and_then(|dir| rename_target(dir, buffer))
                        .is_some()
                        || buffer.trim() == row.name;
                    let border = if valid {
                        theme.text.link
                    } else {
                        theme.alerts.caution
                    };
                    painter.fill(
                        self.geometry.name_x - 5.0,
                        ry + 2.0,
                        field_w,
                        ROW_H - 8.0,
                        4.0,
                        theme.ui.overlay_bg,
                    );
                    painter.stroke(
                        self.geometry.name_x - 5.0,
                        ry + 2.0,
                        field_w,
                        ROW_H - 8.0,
                        4.0,
                        1.0,
                        border,
                    );
                    if let Some(range) = field.selection() {
                        let from = painter.measure(&buffer[..range.start], BODY_FAMILY, 15.0, 400);
                        let to = painter.measure(&buffer[..range.end], BODY_FAMILY, 15.0, 400);
                        painter.fill(
                            self.geometry.name_x + from,
                            ry + 7.0,
                            to - from,
                            17.0,
                            0.0,
                            theme.ui.selection_bg,
                        );
                    }
                    painter.text(
                        self.geometry.name_x,
                        ry + 6.0,
                        buffer,
                        BODY_FAMILY,
                        15.0,
                        400,
                        theme.ui.overlay_fg,
                    );
                    let caret = field.caret_offset(|s| painter.measure(s, BODY_FAMILY, 15.0, 400));
                    painter.fill(
                        self.geometry.name_x + caret + 1.5,
                        ry + 7.0,
                        1.5,
                        17.0,
                        0.0,
                        theme.ui.overlay_fg,
                    );
                    rename_box = Some((
                        field.offsets(|s| painter.measure(s, BODY_FAMILY, 15.0, 400)),
                        (self.geometry.name_x - 5.0, ry + 2.0, field_w, ROW_H - 8.0),
                    ));
                }
                _ => {
                    painter.text(
                        self.geometry.name_x,
                        ry + 6.0,
                        &row.name,
                        BODY_FAMILY,
                        15.0,
                        if index == self.selected { 700 } else { 400 },
                        theme.ui.overlay_fg,
                    );
                }
            }

            // Pencil icon: diagonal shaft with a nib dot.
            let icon_y = ry + (ROW_H - 4.0 - 13.0) / 2.0;
            let pen_x = self.geometry.edit_x + 4.0;
            painter.line(
                pen_x + 3.0,
                icon_y + 10.0,
                pen_x + 10.0,
                icon_y + 3.0,
                2.2,
                theme.ui.overlay_fg,
            );
            painter.fill(
                pen_x + 0.5,
                icon_y + 11.0,
                2.5,
                2.5,
                1.0,
                theme.ui.overlay_fg,
            );

            // Duplicate icon: two offset squares, front one masking the back.
            let dup_x = self.geometry.duplicate_x + 4.0;
            painter.stroke(dup_x + 4.0, icon_y, 9.0, 9.0, 2.0, 1.2, theme.ui.overlay_fg);
            painter.fill(dup_x, icon_y + 4.0, 9.0, 9.0, 2.0, row_bg);
            painter.stroke(dup_x, icon_y + 4.0, 9.0, 9.0, 2.0, 1.2, theme.ui.overlay_fg);

            let pending = self.pending_delete == Some(index);
            let cross_w = painter.measure("\u{00D7}", BODY_FAMILY, 19.0, 400);
            painter.text(
                self.geometry.delete_x + (ICON_BOX - cross_w) / 2.0,
                ry + 4.0,
                "\u{00D7}",
                BODY_FAMILY,
                19.0,
                if pending { 700 } else { 400 },
                if pending {
                    theme.alerts.caution
                } else {
                    theme.ui.overlay_fg
                },
            );
        }

        // Masks cover row overflow above and below the list viewport.
        painter.fill(px, py, panel_w, list_top - py, RADIUS, theme.ui.overlay_bg);
        painter.fill(
            px,
            list_bottom,
            panel_w,
            panel_h - (list_bottom - py),
            RADIUS,
            theme.ui.overlay_bg,
        );

        let title = "Themes";
        let title_w = painter.measure(title, BODY_FAMILY, 17.0, 700);
        painter.text(
            px + (panel_w - title_w) / 2.0,
            py + 13.0,
            title,
            BODY_FAMILY,
            17.0,
            700,
            theme.ui.overlay_fg,
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
        let (offsets, rect) = rename_box.unwrap_or_default();
        self.rename_offsets = offsets;
        self.rename_rect = rect;
    }

    fn key(&mut self, key: &Key, ctrl: bool, shift: bool) -> OverlayResult {
        if self.renaming.is_some() {
            return self.rename_key(key, ctrl, shift);
        }
        match key {
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            Key::Named(NamedKey::ArrowDown) => {
                if self.selected + 1 < self.rows.len() {
                    self.select(self.selected + 1);
                    return self.preview();
                }
            }
            Key::Named(NamedKey::ArrowUp) => {
                if self.selected > 0 {
                    self.select(self.selected - 1);
                    return self.preview();
                }
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(row) = self.rows.get(self.selected) {
                    // The confirm is the reader's last word here: apply,
                    // persist, and close in the one keypress. The click
                    // path keeps the browser open for comparing.
                    return OverlayResult::ApplyAndClose(Action::SetTheme(row.path.clone()));
                }
            }
            _ => {}
        }
        OverlayResult::Open
    }

    fn click(&mut self, x: f32, y: f32) -> OverlayResult {
        if self.renaming.is_some() {
            if inside(self.rename_rect, x, y) {
                let text_x = self.rename_rect.0 + 5.0;
                let offsets = std::mem::take(&mut self.rename_offsets);
                if let Some((_, field)) = self.renaming.as_mut() {
                    field.click(x - text_x, &offsets, Instant::now());
                }
                self.rename_offsets = offsets;
                return OverlayResult::Open;
            }
            // A click away commits when valid, otherwise abandons the edit.
            if let Commit::Renamed(from, to) = self.commit_rename() {
                return OverlayResult::Apply(Action::RenamedTheme { from, to });
            }
            self.renaming = None;
            return OverlayResult::Open;
        }
        let (px, py, pw, ph) = self.geometry.panel;
        if x < px || x > px + pw || y < py || y > py + ph {
            return OverlayResult::Close;
        }
        if y < py + HEADER_H {
            self.moving = true;
            self.grab = (x - px, y - py);
            return OverlayResult::Open;
        }
        if y < self.geometry.list_top || y > self.geometry.list_top + self.geometry.list_h {
            return OverlayResult::Open;
        }
        let index = ((y - self.geometry.list_top + self.scroll) / ROW_H).floor() as usize;
        if index >= self.rows.len() {
            return OverlayResult::Open;
        }
        if x >= self.geometry.delete_x {
            if self.pending_delete == Some(index) {
                self.delete(index);
            } else {
                self.pending_delete = Some(index);
            }
            return OverlayResult::Open;
        }
        if x >= self.geometry.duplicate_x {
            self.duplicate(index);
            return OverlayResult::Open;
        }
        if x >= self.geometry.edit_x {
            return OverlayResult::Apply(Action::EditTheme(self.rows[index].path.clone()));
        }
        // Double click on the name starts an inline rename.
        let now = Instant::now();
        if let Some((last_index, at)) = self.last_name_click {
            if last_index == index && now.duration_since(at) < DOUBLE_CLICK {
                self.last_name_click = None;
                let name = self.rows[index].name.clone();
                self.start_rename(index, name);
                return OverlayResult::Open;
            }
        }
        self.last_name_click = Some((index, now));
        self.select(index);
        if let Some(row) = self.rows.get(index) {
            return OverlayResult::Apply(Action::SetTheme(row.path.clone()));
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
        self.scroll = (self.scroll + lines * ROW_H).clamp(0.0, self.max_scroll());
        OverlayResult::Open
    }
}

/// Target file for renaming a theme, None when the trimmed name is empty,
/// contains a path separator, or is already taken in the directory.
pub fn rename_target(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }
    let target = dir.join(format!("{name}.toml"));
    (!target.exists()).then_some(target)
}

/// Path for a duplicate of a theme file: `<stem>-copy.toml`, counting up
/// while the name is taken.
pub fn duplicate_path(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("theme");
    let first = dir.join(format!("{stem}-copy.toml"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}-copy-{n}.toml")))
        .find(|p| !p.exists())
        .expect("some copy name is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-dup-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn light_themes_sort_before_dark_ones() {
        let dir = temp_dir("shelves");
        std::fs::write(
            dir.join("alpha-dark.toml"),
            "[surface]\nbackground = \"#282a36\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("zulu-light.toml"),
            "[surface]\nbackground = \"#fdf6e3\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("beta-light.toml"),
            "[surface]\nbackground = \"#ffffff\"\n",
        )
        .unwrap();
        let browser = ThemeBrowser::new(vec![dir.clone()], "alpha-dark");
        let names: Vec<&str> = browser.rows.iter().map(|r| r.name.as_str()).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["beta-light", "zulu-light", "alpha-dark"]);
    }

    #[test]
    fn stepping_with_the_keyboard_previews_the_highlighted_theme() {
        let dir = temp_dir("preview");
        std::fs::write(
            dir.join("alpha-dark.toml"),
            "[surface]\nbackground = \"#282a36\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("beta-light.toml"),
            "[surface]\nbackground = \"#ffffff\"\n",
        )
        .unwrap();
        let mut browser = ThemeBrowser::new(vec![dir.clone()], "beta-light");
        let stepped = browser.key(&Key::Named(NamedKey::ArrowDown), false, false);
        let pinned = browser.key(&Key::Named(NamedKey::ArrowDown), false, false);
        std::fs::remove_dir_all(&dir).unwrap();
        match stepped {
            OverlayResult::Apply(Action::PreviewTheme(theme)) => {
                assert_eq!(
                    theme.surface.background,
                    theme::parse_hex("#282a36").unwrap(),
                    "the highlighted theme is the one previewed"
                );
            }
            _ => panic!("stepping previews live"),
        }
        assert!(
            matches!(pinned, OverlayResult::Open),
            "a step against the end moves nothing and previews nothing"
        );
    }

    #[test]
    fn enter_confirms_and_closes() {
        let dir = temp_dir("confirm");
        std::fs::write(
            dir.join("alpha-dark.toml"),
            "[surface]\nbackground = \"#282a36\"\n",
        )
        .unwrap();
        let mut browser = ThemeBrowser::new(vec![dir.clone()], "alpha-dark");
        let result = browser.key(&Key::Named(NamedKey::Enter), false, false);
        std::fs::remove_dir_all(&dir).unwrap();
        match result {
            OverlayResult::ApplyAndClose(Action::SetTheme(path)) => {
                assert_eq!(path.file_name().unwrap(), "alpha-dark.toml");
            }
            _ => panic!("enter chooses the theme and closes the browser"),
        }
    }

    #[test]
    fn rename_target_accepts_free_simple_name() {
        let dir = temp_dir("rename");
        std::fs::write(dir.join("old.toml"), "").unwrap();
        let target = rename_target(&dir, "fresh name").unwrap();
        assert_eq!(target.file_name().unwrap(), "fresh name.toml");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_target_rejects_empty_separators_and_taken() {
        let dir = temp_dir("reject");
        std::fs::write(dir.join("taken.toml"), "").unwrap();
        assert!(rename_target(&dir, "").is_none());
        assert!(rename_target(&dir, "   ").is_none());
        assert!(rename_target(&dir, "a/b").is_none());
        assert!(rename_target(&dir, "a\\b").is_none());
        assert!(rename_target(&dir, "taken").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn duplicate_derives_copy_name() {
        let dir = temp_dir("first");
        let original = dir.join("nord.toml");
        std::fs::write(&original, "").unwrap();
        let dup = duplicate_path(&original);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(dup.file_name().unwrap(), "nord-copy.toml");
    }

    #[test]
    fn duplicate_counts_past_taken_names() {
        let dir = temp_dir("taken");
        std::fs::write(dir.join("nord.toml"), "").unwrap();
        std::fs::write(dir.join("nord-copy.toml"), "").unwrap();
        std::fs::write(dir.join("nord-copy-2.toml"), "").unwrap();
        let dup = duplicate_path(&dir.join("nord.toml"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(dup.file_name().unwrap(), "nord-copy-3.toml");
    }
}
