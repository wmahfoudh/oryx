use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oryx::doc::images::{MediaCache, Waker};
use oryx::doc::load;
use oryx::doc::model::Document;
use oryx::input::keymap::{self, Command};
use oryx::layout::{layout, metrics, DecoRect, LayoutDoc, ViewConfig};
use oryx::paint;
use oryx::paint::painter::Painter;
use oryx::paint::scroll::{self, BandCache};
use oryx::platform::config::{self, Config};
use oryx::style::fonts::FontStore;
use oryx::style::theme::{self, Theme};
use oryx::ui::help::Help;
use oryx::ui::overlay::{Action, Overlay, OverlayResult};
use oryx::ui::scrollbar;
use oryx::ui::selection::{self, RunPos, Selection};
use oryx::ui::settings::{self, Settings};
use oryx::ui::sidebar::{self, Sidebar};
use oryx::ui::theme_browser::ThemeBrowser;
use oryx::ui::theme_editor::ThemeEditor;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// The window icon raster produced by the build script.
const ICON_64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_64.rgba"));

pub fn run(path: Option<PathBuf>, theme_name: Option<String>) -> anyhow::Result<()> {
    let document = match &path {
        Some(p) => load::open(p)?,
        None => Document::default(),
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
        ..ViewConfig::default()
    };
    let theme_choice = theme_name.as_deref().unwrap_or(&config.theme);
    let mut app = App {
        gfx: None,
        document,
        path: path.clone(),
        theme: startup_theme(Some(theme_choice)),
        cfg,
        config,
        fonts: FontStore::new(),
        media,
        waker,
        layout: None,
        layout_width: 0.0,
        band: None,
        scroll_y: 0.0,
        modifiers: ModifiersState::empty(),
        cursor: PhysicalPosition::new(0.0, 0.0),
        drag: None,
        hover_link: false,
        sel_anchor: None,
        selection: None,
        clipboard: None,
        overlay: None,
        overlay_mouse: false,
        pre_edit: None,
        overlay_canvas: None,
        sidebar: None,
        sidebar_canvas: None,
        pending_band_for: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
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

/// Startup theme: the `--theme` override when given, otherwise oryx-light
/// until config persistence lands, dark fallback either way.
fn startup_theme(name: Option<&str>) -> Theme {
    let dirs = theme_dirs();
    if let Some(name) = name {
        match theme::find(&dirs, name) {
            Some(theme) => return theme,
            None => eprintln!("oryx: theme {name:?} not found, using the default"),
        }
    }
    theme::find(&dirs, "oryx-light").unwrap_or_else(Theme::default_dark)
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
    media: MediaCache,
    /// Handed to every media cache so fetch threads can wake the loop.
    waker: Waker,
    layout: Option<LayoutDoc>,
    layout_width: f32,
    band: Option<BandCache>,
    scroll_y: f32,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    /// Scrollbar drag: cursor offset from the thumb top when grabbed.
    drag: Option<f32>,
    /// Whether the cursor currently sits over a link, for the pointer icon.
    hover_link: bool,
    /// Selection drag in progress: the caret grabbed at mouse down.
    sel_anchor: Option<RunPos>,
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
    /// Reused overlay canvas and the region the last frame painted.
    overlay_canvas: Option<OverlayCanvas>,
    /// Folder sidebar while open; the document lays out beside it.
    sidebar: Option<Sidebar>,
    /// Reused sidebar canvas, mirroring the overlay canvas mechanics.
    sidebar_canvas: Option<OverlayCanvas>,
    /// Deferred band rebuild, tagged with the window size it was scheduled
    /// at. Interactive frames (drag, live resize) paint the viewport
    /// directly; the expensive band builds one frame later, once stable.
    pending_band_for: Option<(u32, u32)>,
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
            Command::Reload => self.reload(),
            Command::Sidebar => self.toggle_sidebar(),
            Command::Help => self.toggle_help(),
            Command::Settings => self.toggle_settings(),
            Command::ThemeBrowser => self.toggle_theme_browser(),
            Command::ZoomIn => {
                self.set_zoom(settings::step_zoom(self.cfg.zoom, settings::ZOOM_STEP));
            }
            Command::ZoomOut => {
                self.set_zoom(settings::step_zoom(self.cfg.zoom, -settings::ZOOM_STEP));
            }
            Command::ZoomReset => self.set_zoom(1.0),
            Command::SelectAll => self.select_all(),
            Command::CopyText => self.copy_selection(false),
            Command::CopyMarkdown => self.copy_selection(true),
            Command::LineUp => self.scroll_by(-self.line_step()),
            Command::LineDown => self.scroll_by(self.line_step()),
            Command::PageUp => self.scroll_by(-self.page_step()),
            Command::PageDown => self.scroll_by(self.page_step()),
            Command::Top => self.scroll_to(0.0),
            Command::Bottom => self.scroll_to(self.doc_height()),
            // Escape cascades: the overlay branch catches it first, then
            // an open sidebar absorbs it, then it quits.
            Command::Quit => {
                if self.sidebar.is_some() {
                    self.toggle_sidebar();
                } else {
                    event_loop.exit();
                }
            }
        }
    }

    fn thumb(&self) -> Option<(f32, f32)> {
        let vh = self.viewport_h();
        scrollbar::thumb(self.doc_height(), vh, self.scroll_y, vh)
    }

    fn drag_to(&mut self, cursor_y: f32) {
        let (Some(grab), Some((_, thumb_h))) = (self.drag, self.thumb()) else {
            return;
        };
        let vh = self.viewport_h();
        let target =
            scrollbar::scroll_for_thumb(cursor_y - grab, thumb_h, vh, self.doc_height(), vh);
        self.scroll_to(target);
    }

    /// Grabs a selection anchor at the cursor and clears any previous
    /// selection.
    fn begin_selection(&mut self) {
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        self.sel_anchor = selection::pos_at(lay, &mut self.fonts, x, y);
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
        let Some(end) = selection::pos_at(lay, &mut self.fonts, x, y) else {
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
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        } else {
            self.selection = None;
            self.link_press();
        }
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
            )));
            self.request_redraw();
        }
    }

    /// Document area x offset: the sidebar width while it is open.
    fn inset(&self) -> f32 {
        if self.sidebar.is_some() {
            sidebar::WIDTH
        } else {
            0.0
        }
    }

    fn toggle_sidebar(&mut self) {
        if self.sidebar.take().is_none() {
            let dir = self
                .path
                .as_ref()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .filter(|d| !d.as_os_str().is_empty())
                .unwrap_or_else(|| PathBuf::from("."));
            let mut side = Sidebar::new(&dir);
            if let Some(path) = &self.path {
                side.set_current(path);
            }
            self.sidebar = Some(side);
        } else {
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
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let loaded = load::open(&path);
        let opened = loaded.is_ok();
        self.document = loaded.unwrap_or_else(|err| load::message(&err.to_string()));
        self.path = Some(path.to_path_buf());
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        if opened {
            let dir_text = dir.display().to_string();
            if self.config.last_dir != dir_text {
                self.config.last_dir = dir_text;
                config::save(&self.config);
            }
        }
        self.media = MediaCache::new(dir.clone());
        self.media.set_waker(self.waker.clone());
        self.scroll_y = 0.0;
        self.selection = None;
        self.sel_anchor = None;
        self.layout = None;
        self.band = None;
        if let Some(side) = self.sidebar.as_mut() {
            if reroot && side.root() != dir {
                *side = Sidebar::new(&dir);
            }
            side.set_current(&path);
        }
        self.request_redraw();
    }

    /// Re-reads the open file from disk, keeping the scroll position, for
    /// documents being edited in parallel.
    fn reload(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let scroll = self.scroll_y;
        self.open_file(&path, false);
        self.scroll_y = scroll;
    }

    /// Native open dialog filtered to the recognized extensions.
    fn open_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Supported files", &load::recognized_extensions())
            .add_filter("All files", &["*"]);
        let start = self
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|d| !d.as_os_str().is_empty())
            .or_else(|| {
                (!self.config.last_dir.is_empty()).then(|| PathBuf::from(&self.config.last_dir))
            })
            .filter(|d| d.is_dir());
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_file(&path, true);
        }
    }

    /// Opens the shortcuts help, or closes the active overlay.
    fn toggle_help(&mut self) {
        if self.overlay.is_some() {
            self.overlay_result(OverlayResult::Close);
        } else {
            self.overlay = Some(Box::new(Help::new()));
            self.request_redraw();
        }
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

    /// Applies what an overlay asked for after handling an event.
    fn overlay_result(&mut self, result: OverlayResult) {
        match result {
            OverlayResult::Open => {}
            OverlayResult::Close => {
                self.overlay = None;
                // An editor closed without saving: back to the last
                // applied state.
                if let Some(previous) = self.pre_edit.take() {
                    self.set_live_theme(previous);
                }
            }
            OverlayResult::Apply(Action::SetTheme(path)) => {
                self.apply_theme(&path);
                // A save from the editor moves its revert point forward.
                if self.pre_edit.is_some() {
                    self.pre_edit = Some(self.theme.clone());
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
                    self.pre_edit = Some(self.theme.clone());
                    let preview = editor.current();
                    self.overlay = Some(Box::new(editor));
                    self.set_live_theme(preview);
                }
            }
            OverlayResult::Apply(Action::PreviewTheme(theme)) => {
                self.set_live_theme(*theme);
            }
            OverlayResult::Apply(Action::SetView {
                body_family,
                code_family,
                body_size,
                code_size,
            }) => {
                self.cfg.body_family = body_family.clone();
                self.cfg.code_family = code_family.clone();
                self.cfg.body_size = body_size;
                self.cfg.code_size = code_size;
                self.config.body_family = body_family;
                self.config.code_family = code_family;
                self.config.body_size = body_size;
                self.config.code_size = code_size;
                config::save(&self.config);
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

    /// Selects the whole document.
    fn select_all(&mut self) {
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let Some(sel) = selection::all(lay) else {
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
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let text = if as_markdown {
            selection::markdown(&sel, lay, &self.document)
        } else {
            selection::plain_text(&sel, lay, &self.document)
        };
        if text.is_empty() {
            return;
        }
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

    /// Follow the link under the cursor: anchors scroll, http links open
    /// in the system browser.
    fn link_press(&mut self) {
        let Some(lay) = self.layout.as_ref() else {
            return;
        };
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let Some(target) = lay.link_at(x, y).map(str::to_owned) else {
            return;
        };
        if let Some(anchor) = lay.anchor_y(&target) {
            self.scroll_to(anchor);
        } else if target.starts_with("http://") || target.starts_with("https://") {
            if let Err(err) = open::that_detached(&target) {
                eprintln!("oryx: cannot open {target}: {err}");
            }
        }
    }

    /// Track whether a link sits under the cursor and swap the pointer icon
    /// on transitions.
    fn update_hover(&mut self) {
        let x = self.cursor.x as f32 - self.inset();
        let y = self.cursor.y as f32 + self.scroll_y;
        let hovering = self
            .layout
            .as_ref()
            .is_some_and(|l| l.link_at(x, y).is_some());
        if hovering != self.hover_link {
            self.hover_link = hovering;
            if let Some(gfx) = self.gfx.as_ref() {
                let icon = if hovering {
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
        if x < width - scrollbar::STRIP_WIDTH {
            return;
        }
        if y >= thumb_y && y <= thumb_y + thumb_h {
            self.drag = Some(y - thumb_y);
        } else {
            // Track click: jump so the thumb centers on the cursor.
            self.drag = Some(thumb_h / 2.0);
            self.drag_to(y);
        }
    }

    fn redraw(&mut self) {
        let inset = self.inset() as u32;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let size = gfx.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let avail_px = size.width.saturating_sub(inset).max(1);
        let avail = avail_px as f32;
        if self.layout.is_none() || self.layout_width != avail {
            self.layout = Some(layout(
                &self.document,
                &self.theme,
                &mut self.fonts,
                &mut self.media,
                &self.cfg,
                avail,
            ));
            self.layout_width = avail;
            self.band = None;
            self.pending_band_for = None;
            // Selection positions index the old layout's runs.
            self.selection = None;
            self.sel_anchor = None;
        }
        let lay = self.layout.as_ref().expect("layout exists");
        self.scroll_y = scroll::clamp(self.scroll_y, lay.height, size.height as f32);
        let highlight: Vec<DecoRect> = match &self.selection {
            Some(sel) => selection::rects(sel, lay, &mut self.fonts)
                .into_iter()
                .map(|(x, y, w, h)| DecoRect::fill(x, y, w, h, self.theme.ui.selection_bg))
                .collect(),
            None => Vec::new(),
        };

        let band_usable = self.band.as_ref().is_some_and(|b| {
            b.width == avail_px
                && b.height == size.height * 5
                && !b.needs_repaint(self.scroll_y, size.height as f32)
        });
        let size_tag = (size.width, size.height);
        let interactive = self.drag.is_some() || self.sel_anchor.is_some() || self.overlay_mouse;
        let mut direct: Option<Vec<u32>> = None;
        if !band_usable {
            let build_now = !interactive && self.pending_band_for == Some(size_tag);
            if build_now {
                self.band = Some(BandCache::repaint(
                    lay,
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
        ) {
            let color = if self.drag.is_some() {
                self.theme.ui.scrollbar_hover
            } else {
                self.theme.ui.scrollbar
            };
            scrollbar::draw(&mut buffer, size.width, size.height, thumb, color);
        }
        if let Some(side) = self.sidebar.as_mut() {
            let fits = self
                .sidebar_canvas
                .as_ref()
                .is_some_and(|(p, _)| p.width() == size.width && p.height() == size.height);
            if !fits {
                self.sidebar_canvas =
                    tiny_skia::Pixmap::new(size.width, size.height).map(|pixmap| (pixmap, None));
            }
            if let Some((canvas, stale)) = self.sidebar_canvas.as_mut() {
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take());
                side.draw(&mut painter, &self.theme);
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
                let mut painter = Painter::new(canvas, &mut self.fonts, stale.take());
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
    /// A background fetch landed: fold it in and relayout.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        if self.media.drain_remote() {
            self.layout = None;
            self.band = None;
            self.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // X11 and Windows take the icon here; Wayland resolves it from the
        // desktop entry matching the app_id instead.
        let icon = winit::window::Icon::from_rgba(ICON_64.to_vec(), 64, 64).ok();
        let attributes = Window::default_attributes()
            .with_title("oryx")
            .with_window_icon(icon);
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
        self.gfx = Some(Gfx { window, surface });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();
                // The overlay toggles stay global so their chord closes the
                // overlay it opened; everything else feeds an open overlay.
                match keymap::command(&logical_key, ctrl, shift) {
                    Some(
                        cmd @ (Command::ThemeBrowser
                        | Command::Settings
                        | Command::Help
                        | Command::Sidebar
                        | Command::OpenFile),
                    ) => {
                        self.run_command(cmd, event_loop);
                    }
                    _ if self.overlay.is_some() => {
                        let overlay = self.overlay.as_mut().expect("overlay open");
                        let result = overlay.key(&logical_key, ctrl);
                        self.overlay_result(result);
                    }
                    Some(Command::LineUp) if self.sidebar.is_some() => {
                        if let Some(side) = self.sidebar.as_mut() {
                            side.move_selection(-1);
                        }
                        self.request_redraw();
                    }
                    Some(Command::LineDown) if self.sidebar.is_some() => {
                        if let Some(side) = self.sidebar.as_mut() {
                            side.move_selection(1);
                        }
                        self.request_redraw();
                    }
                    None if self.sidebar.is_some()
                        && matches!(logical_key, Key::Named(NamedKey::Enter)) =>
                    {
                        let opened = self.sidebar.as_mut().and_then(|s| s.enter());
                        if let Some(path) = opened {
                            self.open_file(&path, false);
                        }
                        self.request_redraw();
                    }
                    Some(cmd) => self.run_command(cmd, event_loop),
                    None => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => -lines,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.line_step(),
                };
                let over_sidebar = (self.cursor.x as f32) < sidebar::WIDTH;
                if let Some(overlay) = self.overlay.as_mut() {
                    let result = overlay.scroll(lines);
                    self.overlay_result(result);
                } else if let Some(side) = self.sidebar.as_mut().filter(|_| over_sidebar) {
                    side.wheel(lines * 3.0);
                    self.request_redraw();
                } else {
                    self.scroll_by(lines * 3.0 * self.line_step());
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                if self.overlay.is_some() {
                    if self.overlay_mouse {
                        let (x, y) = (position.x as f32, position.y as f32);
                        let result = self.overlay.as_mut().expect("overlay open").drag(x, y);
                        self.overlay_result(result);
                    }
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
            } => match state {
                ElementState::Pressed => {
                    if let Some(overlay) = self.overlay.as_mut() {
                        self.overlay_mouse = true;
                        let (x, y) = (self.cursor.x as f32, self.cursor.y as f32);
                        let result = overlay.click(x, y);
                        self.overlay_result(result);
                    } else if (self.cursor.x as f32) < sidebar::WIDTH && self.sidebar.is_some() {
                        let (x, y) = (self.cursor.x as f32, self.cursor.y as f32);
                        let opened = self.sidebar.as_mut().and_then(|s| s.click(x, y));
                        if let Some(path) = opened {
                            self.open_file(&path, false);
                        }
                        self.request_redraw();
                    } else {
                        self.scrollbar_press();
                        if self.drag.is_none() {
                            self.begin_selection();
                        }
                    }
                }
                ElementState::Released => {
                    self.overlay_mouse = false;
                    if let Some(overlay) = self.overlay.as_mut() {
                        overlay.release();
                    } else if self.drag.take().is_some() {
                        if let Some(gfx) = self.gfx.as_ref() {
                            gfx.window.request_redraw();
                        }
                    } else {
                        self.end_selection();
                    }
                }
            },
            WindowEvent::Resized(_) => {
                if let Some(gfx) = self.gfx.as_ref() {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
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
