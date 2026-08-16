use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oryx::doc::epub;
use oryx::doc::images::{self, MediaCache, Waker};
use oryx::doc::load;
use oryx::doc::model::{BlockKind, Document};
use oryx::doc::stream::{self, ParseWorker};
use oryx::edit::{
    self,
    caret::{self, Caret, CaretBox, Motion},
    splice::{self, Ledger},
    undo::{Kind, Undo},
};
use oryx::export::{self, ExportPass, ExportSettings};
use oryx::input::{
    self,
    keymap::{self, Command},
    touch,
};
use oryx::layout::{
    self, layout_begin, layout_extend, layout_more, metrics, recolor_batch, window_to, DecoRect,
    LayoutDoc, LayoutPass, ShapePool, ViewConfig, OPEN_SLICE, SLICE,
};
use oryx::paint;
use oryx::paint::painter::Painter;
use oryx::paint::scroll::{self, BandCache};
use oryx::platform::config::{self, Config, WindowState};
use oryx::platform::save;
use oryx::style::fonts::FontStore;
use oryx::style::highlight::{self, Highlighter, PendingBlock};
use oryx::style::theme::{self, Rgba, Theme};
use oryx::ui::confirm;
use oryx::ui::export::{ExportDialog, ExportProgress};
use oryx::ui::help;
use oryx::ui::notice::{self, Notice};
use oryx::ui::outline::{entry_offset, OutlineTree};
use oryx::ui::overlay::{Action, Overlay, OverlayResult};
use oryx::ui::scrollbar;
use oryx::ui::search::{self, BarHit, ReplaceRow, SearchState};
use oryx::ui::selection::{self, ModelPos, Selection};
use oryx::ui::settings::{self, Settings};
use oryx::ui::sidebar::{self, Sidebar};
use oryx::ui::textfield::{Edit, TextField};
use oryx::ui::theme_browser::ThemeBrowser;
use oryx::ui::theme_editor::ThemeEditor;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// How often owed recolors land while highlights wash in.
const RECOLOR_WAVE: Duration = Duration::from_millis(400);

/// How often a fling advances the scroll.
const FLING_TICK: Duration = Duration::from_millis(16);

/// The caret's blink half-period; every caret action restarts the
/// visible half.
const CARET_BLINK: Duration = Duration::from_millis(530);

/// Typing rest before the highlight worker re-runs the edited block;
/// each keystroke kills in-flight work through the generation, so the
/// whole-block pass runs once per pause, never per key.
const REHIGHLIGHT_REST: Duration = Duration::from_millis(400);

/// How long after a touch pan the emulated mouse stays ignored, long
/// enough to cover the click Windows synthesizes behind a lifted finger.
const MOUSE_MUTE: Duration = Duration::from_millis(150);

/// The window icon raster produced by the build script.
const ICON_64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_64.rgba"));

pub fn run(path: Option<PathBuf>, theme_name: Option<String>) -> anyhow::Result<()> {
    // One form of the path for the whole session. Everything keyed on
    // it, the edit marks, the resume note, the disk identity, has to
    // match what `open_file` stores for every later file, and that is
    // the canonical form; a path as typed on the command line would
    // key the first file differently from the same file reopened.
    let path = path.map(|p| p.canonicalize().unwrap_or(p));
    let (document, pending, streamed, book, book_toc, lossy, opened_crlf) = match &path {
        Some(p) => {
            let opened = load::open(p, Some(Instant::now() + load::OPEN_BUDGET))?;
            (
                opened.document,
                opened.pending,
                opened.streamed,
                opened.book,
                opened.toc,
                opened.lossy,
                opened.crlf,
            )
        }
        None => (
            Document::default(),
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
            Vec::new(),
        ),
    };
    // Absolute from here on: a bare relative name like `README.md` has the
    // empty string as parent, which breaks the sidebar root and the dialog.
    let path = path.map(|p| p.canonicalize().unwrap_or(p));
    let doc_dir = path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let event_loop = EventLoop::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    // Background fetches wake the loop so arrived pixels trigger a
    // relayout without polling.
    let waker: Waker = Arc::new(move || {
        let _ = proxy.send_event(());
    });
    let mut media = MediaCache::new(doc_dir.clone());
    media.set_waker(waker.clone());
    let mut highlighter = Highlighter::new();
    {
        let waker = waker.clone();
        highlighter.start(pending, move || waker());
    }
    let mut parser = ParseWorker::new();
    // A book starts its worker through `start_book` once the app owns
    // the media cache; only the markdown prefix starts here.
    if streamed && book.is_none() {
        let waker = waker.clone();
        parser.start(document.source.clone(), move || waker());
    }
    let mut config = config::load();
    if path.is_some() {
        let dir_text = doc_dir.display().to_string();
        if !dir_text.is_empty() && config.last_dir != dir_text {
            config.last_dir = dir_text;
            config::save(&config);
        }
    }
    let cfg = ViewConfig {
        body_family: config.body_family.clone(),
        code_family: config.code_family.clone(),
        body_size: config.body_size,
        code_size: config.code_size,
        justify: justify_pref(&config, &document),
        ..ViewConfig::default()
    };
    let theme_choice = theme_name.as_deref().unwrap_or(&config.theme);
    // At least one theme file always exists: an emptied collection is
    // reseeded from the compiled palette before resolution runs.
    if let Some(base) = directories::BaseDirs::new() {
        let target = base.data_dir().join("oryx/themes");
        if let Err(err) = theme::seed(&theme_dirs(), &target) {
            eprintln!("oryx: cannot seed themes: {err}");
        }
    }
    let fonts = FontStore::new();
    let pool_width = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .clamp(1, 8);
    let pool = Arc::new(ShapePool::new(pool_width, &fonts.seed()));
    let outline = if book_toc.is_empty() {
        OutlineTree::build(&document)
    } else {
        OutlineTree::from_toc(&book_toc, &document)
    };
    let mut app = App {
        gfx: None,
        document,
        path: path.clone(),
        theme: startup_theme(Some(theme_choice)),
        cfg,
        config,
        fonts,
        pool,
        media,
        waker,
        highlighter,
        parser,
        pending_recolor: Vec::new(),
        last_recolor: Instant::now(),
        parse_pending: streamed,
        layout: None,
        pass: None,
        last_pass: Duration::ZERO,
        settle_at: None,
        pass_spent: Duration::ZERO,
        pending_scroll: None,
        pending_anchor: None,
        pending_offset: None,
        book_toc,
        positions: config::Positions::load(),
        layout_width: 0.0,
        scale: 1.0,
        os_scale: 1.0,
        user_zoom: 1.0,
        touch: touch::Tracker::new(1.0),
        pan_target: PanTarget::Document,
        fling: None,
        mute_mouse_until: None,
        pinch_base: 1.0,
        band: None,
        scroll_y: 0.0,
        modifiers: ModifiersState::empty(),
        cursor: PhysicalPosition::new(0.0, 0.0),
        drag: None,
        last_edge_click: None,
        hover_edge: false,
        hover_link: false,
        sel_anchor: None,
        last_click: None,
        selection: None,
        clipboard: None,
        overlay: None,
        overlay_mouse: false,
        export: None,
        export_warning: None,
        view_dirty: false,
        pre_edit: None,
        pre_browse: None,
        overlay_canvas: None,
        sidebar: None,
        key_pane: KeyPane::Document,
        outline,
        sidebar_canvas: None,
        search: None,
        search_canvas: None,
        last_query: String::new(),
        last_regex: false,
        last_replace: String::new(),
        search_mouse: false,
        pending_band_for: None,
        mode: edit::Mode::Read,
        caret: None,
        lossy,
        edit_marks: HashMap::new(),
        read_marks: HashMap::new(),
        ledger: None,
        undo: None,
        rehighlight_at: None,
        seams: HashMap::new(),
        spec_band: None,
        bottom_hold: scroll::BottomHold::default(),
        confirm: None,
        help_stash: None,
        edit_park: None,
        resume_edit: HashSet::new(),
        pending_row: None,
        disk_seen: None,
        disk_check_at: Instant::now(),
        crlf: opened_crlf,
        caret_snap: false,
        blink_visible: true,
        blink_flip: Instant::now(),
        notice: None,
        notice_canvas: None,
    };
    if let Some(job) = book {
        app.start_book(job);
    }
    app.pending_offset = app
        .document
        .book_id
        .as_deref()
        .and_then(|key| app.positions.lookup(key));
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Window title: the open file's name, path stripped.
/// The outline entry carrying the reading-position highlight, with a
/// minimal list scroll when the section changes. Free-standing so the
/// redraw can call it under its buffer borrows.
fn outline_current(
    outline: &mut OutlineTree,
    side: &Sidebar,
    lay: &LayoutDoc,
    scroll_y: f32,
) -> Option<usize> {
    if side.tab() != sidebar::Tab::Outline {
        return None;
    }
    let entry = outline
        .current_of(scroll_y, |b| {
            (b != usize::MAX).then(|| lay.approx_top(b, 0)).flatten()
        })
        .map(|e| outline.visible_entry(e));
    outline.track_current(entry, sidebar::ROW_H, side.list_h());
    entry
}

/// A link target naming a file on disk: resolved against the document's
/// folder and split from its `#fragment`. Anchors, URLs, schemes, and
/// paths that do not exist answer None. `%20` decodes, the one escape
/// README links carry in practice.
fn file_link_target(target: &str, base: Option<&Path>) -> Option<(PathBuf, Option<String>)> {
    if target.starts_with('#') || target.contains("://") || target.contains(':') {
        return None;
    }
    let (path_part, fragment) = match target.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (target, None),
    };
    if path_part.is_empty() {
        return None;
    }
    let path = base?.join(path_part.replace("%20", " "));
    path.is_file().then_some((path, fragment))
}

/// Fills the caret bar into a 0RGB frame, clipped to the viewport. The
/// box arrives in document space; the frame starts at the sidebar inset
/// and scrolls by `scroll_y`.
#[allow(clippy::too_many_arguments)]
fn draw_caret(
    frame: &mut [u32],
    width: u32,
    height: u32,
    inset: u32,
    scroll_y: f32,
    scale: f32,
    caret: CaretBox,
    color: Rgba,
) {
    let value = ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
    let bar = ((1.5 * scale).round() as u32).max(1);
    let x0 = (caret.x.max(0.0) as u32).saturating_add(inset).min(width);
    let x1 = x0.saturating_add(bar).min(width);
    let top = caret.y - scroll_y;
    let y0 = (top.max(0.0) as u32).min(height);
    let y1 = ((top + caret.h).max(0.0) as u32).min(height);
    for y in y0..y1 {
        let row = (y * width) as usize;
        for x in x0..x1 {
            frame[row + x as usize] = value;
        }
    }
}

/// A book's `dc:title` wins over the file name; files have no title.
/// A dirty buffer carries the leading asterisk.
/// Everything the help page displaces, moved back verbatim on return.
/// The rendered page set aside while its source is edited. A return
/// that changed no byte puts it back whole, so looking at the source
/// and coming out costs nothing at any file size.
struct Parked {
    document: Document,
    layout: Option<LayoutDoc>,
    pass: Option<LayoutPass>,
    scroll_y: f32,
    layout_width: f32,
    /// The undo head at the crossing; an equal head on the way out
    /// means equal bytes, so the parked page is still the truth.
    head: usize,
    /// The look the parked layout was built under. A theme or zoom
    /// change while editing drops the layout and keeps the model.
    zoom: f32,
    theme: String,
}

struct Stash {
    path: Option<PathBuf>,
    document: Document,
    ledger: Option<Ledger>,
    undo: Option<Undo>,
    crlf: Vec<u32>,
    lossy: bool,
    mode: edit::Mode,
    caret: Option<Caret>,
    scroll_y: f32,
    layout: Option<LayoutDoc>,
    pass: Option<LayoutPass>,
    layout_width: f32,
    media: MediaCache,
    book_toc: Vec<epub::TocEntry>,
    disk_seen: Option<(std::time::SystemTime, u64)>,
    /// The selection anchors on the model, which returns untouched, so
    /// it survives the trip whole.
    selection: Option<Selection>,
    /// The look the stashed layout was built under; a change while the
    /// help page showed drops it on return.
    zoom: f32,
    theme: String,
    /// A parse was still streaming at the swap, so the model is
    /// partial; the return reopens from disk instead of restoring it.
    reopen: bool,
}

/// The help document, parsed from the generated page.
fn markdown_help() -> Document {
    oryx::doc::markdown::parse(help::page())
}

/// The viewer's justification for a document: the book preference on a
/// book, the markdown preference on other prose, never on code and
/// text files.
fn justify_pref(config: &Config, doc: &Document) -> bool {
    doc.justifiable()
        && if doc.book_id.is_some() {
            config.justify
        } else {
            config.justify_markdown
        }
}

fn window_title(book: Option<&str>, path: Option<&Path>, dirty: bool) -> String {
    let name = book.or_else(|| path.and_then(|p| p.file_name()).and_then(|n| n.to_str()));
    let star = if dirty { "*" } else { "" };
    match name {
        Some(name) => format!("{star}{name} \u{00B7} oryx"),
        None => "oryx".to_string(),
    }
}

/// Theme directories in lookup order: next to the binary, the XDG data
/// directory an installation fills, then the working directory.
fn theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("themes"));
        }
    }
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.data_dir().join("oryx/themes"));
    }
    dirs.push(PathBuf::from("themes"));
    dirs
}

/// Resolves the launch theme by name, falling back to the dracula file,
/// then to the compiled default when no theme file is found.
fn startup_theme(name: Option<&str>) -> Theme {
    let dirs = theme_dirs();
    if let Some(name) = name {
        match theme::find(&dirs, name) {
            Some(theme) => return theme,
            None => eprintln!("oryx: theme {name:?} not found, using the default"),
        }
    }
    theme::find(&dirs, "dracula").unwrap_or_else(Theme::default_dark)
}

struct App {
    gfx: Option<Gfx>,
    document: Document,
    /// The file shown in the document area, None on an empty launch.
    path: Option<PathBuf>,
    theme: Theme,
    cfg: ViewConfig,
    config: Config,
    fonts: FontStore,
    /// The shaping pool every layout pass and export attaches to.
    pool: Arc<ShapePool>,
    media: MediaCache,
    /// Handed to every media cache so fetch threads can wake the loop.
    waker: Waker,
    /// Background syntax highlighting worker and its arrivals queue.
    highlighter: Highlighter,
    /// Background full-parse worker behind a streamed open.
    parser: ParseWorker,
    /// Recolors owed to arrived highlights, applied in throttled waves.
    pending_recolor: Vec<(usize, std::ops::Range<usize>)>,
    /// When the last wave ran, pacing the next.
    last_recolor: Instant,
    /// True from a streamed open until the worker's document lands.
    parse_pending: bool,
    layout: Option<LayoutDoc>,
    /// The pass while a layout exists. It outlives completion so the
    /// parse swap can extend it over the appended tail. Positions already
    /// placed never move, so everything that indexes the layout stays
    /// valid as it grows.
    pass: Option<LayoutPass>,
    /// How long the last complete pass took, which decides whether a
    /// resize reflows live or waits for the size to settle.
    last_pass: Duration,
    /// A relayout deferred during a live resize, and when to run it.
    settle_at: Option<Instant>,
    /// Slice time the running pass has spent, which becomes `last_pass`.
    pass_spent: Duration,
    /// Scroll position to restore once the pass places it: a reload, or a
    /// relayout that must keep the reading position.
    pending_scroll: Option<f32>,
    /// Anchor target clicked before its heading was placed.
    pending_anchor: Option<String>,
    /// A book source offset to land on once delivered and placed: a
    /// restored reading position or an internal link's target.
    pending_offset: Option<usize>,
    /// A book's table of contents as authored; empty for files, whose
    /// outline scans headings instead.
    book_toc: Vec<epub::TocEntry>,
    /// Where reading stopped per book, written on switch and close.
    positions: config::Positions,
    layout_width: f32,
    /// Physical pixels per logical unit: the monitor's factor times the
    /// configured manual adjustment. The chrome multiplies by it in the
    /// painter; the document folds it into the zoom product.
    scale: f32,
    /// The display's own factor as winit reported it.
    os_scale: f32,
    /// The reader's own zoom step, 1.0 at rest. What the layout sees is
    /// always `user_zoom * scale`.
    user_zoom: f32,
    /// Touch gesture state: pans, taps, pinches.
    touch: touch::Tracker,
    /// Which pane the active touch pan scrolls.
    pan_target: PanTarget,
    /// A running fling: its velocity and when it last stepped.
    fling: Option<(f32, Instant)>,
    /// Emulated mouse events are dropped until then after a touch pan.
    mute_mouse_until: Option<Instant>,
    /// The reader zoom captured when a pinch began.
    pinch_base: f32,
    band: Option<BandCache>,
    scroll_y: f32,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    /// Pointer gesture on the app's own chrome, if any.
    drag: Option<Drag>,
    /// When the sidebar edge was last clicked, for the double click that
    /// restores the default width.
    last_edge_click: Option<Instant>,
    /// Cursor currently over the sidebar's drag zone.
    hover_edge: bool,
    /// Whether the cursor currently sits over a link, for the pointer icon.
    hover_link: bool,
    /// Selection drag in progress: the caret grabbed at mouse down.
    sel_anchor: Option<ModelPos>,
    /// The last document-area press, for the double and triple click
    /// chain: when, where, and how many clicks it had reached.
    last_click: Option<(Instant, f32, f32, u8)>,
    /// Current selection, kept after the mouse releases.
    selection: Option<Selection>,
    /// Created on first copy and kept alive so the content outlives the
    /// call on X11.
    clipboard: Option<arboard::Clipboard>,
    /// The single active modal overlay; receives keys, clicks, and wheel
    /// while open.
    overlay: Option<Box<dyn Overlay>>,
    /// Left button held while an overlay is open, for drag routing.
    overlay_mouse: bool,
    /// Theme to restore when the editor closes without saving.
    pre_edit: Option<Theme>,
    /// The confirmed theme while the browser is open: keyboard stepping
    /// previews live, and a close without Enter or a click puts this back.
    pre_browse: Option<Theme>,
    /// Reused overlay canvas and the region the last frame painted.
    overlay_canvas: Option<OverlayCanvas>,
    /// A running export, driven a slice at a time from the redraw.
    export: Option<ExportPass>,
    /// Set when the export's chosen theme no longer resolves, so the
    /// result line can say the active one was used instead.
    export_warning: Option<String>,
    /// A view change waiting for its dialog to close before it writes
    /// the config, so a held arrow key is not a disk write per repeat.
    view_dirty: bool,
    /// Folder sidebar while open; the document lays out beside it.
    sidebar: Option<Sidebar>,
    /// The pane Up, Down, and Enter act on: the pane last acted on.
    key_pane: KeyPane,
    /// The document's heading outline, behind the sidebar's second tab.
    /// Rebuilt with the document, extended as the parse worker delivers.
    outline: OutlineTree,
    /// Reused sidebar canvas, mirroring the overlay canvas mechanics.
    sidebar_canvas: Option<OverlayCanvas>,
    /// Find session while the search bar is open.
    search: Option<SearchState>,
    /// Reused search bar canvas, mirroring the overlay canvas mechanics.
    search_canvas: Option<OverlayCanvas>,
    /// Query of the last closed search, restored when the bar reopens.
    last_query: String,
    /// Matching mode of the last search, restored the same way.
    last_regex: bool,
    /// Replacement text of the last closed replace row, restored the
    /// same way.
    last_replace: String,
    /// The press that opened or hit the search bar's toggle; its release
    /// must not click the document underneath.
    search_mouse: bool,
    /// Deferred band rebuild, tagged with the window size it was scheduled
    /// at. Interactive frames (drag, live resize) paint the viewport
    /// directly; the expensive band builds one frame later, once stable.
    pending_band_for: Option<(u32, u32)>,
    /// Reading or editing; the door is Ctrl+E, through `edit::toggle`.
    mode: edit::Mode,
    /// The caret while editing, anchored to a source offset.
    caret: Option<Caret>,
    /// The open file decoded only through lossy UTF-8 replacement, so
    /// the editing door refuses it.
    lossy: bool,
    /// Caret offsets remembered per file for this session, feeding the
    /// landing precedence on re-entry.
    edit_marks: HashMap<PathBuf, usize>,
    /// The reading position a file switch left behind, the top block's
    /// source offset by canonical path. In memory only: a revisit in
    /// the same session resumes there, a new session starts at the top.
    read_marks: HashMap<PathBuf, usize>,
    /// The splice ledger, from the first edit-mode entry until the file
    /// closes or saves; unsaved edits survive mode flips inside it.
    ledger: Option<Ledger>,
    /// The undo stack, living and dying with the ledger; the save point
    /// inside it drives the title asterisk.
    undo: Option<Undo>,
    /// When the edited block re-highlights, set by each keystroke and
    /// fired once typing rests.
    rehighlight_at: Option<Instant>,
    /// The live generation's seam table per block, from the exact
    /// sweep's arrivals: the (line, state) pairs a later sweep can
    /// resume from and converge against.
    seams: HashMap<usize, Vec<(usize, highlight::Seam)>>,
    /// The band the standing speculation covers, so a resting view
    /// asks once.
    spec_band: Option<(usize, std::ops::Range<usize>)>,
    /// A jump to the bottom of a document the pass had not finished
    /// placing, waiting for the whole document to seat it.
    bottom_hold: scroll::BottomHold,
    /// The unsaved-changes modal and the action it guards.
    confirm: Option<confirm::Pending>,
    /// The document stashed whole while the help page shows, edits and
    /// layout included, so F1 out and back never touches the disk.
    help_stash: Option<Box<Stash>>,
    /// The rendered page held while its own source is edited. Only the
    /// kinds that render to something other than their bytes park one.
    edit_park: Option<Box<Parked>>,
    /// Files left while they were being edited, which reopen in the
    /// editor. Switching away is not a decision to stop editing;
    /// Escape is, and it clears the entry.
    resume_edit: HashSet<PathBuf>,
    /// A row to seat at the top of the editor once the layout places
    /// it, the source view's counterpart to `pending_offset`.
    pending_row: Option<usize>,
    /// The open file's on-disk identity at last read or write, for the
    /// external-change check.
    disk_seen: Option<(std::time::SystemTime, u64)>,
    /// The earliest next disk check; the check runs on focus and on
    /// interaction frames, never on a timer, so idle stays idle.
    disk_check_at: Instant,
    /// Normalized-text offsets of the open file's CRLF endings, for the
    /// ledger's byte-exact emission.
    crlf: Vec<u32>,
    /// A typing edit or caret motion moved content under a stale
    /// layout; the next placed frame snaps the view to the caret.
    caret_snap: bool,
    /// The blink's current half and when it flips, driven by the timer;
    /// every caret action restarts the visible half.
    blink_visible: bool,
    blink_flip: Instant,
    /// The transient corner notice, while one holds or fades.
    notice: Option<Notice>,
    /// Reused notice canvas, mirroring the overlay canvas mechanics.
    notice_canvas: Option<OverlayCanvas>,
}

