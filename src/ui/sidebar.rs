//! Folder sidebar: a persistent panel listing the open file's folder as a
//! tree. Directories sort before files, both alphabetical; the listing
//! keeps renderable files, well-known extensionless names, and dot entries
//! (drawn dimmed). Expansion is in place and children are read on demand.

use std::path::{Path, PathBuf};

use crate::doc::load::{self, FileKind};
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};

/// Panel width in pixels while open.
pub const WIDTH: f32 = 260.0;

const ROW_H: f32 = 26.0;
const PAD: f32 = 10.0;
const INDENT: f32 = 14.0;
const TEXT_SIZE: f32 = 13.5;
/// Room the folder icon column takes before a row's name.
const ICON_W: f32 = 16.0;

/// Extensionless files worth listing; they open as plain text.
const KNOWN_NAMES: &[&str] = &["readme", "license", "changelog", "makefile"];

/// One visible row of the tree.
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
    /// Dot entry, rendered dimmed.
    pub hidden: bool,
}

pub struct Sidebar {
    root: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    /// The file currently displayed in the document area.
    current: Option<PathBuf>,
    scroll: f32,
    list_h: f32,
}

/// Whether a directory entry belongs in the tree.
fn recognized(name: &str, is_dir: bool) -> bool {
    if is_dir || name.starts_with('.') {
        return true;
    }
    let path = Path::new(name);
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.eq_ignore_ascii_case("txt") || load::detect(path) != FileKind::Plain,
        None => KNOWN_NAMES.contains(&name.to_ascii_lowercase().as_str()),
    }
}

