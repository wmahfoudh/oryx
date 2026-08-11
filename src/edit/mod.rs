//! Edit mode: the door in and out, and the Escape ladder.
//!
//! The rendered page is the only editing surface. Reading is the app as
//! it stands; editing adds a caret and hands it every bare key. The
//! transitions are pure functions so the tables are testable on their
//! own; `App` owns the wiring.

pub mod caret;
pub mod splice;

use crate::doc::load::{self, FileKind};
use crate::doc::model::{BlockKind, Document};

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

/// Re-derives the document after a ledger edit: the current text
/// reparsed by its kind, with every computed highlight carried over so
/// no row flashes plain while the worker re-runs. The touched rows wear
/// their old colors, close enough until the worker corrects them; a
/// stale span that no longer fits its row drops. `old_touched` and
/// `new_touched` are the edit's line ranges before and after.
pub fn reparse(
    kind: FileKind,
    current: &str,
    old: &Document,
    old_touched: std::ops::Range<usize>,
    new_touched: std::ops::Range<usize>,
) -> Document {
    let mut new = match kind {
        FileKind::Code(token) => load::code_document(Some(token), current),
        FileKind::Unknown => load::code_document(None, current),
        _ => load::text_document(current),
    };
    let empty = Vec::new();
    let old_high = match old.blocks.first().map(|b| &b.kind) {
        Some(BlockKind::CodeBlock { highlights, .. }) => highlights,
        _ => &empty,
    };
    if let Some(BlockKind::CodeBlock {
        highlights, lines, ..
    }) = new.blocks.first_mut().map(|b| &mut b.kind)
    {
        let mut carried = old_high[..old_touched.start.min(old_high.len())].to_vec();
        for i in 0..new_touched.len() {
            let mut spans = old_high
                .get(old_touched.start + i)
                .filter(|_| old_touched.start + i < old_touched.end)
                .cloned()
                .unwrap_or_default();
            let line_index = new_touched.start + i;
            if line_index < lines.len() {
                let text = lines.line(current, line_index);
                spans.retain(|(range, _)| {
                    range.end <= text.len()
                        && text.is_char_boundary(range.start)
                        && text.is_char_boundary(range.end)
                });
            } else {
                spans.clear();
            }
            carried.push(spans);
        }
        if old_touched.end < old_high.len() {
            carried.extend_from_slice(&old_high[old_touched.end..]);
        }
        carried.truncate(lines.len());
        *highlights = carried;
    }
    new
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

    use crate::style::highlight::LineSpans;

    fn fixture() -> (Document, [LineSpans; 3]) {
        use crate::style::highlight::SyntaxRole;
        let mut old = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\nlet c = 3;\n");
        let rows = [
            vec![(0..3, SyntaxRole::Keyword)],
            vec![(0..3, SyntaxRole::String), (6..9, SyntaxRole::Number)],
            vec![(0..3, SyntaxRole::Type)],
        ];
        let BlockKind::CodeBlock { highlights, .. } = &mut old.blocks[0].kind else {
            panic!("a code file is one code block");
        };
        *highlights = rows.to_vec();
        (old, rows)
    }

    fn rows_of(doc: &Document) -> &[LineSpans] {
        let BlockKind::CodeBlock { highlights, .. } = &doc.blocks[0].kind else {
            panic!("a code file stays one code block");
        };
        highlights
    }

    #[test]
    fn reparse_keeps_every_row_colored_through_an_edit() {
        let (old, rows) = fixture();
        let new = reparse(
            FileKind::Code("rust"),
            "let a = 1;\nlet bb = 2;\nlet c = 3;\n",
            &old,
            1..2,
            1..2,
        );
        assert!(new.code_file, "the reparse keeps the document a code file");
        let high = rows_of(&new);
        assert_eq!(high.len(), 3, "no row goes blank while the worker runs");
        assert_eq!(high[0], rows[0]);
        assert_eq!(high[1], rows[1], "the touched row wears its old colors");
        assert_eq!(high[2], rows[2], "the tail keeps its colors");
    }

    #[test]
    fn a_split_shifts_the_highlight_tail_and_drops_unfit_spans() {
        let (old, rows) = fixture();
        let new = reparse(
            FileKind::Code("rust"),
            "let a = 1;\nlet b\n = 2;\nlet c = 3;\n",
            &old,
            1..2,
            1..3,
        );
        let high = rows_of(&new);
        assert_eq!(high.len(), 4);
        assert_eq!(high[0], rows[0]);
        assert_eq!(
            high[1],
            vec![rows[1][0].clone()],
            "the span past the shortened row drops"
        );
        assert!(
            high[2].is_empty(),
            "the just-opened row waits for the worker"
        );
        assert_eq!(high[3], rows[2], "the tail shifts down intact");
    }

    #[test]
    fn a_join_pulls_the_highlight_tail_up() {
        let (old, rows) = fixture();
        let new = reparse(
            FileKind::Code("rust"),
            "let a = 1;let b = 2;\nlet c = 3;\n",
            &old,
            0..2,
            0..1,
        );
        let high = rows_of(&new);
        assert_eq!(high.len(), 2);
        assert_eq!(high[0], rows[0], "the joined row wears the first old row");
        assert_eq!(high[1], rows[2]);
    }

    #[test]
    fn reparse_of_a_text_file_stays_line_oriented_prose() {
        let old = load::text_document("hello\n");
        let new = reparse(FileKind::Text, "hello world\n", &old, 0..1, 0..1);
        assert!(matches!(
            new.blocks[0].kind,
            BlockKind::CodeBlock { language: None, .. }
        ));
        assert!(new.plain_file);
        assert!(!new.code_file);
        assert_eq!(&*new.source, "hello world\n");
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
