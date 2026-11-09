//! The path field: the way out when the system's file dialog cannot
//! appear, on a window manager without a desktop portal or zenity
//! (GitHub issue 12). A pill over the document's top-right corner, in
//! the form of the Go to line field, holds the path the dialog would
//! have offered, the caret at its end; Enter answers with the path and
//! Escape cancels. An existing file is replaced on the second Enter,
//! the first one saying so, the way an editor refuses to overwrite
//! until asked twice.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};

use crate::paint::painter::Painter;
use crate::style::fonts::CODE_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::search::{self, FieldView};
use crate::ui::textfield::TextField;

/// The pill's width when the window has room for it.
pub const BAR_WIDTH: f32 = 560.0;

/// What the path is for: the flow that asked for the dialog, and goes
/// on with the path when the field answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Open,
    SaveAs,
    New,
    Export,
}

/// What Enter does with the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The path to open, save, create or export to.
    Go(PathBuf),
    /// A file is at the path: the next Enter on the same path replaces
    /// it, and the caller says so.
    Exists(PathBuf),
    /// An empty field.
    Nothing,
}

/// The open field: what it is for, its text, where it was last drawn,
/// the folder a bare name joins, and the existing file the last Enter
/// named, which the next Enter replaces.
pub struct PathState {
    pub ask: Ask,
    pub field: TextField,
    pub view: FieldView,
    pub folder: PathBuf,
    armed: Option<PathBuf>,
}

impl PathState {
    /// The field on the folder and the name the dialog would have
    /// offered: the folder alone ends with its separator, so a name
    /// typed after it lands inside.
    pub fn new(ask: Ask, folder: PathBuf, name: Option<&str>) -> PathState {
        let text = match name {
            Some(name) => folder.join(name).to_string_lossy().into_owned(),
            None => {
                let mut text = folder.to_string_lossy().into_owned();
                if !text.ends_with(MAIN_SEPARATOR) && !text.ends_with('/') {
                    text.push(MAIN_SEPARATOR);
                }
                text
            }
        };
        PathState {
            ask,
            field: TextField::new(text),
            view: FieldView::default(),
            folder,
            armed: None,
        }
    }

    /// The path the text names: trimmed, a bare name joined to the
    /// folder; nothing for an empty field.
    pub fn path(&self) -> Option<PathBuf> {
        let text = self.field.text().trim();
        if text.is_empty() {
            return None;
        }
        let path = Path::new(text);
        Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.folder.join(path)
        })
    }

    /// The word at the left of the pill.
    pub fn label(&self) -> &'static str {
        match self.ask {
            Ask::Open => "Open",
            Ask::SaveAs => "Save as",
            Ask::New => "New file",
            Ask::Export => "Export to PDF",
        }
    }

    /// Enter: the path, or the warning that a file is there. `exists`
    /// is whether something is at the path. An open wants the file to
    /// exist, so it never warns; a save, a new file or an export
    /// replaces an existing file on the second Enter on the same path.
    pub fn enter(&mut self, exists: bool) -> Verdict {
        let Some(path) = self.path() else {
            self.armed = None;
            return Verdict::Nothing;
        };
        if self.ask == Ask::Open || !exists || self.armed.as_deref() == Some(path.as_path()) {
            self.armed = None;
            return Verdict::Go(path);
        }
        self.armed = Some(path.clone());
        Verdict::Exists(path)
    }
}

/// Whether the field takes `text` as typed or pasted: anything on one
/// line, since a path can hold any character but a line break.
pub fn accepts(text: &str) -> bool {
    !text.is_empty() && !text.contains(['\n', '\r'])
}

/// The pill's box for a window `width` wide: x, y, width, height. The
/// pill is as wide as a path wants, or as the window allows.
pub fn bar_rect(width: f32) -> (f32, f32, f32, f32) {
    let w = BAR_WIDTH.min(width - 2.0 * search::MARGIN);
    let x = (width - w - search::MARGIN).max(search::MARGIN);
    (x, search::MARGIN, w, search::BAR_HEIGHT)
}

/// Whether a point at logical window coordinates lies on the pill.
pub fn bar_hit(width: f32, px: f32, py: f32) -> bool {
    let (x, y, w, h) = bar_rect(width);
    px >= x && px < x + w && py >= y && py < y + h
}

