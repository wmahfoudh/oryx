//! Edit mode: the door in and out, and the Escape ladder.
//!
//! The rendered page is the only editing surface. Reading is the app as
//! it stands; editing adds a caret and hands it every bare key. The
//! transitions are pure functions so the tables are testable on their
//! own; `App` owns the wiring.

pub mod caret;
pub mod splice;
pub mod undo;

use crate::doc::load::{self, FileKind};
use crate::doc::model::{BlockKind, Document};
use crate::style::highlight::SyntaxRole;

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
/// their old colors as far as they still cover the row, and the rest of
/// the row reads plain until the worker corrects it: the engine draws
/// only spanned bytes, so carried spans must tile a row to its end or
/// its tail would vanish. A plain file keeps its fresh parse's
/// full-line spans, since no worker follows to correct a carried one.
/// `old_touched` and `new_touched` are the edit's line ranges before
/// and after.
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
    if new.plain_file {
        return new;
    }
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
                cover_row(&mut spans, lines.line(current, line_index));
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

/// Keeps a carried row's spans only while they tile the line
/// contiguously from its start, then fills the rest with Plain: the
/// engine draws only spanned bytes, so a covered row never loses text,
/// and the worker's next pass restores exact colors.
fn cover_row(spans: &mut Vec<(std::ops::Range<usize>, SyntaxRole)>, text: &str) {
    let mut covered = 0;
    spans.retain(|(range, _)| {
        let keep =
            range.start == covered && range.end <= text.len() && text.is_char_boundary(range.end);
        if keep {
            covered = range.end;
        }
        keep
    });
    if covered < text.len() {
        match spans.last_mut() {
            Some((range, SyntaxRole::Plain)) => range.end = text.len(),
            _ => spans.push((covered..text.len(), SyntaxRole::Plain)),
        }
    }
}

