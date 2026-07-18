//! Theme browser overlay: the scanned collection with preview swatches,
//! keyboard navigation, and per-row duplicate and delete actions.

use std::path::{Path, PathBuf};

use winit::keyboard::{Key, NamedKey};

use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{self, Rgba, Theme};
use crate::ui::overlay::{Action, Overlay, OverlayResult};

const ROW_H: f32 = 36.0;
const PAD: f32 = 12.0;
const HEADER_H: f32 = 46.0;
const PANEL_W: f32 = 400.0;
const SWATCH: f32 = 16.0;
const ICON_BOX: f32 = 22.0;

struct Row {
    name: String,
    path: PathBuf,
    swatches: Option<(Rgba, Rgba)>,
}

/// Panel geometry cached at draw time for click hit testing.
#[derive(Default, Clone, Copy)]
struct Geometry {
    panel: (f32, f32, f32, f32),
    list_top: f32,
    visible: usize,
    delete_x: f32,
    duplicate_x: f32,
}

pub struct ThemeBrowser {
    dirs: Vec<PathBuf>,
    rows: Vec<Row>,
    selected: usize,
    top: usize,
    pending_delete: Option<usize>,
    geometry: Geometry,
}

impl ThemeBrowser {
    pub fn new(dirs: Vec<PathBuf>, active: &str) -> ThemeBrowser {
        let mut browser = ThemeBrowser {
            dirs,
            rows: Vec::new(),
            selected: 0,
            top: 0,
            pending_delete: None,
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
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
        self.pending_delete = None;
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows.len().saturating_sub(1));
        self.pending_delete = None;
        self.scroll_into_view();
    }

    fn scroll_into_view(&mut self) {
        let visible = self.geometry.visible.max(1);
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
        }
    }

    fn select_by_name(&mut self, name: &str) {
        if let Some(index) = self.rows.iter().position(|r| r.name == name) {
            self.select(index);
        }
    }

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
}

impl Overlay for ThemeBrowser {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        // Dim the document so the panel reads as modal.
        painter.fill(
            0.0,
            0.0,
            w,
            h,
            0.0,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 90,
            },
        );

        let panel_w = PANEL_W.min(w - 40.0);
        let max_h = (h * 0.8).max(ROW_H + HEADER_H + 2.0 * PAD);
        let want_h = HEADER_H + PAD + self.rows.len() as f32 * ROW_H + PAD;
        let panel_h = want_h.min(max_h);
        let visible = (((panel_h - HEADER_H - 2.0 * PAD) / ROW_H).floor() as usize).max(1);
        let px = ((w - panel_w) / 2.0).floor();
        let py = ((h - panel_h) / 2.0).floor();

        painter.fill(px, py, panel_w, panel_h, 8.0, theme.ui.overlay_bg);
        painter.stroke(
            px,
            py,
            panel_w,
            panel_h,
            8.0,
            1.0,
            theme.blocks.table_border,
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

        let list_top = py + HEADER_H + PAD;
        self.geometry = Geometry {
            panel: (px, py, panel_w, panel_h),
            list_top,
            visible,
            delete_x: px + panel_w - PAD - ICON_BOX,
            duplicate_x: px + panel_w - PAD - 2.0 * ICON_BOX - 6.0,
        };
        if self.top + visible > self.rows.len() {
            self.top = self.rows.len().saturating_sub(visible);
        }

        for (slot, index) in (self.top..self.rows.len().min(self.top + visible)).enumerate() {
            let row = &self.rows[index];
            let ry = list_top + slot as f32 * ROW_H;
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
                painter.fill(
                    px + PAD,
                    ry + (ROW_H - 4.0 - SWATCH) / 2.0,
                    SWATCH,
                    SWATCH,
                    4.0,
                    bg,
                );
                painter.stroke(
                    px + PAD,
                    ry + (ROW_H - 4.0 - SWATCH) / 2.0,
                    SWATCH,
                    SWATCH,
                    4.0,
                    1.0,
                    theme.blocks.table_border,
                );
                painter.fill(
                    px + PAD + SWATCH + 6.0,
                    ry + (ROW_H - 4.0 - SWATCH) / 2.0,
                    SWATCH,
                    SWATCH,
                    4.0,
                    h1,
                );
            }

            painter.text(
                px + PAD + 2.0 * SWATCH + 16.0,
                ry + 6.0,
                &row.name,
                BODY_FAMILY,
                15.0,
                if index == self.selected { 700 } else { 400 },
                theme.ui.overlay_fg,
            );

            // Duplicate icon: two offset squares, front one masking the back.
            let icon_y = ry + (ROW_H - 4.0 - 13.0) / 2.0;
            let dup_x = self.geometry.duplicate_x + 4.0;
            painter.stroke(dup_x + 4.0, icon_y, 9.0, 9.0, 2.0, 1.2, theme.ui.overlay_fg);
            painter.fill(dup_x, icon_y + 4.0, 9.0, 9.0, 2.0, row_bg);
            painter.stroke(dup_x, icon_y + 4.0, 9.0, 9.0, 2.0, 1.2, theme.ui.overlay_fg);

            let delete_color = if self.pending_delete == Some(index) {
                theme.alerts.caution
            } else {
                theme.ui.overlay_fg
            };
            let cross_w = painter.measure("\u{00D7}", BODY_FAMILY, 19.0, 400);
            painter.text(
                self.geometry.delete_x + (ICON_BOX - cross_w) / 2.0,
                ry + 4.0,
                "\u{00D7}",
                BODY_FAMILY,
                19.0,
                if self.pending_delete == Some(index) {
                    700
                } else {
                    400
                },
                delete_color,
            );
        }
    }

    fn key(&mut self, key: &Key) -> OverlayResult {
        match key {
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            Key::Named(NamedKey::ArrowDown) => {
                if self.selected + 1 < self.rows.len() {
                    self.select(self.selected + 1);
                }
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.select(self.selected.saturating_sub(1));
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(row) = self.rows.get(self.selected) {
                    return OverlayResult::Apply(Action::SetTheme(row.path.clone()));
                }
            }
            _ => {}
        }
        OverlayResult::Open
    }

    fn click(&mut self, x: f32, y: f32) -> OverlayResult {
        let (px, py, pw, ph) = self.geometry.panel;
        if x < px || x > px + pw || y < py || y > py + ph {
            return OverlayResult::Close;
        }
        let slot = ((y - self.geometry.list_top) / ROW_H).floor();
        if slot < 0.0 || y < self.geometry.list_top {
            return OverlayResult::Open;
        }
        let index = self.top + slot as usize;
        if index >= self.rows.len() || slot as usize >= self.geometry.visible {
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
        self.select(index);
        if let Some(row) = self.rows.get(index) {
            return OverlayResult::Apply(Action::SetTheme(row.path.clone()));
        }
        OverlayResult::Open
    }

    fn scroll(&mut self, lines: f32) -> OverlayResult {
        let max_top = self.rows.len().saturating_sub(self.geometry.visible.max(1));
        let next = self.top as f32 + lines;
        self.top = next.clamp(0.0, max_top as f32) as usize;
        OverlayResult::Open
    }
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