/// The floating pill over the document's top-right corner, where the
/// search bar stands: the ask's word on the left, the field after it.
/// `caret_lit` is the blink's lit half while the field has the keys.
pub fn draw_bar(
    painter: &mut Painter,
    theme: &Theme,
    state: &mut PathState,
    width: f32,
    caret_lit: bool,
) {
    let ui = &theme.ui;
    let (x, y, w, h) = bar_rect(width);
    for (grow, alpha) in [(6.0, 16), (3.0, 28)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            w + 2.0 * grow,
            h + 2.0 * grow,
            search::RADIUS + grow,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: alpha,
            },
        );
    }
    painter.fill(x, y, w, h, search::RADIUS, ui.overlay_bg);
    painter.stroke(x, y, w, h, search::RADIUS, 1.0, theme.blocks.table_border);

    let label = state.label();
    let label_w = painter.measure(label, CODE_FAMILY, search::COUNTER_SIZE, 400);
    painter.text(
        x + search::PAD,
        y + 10.0 + search::TEXT_DROP + (search::QUERY_SIZE - search::COUNTER_SIZE) * 1.1,
        label,
        CODE_FAMILY,
        search::COUNTER_SIZE,
        400,
        theme.blocks.frontmatter_fg,
    );
    let left = x + search::PAD + label_w + 12.0;
    let avail = w - 2.0 * search::PAD - label_w - 12.0;
    state.view = search::draw_field(
        painter,
        theme,
        &state.field,
        left,
        y + 10.0,
        avail,
        "path",
        true,
        caret_lit,
        state.view.window.start,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder() -> PathBuf {
        PathBuf::from("/home/me/notes")
    }

    fn state(ask: Ask, name: Option<&str>) -> PathState {
        PathState::new(ask, folder(), name)
    }

    #[test]
    fn the_field_opens_on_the_folder_and_the_name_with_the_caret_at_the_end() {
        let s = state(Ask::SaveAs, Some("a.md"));
        assert_eq!(s.field.text(), folder().join("a.md").to_str().unwrap());
        assert_eq!(s.field.caret(), s.field.text().len());
        let s = state(Ask::Open, None);
        assert_eq!(
            s.field.text(),
            format!("{}{}", folder().display(), std::path::MAIN_SEPARATOR),
            "a folder ends with its separator, ready for a name"
        );
    }

    #[test]
    fn the_path_is_the_text_and_a_bare_name_joins_the_folder() {
        let mut s = state(Ask::SaveAs, Some("a.md"));
        assert_eq!(s.path(), Some(folder().join("a.md")));
        s.field.set_text(" b.md ");
        assert_eq!(s.path(), Some(folder().join("b.md")));
        s.field.set_text("/elsewhere/c.md");
        assert_eq!(s.path(), Some(PathBuf::from("/elsewhere/c.md")));
        s.field.set_text("   ");
        assert_eq!(s.path(), None);
    }

    #[test]
    fn the_label_names_the_ask() {
        assert_eq!(state(Ask::Open, None).label(), "Open");
        assert_eq!(state(Ask::SaveAs, None).label(), "Save as");
        assert_eq!(state(Ask::New, None).label(), "New file");
        assert_eq!(state(Ask::Export, None).label(), "Export to PDF");
    }

    #[test]
    fn enter_names_the_path_and_an_existing_file_is_replaced_on_the_second_enter() {
        let mut s = state(Ask::SaveAs, Some("a.md"));
        let a = folder().join("a.md");
        assert_eq!(
            s.enter(false),
            Verdict::Go(a.clone()),
            "a new file is written at once"
        );
        assert_eq!(
            s.enter(true),
            Verdict::Exists(a.clone()),
            "an existing one asks"
        );
        assert_eq!(
            s.enter(true),
            Verdict::Go(a.clone()),
            "the second Enter replaces it"
        );
        s.field.set_text("b.md");
        let b = folder().join("b.md");
        assert_eq!(s.enter(true), Verdict::Exists(b), "another name asks again");
        s.field.set_text("a.md");
        assert_eq!(
            s.enter(true),
            Verdict::Exists(a.clone()),
            "the first name asks again: the second Enter must follow the first"
        );
        s.field.set_text("");
        assert_eq!(s.enter(false), Verdict::Nothing);
        let mut o = state(Ask::Open, Some("a.md"));
        assert_eq!(
            o.enter(true),
            Verdict::Go(a),
            "an open wants the file to exist"
        );
    }

    #[test]
    fn the_field_takes_any_text_but_a_line_break() {
        for taken in ["a", "/home/me/a b.md", "C:\\notes\\a.md", "é", " "] {
            assert!(accepts(taken), "{taken:?}");
        }
        for refused in ["", "\n", "a\nb", "\r"] {
            assert!(!accepts(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_pill_stands_at_the_right_and_fits_a_narrow_window() {
        let (x, _, w, _) = bar_rect(1000.0);
        assert_eq!(w, BAR_WIDTH);
        assert_eq!(x + w, 1000.0 - search::MARGIN);
        let (x, _, w, _) = bar_rect(400.0);
        assert_eq!(x, search::MARGIN);
        assert_eq!(w, 400.0 - 2.0 * search::MARGIN);
        assert!(bar_hit(1000.0, 800.0, 30.0));
        assert!(!bar_hit(1000.0, 100.0, 30.0));
    }
}
