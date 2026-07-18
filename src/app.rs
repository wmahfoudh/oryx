use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::model::Document;
use oryx::layout::{layout, metrics, DecoRect, LayoutDoc, ViewConfig};
use oryx::paint;
use oryx::paint::painter::Painter;
use oryx::paint::scroll::{self, BandCache};
use oryx::platform::config::{self, Config};
use oryx::style::fonts::FontStore;
use oryx::style::theme::{self, Theme};
use oryx::ui::overlay::{Action, Overlay, OverlayResult};
use oryx::ui::scrollbar;
use oryx::ui::selection::{self, RunPos, Selection};
use oryx::ui::theme_browser::ThemeBrowser;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

pub fn run(path: Option<PathBuf>, theme_name: Option<String>) -> anyhow::Result<()> {
    let document = match &path {
        Some(p) => load::open(p)?,
        None => Document::default(),
    };
    let doc_dir = path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let config = config::load();
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
        theme: startup_theme(Some(theme_choice)),
        cfg,
        config,
        fonts: FontStore::new(),
        media: MediaCache::new(doc_dir),
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
        pending_band_for: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Theme directories in lookup order: next to the binary, then the
/// working directory.
fn theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("themes"));
        }
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
    theme: Theme,
    cfg: ViewConfig,
    config: Config,
    fonts: FontStore,
    media: MediaCache,
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
    /// Deferred band rebuild, tagged with the window size it was scheduled
    /// at. Interactive frames (drag, live resize) paint the viewport
    /// directly; the expensive band builds one frame later, once stable.
    pending_band_for: Option<(u32, u32)>,
}

struct Gfx {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

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

    fn handle_key(&mut self, key: &NamedKey) -> bool {
        match key {
            NamedKey::ArrowDown => self.scroll_by(self.line_step()),
            NamedKey::ArrowUp => self.scroll_by(-self.line_step()),
            NamedKey::PageDown => self.scroll_by(self.page_step()),
            NamedKey::PageUp => self.scroll_by(-self.page_step()),
            NamedKey::Space if self.modifiers.shift_key() => self.scroll_by(-self.page_step()),
            NamedKey::Space => self.scroll_by(self.page_step()),
            NamedKey::Home => self.scroll_to(0.0),
            NamedKey::End => self.scroll_to(self.doc_height()),
            _ => return false,
        }
        true
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
        let x = self.cursor.x as f32;
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
        let x = self.cursor.x as f32;
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

    /// Opens the theme browser, or closes it when already open.
    fn toggle_theme_browser(&mut self) {
        if self.overlay.is_some() {
            self.overlay = None;
        } else {
            self.overlay = Some(Box::new(ThemeBrowser::new(
                theme_dirs(),
                &self.config.theme,
            )));
        }
        self.request_redraw();
    }

    /// Applies what an overlay asked for after handling an event.
    fn overlay_result(&mut self, result: OverlayResult) {
        match result {
            OverlayResult::Open => {}
            OverlayResult::Close => self.overlay = None,
            OverlayResult::Apply(Action::SetTheme(path)) => self.apply_theme(&path),
            OverlayResult::Apply(Action::RenamedTheme { from, to }) => {
                if self.config.theme == from {
                    self.config.theme = to;
                    config::save(&self.config);
                }
            }
        }
        self.request_redraw();
    }

    /// Switches to a theme file, persists the choice, and restyles.
    fn apply_theme(&mut self, path: &std::path::Path) {
        let Some(theme) = theme::load_file(path) else {
            return;
        };
        self.theme = theme;
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            self.config.theme = name.to_string();
            config::save(&self.config);
        }
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
        let x = self.cursor.x as f32;
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
        let x = self.cursor.x as f32;
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
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let size = gfx.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let w = size.width as f32;
        if self.layout.is_none() || self.layout_width != w {
            self.layout = Some(layout(
                &self.document,
                &self.theme,
                &mut self.fonts,
                &mut self.media,
                &self.cfg,
                w,
            ));
            self.layout_width = w;
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
            b.width == size.width
                && b.height == size.height * 5
                && !b.needs_repaint(self.scroll_y, size.height as f32)
        });
        let size_tag = (size.width, size.height);
        let interactive = self.drag.is_some() || self.sel_anchor.is_some();
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
                    size.width,
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
                    size.width,
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
        let len = view.len().min(buffer.len());
        buffer[..len].copy_from_slice(&view[..len]);
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
        if let Some(overlay) = self.overlay.as_mut() {
            let mut painter = Painter::new(size.width, size.height, &mut self.fonts);
            overlay.draw(&mut painter, &self.theme);
            painter.composite(&mut buffer, size.width);
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
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("oryx");
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
            } => match logical_key {
                Key::Character(c)
                    if self.modifiers.control_key() && c.as_str().eq_ignore_ascii_case("t") =>
                {
                    self.toggle_theme_browser();
                }
                key if self.overlay.is_some() => {
                    let result = self.overlay.as_mut().expect("overlay open").key(&key);
                    self.overlay_result(result);
                }
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Named(named) => {
                    self.handle_key(&named);
                }
                Key::Character(c) if self.modifiers.control_key() => match c.as_str() {
                    "c" | "C" => self.copy_selection(self.modifiers.shift_key()),
                    "a" | "A" => self.select_all(),
                    _ => {}
                },
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => -lines,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.line_step(),
                };
                if let Some(overlay) = self.overlay.as_mut() {
                    let result = overlay.scroll(lines);
                    self.overlay_result(result);
                } else {
                    self.scroll_by(lines * 3.0 * self.line_step());
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                if self.overlay.is_some() {
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
                        let (x, y) = (self.cursor.x as f32, self.cursor.y as f32);
                        let result = overlay.click(x, y);
                        self.overlay_result(result);
                    } else {
                        self.scrollbar_press();
                        if self.drag.is_none() {
                            self.begin_selection();
                        }
                    }
                }
                ElementState::Released => {
                    if self.overlay.is_some() {
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
