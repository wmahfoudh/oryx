use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use oryx::doc::load;
use oryx::doc::model::Document;
use oryx::layout::{layout, metrics, LayoutDoc, ViewConfig};
use oryx::paint::scroll::{self, BandCache};
use oryx::style::fonts::FontStore;
use oryx::style::theme::{self, Theme};
use oryx::ui::scrollbar;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

pub fn run(path: Option<PathBuf>) -> anyhow::Result<()> {
    let document = match &path {
        Some(p) => load::open(p)?,
        None => Document::default(),
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        gfx: None,
        document,
        theme: startup_theme(),
        cfg: ViewConfig::default(),
        fonts: FontStore::new(),
        layout: None,
        layout_width: 0.0,
        band: None,
        scroll_y: 0.0,
        modifiers: ModifiersState::empty(),
        cursor: PhysicalPosition::new(0.0, 0.0),
        drag: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Initial theme until config persistence lands: oryx-light from the themes
/// directory next to the binary or in the working directory, dark fallback.
fn startup_theme() -> Theme {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("themes/oryx-light.toml"));
        }
    }
    candidates.push(PathBuf::from("themes/oryx-light.toml"));
    candidates
        .iter()
        .filter(|p| p.exists())
        .find_map(|p| theme::load_file(p))
        .unwrap_or_else(Theme::default_dark)
}

struct App {
    gfx: Option<Gfx>,
    document: Document,
    theme: Theme,
    cfg: ViewConfig,
    fonts: FontStore,
    layout: Option<LayoutDoc>,
    layout_width: f32,
    band: Option<BandCache>,
    scroll_y: f32,
    modifiers: ModifiersState,
    cursor: PhysicalPosition<f64>,
    /// Scrollbar drag: cursor offset from the thumb top when grabbed.
    drag: Option<f32>,
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
                &self.cfg,
                w,
            ));
            self.layout_width = w;
            self.band = None;
        }
        let lay = self.layout.as_ref().expect("layout exists");
        self.scroll_y = scroll::clamp(self.scroll_y, lay.height, size.height as f32);

        let band_stale = match &self.band {
            None => true,
            Some(b) => {
                b.width != size.width
                    || b.height != size.height * 5
                    || b.needs_repaint(self.scroll_y, size.height as f32)
            }
        };
        if band_stale {
            self.band = Some(BandCache::repaint(
                lay,
                &self.theme,
                &mut self.fonts,
                self.scroll_y,
                size.width,
                size.height,
            ));
        }
        let band = self.band.as_ref().expect("band exists");
        let view = band.view(self.scroll_y, size.height);

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
        buffer.present().expect("present failed");
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
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Named(named) => {
                    self.handle_key(&named);
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::LineDelta(_, lines) => {
                    self.scroll_by(-lines * 3.0 * self.line_step())
                }
                MouseScrollDelta::PixelDelta(p) => self.scroll_by(-p.y as f32),
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                if self.drag.is_some() {
                    self.drag_to(position.y as f32);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.scrollbar_press(),
                ElementState::Released => {
                    if self.drag.take().is_some() {
                        if let Some(gfx) = self.gfx.as_ref() {
                            gfx.window.request_redraw();
                        }
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
