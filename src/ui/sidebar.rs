//! Folder sidebar: a persistent panel listing the open file's folder as a
//! tree. Directories sort before files, both alphabetical; the listing
//! keeps every file Oryx can display, including dot entries, which are
//! drawn dimmed. Expansion is in place and children are read on demand.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::load::{self, FileKind};
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::outline::OutlineTree;

/// Panel width in pixels for a reader who has never dragged the edge.
pub const DEFAULT_WIDTH: f32 = 260.0;
/// Narrowest the panel goes, below which names stop being readable.
pub const MIN_WIDTH: f32 = 160.0;
/// Widest the panel goes whatever the window size.
pub const MAX_WIDTH: f32 = 640.0;
/// Document area a sidebar drag may never squeeze below.
const MIN_DOC: f32 = 240.0;
/// Half-width of the grab zone straddling the right edge.
pub const GRAB: f32 = 4.0;

pub const ROW_H: f32 = 30.0;
const PAD: f32 = 10.0;
const INDENT: f32 = 14.0;
const TEXT_SIZE: f32 = 15.0;
/// Room the type icon column takes before a row's name.
const ICON_W: f32 = 18.0;
/// Height of the caption row naming the two tabs. Chrome, like the row
/// metrics: fixed against zoom.
pub const CAPTION_H: f32 = 34.0;
/// Dead zone either side of the caption row's middle.
const CAPTION_GAP: f32 = 3.0;

/// Which panel tab is active; persisted in config.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Files,
    Outline,
}

/// What a click inside the panel asks the app to do.
#[derive(Debug, PartialEq, Eq)]
pub enum SideClick {
    None,
    /// The active tab switched; the app persists it.
    Tab,
    /// Open this file in the document area.
    Open(PathBuf),
    /// Scroll the document to this heading block.
    Jump(usize),
}

/// The tab a click at panel coordinates lands on; None outside the
/// caption row and in the dead zone between the two captions.
pub fn caption_hit(width: f32, x: f32, y: f32) -> Option<Tab> {
    if !(0.0..CAPTION_H).contains(&y) {
        return None;
    }
    let mid = width / 2.0;
    if x >= PAD && x < mid - CAPTION_GAP {
        Some(Tab::Files)
    } else if x >= mid + CAPTION_GAP && x < width - PAD {
        Some(Tab::Outline)
    } else {
        None
    }
}

