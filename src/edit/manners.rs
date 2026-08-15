//! The editor's manners: what Enter and Tab insert beyond the bare
//! byte. Every decision is a pure function from line bytes to inserted
//! bytes, so the covenant stays auditable: nothing is inserted that is
//! not a newline, a marker, or a prefix of what the line already holds.

/// The bytes Enter inserts with the caret `col` bytes into `line`: a
/// newline followed by the line's leading whitespace, copied byte for
/// byte and clipped at the caret, so a split inside the indentation
/// carries only what stands before it. Only ASCII space and tab count
/// as indentation.
pub fn enter_text(line: &str, col: usize) -> String {
    let head = &line[..col.min(line.len())];
    let indent = head.len() - head.trim_start_matches([' ', '\t']).len();
    let mut text = String::with_capacity(1 + indent);
    text.push('\n');
    text.push_str(&head[..indent]);
    text
}

/// What Enter does after a markdown marker: continue the construct on
/// the new line, or end it when the item stands empty.
#[derive(Debug, PartialEq, Eq)]
pub enum MarkdownEnter {
    /// The bytes to insert at the caret, newline included.
    Insert(String),
    /// The item is bare marker and the caret sits at its end: delete
    /// this many bytes from the line start and insert nothing.
    Unwind(usize),
}

/// The markdown continuation decision for Enter with the caret `col`
/// bytes into `line`. None when the line opens with no quote or list
/// marker, or the caret sits inside the marker; the plain indent carry
/// of `enter_text` is the fallback either way.
pub fn markdown_enter(line: &str, col: usize) -> Option<MarkdownEnter> {
    let (prefix, continuation) = markdown_prefix(line)?;
    if col < prefix {
        return None;
    }
    if line.len() == prefix {
        return Some(MarkdownEnter::Unwind(prefix));
    }
    Some(MarkdownEnter::Insert(format!("\n{continuation}")))
}

/// The prefix a markdown line hands to the next: indentation, a quote
/// run, and at most one list marker with its task box, every byte
/// copied verbatim except the count and the box. Answers the prefix
/// length and the continuation it produces, ordered numbers counted
/// on and task boxes blanked; None when the line opens with neither
/// quote nor marker.
fn markdown_prefix(line: &str) -> Option<(usize, String)> {
    let b = line.as_bytes();
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut i = indent;
    while i < b.len() && b[i] == b'>' {
        i += 1;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
    }
    let quote = i;
    match list_marker(&line[quote..]) {
        Some((len, marker)) => Some((quote + len, format!("{}{marker}", &line[..quote]))),
        None if quote > indent => Some((quote, line[..quote].to_string())),
        None => None,
    }
}

