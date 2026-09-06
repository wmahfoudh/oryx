//! Folder sidebar: a persistent panel listing the open file's folder as a
//! tree. Directories sort before files, both alphabetical; the listing
//! keeps every file Oryx can display, including dot entries, which are
//! drawn dimmed. Expansion is in place and children are read on demand.
//! A folder reached through a symbolic link is entered rather than
//! expanded: the tree moves to the real folder, as if it had been
//! opened directly.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::load::{self, FileKind};
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme, Ui};
use crate::ui::outline::OutlineTree;
use crate::ui::overlay::{accent_fill, dim, guide, hover_fill, soft};
use crate::ui::scrollbar;

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
/// Inset of the row fills and the accent bar from the panel's left edge.
const FILL_X: f32 = 5.0;
/// Width of the accent bar marking the open file or the current heading.
const BAR_W: f32 = 3.0;
/// Height of a type mark; the folder and the pages share it.
const MARK_H: f32 = 12.0;
/// The triangle's center within its column, a pixel left of the middle
/// so the triangle and the mark after it breathe.
const TRIANGLE_CX: f32 = INDENT / 2.0 - 1.0;
/// The scrollbar thumb: its width and its distance from the panel's
/// right edge, which keeps it clear of the resize grab zone.
const THUMB_W: f32 = 6.0;
const THUMB_INSET: f32 = 5.0;
/// The rightmost band of the panel, where a press goes to the bar rather
/// than to a row while the list scrolls; names stop short of it.
const STRIP_W: f32 = 14.0;

/// Where a row's parts sit for its depth: the triangle column, the mark
/// column and the name. The outline has no marks, so its names start
/// where the mark column would.
#[derive(Debug, PartialEq)]
pub struct RowLayout {
    pub triangle_x: f32,
    pub mark_x: f32,
    pub text_x: f32,
}

pub fn row_layout(depth: usize, marks: bool) -> RowLayout {
    let triangle_x = PAD + depth as f32 * INDENT;
    let mark_x = triangle_x + INDENT;
    RowLayout {
        triangle_x,
        mark_x,
        text_x: if marks { mark_x + ICON_W } else { mark_x },
    }
}

/// The x of the guide a row draws for `level`, one of the levels above
/// its own: the center of that level's triangle column.
pub fn guide_x(level: usize) -> f32 {
    PAD + level as f32 * INDENT + INDENT / 2.0
}

/// What the mouse rests on inside the panel.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Hover {
    Row(usize),
    Thumb,
}

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
    /// The press took the scrollbar thumb; the app drags it until release.
    Thumb,
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
        FileKind::Epub | FileKind::Fb2 | FileKind::Kindle | FileKind::Comic => Icon::Document,
        FileKind::Undisplayable | FileKind::Unknown => Icon::Unknown,
    }
}

/// One visible row of the tree.
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// A symbolic link to a folder; activating it moves the tree there.
    pub linked: bool,
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
    /// The row or the thumb under the mouse, from the last cursor move.
    hover: Option<Hover>,
    /// The cursor's offset from the thumb's top while the thumb is held.
    thumb_grab: Option<f32>,
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
        FileKind::Undisplayable => false,
        _ => true,
    }
}