/// The pane owning Up, Down, and Enter. There is no focus system: the
/// owner is the pane last acted on, and every ownership-moving event
/// funnels through `owner_after`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyPane {
    Document,
    Sidebar,
}

/// An act that can move key ownership.
#[derive(Debug, Clone, Copy)]
enum PaneAct {
    OpenSidebar,
    CloseSidebar,
    ClickSidebar,
    ClickDocument,
    WheelSidebar,
    WheelDocument,
    /// The keyboard tab switch, an act on the panel.
    SwitchTab,
    /// Activation inside the sidebar; navigation flows keep their keys.
    Enter,
    /// The explicit transfer keys. Left claims only an existing sidebar.
    Left,
    Right,
}

/// The owner after an act. Pure, so the transition table is testable on
/// its own.
fn owner_after(owner: KeyPane, act: PaneAct, sidebar_open: bool) -> KeyPane {
    match act {
        PaneAct::OpenSidebar
        | PaneAct::ClickSidebar
        | PaneAct::WheelSidebar
        | PaneAct::SwitchTab => KeyPane::Sidebar,
        PaneAct::CloseSidebar
        | PaneAct::ClickDocument
        | PaneAct::WheelDocument
        | PaneAct::Right => KeyPane::Document,
        PaneAct::Left if sidebar_open => KeyPane::Sidebar,
        PaneAct::Left | PaneAct::Enter => owner,
    }
}

/// A pointer gesture on the app's own chrome.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Scrollbar thumb, holding the cursor offset from the thumb top.
    Scrollbar(f32),
    /// The sidebar's right edge, holding the cursor offset from it.
    SidebarEdge(f32),
}

fn drag_is_edge(drag: Drag) -> bool {
    matches!(drag, Drag::SidebarEdge(_))
}

/// Which pane a touch pan scrolls, fixed where the gesture began.
#[derive(Clone, Copy, PartialEq)]
enum PanTarget {
    Document,
    Sidebar,
    Overlay,
}

struct Gfx {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

/// Reused overlay pixmap plus the region the last frame painted.
type OverlayCanvas = (tiny_skia::Pixmap, Option<(f32, f32, f32, f32)>);

impl App {
    fn line_step(&self) -> f32 {
        metrics::LINE_HEIGHT * self.cfg.body_size * self.cfg.zoom
    }

    fn doc_height(&self) -> f32 {
        self.layout.as_ref().map(|l| l.height).unwrap_or(0.0)
    }

    fn viewport_h(&self) -> f32 {
        self.gfx
            .as_ref()
            .map(|g| g.window.inner_size().height as f32)
            .unwrap_or(0.0)
    }

    fn scroll_to(&mut self, y: f32) {
        let clamped = scroll::clamp(y, self.doc_height(), self.viewport_h());
        if clamped != self.scroll_y {
            self.scroll_y = clamped;
            if self.drag.is_none() {
                self.update_hover();
            }
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        }
    }

    fn scroll_by(&mut self, delta: f32) {
        self.scroll_to(self.scroll_y + delta);
    }

    fn page_step(&self) -> f32 {
        (self.viewport_h() - self.line_step()).max(self.line_step())
    }

    /// Executes a shortcut resolved by the keymap.
    fn run_command(&mut self, cmd: Command, event_loop: &ActiveEventLoop) {
        match cmd {
            Command::OpenFile => self.open_dialog(),
            Command::Export => self.export_now(),
            Command::ExportSettings => self.toggle_export_settings(),
            Command::Reload => self.reload(),
            Command::Sidebar => self.toggle_sidebar(),
            Command::Help => self.toggle_help(),
            Command::Settings => self.toggle_settings(),
            Command::ThemeBrowser => self.toggle_theme_browser(),
            Command::ZoomIn => {
                self.user_zoom = settings::step_zoom(self.user_zoom, settings::ZOOM_STEP);
                self.apply_zoom();
            }
            Command::ZoomOut => {
                self.user_zoom = settings::step_zoom(self.user_zoom, -settings::ZOOM_STEP);
                self.apply_zoom();
            }
            Command::ZoomReset => {
                self.user_zoom = 1.0;
                self.apply_zoom();
            }
            Command::Justify => self.toggle_justify(),
            Command::SelectAll => self.select_all(),
            Command::CopyText => self.copy_selection(false),
            Command::CopyMarkdown => self.copy_selection(true),
            Command::Find => self.open_search(),
            Command::FindNext => self.step_search(true),
            Command::FindPrev => self.step_search(false),
            Command::Replace => self.open_replace(),
            Command::LineUp => self.scroll_by(-self.line_step()),
            Command::LineDown => self.scroll_by(self.line_step()),
            Command::PaneLeft => self.move_ownership(PaneAct::Left),
            Command::PaneRight => self.move_ownership(PaneAct::Right),
            Command::SidebarTab => self.switch_sidebar_tab(),
            Command::PageUp => self.scroll_by(-self.page_step()),
            Command::PageDown => self.scroll_by(self.page_step()),
            Command::Top => self.scroll_to(0.0),
            Command::Bottom => {
                self.scroll_to(self.doc_height());
                // The placed height is all Oryx knows, so on a document
                // still streaming this lands short of the file's end;
                // the intent is held and the completing pass spends it.
                self.bottom_hold.take(self.scroll_y, !self.layout_pending());
            }
            Command::Edit => self.toggle_edit(),
            Command::Cut => self.cut_selection(),
            Command::Paste => self.paste_clipboard(),
            Command::Undo => self.undo_edit(),
            Command::Redo => self.redo_edit(),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => self.save_as(),
            Command::NewFile => {
                if self.guard_unsaved(confirm::Pending::New) {
                    self.new_file();
                }
            }
            // The Escape ladder, innermost out. The overlay branch
            // catches it upstream; the find bar rung is normally spent
            // there too and stands here for totality.
            Command::Quit => {
                let act = edit::escape(
                    self.mode,
                    self.search.is_some(),
                    self.selection.is_some_and(|s| !s.is_empty()),
                );
                match act {
                    edit::EscapeAct::CloseFind => self.close_search(),
                    edit::EscapeAct::ClearSelection => {
                        self.selection = None;
                        self.sel_anchor = None;
                        self.band = None;
                        self.request_redraw();
                    }
                    edit::EscapeAct::LeaveEdit => self.leave_edit(),
                    edit::EscapeAct::Quit => {
                        // While the help page shows, Escape means
                        // leaving it; the sidebar stays as it stands
                        // either way, hiding belongs to Ctrl+Shift+B.
                        if self.help_stash.is_some() {
                            self.help_return();
                        } else if self.guard_unsaved(confirm::Pending::Quit) {
                            self.remember_position();
                            event_loop.exit();
                        }
                    }
                }
            }
        }
    }

    /// Answers Ctrl+E through the door table; a refusal shows the
    /// notice instead of the mode.
    fn toggle_edit(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        match edit::toggle(self.mode, load::detect(&path), self.lossy) {
            Ok(edit::Mode::Edit) => self.enter_edit(),
            Ok(edit::Mode::Read) => self.leave_edit(),
            Err(refusal) => self.show_notice(refusal.message()),
        }
    }

    /// Opens the editor on the file's own bytes. A code or text file
    /// already shows them, so nothing swaps and the caret simply
    /// appears; markdown parks its rendered page and puts its source
    /// on screen in its place.
    fn enter_edit(&mut self) {
        let Some(kind) = self.path.as_deref().map(load::detect) else {
            return;
        };
        // A page parked mid-stream would be a prefix, and the worker's
        // delivery would swap it under the return. Landing it first
        // costs nothing on the kinds that never stream.
        self.finish_parse();
        let view_h = self.viewport_h();
        let remembered = self
            .path
            .as_ref()
            .and_then(|p| self.edit_marks.get(p))
            .copied();
        let sel = self.selection.filter(|s| !s.is_empty());
        let offset = match self.layout.as_ref() {
            Some(lay) => caret::landing(
                lay,
                &self.document,
                sel.as_ref(),
                remembered,
                self.scroll_y,
                view_h,
            ),
            None => remembered.unwrap_or(0),
        };
        self.mode = edit::Mode::Edit;
        self.caret = Some(Caret::at(offset));
        self.ensure_ledger();
        let text = Arc::clone(&self.document.source);
        if let Some(source) = edit::source_document(kind, &text) {
            let head = self.undo.as_ref().map_or(0, Undo::head);
            let parked = Parked {
                document: std::mem::replace(&mut self.document, source),
                layout: self.layout.take(),
                pass: self.pass.take(),
                scroll_y: self.scroll_y,
                layout_width: self.layout_width,
                head,
                zoom: self.cfg.zoom,
                theme: self.config.theme.clone(),
            };
            self.edit_park = Some(Box::new(parked));
            self.swapped_document(None, None);
            // The reading scroll means nothing in the source view: the
            // two documents share no coordinate but the bytes. Seat the
            // editor on the row the caret landed on, which is the row
            // the page was resting at.
            self.seat_editor_on(offset);
        }
        // The caret owns the keys; a sidebar holding them would strand
        // the arrows. Same funnel as the Right key's explicit handoff.
        self.move_ownership(PaneAct::Right);
        self.wake_caret();
        self.request_redraw();
    }

    /// The state that belongs to a document rather than to the file,
    /// reset after either crossing swaps one for the other: the layout
    /// and its pass, the painted band, a selection that anchors on the
    /// old model, and the highlight worker. A restored page brings its
    /// own layout back; a fresh one passes None and the next frame
    /// places it. The outline is not reset here: it describes the
    /// rendered page and outlives the trip into the source.
    fn swapped_document(&mut self, layout: Option<LayoutDoc>, pass: Option<LayoutPass>) {
        self.layout = layout;
        self.pass = pass;
        self.band = None;
        self.pending_band_for = None;
        self.pending_row = None;
        self.selection = None;
        self.sel_anchor = None;
        self.close_search();
        self.start_highlight(load::pending(&self.document));
    }

    /// Puts the row holding `offset` at the top of the editor, the way
    /// a jump lands rather than the way a typed caret is kept in view.
    /// A placed row answers exactly; past the placed height the block
    /// table answers by line index, which is what a source view is
    /// indexed by.
    fn seat_editor_on(&mut self, offset: usize) {
        match self.editor_row_y(offset) {
            Some(y) => {
                self.pending_row = None;
                self.scroll_to(y);
            }
            None => self.pending_row = Some(offset),
        }
    }

    /// The y of the row an offset stands on in the editor. Placed runs
    /// answer exactly, which covers a caret near the view. Beyond the
    /// layout window there are no runs to read, and a jump across a
    /// whole file is exactly that case, so the block table answers by
    /// line index, which is how a source view is indexed.
    fn editor_row_y(&self, offset: usize) -> Option<f32> {
        let lay = self.layout.as_ref()?;
        if let Some(y) = caret::row_top(lay, &self.document, offset) {
            return Some(y);
        }
        let block = self.document.block_at_offset(offset)?;
        let start = self.document.blocks.get(block)?.range.start;
        let end = offset.min(self.document.source.len()).max(start);
        let line = self.document.source[start..end].matches('\n').count();
        lay.approx_top(block, line)
    }

    /// Rebuilds the page held behind the editor from the buffer, which
    /// a save makes the file. The outline refreshes with it, and the
    /// next return costs no parse, since the parked page is the
    /// buffer's own render again.
    fn refresh_parked_page(&mut self) {
        let Some(kind) = self.path.as_deref().map(load::detect) else {
            return;
        };
        let head = self.undo.as_ref().map_or(0, Undo::head);
        if !self.edit_park.as_ref().is_some_and(|p| p.head != head) {
            return;
        }
        let page = edit::rendered_document(kind, &self.document.source);
        let outline = OutlineTree::build(&page);
        if let Some(parked) = self.edit_park.as_mut() {
            parked.document = page;
            parked.head = head;
            parked.layout = None;
            parked.pass = None;
        }
        self.outline = outline;
    }

    /// Back to reading; the caret offset is remembered for this file so
    /// re-entry lands where editing stopped. A parked page returns
    /// whole when no byte changed, and is rebuilt from the buffer when
    /// one did, so unsaved edits show on the page and not just on disk.
    fn leave_edit(&mut self) {
        // The replace row is an editor tool; reading keeps the plain bar.
        let dropped = self.search.as_mut().and_then(|state| state.replace.take());
        if let Some(row) = dropped {
            self.last_replace = row.field.text().to_string();
        }
        let left_at = self.caret.map(|c| c.offset);
        if let (Some(path), Some(c)) = (self.path.clone(), self.caret) {
            self.edit_marks.insert(path, c.offset);
        }
        self.mode = edit::Mode::Read;
        self.caret = None;
        if let Some(path) = self.path.as_ref() {
            self.resume_edit.remove(path);
        }
        if let Some(parked) = self.edit_park.take() {
            let head = self.undo.as_ref().map_or(0, Undo::head);
            let same_look = self.cfg.zoom == parked.zoom && self.config.theme == parked.theme;
            if head == parked.head {
                // The held outline describes this very page; rebuilding
                // it here cost 273ms at the 8MB tier for nothing.
                self.document = parked.document;
                self.layout_width = parked.layout_width;
                self.swapped_document(
                    parked.layout.filter(|_| same_look),
                    parked.pass.filter(|_| same_look),
                );
            } else {
                let kind = self.path.as_deref().map(load::detect);
                let text = Arc::clone(&self.document.source);
                if let Some(kind) = kind {
                    self.document = edit::rendered_document(kind, &text);
                }
                self.swapped_document(None, None);
                self.outline = OutlineTree::build(&self.document);
            }
            // The page the reader came in from, then the row they are
            // leaving on: a restored layout answers exactly, and a page
            // still to be laid out answers through the pending target
            // once it reaches that far.
            self.scroll_y = parked.scroll_y;
            if let Some(offset) = left_at {
                match self
                    .layout
                    .as_ref()
                    .and_then(|lay| caret::row_top(lay, &self.document, offset))
                {
                    Some(y) => self.scroll_to(y),
                    None => self.pending_offset = Some(offset),
                }
            }
        }
        self.request_redraw();
    }

    /// Restarts the blink at full visibility, as every caret action
    /// does, so the caret never blinks away mid-motion.
    fn wake_caret(&mut self) {
        self.blink_visible = true;
        self.blink_flip = Instant::now() + CARET_BLINK;
    }

    fn show_notice(&mut self, text: &str) {
        self.notice = Some(Notice::new(text, Instant::now()));
        self.request_redraw();
    }

    /// One caret motion: step through the runs, restart the blink, and
    /// snap the view back to the caret.
    fn step_caret(&mut self, motion: Motion) {
        let page = self.page_step();
        let view_h = self.viewport_h();
        let (Some(caret), Some(lay)) = (self.caret, self.layout.as_ref()) else {
            return;
        };
        let stepped = caret.step(motion, lay, &self.document, &mut self.fonts, page);
        let snapped = stepped
            .geometry(lay, &self.document, &mut self.fonts)
            .map(|b| caret::snap(self.scroll_y, view_h, b));
        self.caret = Some(stepped);
        self.wake_caret();
        if let Some(target) = snapped {
            if target != self.scroll_y {
                self.scroll_to(target);
            }
        }
        self.request_redraw();
    }

    /// Bare keys the caret owns while editing. Chords fall through and
    /// keep their app-wide meaning; the typing keys are consumed and
    /// inert until the splice ledger arrives. Escape stays a command so
    /// the ladder handles it, and the function keys keep their rows.
    fn edit_key(&mut self, key: &Key, ctrl: bool, shift: bool) -> bool {
        // Alt chords belong to the desktop, never to typing.
        if self.modifiers.alt_key() {
            return false;
        }
        if ctrl {
            // The familiar editor chords the caret answers: document
            // jumps, word jumps, and word deletion, growing the
            // selection under Shift. Everything else keeps its
            // app-wide meaning.
            let jump = match key {
                Key::Named(NamedKey::Home) => Some(Motion::DocStart),
                Key::Named(NamedKey::End) => Some(Motion::DocEnd),
                Key::Named(NamedKey::ArrowLeft) => Some(Motion::WordLeft),
                Key::Named(NamedKey::ArrowRight) => Some(Motion::WordRight),
                _ => None,
            };
            if let Some(jump) = jump {
                self.move_caret(jump, shift);
                return true;
            }
            let at = self.caret.map(|c| c.offset);
            match (key, at) {
                (Key::Named(NamedKey::Backspace), Some(at)) => {
                    if !self.delete_selection() {
                        let to = caret::word_left(&self.document.source, at);
                        if to < at {
                            self.type_edit(to..at, "", Kind::Structural);
                        }
                    }
                    return true;
                }
                (Key::Named(NamedKey::Delete), Some(at)) => {
                    if !self.delete_selection() {
                        let to = caret::word_right(&self.document.source, at);
                        if to > at {
                            self.type_edit(at..to, "", Kind::Structural);
                        }
                    }
                    return true;
                }
                _ => {}
            }
            return false;
        }
        let motion = match key {
            Key::Named(NamedKey::ArrowLeft) => Some(Motion::Left),
            Key::Named(NamedKey::ArrowRight) => Some(Motion::Right),
            Key::Named(NamedKey::ArrowUp) => Some(Motion::Up),
            Key::Named(NamedKey::ArrowDown) => Some(Motion::Down),
            Key::Named(NamedKey::Home) => Some(Motion::Home),
            Key::Named(NamedKey::End) => Some(Motion::End),
            Key::Named(NamedKey::PageUp) => Some(Motion::PageUp),
            Key::Named(NamedKey::PageDown) => Some(Motion::PageDown),
            _ => None,
        };
        if let Some(motion) = motion {
            self.move_caret(motion, shift);
            return true;
        }
        let Some(at) = self.caret.map(|c| c.offset) else {
            return false;
        };
        match key {
            // Enter is a structural edit: a line split never joins a
            // typing unit.
            Key::Named(NamedKey::Enter) => {
                self.press_enter();
                true
            }
            Key::Named(NamedKey::Tab) => {
                self.press_tab(shift);
                true
            }
            Key::Named(NamedKey::Space) => {
                self.type_over(" ", Kind::Insert);
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if !self.delete_selection() {
                    let prev = self.document.source[..at]
                        .chars()
                        .next_back()
                        .map(|c| at - c.len_utf8());
                    if let Some(prev) = prev {
                        self.type_edit(prev..at, "", Kind::Delete);
                    }
                }
                true
            }
            Key::Named(NamedKey::Delete) => {
                if !self.delete_selection() {
                    let next = self.document.source[at..]
                        .chars()
                        .next()
                        .map(|c| at + c.len_utf8());
                    if let Some(next) = next {
                        self.type_edit(at..next, "", Kind::Delete);
                    }
                }
                true
            }
            Key::Character(s) if !s.chars().any(char::is_control) && !s.is_empty() => {
                let text = s.clone();
                self.type_over(&text, Kind::Insert);
                true
            }
            Key::Character(_) => true,
            _ => false,
        }
    }