/// One list marker at the start of `rest`: a bullet or a counted item,
/// its following whitespace, and an optional task box. The recognizer
/// stays conservative, a marker without a space after it is content.
fn list_marker(rest: &str) -> Option<(usize, String)> {
    let b = rest.as_bytes();
    let (head, mut cont) = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        (1, rest[..1].to_string())
    } else {
        let digits = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let delim = *b.get(digits)?;
        if delim != b'.' && delim != b')' {
            return None;
        }
        let next = rest[..digits].parse::<u64>().ok()?.checked_add(1)?;
        (digits + 1, format!("{next}{}", delim as char))
    };
    let ws = rest[head..]
        .bytes()
        .take_while(|c| *c == b' ' || *c == b'\t')
        .count();
    if ws == 0 {
        return None;
    }
    let mut len = head + ws;
    cont.push_str(&rest[head..len]);
    let after = &rest.as_bytes()[len..];
    if after.len() > 3
        && after[0] == b'['
        && matches!(after[1], b' ' | b'x' | b'X')
        && after[2] == b']'
    {
        let bws = rest[len + 3..]
            .bytes()
            .take_while(|c| *c == b' ' || *c == b'\t')
            .count();
        if bws > 0 {
            cont.push_str("[ ]");
            cont.push_str(&rest[len + 3..len + 3 + bws]);
            len += 3 + bws;
        }
    }
    Some((len, cont))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_carries_the_indent() {
        assert_eq!(enter_text("    let x = 1;", 14), "\n    ");
    }

    #[test]
    fn tabs_carry_as_tabs() {
        assert_eq!(enter_text("\trecipe", 7), "\n\t");
    }

    #[test]
    fn mixed_runs_carry_verbatim() {
        assert_eq!(enter_text(" \t body", 7), "\n \t ");
    }

    #[test]
    fn a_caret_inside_the_indentation_clips_the_copy() {
        assert_eq!(enter_text("        x", 2), "\n  ");
    }

    #[test]
    fn a_caret_at_line_start_carries_nothing() {
        assert_eq!(enter_text("    x", 0), "\n");
    }

    #[test]
    fn an_unindented_line_splits_bare() {
        assert_eq!(enter_text("plain text", 5), "\n");
    }

    fn insert(text: &str) -> Option<MarkdownEnter> {
        Some(MarkdownEnter::Insert(text.to_string()))
    }

    fn unwind(len: usize) -> Option<MarkdownEnter> {
        Some(MarkdownEnter::Unwind(len))
    }

    #[test]
    fn bullets_continue() {
        assert_eq!(markdown_enter("- item", 6), insert("\n- "));
        assert_eq!(markdown_enter("* item", 6), insert("\n* "));
        assert_eq!(markdown_enter("+ item", 6), insert("\n+ "));
    }

    #[test]
    fn an_indented_item_keeps_its_indent() {
        assert_eq!(markdown_enter("  - item", 8), insert("\n  - "));
    }

    #[test]
    fn ordered_items_count_on_with_their_delimiter() {
        assert_eq!(markdown_enter("1. one", 6), insert("\n2. "));
        assert_eq!(markdown_enter("9) nine", 7), insert("\n10) "));
    }

    #[test]
    fn a_task_continues_unchecked() {
        assert_eq!(markdown_enter("- [x] done", 10), insert("\n- [ ] "));
        assert_eq!(markdown_enter("- [ ] open", 10), insert("\n- [ ] "));
    }

    #[test]
    fn quotes_carry_shallow_and_nested() {
        assert_eq!(markdown_enter("> quoted", 8), insert("\n> "));
        assert_eq!(markdown_enter("> > deep", 8), insert("\n> > "));
        assert_eq!(markdown_enter(">bare", 5), insert("\n>"));
    }

    #[test]
    fn a_quoted_list_carries_both() {
        assert_eq!(markdown_enter("> 1. x", 6), insert("\n> 2. "));
    }

    #[test]
    fn a_split_mid_item_carries_the_marker() {
        assert_eq!(markdown_enter("- one two", 5), insert("\n- "));
    }

    #[test]
    fn a_caret_inside_the_marker_declines() {
        assert_eq!(markdown_enter("- item", 1), None);
        assert_eq!(markdown_enter("- item", 0), None);
        assert_eq!(markdown_enter("- ", 1), None);
    }

    #[test]
    fn near_markers_decline() {
        assert_eq!(markdown_enter("-x", 2), None, "no space, no list");
        assert_eq!(markdown_enter("1.5", 3), None, "a decimal is not a count");
        assert_eq!(markdown_enter("---", 3), None, "a rule is not a list");
        assert_eq!(markdown_enter("**bold**", 8), None);
        assert_eq!(markdown_enter("plain", 5), None);
        assert_eq!(
            markdown_enter("18446744073709551615. x", 23),
            None,
            "a count that cannot step declines whole"
        );
    }

    #[test]
    fn an_empty_item_ends_its_list() {
        assert_eq!(markdown_enter("- ", 2), unwind(2));
        assert_eq!(markdown_enter("  - ", 4), unwind(4));
        assert_eq!(markdown_enter("1. ", 3), unwind(3));
        assert_eq!(markdown_enter("- [ ] ", 6), unwind(6));
        assert_eq!(
            markdown_enter("> ", 2),
            unwind(2),
            "an empty quote ends too"
        );
    }

    // The wired shape: the insertion is one structural unit and one
    // undo heals the split whole, ledger and stack agreeing.
    #[test]
    fn one_undo_heals_the_split() {
        use crate::edit::splice::Ledger;
        use crate::edit::undo::{Kind, Undo};
        let base = "  one two\n";
        let mut ledger = Ledger::new(std::sync::Arc::from(base), Vec::new());
        let mut undo = Undo::new();
        let at = 5;
        let text = enter_text(base.lines().next().unwrap(), at);
        assert_eq!(text, "\n  ");
        ledger.edit(at..at, &text);
        undo.record(
            at..at,
            &text,
            "",
            (at, at + text.len()),
            Kind::Structural,
            std::time::Instant::now(),
        );
        assert_eq!(ledger.current(), "  one\n   two\n");
        let (splice, caret) = undo.undo().expect("one unit stands");
        ledger.edit(splice.range, &splice.text);
        assert_eq!(ledger.current(), base, "the split heals in one step");
        assert_eq!(caret, at);
    }
}