/// Applies one edit to a code or text document in place, the keystroke
/// fast path: the source swaps to `current`, the touched line entries
/// rebuild through `CodeBody::splice`, the touched highlight rows
/// splice under the covering rule, and the block range follows. No
/// other part of the document derives from the source, so the splice
/// is the whole re-derivation. Answers the line ranges actually
/// spliced, the layout splice's input; None means the document is not
/// a single-block file and must reparse.
pub fn splice_document(
    doc: &mut Document,
    current: &str,
    old_touched: std::ops::Range<usize>,
    new_touched: std::ops::Range<usize>,
) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    if !doc.code_file && !doc.plain_file {
        return None;
    }
    let plain = doc.plain_file;
    let delta = current.len() as isize - doc.source.len() as isize;
    let [block] = &mut doc.blocks[..] else {
        return None;
    };
    let BlockKind::CodeBlock {
        lines, highlights, ..
    } = &mut block.kind
    else {
        return None;
    };
    let old_len = lines.len();
    if !lines.splice(
        current,
        old_touched.clone(),
        new_touched.clone(),
        delta,
        plain,
    ) {
        return None;
    }
    let new_len = lines.len();
    // The ranges the vector actually replaced: an edit reaching the
    // last entry or past it rebuilt the whole suffix.
    let (old_eff, new_eff) = if old_touched.end >= old_len {
        let from = old_touched.start.min(old_len);
        (from..old_len, from..new_len)
    } else {
        (old_touched, new_touched)
    };
    // Rows splice only as far as the computed prefix reaches; rows the
    // worker never colored stay absent and draw whole.
    if highlights.len() > old_eff.start {
        let upto = old_eff.end.min(highlights.len());
        let rows: Vec<Vec<(std::ops::Range<usize>, SyntaxRole)>> = (0..new_eff.len())
            .map(|i| {
                let mut spans = highlights
                    .get(old_eff.start + i)
                    .filter(|_| old_eff.start + i < upto && !plain)
                    .cloned()
                    .unwrap_or_default();
                if !plain {
                    cover_row(&mut spans, lines.line(current, new_eff.start + i));
                }
                spans
            })
            .collect();
        highlights.splice(old_eff.start..upto, rows);
    }
    block.range = 0..current.len();
    doc.source = std::sync::Arc::from(current);
    Some((old_eff, new_eff))
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
        // Rows tile their lines with no gap, the way worker spans
        // arrive: plain pieces included, every byte covered.
        let mut old = load::code_document(Some("rust"), "let a = 1;\nlet b = 2;\nlet c = 3;\n");
        let rows = [
            vec![(0..3, SyntaxRole::Keyword), (3..10, SyntaxRole::Plain)],
            vec![
                (0..3, SyntaxRole::Keyword),
                (3..8, SyntaxRole::Plain),
                (8..9, SyntaxRole::Number),
                (9..10, SyntaxRole::Plain),
            ],
            vec![(0..3, SyntaxRole::Type), (3..10, SyntaxRole::Plain)],
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
        assert_eq!(
            high[1],
            vec![
                (0..3, SyntaxRole::Keyword),
                (3..8, SyntaxRole::Plain),
                (8..9, SyntaxRole::Number),
                (9..11, SyntaxRole::Plain),
            ],
            "the touched row wears its old colors, covered to its end"
        );
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
            vec![(0..3, SyntaxRole::Keyword), (3..5, SyntaxRole::Plain)],
            "the spans past the shortened row drop and plain covers the rest"
        );
        assert_eq!(
            high[2],
            vec![(0..5, SyntaxRole::Plain)],
            "the just-opened row reads plain until the worker colors it"
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
        assert_eq!(
            high[0],
            vec![(0..3, SyntaxRole::Keyword), (3..20, SyntaxRole::Plain)],
            "the joined row wears the first old row, covered to its end"
        );
        assert_eq!(high[1], rows[2]);
    }

    use crate::style::highlight::SyntaxRole;

    #[test]
    fn a_text_file_edit_keeps_every_line_fully_spanned() {
        // A plain file has no worker to correct a stale carried span,
        // and the engine draws only spanned bytes, whole rows when a
        // row has no spans at all: a carried span shorter than its
        // grown line makes the line's tail vanish from the page. The
        // opened file's rows carry the open pass's all-plain spans.
        let mut old = load::text_document("hello\nworld\n");
        let BlockKind::CodeBlock { highlights, .. } = &mut old.blocks[0].kind else {
            panic!("a text file is one line-oriented block");
        };
        *highlights = vec![
            vec![(0..5, SyntaxRole::Plain)],
            vec![(0..5, SyntaxRole::Plain)],
        ];
        let new = reparse(FileKind::Text, "hello there\nworld\n", &old, 0..1, 0..1);
        let BlockKind::CodeBlock {
            highlights, lines, ..
        } = &new.blocks[0].kind
        else {
            panic!("the reparse keeps the block");
        };
        for (i, row) in highlights.iter().enumerate() {
            if row.is_empty() {
                continue;
            }
            let text = lines.line(&new.source, i);
            let mut covered = 0;
            for (range, _) in row {
                assert_eq!(range.start, covered, "row {i} spans tile with no gap");
                covered = range.end;
            }
            assert_eq!(covered, text.len(), "row {i} spans cover it whole");
        }
    }

    #[test]
    fn a_grown_code_line_keeps_its_tail_covered() {
        let (old, rows) = fixture();
        let new = reparse(
            FileKind::Code("rust"),
            "let a = 1;\nlet b = 2 + 2;\nlet c = 3;\n",
            &old,
            1..2,
            1..2,
        );
        let high = rows_of(&new);
        let grown = "let b = 2 + 2;".len();
        assert_eq!(
            high[1].last().map(|(range, _)| range.end),
            Some(grown),
            "the carried spans still cover the line to its end"
        );
        let mut covered = 0;
        for (range, _) in &high[1] {
            assert_eq!(range.start, covered, "spans tile with no gap");
            covered = range.end;
        }
        assert_eq!(high[0], rows[0]);
        assert_eq!(high[2], rows[2]);
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

    /// Drives one edit through both pipes: the in-place splice on `fast`
    /// and the full reparse on `slow`, which is the splice's referee.
    fn edit_both(
        fast: &mut Document,
        slow: &mut Document,
        kind: FileKind,
        range: std::ops::Range<usize>,
        text: &str,
    ) {
        let removed = slow.source[range.clone()].matches('\n').count();
        let start_line = slow.source[..range.start].matches('\n').count();
        let old_touched = start_line..start_line + removed + 1;
        let new_touched = start_line..start_line + text.matches('\n').count() + 1;
        let mut current = slow.source.to_string();
        current.replace_range(range, text);
        let spliced = splice_document(fast, &current, old_touched.clone(), new_touched.clone());
        assert!(spliced.is_some(), "a file document takes the splice");
        *slow = reparse(kind, &current, slow, old_touched, new_touched);
    }

    fn assert_docs_match(fast: &Document, slow: &Document, colors_too: bool) {
        assert_eq!(&*fast.source, &*slow.source, "sources match");
        assert_eq!(fast.blocks[0].range, slow.blocks[0].range, "ranges match");
        let (BlockKind::CodeBlock {
            lines: fl,
            highlights: fh,
            ..
        },) = (&fast.blocks[0].kind,)
        else {
            panic!("a file is one block");
        };
        let (BlockKind::CodeBlock {
            lines: sl,
            highlights: sh,
            ..
        },) = (&slow.blocks[0].kind,)
        else {
            panic!("the referee is one block");
        };
        assert_eq!(fl.len(), sl.len(), "line counts match");
        for i in 0..fl.len() {
            assert_eq!(
                fl.line(&fast.source, i),
                sl.line(&slow.source, i),
                "line {i} matches"
            );
        }
        if colors_too {
            assert_eq!(fh, sh, "highlight rows match");
        } else {
            // A plain file's rows: empty draws whole, anything else
            // must tile its line.
            for (i, row) in fh.iter().enumerate() {
                if row.is_empty() {
                    continue;
                }
                let text = fl.line(&fast.source, i);
                let mut covered = 0;
                for (range, _) in row {
                    assert_eq!(range.start, covered, "row {i} tiles");
                    covered = range.end;
                }
                assert_eq!(covered, text.len(), "row {i} covers its line");
            }
        }
    }

    #[test]
    fn the_splice_matches_the_reparse_on_a_code_file() {
        let (mut fast, _) = fixture();
        let (mut slow, _) = fixture();
        let kind = FileKind::Code("rust");
        // Typing, a split, a join, an edit on the last line, an edit at
        // the very end of the file.
        edit_both(&mut fast, &mut slow, kind, 15..15, "x");
        assert_docs_match(&fast, &slow, true);
        edit_both(&mut fast, &mut slow, kind, 16..16, "\n");
        assert_docs_match(&fast, &slow, true);
        edit_both(&mut fast, &mut slow, kind, 10..11, "");
        assert_docs_match(&fast, &slow, true);
        let end = fast.source.len();
        edit_both(&mut fast, &mut slow, kind, end - 1..end, "");
        assert_docs_match(&fast, &slow, true);
        let end = fast.source.len();
        edit_both(&mut fast, &mut slow, kind, end..end, "tail");
        assert_docs_match(&fast, &slow, true);
    }

    #[test]
    fn the_splice_matches_the_reparse_on_a_text_file() {
        let mut fast = load::text_document("hello\nworld\n\ntail\n");
        let mut slow = load::text_document("hello\nworld\n\ntail\n");
        let kind = FileKind::Text;
        edit_both(&mut fast, &mut slow, kind, 2..2, "y");
        assert_docs_match(&fast, &slow, false);
        edit_both(&mut fast, &mut slow, kind, 4..4, "\n");
        assert_docs_match(&fast, &slow, false);
        edit_both(&mut fast, &mut slow, kind, 8..9, "");
        assert_docs_match(&fast, &slow, false);
        let end = fast.source.len();
        edit_both(&mut fast, &mut slow, kind, end..end, "\n\n");
        assert_docs_match(&fast, &slow, false);
    }

    #[test]
    fn a_markdown_document_declines_the_splice() {
        let mut doc = crate::doc::markdown::parse("# title\n\nbody\n");
        assert!(
            splice_document(&mut doc, "# title\n\nbodyx\n", 2..3, 2..3).is_none(),
            "only single-block files splice"
        );
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