    /// One typing edit: the history records it, the ledger applies it,
    /// and the caret lands after the inserted text.
    fn type_edit(&mut self, range: std::ops::Range<usize>, text: &str, kind: Kind) {
        let replaced = self
            .document
            .source
            .get(range.clone())
            .unwrap_or_default()
            .to_string();
        let caret_before = self.caret.map_or(range.start, |c| c.offset);
        let caret_after = range.start + text.len();
        if !self.apply_edit(range.clone(), text) {
            return;
        }
        if let Some(hist) = self.undo.as_mut() {
            hist.record(
                range,
                text,
                &replaced,
                (caret_before, caret_after),
                kind,
                Instant::now(),
            );
        }
        self.seat_caret_after_edit(caret_after);
    }

    /// Arms the splice ledger and the undo stack when none stand: the
    /// first edit fixes the baseline, whichever door asks, Ctrl+E or
    /// the rendered page's checkbox click. Re-entry keeps the ledger
    /// and the unsaved edits inside it, and the undo stack with them.
    fn ensure_ledger(&mut self) {
        if self.ledger.is_none() {
            self.ledger = Some(Ledger::new(self.document.source.clone(), self.crlf.clone()));
            self.undo = Some(Undo::new());
        }
    }

    /// A click on a rendered task checkbox, the one edit reading
    /// allows. The flip replaces one byte with one byte, so nothing on
    /// the page moves: the model flips in place and the band simply
    /// re-materializes at its recorded positions. Books and lossy
    /// reads refuse the way the editor's door does, quietly.
    fn click_checkbox(&mut self, block: usize) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if edit::toggle(edit::Mode::Read, load::detect(&path), self.lossy).is_err() {
            return;
        }
        self.ensure_ledger();
        let Some((range, text)) = self.document.flip_task(block) else {
            return;
        };
        let replaced = if text == "x" { " " } else { "x" };
        if let Some(ledger) = self.ledger.as_mut() {
            ledger.edit(range.clone(), text);
        }
        if let Some(hist) = self.undo.as_mut() {
            hist.record(
                range.clone(),
                text,
                replaced,
                (range.start, range.start),
                Kind::Structural,
                Instant::now(),
            );
        }
        let settled = self.pass.as_ref().is_some_and(LayoutPass::is_complete);
        let refilled = settled
            && self
                .layout
                .as_mut()
                .is_some_and(layout::LayoutDoc::rematerialize);
        if refilled {
            self.band = None;
            self.pending_band_for = None;
            self.request_redraw();
        } else {
            self.restart_layout();
        }
        self.refresh_title();
    }

    /// Steps the history one unit back and applies its inverse; the
    /// one deliberate view move, the snap to the restored caret. In
    /// read mode the checkbox flips are undoable without entering the
    /// editor; deeper text units are legal too and pay the rendered
    /// rebuild.
    fn undo_edit(&mut self) {
        let Some((splice, caret)) = self.undo.as_mut().and_then(Undo::undo) else {
            return;
        };
        match self.mode {
            edit::Mode::Edit => {
                if self.apply_edit(splice.range, &splice.text) {
                    self.seat_caret_after_edit(caret);
                }
            }
            edit::Mode::Read => self.apply_read_edit(splice.range, &splice.text, caret),
        }
    }

