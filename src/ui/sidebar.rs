//! Folder sidebar: a persistent panel listing the open file's folder as a
//! tree. Directories sort before files, both alphabetical; the listing
//! keeps every file Oryx can display. Dot entries stay out of the tree
//! until the hidden-files toggle is on, when they are drawn dimmed; the
//! file shown in the document keeps its row whatever the toggle.
//! Expansion is in place and children are read on demand.
//! A folder reached through a symbolic link is entered rather than
//! expanded: the tree moves to the real folder, as if it had been
//! opened directly. A folder the system refuses to list shows one
//! notice row in place of its entries, so the tree never goes blank.

use std::path::{Component, Path, PathBuf, Prefix};
use std::time::SystemTime;

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

/// The one row a folder shows when the system refuses to list it.
pub const UNREADABLE: &str = "This folder cannot be read";

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
    /// A notice standing in for a folder's entries, not an entry itself:
    /// drawn dimmed without a mark, opens nothing, takes no hover.
    pub notice: bool,
    /// A drive in the drives list; activating it moves the tree to the
    /// drive's top folder.
    pub drive: bool,
}

impl Entry {
    fn notice(dir: &Path, depth: usize) -> Entry {
        Entry {
            name: UNREADABLE.to_string(),
            path: dir.to_path_buf(),
            is_dir: false,
            linked: false,
            depth,
            expanded: false,
            hidden: false,
            notice: true,
            drive: false,
        }
    }
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
    /// The folder last opened or closed with a click or Enter, the
    /// accent's folder while no file is open.
    acted_dir: Option<PathBuf>,
    scroll: f32,
    list_h: f32,
    /// The row or the thumb under the mouse, from the last cursor move.
    hover: Option<Hover>,
    /// The cursor's offset from the thumb's top while the thumb is held.
    thumb_grab: Option<f32>,
    /// The modified time of every folder the tree shows, the root and
    /// the expanded ones, as last read. A folder's time moves when an
    /// entry inside it is added, removed or renamed, on every platform,
    /// so a moved stamp is the signal to read the tree again.
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    /// Whether dot entries list; off by default, the file managers'
    /// convention, and remembered in the config.
    show_hidden: bool,
    /// Whether the files tab shows the drives list, the level above a
    /// drive's top folder. The root stays the folder left, so the
    /// dialogs still open on a real folder and nothing is remembered.
    at_drives: bool,
    /// A list standing in for the system's drives, set by the tests;
    /// None reads the system's.
    fixed_drives: Option<Vec<PathBuf>>,
}

/// A folder's modified time, None when it cannot be read.
fn stamp(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir).and_then(|meta| meta.modified()).ok()
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

/// What a scan lists of the dot entries: none, or all of them, and in
/// either case the file shown in the document, which keeps its row, and
/// the dot folders on the way down to it, without which the row has
/// nowhere to stand.
#[derive(Clone, Copy)]
struct Filter<'a> {
    show_hidden: bool,
    current: Option<&'a Path>,
}

impl Filter<'_> {
    fn keeps(&self, name: &str, path: &Path) -> bool {
        self.show_hidden
            || !name.starts_with('.')
            || self.current.is_some_and(|open| open.starts_with(path))
    }
}

/// The recognized entries of one directory, directories first, both
/// groups alphabetical and case-insensitive. A symbolic link counts as
/// what it points at, so a linked folder lists among the folders. A
/// directory that cannot be read yields its notice row instead. Dot
/// entries pass through the filter.
fn scan(dir: &Path, depth: usize, filter: Filter<'_>) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![Entry::notice(dir, depth)];
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if !filter.keeps(&name, &e.path()) {
                return None;
            }
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
                notice: false,
                drive: false,
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

/// The machine's drives, a top folder per assigned letter. The call
/// reads the letters the system has assigned and touches no drive, so
/// an empty card reader or an offline network share costs nothing here.
#[cfg(windows)]
fn system_drives() -> Vec<PathBuf> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
    }
    // Bit 0 is A:, bit 25 is Z:; zero on failure lists nothing.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| PathBuf::from(format!("{}:\\", char::from(b'A' + bit))))
        .collect()
}

