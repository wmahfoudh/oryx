//! Edit mode: the door in and out, and the Escape ladder.
//!
//! The rendered page is the only editing surface. Reading is the app as
//! it stands; editing adds a caret and hands it every bare key. The
//! transitions are pure functions so the tables are testable on their
//! own; `App` owns the wiring.

pub mod caret;

use crate::doc::load::FileKind;

/// The app-wide mode. Reading is the default; editing is entered
/// through Ctrl+E and left through Escape or Ctrl+E again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Read,
    Edit,
}

/// Why the door stays shut. Markdown waits for its editing milestone; a
/// book and a lossy read can never promise byte fidelity back to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Markdown,
    Book,
    Lossy,
}

impl Refusal {
    /// The corner-notice line shown for the refusal.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Markdown => "Markdown files cannot be edited yet",
            Refusal::Book => "Books cannot be edited",
            Refusal::Lossy => "This file did not decode cleanly and cannot be edited",
        }
    }
}

/// Answers Ctrl+E: the flipped mode, or the refusal the notice shows.
/// Leaving edit mode never refuses.
pub fn toggle(mode: Mode, kind: FileKind, lossy: bool) -> Result<Mode, Refusal> {
    if mode == Mode::Edit {
        return Ok(Mode::Read);
    }
    match kind {
        // Undisplayable never becomes a document; the arm keeps the
        // match total.
        FileKind::Epub | FileKind::Undisplayable => Err(Refusal::Book),
        FileKind::Markdown => Err(Refusal::Markdown),
        _ if lossy => Err(Refusal::Lossy),
        FileKind::Code(_) | FileKind::Text | FileKind::Unknown => Ok(Mode::Edit),
    }
}

/// What one Escape press closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeAct {
    CloseFind,
    ClearSelection,
    LeaveEdit,
    CloseSidebar,
    Quit,
}

/// The Escape ladder, innermost out, one press each. While editing: the
/// find bar, then the selection, then the mode itself. While reading
/// the cascade stays as it stands: the find bar, then an open sidebar,
/// then the app. Overlays intercept upstream and never reach here.
pub fn escape(mode: Mode, find_open: bool, has_selection: bool, sidebar_open: bool) -> EscapeAct {
    if find_open {
        return EscapeAct::CloseFind;
    }
    match mode {
        Mode::Edit if has_selection => EscapeAct::ClearSelection,
        Mode::Edit => EscapeAct::LeaveEdit,
        Mode::Read if sidebar_open => EscapeAct::CloseSidebar,
        Mode::Read => EscapeAct::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_door_opens_on_text_code_and_unknown_files() {
        assert_eq!(toggle(Mode::Read, FileKind::Text, false), Ok(Mode::Edit));
        assert_eq!(
            toggle(Mode::Read, FileKind::Code("rust"), false),
            Ok(Mode::Edit)
        );
        assert_eq!(
            toggle(Mode::Read, FileKind::Unknown, false),
            Ok(Mode::Edit),
            "an unknown file displays as code and edits as code"
        );
    }

    #[test]
    fn the_door_refuses_markdown_books_and_lossy_reads() {
        assert_eq!(
            toggle(Mode::Read, FileKind::Markdown, false),
            Err(Refusal::Markdown)
        );
        assert_eq!(
            toggle(Mode::Read, FileKind::Epub, false),
            Err(Refusal::Book)
        );
        assert_eq!(
            toggle(Mode::Read, FileKind::Text, true),
            Err(Refusal::Lossy),
            "a lossy read refuses whatever the kind"
        );
    }

    #[test]
    fn ctrl_e_always_leads_back_to_reading() {
        assert_eq!(toggle(Mode::Edit, FileKind::Text, false), Ok(Mode::Read));
        assert_eq!(
            toggle(Mode::Edit, FileKind::Code("rust"), true),
            Ok(Mode::Read),
            "leaving never refuses"
        );
    }

    #[test]
    fn refusals_carry_distinct_messages() {
        let texts = [
            Refusal::Markdown.message(),
            Refusal::Book.message(),
            Refusal::Lossy.message(),
        ];
        for text in texts {
            assert!(!text.is_empty());
        }
        assert_ne!(texts[0], texts[1]);
        assert_ne!(texts[1], texts[2]);
        assert_ne!(texts[0], texts[2]);
    }

    #[test]
    fn escape_layers_innermost_out_while_editing() {
        assert_eq!(escape(Mode::Edit, true, true, true), EscapeAct::CloseFind);
        assert_eq!(
            escape(Mode::Edit, false, true, true),
            EscapeAct::ClearSelection
        );
        assert_eq!(escape(Mode::Edit, false, false, true), EscapeAct::LeaveEdit);
        assert_eq!(
            escape(Mode::Edit, false, false, false),
            EscapeAct::LeaveEdit,
            "the sidebar outlives the mode; Escape reaches it in read mode"
        );
    }

    #[test]
    fn escape_keeps_the_reading_cascade() {
        assert_eq!(escape(Mode::Read, true, false, true), EscapeAct::CloseFind);
        assert_eq!(
            escape(Mode::Read, false, true, true),
            EscapeAct::CloseSidebar,
            "read mode never spends Escape on the selection"
        );
        assert_eq!(escape(Mode::Read, false, false, false), EscapeAct::Quit);
    }
}