/// The recognized entries of one directory, directories first, both
/// groups alphabetical and case-insensitive.
fn scan(dir: &Path, depth: usize) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let is_dir = e.file_type().ok()?.is_dir();
            recognized(&name, is_dir).then(|| Entry {
                hidden: name.starts_with('.'),
                path: e.path(),
                is_dir,
                depth,
                expanded: false,
                name,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// The visible rows for a root: a `..` row up front when a parent exists,
/// then the root's own entries.
fn tree(root: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(parent) = root.parent() {
        entries.push(Entry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
            depth: 0,
            expanded: false,
            hidden: false,
        });
    }
    entries.extend(scan(root, 0));
    entries
}

impl Sidebar {
    pub fn new(root: &Path) -> Sidebar {
        Sidebar {
            root: root.to_path_buf(),
            entries: tree(root),
            selected: 0,
            current: None,
            scroll: 0.0,
            list_h: 0.0,
        }
    }

    /// Rebuilds the tree one level up, keeping the folder just left
    /// selected and the displayed file marked.
    fn go_up(&mut self, parent: &Path) {
        let left = self.root.clone();
        self.root = parent.to_path_buf();
        self.entries = tree(parent);
        self.scroll = 0.0;
        self.selected = self
            .entries
            .iter()
            .position(|e| e.path == left)
            .unwrap_or(0);
        self.scroll_to_selection();
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Opens or closes a directory row in place.
    fn toggle_dir(&mut self, index: usize) {
        let (path, depth, expanded) = {
            let e = &self.entries[index];
            (e.path.clone(), e.depth, e.expanded)
        };
        if expanded {
            let end = self.entries[index + 1..]
                .iter()
                .position(|e| e.depth <= depth)
                .map_or(self.entries.len(), |p| index + 1 + p);
            self.entries.drain(index + 1..end);
            if self.selected > index && self.selected < end {
                self.selected = index;
            } else if self.selected >= end {
                self.selected -= end - index - 1;
            }
        } else {
            let children = scan(&path, depth + 1);
            if self.selected > index {
                self.selected += children.len();
            }
            self.entries.splice(index + 1..index + 1, children);
        }
        self.entries[index].expanded = !expanded;
    }

    /// A row was chosen: a file returns its path to open, a directory
    /// toggles its expansion.
    pub fn activate(&mut self, index: usize) -> Option<PathBuf> {
        let entry = self.entries.get(index)?;
        self.selected = index;
        if index == 0 && entry.name == ".." {
            let parent = entry.path.clone();
            self.go_up(&parent);
            None
        } else if entry.is_dir {
            self.toggle_dir(index);
            None
        } else {
            Some(entry.path.clone())
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as i64 - 1;
        self.selected = (self.selected as i64 + delta as i64).clamp(0, max) as usize;
        self.scroll_to_selection();
    }

    /// Activates the selected row.
    pub fn enter(&mut self) -> Option<PathBuf> {
        self.activate(self.selected)
    }

    /// Marks the file shown in the document area.
    pub fn set_current(&mut self, path: &Path) {
        self.current = Some(path.to_path_buf());
        if let Some(index) = self.entries.iter().position(|e| e.path == path) {
            self.selected = index;
        }
    }

    fn max_scroll(&self) -> f32 {
        (self.entries.len() as f32 * ROW_H - self.list_h).max(0.0)
    }

    /// Keeps the selected row inside the viewport.
    fn scroll_to_selection(&mut self) {
        let top = self.selected as f32 * ROW_H;
        let list_h = self.list_h.max(ROW_H);
        if top < self.scroll {
            self.scroll = top;
        } else if top + ROW_H > self.scroll + list_h {
            self.scroll = top + ROW_H - list_h;
        }
    }

    /// Row activation from a click inside the panel.
    pub fn click(&mut self, _x: f32, y: f32) -> Option<PathBuf> {
        let index = ((y - PAD + self.scroll) / ROW_H).floor();
        if index < 0.0 || index as usize >= self.entries.len() {
            return None;
        }
        self.activate(index as usize)
    }

    pub fn wheel(&mut self, lines: f32) {
        self.scroll = (self.scroll + lines * ROW_H).clamp(0.0, self.max_scroll());
    }

    pub fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let h = painter.height();
        let ui = &theme.ui;
        painter.fill(0.0, 0.0, WIDTH, h, 0.0, ui.sidebar_bg);
        painter.line(
            WIDTH - 0.5,
            0.0,
            WIDTH - 0.5,
            h,
            1.0,
            theme.blocks.table_border,
        );
        self.list_h = h - 2.0 * PAD;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = -(self.scroll - first as f32 * ROW_H);
        let mut slot = 0usize;
        loop {
            let index = first + slot;
            let ry = PAD + offset + slot as f32 * ROW_H;
            if index >= self.entries.len() || ry > h - PAD {
                break;
            }
            slot += 1;
            let entry = &self.entries[index];
            if index == self.selected {
                painter.fill(3.0, ry, WIDTH - 8.0, ROW_H - 2.0, 5.0, ui.overlay_highlight);
            }
            let x = PAD + entry.depth as f32 * INDENT;
            let mut color = if entry.is_dir {
                ui.sidebar_dir
            } else {
                ui.sidebar_fg
            };
            if entry.hidden {
                color = dim(color);
            }
            if index == 0 && entry.name == ".." {
                // Up chevron for the parent row.
                let iy = ry + ROW_H / 2.0;
                painter.line(x + 1.0, iy + 2.0, x + 5.5, iy - 2.5, 1.6, color);
                painter.line(x + 5.5, iy - 2.5, x + 10.0, iy + 2.0, 1.6, color);
            } else if entry.is_dir {
                // Folder icon: a tab over a solid body.
                let iy = ry + (ROW_H - 12.0) / 2.0;
                painter.fill(x, iy, 5.5, 3.0, 1.0, color);
                painter.fill(x, iy + 2.5, 11.0, 8.0, 1.5, color);
            }
            let current = self.current.as_deref() == Some(entry.path.as_path());
            let weight = if current { 700 } else { 400 };
            let avail = WIDTH - x - ICON_W - PAD;
            let name = truncated(painter, &entry.name, avail, weight);
            painter.text(
                x + ICON_W,
                ry + 5.0,
                &name,
                BODY_FAMILY,
                TEXT_SIZE,
                weight,
                color,
            );
        }
    }
}

/// Dot entries render at reduced opacity.
fn dim(color: Rgba) -> Rgba {
    Rgba {
        a: (color.a as f32 * 0.55) as u8,
        ..color
    }
}

/// Shortens a name with an ellipsis to fit the available width.
fn truncated(painter: &mut Painter, name: &str, avail: f32, weight: u16) -> String {
    if painter.measure(name, BODY_FAMILY, TEXT_SIZE, weight) <= avail {
        return name.to_string();
    }
    let mut cut = name.to_string();
    while !cut.is_empty() {
        cut.pop();
        let candidate = format!("{cut}\u{2026}");
        if painter.measure(&candidate, BODY_FAMILY, TEXT_SIZE, weight) <= avail {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-side-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/subsub")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        for f in [
            "zeta.md",
            "Alpha.rs",
            "notes.txt",
            "README",
            "photo.png",
            "Cargo.lock",
            ".gitignore",
            "sub/inner.md",
            "sub/junk.bin",
            "sub/subsub/deep.md",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    fn names(side: &Sidebar) -> Vec<String> {
        side.entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn recognizes_renderable_known_and_dot_names() {
        for name in ["a.md", "b.rs", "c.txt", "README", "LICENSE", "Makefile"] {
            assert!(recognized(name, false), "{name}");
        }
        for name in [".gitignore", ".env"] {
            assert!(recognized(name, false), "{name}");
        }
        for name in ["photo.png", "Cargo.lock", "binary", "a.pdf"] {
            assert!(!recognized(name, false), "{name}");
        }
        for name in ["src", ".git", "node_modules"] {
            assert!(recognized(name, true), "{name}");
        }
    }

    #[test]
    fn scan_orders_directories_first_both_alphabetical() {
        let dir = temp_tree("order");
        let side = Sidebar::new(&dir);
        assert_eq!(
            names(&side),
            [
                "..",
                ".git",
                "sub",
                ".gitignore",
                "Alpha.rs",
                "notes.txt",
                "README",
                "zeta.md"
            ]
        );
        assert!(side.entries[0].is_dir && !side.entries[0].hidden);
        assert!(side.entries[1].is_dir && side.entries[1].hidden);
        assert!(side.entries[2].is_dir && !side.entries[2].hidden);
        assert!(side.entries[3].hidden && !side.entries[3].is_dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parent_row_absent_at_the_filesystem_root() {
        let side = Sidebar::new(Path::new("/"));
        assert!(side.entries.first().is_none_or(|e| e.name != ".."));
    }

    #[test]
    fn parent_row_reroots_and_keeps_the_left_folder_selected() {
        let dir = temp_tree("up");
        let mut side = Sidebar::new(&dir.join("sub"));
        assert_eq!(side.entries[0].name, "..");
        assert!(side.activate(0).is_none());
        assert_eq!(side.root(), dir.as_path());
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert_eq!(side.selected, sub);
        assert!(names(&side).contains(&"zeta.md".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_inserts_children_in_place_and_collapse_removes_them() {
        let dir = temp_tree("expand");
        let mut side = Sidebar::new(&dir);
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.activate(sub).is_none());
        assert_eq!(
            names(&side),
            [
                "..",
                ".git",
                "sub",
                "subsub",
                "inner.md",
                ".gitignore",
                "Alpha.rs",
                "notes.txt",
                "README",
                "zeta.md"
            ]
        );
        assert_eq!(side.entries[sub + 1].depth, 1);
        let subsub = sub + 1;
        assert!(side.activate(subsub).is_none());
        assert_eq!(side.entries[subsub + 1].name, "deep.md");
        assert_eq!(side.entries[subsub + 1].depth, 2);
        // Collapsing the top directory removes every deeper row at once.
        assert!(side.activate(sub).is_none());
        assert_eq!(
            names(&side),
            [
                "..",
                ".git",
                "sub",
                ".gitignore",
                "Alpha.rs",
                "notes.txt",
                "README",
                "zeta.md"
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn activating_a_file_returns_its_path() {
        let dir = temp_tree("open");
        let mut side = Sidebar::new(&dir);
        let md = side
            .entries
            .iter()
            .position(|e| e.name == "zeta.md")
            .unwrap();
        assert_eq!(side.activate(md), Some(dir.join("zeta.md")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn selection_moves_within_bounds_and_enter_activates() {
        let dir = temp_tree("select");
        let mut side = Sidebar::new(&dir);
        side.move_selection(-3);
        assert_eq!(side.selected, 0);
        for _ in 0..20 {
            side.move_selection(1);
        }
        assert_eq!(side.selected, side.entries.len() - 1);
        assert_eq!(side.enter(), Some(dir.join("zeta.md")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