/// Outside Windows every drive is mounted under the one tree.
#[cfg(not(windows))]
fn system_drives() -> Vec<PathBuf> {
    Vec::new()
}

/// The letter of a drive's top folder, `C:\` or the long form
/// `\\?\C:\` that canonical paths take, uppercase; None for any other
/// path, and for every path outside Windows.
fn drive_letter(path: &Path) -> Option<char> {
    if path.parent().is_some() {
        return None;
    }
    match path.components().next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some(char::from(letter).to_ascii_uppercase())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Whether a folder is the top of a listed drive, in either form.
fn is_drive(dir: &Path, drive: &Path) -> bool {
    dir == drive || drive_letter(dir).is_some_and(|letter| drive_letter(drive) == Some(letter))
}

/// A drive's row name, its letter and colon: `C:`.
fn drive_name(drive: &Path) -> String {
    drive
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string()
}

/// Where a root's `..` row leads.
#[derive(Debug, PartialEq)]
enum Up {
    Folder(PathBuf),
    Drives,
}

/// A listed drive's top climbs to the drives list, any other folder to
/// its parent. A top with no parent that is no listed drive, a network
/// share's, climbs to the drives too when there are any; with none, as
/// at `/`, there is no `..` row.
fn up_of(root: &Path, drives: &[PathBuf]) -> Option<Up> {
    if drives.iter().any(|drive| is_drive(root, drive)) {
        return Some(Up::Drives);
    }
    match root.parent() {
        Some(parent) => Some(Up::Folder(parent.to_path_buf())),
        None => (!drives.is_empty()).then_some(Up::Drives),
    }
}

/// The visible rows for a root: a `..` row up front when the root has
/// somewhere to climb, then the root's own entries.
fn tree(root: &Path, filter: Filter<'_>, drives: &[PathBuf]) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(up) = up_of(root, drives) {
        // The row's path names where it leads; the drives list has no
        // folder, so that row names the root it climbs from.
        let path = match up {
            Up::Folder(parent) => parent,
            Up::Drives => root.to_path_buf(),
        };
        entries.push(Entry {
            name: "..".to_string(),
            path,
            is_dir: true,
            linked: false,
            depth: 0,
            expanded: false,
            hidden: false,
            notice: false,
            drive: false,
        });
    }
    entries.extend(scan(root, 0, filter));
    entries
}

/// The drives list: a row per drive, in letter order.
fn drive_rows(drives: &[PathBuf]) -> Vec<Entry> {
    drives
        .iter()
        .map(|drive| Entry {
            name: drive_name(drive),
            path: drive.clone(),
            is_dir: true,
            linked: false,
            depth: 0,
            expanded: false,
            hidden: false,
            notice: false,
            drive: true,
        })
        .collect()
}

impl Sidebar {
    pub fn new(root: &Path) -> Sidebar {
        let filter = Filter {
            show_hidden: false,
            current: None,
        };
        let mut side = Sidebar {
            width: DEFAULT_WIDTH,
            tab: Tab::Files,
            root: root.to_path_buf(),
            entries: tree(root, filter, &system_drives()),
            selected: 0,
            current: None,
            acted_dir: None,
            scroll: 0.0,
            list_h: 0.0,
            hover: None,
            thumb_grab: None,
            stamps: Vec::new(),
            show_hidden: false,
            at_drives: false,
            fixed_drives: None,
        };
        side.restamp();
        side
    }