    /// A read-mode undo or redo applied: the ledger replays the
    /// splice, the rendered page rebuilds from the buffer, and the
    /// view stays put unless the restored edit is off screen, where
    /// its block seats through the pending target.
    fn apply_read_edit(&mut self, range: std::ops::Range<usize>, text: &str, caret: usize) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(ledger) = self.ledger.as_mut() else {
            return;
        };
        let touched = ledger.edit(range.clone(), text);
        let removed = self
            .document
            .source
            .get(range)
            .map_or(0, |cut| cut.matches('\n').count());
        let old_touched = touched.start..touched.start + removed + 1;
        if let Some(table) = self.seams.get_mut(&0) {
            highlight::shift_seams(table, old_touched, touched);
        }
        let current = ledger.current();
        let visible = self
            .layout
            .as_ref()
            .and_then(|l| caret::row_top(l, &self.document, caret))
            .is_some_and(|y| y >= self.scroll_y && y < self.scroll_y + self.viewport_h());
        self.document = edit::rendered_document(load::detect(&path), &current);
        self.restart_layout();
        self.outline = OutlineTree::build(&self.document);
        self.cancel_highlight();
        self.rehighlight_at =
            (!self.document.plain_file).then(|| Instant::now() + REHIGHLIGHT_REST);
        self.selection = None;
        self.sel_anchor = None;
        if let Some(state) = self.search.as_mut() {
            state.matches.clear();
            state.rects.clear();
            state.stale = true;
        }
        if !visible {
            self.pending_offset = Some(caret);
        }
        self.refresh_title();
    }

    /// Steps the history one unit forward and reapplies its splice.
    /// Inert outside edit mode.
    fn redo_edit(&mut self) {
        let Some((splice, caret)) = self.undo.as_mut().and_then(Undo::redo) else {
            return;
        };
        match self.mode {
            edit::Mode::Edit => {
                if self.apply_edit(splice.range, &splice.text) {
                    self.seat_caret_after_edit(caret);
                }
            }
            edit::Mode::Read => self.apply_read_edit(splice.range, &splice.text, caret),
        }
    }

    /// The caret after an edit or an undo step: seated, blink woken,
    /// the next placed frame snapping the view to it.
    fn seat_caret_after_edit(&mut self, offset: usize) {
        self.caret = Some(Caret::at(offset));
        self.wake_caret();
        self.caret_snap = true;
        self.refresh_title();
    }

    /// One edit applied: the ledger splices it, the document and the
    /// layout splice in place on the fast path, and the edited block
    /// re-highlights once typing rests. The full reparse and relayout
    /// survive as the fallback: a pass still measuring, or a splice the
    /// model declines. False when no ledger stands.
    fn apply_edit(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        let Some(ledger) = self.ledger.as_mut() else {
            return false;
        };
        let removed = self
            .document
            .source
            .get(range.clone())
            .map_or(0, |cut| cut.matches('\n').count());
        let touched = ledger.edit(range.clone(), text);
        let old_touched = touched.start..touched.start + removed + 1;
        let current = ledger.current();
        let settled =
            self.pass.as_ref().is_some_and(LayoutPass::is_complete) && self.layout.is_some();
        let spliced = settled
            .then(|| {
                edit::splice_document(&mut self.document, &current, range.clone(), touched.clone())
            })
            .flatten();
        match spliced {
            Some((old_lines, new_lines)) => {
                if let Some(table) = self.seams.get_mut(&0) {
                    highlight::shift_seams(table, old_lines.clone(), new_lines.clone());
                }
                // The model spliced; the layout follows in place, or a
                // restart re-measures the already-correct model.
                let lay = self.layout.as_mut().expect("settled layout");
                let placed = layout::edit_code_lines(
                    lay,
                    &self.document,
                    &self.theme,
                    &mut self.fonts,
                    &self.cfg,
                    0,
                    old_lines,
                    new_lines,
                );
                if placed {
                    // The next frame's window slide re-materializes the
                    // band from the corrected table.
                    self.band = None;
                    self.pending_band_for = None;
                    self.pending_recolor.clear();
                    self.request_redraw();
                } else {
                    self.restart_layout();
                }
            }
            None => {
                if let Some(table) = self.seams.get_mut(&0) {
                    highlight::shift_seams(table, old_touched.clone(), touched.clone());
                }
                self.document = edit::reparse(
                    load::detect(&path),
                    &current,
                    &self.document,
                    old_touched,
                    touched,
                );
                self.restart_layout();
            }
        }
        // The outline belongs to the rendered page. While a park
        // stands, the document on screen is the source view, whose
        // build would empty the panel; the held outline refreshes on
        // save and on the crossing back.
        if self.edit_park.is_none() {
            self.outline = OutlineTree::build(&self.document);
        }
        // The keystroke kills in-flight highlight work, so stale spans
        // never fold against shifted lines; the resumed re-run waits
        // for the typing to rest and needs the shifted seam table, so
        // only the worker cancels. Prose has no colors to correct.
        self.cancel_highlight();
        self.rehighlight_at =
            (!self.document.plain_file).then(|| Instant::now() + REHIGHLIGHT_REST);
        self.selection = None;
        self.sel_anchor = None;
        // The match set holds positions of the text that just changed;
        // the next frame recomputes it against the edit.
        if let Some(state) = self.search.as_mut() {
            state.matches.clear();
            state.rects.clear();
            state.stale = true;
        }
        true
    }

    /// Unsaved edits stand: the undo head is away from the save point,
    /// or the ledger holds splices the stack cannot see.
    fn edits_unsaved(&self) -> bool {
        self.undo.as_ref().is_some_and(Undo::is_dirty)
            || self.ledger.as_ref().is_some_and(Ledger::is_dirty)
    }

    /// Ctrl+S: the ledger's emission written atomically, the baseline
    /// re-fixed on the written bytes, the save point marked, and the
    /// receipt shown with its lines-changed figure. True when nothing
    /// was left unsaved.
    fn save(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        let Some(ledger) = self.ledger.as_ref() else {
            return false;
        };
        if !self.edits_unsaved() {
            self.show_notice("No unsaved changes");
            return true;
        }
        let lines = ledger.touched_lines();
        let bytes = ledger.emit();
        match save::write_atomic(&path, &bytes) {
            Ok(()) => {
                if let Some(ledger) = self.ledger.as_mut() {
                    ledger.commit();
                }
                if let Some(undo) = self.undo.as_mut() {
                    undo.mark_saved();
                }
                self.refresh_parked_page();
                self.note_disk_state();
                self.refresh_title();
                let figure = if lines == 1 {
                    "1 line changed".to_string()
                } else {
                    format!("{lines} lines changed")
                };
                self.show_notice(&format!("Saved, {figure}"));
                true
            }
            Err(err) => {
                self.show_notice(&format!("Save failed: {err}"));
                false
            }
        }
    }

    /// Ctrl+Shift+S: the current text written to a chosen path, which
    /// becomes the open file; the mode survives the move.
    fn save_as(&mut self) {
        let Some(ledger) = self.ledger.as_ref() else {
            return;
        };
        let bytes = ledger.emit();
        let mut dialog = rfd::FileDialog::new();
        if let Some(path) = self.path.as_deref() {
            if let Some(dir) = path.parent() {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                dialog = dialog.set_file_name(name);
            }
        }
        let Some(target) = dialog.save_file() else {
            return;
        };
        match save::write_atomic(&target, &bytes) {
            Ok(()) => {
                let editing = self.mode == edit::Mode::Edit;
                let scroll = self.scroll_y;
                self.open_file(&target, false);
                self.scroll_y = scroll;
                if editing {
                    self.toggle_edit();
                }
                self.show_notice("Saved");
            }
            Err(err) => self.show_notice(&format!("Save as failed: {err}")),
        }
    }

    /// Ctrl+N: the save dialog first, name and extension choosing the
    /// type, then the empty file created, opened with the sidebar moved
    /// to its folder, and edit mode entered on the blank page.
    fn new_file(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_file_name("untitled.txt");
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(target) = dialog.save_file() else {
            return;
        };
        if let Err(err) = save::write_atomic(&target, b"") {
            self.show_notice(&format!("Could not create the file: {err}"));
            return;
        }
        self.open_file(&target, true);
        self.toggle_edit();
    }

    /// Records the open file's on-disk identity after a read or a
    /// write of our own, the reference the change check compares to.
    fn note_disk_state(&mut self) {
        self.disk_seen = self.path.as_deref().and_then(|path| {
            let meta = std::fs::metadata(path).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        });
    }

    /// The external-change check, run on focus and rate-limited on
    /// interaction frames: a changed file reloads under a clean
    /// document and reports through the notice over unsaved edits.
    fn check_disk(&mut self) {
        let now = Instant::now();
        if now < self.disk_check_at {
            return;
        }
        self.disk_check_at = now + Duration::from_secs(1);
        let Some(seen) = self.disk_seen else {
            return;
        };
        let Some(path) = self.path.clone() else {
            return;
        };
        let state = std::fs::metadata(&path)
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        let Some(state) = state else {
            return;
        };
        if state == seen {
            return;
        }
        self.disk_seen = Some(state);
        if self.edits_unsaved() {
            self.show_notice("The file changed on disk");
        } else {
            self.reload_now();
        }
    }

    /// Guards an action that would discard unsaved edits behind the
    /// confirm modal; answers whether the action may run now. From
    /// inside the help page the stash restores first, so the check and
    /// the action both see the real document.
    fn guard_unsaved(&mut self, pending: confirm::Pending) -> bool {
        if self.help_stash.is_some() {
            self.help_return();
        }
        if !self.edits_unsaved() {
            return true;
        }
        self.confirm = Some(pending);
        self.request_redraw();
        false
    }

    /// Runs the action the confirm was holding.
    fn resolve_confirm(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pending) = self.confirm.take() else {
            return;
        };
        match pending {
            confirm::Pending::Quit => {
                self.remember_position();
                event_loop.exit();
            }
            confirm::Pending::Reload => self.reload_now(),
            confirm::Pending::Open(path, reroot) => self.open_file(&path, reroot),
            confirm::Pending::New => self.new_file(),
        }
        self.request_redraw();
    }

    /// One key against the open confirm modal.
    fn confirm_key(&mut self, key: &Key, event_loop: &ActiveEventLoop) {
        match confirm::decide(key) {
            confirm::Decision::Save => {
                if self.save() {
                    self.resolve_confirm(event_loop);
                } else {
                    self.confirm = None;
                    self.request_redraw();
                }
            }
            confirm::Decision::Discard => {
                self.ledger = None;
                self.undo = None;
                self.rehighlight_at = None;
                self.refresh_title();
                self.resolve_confirm(event_loop);
            }
            confirm::Decision::Cancel => {
                self.confirm = None;
                self.request_redraw();
            }
            confirm::Decision::Hold => {}
        }
    }

    /// Restarts the highlight worker over the edited block, the rest
    /// timer's due action. The sweep resumes from the nearest stored
    /// seam at or before the exact frontier, which every splice clamps
    /// to the burst's first touched line, and it converges against the
    /// shifted table: typing anywhere re-colors about one chunk, not
    /// the block.
    fn rehighlight_edited(&mut self) {
        let pending = match self.document.blocks.first().map(|b| &b.kind) {
            Some(BlockKind::CodeBlock {
                language,
                lines,
                exact,
                ..
            }) if !self.document.plain_file => {
                let resume = self.seams.get(&0).map(|table| {
                    let at = table.partition_point(|(line, _)| *line <= *exact);
                    let (start_line, seam) = match at.checked_sub(1).and_then(|i| table.get(i)) {
                        Some((line, seam)) => (*line, Some(seam.clone())),
                        None => (0, None),
                    };
                    highlight::Resume {
                        start_line,
                        seam,
                        expected: table[at..].to_vec(),
                    }
                });
                vec![PendingBlock {
                    block: 0,
                    language: language.clone(),
                    source: self.document.source.clone(),
                    lines: lines.clone(),
                    resume,
                }]
            }
            _ => Vec::new(),
        };
        // The resumed sweep converges against the stored table, which
        // therefore survives the start; its arrivals upsert their own
        // lines.
        let waker = self.waker.clone();
        self.highlighter.start(pending, move || waker());
    }

    /// Repaints the title; the asterisk follows the undo head against
    /// the save point, correct through undo past a save.
    fn refresh_title(&mut self) {
        let dirty = self.undo.as_ref().is_some_and(Undo::is_dirty);
        if let (Some(gfx), Some(path)) = (self.gfx.as_ref(), self.path.as_deref()) {
            gfx.window.set_title(&window_title(
                self.document.title.as_deref(),
                Some(path),
                dirty,
            ));
        }
    }

    /// The source range the standing selection covers, when one does.
    fn selection_source_range(&self) -> Option<std::ops::Range<usize>> {
        let sel = self.selection.filter(|s| !s.is_empty())?;
        caret::selection_range(&self.document, &sel)
    }

    /// The anchor a growing selection keeps: its non-active end.
    fn selection_anchor_offset(&self) -> Option<usize> {
        let sel = self.selection.filter(|s| !s.is_empty())?;
        caret::model_offset(&self.document, &sel.start)
    }

    /// Enter: the markdown continuation when the source view speaks
    /// markdown and no selection stands, else the plain indent carry.
    /// The split point is the selection start when one stands, since
    /// the replacement happens in the same splice and the caret lands
    /// on that line. Structural either way.
    fn press_enter(&mut self) {
        let selected = self.selection_source_range();
        let at = selected
            .clone()
            .map_or_else(|| self.caret.map_or(0, |c| c.offset), |r| r.start);
        let (start, decision, plain) = {
            let source = &self.document.source;
            let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = source[at..].find('\n').map_or(source.len(), |i| at + i);
            let line = &source[start..end];
            let col = at - start;
            let decision = (selected.is_none() && self.markdown_source())
                .then(|| edit::manners::markdown_enter(line, col))
                .flatten();
            (start, decision, edit::manners::enter_text(line, col))
        };
        match decision {
            Some(edit::manners::MarkdownEnter::Insert(text)) => {
                self.type_over(&text, Kind::Structural);
            }
            Some(edit::manners::MarkdownEnter::Unwind(len)) => {
                self.type_edit(start..start + len, "", Kind::Structural);
            }
            None => self.type_over(&plain, Kind::Structural),
        }
    }

    /// Tab and Shift+Tab: the file's indent unit inserted at the
    /// caret, or the touched lines moved whole. A selection spanning
    /// more than one line indents and outdents every line it touches
    /// in one structural unit, and survives the edit.
    fn press_tab(&mut self, outdent: bool) {
        let multi = self
            .selection_source_range()
            .is_some_and(|r| self.document.source[r].contains('\n'));
        if !outdent && !multi {
            // On a markdown list item with the caret at or before the
            // content, where Enter's continuation leaves it, Tab nests
            // the item instead of typing into it.
            if self.selection_source_range().is_none() && self.markdown_source() {
                let at = self.caret.map_or(0, |c| c.offset);
                let source = &self.document.source;
                let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
                let end = source[at..].find('\n').map_or(source.len(), |i| at + i);
                if edit::manners::tab_nests(&source[start..end], at - start) {
                    self.indent_lines(false);
                    return;
                }
            }
            let unit = edit::manners::indent_unit(&self.document.source);
            self.type_over(&unit.text(), Kind::Insert);
            return;
        }
        self.indent_lines(outdent);
    }

    /// The line burst behind press_tab: the lines the selection
    /// touches, or the caret's line alone, re-indented as one splice
    /// and one undo unit. Caret and selection ride the per-line deltas
    /// rather than the splice's own seat, so an outdent never yanks
    /// the caret to the line start.
    fn indent_lines(&mut self, outdent: bool) {
        let unit = edit::manners::indent_unit(&self.document.source);
        let sel = self.selection_source_range();
        let caret_before = self.caret.map_or(0, |c| c.offset);
        let anchor_before = self.selection_anchor_offset();
        let (from, to) = sel.map_or((caret_before, caret_before), |r| (r.start, r.end));
        let source = &self.document.source;
        let start = source[..from].rfind('\n').map_or(0, |i| i + 1);
        // A selection ending at a line start leaves that line alone.
        let last = if to > from && source[..to].ends_with('\n') {
            to - 1
        } else {
            to
        };
        let end = source[last..].find('\n').map_or(source.len(), |i| last + i);
        let (text, deltas) = edit::manners::reindent(&source[start..end], &unit, outdent);
        if deltas.iter().all(|&d| d == 0) {
            return;
        }
        let mut line_starts = vec![start];
        for (i, b) in source[start..end].bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(start + i + 1);
            }
        }
        let map = |p: usize| -> usize {
            let k = line_starts.partition_point(|ls| *ls <= p) - 1;
            let prefix: i64 = deltas[..k].iter().sum();
            let (ls, d) = (line_starts[k], deltas[k]);
            let new = if d < 0 && p < ls + (-d) as usize {
                // Inside the bytes the outdent removed: the position
                // clamps to its line start.
                ls as i64 + prefix
            } else if d > 0 && p == ls {
                // A selection end hugging the line start stays there,
                // keeping the inserted unit inside the selection.
                ls as i64 + prefix
            } else {
                p as i64 + prefix + d
            };
            new as usize
        };
        let caret_after = map(caret_before);
        let anchor_after = anchor_before.map(map);
        let replaced = source[start..end].to_string();
        if !self.apply_edit(start..end, &text) {
            return;
        }
        if let Some(hist) = self.undo.as_mut() {
            hist.record(
                start..end,
                &text,
                &replaced,
                (caret_before, caret_after),
                Kind::Structural,
                Instant::now(),
            );
        }
        self.seat_caret_after_edit(caret_after);
        // The selection apply_edit cleared comes back, its ends mapped.
        if let Some(anchor) = anchor_after.filter(|a| *a != caret_after) {
            if let Some(s) = caret::span_selection(&self.document, anchor, caret_after) {
                self.sel_anchor = Some(s.start);
                self.selection = Some(s);
            }
        }
    }

    /// True while the editor shows a markdown file's own bytes, the
    /// one kind whose Enter continues lists and quotes.
    fn markdown_source(&self) -> bool {
        self.mode == edit::Mode::Edit
            && self
                .path
                .as_deref()
                .is_some_and(|p| load::detect(p) == load::FileKind::Markdown)
    }

    /// Typing consumes the selection when one stands, else the caret
    /// point. Replacing a selection is structural whatever the key.
    fn type_over(&mut self, text: &str, kind: Kind) {
        match self.selection_source_range() {
            Some(range) => self.type_edit(range, text, Kind::Structural),
            None => {
                let at = self.caret.map_or(0, |c| c.offset);
                self.type_edit(at..at, text, kind);
            }
        }
    }

    /// Deletes the standing selection; false when there is none.
    fn delete_selection(&mut self) -> bool {
        match self.selection_source_range() {
            Some(range) => {
                self.type_edit(range, "", Kind::Structural);
                true
            }
            None => false,
        }
    }

    /// Seats the caret at an offset, blink restarted, view snapped.
    fn place_caret_at(&mut self, offset: usize) {
        let view_h = self.viewport_h();
        self.caret = Some(Caret::at(offset));
        self.wake_caret();
        if let (Some(c), Some(lay)) = (self.caret, self.layout.as_ref()) {
            if let Some(b) = c.geometry(lay, &self.document, &mut self.fonts) {
                let target = caret::snap(self.scroll_y, view_h, b);
                if target != self.scroll_y {
                    self.scroll_to(target);
                }
            }
        }
        self.request_redraw();
    }

    /// One caret motion: growing the selection while Shift holds, and
    /// otherwise collapsing it, plain Left and Right to its edges the
    /// way every editor does.
    fn move_caret(&mut self, motion: Motion, shift: bool) {
        if shift {
            let anchor = self
                .selection_anchor_offset()
                .or_else(|| self.caret.map(|c| c.offset));
            self.step_caret(motion);
            let caret_off = self.caret.map(|c| c.offset);
            self.selection = match (anchor, caret_off) {
                (Some(a), Some(c)) if a != c => caret::span_selection(&self.document, a, c),
                _ => None,
            };
            self.band = None;
            self.request_redraw();
            return;
        }
        if let Some(range) = self.selection_source_range() {
            self.selection = None;
            self.sel_anchor = None;
            self.band = None;
            match motion {
                Motion::Left => self.place_caret_at(range.start),
                Motion::Right => self.place_caret_at(range.end),
                _ => self.step_caret(motion),
            }
        } else {
            self.step_caret(motion);
        }
    }

    /// After a mouse selection, the caret takes the active end.
    fn caret_to_selection_edge(&mut self) {
        if let Some(sel) = self.selection {
            if let Some(off) = caret::model_offset(&self.document, &sel.end) {
                self.place_caret_at(off);
            }
        }
    }

    /// Cut is copy plus a delete splice; inert outside edit mode.
    fn cut_selection(&mut self) {
        if self.mode != edit::Mode::Edit {
            return;
        }
        let Some(range) = self.selection_source_range() else {
            return;
        };
        self.copy_selection(false);
        self.type_edit(range, "", Kind::Structural);
    }

    /// Pastes the clipboard as source bytes at the caret, replacing the
    /// selection when one stands; inert outside edit mode.
    fn paste_clipboard(&mut self) {
        if self.mode != edit::Mode::Edit {
            return;
        }
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new()
                .map_err(|err| eprintln!("oryx: no clipboard: {err}"))
                .ok();
        }
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else {
            return;
        };
        // Clipboard line endings normalize like the load; the ledger
        // writes the file's own ending back on save.
        let text = text.replace("\r\n", "\n");
        if !text.is_empty() {
            self.type_over(&text, Kind::Structural);
        }
    }

    /// A click while editing places the caret at the character.
    fn place_caret(&mut self) {
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        if let Some(placed) = caret::place(lay, &self.document, &mut self.fonts, x, y) {
            self.caret = Some(placed);
            if self.selection.take().is_some() {
                self.band = None;
            }
            self.sel_anchor = None;
            self.wake_caret();
            self.request_redraw();
        }
    }

    /// Opens the search bar with the last query standing selected, so
    /// typing replaces it; Ctrl+F on an open bar reselects the same way.
    fn open_search(&mut self) {
        if let Some(state) = self.search.as_mut() {
            state.query.select_all();
            if let Some(row) = state.replace.as_mut() {
                row.focused = false;
            }
            state.doc_intent = false;
            self.request_redraw();
            return;
        }
        let mut query = TextField::new(self.last_query.clone());
        query.select_all();
        self.search = Some(SearchState {
            query,
            regex: self.last_regex,
            error: false,
            replace: None,
            seek: None,
            doc_intent: false,
            matches: Vec::new(),
            rects: Vec::new(),
            rects_scroll: 0.0,
            current: 0,
            stale: true,
            settle: false,
        });
        self.band = None;
        self.request_redraw();
    }

    /// Opens find and replace: the search bar with the replace row,
    /// editing only. On an already open bar the focus moves to the
    /// replace field; a fresh open starts in the query.
    fn open_replace(&mut self) {
        if self.mode != edit::Mode::Edit {
            return;
        }
        let was_open = self.search.is_some();
        self.open_search();
        let Some(state) = self.search.as_mut() else {
            return;
        };
        if state.replace.is_none() {
            state.replace = Some(ReplaceRow {
                field: TextField::new(self.last_replace.clone()),
                focused: false,
            });
        }
        let row = state.replace.as_mut().expect("row just ensured");
        row.focused = was_open;
        if was_open {
            row.field.select_all();
        }
        self.request_redraw();
    }

    fn close_search(&mut self) {
        self.search_mouse = false;
        if let Some(state) = self.search.take() {
            self.last_query = state.query.text().to_string();
            self.last_regex = state.regex;
            if let Some(row) = state.replace.as_ref() {
                self.last_replace = row.field.text().to_string();
            }
            self.band = None;
            self.request_redraw();
        }
    }

    /// Flips the search bar between plain and regex matching.
    fn toggle_search_regex(&mut self) {
        let Some(state) = self.search.as_mut() else {
            return;
        };
        state.regex = !state.regex;
        state.stale = true;
        let regex = state.regex;
        self.last_regex = regex;
        self.band = None;
        self.request_redraw();
    }

    /// The source edits a replace acts on, in match order: the current
    /// match alone or every match, plain text literal, regex expanded
    /// with each match's own captures. `None` when replacing cannot
    /// proceed: not editing, no bar or row, an invalid or stale pattern,
    /// no matches, or a match list that no longer describes this text.
    fn replacement_edits(&mut self, all: bool) -> Option<Vec<(std::ops::Range<usize>, String)>> {
        if self.mode != edit::Mode::Edit {
            return None;
        }
        self.sync_search();
        let state = self.search.as_ref()?;
        if state.error || state.stale || state.matches.is_empty() {
            return None;
        }
        let template = state.replace.as_ref()?.field.text().to_string();
        let pairs: Vec<(Selection, String)> = if state.regex {
            let pairs = search::regex_replacements(&self.document, state.query.text(), &template)?;
            // The pairs mirror the match list; a length drift means the
            // matches no longer describe this text, so the replace waits.
            if pairs.len() != state.matches.len() {
                return None;
            }
            pairs
        } else {
            state
                .matches
                .iter()
                .map(|sel| (*sel, template.clone()))
                .collect()
        };
        let index = state.current.min(pairs.len() - 1);
        let picked = if all {
            pairs
        } else {
            vec![pairs.into_iter().nth(index).expect("index bounded")]
        };
        let edits: Vec<(std::ops::Range<usize>, String)> = picked
            .into_iter()
            .filter_map(|(sel, text)| {
                caret::selection_range(&self.document, &sel).map(|range| (range, text))
            })
            .collect();
        (!edits.is_empty()).then_some(edits)
    }

    /// Splices the edits as one structural undo unit and seats the
    /// search past them: the recompute honors the seek once the relayout
    /// lets it run, and the splice was a document act, so Ctrl+Z right
    /// after takes it back.
    fn apply_replacements(
        &mut self,
        edits: Vec<(std::ops::Range<usize>, String)>,
    ) -> Option<usize> {
        let count = edits.len();
        let (region, text) = splice::combine(&self.document.source, &edits)?;
        self.selection = None;
        let after = region.start + text.len();
        self.type_edit(region, &text, Kind::Structural);
        if let Some(state) = self.search.as_mut() {
            state.seek = Some(after);
            state.doc_intent = true;
        }
        self.request_redraw();
        Some(count)
    }

    /// Replaces the current match and advances to the first match at or
    /// after the splice.
    fn replace_current(&mut self) {
        if let Some(edits) = self.replacement_edits(false) {
            self.apply_replacements(edits);
        }
    }

    /// Replaces every match in one region splice, one undo unit and one
    /// relayout, and reports the count in the corner notice.
    fn replace_all(&mut self) {
        if let Some(edits) = self.replacement_edits(true) {
            if let Some(count) = self.apply_replacements(edits) {
                self.show_notice(&format!("{count} replaced"));
            }
        }
    }

    /// A left press anywhere on the search bar, consumed before the
    /// document sees the click: the toggle flips the mode, and a row
    /// click focuses that row's field, a bar act either way.
    fn search_bar_press(&mut self) -> bool {
        let Some(replace_row) = self.search.as_ref().map(|state| state.replace.is_some()) else {
            return false;
        };
        let Some(width) = self.logical_width() else {
            return false;
        };
        let (x, y) = self.ui_cursor();
        let Some(hit) = search::bar_hit(width, replace_row, x, y) else {
            return false;
        };
        self.search_mouse = true;
        match hit {
            BarHit::Toggle => self.toggle_search_regex(),
            BarHit::Query | BarHit::Replace => {
                let state = self.search.as_mut().expect("search open");
                if let Some(row) = state.replace.as_mut() {
                    row.focused = matches!(hit, BarHit::Replace);
                }
                state.doc_intent = false;
                self.request_redraw();
            }
        }
        true
    }

    /// Window width in logical units, the space bar geometry lives in.
    fn logical_width(&self) -> Option<f32> {
        self.gfx
            .as_ref()
            .map(|g| g.window.inner_size().width as f32 / self.scale)
    }

    /// Moves to the neighboring match; with the bar closed, reopens it.
    fn step_search(&mut self, forward: bool) {
        let Some(state) = self.search.as_mut() else {
            self.open_search();
            return;
        };
        state.query.set_caret(usize::MAX);
        if state.matches.is_empty() {
            return;
        }
        state.current = search::step(state.current, state.matches.len(), forward);
        let block = state.matches[state.current].ordered().0.block;
        self.band = None;
        if self.document.reveal(block) {
            // The match sits inside a folded details group: open the
            // chain, restart the pass, and let the settle path center it
            // once the region is placed.
            if let Some(state) = self.search.as_mut() {
                state.settle = true;
            }
            self.restart_layout();
        } else {
            self.scroll_match_into_view();
        }
        self.request_redraw();
    }

    /// Keys the open search bar consumes: query edits, Enter stepping,
    /// Escape closing. Everything else falls through to the document, so
    /// scrolling and shortcuts stay live under the bar.
    fn search_key(&mut self, key: &Key, ctrl: bool, shift: bool) -> bool {
        if self.search.is_none() {
            return false;
        }
        // Alt chords belong to the desktop, except the bar's own toggle.
        if self.modifiers.alt_key() {
            if matches!(key, Key::Character(s) if s.eq_ignore_ascii_case("r")) {
                self.toggle_search_regex();
                return true;
            }
            return false;
        }
        match key {
            Key::Named(NamedKey::Escape) => {
                self.close_search();
                true
            }
            // While the replace row is open, Enter replaces from either
            // field, since Ctrl+H announced the intent, and Ctrl+Enter
            // replaces every match; F3 and Shift+Enter still step
            // without replacing.
            Key::Named(NamedKey::Enter) => {
                let replacing = self
                    .search
                    .as_ref()
                    .is_some_and(|state| state.replace.is_some());
                if replacing && ctrl {
                    self.replace_all();
                } else if replacing && !shift {
                    self.replace_current();
                } else {
                    self.step_search(!shift);
                }
                true
            }
            // Tab moves between the two fields while the replace row is
            // shown; without it the key keeps its editor meaning.
            Key::Named(NamedKey::Tab) => {
                let state = self.search.as_mut().expect("search open");
                if let Some(row) = state.replace.as_mut() {
                    row.focused = !row.focused;
                    state.doc_intent = false;
                    self.request_redraw();
                    true
                } else {
                    false
                }
            }
            Key::Character(s) if ctrl && s.eq_ignore_ascii_case("v") => {
                if self.clipboard.is_none() {
                    self.clipboard = arboard::Clipboard::new()
                        .map_err(|err| eprintln!("oryx: no clipboard: {err}"))
                        .ok();
                }
                let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok());
                if let Some(text) = text {
                    self.push_query(&text);
                }
                true
            }
            // Copy and cut act on the focused field's own selection;
            // without one the key keeps its document meaning.
            Key::Character(s) if ctrl && s.eq_ignore_ascii_case("c") && !shift => {
                match self
                    .search
                    .as_ref()
                    .and_then(SearchState::focused_selection)
                {
                    Some(text) => {
                        self.set_clipboard(text);
                        true
                    }
                    None => false,
                }
            }
            Key::Character(s) if ctrl && s.eq_ignore_ascii_case("x") && !shift => {
                match self
                    .search
                    .as_ref()
                    .and_then(SearchState::focused_selection)
                {
                    Some(text) => {
                        self.set_clipboard(text);
                        let state = self.search.as_mut().expect("search open");
                        let on_query = !state.replace_focused();
                        state.focused_mut().delete_selection();
                        state.doc_intent = false;
                        if on_query {
                            state.stale = true;
                            self.band = None;
                        }
                        self.request_redraw();
                        true
                    }
                    None => false,
                }
            }
            key => {
                let state = self.search.as_mut().expect("search open");
                // With intent on the document, undo and redo pass
                // through to it instead of the field.
                if state.doc_intent
                    && ctrl
                    && matches!(key, Key::Character(s) if s.eq_ignore_ascii_case("z"))
                {
                    return false;
                }
                let on_query = !state.replace_focused();
                match state.focused_mut().key(key, ctrl, shift) {
                    Edit::Ignored => false,
                    // A claimed key is a bar act either way, so the
                    // intent turns back to the field.
                    Edit::Handled => {
                        state.doc_intent = false;
                        self.request_redraw();
                        true
                    }
                    Edit::Changed => {
                        state.doc_intent = false;
                        if on_query {
                            state.stale = true;
                            self.band = None;
                        }
                        self.request_redraw();
                        true
                    }
                }
            }
        }
    }

    /// Inserts into the focused field at its caret, replacing the
    /// selection. The field drops control characters a paste may carry,
    /// since a tab or newline could otherwise cross line boundaries.
    fn push_query(&mut self, text: &str) {
        let Some(state) = self.search.as_mut() else {
            return;
        };
        let on_query = !state.replace_focused();
        if state.focused_mut().insert(text) != Edit::Changed {
            return;
        }
        state.doc_intent = false;
        if on_query {
            state.stale = true;
            self.band = None;
        }
        self.request_redraw();
    }

    /// Recomputes stale matches against the current layout, then keeps
    /// the reading position: the current match becomes the first at or
    /// below the viewport top. Runs inside redraw once layout exists, so
    /// query edits and relayouts from zoom, resize, or reload all land
    /// here.
    fn sync_search(&mut self) {
        if !self.search.as_ref().is_some_and(|s| s.stale) {
            return;
        }
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let scroll = self.scroll_y;
        let state = self.search.as_mut().expect("search open");
        let prev = state.matches.get(state.current).copied();
        let found = if state.regex {
            search::regex_matches(&self.document, state.query.text())
        } else {
            Some(search::matches(&self.document, state.query.text()))
        };
        state.error = found.is_none();
        state.matches = found.unwrap_or_default();
        state.stale = false;
        let seek = state.seek.take();
        // When the recompute yields the same match (a recolor or layout
        // growth went stale, not the text or the query), the current one
        // keeps its identity and the view stays put. Otherwise a replace
        // seek beats the viewport rule, so the current match moves past
        // the splice instead of re-seating at the top of the screen.
        let kept = match seek {
            Some(_) => None,
            None => prev.and_then(|p| state.matches.iter().position(|s| *s == p)),
        };
        if let Some(index) = kept {
            state.current = index;
        } else {
            let document = &self.document;
            let tops = selection::match_tops(lay, &state.matches);
            state.current = match seek {
                Some(offset) => state
                    .matches
                    .iter()
                    .position(|sel| {
                        caret::model_offset(document, &sel.ordered().0)
                            .is_some_and(|at| at >= offset)
                    })
                    .unwrap_or(0),
                None => tops.iter().position(|top| *top >= scroll).unwrap_or(0),
            };
            self.scroll_match_into_view();
        }
        self.refresh_search_rects();
    }

    /// Recomputes the highlight rects for the matches inside the band
    /// window around the current scroll. Bounding the geometry is what
    /// keeps a keystroke over thousands of matches cheap; scrolling past
    /// the window refreshes it.
    fn refresh_search_rects(&mut self) {
        let vh = self.viewport_h();
        let (lo, hi) = (self.scroll_y - 2.0 * vh, self.scroll_y + 3.0 * vh);
        let scroll = self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let Some(state) = self.search.as_mut() else {
            return;
        };
        if state.stale {
            return;
        }
        let mut rects = Vec::new();
        // One shaped buffer per run for the whole pass, however many
        // matches the run holds; geometry only inside the band window.
        let mut shaped = selection::ShapeCache::default();
        let tops = selection::match_tops(lay, &state.matches);
        for (index, m) in state.matches.iter().enumerate() {
            if tops[index] < lo || tops[index] > hi {
                continue;
            }
            for rect in selection::rects_window(
                m,
                lay,
                &self.document,
                &mut self.fonts,
                &mut shaped,
                lo,
                hi,
            ) {
                rects.push((index, rect));
            }
        }
        state.rects = rects;
        state.rects_scroll = scroll;
    }

    /// Centers the current match vertically when it sits off screen. A
    /// cold match scrolls to its recorded top from the block table and
    /// settles on the exact anchor once the slide materializes it.
    fn scroll_match_into_view(&mut self) {
        let (Some(lay), Some(state)) = (self.layout.as_ref(), self.search.as_ref()) else {
            return;
        };
        let Some(m) = state.matches.get(state.current) else {
            return;
        };
        let anchor = selection::match_anchor(lay, m);
        let (top, size) = match anchor {
            Some(exact) => exact,
            None => {
                let pos = m.ordered().0;
                match lay.approx_top(pos.block, pos.span) {
                    Some(top) => (top, self.cfg.body_size * self.cfg.zoom),
                    None => return,
                }
            }
        };
        if anchor.is_none() {
            if let Some(state) = self.search.as_mut() {
                state.settle = true;
            }
        }
        let line_h = metrics::LINE_HEIGHT * size;
        let vh = self.viewport_h();
        if top < self.scroll_y || top + line_h > self.scroll_y + vh {
            self.scroll_to(top - (vh - line_h) / 2.0);
        }
    }

    /// Finishes a cold match's landing: once the slide materializes the
    /// region, the exact anchor centers it and the settle flag clears.
    fn settle_search_anchor(&mut self) {
        if !self.search.as_ref().is_some_and(|s| s.settle && !s.stale) {
            return;
        }
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let state = self.search.as_ref().expect("search open");
        let Some(m) = state.matches.get(state.current) else {
            self.search.as_mut().expect("search open").settle = false;
            return;
        };
        let Some((top, size)) = selection::match_anchor(lay, m) else {
            return;
        };
        self.search.as_mut().expect("search open").settle = false;
        let line_h = metrics::LINE_HEIGHT * size;
        let vh = self.viewport_h();
        if top < self.scroll_y || top + line_h > self.scroll_y + vh {
            self.scroll_to(top - (vh - line_h) / 2.0);
        }
    }

    fn thumb(&self) -> Option<(f32, f32)> {
        let vh = self.viewport_h();
        scrollbar::thumb(self.doc_height(), vh, self.scroll_y, vh, self.scale)
    }

    fn drag_to(&mut self, cursor_y: f32) {
        let (Some(Drag::Scrollbar(grab)), Some((_, thumb_h))) = (self.drag, self.thumb()) else {
            return;
        };
        let vh = self.viewport_h();
        let target =
            scrollbar::scroll_for_thumb(cursor_y - grab, thumb_h, vh, self.doc_height(), vh);
        self.scroll_to(target);
    }

    /// Advances the multi-click chain for a document-area press and
    /// answers the click count it reached.
    fn register_click(&mut self) -> u8 {
        // Logical coordinates keep the chain's slop radius the same
        // size on every display.
        let (x, y) = self.ui_cursor();
        let now = Instant::now();
        let prev = self.last_click.map(|(t, px, py, count)| {
            (
                now.duration_since(t) <= Duration::from_millis(400),
                px,
                py,
                count,
            )
        });
        let count = selection::click_chain(
            prev.map(|(_, px, py, count)| (count, px, py)),
            prev.is_some_and(|(within, ..)| within),
            x,
            y,
        );
        self.last_click = Some((now, x, y, count));
        count
    }

    /// Selects the word (double click) or the paragraph, code line, or
    /// table cell (triple click) under the cursor. No anchor is set, so
    /// a following cursor move does not re-extend character-wise.
    fn select_unit(&mut self, paragraph: bool) {
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let Some(pos) = selection::pos_at(lay, &self.document, &mut self.fonts, x, y) else {
            return;
        };
        self.sel_anchor = None;
        let sel = if paragraph {
            selection::paragraph_at(&self.document, pos)
        } else {
            selection::word_at(&self.document, pos)
        };
        if let Some(sel) = sel {
            self.selection = Some(sel);
            self.band = None;
            self.request_redraw();
        }
    }

    /// Grabs a selection anchor at the cursor and clears any previous
    /// selection.
    fn begin_selection(&mut self) {
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        self.sel_anchor = selection::pos_at(lay, &self.document, &mut self.fonts, x, y);
        if self.selection.take().is_some() {
            self.band = None;
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        }
    }

    /// Extends the selection from the anchor to the cursor during a drag.
    fn extend_selection(&mut self) {
        let Some(start) = self.sel_anchor else {
            return;
        };
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let Some(end) = selection::pos_at(lay, &self.document, &mut self.fonts, x, y) else {
            return;
        };
        let sel = Selection { start, end };
        if self.selection != Some(sel) {
            self.selection = Some(sel);
            self.band = None;
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        }
    }

    /// Ends a selection drag. A drag that never left its starting caret is
    /// a click and follows the link under the cursor instead.
    fn end_selection(&mut self) {
        self.sel_anchor = None;
        if self.selection.is_some_and(|s| !s.is_empty()) {
            // The drag's release point is where editing continues.
            if self.mode == edit::Mode::Edit {
                self.caret_to_selection_edge();
            }
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        } else {
            self.selection = None;
            // While editing, a click already placed the caret; a link
            // never follows.
            if self.mode == edit::Mode::Read {
                self.link_press();
            }
        }
        self.fold_highlights();
    }

    /// A left button press at the current cursor, from the mouse or a
    /// synthesized tap.
    fn left_press(&mut self) {
        // A stale latch from a lost release must not eat this press's
        // own release.
        self.search_mouse = false;
        if let Some(overlay) = self.overlay.as_mut() {
            self.overlay_mouse = true;
            let (x, y) = (
                self.cursor.x as f32 / self.scale,
                self.cursor.y as f32 / self.scale,
            );
            let result = overlay.click(x, y);
            self.overlay_result(result);
        } else if self.search_bar_press() {
        } else {
            // A click anywhere off the bar closes it, the panels' own
            // outside-click manner; the click still acts on what it hit,
            // and the document owns every key again.
            self.close_search();
            if self.sidebar_edge_press() {
            } else if (self.cursor.x as f32) < self.inset() && self.sidebar.is_some() {
                let (x, y) = self.ui_cursor();
                self.sidebar_click(x, y);
                self.move_ownership(PaneAct::ClickSidebar);
            } else {
                self.move_ownership(PaneAct::ClickDocument);
                self.scrollbar_press();
                if self.drag.is_none() {
                    if self.mode == edit::Mode::Edit {
                        match self.register_click() {
                            2 => {
                                self.select_unit(false);
                                self.caret_to_selection_edge();
                            }
                            3 => {
                                self.select_unit(true);
                                self.caret_to_selection_edge();
                            }
                            _ => {
                                self.place_caret();
                                self.begin_selection();
                            }
                        }
                    } else {
                        match self.register_click() {
                            2 => self.select_unit(false),
                            3 => self.select_unit(true),
                            _ => self.begin_selection(),
                        }
                    }
                }
            }
        }
    }

    /// The matching release.
    fn left_release(&mut self) {
        self.overlay_mouse = false;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.release();
        } else if self.search_mouse {
            self.search_mouse = false;
        } else if let Some(drag) = self.drag.take() {
            if drag_is_edge(drag) {
                self.save_sidebar_state();
            }
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        } else {
            self.end_selection();
        }
        // The gesture held the pass off, and possibly a parked parse
        // delivery. A release that changed nothing else still has to
        // hand the loop back to them.
        self.fold_parse();
        if self.layout_pending() {
            self.request_redraw();
        }
    }

    /// Emulated mouse events shadowing an active or just-finished touch
    /// pan are dropped; a tap's emulated click passes through.
    fn mouse_muted(&self) -> bool {
        self.touch.panning()
            || self
                .mute_mouse_until
                .is_some_and(|until| Instant::now() < until)
    }

    /// Routes a raw touch event: a swipe scrolls the pane under the
    /// gesture's origin, a pinch drives the reader zoom, and a tap
    /// clicks where the platform does not emulate one.
    fn on_touch(&mut self, event: winit::event::Touch) {
        let phase = match event.phase {
            TouchPhase::Started => touch::Phase::Started,
            TouchPhase::Moved => touch::Phase::Moved,
            TouchPhase::Ended => touch::Phase::Ended,
            TouchPhase::Cancelled => touch::Phase::Cancelled,
        };
        if matches!(phase, touch::Phase::Started) {
            // A landing finger catches a flinging page.
            self.fling = None;
        }
        let (x, y) = (event.location.x as f32, event.location.y as f32);
        let act = self.touch.on(event.id, phase, x, y, Instant::now());
        match act {
            touch::Act::None => {}
            touch::Act::PanStart => {
                let start_x = self.touch.start().map_or(x, |(sx, _)| sx);
                self.pan_target = if self.overlay.is_some() {
                    PanTarget::Overlay
                } else if self.sidebar.is_some() && start_x < self.inset() {
                    PanTarget::Sidebar
                } else {
                    PanTarget::Document
                };
                // The pre-slop tail of the gesture may have pressed the
                // emulated mouse button; a swipe selects nothing.
                self.sel_anchor = None;
                self.drag = None;
                match self.pan_target {
                    PanTarget::Sidebar => self.move_ownership(PaneAct::WheelSidebar),
                    PanTarget::Document => self.move_ownership(PaneAct::WheelDocument),
                    PanTarget::Overlay => {}
                }
            }
            touch::Act::Pan { dy } => match self.pan_target {
                PanTarget::Document => self.scroll_by(dy),
                PanTarget::Sidebar => {
                    let lines = dy / (sidebar::ROW_H * self.scale);
                    if let Some(side) = self.sidebar.as_mut() {
                        side.wheel(lines, &mut self.outline);
                        self.request_redraw();
                    }
                }
                PanTarget::Overlay => {
                    let lines = dy / self.line_step();
                    if let Some(overlay) = self.overlay.as_mut() {
                        let result = overlay.scroll(lines);
                        self.overlay_result(result);
                    }
                }
            },
            touch::Act::Tap { x, y } => {
                // Windows emulates a mouse click for every tap;
                // synthesizing another would double it. Linux delivers
                // only the touch, so the tap clicks here.
                #[cfg(windows)]
                let _ = (x, y);
                #[cfg(not(windows))]
                {
                    self.cursor = PhysicalPosition::new(x as f64, y as f64);
                    self.left_press();
                    self.left_release();
                }
            }
            touch::Act::Fling { velocity } => {
                if self.pan_target == PanTarget::Document {
                    self.fling = Some((velocity, Instant::now()));
                }
                self.mute_mouse_until = Some(Instant::now() + MOUSE_MUTE);
            }
            touch::Act::PinchStart => {
                self.pinch_base = self.user_zoom;
                self.sel_anchor = None;
                self.drag = None;
            }
            touch::Act::Pinch { factor } => {
                if self.overlay.is_none() {
                    self.user_zoom =
                        (self.pinch_base * factor).clamp(settings::ZOOM_MIN, settings::ZOOM_MAX);
                    self.apply_zoom();
                }
            }
            touch::Act::End => {
                self.mute_mouse_until = Some(Instant::now() + MOUSE_MUTE);
            }
        }
    }

    /// Advances a running fling and keeps the loop ticking while it
    /// lasts. Friction or the document's edge retires it.
    fn step_fling(&mut self, event_loop: &ActiveEventLoop) {
        let Some((velocity, at)) = self.fling else {
            return;
        };
        let now = Instant::now();
        let dt = now.duration_since(at).as_secs_f32();
        if dt > 0.0 {
            let (delta, next) = touch::fling_step(velocity, dt);
            let before = self.scroll_y;
            self.scroll_by(delta);
            let walled = delta.abs() >= 0.5 && self.scroll_y == before;
            if touch::fling_done(next, self.scale) || walled {
                self.fling = None;
                return;
            }
            self.fling = Some((next, now));
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(now + FLING_TICK));
    }

    /// Hands pending highlight work to the worker. An empty list still
    /// bumps the generation, so arrivals from the previous document are
    /// dropped at the next drain. Every start here sweeps blocks from
    /// their first line, so the seam table rebuilds from its arrivals.
    fn start_highlight(&mut self, pending: Vec<PendingBlock>) {
        self.seams.clear();
        self.spec_band = None;
        let waker = self.waker.clone();
        self.highlighter.start(pending, move || waker());
    }

    /// Cancels in-flight highlight work without touching the seam
    /// table: a keystroke kills the worker between chunks, and the
    /// rest timer's resumed sweep still needs the seams the old
    /// generation recorded. Stale arrivals drop at the drain, so the
    /// table only ever learns from the live generation.
    fn cancel_highlight(&mut self) {
        self.spec_band = None;
        let waker = self.waker.clone();
        self.highlighter.start(Vec::new(), move || waker());
    }

    /// Colors the visible band ahead of the exact sweep when the view
    /// rests on lines the sweep has not reached, so a jump into a huge
    /// file does not sit on plain text for the length of the wash. The
    /// band is padded a screen each way; a band the standing request
    /// already covers asks for nothing.
    fn maybe_speculate(&mut self) {
        if self.document.plain_file || self.drag.is_some() || self.sel_anchor.is_some() {
            return;
        }
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let view_h = self.viewport_h();
        let visible = layout::code_lines_in(
            lay,
            &self.document,
            self.scroll_y - view_h..self.scroll_y + 2.0 * view_h,
        );
        for (block, lines) in visible {
            let Some(BlockKind::CodeBlock {
                language,
                lines: body,
                exact,
                ..
            }) = self.document.blocks.get(block).map(|b| &b.kind)
            else {
                continue;
            };
            if language.is_none() || *exact >= lines.end {
                continue;
            }
            let band = (*exact).max(lines.start)..lines.end;
            if self
                .spec_band
                .as_ref()
                .is_some_and(|(b, r)| *b == block && r.start <= band.start && r.end >= band.end)
            {
                continue;
            }
            let job = highlight::SpecJob {
                block,
                language: language.clone(),
                source: Arc::clone(&self.document.source),
                lines: body.clone(),
                range: band.clone(),
            };
            self.spec_band = Some((block, band));
            let waker = self.waker.clone();
            self.highlighter.speculate(job, move || waker());
            return;
        }
    }

    /// Hands the full source to the parse worker; the prefix on screen
    /// grows into its delivery when it lands.
    fn start_parse(&mut self) {
        self.parse_pending = true;
        let waker = self.waker.clone();
        self.parser
            .start(self.document.source.clone(), move || waker());
    }

    /// Continues a book past its prefix on the parse worker. The
    /// prefix's image sources adopt here, before the first layout, so
    /// every prefix size is known from the first frame; pixels decode
    /// on demand as paint reaches them.
    fn start_book(&mut self, mut job: epub::BookJob) {
        self.media.adopt(job.take_sources());
        if job.has_chapters() {
            let sources = self.media.source_sink();
            self.parse_pending = true;
            let waker = self.waker.clone();
            self.parser
                .start_with(move |bail| epub::run(job, bail, sources), move || waker());
        }
    }

    /// Lands a parked parse delivery. Deferred while any drag is live: a
    /// replace would pull the layout out from under it.
    fn fold_parse(&mut self) {
        if self.sel_anchor.is_some() || self.drag.is_some() {
            return;
        }
        if let Some(delivered) = self.parser.drain() {
            self.land_parse(delivered);
        }
    }

    /// Joins the parse worker and lands its document now, for the
    /// completions that need the whole model.
    fn finish_parse(&mut self) {
        if let Some(delivered) = self.parser.finish() {
            self.land_parse(delivered);
        }
    }

    /// Lands the worker's blocks. A splice appends behind the kept prefix
    /// and the pass resumes over the tail, moving nothing on screen; a
    /// replace swaps the model and relayouts from scratch with the scroll
    /// held. Either way the highlight worker restarts over the whole
    /// document.
    fn land_parse(&mut self, delivered: stream::Delivered) {
        // Owed recolors index the pre-swap model; they apply while every
        // index is still valid.
        self.flush_recolor();
        self.parse_pending = false;
        let stream::Delivered {
            blocks,
            details,
            source,
            anchors,
        } = delivered;
        // A book's delivery grows the source; the prefix is its head bit
        // for bit, so every kept range stays valid on the longer text.
        if let Some(source) = source {
            self.document.source = source;
            self.document.anchors = anchors.into_iter().collect();
        }
        let spliced = match stream::swap(&self.document.blocks, blocks) {
            stream::Swap::Splice(tail) => {
                self.document.blocks.extend(tail);
                // The tail's group ids index the full parse's vector;
                // toggles already made on prefix groups carry over.
                self.document.details = stream::adopt_details(&self.document.details, details);
                if self.book_toc.is_empty() {
                    self.outline.extend(&self.document);
                } else {
                    self.outline.re_resolve(&self.document);
                }
                self.pass
                    .as_mut()
                    .is_some_and(|pass| layout_extend(&self.document, pass))
            }
            stream::Swap::Replace(blocks) => {
                self.document.blocks = blocks;
                self.document.details = details;
                self.outline = OutlineTree::build(&self.document);
                false
            }
        };
        if spliced {
            // Append-only growth: placed positions and the painted band
            // stay valid and the selection keeps its runs. Search grows
            // stale to pick up the tail.
            if let Some(state) = self.search.as_mut() {
                state.stale = true;
            }
        } else {
            self.layout = None;
            self.pass = None;
            self.band = None;
            self.pending_band_for = None;
            self.selection = None;
            self.sel_anchor = None;
        }
        self.start_highlight(load::pending(&self.document));
        if let Some(pass) = self.pass.as_mut() {
            pass.invalidate_pool();
        }
        self.request_redraw();
    }

    /// Folds queued highlight chunks into the document and recolors the
    /// affected laid-out lines in one batch: a backlog costs one pass
    /// over the run vector however many arrivals it holds. Deferred while
    /// a selection drag is active; releasing the mouse folds the queue.
    fn fold_highlights(&mut self) {
        if self.sel_anchor.is_some() {
            return;
        }
        let arrivals = self.highlighter.drain();
        if arrivals.is_empty() {
            // The worker's last chunk can land between wakes; the wave
            // timer in about_to_wait picks the leftovers up.
            if !self.pending_recolor.is_empty() && !self.highlighter.is_running() {
                self.flush_recolor();
            }
            return;
        }
        for arrival in &arrivals {
            load::fold(&mut self.document, arrival);
            // The exact sweep's seams feed the table a later sweep
            // resumes from; speculative arrivals never do.
            if let (Some(seam), false) = (&arrival.seam, arrival.speculative) {
                let line = arrival.start_line + arrival.spans.len();
                let table = self.seams.entry(arrival.block).or_default();
                highlight::record_seam(table, line, seam);
            }
        }
        self.pending_recolor.extend(
            arrivals
                .iter()
                .map(|a| (a.block, a.start_line..a.start_line + a.spans.len())),
        );
        // Recolors land in waves: the model folds immediately, but the
        // run vector rebuilds at most once per wave, so a trickling
        // wash-in pays a handful of ranged rebuilds, not hundreds.
        if self.last_recolor.elapsed() >= RECOLOR_WAVE || !self.highlighter.is_running() {
            self.flush_recolor();
        }
    }

    /// Applies the owed recolors in one ranged rebuild and resets the
    /// relayout contract state.
    fn flush_recolor(&mut self) {
        self.last_recolor = Instant::now();
        if self.pending_recolor.is_empty() {
            return;
        }
        let patches = std::mem::take(&mut self.pending_recolor);
        if let Some(lay) = self.layout.as_mut() {
            let spliced = recolor_batch(
                lay,
                &self.document,
                &self.theme,
                &mut self.fonts,
                &self.cfg,
                &patches,
            );
            if spliced.is_some() {
                // Selection and matches anchor on the model, which the
                // recolor does not touch; only geometry refreshes.
                if let Some(state) = self.search.as_mut() {
                    state.stale = true;
                }
                self.band = None;
                self.pending_band_for = None;
            }
        }
        // Seeded jobs cloned the model before the fold; the reseed reads
        // it fresh.
        if let Some(pass) = self.pass.as_mut() {
            pass.invalidate_pool();
        }
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// Opens the theme browser, or closes the active overlay.
    fn toggle_theme_browser(&mut self) {
        if self.overlay.is_some() {
            self.overlay_result(OverlayResult::Close);
        } else {
            self.pre_browse = Some(self.theme.clone());
            self.overlay = Some(Box::new(ThemeBrowser::new(
                theme_dirs(),
                &self.config.theme,
            )));
            self.request_redraw();
        }
    }

    /// Opens the settings dialog, or closes the active overlay.
    fn toggle_settings(&mut self) {
        if self.overlay.is_some() {
            self.overlay_result(OverlayResult::Close);
        } else {
            self.overlay = Some(Box::new(Settings::new(
                self.fonts.families(),
                self.cfg.body_family.clone(),
                self.cfg.code_family.clone(),
                self.cfg.body_size,
                self.cfg.code_size,
                self.config.ui_scale,
            )));
            self.request_redraw();
        }
    }

    /// Grabs the sidebar's right edge when the cursor is on it, and
    /// restores the default width on a double click. Reports whether the
    /// press belonged to the edge.
    fn sidebar_edge_press(&mut self) -> bool {
        let (Some(side), x) = (self.sidebar.as_ref(), self.cursor.x as f32 / self.scale) else {
            return false;
        };
        if !sidebar::on_edge(side.width(), x) {
            return false;
        }
        let now = Instant::now();
        let again = self
            .last_edge_click
            .is_some_and(|at| now.duration_since(at) < input::DOUBLE_CLICK);
        if again {
            self.last_edge_click = None;
            self.resize_sidebar(sidebar::DEFAULT_WIDTH);
            // No drag release follows a double click, so the reset
            // persists here or not at all.
            self.save_sidebar_state();
        } else {
            self.last_edge_click = Some(now);
            self.drag = Some(Drag::SidebarEdge(x - side.width()));
        }
        true
    }

    /// Applies a dragged width, in logical units, and relays out the
    /// document under it.
    fn resize_sidebar(&mut self, want: f32) {
        let window_w = self
            .gfx
            .as_ref()
            .map(|g| g.window.inner_size().width as f32 / self.scale)
            .unwrap_or(want + sidebar::MIN_WIDTH);
        let before = self.inset();
        if let Some(side) = self.sidebar.as_mut() {
            side.set_width(want, window_w);
        }
        if self.inset() != before {
            self.band = None;
            self.sidebar_canvas = None;
            // The same deferral a window resize gets: restarting a
            // slow pass on every dragged frame would strand the reader
            // at the top for the whole drag.
            if self.layout.is_some() && scroll::defer_relayout(self.last_pass, SLICE) {
                self.settle_at = Some(Instant::now() + scroll::SETTLE);
            }
        }
        self.request_redraw();
    }

    /// Folder of the open document, if it has one.
    fn document_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|d| !d.as_os_str().is_empty())
    }

    /// Folder the last run was left browsing, if one was recorded.
    fn remembered_dir(&self) -> Option<PathBuf> {
        (!self.config.last_dir.is_empty()).then(|| PathBuf::from(&self.config.last_dir))
    }

    /// Records a folder as where the reader was last looking. Called when a
    /// file opens and when the sidebar re-roots, so browsing away without
    /// opening anything is still remembered.
    fn remember_dir(&mut self, dir: &Path) {
        let text = dir.display().to_string();
        if text.is_empty() || self.config.last_dir == text {
            return;
        }
        self.config.last_dir = text;
        config::save(&self.config);
    }

    /// Routes a click inside the panel: a tab switch persists, a file
    /// open runs the browse bookkeeping, an outline jump rides the
    /// anchor path.
    fn sidebar_click(&mut self, x: f32, y: f32) {
        let Some(side) = self.sidebar.as_mut() else {
            return;
        };
        let before = side.root().to_path_buf();
        let click = side.click(x, y, &mut self.outline);
        let after = side.root().to_path_buf();
        let tab = side.tab();
        if after != before {
            self.remember_dir(&after);
        }
        match click {
            sidebar::SideClick::Open(path) => {
                if self.guard_unsaved(confirm::Pending::Open(path.clone(), false)) {
                    self.open_file(&path, false);
                }
            }
            sidebar::SideClick::Jump(block) => self.jump_to_heading(block),
            sidebar::SideClick::Tab => {
                self.config.sidebar_tab = tab;
                config::save(&self.config);
            }
            sidebar::SideClick::None => {}
        }
        self.request_redraw();
    }

    /// Toggles the sidebar between its two tabs, the caption click's
    /// keyboard twin: same persistence, and the act takes the keys for
    /// the panel. A closed sidebar stays closed.
    fn switch_sidebar_tab(&mut self) {
        let Some(side) = self.sidebar.as_mut() else {
            return;
        };
        let tab = match side.tab() {
            sidebar::Tab::Files => sidebar::Tab::Outline,
            sidebar::Tab::Outline => sidebar::Tab::Files,
        };
        side.set_tab(tab);
        self.config.sidebar_tab = tab;
        config::save(&self.config);
        self.move_ownership(PaneAct::SwitchTab);
        self.request_redraw();
    }

    /// Moves the active tab's keyboard selection.
    fn sidebar_move(&mut self, delta: i32) {
        let Some(side) = self.sidebar.as_mut() else {
            return;
        };
        match side.tab() {
            sidebar::Tab::Files => side.move_selection(delta),
            sidebar::Tab::Outline => {
                let list_h = side.list_h();
                self.outline.move_selection(delta, sidebar::ROW_H, list_h);
            }
        }
        self.request_redraw();
    }

    /// Scrolls to a heading block: a placed anchor jumps now; a folded
    /// or unplaced one reveals and lands through the pending target.
    fn jump_to_heading(&mut self, block: usize) {
        // While the source is on screen the outline still describes the
        // page parked behind it, so the entry resolves against that
        // model and the caret lands on the heading's own line.
        if self.edit_park.is_some() {
            let offset = self
                .edit_park
                .as_ref()
                .and_then(|parked| entry_offset(&parked.document, block));
            if let Some(offset) = offset {
                self.caret = Some(Caret::at(offset));
                self.seat_editor_on(offset);
                self.wake_caret();
            }
            self.request_redraw();
            return;
        }
        match self.document.blocks.get(block).map(|b| &b.kind) {
            Some(BlockKind::Heading { anchor, .. }) => {
                let target = format!("#{anchor}");
                if let Some(y) = self.layout.as_ref().and_then(|l| l.anchor_y(&target)) {
                    self.scroll_to(y);
                    return;
                }
                if self.document.reveal(block) {
                    self.restart_layout();
                }
                self.pending_anchor = Some(target);
                self.request_redraw();
            }
            // A book outline entry lands on any block kind; the jump
            // goes by source offset through the pending path, which
            // covers placed and not-yet-placed alike. An unresolved
            // entry has no block and goes nowhere.
            Some(_) => {
                let offset = self.document.blocks[block].range.start;
                if self.document.reveal(block) {
                    self.restart_layout();
                }
                self.pending_offset = Some(offset);
                self.request_redraw();
            }
            None => {}
        }
    }

    /// Files the reading position the screen is leaving: a book's into
    /// the persisted store, a plain file's into the session's map. A
    /// file resting in edit mode is skipped, since it returns through
    /// its caret rather than through a reading position.
    fn remember_position(&mut self) {
        let Some(offset) = self.top_offset() else {
            return;
        };
        if let Some(key) = self.document.book_id.clone() {
            self.positions.remember(&key, offset);
            self.positions.save();
        } else if self.mode == edit::Mode::Read {
            if let Some(path) = self.path.clone() {
                self.read_marks.insert(path, offset);
            }
        }
    }

    /// The source offset of the block at the viewport top.
    fn top_offset(&self) -> Option<usize> {
        let lay = self.layout.as_ref()?;
        let mut offset = 0usize;
        for (index, block) in self.document.blocks.iter().enumerate() {
            match lay.approx_top(index, 0) {
                Some(top) if top <= self.scroll_y + 1.0 => offset = block.range.start,
                _ => break,
            }
        }
        Some(offset)
    }

    /// Runs a sidebar action and persists the tree's root when it moved.
    fn sidebar_action(&mut self, act: impl FnOnce(&mut Sidebar) -> Option<PathBuf>) {
        let Some(side) = self.sidebar.as_mut() else {
            return;
        };
        let before = side.root().to_path_buf();
        let opened = act(side);
        let after = self.sidebar.as_ref().map(|s| s.root().to_path_buf());
        if let Some(after) = after.filter(|after| *after != before) {
            self.remember_dir(&after);
        }
        if let Some(path) = opened {
            if self.guard_unsaved(confirm::Pending::Open(path.clone(), false)) {
                self.open_file(&path, false);
            }
        }
        self.request_redraw();
    }

    /// Persists whether the panel is open and how wide, after a gesture
    /// ends or the panel is toggled, never per frame.
    fn save_sidebar_state(&mut self) {
        self.config.sidebar_open = self.sidebar.is_some();
        if let Some(side) = self.sidebar.as_ref() {
            self.config.sidebar_width = side.width();
        }
        config::save(&self.config);
    }

    /// Document area x offset in physical pixels: the sidebar width
    /// while it is open. The sidebar itself thinks in logical units.
    fn inset(&self) -> f32 {
        self.sidebar
            .as_ref()
            .map_or(0.0, |side| side.width() * self.scale)
    }

    /// The cursor in the chrome's logical coordinates.
    fn ui_cursor(&self) -> (f32, f32) {
        (
            self.cursor.x as f32 / self.scale,
            self.cursor.y as f32 / self.scale,
        )
    }

    fn toggle_sidebar(&mut self) {
        let opening = self.sidebar.is_none();
        self.open_sidebar(opening);
        self.move_ownership(if opening {
            PaneAct::OpenSidebar
        } else {
            PaneAct::CloseSidebar
        });
        self.save_sidebar_state();
    }

    /// Whether the sidebar owns Up, Down, and Enter right now.
    fn sidebar_owns_keys(&self) -> bool {
        self.key_pane == KeyPane::Sidebar && self.sidebar.is_some()
    }

    /// Feeds an ownership-moving act through the transition table. A
    /// change re-renders the panel, whose live marks dim with ownership.
    fn move_ownership(&mut self, act: PaneAct) {
        let owner = owner_after(self.key_pane, act, self.sidebar.is_some());
        if owner != self.key_pane {
            self.key_pane = owner;
            self.sidebar_canvas = None;
            self.request_redraw();
        }
    }

    /// Opens or closes the panel, restoring the persisted width when it
    /// comes back.
    fn open_sidebar(&mut self, open: bool) {
        if open && self.sidebar.is_none() {
            let dir = config::browse_dir([self.document_dir(), self.remembered_dir()]);
            let mut side = Sidebar::new(&dir);
            side.set_tab(self.config.sidebar_tab);
            if let Some(path) = &self.path {
                side.set_current(path);
            }
            let window_w = self
                .gfx
                .as_ref()
                .map(|g| g.window.inner_size().width as f32 / self.scale)
                .unwrap_or(f32::MAX);
            side.set_width(self.config.sidebar_width, window_w);
            self.sidebar = Some(side);
        } else {
            self.sidebar = None;
            self.sidebar_canvas = None;
        }
        self.layout = None;
        self.band = None;
        self.request_redraw();
    }

    /// Shows a file in the document area. A failure renders the error
    /// message instead and the app stays up. Dialog opens re-root the
    /// sidebar; sidebar clicks keep the tree in place.
    fn open_file(&mut self, path: &Path, reroot: bool) {
        // An in-flight export steps against the document it was built
        // for; it cannot survive the swap. Its progress overlay dies
        // with it, since nothing would ever advance it again.
        if self.export.take().is_some() {
            self.overlay = None;
        }
        self.export_warning = None;
        self.remember_position();
        // The mark is stored against the outgoing path, which `leave_edit`
        // reads before the new one lands below. The parked page goes
        // first: rebuilding a page this call is about to replace would
        // cost a full parse of the outgoing file for nothing.
        if self.mode == edit::Mode::Edit {
            self.edit_park = None;
            self.leave_edit();
            // Switching files is not a decision to stop editing, so the
            // outgoing file is noted and comes back the way it was left.
            // Escape is that decision, and `leave_edit` clears the note,
            // which is why this stands after it.
            if let Some(path) = self.path.clone() {
                self.resume_edit.insert(path);
            }
        }
        self.ledger = None;
        self.undo = None;
        self.rehighlight_at = None;
        self.notice = None;
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let loaded = load::open(&path, Some(Instant::now() + load::OPEN_BUDGET));
        let opened = loaded.is_ok();
        let mut book_job = None;
        self.book_toc = Vec::new();
        match loaded {
            Ok(o) => {
                self.document = o.document;
                self.lossy = o.lossy;
                self.crlf = o.crlf;
                book_job = o.book;
                self.book_toc = o.toc;
                self.start_highlight(o.pending);
                if o.streamed && book_job.is_none() {
                    self.start_parse();
                } else {
                    // A book's worker starts through `start_book` below,
                    // once the fresh media cache exists for its sink.
                    self.parser.cancel();
                    self.parse_pending = false;
                }
            }
            Err(err) => {
                self.document = load::message(&err.to_string());
                // The message document is not the file's bytes; branding
                // it lossy keeps the editing door shut on it.
                self.lossy = true;
                self.crlf = Vec::new();
                self.start_highlight(Vec::new());
                self.parser.cancel();
                self.parse_pending = false;
            }
        }
        self.outline = if self.book_toc.is_empty() {
            OutlineTree::build(&self.document)
        } else {
            OutlineTree::from_toc(&self.book_toc, &self.document)
        };
        self.path = Some(path.to_path_buf());
        self.note_disk_state();
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        if opened {
            self.remember_dir(&dir);
        }
        self.media = MediaCache::new(dir.clone());
        self.media.set_waker(self.waker.clone());
        if let Some(job) = book_job {
            self.start_book(job);
        }
        self.scroll_y = 0.0;
        self.selection = None;
        self.sel_anchor = None;
        self.pending_recolor.clear();
        self.cfg.justify = justify_pref(&self.config, &self.document);
        self.layout = None;
        self.band = None;
        // Both hold targets in the old document's coordinates; left
        // alone they fire against the new one once its layout grows.
        // The bottom jump is the third, and it would seat a fresh
        // reader at the end of a file they just opened.
        self.pending_scroll = None;
        self.pending_anchor = None;
        self.pending_row = None;
        self.bottom_hold.clear();
        // A remembered file resumes where reading stopped, once placed:
        // a book from the persisted store, a plain file from the
        // session's map. A file about to resume editing returns through
        // its caret instead, so no offset target competes with the seat.
        self.pending_offset = self
            .document
            .book_id
            .as_deref()
            .and_then(|key| self.positions.lookup(key))
            .or_else(|| {
                if self.resume_edit.contains(&path) {
                    None
                } else {
                    self.read_marks.get(&path).copied()
                }
            });
        if let Some(side) = self.sidebar.as_mut() {
            if reroot && side.root() != dir {
                let tab = side.tab();
                *side = Sidebar::new(&dir);
                side.set_tab(tab);
            }
            side.set_current(&path);
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.set_title(&window_title(
                self.document.title.as_deref(),
                Some(&path),
                false,
            ));
        }
        // A file left mid-edit reopens in the editor at the caret it
        // was left on. The layout is fresh, so the seat waits for the
        // row to be placed. The swap kinds seat inside `enter_edit`;
        // the in-place kinds keep the scroll, which is right for a
        // same-file crossing and wrong here, the reopened file having
        // arrived with the scroll at zero, so they get the same seat.
        if self.resume_edit.remove(&path) && opened {
            self.enter_edit();
            if self.edit_park.is_none() {
                if let Some(offset) = self.caret.map(|c| c.offset) {
                    self.seat_editor_on(offset);
                }
            }
        }
        self.request_redraw();
    }

    /// Re-reads the open file from disk, keeping the scroll position, for
    /// documents being edited in parallel.
    /// F5 and Ctrl+R: a reload discards the buffer, so unsaved edits
    /// put the confirm in front of it.
    fn reload(&mut self) {
        if self.guard_unsaved(confirm::Pending::Reload) {
            self.reload_now();
        }
    }

    fn reload_now(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let scroll = self.scroll_y;
        self.open_file(&path, false);
        // The reload keeps its exact scroll; the revisit target would
        // land it on the block's top instead.
        self.pending_offset = None;
        self.scroll_y = scroll;
    }

    /// Native open dialog filtered to the recognized extensions.
    /// Starts an export with the saved settings, or the ones seeded from
    /// the appearance settings when nothing was ever exported. Does
    /// nothing without a document, since there would be nothing to write.
    fn export_now(&mut self) {
        if self.path.is_none() {
            return;
        }
        let settings = self.export_settings();
        self.start_export(settings);
    }

    /// The saved export settings, or the ones seeded from the appearance
    /// settings the first time anything is exported. Justification
    /// follows the viewer's preference for the open document's kind, so
    /// the page exports the way it reads.
    fn export_settings(&self) -> ExportSettings {
        let mut settings = self
            .config
            .export
            .clone()
            .unwrap_or_else(|| ExportSettings::seeded_from(&self.config));
        settings.justify = justify_pref(&self.config, &self.document);
        settings
    }

    /// Opens the export settings dialog on a working copy of them.
    fn toggle_export_settings(&mut self) {
        if self.path.is_none() {
            return;
        }
        if self.overlay.is_some() {
            self.overlay = None;
            return;
        }
        let mut themes: Vec<(String, Option<(Rgba, Rgba)>)> = Vec::new();
        for dir in theme_dirs() {
            for entry in theme::scan(&dir) {
                if themes.iter().any(|(name, _)| *name == entry.name) {
                    continue;
                }
                let preview = theme::preview(&entry.path);
                themes.push((entry.name, preview));
            }
        }
        // The same shelves the browser shows: light first, dark after.
        themes.sort_by(|a, b| (theme::dark_rank(&a.1), &a.0).cmp(&(theme::dark_rank(&b.1), &b.0)));
        let settings = self.export_settings();
        let dialog = ExportDialog::new(
            settings,
            self.fonts.families(),
            themes,
            self.document.justifiable(),
        );
        self.overlay = Some(Box::new(dialog));
        self.request_redraw();
    }

    /// Asks where the file goes, then starts the pass that writes it.
    fn start_export(&mut self, settings: ExportSettings) {
        // The export steps against the document it is built for; a
        // prefix would write a truncated file.
        self.finish_parse();
        let (theme, fell_back) = export::resolve_theme(&theme_dirs(), &settings.theme, &self.theme);
        let stem = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("document"));
        let start = config::browse_dir([
            self.document_dir(),
            self.sidebar.as_ref().map(|side| side.root().to_path_buf()),
            self.remembered_dir(),
        ]);
        let target = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.pdf"))
            .set_directory(start)
            .add_filter("PDF", &["pdf"])
            .save_file();
        let Some(target) = target else {
            return;
        };
        let pass = ExportPass::new(&settings, theme, target).with_toc(self.book_toc.clone());
        self.overlay = Some(Box::new(ExportProgress::new(pass.progress())));
        self.export_warning = fell_back.then(|| format!("theme {} is gone", settings.theme));
        self.export = Some(pass);
        self.request_redraw();
    }

    /// Gives the export its slice of the frame and reports where it got
    /// to. The document's own pass waits while one is running.
    fn export_slice(&mut self) {
        let Some(pass) = self.export.as_mut() else {
            return;
        };
        let deadline = Instant::now() + SLICE;
        let progress = pass.step(
            deadline,
            &self.document,
            &mut self.fonts,
            &mut self.media,
            self.highlighter.is_running(),
            Some(&self.pool),
        );
        let done = pass.is_done();
        self.overlay = Some(Box::new(ExportProgress::new(progress)));
        if done {
            let pass = self.export.take().expect("checked");
            // The reader chose the folder a moment ago in the dialog, so
            // the name is what the line is for. A failure keeps the whole
            // path, which is what makes it diagnosable.
            let name = pass
                .target()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| pass.target().display().to_string());
            let target = pass.target().display().to_string();
            // A finished export closes its overlay: the file on disk is
            // the confirmation. A failure keeps it up, since a message
            // that dismissed itself would be a message nobody read.
            match pass.finish(&self.document, &self.fonts) {
                Ok(pages) => {
                    self.overlay = match self.export_warning.take() {
                        Some(warning) => Some(Box::new(ExportProgress::settled(format!(
                            "{pages} pages to {name}, {warning}"
                        )))),
                        None => None,
                    };
                }
                Err(err) => {
                    self.overlay = Some(Box::new(ExportProgress::settled(format!(
                        "cannot write {target}: {err}"
                    ))));
                }
            }
        }
        self.request_redraw();
    }

    fn open_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Supported files", &load::recognized_extensions())
            .add_filter("All files", &["*"]);
        // The open document's folder anchors the dialog. The sidebar's root
        // carries a session with no file open, where the first candidate is
        // absent and the panel is the only record of where the reader is.
        let start = config::browse_dir([
            self.document_dir(),
            self.sidebar.as_ref().map(|side| side.root().to_path_buf()),
            self.remembered_dir(),
        ]);
        dialog = dialog.set_directory(start);
        if let Some(path) = dialog.pick_file() {
            if self.guard_unsaved(confirm::Pending::Open(path.clone(), true)) {
                self.open_file(&path, true);
            }
        }
    }

    /// F1: the help page in the document's place and back. The open
    /// document is stashed whole, edits, undo and layout included, and
    /// nothing touches the disk in either direction.
    fn toggle_help(&mut self) {
        if self.help_stash.is_some() {
            self.help_return();
            return;
        }
        // A parse still streaming would deliver into the help page's
        // document; the return reopens from disk instead, where the
        // position memory already knows the place.
        let reopen = self.parse_pending;
        if reopen {
            self.parser.cancel();
            self.parse_pending = false;
        }
        self.start_highlight(Vec::new());
        self.rehighlight_at = None;
        self.close_search();
        self.sel_anchor = None;
        self.remember_position();
        let mode = std::mem::replace(&mut self.mode, edit::Mode::Read);
        self.help_stash = Some(Box::new(Stash {
            path: self.path.take(),
            document: std::mem::take(&mut self.document),
            ledger: self.ledger.take(),
            undo: self.undo.take(),
            crlf: std::mem::take(&mut self.crlf),
            lossy: std::mem::replace(&mut self.lossy, false),
            mode,
            caret: self.caret.take(),
            scroll_y: self.scroll_y,
            layout: self.layout.take(),
            pass: self.pass.take(),
            layout_width: self.layout_width,
            media: std::mem::replace(&mut self.media, MediaCache::new(PathBuf::from("."))),
            book_toc: std::mem::take(&mut self.book_toc),
            disk_seen: self.disk_seen.take(),
            selection: self.selection.take(),
            zoom: self.cfg.zoom,
            theme: self.config.theme.clone(),
            reopen,
        }));
        self.document = markdown_help();
        self.outline = OutlineTree::build(&self.document);
        self.cfg.justify = justify_pref(&self.config, &self.document);
        self.scroll_y = 0.0;
        self.band = None;
        self.pending_band_for = None;
        self.pending_scroll = None;
        self.pending_anchor = None;
        self.pending_offset = None;
        self.pending_recolor.clear();
        self.bottom_hold.clear();
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.set_title("Oryx help");
        }
        self.request_redraw();
    }

    /// Back from the help page: the stashed document returns exactly
    /// as it was. A look changed while help showed (zoom, theme) drops
    /// the stashed layout, and the streaming pass re-measures.
    fn help_return(&mut self) {
        let Some(stash) = self.help_stash.take() else {
            return;
        };
        if stash.reopen {
            if let Some(path) = stash.path {
                self.open_file(&path, false);
            }
            return;
        }
        self.document = stash.document;
        self.path = stash.path;
        self.ledger = stash.ledger;
        self.undo = stash.undo;
        self.crlf = stash.crlf;
        self.lossy = stash.lossy;
        self.mode = stash.mode;
        self.caret = stash.caret;
        self.scroll_y = stash.scroll_y;
        self.layout_width = stash.layout_width;
        self.media = stash.media;
        self.book_toc = stash.book_toc;
        self.disk_seen = stash.disk_seen;
        let same_look = self.cfg.zoom == stash.zoom && self.config.theme == stash.theme;
        self.layout = stash.layout.filter(|_| same_look);
        self.pass = stash.pass.filter(|_| same_look);
        // The outline belongs to the rendered page. A return into a
        // parked markdown edit must read the parked page, not the
        // source view on screen, or the panel blanks to "No headings"
        // and holds blank for the rest of the edit.
        self.outline = if let Some(parked) = self.edit_park.as_deref() {
            OutlineTree::build(&parked.document)
        } else if self.book_toc.is_empty() {
            OutlineTree::build(&self.document)
        } else {
            OutlineTree::from_toc(&self.book_toc, &self.document)
        };
        self.cfg.justify = justify_pref(&self.config, &self.document);
        self.selection = stash.selection;
        self.sel_anchor = None;
        self.band = None;
        self.pending_band_for = None;
        self.close_search();
        self.start_highlight(load::pending(&self.document));
        if self.mode == edit::Mode::Edit {
            self.wake_caret();
        }
        if let (Some(side), Some(path)) = (self.sidebar.as_mut(), self.path.as_deref()) {
            side.set_current(path);
        }
        self.refresh_title();
        self.request_redraw();
    }

    /// Adopts the display's own factor, as reported at window creation
    /// or when the window changes monitors.
    fn rescale(&mut self, os_scale: f32) {
        self.os_scale = os_scale;
        self.refresh_scale();
    }

    /// Recomputes the effective scale from the display factor and the
    /// configured manual adjustment: the chrome painter picks it up on
    /// the next frame, the document folds it into the zoom product.
    fn refresh_scale(&mut self) {
        let manual = self
            .config
            .ui_scale
            .clamp(settings::UI_SCALE_MIN, settings::UI_SCALE_MAX);
        let scale = self.os_scale * manual;
        if (scale - self.scale).abs() < 0.001 {
            return;
        }
        self.scale = scale;
        self.touch = touch::Tracker::new(scale);
        self.band = None;
        self.sidebar_canvas = None;
        self.search_canvas = None;
        self.overlay_canvas = None;
        self.apply_zoom();
        self.request_redraw();
    }

    /// What the layout sees is always the reader's zoom step times the
    /// display scale, so Ctrl+0 lands on the display's density, not
    /// under it.
    fn apply_zoom(&mut self) {
        self.set_zoom(self.user_zoom * self.scale);
    }

    /// Session zoom around the current scroll position; never persisted.
    fn set_zoom(&mut self, zoom: f32) {
        if (zoom - self.cfg.zoom).abs() < f32::EPSILON {
            return;
        }
        self.scroll_y *= zoom / self.cfg.zoom;
        self.cfg.zoom = zoom;
        self.layout = None;
        self.band = None;
        self.request_redraw();
    }

    /// Flips prose justification and relayouts, a no-op on code and
    /// text files. Books and markdown each remember their own choice,
    /// and both persist across sessions.
    fn toggle_justify(&mut self) {
        if !self.document.justifiable() {
            return;
        }
        let pref = if self.document.book_id.is_some() {
            &mut self.config.justify
        } else {
            &mut self.config.justify_markdown
        };
        *pref = !*pref;
        let value = *pref;
        config::save(&self.config);
        self.cfg.justify = value;
        self.layout = None;
        self.band = None;
        self.request_redraw();
    }

    /// Applies what an overlay asked for after handling an event.
    fn overlay_result(&mut self, result: OverlayResult) {
        match result {
            OverlayResult::Open => {}
            // The apply moves the revert point forward, so the close
            // that follows restores nothing.
            OverlayResult::ApplyAndClose(action) => {
                self.overlay_result(OverlayResult::Apply(action));
                self.overlay_result(OverlayResult::Close);
            }
            OverlayResult::Close => {
                self.overlay = None;
                if self.view_dirty {
                    config::save(&self.config);
                    self.view_dirty = false;
                }
                // Closing the progress overlay cancels the export, and
                // nothing has reached the disk yet.
                self.export = None;
                self.export_warning = None;
                // An editor closed without saving: back to the last
                // applied state.
                if let Some(previous) = self.pre_edit.take() {
                    self.set_live_theme(previous);
                }
                // A browser closed on an unconfirmed keyboard preview:
                // back to the confirmed theme.
                if let Some(previous) = self.pre_browse.take() {
                    if previous != self.theme {
                        self.set_live_theme(previous);
                    }
                }
            }
            OverlayResult::Apply(Action::SetTheme(path)) => {
                self.apply_theme(&path);
                // A save from the editor or a confirm from the browser
                // moves the revert point forward.
                if self.pre_edit.is_some() {
                    self.pre_edit = Some(self.theme.clone());
                }
                if self.pre_browse.is_some() {
                    self.pre_browse = Some(self.theme.clone());
                }
            }
            OverlayResult::Apply(Action::RenamedTheme { from, to }) => {
                if self.config.theme == from {
                    self.config.theme = to;
                    config::save(&self.config);
                }
            }
            OverlayResult::Apply(Action::EditTheme(path)) => {
                if let Some(editor) = ThemeEditor::new(&path) {
                    // Opened from the browser, the revert point is the
                    // confirmed theme, not a preview the arrows left live.
                    self.pre_edit = Some(match self.pre_browse.take() {
                        Some(confirmed) => confirmed,
                        None => self.theme.clone(),
                    });
                    let preview = editor.current();
                    self.overlay = Some(Box::new(editor));
                    self.set_live_theme(preview);
                }
            }
            OverlayResult::Apply(Action::Export(settings)) => {
                // The dialog is the reader's statement of preference, so
                // it persists whether or not the export then completes.
                self.config.export = Some(*settings.clone());
                config::save(&self.config);
                self.overlay = None;
                self.start_export(*settings);
            }
            OverlayResult::Apply(Action::PreviewTheme(theme)) => {
                self.set_live_theme(*theme);
            }
            OverlayResult::Apply(Action::SetView {
                body_family,
                code_family,
                body_size,
                code_size,
                ui_scale,
            }) => {
                self.cfg.body_family = body_family.clone();
                self.cfg.code_family = code_family.clone();
                self.cfg.body_size = body_size;
                self.cfg.code_size = code_size;
                self.config.body_family = body_family;
                self.config.code_family = code_family;
                self.config.body_size = body_size;
                self.config.code_size = code_size;
                self.config.ui_scale = ui_scale;
                self.refresh_scale();
                // A held arrow key repeats this apply dozens of times a
                // second; the disk write waits for the dialog to close.
                self.view_dirty = true;
                self.layout = None;
                self.band = None;
            }
        }
        self.request_redraw();
    }

    /// Switches to a theme file, persists the choice, and restyles.
    fn apply_theme(&mut self, path: &std::path::Path) {
        let Some(theme) = theme::load_file(path) else {
            return;
        };
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            self.config.theme = name.to_string();
            config::save(&self.config);
        }
        self.set_live_theme(theme);
    }

    /// Restyles with an in-memory theme, persisting nothing.
    fn set_live_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.layout = None;
        self.band = None;
    }

    /// Selects the whole document, placing the rest of it first so the
    /// selection covers what a copy will read.
    fn select_all(&mut self) {
        // The whole model first: a selection over a prefix would copy a
        // truncated document. The layout is not needed; the selection
        // anchors on the model.
        self.finish_parse();
        let Some(sel) = selection::all(&self.document) else {
            return;
        };
        if self.selection != Some(sel) {
            self.selection = Some(sel);
            self.band = None;
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        }
    }

    /// Puts the selection on the clipboard, as markdown or plain text.
    fn copy_selection(&mut self, as_markdown: bool) {
        let Some(sel) = self.selection else {
            return;
        };
        let text = if as_markdown {
            selection::markdown(&sel, &self.document)
        } else {
            selection::plain_text(&sel, &self.document)
        };
        if text.is_empty() {
            return;
        }
        self.set_clipboard(text);
    }

    fn set_clipboard(&mut self, text: String) {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new()
                .map_err(|err| eprintln!("oryx: no clipboard: {err}"))
                .ok();
        }
        if let Some(clipboard) = self.clipboard.as_mut() {
            if let Err(err) = clipboard.set_text(text) {
                eprintln!("oryx: clipboard copy failed: {err}");
            }
        }
    }

    /// Restarts the streaming layout in place: the scroll position holds
    /// and the pass rebuilds behind it, the zoom contract. Fold toggles
    /// and reveals come through here.
    fn restart_layout(&mut self) {
        self.layout = None;
        self.band = None;
        self.request_redraw();
    }

    /// The block a `#anchor` target names, wherever it sits in the model,
    /// placed or not.
    fn anchor_block(&self, target: &str) -> Option<usize> {
        let slug = target.strip_prefix('#')?;
        self.document
            .blocks
            .iter()
            .position(|b| matches!(&b.kind, BlockKind::Heading { anchor, .. } if anchor == slug))
    }

    /// Follow the link under the cursor: anchors scroll, http links open
    /// in the system browser. A click on a summary row with no link under
    /// it toggles the row's details group.
    fn link_press(&mut self) {
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        if let Some(block) = lay.checkbox_at(x, y) {
            self.click_checkbox(block);
            return;
        }
        let Some(target) = lay.link_at(&self.document, x, y).map(str::to_owned) else {
            if let Some(group) = lay.summary_at(&self.document, x, y) {
                self.document.toggle_details(group);
                self.restart_layout();
            }
            return;
        };
        if let Some(anchor) = lay.anchor_y(&target) {
            self.scroll_to(anchor);
        } else if let Some(rest) = target.strip_prefix("book:") {
            // An internal book link: the whole model first, so a forward
            // reference resolves, then the anchor map answers.
            self.finish_parse();
            let (path, fragment) = match rest.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (rest, None),
            };
            if let Some(offset) = epub::resolve_target(&self.document, path, fragment) {
                if let Some(block) = self.document.block_at_offset(offset) {
                    if self.document.reveal(block) {
                        self.restart_layout();
                    }
                }
                self.pending_offset = Some(offset);
                self.request_redraw();
            }
        } else if target.starts_with("http://") || target.starts_with("https://") {
            if let Err(err) = open::that_detached(&target) {
                eprintln!("oryx: cannot open {target}: {err}");
            }
        } else if let Some((path, fragment)) =
            file_link_target(&target, self.path.as_ref().and_then(|p| p.parent()))
        {
            // A relative link to a sibling file: displayable files open
            // in place, anything else goes to the system handler. A
            // fragment lands once the new document's layout reaches it.
            if load::is_text_file(&path) {
                if self.guard_unsaved(confirm::Pending::Open(path.clone(), false)) {
                    self.open_file(&path, false);
                    if let Some(fragment) = fragment {
                        self.pending_anchor = Some(format!("#{fragment}"));
                    }
                }
            } else if let Err(err) = open::that_detached(&path) {
                eprintln!("oryx: cannot open {}: {err}", path.display());
            }
        } else if let Some(block) = self.anchor_block(&target) {
            // The heading exists but is not placed: folded away, beyond
            // the pass, or both. Reveal opens the chain and the pending
            // target lands once the pass places it.
            if self.document.reveal(block) {
                self.restart_layout();
            }
            self.pending_anchor = Some(target);
            self.request_redraw();
        } else if self.layout_pending() {
            // Not in the model yet either; the parse may still deliver
            // it, so the jump waits instead of doing nothing.
            self.pending_anchor = Some(target);
        }
    }

    /// Track whether a link sits under the cursor and swap the pointer icon
    /// on transitions.
    fn update_hover(&mut self) {
        let on_edge = self
            .sidebar
            .as_ref()
            .is_some_and(|side| sidebar::on_edge(side.width(), self.cursor.x as f32 / self.scale));
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let (ux, uy) = self.ui_cursor();
        let on_toggle = self.search.is_some()
            && self
                .logical_width()
                .is_some_and(|width| search::toggle_hit(width, ux, uy));
        let hovering = on_toggle
            || (!on_edge
                && self.layout.as_ref().is_some_and(|l| {
                    l.link_at(&self.document, x, y).is_some()
                        || l.summary_at(&self.document, x, y).is_some()
                        || (self.mode == edit::Mode::Read && l.checkbox_at(x, y).is_some())
                }));
        if hovering != self.hover_link || on_edge != self.hover_edge {
            self.hover_link = hovering;
            self.hover_edge = on_edge;
            if let Some(gfx) = self.gfx.as_ref() {
                let icon = if on_edge {
                    CursorIcon::ColResize
                } else if hovering {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                };
                gfx.window.set_cursor(icon);
            }
        }
    }

    fn scrollbar_press(&mut self) {
        let Some((thumb_y, thumb_h)) = self.thumb() else {
            return;
        };
        let (x, y) = (self.cursor.x as f32, self.cursor.y as f32);
        let width = self
            .gfx
            .as_ref()
            .map(|g| g.window.inner_size().width as f32)
            .unwrap_or(0.0);
        if x < width - scrollbar::STRIP_WIDTH * self.scale {
            return;
        }
        if y >= thumb_y && y <= thumb_y + thumb_h {
            self.drag = Some(Drag::Scrollbar(y - thumb_y));
        } else {
            // Track click: jump so the thumb centers on the cursor.
            self.drag = Some(Drag::Scrollbar(thumb_h / 2.0));
            self.drag_to(y);
        }
    }

    /// Starts a pass when the layout is missing or the content width
    /// changed, and reports whether one began. Everything that indexes the
    /// layout is dropped here, as a full relayout always did.
    fn start_pass(&mut self, avail: f32) -> bool {
        if self.layout.is_some() && (self.layout_width == avail || self.settle_at.is_some()) {
            return false;
        }
        if self.scroll_y > 0.0 {
            self.pending_scroll = Some(self.scroll_y);
        }
        let (out, mut pass) = layout_begin(&self.document, &self.cfg, avail);
        pass.attach_pool(Arc::clone(&self.pool));
        // A fresh pass shapes with the model's current colors; owed
        // recolors have nothing left to patch.
        self.pending_recolor.clear();
        self.layout = Some(out);
        self.pass = Some(pass);
        self.pass_spent = Duration::ZERO;
        self.layout_width = avail;
        self.band = None;
        // The new width obsoletes every resized image buffer.
        self.media.clear_scaled();
        self.pending_band_for = None;
        // The selection anchors on the model and survives the restart;
        // search matches do too, but their geometry is stale.
        if let Some(state) = self.search.as_mut() {
            state.stale = true;
        }
        true
    }

    /// Whether a pass exists with blocks still to place.
    fn pass_active(&self) -> bool {
        self.pass.as_ref().is_some_and(|p| !p.is_complete())
    }

    /// Whether the layout still owes places: the pass has blocks left, or
    /// the parse worker owes a document that will grow it.
    fn layout_pending(&self) -> bool {
        self.pass_active() || self.parse_pending
    }

    /// Advances the pass by one slice. A pointer gesture holds it off so
    /// selection and scrollbar dragging stay smooth; the queue resumes on
    /// release.
    fn slice(&mut self, budget: Duration) {
        if !self.pass_active()
            || self.drag.is_some()
            || self.sel_anchor.is_some()
            || self.overlay_mouse
        {
            return;
        }
        let before = self.doc_height();
        let started = Instant::now();
        let viewport_h = self.viewport_h();
        let done = {
            let lay = self.layout.as_mut().expect("a pass has a layout");
            let pass = self.pass.as_mut().expect("a pass is running");
            pass.retain_around(self.scroll_y, viewport_h);
            layout_more(
                &self.document,
                &self.theme,
                &mut self.fonts,
                &mut self.media,
                &self.cfg,
                lay,
                pass,
                Some(started + budget),
            )
        };
        self.pass_spent += started.elapsed();
        if done {
            self.last_pass = self.pass_spent;
            // Matches were found against the prefix; the whole document
            // is searchable now.
            if let Some(state) = self.search.as_mut() {
                state.stale = true;
            }
        } else {
            self.request_redraw();
        }
        self.grow_band(before);
        // A jump to the bottom taken while the document was still
        // being placed is spent here, on the whole document.
        if self
            .bottom_hold
            .settle(self.scroll_y, !self.layout_pending())
        {
            self.scroll_to(self.doc_height());
        }
    }

    /// Content appended below the painted band leaves its pixels valid,
    /// and only its document height has to follow. Appending into the band
    /// drops it.
    fn grow_band(&mut self, before: f32) {
        let after = self.doc_height();
        if after == before {
            return;
        }
        match self.band.as_mut() {
            Some(band) if before < band.y_top + band.height as f32 => {
                self.band = None;
                self.pending_band_for = None;
            }
            Some(band) => band.doc_height = after,
            None => {}
        }
    }

    /// Applies a scroll position or an anchor asked for before the pass
    /// had placed it.
    fn resolve_pending(&mut self) {
        if self.pending_scroll.is_none()
            && self.pending_anchor.is_none()
            && self.pending_offset.is_none()
            && self.pending_row.is_none()
        {
            return;
        }
        let height = self.doc_height();
        let vh = self.viewport_h();
        if let Some(target) = self.pending_scroll {
            if scroll::reached(target, height, vh) {
                self.pending_scroll = None;
                self.scroll_y = target;
            }
        }
        // A row asked for before the layout reached it: the same
        // pending discipline as the offset target below, resolved
        // against rows rather than blocks, since the editor is indexed
        // by lines and a source view is one block.
        if let Some(offset) = self.pending_row {
            match self.editor_row_y(offset) {
                Some(y) if scroll::reached(y, height, vh) || !self.layout_pending() => {
                    self.pending_row = None;
                    self.scroll_to(y);
                }
                None if self.layout.is_some() && !self.layout_pending() => self.pending_row = None,
                _ => {}
            }
        }
        if let Some(offset) = self.pending_offset {
            // Held while the offset lies past the delivered source; the
            // worker's delivery brings the rest.
            let covered = offset < self.document.source.len() || !self.parse_pending;
            if covered {
                let placed = self
                    .document
                    .block_at_offset(offset)
                    .and_then(|b| self.layout.as_ref().and_then(|l| l.approx_top(b, 0)));
                match placed {
                    Some(y) if scroll::reached(y, height, vh) || !self.layout_pending() => {
                        self.pending_offset = None;
                        self.scroll_to(y);
                    }
                    None if !self.layout_pending() && !self.parse_pending => {
                        self.pending_offset = None;
                    }
                    _ => {}
                }
            }
        }
        let Some(name) = self.pending_anchor.clone() else {
            return;
        };
        match self.layout.as_ref().and_then(|l| l.anchor_y(&name)) {
            Some(y) if scroll::reached(y, height, vh) || !self.layout_pending() => {
                self.pending_anchor = None;
                self.scroll_to(y);
            }
            // The pass ended without ever placing that heading.
            None if !self.layout_pending() => self.pending_anchor = None,
            _ => {}
        }
    }

    fn redraw(&mut self) {
        // Frames mean interaction; idle draws nothing, so the disk
        // check rides them without ever waking the loop itself.
        self.check_disk();
        let inset = self.inset() as u32;
        let Some(size) = self.gfx.as_ref().map(|g| g.window.inner_size()) else {
            return;
        };
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let avail_px = size.width.saturating_sub(inset).max(1);
        let avail = avail_px as f32;
        let budget = if self.start_pass(avail) {
            OPEN_SLICE
        } else {
            SLICE
        };
        // An export owns the slice while it runs; the document's own pass
        // picks up where it left off once the file is written.
        if self.export.is_some() {
            self.export_slice();
        } else {
            self.slice(budget);
        }
        self.resolve_pending();
        // An open search counts as interactive: every keystroke edits the
        // highlights, so frames paint direct and the expensive band
        // rebuild waits until the bar closes.
        let interactive = self.drag.is_some()
            || self.sel_anchor.is_some()
            || self.overlay_mouse
            || self.search.is_some();
        // The materialized window slides to the scroll position before
        // anything reads geometry: the full band when settled, only the
        // viewport under a live gesture.
        if let Some(lay) = self.layout.as_mut() {
            self.scroll_y = scroll::clamp(self.scroll_y, lay.height, size.height as f32);
            window_to(
                &self.document,
                &self.theme,
                &mut self.fonts,
                &mut self.media,
                &self.cfg,
                lay,
                Some(&self.pool),
                self.scroll_y,
                size.height as f32,
                !interactive,
            );
            // Anything the slide, the pass or a recolor left unindexed
            // joins the y index before this frame queries it.
            lay.index_more();
        }
        let before_search = self.scroll_y;
        self.sync_search();
        self.settle_search_anchor();
        // A typing edit landed under a fresh layout: snap the view to
        // the caret once its line is placed, inside the same re-window
        // the search landing uses.
        if self.caret_snap {
            if let (Some(c), Some(lay)) = (self.caret, self.layout.as_ref()) {
                if let Some(cb) = c.geometry(lay, &self.document, &mut self.fonts) {
                    let target = caret::snap(self.scroll_y, size.height as f32, cb);
                    self.scroll_y = scroll::clamp(target, lay.height, size.height as f32);
                    self.caret_snap = false;
                }
            }
        }
        // A search step that scrolled re-slides the window so this
        // frame paints the landing, not a cold viewport; the match
        // rects recompute over what just materialized.
        if self.scroll_y != before_search {
            if let Some(lay) = self.layout.as_mut() {
                window_to(
                    &self.document,
                    &self.theme,
                    &mut self.fonts,
                    &mut self.media,
                    &self.cfg,
                    lay,
                    Some(&self.pool),
                    self.scroll_y,
                    size.height as f32,
                    !interactive,
                );
                lay.index_more();
            }
            self.refresh_search_rects();
        }
        let drifted = self.search.as_ref().is_some_and(|state| {
            !state.stale && (self.scroll_y - state.rects_scroll).abs() > self.viewport_h()
        });
        if drifted {
            self.refresh_search_rects();
        }
        let lay = self.layout.as_ref().expect("layout exists");
        let mut highlight: Vec<DecoRect> = match &self.selection {
            Some(sel) => selection::rects(sel, lay, &self.document, &mut self.fonts)
                .into_iter()
                .map(|(x, y, w, h)| DecoRect::fill(x, y, w, h, self.theme.ui.selection_bg))
                .collect(),
            None => Vec::new(),
        };
        if let Some(state) = self.search.as_ref() {
            for &(index, (x, y, w, h)) in &state.rects {
                let color = if index == state.current {
                    self.theme.ui.search_current_bg
                } else {
                    self.theme.ui.search_match_bg
                };
                highlight.push(DecoRect::fill(x, y, w, h, color));
            }
        }

        let band_usable = self.band.as_ref().is_some_and(|b| {
            b.width == avail_px
                && b.height == size.height * 5
                && !b.needs_repaint(self.scroll_y, size.height as f32)
        });
        let size_tag = (size.width, size.height);
        let mut direct: Option<Vec<u32>> = None;
        if !band_usable {
            let build_now = !interactive && self.pending_band_for == Some(size_tag);
            if build_now {
                self.band = Some(BandCache::repaint(
                    lay,
                    &self.document,
                    &self.theme,
                    &mut self.fonts,
                    &mut self.media,
                    &highlight,
                    self.scroll_y,
                    avail_px,
                    size.height,
                ));
                self.pending_band_for = None;
            } else {
                direct = Some(paint::band(
                    lay,
                    &self.document,
                    &self.theme,
                    &mut self.fonts,
                    &mut self.media,
                    &highlight,
                    self.scroll_y,
                    avail_px,
                    size.height,
                ));
                self.pending_band_for = (!interactive).then_some(size_tag);
            }
        }
        let view: &[u32] = match &direct {
            Some(pixels) => pixels,
            None => self
                .band
                .as_ref()
                .expect("band exists")
                .view(self.scroll_y, size.height),
        };

        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        gfx.surface
            .resize(width, height)
            .expect("surface resize failed");
        let mut buffer = gfx.surface.buffer_mut().expect("buffer borrow failed");
        if inset == 0 {
            let len = view.len().min(buffer.len());
            buffer[..len].copy_from_slice(&view[..len]);
        } else {
            // The band is narrower than the window: copy it row by row at
            // the sidebar offset.
            let bw = avail_px as usize;
            let stride = size.width as usize;
            for row in 0..size.height as usize {
                let src = row * bw;
                let dst = row * stride + inset as usize;
                if src + bw > view.len() || dst + bw > buffer.len() {
                    break;
                }
                buffer[dst..dst + bw].copy_from_slice(&view[src..src + bw]);
            }
        }
        if let Some(thumb) = scrollbar::thumb(
            lay.height,
            size.height as f32,
            self.scroll_y,
            size.height as f32,
            self.scale,
        ) {
            let color = if matches!(self.drag, Some(Drag::Scrollbar(_))) {
                self.theme.ui.scrollbar_hover
            } else {
                self.theme.ui.scrollbar
            };
            scrollbar::draw(
                &mut buffer,
                size.width,
                size.height,
                thumb,
                color,
                self.scale,
            );
        }
        if self.mode == edit::Mode::Edit && self.blink_visible {
            if let Some(c) = self.caret {
                if let Some(b) = c.geometry(lay, &self.document, &mut self.fonts) {
                    draw_caret(
                        &mut buffer,
                        size.width,
                        size.height,
                        inset,
                        self.scroll_y,
                        self.scale,
                        b,
                        self.theme.text.body,
                    );
                }
            }
        }
        // While editing the caret owns every bare key, so the panel's
        // live marks stay dimmed whatever the recorded owner; the
        // ownership machine is a read-mode concept.
        let owns_keys = self.key_pane == KeyPane::Sidebar && self.mode == edit::Mode::Read;
        if let Some(side) = self.sidebar.as_mut() {
            // The outline describes the rendered page, so while the
            // source is on screen it holds the mark it had rather than
            // tracking a layout of a different model.
            let current = if self.mode == edit::Mode::Read {
                outline_current(&mut self.outline, side, lay, self.scroll_y)
            } else {
                self.outline.last_current()
            };
            let fits = self
                .sidebar_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.sidebar_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let Some((canvas, stale)) = self.sidebar_canvas.as_mut() {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take(), self.scale);
                side.draw(
                    &mut painter,
                    &self.theme,
                    &mut self.outline,
                    current,
                    owns_keys,
                );
                painter.composite(&mut buffer, size.width);
                *stale = painter.dirty();
            }
        }
        if let Some(state) = self.search.as_ref() {
            let fits = self
                .search_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.search_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let Some((canvas, stale)) = self.search_canvas.as_mut() {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take(), self.scale);
                search::draw_bar(
                    &mut painter,
                    &self.theme,
                    state,
                    size.width as f32 / self.scale,
                );
                painter.composite(&mut buffer, size.width);
                *stale = painter.dirty();
            }
        }
        if let Some(alpha) = self
            .notice
            .as_ref()
            .and_then(|notice| notice.alpha(Instant::now()))
        {
            let fits = self
                .notice_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.notice_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let (Some((canvas, stale)), Some(n)) =
                (self.notice_canvas.as_mut(), self.notice.as_ref())
            {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take(), self.scale);
                notice::draw(
                    &mut painter,
                    &self.theme,
                    n.text(),
                    alpha,
                    size.width as f32 / self.scale,
                    size.height as f32 / self.scale,
                );
                painter.composite(&mut buffer, size.width);
                *stale = painter.dirty();
            }
        }
        if self.confirm.is_some() {
            let fits = self
                .notice_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.notice_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let Some((canvas, stale)) = self.notice_canvas.as_mut() {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take(), self.scale);
                confirm::draw(
                    &mut painter,
                    &self.theme,
                    size.width as f32 / self.scale,
                    size.height as f32 / self.scale,
                );
                painter.composite(&mut buffer, size.width);
                *stale = painter.dirty();
            }
        }
        if let Some(overlay) = self.overlay.as_mut() {
            let fits = self
                .overlay_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.overlay_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let Some((canvas, stale)) = self.overlay_canvas.as_mut() {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take(), self.scale);
                overlay.draw(&mut painter, &self.theme);
                painter.composite(&mut buffer, size.width);
                *stale = painter.dirty();
            }
        }
        buffer.present().expect("present failed");
        // A direct frame leaves the band stale: build it in a follow-up
        // frame so the visible one stayed cheap. Skipped during drags.
        if direct.is_some() && self.pending_band_for.is_some() {
            gfx.window.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    /// A relayout deferred by a live resize waits for the size to hold
    /// still, and the timer is the only thing that wakes an idle loop.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.pending_recolor.is_empty() {
            let due = self.last_recolor + RECOLOR_WAVE;
            if Instant::now() >= due {
                self.flush_recolor();
                event_loop.set_control_flow(ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(due));
            }
        }
        if let Some(at) = self.settle_at {
            if Instant::now() < at {
                event_loop.set_control_flow(ControlFlow::WaitUntil(at));
            } else {
                self.settle_at = None;
                self.layout = None;
                self.pass = None;
                event_loop.set_control_flow(ControlFlow::Wait);
                self.request_redraw();
            }
        }
        if let Some(at) = self.rehighlight_at {
            if Instant::now() >= at {
                self.rehighlight_at = None;
                self.rehighlight_edited();
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(at));
            }
        }
        if self.mode == edit::Mode::Edit && self.caret.is_some() {
            let now = Instant::now();
            if now >= self.blink_flip {
                self.blink_visible = !self.blink_visible;
                self.blink_flip = now + CARET_BLINK;
                self.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.blink_flip));
        }
        if let Some(notice) = self.notice.as_ref() {
            let now = Instant::now();
            match notice.alpha(now) {
                None => {
                    self.notice = None;
                    event_loop.set_control_flow(ControlFlow::Wait);
                    self.request_redraw();
                }
                Some(alpha) => {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(notice.wake(now)));
                    if alpha < 1.0 {
                        self.request_redraw();
                    }
                }
            }
        }
        self.maybe_speculate();
        // Last, so its near tick wins the wake; an early wake costs the
        // timers above nothing.
        self.step_fling(event_loop);
    }

    /// A background fetch, parse delivery or highlight chunk landed: fold
    /// it in. Fetches relayout; highlights only recolor. The parse lands
    /// first so a stale highlight generation dies before it recolors.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        match self.media.drain_remote() {
            images::Folded::Relayout => {
                self.layout = None;
                self.band = None;
                self.request_redraw();
            }
            images::Folded::Repaint => {
                self.band = None;
                self.request_redraw();
            }
            images::Folded::Nothing => {}
        }
        self.fold_parse();
        self.fold_highlights();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // X11 and Windows take the icon here; Wayland resolves it from the
        // desktop entry matching the app_id instead.
        let icon = winit::window::Icon::from_rgba(ICON_64.to_vec(), 64, 64).ok();
        let mut attributes = Window::default_attributes()
            .with_title(window_title(
                self.document.title.as_deref(),
                self.path.as_deref(),
                false,
            ))
            .with_window_icon(icon);
        // Reopen as last closed: size, position when it still lands on a
        // monitor, and the maximized state on top so unmaximizing falls
        // back to the floating geometry. Wayland ignores the position.
        if let Some(win) = self.config.window.filter(|w| w.width > 0 && w.height > 0) {
            attributes = attributes.with_inner_size(PhysicalSize::new(win.width, win.height));
            let monitors: Vec<(i32, i32, u32, u32)> = event_loop
                .available_monitors()
                .map(|m| {
                    (
                        m.position().x,
                        m.position().y,
                        m.size().width,
                        m.size().height,
                    )
                })
                .collect();
            if let Some((x, y)) = win.position_on(&monitors) {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
            attributes = attributes.with_maximized(win.maximized);
        }
        // Wayland compositors resolve the window icon from a desktop entry
        // matching this app_id; the same call sets WM_CLASS on X11.
        #[cfg(target_os = "linux")]
        let attributes = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attributes.with_name("oryx", "oryx")
        };
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("window creation failed"),
        );
        let context = softbuffer::Context::new(window.clone()).expect("display context failed");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("surface creation failed");
        let scale = window.scale_factor() as f32;
        self.gfx = Some(Gfx { window, surface });
        self.rescale(scale);
        // The panel comes back the way it was left, at its saved width.
        if self.config.sidebar_open {
            self.open_sidebar(true);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if self.guard_unsaved(confirm::Pending::Quit) {
                    self.remember_position();
                    event_loop.exit();
                }
            }
            WindowEvent::Focused(true) => {
                // Coming back to the window is when an external change
                // is most likely to have landed.
                self.disk_check_at = Instant::now();
                self.check_disk();
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                self.fling = None;
                // The confirm modal owns the keyboard outright until a
                // decision lands.
                if self.confirm.is_some() {
                    self.confirm_key(&logical_key, event_loop);
                    return;
                }
                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();
                // The overlay toggles stay global so their chord closes the
                // overlay it opened; everything else feeds an open overlay.
                match keymap::command(&logical_key, ctrl, shift) {
                    Some(
                        cmd @ (Command::ThemeBrowser
                        | Command::Settings
                        | Command::Sidebar
                        | Command::OpenFile),
                    ) => {
                        self.run_command(cmd, event_loop);
                    }
                    _ if self.overlay.is_some() => {
                        let overlay = self.overlay.as_mut().expect("overlay open");
                        let result = overlay.key(&logical_key, ctrl, shift);
                        self.overlay_result(result);
                    }
                    _ if self.search_key(&logical_key, ctrl, shift) => {}
                    _ if self.mode == edit::Mode::Edit
                        && self.edit_key(&logical_key, ctrl, shift) => {}
                    Some(Command::LineUp) if self.sidebar_owns_keys() => {
                        self.sidebar_move(-1);
                    }
                    Some(Command::LineDown) if self.sidebar_owns_keys() => {
                        self.sidebar_move(1);
                    }
                    None if self.sidebar_owns_keys()
                        && matches!(logical_key, Key::Named(NamedKey::Enter)) =>
                    {
                        let outline_tab = self
                            .sidebar
                            .as_ref()
                            .is_some_and(|s| s.tab() == sidebar::Tab::Outline);
                        if outline_tab {
                            if let Some(block) = self.outline.selected_block() {
                                self.jump_to_heading(block);
                            }
                        } else {
                            self.sidebar_action(|side| side.enter());
                        }
                        // Activation keeps the keys: the table says Enter
                        // holds, so arrow-and-enter stepping stays a flow.
                        self.move_ownership(PaneAct::Enter);
                    }
                    Some(cmd) => self.run_command(cmd, event_loop),
                    None => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.fling = None;
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => -lines,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.line_step(),
                };
                let over_sidebar = (self.cursor.x as f32) < self.inset();
                if let Some(overlay) = self.overlay.as_mut() {
                    let result = overlay.scroll(lines);
                    self.overlay_result(result);
                } else if let Some(side) = self.sidebar.as_mut().filter(|_| over_sidebar) {
                    side.wheel(lines * 3.0, &mut self.outline);
                    self.move_ownership(PaneAct::WheelSidebar);
                    self.request_redraw();
                } else {
                    self.scroll_by(lines * 3.0 * self.line_step());
                    self.move_ownership(PaneAct::WheelDocument);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_muted() {
                    return;
                }
                self.cursor = position;
                if self.overlay.is_some() {
                    if self.overlay_mouse {
                        let (x, y) = (
                            position.x as f32 / self.scale,
                            position.y as f32 / self.scale,
                        );
                        let result = self.overlay.as_mut().expect("overlay open").drag(x, y);
                        self.overlay_result(result);
                    }
                } else if let Some(Drag::SidebarEdge(grab)) = self.drag {
                    self.resize_sidebar(position.x as f32 / self.scale - grab);
                } else if self.drag.is_some() {
                    self.drag_to(position.y as f32);
                } else if self.sel_anchor.is_some() {
                    self.extend_selection();
                } else {
                    self.update_hover();
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if self.mouse_muted() {
                    return;
                }
                match state {
                    ElementState::Pressed => {
                        self.fling = None;
                        self.left_press();
                    }
                    ElementState::Released => self.left_release(),
                }
            }
            WindowEvent::Resized(size) => {
                // A drag delivers a width per frame. When a full pass
                // outlasts a slice, restarting it on each one would strand
                // the reader at the top for the whole drag, so the current
                // layout keeps painting until the size holds still.
                if self.layout.is_some() && scroll::defer_relayout(self.last_pass, SLICE) {
                    self.settle_at = Some(Instant::now() + scroll::SETTLE);
                }
                if let Some(gfx) = self.gfx.as_ref() {
                    // Track only the floating geometry; a maximized or
                    // restored-maximized window must not overwrite it.
                    if !gfx.window.is_maximized() {
                        let win = self.config.window.get_or_insert_with(WindowState::default);
                        win.width = size.width;
                        win.height = size.height;
                    }
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::Moved(position) => {
                if let Some(gfx) = self.gfx.as_ref() {
                    if !gfx.window.is_maximized() {
                        let win = self.config.window.get_or_insert_with(WindowState::default);
                        win.x = Some(position.x);
                        win.y = Some(position.y);
                    }
                }
            }
            WindowEvent::Touch(t) => self.on_touch(t),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.rescale(scale_factor as f32);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    /// Both exit paths, the close button and the quit key, land here.
    /// One save stamps the maximized flag; when floating, a direct query
    /// beats the tracked values for the final geometry. Wayland reports
    /// no position, so x and y keep their earlier state there.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_ref() else { return };
        let maximized = gfx.window.is_maximized();
        let mut win = self.config.window.unwrap_or_default();
        if !maximized {
            let size = gfx.window.inner_size();
            win.width = size.width;
            win.height = size.height;
            if let Ok(pos) = gfx.window.outer_position() {
                win.x = Some(pos.x);
                win.y = Some(pos.y);
            }
        }
        win.maximized = maximized;
        self.config.window = Some(win);
        config::save(&self.config);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_ownership_follows_the_last_touched_pane() {
        use super::{owner_after, KeyPane, PaneAct};
        let d = KeyPane::Document;
        let s = KeyPane::Sidebar;
        assert_eq!(owner_after(d, PaneAct::OpenSidebar, true), s, "open grants");
        assert_eq!(
            owner_after(s, PaneAct::CloseSidebar, false),
            d,
            "close returns"
        );
        assert_eq!(
            owner_after(s, PaneAct::ClickDocument, true),
            d,
            "a document click returns"
        );
        assert_eq!(
            owner_after(d, PaneAct::ClickSidebar, true),
            s,
            "a sidebar click grants"
        );
        assert_eq!(
            owner_after(s, PaneAct::WheelDocument, true),
            d,
            "a wheel over the text returns"
        );
        assert_eq!(
            owner_after(d, PaneAct::WheelSidebar, true),
            s,
            "a wheel over the panel grants"
        );
        assert_eq!(owner_after(s, PaneAct::Enter, true), s, "enter holds");
        assert_eq!(
            owner_after(d, PaneAct::Enter, true),
            d,
            "enter holds for the document too"
        );
        assert_eq!(
            owner_after(d, PaneAct::Left, true),
            s,
            "left claims an open sidebar"
        );
        assert_eq!(
            owner_after(d, PaneAct::Left, false),
            d,
            "left without a sidebar does nothing"
        );
        assert_eq!(owner_after(s, PaneAct::Right, true), d, "right hands back");
        assert_eq!(
            owner_after(d, PaneAct::SwitchTab, true),
            s,
            "a tab switch grants"
        );
    }

    #[test]
    fn file_links_resolve_against_the_document_folder() {
        use std::path::{Path, PathBuf};
        let dir = std::env::temp_dir().join(format!("oryx-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("SYNTAX.md"), "# s").unwrap();
        std::fs::write(dir.join("docs/deep guide.md"), "# g").unwrap();
        let base = Some(dir.as_path());

        let (path, fragment) = super::file_link_target("SYNTAX.md", base).expect("a sibling file");
        assert_eq!(path, dir.join("SYNTAX.md"));
        assert_eq!(fragment, None);

        let (path, fragment) =
            super::file_link_target("docs/deep%20guide.md#tables", base).expect("nested, spaced");
        assert_eq!(path, dir.join("docs/deep guide.md"));
        assert_eq!(fragment.as_deref(), Some("tables"));

        assert_eq!(
            super::file_link_target("#anchor", base),
            None,
            "anchors pass"
        );
        assert_eq!(
            super::file_link_target("https://example.com/a.md", base),
            None,
            "urls pass"
        );
        assert_eq!(
            super::file_link_target("mailto:x@y.z", base),
            None,
            "schemes pass"
        );
        assert_eq!(
            super::file_link_target("missing.md", base),
            None,
            "a file that is not there stays a dead link"
        );
        assert_eq!(
            super::file_link_target("SYNTAX.md", None::<&Path>.map(PathBuf::from).as_deref()),
            None,
            "no document folder, no resolution"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn window_title_carries_the_file_name() {
        use std::path::Path;
        assert_eq!(
            super::window_title(None, Some(Path::new("/docs/notes/README.md")), false),
            "README.md · oryx"
        );
        assert_eq!(super::window_title(None, None, false), "oryx");
    }

    #[test]
    fn a_dirty_buffer_carries_the_asterisk() {
        use std::path::Path;
        assert_eq!(
            super::window_title(None, Some(Path::new("/docs/notes.txt")), true),
            "*notes.txt · oryx"
        );
    }

    #[test]
    fn window_title_prefers_the_book_title() {
        use std::path::Path;
        assert_eq!(
            super::window_title(Some("Test Book"), Some(Path::new("/books/b.epub")), false),
            "Test Book · oryx"
        );
    }

    #[test]
    fn theme_dirs_include_the_xdg_data_dir() {
        let dirs = super::theme_dirs();
        assert!(
            dirs.iter().any(|d| d.ends_with("oryx/themes")),
            "installed themes must resolve from the data dir, got {dirs:?}"
        );
    }

    #[test]
    fn icon_pipeline_produces_rgba_and_ico() {
        assert_eq!(super::ICON_64.len(), 64 * 64 * 4);
        let ico: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/oryx.ico"));
        assert_eq!(&ico[..4], &[0, 0, 1, 0], "ICO header magic");
    }
}
