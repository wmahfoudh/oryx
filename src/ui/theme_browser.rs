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
use crate::ui::overlay::{self, inside, Action, Overlay, OverlayResult, PanelDrag};
use crate::ui::scrollbar;
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
/// The scrollbar thumb: its width and its distance from the panel's
/// right edge, centered in the pad beside the rows.
const THUMB_W: f32 = 6.0;
const THUMB_INSET: f32 = 3.0;

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
    /// The user's themes folder, where a duplicate is written; None on
    /// a run with no home, where the copy goes beside its source.
    user_dir: Option<PathBuf>,
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
    drag: PanelDrag,
    geometry: Geometry,
    /// The cursor's offset from the thumb's top while the thumb is held.
    thumb_grab: Option<f32>,
    /// Whether the first draw has seated the list on the current theme;
    /// the panel learns its height only there.
    seated: bool,
}

impl ThemeBrowser {
    pub fn new(dirs: Vec<PathBuf>, user_dir: Option<PathBuf>, active: &str) -> ThemeBrowser {
        let mut browser = ThemeBrowser {
            dirs,
            user_dir,
            rows: Vec::new(),
            selected: 0,
            scroll: 0.0,
            pending_delete: None,
            renaming: None,
            rename_offsets: Vec::new(),
            rename_rect: (0.0, 0.0, 0.0, 0.0),
            clipboard: None,
            last_name_click: None,
            drag: PanelDrag::default(),
            geometry: Geometry::default(),
            thumb_grab: None,
            seated: false,
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

    /// The thumb as `(y, height)` from the list's top, or None when the
    /// list fits its viewport.
    fn thumb(&self) -> Option<(f32, f32)> {
        let list_h = self.geometry.list_h;
        scrollbar::thumb(
            self.rows.len() as f32 * ROW_H,
            list_h,
            self.scroll,
            list_h,
            1.0,
        )
    }

    /// Moves the list so the held thumb follows the cursor at `y`.
    fn drag_thumb(&mut self, y: f32) {
        let (Some(grab), Some((_, th))) = (self.thumb_grab, self.thumb()) else {
            return;
        };
        let list_h = self.geometry.list_h;
        self.scroll = scrollbar::scroll_for_thumb(
            y - self.geometry.list_top - grab,
            th,
            list_h,
            self.rows.len() as f32 * ROW_H,
            list_h,
        );
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows.len().saturating_sub(1));
        self.pending_delete = None;
        self.scroll =
            overlay::scroll_into_view(self.selected, ROW_H, self.scroll, self.geometry.list_h);
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

    /// Copies the row's file into the user's themes folder, never
    /// beside the source, read-only on a package install, and opens
    /// the copy for renaming. A copy that cannot be written is a
    /// notice with the reason.
    fn duplicate(&mut self, index: usize) -> OverlayResult {
        let Some(row) = self.rows.get(index) else {
            return OverlayResult::Open;
        };
        let dir = match &self.user_dir {
            Some(dir) => dir.clone(),
            None => row.path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        };
        let target = duplicate_path(&dir, &row.name, |name| {
            self.rows.iter().any(|r| r.name == name)
        });
        let name = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let copied = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(&row.path, &target));
        if let Err(err) = copied {
            return OverlayResult::Apply(Action::Notice(format!(
                "Cannot save the copy of the theme in {}: {err}",
                dir.display()
            )));
        }
        self.rescan();
        self.select_by_name(&name);
        let index = self.selected;
        self.start_rename(index, name);
        OverlayResult::Open
    }

    /// A shipped theme keeps its file and its name: the editor saves
    /// its changes in a copy, and the browser offers the copy only.
    fn shipped(&self, index: usize) -> bool {
        self.rows
            .get(index)
            .is_some_and(|row| theme::is_bundled(&row.name))
    }

    fn delete(&mut self, index: usize) {
        if self.shipped(index) {
            return;
        }
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
    fn field_has_keys(&self) -> bool {
        self.renaming.is_some()
    }

    fn draw(&mut self, painter: &mut Painter, theme: &Theme, caret_lit: bool) {
        let mut rename_box: Option<RenameBox> = None;
        let (w, h) = (painter.width(), painter.height());
        let panel_w = PANEL_W.min(w - 40.0);
        let max_h = (h * 0.8).max(ROW_H + HEADER_H + 2.0 * PAD);
        let want_h = HEADER_H + PAD + self.rows.len() as f32 * ROW_H + PAD;
        let panel_h = want_h.min(max_h);
        let center = (((w - panel_w) / 2.0).floor(), ((h - panel_h) / 2.0).floor());
        let (px, py) = self.drag.place(center, panel_w, w, h);
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
        // The first draw seats the list on the current theme, centered
        // so its neighbors show; the arrows keep their manner after.
        if !self.seated {
            self.scroll = self.selected as f32 * ROW_H - (list_h - ROW_H) / 2.0;
            self.seated = true;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());

        // Soft shadow lifts the panel off the untouched document.
        overlay::panel_shadow(painter, px, py, panel_w, panel_h, RADIUS);
        painter.fill(px, py, panel_w, panel_h, RADIUS, theme.ui.overlay_bg);

        // The rows are drawn under a clip to the list's viewport: a row
        // cut by either edge ends there, never over the header or past
        // the panel's bottom pad.
        painter.clip(Some((px, list_top, panel_w, list_h)));
        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = -(self.scroll - first as f32 * ROW_H);
        let mut slot = 0usize;
        loop {
            let index = first + slot;
            let ry = list_top + offset + slot as f32 * ROW_H;
            if index >= self.rows.len() || ry >= list_bottom {
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
                    theme.ui.selection_bg,
                );
                theme.ui.selection_bg
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
                    let positions = painter.boundaries(buffer, BODY_FAMILY, 15.0, 400);
                    if let Some(range) = field.selection() {
                        let from = field.offset_of(range.start, &positions);
                        let to = field.offset_of(range.end, &positions);
                        // On right-to-left text the end stands left of the start.
                        let (from, to) = (from.min(to), from.max(to));
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
                    if caret_lit {
                        let caret = field.caret_offset(&positions);
                        painter.fill(
                            self.geometry.name_x + caret + 1.5,
                            ry + 7.0,
                            1.5,
                            17.0,
                            0.0,
                            theme.ui.overlay_fg,
                        );
                    }
                    rename_box = Some((
                        positions,
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

            if self.shipped(index) {
                continue;
            }
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

        painter.clip(None);
        if let Some((ty, th)) = self.thumb() {
            let color = if self.thumb_grab.is_some() {
                theme.ui.scrollbar_hover
            } else {
                theme.ui.scrollbar
            };
            painter.fill(
                px + panel_w - THUMB_INSET - THUMB_W,
                list_top + ty,
                THUMB_W,
                th,
                THUMB_W / 2.0,
                color,
            );
        }
        overlay::panel_header(painter, px, py, panel_w, HEADER_H, RADIUS, theme);

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
            self.drag.press(x, y, px, py);
            return OverlayResult::Open;
        }
        if y < self.geometry.list_top || y > self.geometry.list_top + self.geometry.list_h {
            return OverlayResult::Open;
        }
        // The pad beside the rows is the bar: a press on the thumb keeps
        // the cursor's offset, a press on the track jumps the thumb to
        // center on the cursor; neither reaches the delete icon.
        if x >= px + pw - PAD {
            if let Some((ty, th)) = self.thumb() {
                let at = y - self.geometry.list_top;
                let grab = if at >= ty && at < ty + th {
                    at - ty
                } else {
                    th / 2.0
                };
                self.thumb_grab = Some(grab);
                self.drag_thumb(y);
            }
            return OverlayResult::Open;
        }
        let index = ((y - self.geometry.list_top + self.scroll) / ROW_H).floor() as usize;
        if index >= self.rows.len() {
            return OverlayResult::Open;
        }
        if x >= self.geometry.delete_x {
            if self.shipped(index) {
                return OverlayResult::Open;
            }
            if self.pending_delete == Some(index) {
                self.delete(index);
            } else {
                self.pending_delete = Some(index);
            }
            return OverlayResult::Open;
        }
        if x >= self.geometry.duplicate_x {
            return self.duplicate(index);
        }
        if x >= self.geometry.edit_x {
            return OverlayResult::Apply(Action::EditTheme(self.rows[index].path.clone()));
        }
        // Double click on the name starts an inline rename.
        let now = Instant::now();
        if let Some((last_index, at)) = self.last_name_click {
            if last_index == index && now.duration_since(at) < DOUBLE_CLICK && !self.shipped(index)
            {
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
        if self.thumb_grab.is_some() {
            self.drag_thumb(y);
        } else {
            self.drag.to(x, y, self.geometry.center);
        }
        OverlayResult::Open
    }

    fn release(&mut self) {
        self.thumb_grab = None;
        self.drag.release();
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

/// The file of a copy of a theme in `dir`: `<stem>-copy.toml`, counting
/// up while the name is a file of `dir` or `taken` says the list shows
/// it, a file of another folder that would hide the copy.
pub fn duplicate_path(dir: &Path, stem: &str, taken: impl Fn(&str) -> bool) -> PathBuf {
    let free = |name: &str| !dir.join(format!("{name}.toml")).exists() && !taken(name);
    let first = format!("{stem}-copy");
    let name = if free(&first) {
        first
    } else {
        (2..)
            .map(|n| format!("{stem}-copy-{n}"))
            .find(|name| free(name))
            .expect("some copy name is free")
    };
    dir.join(format!("{name}.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::painter::Painter;
    use crate::style::fonts::FontStore;
    use tiny_skia::Pixmap;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-dup-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The rows are drawn under a clip to the list's viewport, so a
    /// row cut by the list's bottom ends inside the panel and never
    /// shows over the document below it.
    #[test]
    fn a_partly_visible_row_never_shows_below_the_panel() {
        let dir = temp_dir("spill");
        for n in 0..40 {
            std::fs::write(
                dir.join(format!("theme-{n:02}.toml")),
                "[surface]\nbackground = \"#282a36\"\n",
            )
            .unwrap();
        }
        let mut browser = ThemeBrowser::new(vec![dir.clone()], None, "theme-00");
        std::fs::remove_dir_all(&dir).unwrap();
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let (w, h) = (460, 320);
        // Scrolled to the end, the last row sits flush with the list's
        // bottom: nothing reaches below the panel but its shadow.
        let mut aligned = Pixmap::new(w, h).unwrap();
        browser.scroll = f32::MAX;
        browser.draw(
            &mut Painter::new(&mut aligned, &mut fonts, None, 1.0),
            &theme,
            true,
        );
        let mut spilling = Pixmap::new(w, h).unwrap();
        browser.scroll = 0.0;
        browser.draw(
            &mut Painter::new(&mut spilling, &mut fonts, None, 1.0),
            &theme,
            true,
        );
        let (_, py, _, ph) = browser.geometry.panel;
        let overflow = ROW_H - browser.geometry.list_h % ROW_H;
        assert!(
            overflow > PAD,
            "at scroll 0 the last drawn row reaches {overflow} below the list, past the panel's pad"
        );
        let bottom = (py + ph) as u32;
        for y in bottom + 1..(bottom + ROW_H as u32).min(h) {
            for x in 0..w {
                assert_eq!(
                    aligned.pixel(x, y),
                    spilling.pixel(x, y),
                    "pixel ({x}, {y}) below the panel changed with the scroll"
                );
            }
        }
    }

    /// The rename field has the keys from its opening to its close, so
    /// its caret blinks; the list alone has no caret.
    #[test]
    fn the_rename_field_has_the_keys_while_it_is_open() {
        let dir = temp_dir("rename-keys");
        std::fs::write(
            dir.join("mine.toml"),
            "[surface]\nbackground = \"#282a36\"\n",
        )
        .unwrap();
        let mut browser = ThemeBrowser::new(vec![dir.clone()], None, "mine");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!browser.field_has_keys(), "the list alone");
        browser.start_rename(0, "mine".to_string());
        assert!(browser.field_has_keys(), "the rename field open");
        browser.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(!browser.field_has_keys(), "the rename field closed");
    }

    /// Forty themes in a folder, the browser open on one of them.
    fn long_browser(name: &str, active: &str) -> ThemeBrowser {
        let dir = temp_dir(name);
        for n in 0..40 {
            std::fs::write(
                dir.join(format!("theme-{n:02}.toml")),
                "[surface]\nbackground = \"#282a36\"\n",
            )
            .unwrap();
        }
        let browser = ThemeBrowser::new(vec![dir.clone()], None, active);
        std::fs::remove_dir_all(&dir).unwrap();
        browser
    }

    fn drawn(browser: &mut ThemeBrowser, fonts: &mut FontStore, w: u32, h: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let mut painter = Painter::new(&mut pixmap, fonts, None, 1.0);
        browser.draw(&mut painter, &Theme::default_dark(), true);
        pixmap
    }

    /// The editor saves a copy of a shipped theme and leaves its file
    /// alone. The browser follows the same rule: the cross and the
    /// rename work on the themes of the user only.
    #[test]
    fn a_shipped_theme_is_not_deleted_or_renamed_from_the_browser() {
        let dir = temp_dir("shipped");
        for name in ["dracula", "mine"] {
            std::fs::write(
                dir.join(format!("{name}.toml")),
                "[surface]\nbackground = \"#282a36\"\n",
            )
            .unwrap();
        }
        let mut fonts = FontStore::new();
        let mut browser = ThemeBrowser::new(vec![dir.clone()], None, "dracula");
        drawn(&mut browser, &mut fonts, 460, 320);
        let row_y = |browser: &ThemeBrowser, name: &str| {
            let index = browser.rows.iter().position(|r| r.name == name).unwrap();
            browser.geometry.list_top - browser.scroll + index as f32 * ROW_H + ROW_H / 2.0
        };
        let cross = browser.geometry.delete_x + 2.0;
        let name_x = browser.geometry.name_x + 4.0;

        let y = row_y(&browser, "dracula");
        browser.click(cross, y);
        assert_eq!(
            browser.pending_delete, None,
            "the cross of a shipped theme arms nothing"
        );
        browser.click(cross, y);
        assert!(dir.join("dracula.toml").exists(), "the shipped file stays");
        browser.click(name_x, y);
        browser.click(name_x, y);
        assert!(
            browser.renaming.is_none(),
            "a double click on a shipped name opens no rename"
        );

        let y = row_y(&browser, "mine");
        browser.click(name_x, y);
        browser.click(name_x, y);
        assert!(browser.renaming.is_some(), "a theme of the user is renamed");
        browser.renaming = None;
        browser.click(cross, y);
        browser.click(cross, y);
        assert!(
            !dir.join("mine.toml").exists(),
            "a theme of the user is deleted"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_browser_opens_with_the_current_theme_centered() {
        let mut fonts = FontStore::new();
        let mut browser = long_browser("seat", "theme-30");
        assert_eq!(browser.selected, 30);
        drawn(&mut browser, &mut fonts, 460, 320);
        let list_h = browser.geometry.list_h;
        let centered = 30.0 * ROW_H - (list_h - ROW_H) / 2.0;
        assert!(
            (browser.scroll - centered).abs() < 0.5,
            "the current row sits in the middle: {} against {centered}",
            browser.scroll
        );
        // The arrows keep their manner: a step down moves the list only
        // when the row leaves the view.
        browser.key(&Key::Named(NamedKey::ArrowDown), false, false);
        assert!(
            (browser.scroll - centered).abs() < 0.5,
            "a step inside the view moves nothing"
        );
        // A theme on the first page opens at the top, one at the end
        // at the end: the seat is clamped like any scroll.
        let mut first = long_browser("seat-first", "theme-01");
        drawn(&mut first, &mut fonts, 460, 320);
        assert_eq!(first.scroll, 0.0);
        let mut last = long_browser("seat-last", "theme-39");
        drawn(&mut last, &mut fonts, 460, 320);
        assert_eq!(last.scroll, last.max_scroll());
    }

    #[test]
    fn the_bar_scrolls_the_list_and_never_arms_a_delete() {
        let mut fonts = FontStore::new();
        let mut browser = long_browser("bar", "theme-00");
        let pixmap = drawn(&mut browser, &mut fonts, 460, 320);
        let (px, _, pw, _) = browser.geometry.panel;
        let list_top = browser.geometry.list_top;
        let list_h = browser.geometry.list_h;
        let (ty, th) = browser.thumb().expect("a long list shows a thumb");
        assert_eq!(ty, 0.0, "at the top the thumb sits at the top");
        let sb = Theme::default_dark().ui.scrollbar;
        let p = pixmap
            .pixel(
                (px + pw - THUMB_INSET - THUMB_W / 2.0) as u32,
                (list_top + th / 2.0) as u32,
            )
            .unwrap();
        assert_eq!(
            (p.red(), p.green(), p.blue()),
            (sb.r, sb.g, sb.b),
            "the thumb in the scrollbar color"
        );
        // A press on the track's bottom jumps there, takes the thumb and arms nothing.
        browser.click(px + pw - 6.0, list_top + list_h - 2.0);
        assert!(browser.scroll > 0.0, "the list jumped");
        assert!(browser.thumb_grab.is_some(), "the thumb is held");
        assert_eq!(
            browser.pending_delete, None,
            "the strip is no delete button"
        );
        let jumped = browser.scroll;
        browser.drag(px + pw - 6.0, list_top + 10.0);
        assert!(browser.scroll < jumped, "dragging up scrolls back");
        browser.release();
        assert!(browser.thumb_grab.is_none());
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
        let browser = ThemeBrowser::new(vec![dir.clone()], None, "alpha-dark");
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
        let mut browser = ThemeBrowser::new(vec![dir.clone()], None, "beta-light");
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
        let mut browser = ThemeBrowser::new(vec![dir.clone()], None, "alpha-dark");
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
        let dup = duplicate_path(&dir, "nord", |_| false);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(dup, dir.join("nord-copy.toml"));
    }

    #[test]
    fn duplicate_counts_past_taken_names() {
        let dir = temp_dir("taken");
        std::fs::write(dir.join("nord-copy.toml"), "").unwrap();
        std::fs::write(dir.join("nord-copy-2.toml"), "").unwrap();
        let dup = duplicate_path(&dir, "nord", |_| false);
        assert_eq!(dup.file_name().unwrap(), "nord-copy-3.toml");
        // A name the list shows from another folder would hide the copy.
        let dup = duplicate_path(&dir, "nord", |name| name == "nord-copy-3");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(dup.file_name().unwrap(), "nord-copy-4.toml");
    }

    /// A duplicate goes into the user's themes folder, never beside the
    /// source: the bundled folder is read-only on a package install
    /// (Task 339, claim 16 of the hunt of 1.3.0). A copy that cannot be
    /// written is a notice. Without a user folder the copy goes beside
    /// the source, as a run with no home did.
    #[cfg(unix)]
    #[test]
    fn a_duplicate_lands_in_the_users_folder_not_beside_the_source() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_dir("user-folder");
        let bundled = root.join("bundled");
        std::fs::create_dir_all(&bundled).unwrap();
        theme::save(&bundled.join("dracula.toml"), &Theme::default_dark()).unwrap();
        std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o555)).unwrap();
        let user = root.join("user/themes");
        let mut browser = ThemeBrowser::new(
            vec![bundled.clone(), user.clone()],
            Some(user.clone()),
            "dracula",
        );
        let result = browser.duplicate(0);
        assert!(matches!(result, OverlayResult::Open), "the copy made");
        assert!(
            user.join("dracula-copy.toml").is_file(),
            "the copy in the user's folder, created on the way"
        );
        assert_eq!(
            std::fs::read_dir(&bundled).unwrap().count(),
            1,
            "nothing beside the source"
        );
        assert!(
            browser.rows.iter().any(|r| r.name == "dracula-copy"),
            "the copy listed"
        );
        assert!(browser.renaming.is_some(), "the copy open for renaming");
        let sealed = root.join("sealed");
        std::fs::create_dir_all(&sealed).unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut browser = ThemeBrowser::new(
            vec![bundled.clone()],
            Some(sealed.join("themes")),
            "dracula",
        );
        let result = browser.duplicate(0);
        assert!(
            matches!(result, OverlayResult::Apply(Action::Notice(ref text)) if text.contains("sealed")),
            "a notice naming the folder"
        );
        assert!(browser.renaming.is_none());
        std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut browser = ThemeBrowser::new(vec![bundled.clone()], None, "dracula");
        browser.duplicate(0);
        assert!(
            bundled.join("dracula-copy.toml").is_file(),
            "no user folder: beside the source"
        );
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