    /// The scan's filter as the panel stands: the toggle, and the file
    /// shown in the document, which keeps its row whatever the toggle.
    fn filter(&self) -> Filter<'_> {
        Filter {
            show_hidden: self.show_hidden,
            current: self.current.as_deref(),
        }
    }

    fn drives(&self) -> Vec<PathBuf> {
        match &self.fixed_drives {
            Some(list) => list.clone(),
            None => system_drives(),
        }
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Flips the dot entries in or out of the tree, the tree read again
    /// on the same root: the expanded folders that still exist stay
    /// expanded, the selection follows its path where it is still
    /// listed, the scroll and the current mark stay.
    pub fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden == show {
            return;
        }
        self.show_hidden = show;
        self.rebuild();
    }

    /// Reads the tree again on the same root with the panel's filter,
    /// keeping what a reader would expect kept: the expanded folders
    /// still there, the selection by path or in range, the scroll.
    fn rebuild(&mut self) {
        let expanded: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.expanded)
            .map(|e| e.path.clone())
            .collect();
        let selected = self.entries.get(self.selected).map(|e| e.path.clone());
        let fallback = self.selected;
        // The hover names a row by its place in the old list; the next
        // pointer move finds the row under it again.
        if matches!(self.hover, Some(Hover::Row(_))) {
            self.hover = None;
        }
        self.entries = if self.at_drives {
            drive_rows(&self.drives())
        } else {
            tree(&self.root, self.filter(), &self.drives())
        };
        // Parents come before their children in the list, so each
        // folder is found once its parent has been expanded again.
        for dir in expanded {
            if let Some(index) = self
                .entries
                .iter()
                .position(|e| e.is_dir && !e.expanded && e.path == dir)
            {
                self.toggle_dir(index);
            }
        }
        let last = self.entries.len().saturating_sub(1);
        self.selected = selected
            .and_then(|path| self.entries.iter().position(|e| e.path == path))
            .unwrap_or(fallback.min(last));
        self.restamp();
    }

    /// Reads the stamps of the shown folders, after any change to the
    /// set: the root, then every expanded folder. The drives list shows
    /// no folder; its rows are compared with the drives instead.
    fn restamp(&mut self) {
        if self.at_drives {
            self.stamps.clear();
            return;
        }
        let mut folders = vec![self.root.clone()];
        folders.extend(
            self.entries
                .iter()
                .filter(|e| e.expanded)
                .map(|e| e.path.clone()),
        );
        self.stamps = folders
            .into_iter()
            .map(|dir| {
                let seen = stamp(&dir);
                (dir, seen)
            })
            .collect();
    }

    /// Reads the tree again when a shown folder changed on disk since
    /// the last read, and answers whether it did. The expanded folders
    /// that still exist stay expanded, the selection follows its path,
    /// the scroll and the current file's mark stay. One stat per shown
    /// folder when nothing changed. Called from the app's disk check,
    /// so it runs on interaction and on focus, never while idle. The
    /// kernel stamps a folder with its coarse clock, so two changes a
    /// few milliseconds apart can share a stamp; the next change is
    /// caught, and a person's actions are never that close.
    pub fn refresh_if_changed(&mut self) -> bool {
        let moved = if self.at_drives {
            !self
                .entries
                .iter()
                .map(|e| &e.path)
                .eq(self.drives().iter())
        } else {
            self.stamps.iter().any(|(dir, seen)| stamp(dir) != *seen)
        };
        if !moved {
            return false;
        }
        self.rebuild();
        true
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
        self.entries = tree(parent, self.filter(), &self.drives());
        self.scroll = 0.0;
        self.selected = self
            .entries
            .iter()
            .position(|e| e.path == left)
            .unwrap_or(0);
        self.scroll_to_selection();
        self.restamp();
    }

    /// Shows the drives list with the drive just left selected, or the
    /// first drive when the root was a network share's.
    fn show_drives(&mut self) {
        let drives = self.drives();
        self.at_drives = true;
        self.entries = drive_rows(&drives);
        self.scroll = 0.0;
        self.selected = drives
            .iter()
            .position(|drive| is_drive(&self.root, drive))
            .unwrap_or(0);
        self.scroll_to_selection();
        self.restamp();
    }

    /// Rebuilds the tree at the real folder a path names, a link's
    /// target or a drive's top, in the canonical form every other path
    /// in the app takes, the first entry inside it selected; `..` climbs
    /// the real path from there.
    fn enter_real(&mut self, dir: &Path) {
        let target = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        self.at_drives = false;
        self.root = target.clone();
        self.entries = tree(&target, self.filter(), &self.drives());
        self.scroll = 0.0;
        let first = usize::from(self.entries.first().is_some_and(|e| e.name == ".."));
        self.selected = first.min(self.entries.len().saturating_sub(1));
        self.restamp();
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the tree shows this folder at its top, and not the
    /// drives list above it.
    pub fn shows(&self, dir: &Path) -> bool {
        !self.at_drives && self.root == dir
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
            let children = scan(&path, depth + 1, self.filter());
            if self.selected > index {
                self.selected += children.len();
            }
            self.entries.splice(index + 1..index + 1, children);
        }
        self.entries[index].expanded = !expanded;
        self.restamp();
    }

    /// A row was chosen: a file returns its path to open, a directory
    /// toggles its expansion, a linked directory or a drive becomes the
    /// root, a notice does nothing.
    pub fn activate(&mut self, index: usize) -> Option<PathBuf> {
        let entry = self.entries.get(index)?;
        self.selected = index;
        if index == 0 && entry.name == ".." {
            match up_of(&self.root, &self.drives()) {
                Some(Up::Folder(parent)) => self.go_up(&parent),
                Some(Up::Drives) => self.show_drives(),
                None => {}
            }
            None
        } else if entry.notice {
            None
        } else if entry.linked || entry.drive {
            let dir = entry.path.clone();
            self.enter_real(&dir);
            None
        } else if entry.is_dir {
            self.acted_dir = Some(entry.path.clone());
            self.toggle_dir(index);
            None
        } else {
            Some(entry.path.clone())
        }
    }

    /// The folder the accent names in the tree: the open file's, or,
    /// while no file is open, the folder last opened or closed with a
    /// click or Enter. Its name and mark take the accent color; the
    /// accent fill stays the open file's own.
    pub fn accent_dir(&self) -> Option<&Path> {
        match &self.current {
            Some(file) => file.parent(),
            None => self.acted_dir.as_deref(),
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

    /// The file a row shows, or None for a folder, the parent row and
    /// the notice: what a second window can open.
    fn file_of(&self, index: usize) -> Option<PathBuf> {
        let entry = self.entries.get(index)?;
        let parent = index == 0 && entry.name == "..";
        (!entry.is_dir && !entry.notice && !parent).then(|| entry.path.clone())
    }

    /// The file under a point of the files tab, without touching the
    /// selection or the tree; None off the rows or on the outline tab.
    pub fn file_at(&self, x: f32, y: f32) -> Option<PathBuf> {
        if self.tab != Tab::Files || x < 0.0 || x >= self.width - STRIP_W {
            return None;
        }
        let top = PAD + CAPTION_H;
        if y < top || y >= top + self.list_h {
            return None;
        }
        let index = ((y - top + self.scroll) / ROW_H).floor();
        if index < 0.0 {
            return None;
        }
        self.file_of(index as usize)
    }

    /// The file of the selected row, if the selection is on one.
    pub fn selected_file(&self) -> Option<PathBuf> {
        self.file_of(self.selected)
    }

    /// Marks the file shown in the document area. A dot file kept out
    /// by the toggle gets its row back, alone among the hidden entries,
    /// so the open file always has one.
    pub fn set_current(&mut self, path: &Path) {
        self.current = Some(path.to_path_buf());
        let listed = self.entries.iter().position(|e| e.path == path);
        let hidden_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if listed.is_none() && hidden_name && !self.show_hidden {
            self.rebuild();
        }
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
        let accent_dir = self.accent_dir().map(Path::to_path_buf);
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
            let current = !entry.notice && self.current.as_deref() == Some(entry.path.as_path());
            // A notice takes the selection fill, so the keys stay visible,
            // but no hover fill: there is nothing to click.
            let attended = (!entry.notice && self.hover == Some(Hover::Row(index)))
                || (owns_keys && index == self.selected);
            draw_row_ground(painter, width, ry, current, attended, fg, accent);
            for level in 0..entry.depth {
                draw_guide(painter, level, ry, fg);
            }
            let layout = row_layout(entry.depth, true);
            // The accent's folder reads in the accent color, its name
            // and its mark, with no fill: the fill is the open file's.
            let accent_folder = entry.is_dir
                && !(index == 0 && entry.name == "..")
                && accent_dir.as_deref() == Some(entry.path.as_path());
            let mut color = if current || accent_folder { accent } else { fg };
            if entry.hidden || entry.notice {
                color = dim(color);
            }
            let iy = ry + (ROW_H - MARK_H) / 2.0;
            if entry.notice {
                // The sentence starts where a mark would, so it reads as
                // a notice about the folder rather than as one of its rows.
                let avail = width - layout.mark_x - STRIP_W;
                let text = truncated(painter, &entry.name, avail, 400);
                painter.text(
                    layout.mark_x,
                    ry + 5.0,
                    &text,
                    BODY_FAMILY,
                    TEXT_SIZE,
                    400,
                    color,
                );
                continue;
            }
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
    fn the_accent_folder_is_the_open_files_or_the_last_one_clicked() {
        let dir = temp_tree("accent-dir");
        let mut side = Sidebar::new(&dir);
        assert_eq!(side.accent_dir(), None, "nothing open, nothing clicked");
        let sub = names(&side).iter().position(|n| n == "sub").unwrap();
        side.activate(sub);
        assert_eq!(
            side.accent_dir(),
            Some(dir.join("sub").as_path()),
            "with no file open, the folder last clicked"
        );
        side.activate(sub);
        assert_eq!(
            side.accent_dir(),
            Some(dir.join("sub").as_path()),
            "closing it again keeps it the last one clicked"
        );
        side.set_current(&dir.join("zeta.md"));
        assert_eq!(
            side.accent_dir(),
            Some(dir.as_path()),
            "the open file's folder wins over the click"
        );
        side.set_current(&dir.join("sub/inner.md"));
        assert_eq!(side.accent_dir(), Some(dir.join("sub").as_path()));
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

    /// A sidebar showing its dot entries, for the tests written before
    /// the toggle, when every entry was listed.
    fn showing_all(root: &Path) -> Sidebar {
        let mut side = Sidebar::new(root);
        side.set_show_hidden(true);
        side
    }

    #[test]
    fn scan_orders_directories_first_both_alphabetical() {
        let dir = temp_tree("order");
        let side = showing_all(&dir);
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

    /// A sidebar on a machine whose drives are the given folders, in
    /// place of the system's list.
    fn with_drives(root: &Path, drives: &[PathBuf]) -> Sidebar {
        let mut side = Sidebar::new(root);
        side.fixed_drives = Some(drives.to_vec());
        side.rebuild();
        side
    }

    /// Two folders standing in for a Windows machine's C: and D:.
    fn two_drives(name: &str) -> (PathBuf, PathBuf) {
        let c = temp_tree(&format!("{name}-c"));
        let d = temp_tree(&format!("{name}-d"));
        (c, d)
    }

    fn paths(side: &Sidebar) -> Vec<PathBuf> {
        side.entries.iter().map(|e| e.path.clone()).collect()
    }

    #[test]
    fn the_parent_row_follows_the_machine() {
        let drives = [PathBuf::from("/drive-c"), PathBuf::from("/drive-d")];
        assert_eq!(up_of(Path::new("/"), &[]), None, "one tree, nothing above");
        assert_eq!(
            up_of(Path::new("/"), &drives),
            Some(Up::Drives),
            "a top that is no listed drive, a network share's"
        );
        assert_eq!(up_of(&drives[1], &drives), Some(Up::Drives));
        assert_eq!(
            up_of(Path::new("/drive-c/notes"), &drives),
            Some(Up::Folder(PathBuf::from("/drive-c")))
        );
    }

    #[test]
    fn a_drive_row_is_named_by_its_letter() {
        assert_eq!(drive_name(Path::new("C:\\")), "C:");
    }

    #[test]
    fn a_drives_top_climbs_to_the_drives_with_the_drive_left_selected() {
        let (c, d) = two_drives("drives-up");
        let mut side = with_drives(&d, &[c.clone(), d.clone()]);
        assert_eq!(side.entries[0].name, "..", "a drive's top keeps its row");
        assert!(side.activate(0).is_none());
        assert_eq!(paths(&side), [c.clone(), d.clone()], "a row per drive");
        assert_eq!(side.selected, 1, "the drive just left");
        assert_eq!(side.root(), d.as_path(), "the root stays a real folder");
        assert!(!side.shows(&d), "the tree no longer shows it");
        std::fs::remove_dir_all(&c).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_drive_row_opens_the_drive_at_its_top() {
        let (c, d) = two_drives("drives-open");
        let mut side = with_drives(&c, &[c.clone(), d.clone()]);
        assert!(side.activate(0).is_none());
        assert!(side.activate(1).is_none(), "a drive opens no file");
        let top = d.canonicalize().unwrap();
        assert_eq!(side.root(), top.as_path());
        assert!(side.shows(&top));
        assert_eq!(side.entries[0].name, "..");
        assert!(names(&side).contains(&"zeta.md".to_string()));
        assert_eq!(side.selected, 1, "the first entry inside");
        assert!(side.activate(0).is_none());
        assert_eq!(paths(&side), [c.clone(), d.clone()], "and back up");
        std::fs::remove_dir_all(&c).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_folder_inside_a_drive_climbs_to_its_parent_as_before() {
        let (c, d) = two_drives("drives-inner");
        let mut side = with_drives(&c.join("sub"), &[c.clone(), d.clone()]);
        assert!(side.activate(0).is_none());
        assert_eq!(side.root(), c.as_path());
        assert!(side.shows(&c));
        assert!(names(&side).contains(&"zeta.md".to_string()));
        std::fs::remove_dir_all(&c).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The disk check reads the drives again while their list shows,
    /// so a stick plugged in or pulled out is seen.
    #[test]
    fn the_drives_list_follows_a_drive_plugged_in_or_out() {
        let (c, d) = two_drives("drives-plug");
        let mut side = with_drives(&c, std::slice::from_ref(&c));
        assert!(side.activate(0).is_none());
        assert!(!side.refresh_if_changed(), "nothing changed yet");
        side.fixed_drives = Some(vec![c.clone(), d.clone()]);
        assert!(side.refresh_if_changed(), "a drive plugged in");
        assert_eq!(paths(&side), [c.clone(), d.clone()]);
        assert!(!side.refresh_if_changed(), "seen once");
        side.fixed_drives = Some(vec![c.clone()]);
        assert!(side.refresh_if_changed(), "a drive pulled out");
        assert_eq!(paths(&side), std::slice::from_ref(&c));
        std::fs::remove_dir_all(&c).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The drive left is the root, and a change inside it is no reason
    /// to leave the drives list.
    #[test]
    fn the_drives_list_stays_through_a_change_on_the_drive_left() {
        let (c, d) = two_drives("drives-stay");
        let mut side = with_drives(&c, &[c.clone(), d.clone()]);
        assert!(side.activate(0).is_none());
        settle();
        std::fs::write(c.join("brand-new.md"), "x").unwrap();
        assert!(!side.refresh_if_changed());
        side.set_show_hidden(true);
        assert_eq!(paths(&side), [c.clone(), d.clone()], "the toggle too");
        assert_eq!(side.selected, 0);
        std::fs::remove_dir_all(&c).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
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
        let mut side = showing_all(&dir);
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
        let mut side = showing_all(&dir);
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

    /// A folder the system refuses to list shows one notice row under
    /// `..`, so the reader can climb out; the notice opens nothing.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_shows_a_notice_under_the_parent_row() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_tree("unreadable");
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("secret.md"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            // Running as root: nothing is unreadable, nothing to prove.
            std::fs::remove_dir_all(&dir).unwrap();
            return;
        }
        let mut side = Sidebar::new(&locked);
        assert_eq!(names(&side), ["..", UNREADABLE]);
        assert!(side.entries[1].notice);
        assert!(side.activate(1).is_none(), "the notice opens nothing");
        assert_eq!(side.root(), locked.as_path(), "and moves nowhere");
        assert!(side.activate(0).is_none());
        assert_eq!(
            side.root(),
            dir.as_path(),
            "the parent row still climbs out"
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subfolder_expands_to_its_notice_and_collapses_clean() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_tree("unreadable-sub");
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            std::fs::remove_dir_all(&dir).unwrap();
            return;
        }
        let mut side = Sidebar::new(&dir);
        let row = side
            .entries
            .iter()
            .position(|e| e.name == "locked")
            .unwrap();
        assert!(side.activate(row).is_none());
        assert_eq!(side.entries[row + 1].name, UNREADABLE);
        assert_eq!(
            side.entries[row + 1].depth,
            1,
            "the notice sits inside the folder"
        );
        assert!(side.entries[row + 2].depth == 0, "and nothing else joined");
        assert!(side.activate(row).is_none());
        assert_ne!(
            side.entries[row + 1].name,
            UNREADABLE,
            "collapsing removes it"
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The kernel stamps a folder with its coarse clock, a few
    /// milliseconds wide: two changes inside one tick share a stamp,
    /// so a test lets a tick pass between a read and the next change.
    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    /// The tree follows the disk on the disk check: a file added to a
    /// shown folder appears, one removed goes, and an unchanged folder
    /// costs nothing but a stat.
    #[test]
    fn a_refresh_shows_a_file_added_and_drops_one_removed() {
        let dir = temp_tree("refresh-add");
        let mut side = Sidebar::new(&dir);
        assert!(!side.refresh_if_changed(), "nothing changed yet");
        settle();
        std::fs::write(dir.join("brand-new.md"), "x").unwrap();
        assert!(side.refresh_if_changed(), "the root's stamp moved");
        assert!(names(&side).contains(&"brand-new.md".to_string()));
        assert!(!side.refresh_if_changed(), "seen once");
        settle();
        std::fs::remove_file(dir.join("brand-new.md")).unwrap();
        assert!(side.refresh_if_changed());
        assert!(!names(&side).contains(&"brand-new.md".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refresh_keeps_the_selection_by_path_and_the_expanded_folders() {
        let dir = temp_tree("refresh-keep");
        let mut side = Sidebar::new(&dir);
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.activate(sub).is_none());
        assert!(side.entries[sub].expanded);
        side.selected = side
            .entries
            .iter()
            .position(|e| e.name == "zeta.md")
            .unwrap();
        settle();
        std::fs::remove_file(dir.join("Alpha.rs")).unwrap();
        assert!(side.refresh_if_changed());
        assert_eq!(
            side.entries[side.selected].name, "zeta.md",
            "the selection follows its file"
        );
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.entries[sub].expanded, "still expanded");
        assert_eq!(side.entries[sub + 1].name, "subsub", "with its children");
        settle();
        std::fs::write(dir.join("sub/late.md"), "x").unwrap();
        assert!(
            side.refresh_if_changed(),
            "an expanded folder's stamp counts too"
        );
        assert!(names(&side).contains(&"late.md".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refresh_drops_an_expanded_folder_that_vanished() {
        let dir = temp_tree("refresh-gone");
        let mut side = Sidebar::new(&dir);
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.activate(sub).is_none());
        side.selected = side
            .entries
            .iter()
            .position(|e| e.name == "inner.md")
            .unwrap();
        settle();
        std::fs::remove_dir_all(dir.join("sub")).unwrap();
        assert!(side.refresh_if_changed());
        assert!(!names(&side).contains(&"sub".to_string()));
        assert!(!names(&side).contains(&"inner.md".to_string()));
        assert!(
            side.selected < side.entries.len(),
            "the selection stays in range"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A middle click asks for the file under the pointer without
    /// opening it in place: a file row answers its path, a folder row,
    /// the parent row and the space below the list answer nothing.
    #[test]
    fn the_file_under_a_point_answers_only_for_a_file_row() {
        let dir = temp_tree("file-at");
        let mut side = Sidebar::new(&dir);
        side.list_h = 600.0;
        let row_y = |index: usize| PAD + CAPTION_H + index as f32 * ROW_H + 5.0;
        let zeta = side
            .entries
            .iter()
            .position(|e| e.name == "zeta.md")
            .unwrap();
        assert_eq!(side.file_at(40.0, row_y(zeta)), Some(dir.join("zeta.md")));
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert_eq!(side.file_at(40.0, row_y(sub)), None, "a folder");
        assert_eq!(side.file_at(40.0, row_y(0)), None, "the parent row");
        assert_eq!(
            side.file_at(40.0, row_y(side.entries.len() + 2)),
            None,
            "below the list"
        );
        assert_eq!(side.file_at(40.0, 10.0), None, "the caption row");
        side.selected = zeta;
        assert_eq!(side.selected_file(), Some(dir.join("zeta.md")));
        side.selected = sub;
        assert_eq!(side.selected_file(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Dot entries stay out of the tree until the toggle is on, the
    /// parent row untouched; on, they list flagged hidden, dimmed.
    #[test]
    fn hidden_entries_stay_out_until_the_toggle() {
        let dir = temp_tree("hidden-off");
        let mut side = Sidebar::new(&dir);
        assert!(!side.show_hidden(), "off by default");
        assert_eq!(
            names(&side),
            [
                "..",
                "sub",
                "Alpha.rs",
                "Cargo.lock",
                "notes.txt",
                "README",
                "zeta.md"
            ]
        );
        side.set_show_hidden(true);
        assert!(names(&side).contains(&".git".to_string()));
        assert!(names(&side).contains(&".gitignore".to_string()));
        assert!(side.entries.iter().filter(|e| e.hidden).count() == 2);
        side.set_show_hidden(false);
        assert!(!names(&side).contains(&".git".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The file shown in the document keeps its row whatever the
    /// toggle, alone among the dot entries, and the selection sits on it.
    #[test]
    fn the_open_file_stays_listed_while_hidden_entries_are_off() {
        let dir = temp_tree("hidden-current");
        let mut side = Sidebar::new(&dir);
        side.set_current(&dir.join(".gitignore"));
        let row = side
            .entries
            .iter()
            .position(|e| e.name == ".gitignore")
            .expect("the open file is listed");
        assert!(side.entries[row].hidden, "still drawn dimmed");
        assert_eq!(side.selected, row);
        assert!(
            !names(&side).contains(&".git".to_string()),
            "the others stay out"
        );
        side.set_show_hidden(true);
        side.set_show_hidden(false);
        assert!(
            names(&side).contains(&".gitignore".to_string()),
            "through a toggle too"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_toggle_keeps_the_selection_on_its_path_and_the_expanded_folders() {
        let dir = temp_tree("hidden-toggle");
        std::fs::write(dir.join("sub/.secret.md"), "x").unwrap();
        let mut side = Sidebar::new(&dir);
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.activate(sub).is_none());
        assert!(
            !names(&side).contains(&".secret.md".to_string()),
            "filtered inside too"
        );
        side.selected = side
            .entries
            .iter()
            .position(|e| e.name == "zeta.md")
            .unwrap();
        side.set_show_hidden(true);
        assert_eq!(side.entries[side.selected].name, "zeta.md");
        let sub = side.entries.iter().position(|e| e.name == "sub").unwrap();
        assert!(side.entries[sub].expanded, "still expanded");
        assert!(names(&side).contains(&".secret.md".to_string()));
        side.set_show_hidden(false);
        assert_eq!(side.entries[side.selected].name, "zeta.md");
        assert!(!names(&side).contains(&".secret.md".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_open_file_inside_a_dot_folder_keeps_its_row_through_a_toggle() {
        let dir = temp_tree("hidden-inside");
        std::fs::write(dir.join(".git/notes.md"), "x").unwrap();
        let mut side = Sidebar::new(&dir);
        side.set_show_hidden(true);
        let git = side.entries.iter().position(|e| e.name == ".git").unwrap();
        assert!(side.activate(git).is_none());
        side.set_current(&dir.join(".git/notes.md"));
        side.hover = Some(Hover::Row(1));
        side.set_show_hidden(false);
        assert!(
            side.hover.is_none(),
            "a hover from the old list marks no row of the new one"
        );
        let row = side
            .entries
            .iter()
            .position(|e| e.path == dir.join(".git/notes.md"))
            .expect("the open file keeps its row, and its folder with it");
        assert_eq!(side.selected, row, "the selection stays on the open file");
        assert!(
            !names(&side).contains(&".gitignore".to_string()),
            "the other dot entries stay out"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