/// Shortens text with a trailing ellipsis to fit `avail`, measured by
/// the caller's closure so the fitting stays pure and testable.
pub fn fit(text: &str, avail: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= avail {
        return text.to_string();
    }
    let mut cut = text.to_string();
    while !cut.is_empty() {
        cut.pop();
        let candidate = format!("{cut}\u{2026}");
        if measure(&candidate) <= avail {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

/// A width the panel may actually take, given the window it sits in. The
/// window bound wins over `MIN_WIDTH` only when the window is too narrow
/// to honor both, in which case the panel keeps its minimum.
pub fn clamp_width(want: f32, window_w: f32) -> f32 {
    let max = MAX_WIDTH.min((window_w - MIN_DOC).max(MIN_WIDTH));
    want.clamp(MIN_WIDTH, max)
}

/// Whether `x` falls in the drag zone straddling the panel's right edge.
pub fn on_edge(width: f32, x: f32) -> bool {
    (x - width).abs() <= GRAB
}

/// Which shape marks a file in the list.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Icon {
    /// Markdown, the thing Oryx is for.
    Document,
    Code,
    Config,
    Text,
    /// Recognized by its bytes alone.
    Unknown,
}

/// Tokens that read as configuration rather than as source. Which
/// languages belong here is a presentation judgment, not a fact about
/// parsing, so the list sits beside the drawing instead of beside the
/// extension table.
const DATA_TOKENS: &[&str] = &[
    "ini",
    "json",
    "properties",
    "terraform",
    "toml",
    "xml",
    "yaml",
];

fn icon_for(path: &Path) -> Icon {
    match load::detect(path) {
        FileKind::Markdown => Icon::Document,
        FileKind::Code(token) if DATA_TOKENS.contains(&token) => Icon::Config,
        FileKind::Code(_) => Icon::Code,
        FileKind::Text => Icon::Text,
        FileKind::Epub => Icon::Document,
        FileKind::Unknown => Icon::Unknown,
    }
}

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
    width: f32,
    /// The active tab; the caption row switches it.
    tab: Tab,
    root: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    /// The file currently displayed in the document area.
    current: Option<PathBuf>,
    scroll: f32,
    list_h: f32,
}

/// Whether a directory entry belongs in the tree. Directories always do,
/// and a file does when Oryx can display it, which for an extension the
/// table does not name means reading the first bytes.
fn recognized(path: &Path, is_dir: bool) -> bool {
    if is_dir {
        return true;
    }
    match load::detect(path) {
        FileKind::Unknown => load::is_text_file(path),
        _ => true,
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
            recognized(&e.path(), is_dir).then(|| Entry {
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
            width: DEFAULT_WIDTH,
            tab: Tab::Files,
            root: root.to_path_buf(),
            entries: tree(root),
            selected: 0,
            current: None,
            scroll: 0.0,
            list_h: 0.0,
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// Visible list height below the caption row, from the last draw.
    pub fn list_h(&self) -> f32 {
        self.list_h
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

    pub fn width(&self) -> f32 {
        self.width
    }

    /// Sets the panel width, clamped to what the window can carry.
    pub fn set_width(&mut self, want: f32, window_w: f32) {
        self.width = clamp_width(want, window_w);
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

    /// A click inside the panel: the caption row switches tabs, a file
    /// row opens or expands, an outline row folds or jumps.
    pub fn click(&mut self, x: f32, y: f32, outline: &mut OutlineTree) -> SideClick {
        if let Some(tab) = caption_hit(self.width, x, y) {
            if tab != self.tab {
                self.tab = tab;
                return SideClick::Tab;
            }
            return SideClick::None;
        }
        match self.tab {
            Tab::Files => {
                let index = ((y - PAD - CAPTION_H + self.scroll) / ROW_H).floor();
                if index < 0.0 || index as usize >= self.entries.len() {
                    return SideClick::None;
                }
                match self.activate(index as usize) {
                    Some(path) => SideClick::Open(path),
                    None => SideClick::None,
                }
            }
            Tab::Outline => {
                let rows = outline.rows();
                let index = ((y - PAD - CAPTION_H + outline.scroll) / ROW_H).floor();
                if index < 0.0 || index as usize >= rows.len() {
                    return SideClick::None;
                }
                let index = index as usize;
                let row = rows[index];
                outline.selected = index;
                let chevron_end = PAD + row.depth as f32 * INDENT + 12.0;
                if row.has_children && x < chevron_end {
                    outline.toggle_row(index);
                    return SideClick::None;
                }
                SideClick::Jump(outline.entries()[row.entry].block)
            }
        }
    }

    pub fn wheel(&mut self, lines: f32, outline: &mut OutlineTree) {
        match self.tab {
            Tab::Files => {
                self.scroll = (self.scroll + lines * ROW_H).clamp(0.0, self.max_scroll());
            }
            Tab::Outline => {
                let max = (outline.rows().len() as f32 * ROW_H - self.list_h).max(0.0);
                outline.scroll = (outline.scroll + lines * ROW_H).clamp(0.0, max);
            }
        }
    }

    /// Draws the panel: captions, then the active tab's list. `current`
    /// is the outline entry carrying the reading-position highlight.
    pub fn draw(
        &mut self,
        painter: &mut Painter,
        theme: &Theme,
        outline: &mut OutlineTree,
        current: Option<usize>,
    ) {
        let h = painter.height();
        let ui = &theme.ui;
        let width = self.width;
        painter.fill(0.0, 0.0, width, h, 0.0, ui.sidebar_bg);
        self.list_h = h - 2.0 * PAD - CAPTION_H;
        match self.tab {
            Tab::Files => self.draw_files(painter, theme),
            Tab::Outline => self.draw_outline(painter, theme, outline, current),
        }
        // Masks cover row overflow above and below the list viewport; the
        // captions and the border paint over them.
        painter.fill(0.0, 0.0, width, PAD + CAPTION_H, 0.0, ui.sidebar_bg);
        painter.fill(0.0, h - PAD, width, PAD, 0.0, ui.sidebar_bg);
        self.draw_captions(painter, theme);
        painter.line(
            width - 0.5,
            0.0,
            width - 0.5,
            h,
            1.0,
            theme.blocks.table_border,
        );
    }

    /// The two tab captions: a small icon beside a text label, the
    /// active one in full color over an underline, the other dimmed the
    /// way dot entries are. Captions truncate; icons never do.
    fn draw_captions(&self, painter: &mut Painter, theme: &Theme) {
        let ui = &theme.ui;
        let mid = self.width / 2.0;
        let zones = [
            (Tab::Files, PAD, mid - CAPTION_GAP),
            (Tab::Outline, mid + CAPTION_GAP, self.width - PAD),
        ];
        for (tab, x0, x1) in zones {
            let active = tab == self.tab;
            let color = if active {
                ui.sidebar_fg
            } else {
                dim(ui.sidebar_fg)
            };
            let iy = (CAPTION_H - 12.0) / 2.0;
            match tab {
                Tab::Files => {
                    // The folder shape the tree rows use.
                    painter.fill(x0, iy + 1.0, 5.5, 3.0, 1.0, color);
                    painter.fill(x0, iy + 3.5, 11.0, 8.0, 1.5, color);
                }
                Tab::Outline => {
                    // Indented lines, an outline in miniature.
                    painter.fill(x0, iy + 1.5, 10.0, 1.6, 0.8, color);
                    painter.fill(x0 + 3.0, iy + 5.2, 7.0, 1.6, 0.8, color);
                    painter.fill(x0, iy + 8.9, 10.0, 1.6, 0.8, color);
                }
            }
            let label = match tab {
                Tab::Files => "Files",
                Tab::Outline => "Outline",
            };
            let weight = if active { 700 } else { 400 };
            let tx = x0 + ICON_W;
            let text = truncated(painter, label, x1 - tx, weight);
            painter.text(tx, 7.0, &text, BODY_FAMILY, TEXT_SIZE, weight, color);
            if active {
                painter.fill(x0, CAPTION_H - 3.0, x1 - x0, 2.0, 1.0, color);
            }
        }
    }

    /// The outline tab: one row per visible heading, chevrons on rows
    /// with children, the current section bold.
    fn draw_outline(
        &mut self,
        painter: &mut Painter,
        theme: &Theme,
        outline: &mut OutlineTree,
        current: Option<usize>,
    ) {
        let ui = &theme.ui;
        let h = painter.height();
        let rows = outline.rows();
        if rows.is_empty() {
            let color = dim(ui.sidebar_fg);
            painter.text(
                PAD,
                CAPTION_H + PAD + 5.0,
                "No headings",
                BODY_FAMILY,
                TEXT_SIZE,
                400,
                color,
            );
            return;
        }
        let max = (rows.len() as f32 * ROW_H - self.list_h).max(0.0);
        outline.scroll = outline.scroll.clamp(0.0, max);
        outline.selected = outline.selected.min(rows.len() - 1);
        let top = PAD + CAPTION_H;
        let first = (outline.scroll / ROW_H).floor() as usize;
        let offset = -(outline.scroll - first as f32 * ROW_H);
        let mut slot = 0usize;
        loop {
            let index = first + slot;
            let ry = top + offset + slot as f32 * ROW_H;
            if index >= rows.len() || ry > h - PAD {
                break;
            }
            slot += 1;
            let row = rows[index];
            if index == outline.selected {
                painter.fill(
                    3.0,
                    ry,
                    self.width - 8.0,
                    ROW_H - 2.0,
                    5.0,
                    ui.overlay_highlight,
                );
            }
            let x = PAD + row.depth as f32 * INDENT;
            // An unresolved book entry dims like a dot file and jumps
            // nowhere.
            let color = if row.dead {
                dim(ui.sidebar_fg)
            } else {
                ui.sidebar_fg
            };
            if row.has_children {
                let cx = x + 3.0;
                let cy = ry + ROW_H / 2.0;
                if row.collapsed {
                    // Right-pointing: fold closed.
                    painter.line(cx - 1.5, cy - 3.5, cx + 2.5, cy, 1.6, color);
                    painter.line(cx + 2.5, cy, cx - 1.5, cy + 3.5, 1.6, color);
                } else {
                    // Down-pointing: open.
                    painter.line(cx - 3.5, cy - 1.5, cx, cy + 2.5, 1.6, color);
                    painter.line(cx, cy + 2.5, cx + 3.5, cy - 1.5, 1.6, color);
                }
            }
            let is_current = current == Some(row.entry);
            let weight = if is_current { 700 } else { 400 };
            let tx = x + 12.0;
            let avail = self.width - tx - PAD;
            let name = truncated(painter, &outline.entries()[row.entry].text, avail, weight);
            painter.text(tx, ry + 5.0, &name, BODY_FAMILY, TEXT_SIZE, weight, color);
        }
    }

    fn draw_files(&mut self, painter: &mut Painter, theme: &Theme) {
        let h = painter.height();
        let ui = &theme.ui;
        let width = self.width;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = -(self.scroll - first as f32 * ROW_H);
        let mut slot = 0usize;
        loop {
            let index = first + slot;
            let ry = PAD + CAPTION_H + offset + slot as f32 * ROW_H;
            if index >= self.entries.len() || ry > h - PAD {
                break;
            }
            slot += 1;
            let entry = &self.entries[index];
            if index == self.selected {
                painter.fill(3.0, ry, width - 8.0, ROW_H - 2.0, 5.0, ui.overlay_highlight);
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
            } else {
                let iy = ry + (ROW_H - 12.0) / 2.0;
                draw_icon(painter, icon_for(&entry.path), x, iy, color, ui.sidebar_bg);
            }
            let current = self.current.as_deref() == Some(entry.path.as_path());
            let weight = if current { 700 } else { 400 };
            let avail = width - x - ICON_W - PAD;
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

/// The type mark for one file, drawn in an 11 by 12 box at `x`, `y`.
/// Every shape is built from the painter's rectangles and lines, since the
/// UI has no icon font: the three page-shaped marks share a silhouette and
/// differ inside it, and the two others take their own outline.
fn draw_icon(painter: &mut Painter, icon: Icon, x: f32, y: f32, color: Rgba, bg: Rgba) {
    const W: f32 = 10.0;
    const H: f32 = 12.0;
    match icon {
        Icon::Document => {
            painter.fill(x, y, W, H, 1.5, color);
            // Two lines of text knocked out of the page.
            painter.fill(x + 2.0, y + 3.5, W - 4.0, 1.5, 0.0, bg);
            painter.fill(x + 2.0, y + 7.0, W - 4.0, 1.5, 0.0, bg);
        }
        Icon::Text => {
            painter.fill(x, y, W, H, 1.5, color);
        }
        Icon::Unknown => {
            painter.stroke(x, y, W, H, 1.5, 1.2, color);
        }
        Icon::Code => {
            // Angle brackets, the shape source carries everywhere.
            let mid = y + H / 2.0;
            painter.line(x + 4.0, y + 1.5, x + 0.5, mid, 1.4, color);
            painter.line(x + 0.5, mid, x + 4.0, y + H - 1.5, 1.4, color);
            painter.line(x + W - 4.0, y + 1.5, x + W - 0.5, mid, 1.4, color);
            painter.line(x + W - 0.5, mid, x + W - 4.0, y + H - 1.5, 1.4, color);
        }
        Icon::Config => {
            // Three sliders with their knobs at different settings.
            for (row, knob) in [(2.0, 6.5), (6.0, 2.0), (10.0, 5.0)] {
                painter.fill(x, y + row - 0.5, W, 1.2, 0.6, color);
                painter.fill(x + knob, y + row - 2.0, 2.4, 4.0, 1.0, color);
            }
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
    fit(name, avail, |text| {
        painter.measure(text, BODY_FAMILY, TEXT_SIZE, weight)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::markdown;
    use crate::style::fonts::FontStore;
    use tiny_skia::Pixmap;

    /// Pixels outside the list viewport: the caption band above and the
    /// padding strip below. Scrolling the list must never repaint them.
    fn chrome_pixels(
        side: &mut Sidebar,
        outline: &mut OutlineTree,
        fonts: &mut FontStore,
        theme: &Theme,
    ) -> Vec<u8> {
        let (w, h) = (260usize, 300usize);
        let mut pixmap = Pixmap::new(w as u32, h as u32).unwrap();
        let mut painter = Painter::new(&mut pixmap, fonts, None);
        side.draw(&mut painter, theme, outline, None);
        let row = w * 4;
        let top = (PAD + CAPTION_H) as usize;
        let bottom = h - PAD as usize;
        let mut band = pixmap.data()[..top * row].to_vec();
        band.extend_from_slice(&pixmap.data()[bottom * row..]);
        band
    }

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
            "Cargo.lock",
            ".gitignore",
            "sub/inner.md",
            "sub/subsub/deep.md",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        for f in ["photo.png", "sub/junk.bin"] {
            std::fs::write(dir.join(f), b"\x89PNG\r\n\x1a\n\x00\x00\x00\r").unwrap();
        }
        dir
    }

    fn names(side: &Sidebar) -> Vec<String> {
        side.entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn a_scrolled_outline_paints_only_inside_the_list_viewport() {
        let dir = temp_tree("chrome-outline");
        let mut side = Sidebar::new(&dir);
        side.set_tab(Tab::Outline);
        let source: String = (1..=30).map(|i| format!("# Heading {i}\n\n")).collect();
        let doc = markdown::parse(&*source);
        let mut outline = OutlineTree::build(&doc);
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let rested = chrome_pixels(&mut side, &mut outline, &mut fonts, &theme);
        outline.scroll = ROW_H / 2.0;
        let scrolled = chrome_pixels(&mut side, &mut outline, &mut fonts, &theme);
        assert_eq!(rested, scrolled, "a partial row leaked into the chrome");
    }

    #[test]
    fn a_scrolled_file_tree_paints_only_inside_the_list_viewport() {
        let dir = temp_tree("chrome-files");
        let mut side = Sidebar::new(&dir);
        let doc = markdown::parse("");
        let mut outline = OutlineTree::build(&doc);
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let rested = chrome_pixels(&mut side, &mut outline, &mut fonts, &theme);
        side.scroll = ROW_H / 2.0;
        let scrolled = chrome_pixels(&mut side, &mut outline, &mut fonts, &theme);
        assert_eq!(rested, scrolled, "a partial row leaked into the chrome");
    }

    #[test]
    fn caption_hit_answers_each_tab_and_neither_between() {
        let w = 260.0;
        assert_eq!(caption_hit(w, 20.0, 10.0), Some(Tab::Files));
        assert_eq!(caption_hit(w, 200.0, 10.0), Some(Tab::Outline));
        assert_eq!(caption_hit(w, w / 2.0, 10.0), None, "the dead middle");
        assert_eq!(caption_hit(w, 2.0, 10.0), None, "left pad");
        assert_eq!(caption_hit(w, w - 2.0, 10.0), None, "right pad");
        assert_eq!(caption_hit(w, 20.0, CAPTION_H + 1.0), None, "below the row");
    }

    #[test]
    fn fit_keeps_what_fits_and_cuts_with_an_ellipsis() {
        // A fake measure: ten pixels per character.
        let measure = |t: &str| t.chars().count() as f32 * 10.0;
        assert_eq!(fit("short", 100.0, measure), "short");
        assert_eq!(fit("exactly ten", 110.0, measure), "exactly ten");
        let cut = fit("far too long a name", 60.0, measure);
        assert_eq!(cut, "far t\u{2026}", "five chars plus the ellipsis");
        assert_eq!(fit("abc", 5.0, measure), "\u{2026}", "nothing fits");
    }

    #[test]
    fn each_file_kind_takes_its_own_icon() {
        for (name, icon) in [
            ("notes.md", Icon::Document),
            ("README.markdown", Icon::Document),
            ("main.rs", Icon::Code),
            ("build.gradle", Icon::Code),
            ("Cargo.toml", Icon::Config),
            ("config.yaml", Icon::Config),
            ("app.ini", Icon::Config),
            ("data.json", Icon::Config),
            ("notes.txt", Icon::Text),
            ("Makefile", Icon::Code),
            (".gitignore", Icon::Unknown),
        ] {
            assert_eq!(icon_for(Path::new(name)), icon, "{name}");
        }
    }

    #[test]
    fn a_config_language_reads_as_configuration_not_as_source() {
        // Both are FileKind::Code; only the token separates them.
        assert_eq!(icon_for(Path::new("a.toml")), Icon::Config);
        assert_eq!(icon_for(Path::new("a.tf")), Icon::Config);
        assert_eq!(icon_for(Path::new("a.rs")), Icon::Code);
    }

    #[test]
    fn width_clamps_between_its_bounds() {
        let roomy = 1600.0;
        assert_eq!(clamp_width(DEFAULT_WIDTH, roomy), DEFAULT_WIDTH);
        assert_eq!(clamp_width(10.0, roomy), MIN_WIDTH, "below the minimum");
        assert_eq!(clamp_width(9999.0, roomy), MAX_WIDTH, "above the maximum");
    }

    #[test]
    fn a_narrow_window_bounds_the_panel_before_the_maximum_does() {
        // 700 wide leaves 460 once the document keeps its 240.
        assert_eq!(clamp_width(9999.0, 700.0), 460.0);
        // Too narrow to honor both: the panel keeps its minimum and the
        // document gives way, rather than the panel collapsing to nothing.
        assert_eq!(clamp_width(9999.0, 300.0), MIN_WIDTH);
        assert_eq!(clamp_width(50.0, 300.0), MIN_WIDTH);
    }

    #[test]
    fn the_grab_zone_answers_for_the_edge_alone() {
        let w = 260.0;
        assert!(on_edge(w, 260.0), "on it");
        assert!(on_edge(w, 260.0 - GRAB), "just inside");
        assert!(on_edge(w, 260.0 + GRAB), "just outside");
        assert!(!on_edge(w, 260.0 - GRAB - 1.0), "the list");
        assert!(!on_edge(w, 260.0 + GRAB + 1.0), "the document");
        assert!(!on_edge(w, 0.0));
    }

    #[test]
    fn set_width_clamps_and_reports() {
        let dir = temp_tree("width");
        let mut side = Sidebar::new(&dir);
        assert_eq!(side.width(), DEFAULT_WIDTH);
        side.set_width(400.0, 1600.0);
        assert_eq!(side.width(), 400.0);
        side.set_width(20.0, 1600.0);
        assert_eq!(side.width(), MIN_WIDTH);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_known_extension_is_listed_without_reading_the_file() {
        for name in ["a.md", "b.rs", "c.txt", "d.hs"] {
            assert!(recognized(Path::new(name), false), "{name}");
        }
    }

    #[test]
    fn an_unknown_extension_is_listed_when_its_bytes_are_text() {
        let dir = temp_tree("recognize");
        for name in ["README", "Cargo.lock", ".gitignore"] {
            assert!(recognized(&dir.join(name), false), "{name}");
        }
        assert!(!recognized(&dir.join("photo.png"), false));
        assert!(!recognized(&dir.join("sub/junk.bin"), false));
        assert!(!recognized(&dir.join("gone.unknown"), false), "unreadable");
        for name in ["sub", ".git"] {
            assert!(recognized(&dir.join(name), true), "{name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
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
                "Cargo.lock",
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
                "Cargo.lock",
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
                "Cargo.lock",
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