/// The recognized entries of one directory, directories first, both
/// groups alphabetical and case-insensitive. A symbolic link counts as
/// what it points at, so a linked folder lists among the folders.
fn scan(dir: &Path, depth: usize) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let kind = e.file_type().ok()?;
            let is_dir = kind.is_dir() || (kind.is_symlink() && e.path().is_dir());
            recognized(&e.path(), is_dir).then(|| Entry {
                hidden: name.starts_with('.'),
                path: e.path(),
                is_dir,
                linked: is_dir && kind.is_symlink(),
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
            linked: false,
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
            hover: None,
            thumb_grab: None,
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

    /// Rebuilds the tree at the real folder a link points to, the first
    /// entry inside it selected; `..` climbs the real path from there.
    fn enter_link(&mut self, link: &Path) {
        let target = std::fs::canonicalize(link).unwrap_or_else(|_| link.to_path_buf());
        self.root = target.clone();
        self.entries = tree(&target);
        self.scroll = 0.0;
        let first = usize::from(self.entries.first().is_some_and(|e| e.name == ".."));
        self.selected = first.min(self.entries.len().saturating_sub(1));
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
    /// toggles its expansion, a linked directory becomes the root.
    pub fn activate(&mut self, index: usize) -> Option<PathBuf> {
        let entry = self.entries.get(index)?;
        self.selected = index;
        if index == 0 && entry.name == ".." {
            let parent = entry.path.clone();
            self.go_up(&parent);
            None
        } else if entry.linked {
            let link = entry.path.clone();
            self.enter_link(&link);
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

    /// The active list's scroll and its full height.
    fn list_extent(&self, outline: &OutlineTree) -> (f32, f32) {
        match self.tab {
            Tab::Files => (self.scroll, self.entries.len() as f32 * ROW_H),
            Tab::Outline => (outline.scroll, outline.rows().len() as f32 * ROW_H),
        }
    }

    /// The thumb of the active list as `(y, height)` from the list's
    /// top, or None when the list fits its viewport.
    pub fn thumb(&self, outline: &OutlineTree) -> Option<(f32, f32)> {
        let (scroll, content_h) = self.list_extent(outline);
        scrollbar::thumb(content_h, self.list_h, scroll, self.list_h, 1.0)
    }

    /// Records what the mouse rests on at panel coordinates: a row of
    /// the active tab, or the thumb. Reports whether it changed, so the
    /// caller redraws only then.
    pub fn hover_at(&mut self, x: f32, y: f32, outline: &OutlineTree) -> bool {
        let top = PAD + CAPTION_H;
        let in_list = x >= 0.0 && x < self.width && y >= top && y < top + self.list_h;
        let hover = if !in_list {
            None
        } else if let Some((ty, th)) = self.thumb(outline).filter(|_| x >= self.width - STRIP_W) {
            (y >= top + ty && y < top + ty + th).then_some(Hover::Thumb)
        } else {
            let (scroll, content_h) = self.list_extent(outline);
            let index = ((y - top + scroll) / ROW_H).floor();
            (index >= 0.0 && index * ROW_H < content_h).then_some(Hover::Row(index as usize))
        };
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// Forgets the hover, for a cursor that left the panel or the
    /// window. Reports whether there was one.
    pub fn clear_hover(&mut self) -> bool {
        self.hover.take().is_some()
    }

    /// Moves the active list so the held thumb follows the cursor at `y`.
    pub fn drag_thumb(&mut self, y: f32, outline: &mut OutlineTree) {
        let (Some(grab), Some((_, th))) = (self.thumb_grab, self.thumb(outline)) else {
            return;
        };
        let (_, content_h) = self.list_extent(outline);
        let scroll = scrollbar::scroll_for_thumb(
            y - (PAD + CAPTION_H) - grab,
            th,
            self.list_h,
            content_h,
            self.list_h,
        );
        match self.tab {
            Tab::Files => self.scroll = scroll,
            Tab::Outline => outline.scroll = scroll,
        }
    }

    /// The mouse button went up: the thumb, if held, is let go.
    pub fn release(&mut self) {
        self.thumb_grab = None;
    }

    /// A click inside the panel: the caption row switches tabs, a press
    /// on the bar takes the thumb, a file row opens or expands, an
    /// outline row folds or jumps.
    pub fn click(&mut self, x: f32, y: f32, outline: &mut OutlineTree) -> SideClick {
        if let Some(tab) = caption_hit(self.width, x, y) {
            if tab != self.tab {
                self.tab = tab;
                return SideClick::Tab;
            }
            return SideClick::None;
        }
        let top = PAD + CAPTION_H;
        if x >= self.width - STRIP_W && y >= top && y < top + self.list_h {
            if let Some((ty, th)) = self.thumb(outline) {
                // On the thumb the grab keeps the cursor's offset; on the
                // track the thumb jumps to center on the cursor.
                let at = y - top;
                let grab = if at >= ty && at < ty + th {
                    at - ty
                } else {
                    th / 2.0
                };
                self.thumb_grab = Some(grab);
                self.drag_thumb(y, outline);
                return SideClick::Thumb;
            }
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
                let chevron_end = row_layout(row.depth as usize, false).text_x;
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
    /// is the outline entry carrying the reading-position mark.
    /// `owns_keys` is key ownership: while the panel does not own Up,
    /// Down, and Enter, the active caption dims and the keyboard's row
    /// shows no fill.
    pub fn draw(
        &mut self,
        painter: &mut Painter,
        theme: &Theme,
        outline: &mut OutlineTree,
        current: Option<usize>,
        owns_keys: bool,
    ) {
        let h = painter.height();
        let ui = &theme.ui;
        let width = self.width;
        painter.fill(0.0, 0.0, width, h, 0.0, ui.sidebar_bg);
        self.list_h = h - 2.0 * PAD - CAPTION_H;
        // The rows draw under a clip to the list viewport, between the
        // caption row and the bottom pad, so a cut row ends there.
        painter.clip(Some((0.0, PAD + CAPTION_H, width, self.list_h)));
        match self.tab {
            Tab::Files => self.draw_files(painter, theme, owns_keys),
            Tab::Outline => self.draw_outline(painter, theme, outline, current, owns_keys),
        }
        painter.clip(None);
        self.draw_thumb(painter, theme, outline);
        self.draw_captions(painter, theme, owns_keys);
        painter.line(
            width - 0.5,
            0.0,
            width - 0.5,
            h,
            1.0,
            theme.blocks.table_border,
        );
    }

    /// The two tab captions: a small icon beside a text label over a
    /// hairline across the panel, the active one in full color with an
    /// accent underline on the hairline, the other dimmed the way dot
    /// entries are. The active caption dims too while the panel does
    /// not own the keys. Captions truncate; icons never do.
    fn draw_captions(&self, painter: &mut Painter, theme: &Theme, owns_keys: bool) {
        let ui = &theme.ui;
        let mid = self.width / 2.0;
        painter.fill(
            0.0,
            CAPTION_H - 1.0,
            self.width,
            1.0,
            0.0,
            guide(ui.sidebar_fg),
        );
        let zones = [
            (Tab::Files, PAD, mid - CAPTION_GAP),
            (Tab::Outline, mid + CAPTION_GAP, self.width - PAD),
        ];
        for (tab, x0, x1) in zones {
            let active = tab == self.tab;
            let color = if active && owns_keys {
                ui.sidebar_fg
            } else {
                dim(ui.sidebar_fg)
            };
            let iy = (CAPTION_H - 12.0) / 2.0;
            match tab {
                Tab::Files => draw_folder(painter, x0, iy, color, false),
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
                painter.fill(x0, CAPTION_H - 2.0, x1 - x0, 2.0, 1.0, ui.sidebar_dir);
            }
        }
    }

    /// The scrollbar thumb of the active list, in the hover color while
    /// the mouse rests on it or holds it.
    fn draw_thumb(&self, painter: &mut Painter, theme: &Theme, outline: &OutlineTree) {
        let Some((ty, th)) = self.thumb(outline) else {
            return;
        };
        let held = self.thumb_grab.is_some() || self.hover == Some(Hover::Thumb);
        let color = if held {
            theme.ui.scrollbar_hover
        } else {
            theme.ui.scrollbar
        };
        painter.fill(
            self.width - THUMB_INSET - THUMB_W,
            PAD + CAPTION_H + ty,
            THUMB_W,
            th,
            THUMB_W / 2.0,
            color,
        );
    }

    /// The outline tab: one row per visible heading, triangles on rows
    /// with children, the current section in the accent look.
    fn draw_outline(
        &mut self,
        painter: &mut Painter,
        theme: &Theme,
        outline: &mut OutlineTree,
        current: Option<usize>,
        owns_keys: bool,
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
            let is_current = current == Some(row.entry);
            let attended =
                self.hover == Some(Hover::Row(index)) || (owns_keys && index == outline.selected);
            draw_row_ground(
                painter,
                self.width,
                ry,
                is_current,
                attended,
                ui.sidebar_fg,
                ui.sidebar_dir,
            );
            for level in 0..row.depth as usize {
                draw_guide(painter, level, ry, ui.sidebar_fg);
            }
            let layout = row_layout(row.depth as usize, false);
            let color = outline_color(ui, row.depth as usize, row.dead, is_current);
            if row.has_children {
                draw_triangle(
                    painter,
                    layout.triangle_x + TRIANGLE_CX,
                    ry + ROW_H / 2.0,
                    !row.collapsed,
                    soft(color),
                );
            }
            let avail = self.width - layout.text_x - STRIP_W;
            let name = truncated(painter, &outline.entries()[row.entry].text, avail, 400);
            painter.text(
                layout.text_x,
                ry + 5.0,
                &name,
                BODY_FAMILY,
                TEXT_SIZE,
                400,
                color,
            );
        }
    }

    fn draw_files(&mut self, painter: &mut Painter, theme: &Theme, owns_keys: bool) {
        let h = painter.height();
        let ui = &theme.ui;
        let width = self.width;
        let (fg, accent) = (ui.sidebar_fg, ui.sidebar_dir);
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
            let current = self.current.as_deref() == Some(entry.path.as_path());
            let attended =
                self.hover == Some(Hover::Row(index)) || (owns_keys && index == self.selected);
            draw_row_ground(painter, width, ry, current, attended, fg, accent);
            for level in 0..entry.depth {
                draw_guide(painter, level, ry, fg);
            }
            let layout = row_layout(entry.depth, true);
            let mut color = if current { accent } else { fg };
            if entry.hidden {
                color = dim(color);
            }
            let iy = ry + (ROW_H - MARK_H) / 2.0;
            if index == 0 && entry.name == ".." {
                // Up chevron in the mark column for the parent row.
                let (cx, cy) = (layout.mark_x + 5.0, ry + ROW_H / 2.0);
                painter.line(cx - 4.5, cy + 2.0, cx, cy - 2.5, 1.6, color);
                painter.line(cx, cy - 2.5, cx + 4.5, cy + 2.0, 1.6, color);
            } else if entry.is_dir {
                draw_triangle(
                    painter,
                    layout.triangle_x + TRIANGLE_CX,
                    ry + ROW_H / 2.0,
                    entry.expanded,
                    soft(color),
                );
                draw_folder(painter, layout.mark_x, iy, color, entry.linked);
            } else {
                draw_icon(painter, icon_for(&entry.path), layout.mark_x, iy, color);
            }
            let avail = width - layout.text_x - STRIP_W;
            let name = truncated(painter, &entry.name, avail, 400);
            painter.text(
                layout.text_x,
                ry + 5.0,
                &name,
                BODY_FAMILY,
                TEXT_SIZE,
                400,
                color,
            );
        }
    }
}

/// The row's ground: the accent fill and bar for the open file or the
/// current heading, the hover fill for a row under attention, nothing
/// otherwise. The accent look wins when both apply.
fn draw_row_ground(
    painter: &mut Painter,
    width: f32,
    ry: f32,
    current: bool,
    attended: bool,
    fg: Rgba,
    accent: Rgba,
) {
    let fill_w = width - 2.0 * FILL_X;
    if current {
        painter.fill(
            FILL_X,
            ry + 1.0,
            fill_w,
            ROW_H - 2.0,
            5.0,
            accent_fill(accent),
        );
        // The bar spans the fill's straight edge, between its rounded corners.
        painter.fill(FILL_X, ry + 6.0, BAR_W, ROW_H - 12.0, 1.5, accent);
    } else if attended {
        painter.fill(FILL_X, ry + 1.0, fill_w, ROW_H - 2.0, 5.0, hover_fill(fg));
    }
}

/// One row's segment of the guide for `level`, a one pixel line on a
/// whole column so consecutive rows join into one crisp line.
fn draw_guide(painter: &mut Painter, level: usize, ry: f32, fg: Rgba) {
    painter.fill(guide_x(level).floor(), ry, 1.0, ROW_H, 0.0, guide(fg));
}

/// A small filled triangle centered at `cx`, `cy`: pointing down on an
/// open folder or an expanded heading, right on a closed one.
fn draw_triangle(painter: &mut Painter, cx: f32, cy: f32, open: bool, color: Rgba) {
    if open {
        painter.triangle(
            [(cx - 4.0, cy - 2.0), (cx + 4.0, cy - 2.0), (cx, cy + 3.0)],
            color,
        );
    } else {
        painter.triangle(
            [(cx - 2.5, cy - 4.0), (cx + 3.0, cy), (cx - 2.5, cy + 4.0)],
            color,
        );
    }
}

/// The folder mark, 11 by 12 at `x`, `y`: an outlined body under a small
/// tab. A linked folder carries an arrow inside, since a click moves
/// the tree there instead of opening it in place.
fn draw_folder(painter: &mut Painter, x: f32, y: f32, color: Rgba, linked: bool) {
    painter.fill(x, y, 5.0, 2.5, 1.0, color);
    painter.stroke(x + 0.5, y + 2.5, 10.0, 8.5, 1.5, 1.2, color);
    if linked {
        let ay = y + 7.0;
        painter.line(x + 3.0, ay, x + 8.0, ay, 1.2, color);
        painter.line(x + 5.5, ay - 2.5, x + 8.0, ay, 1.2, color);
        painter.line(x + 5.5, ay + 2.5, x + 8.0, ay, 1.2, color);
    }
}

/// The type mark for one file, drawn in a 10 by 12 box at `x`, `y`.
/// Every shape is built from the painter's rectangles and lines, since
/// the UI has no icon font: the three page-shaped marks share an outline
/// and differ by the lines inside it, and the two others take their own
/// shape.
fn draw_icon(painter: &mut Painter, icon: Icon, x: f32, y: f32, color: Rgba) {
    const W: f32 = 10.0;
    const H: f32 = MARK_H;
    fn page(painter: &mut Painter, x: f32, y: f32, color: Rgba) {
        painter.stroke(x + 0.5, y + 0.5, W - 1.0, H - 1.0, 1.5, 1.2, color);
    }
    match icon {
        Icon::Document => {
            page(painter, x, y, color);
            for ly in [4.0, 7.0] {
                painter.fill(x + 2.5, y + ly, W - 5.0, 1.2, 0.6, color);
            }
        }
        Icon::Text => {
            page(painter, x, y, color);
            for ly in [3.5, 5.75, 8.0] {
                painter.fill(x + 2.5, y + ly, W - 5.0, 1.0, 0.5, color);
            }
        }
        Icon::Unknown => page(painter, x, y, color),
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

/// The color of an outline row: the accent when current, dimmed when
/// unresolved, softened below the top level, the text color otherwise.
fn outline_color(ui: &Ui, depth: usize, dead: bool, current: bool) -> Rgba {
    if current {
        ui.sidebar_dir
    } else if dead {
        dim(ui.sidebar_fg)
    } else if depth > 0 {
        soft(ui.sidebar_fg)
    } else {
        ui.sidebar_fg
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
        let mut painter = Painter::new(&mut pixmap, fonts, None, 1.0);
        side.draw(&mut painter, theme, outline, None, true);
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

    /// A link to a folder lists among the folders, and activating it
    /// moves the tree to the real folder, the way opening that folder
    /// directly would; `..` then climbs the real path.
    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_directory_lists_as_a_folder_and_is_entered() {
        let dir = temp_tree("symlink");
        std::os::unix::fs::symlink(dir.join("sub"), dir.join("linked")).unwrap();
        std::os::unix::fs::symlink(dir.join("zeta.md"), dir.join("linked.md")).unwrap();
        let mut side = Sidebar::new(&dir);
        let linked = side
            .entries
            .iter()
            .position(|e| e.name == "linked")
            .unwrap();
        assert!(side.entries[linked].is_dir && side.entries[linked].linked);
        assert!(
            side.entries
                .iter()
                .any(|e| e.name == "linked.md" && !e.is_dir),
            "a linked file is a file"
        );
        assert_eq!(
            names(&side)[..4],
            ["..", ".git", "linked", "sub"],
            "it sorts with the directories"
        );
        assert!(side.activate(linked).is_none());
        let real = std::fs::canonicalize(dir.join("sub")).unwrap();
        assert_eq!(side.root(), real, "the tree moved to the real folder");
        assert_eq!(names(&side), ["..", "subsub", "inner.md"]);
        assert_eq!(side.selected, 1, "the first entry inside is selected");
        assert!(side.activate(0).is_none());
        assert_eq!(
            side.root(),
            real.parent().unwrap(),
            "the parent row climbs the real path"
        );
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

    /// Draws the panel into a fresh canvas of `w` by `h`, the panel
    /// owning the keys, and returns the pixels.
    fn painted(
        side: &mut Sidebar,
        outline: &mut OutlineTree,
        fonts: &mut FontStore,
        theme: &Theme,
        w: u32,
        h: u32,
    ) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let mut painter = Painter::new(&mut pixmap, fonts, None, 1.0);
        side.draw(&mut painter, theme, outline, None, true);
        pixmap
    }

    fn rgba(pixmap: &Pixmap, x: f32, y: f32) -> (u8, u8, u8, u8) {
        let p = pixmap.pixel(x as u32, y as u32).unwrap();
        (p.red(), p.green(), p.blue(), p.alpha())
    }

    fn opaque(c: Rgba) -> (u8, u8, u8, u8) {
        (c.r, c.g, c.b, 255)
    }

    #[test]
    fn row_layout_indents_by_the_triangle_column() {
        let top = row_layout(0, true);
        assert_eq!(
            (top.triangle_x, top.mark_x, top.text_x),
            (PAD, PAD + INDENT, PAD + INDENT + ICON_W)
        );
        let child = row_layout(1, true);
        assert_eq!(
            child.triangle_x, top.mark_x,
            "a child's triangle sits under its parent's mark"
        );
        assert_eq!(child.text_x - top.text_x, INDENT);
        let heading = row_layout(0, false);
        assert_eq!(
            heading.text_x,
            PAD + INDENT,
            "the outline has no mark column"
        );
        assert_eq!(
            guide_x(0),
            PAD + INDENT / 2.0,
            "a guide hangs from the triangle's center"
        );
        assert_eq!(guide_x(1), PAD + INDENT + INDENT / 2.0);
    }

    #[test]
    fn outline_rows_color_by_depth_state_and_currency() {
        let ui = &Theme::default_dark().ui;
        assert_eq!(outline_color(ui, 0, false, false), ui.sidebar_fg);
        assert_eq!(outline_color(ui, 1, false, false), soft(ui.sidebar_fg));
        assert_eq!(outline_color(ui, 2, true, false), dim(ui.sidebar_fg));
        assert_eq!(outline_color(ui, 2, false, true), ui.sidebar_dir);
    }

    #[test]
    fn the_open_file_wears_the_accent_bar_and_a_nested_row_its_guide() {
        let dir = temp_tree("look");
        let mut side = Sidebar::new(&dir);
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.activate(sub).is_none());
        let at =
            |side: &Sidebar, name: &str| side.entries.iter().position(|e| e.name == name).unwrap();
        let (subsub, inner, zeta) = (
            at(&side, "subsub"),
            at(&side, "inner.md"),
            at(&side, "zeta.md"),
        );
        side.set_current(&dir.join("sub/inner.md"));
        let theme = Theme::default_dark();
        let doc = markdown::parse("");
        let mut outline = OutlineTree::build(&doc);
        let mut fonts = FontStore::new();
        let pixmap = painted(&mut side, &mut outline, &mut fonts, &theme, 260, 400);
        std::fs::remove_dir_all(&dir).unwrap();
        let row_y = |index: usize| PAD + CAPTION_H + index as f32 * ROW_H;
        assert_eq!(
            rgba(&pixmap, FILL_X + 1.0, row_y(inner) + ROW_H / 2.0),
            opaque(theme.ui.sidebar_dir),
            "the accent bar at the open file's left edge"
        );
        let bg = opaque(theme.ui.sidebar_bg);
        assert_ne!(
            rgba(&pixmap, guide_x(0), row_y(subsub) + 3.0),
            bg,
            "a nested row draws the guide of the level above it"
        );
        assert_eq!(
            rgba(&pixmap, guide_x(0), row_y(zeta) + 3.0),
            bg,
            "a top-level file has no guide and no triangle"
        );
    }

    #[test]
    fn hover_follows_the_row_under_the_mouse_and_reports_changes() {
        let dir = temp_tree("hover");
        let mut side = Sidebar::new(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        let doc = markdown::parse("");
        let outline = OutlineTree::build(&doc);
        side.list_h = 300.0;
        let top = PAD + CAPTION_H;
        assert!(side.hover_at(50.0, top + 5.0, &outline), "the first row");
        assert_eq!(side.hover, Some(Hover::Row(0)));
        assert!(
            !side.hover_at(60.0, top + 20.0, &outline),
            "the same row again changes nothing"
        );
        assert!(side.hover_at(50.0, top + ROW_H + 5.0, &outline));
        assert_eq!(side.hover, Some(Hover::Row(1)));
        assert!(
            side.hover_at(50.0, 5.0, &outline),
            "the caption row is no row"
        );
        assert_eq!(side.hover, None);
        assert!(!side.clear_hover(), "nothing to clear");
        side.hover_at(50.0, top + 5.0, &outline);
        assert!(side.clear_hover(), "leaving clears it");
        assert_eq!(side.hover, None);
        assert!(
            !side.hover_at(50.0, top + 20.0 * ROW_H, &outline),
            "past the last row there is nothing to hover"
        );
        assert_eq!(side.hover, None);
    }

    #[test]
    fn the_bar_scrolls_a_long_list_and_never_opens_a_row() {
        let dir = temp_tree("bar");
        for n in 0..40 {
            std::fs::write(dir.join(format!("f{n:02}.md")), "x").unwrap();
        }
        let mut side = Sidebar::new(&dir);
        let doc = markdown::parse("");
        let mut outline = OutlineTree::build(&doc);
        let mut fonts = FontStore::new();
        let theme = Theme::default_dark();
        let pixmap = painted(&mut side, &mut outline, &mut fonts, &theme, 260, 300);
        std::fs::remove_dir_all(&dir).unwrap();
        let top = PAD + CAPTION_H;
        let list_h = side.list_h();
        let (ty, th) = side.thumb(&outline).expect("a long list shows a thumb");
        assert_eq!(ty, 0.0, "at rest the thumb sits at the top");
        assert_eq!(
            rgba(
                &pixmap,
                side.width() - THUMB_INSET - THUMB_W / 2.0,
                top + th / 2.0
            ),
            opaque(theme.ui.scrollbar),
            "the thumb in the scrollbar color"
        );
        let click = side.click(side.width() - 8.0, top + list_h - 2.0, &mut outline);
        assert_eq!(
            click,
            SideClick::Thumb,
            "a press on the track takes the thumb"
        );
        assert!(side.scroll > 0.0, "the list jumped to the press");
        let jumped = side.scroll;
        side.drag_thumb(top + 10.0, &mut outline);
        assert!(side.scroll < jumped, "dragging the thumb up scrolls back");
        side.release();
        assert!(side.thumb_grab.is_none());
        assert!(side.hover_at(side.width() - 8.0, top + 3.0, &outline));
        assert_eq!(side.hover, Some(Hover::Thumb), "the mouse over the thumb");
    }

    #[test]
    fn a_short_list_shows_no_thumb_and_the_strip_is_a_plain_row() {
        let dir = temp_tree("short");
        let mut side = Sidebar::new(&dir);
        let doc = markdown::parse("");
        let mut outline = OutlineTree::build(&doc);
        side.list_h = 600.0;
        assert!(side.thumb(&outline).is_none());
        let top = PAD + CAPTION_H;
        let zeta = side
            .entries
            .iter()
            .position(|e| e.name == "zeta.md")
            .unwrap();
        let click = side.click(
            side.width() - 8.0,
            top + zeta as f32 * ROW_H + 5.0,
            &mut outline,
        );
        assert_eq!(click, SideClick::Open(dir.join("zeta.md")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
