use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use oryx::doc::load;
use oryx::doc::model::Document;
use oryx::layout::{layout, LayoutDoc, ViewConfig};
use oryx::paint;
use oryx::style::fonts::FontStore;
use oryx::style::theme::{self, Theme};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
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
        scroll_y: 0.0,
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
    scroll_y: f32,
}

struct Gfx {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

impl App {
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
        }
        let pixels = paint::band(
            self.layout.as_ref().expect("layout exists"),
            &self.theme,
            &mut self.fonts,
            self.scroll_y,
            size.width,
            size.height,
        );
        gfx.surface
            .resize(width, height)
            .expect("surface resize failed");
        let mut buffer = gfx.surface.buffer_mut().expect("buffer borrow failed");
        buffer.copy_from_slice(&pixels);
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
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
