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
/// of `enter_text` is the fallback either way. A marker followed by
/// nothing but whitespace is an empty item wherever the caret stands
/// past the marker's last non-blank byte: layout trims trailing spaces,
/// so a click or an arrow move can seat the caret between the dash and
/// its space, and Enter there must still end the list.
pub fn markdown_enter(line: &str, col: usize) -> Option<MarkdownEnter> {
    let (prefix, continuation) = markdown_prefix(line)?;
    let core = line[..prefix].trim_end_matches([' ', '\t']).len();
    if col < core {
        return None;
    }
    if line[core..].trim_matches([' ', '\t']).is_empty() {
        return Some(MarkdownEnter::Unwind(line.len()));
    }
    if col < prefix {
        return None;
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
    let (indent, quote) = marker_seat(line);
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

/// True when Tab nests the whole item rather than inserting: the line
/// is a markdown list item, quoted or not, and the caret sits at or
/// before its first content byte, which is where Enter's continuation
/// leaves it. A bare quote line never nests, since four leading spaces
/// would turn the quote into an indented code block.
pub fn tab_nests(line: &str, col: usize) -> bool {
    let (_, quote) = marker_seat(line);
    match list_marker(&line[quote..]) {
        Some((len, _)) => col <= quote + len,
        None => false,
    }
}

/// The seat a list marker would stand on: the byte width of the line's
/// indentation, and of the quote run with each `>`'s trailing
/// whitespace after it.
fn marker_seat(line: &str) -> (usize, usize) {
    let b = line.as_bytes();
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut i = indent;
    while i < b.len() && b[i] == b'>' {
        i += 1;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
    }
    (indent, i)
}

/// One indent step, resolved per file the way new line endings resolve
/// to the dominant ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentUnit {
    Tab,
    Spaces(u8),
}

impl IndentUnit {
    /// The bytes Tab inserts.
    pub fn text(self) -> String {
        match self {
            IndentUnit::Tab => "\t".to_string(),
            IndentUnit::Spaces(n) => " ".repeat(n as usize),
        }
    }
}

/// The file's dominant indent: a tab where tab-led lines dominate, the
/// dominant space step where space-led lines do, a tab where the file
/// offers no evidence. The step is the most common leading-width
/// difference between a line and the nearest less-indented line above,
/// kept inside 2..=8; ties go to the smaller step. The scan caps at
/// the first 64KB, which carries any real file's indentation habits:
/// the full pass read 9.5ms on the 8MB fixture, too slow for a held
/// key, and the capped one is free at human rate.
pub fn indent_unit(source: &str) -> IndentUnit {
    let mut cap = source.len().min(64 * 1024);
    while !source.is_char_boundary(cap) {
        cap -= 1;
    }
    let mut tabs = 0usize;
    let mut spaces = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut diffs = [0usize; 9];
    for line in source[..cap].lines() {
        let b = line.as_bytes();
        match b.first() {
            Some(b'\t') => tabs += 1,
            Some(b' ') => {
                spaces += 1;
                let w = b.iter().take_while(|c| **c == b' ').count();
                while stack.last().is_some_and(|&t| t >= w) {
                    stack.pop();
                }
                let d = w - stack.last().copied().unwrap_or(0);
                if (2..=8).contains(&d) {
                    diffs[d] += 1;
                }
                stack.push(w);
            }
            // Content at the margin: the nearest less-indented line
            // above anything after it is this one, at width zero.
            Some(_) => stack.clear(),
            None => {}
        }
    }
    if spaces == 0 || tabs >= spaces {
        return IndentUnit::Tab;
    }
    (2..=8)
        .filter(|&d| diffs[d] > 0)
        .max_by_key(|&d| (diffs[d], std::cmp::Reverse(d)))
        .map_or(IndentUnit::Spaces(4), |d| IndentUnit::Spaces(d as u8))
}

/// Re-indents a region of whole lines, without its trailing newline:
/// one unit onto every non-empty line, or one unit off every line that
/// carries one. Answers the new text and one byte delta per line, the
/// caller's map from old positions to new.
pub fn reindent(region: &str, unit: &IndentUnit, outdent: bool) -> (String, Vec<i64>) {
    let mut out = String::with_capacity(region.len() + 64);
    let mut deltas = Vec::with_capacity(8);
    for (i, line) in region.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let delta = if outdent {
            let cut = outdent_cut(line, unit);
            out.push_str(&line[cut..]);
            -(cut as i64)
        } else if line.is_empty() {
            0
        } else {
            let ins = unit.text();
            out.push_str(&ins);
            out.push_str(line);
            ins.len() as i64
        };
        deltas.push(delta);
    }
    (out, deltas)
}

/// A list kind the line keys set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListKind {
    Bullet,
    Numbered,
    Task,
}

/// The list marker at the start of `rest`, the text after a line's
/// indentation: its kind and its byte length, whitespace included.
fn marker_of(rest: &str) -> Option<(ListKind, usize)> {
    let (len, continuation) = list_marker(rest)?;
    let kind = if continuation.contains("[ ]") {
        ListKind::Task
    } else if rest.as_bytes()[0].is_ascii_digit() {
        ListKind::Numbered
    } else {
        ListKind::Bullet
    };
    Some((kind, len))
}

/// Sets or clears a list over a region of whole lines, without its
/// trailing newline. Every non-blank line gets `kind`'s marker after
/// its indentation, replacing the list marker it carries; a line
/// already of that kind keeps its own (a numbered one is renumbered);
/// when every non-blank line already carries `kind`, the markers come
/// off. Numbered items count from 1 in order. Answers the new text
/// and, per line, the byte column of the change and its delta, the
/// caller's map from old positions to new.
pub fn list_lines(region: &str, kind: ListKind) -> (String, Vec<(usize, i64)>) {
    /// A non-blank line: its indentation and the marker after it.
    struct Item {
        indent: usize,
        marker: Option<(ListKind, usize)>,
    }
    let lines: Vec<&str> = region.split('\n').collect();
    let parsed: Vec<Option<Item>> = lines
        .iter()
        .map(|line| {
            if line.trim_matches([' ', '\t']).is_empty() {
                return None;
            }
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            Some(Item {
                indent,
                marker: marker_of(&line[indent..]),
            })
        })
        .collect();
    let items = parsed.iter().flatten();
    let clearing = items.clone().count() > 0
        && items
            .clone()
            .all(|item| item.marker.map(|(k, _)| k) == Some(kind));
    let mut out = String::with_capacity(region.len() + 8 * lines.len());
    let mut edits = Vec::with_capacity(lines.len());
    let mut count = 0;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let Some(Item { indent, marker }) = parsed[i] else {
            out.push_str(line);
            edits.push((0, 0));
            continue;
        };
        let (old_kind, old_len) = marker.map_or((None, 0), |(k, len)| (Some(k), len));
        let new = if clearing {
            String::new()
        } else {
            count += 1;
            match kind {
                ListKind::Numbered => format!("{count}. "),
                _ if old_kind == Some(kind) => line[indent..indent + old_len].to_string(),
                ListKind::Bullet => "- ".to_string(),
                ListKind::Task => "- [ ] ".to_string(),
            }
        };
        out.push_str(&line[..indent]);
        out.push_str(&new);
        out.push_str(&line[indent + old_len..]);
        edits.push((indent, new.len() as i64 - old_len as i64));
    }
    (out, edits)
}

/// Quotes or unquotes a region of whole lines, without its trailing
/// newline: `> ` after every line's indentation, a bare `>` on a blank
/// line so the quote stays one block; when every non-blank line is
/// already quoted, one mark and its space come off each line. Answers
/// the new text and, per line, the byte column of the change and its
/// delta.
pub fn quote_lines(region: &str) -> (String, Vec<(usize, i64)>) {
    let lines: Vec<&str> = region.split('\n').collect();
    let indent_of = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    let mark_len = |rest: &str| -> usize {
        if !rest.starts_with('>') {
            return 0;
        }
        if rest[1..].starts_with(' ') {
            2
        } else {
            1
        }
    };
    let blank = |line: &str| line.trim_matches([' ', '\t']).is_empty();
    let clearing = lines.iter().any(|line| !blank(line))
        && lines
            .iter()
            .filter(|line| !blank(line))
            .all(|line| mark_len(&line[indent_of(line)..]) > 0);
    let mut out = String::with_capacity(region.len() + 2 * lines.len());
    let mut edits = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let indent = indent_of(line);
        let rest = &line[indent..];
        let old = mark_len(rest);
        let new = if clearing {
            ""
        } else if old > 0 {
            &rest[..old]
        } else if rest.is_empty() {
            ">"
        } else {
            "> "
        };
        if region.is_empty() {
            edits.push((0, 0));
            continue;
        }
        out.push_str(&line[..indent]);
        out.push_str(new);
        out.push_str(&rest[old..]);
        edits.push((indent, new.len() as i64 - old as i64));
    }
    (out, edits)
}

/// Sets, changes or clears the heading level of a region of whole
/// lines, without its trailing newline: `#` times `level` and a space
/// after every non-blank line's indentation, replacing the heading
/// marker it carries; when every non-blank line already sits at that
/// level, the markers come off. A hash run without a following space
/// is text, not a marker. Answers the new text and, per line, the byte
/// column of the change and its delta.
pub fn heading_lines(region: &str, level: u8) -> (String, Vec<(usize, i64)>) {
    let lines: Vec<&str> = region.split('\n').collect();
    let indent_of = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    // The marker's byte length and its level, zero when there is none.
    let marker = |rest: &str| -> (usize, u8) {
        let hashes = rest.bytes().take_while(|b| *b == b'#').count();
        match rest.as_bytes().get(hashes) {
            _ if hashes == 0 || hashes > 6 => (0, 0),
            Some(b' ') => (hashes + 1, hashes as u8),
            None => (hashes, hashes as u8),
            Some(_) => (0, 0),
        }
    };
    let blank = |line: &str| line.trim_matches([' ', '\t']).is_empty();
    let items = lines.iter().filter(|line| !blank(line));
    let clearing = items.clone().count() > 0
        && items
            .clone()
            .all(|line| marker(&line[indent_of(line)..]).1 == level);
    let mut out = String::with_capacity(region.len() + 8 * lines.len());
    let mut edits = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if blank(line) {
            out.push_str(line);
            edits.push((0, 0));
            continue;
        }
        let indent = indent_of(line);
        let rest = &line[indent..];
        let (old, _) = marker(rest);
        let new = if clearing {
            String::new()
        } else {
            format!("{} ", "#".repeat(level as usize))
        };
        out.push_str(&line[..indent]);
        out.push_str(&new);
        out.push_str(&rest[old..]);
        edits.push((indent, new.len() as i64 - old as i64));
    }
    (out, edits)
}

/// Flips the task box of every line in a region of whole lines that
/// carries one, `[ ]` to `[x]` and `[x]` or `[X]` to `[ ]`; lines
/// without a box are untouched. Nothing moves: every delta is zero.
pub fn toggle_tasks(region: &str) -> (String, Vec<(usize, i64)>) {
    let mut out = String::with_capacity(region.len());
    let mut edits = Vec::new();
    for (i, line) in region.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = &line[indent..];
        let boxed = marker_of(rest)
            .filter(|(kind, _)| *kind == ListKind::Task)
            .and_then(|(_, len)| rest[..len].find('[').map(|at| indent + at + 1));
        match boxed {
            Some(at) => {
                let flipped = if &line[at..at + 1] == " " { "x" } else { " " };
                out.push_str(&line[..at]);
                out.push_str(flipped);
                out.push_str(&line[at + 1..]);
            }
            None => out.push_str(line),
        }
        edits.push((indent, 0));
    }
    (out, edits)
}

/// The leading bytes one outdent removes: a tab when the line starts
/// with one, else up to a step of spaces, the unit's own width or the
/// conventional four when the unit is a tab.
fn outdent_cut(line: &str, unit: &IndentUnit) -> usize {
    let b = line.as_bytes();
    if b.first() == Some(&b'\t') {
        return 1;
    }
    let step = match unit {
        IndentUnit::Spaces(n) => *n as usize,
        IndentUnit::Tab => 4,
    };
    b.iter().take_while(|c| **c == b' ').count().min(step)
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

    /// A click or an arrow move past a line's visible end lands before
    /// its trailing spaces, so the caret can sit between the dash and
    /// its space; Enter must still see an empty item there.
    #[test]
    fn a_bare_marker_ends_its_list_from_anywhere_after_the_marker() {
        assert_eq!(
            markdown_enter("- ", 1),
            unwind(2),
            "between the dash and its space"
        );
        assert_eq!(
            markdown_enter("-  ", 2),
            unwind(3),
            "two spaces, the caret between them"
        );
        assert_eq!(
            markdown_enter("-  ", 3),
            unwind(3),
            "two spaces, the caret at the end"
        );
        assert_eq!(markdown_enter("1. ", 2), unwind(3));
        assert_eq!(markdown_enter("- [ ] ", 5), unwind(6), "after the box");
        assert_eq!(markdown_enter("- [ ]  ", 6), unwind(7));
        assert_eq!(markdown_enter("> ", 1), unwind(2));
        assert_eq!(markdown_enter("  - ", 3), unwind(4), "nested");
        assert_eq!(
            markdown_enter("- ", 0),
            None,
            "before the dash is a plain split"
        );
        assert_eq!(markdown_enter("- [ ] ", 3), None, "inside the box");
        assert_eq!(
            markdown_enter("-  x", 2),
            None,
            "content after the whitespace: inside the marker"
        );
    }

    #[test]
    fn list_lines_sets_clears_and_converts() {
        use ListKind::*;
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        assert_eq!(
            list_lines("one\ntwo", Bullet),
            r("- one\n- two", vec![(0, 2), (0, 2)])
        );
        assert_eq!(
            list_lines("  one\n\n  two", Bullet),
            r("  - one\n\n  - two", vec![(2, 2), (0, 0), (2, 2)]),
            "the indent is kept and a blank line skipped"
        );
        assert_eq!(
            list_lines("- one\n- two", Bullet),
            r("one\ntwo", vec![(0, -2), (0, -2)]),
            "every line a bullet: the markers come off"
        );
        assert_eq!(
            list_lines("- one\ntwo", Bullet),
            r("- one\n- two", vec![(0, 0), (0, 2)]),
            "mixed: every line becomes a bullet, the bullet untouched"
        );
        assert_eq!(
            list_lines("- one\n- two", Numbered),
            r("1. one\n2. two", vec![(0, 1), (0, 1)])
        );
        assert_eq!(
            list_lines("3. one\n7. two", Numbered),
            r("one\ntwo", vec![(0, -3), (0, -3)])
        );
        assert_eq!(
            list_lines("one\n\n5. two", Numbered),
            r("1. one\n\n2. two", vec![(0, 3), (0, 0), (0, 0)]),
            "numbers count on over blank lines and an old number is renumbered"
        );
        assert_eq!(
            list_lines("- one\n- [x] two", Task),
            r("- [ ] one\n- [x] two", vec![(0, 4), (0, 0)]),
            "a ticked box stays ticked"
        );
        assert_eq!(
            list_lines("- [x] one\n- [ ] two", Task),
            r("one\ntwo", vec![(0, -6), (0, -6)])
        );
        assert_eq!(
            list_lines("- [x] one", Bullet),
            r("- one", vec![(0, -4)]),
            "the box goes with the kind"
        );
        assert_eq!(list_lines("1) one", Bullet), r("- one", vec![(0, -1)]));
        assert_eq!(
            list_lines("", Bullet),
            r("", vec![(0, 0)]),
            "nothing to list"
        );
    }

    #[test]
    fn quote_lines_quotes_and_unquotes() {
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        assert_eq!(
            quote_lines("one\n\ntwo"),
            r("> one\n>\n> two", vec![(0, 2), (0, 1), (0, 2)]),
            "a blank line inside gets a bare > so the quote stays one block"
        );
        assert_eq!(
            quote_lines("  one"),
            r("  > one", vec![(2, 2)]),
            "the indent is kept"
        );
        assert_eq!(
            quote_lines("> one\n>\n> two"),
            r("one\n\ntwo", vec![(0, -2), (0, -1), (0, -2)]),
            "every line quoted: the marks come off"
        );
        assert_eq!(
            quote_lines(">one"),
            r("one", vec![(0, -1)]),
            "a mark without its space"
        );
        assert_eq!(
            quote_lines("> one\ntwo"),
            r("> one\n> two", vec![(0, 0), (0, 2)]),
            "mixed: every line quoted, the quoted one untouched"
        );
        assert_eq!(quote_lines(""), r("", vec![(0, 0)]));
    }

    #[test]
    fn heading_lines_sets_changes_and_clears_the_level() {
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        assert_eq!(heading_lines("one", 2), r("## one", vec![(0, 3)]));
        assert_eq!(
            heading_lines("## one", 3),
            r("### one", vec![(0, 1)]),
            "another level replaces"
        );
        assert_eq!(
            heading_lines("## one", 2),
            r("one", vec![(0, -3)]),
            "the same level clears"
        );
        assert_eq!(
            heading_lines("  one", 1),
            r("  # one", vec![(2, 2)]),
            "the indent is kept"
        );
        assert_eq!(
            heading_lines("one\n\n# two", 1),
            r("# one\n\n# two", vec![(0, 2), (0, 0), (0, 0)]),
            "mixed: every line set, the blank skipped, the heading untouched"
        );
        assert_eq!(
            heading_lines("#tag", 2),
            r("## #tag", vec![(0, 3)]),
            "a hash without a space is text"
        );
        assert_eq!(
            heading_lines("##", 2),
            r("", vec![(0, -2)]),
            "a bare marker clears too"
        );
        assert_eq!(heading_lines("", 1), r("", vec![(0, 0)]));
    }

    #[test]
    fn toggle_tasks_flips_each_box_and_leaves_the_rest() {
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        assert_eq!(
            toggle_tasks("- [ ] a\n- [x] b\nplain\n  1. [X] c\n"),
            r(
                "- [x] a\n- [ ] b\nplain\n  1. [ ] c\n",
                vec![(0, 0), (0, 0), (0, 0), (2, 0), (0, 0)]
            )
        );
        assert_eq!(
            toggle_tasks("- a"),
            r("- a", vec![(0, 0)]),
            "no box, no change"
        );
        assert_eq!(toggle_tasks(""), r("", vec![(0, 0)]));
    }

    #[test]
    fn tab_nests_at_or_before_the_items_content() {
        assert!(tab_nests("- item", 0), "the line start nests");
        assert!(tab_nests("- item", 1), "inside the marker nests");
        assert!(tab_nests("- item", 2), "right after the marker nests");
        assert!(!tab_nests("- item", 3), "inside the content inserts");
        assert!(tab_nests("  - item", 4));
        assert!(tab_nests("1. one", 3));
        assert!(tab_nests("- [ ] task", 6));
        assert!(!tab_nests("- [ ] task", 7));
        assert!(tab_nests("> - quoted item", 4), "a quoted item still nests");
        assert!(!tab_nests("> quoted", 2), "a bare quote never nests");
        assert!(!tab_nests("plain", 0));
        assert!(!tab_nests("    code", 4));
    }

    #[test]
    fn the_unit_follows_the_dominant_indentation() {
        assert_eq!(
            indent_unit("all:\n\tcc -o all main.c\n\tstrip all\n"),
            IndentUnit::Tab,
            "tab-led lines dominate a Makefile"
        );
        assert_eq!(
            indent_unit("- a\n  - b\n  - c\n"),
            IndentUnit::Spaces(2),
            "two-space nesting reads as a two-space step"
        );
        assert_eq!(
            indent_unit("fn main() {\n    if x {\n        y();\n    }\n}\n"),
            IndentUnit::Spaces(4),
            "four-space blocks read as a four-space step"
        );
        assert_eq!(
            indent_unit("\ta\n\tb\n  c\n"),
            IndentUnit::Tab,
            "tabs outnumber spaces"
        );
        assert_eq!(
            indent_unit("plain\nlines\n"),
            IndentUnit::Tab,
            "no evidence answers a tab"
        );
        assert_eq!(indent_unit(""), IndentUnit::Tab);
    }

    #[test]
    fn reindent_moves_every_line_and_skips_empty_on_indent() {
        let (text, deltas) = reindent("one\n\n  three", &IndentUnit::Spaces(2), false);
        assert_eq!(text, "  one\n\n    three");
        assert_eq!(deltas, vec![2, 0, 2]);
        let (text, deltas) = reindent("a\nb", &IndentUnit::Tab, false);
        assert_eq!(text, "\ta\n\tb");
        assert_eq!(deltas, vec![1, 1]);
    }

    #[test]
    fn outdent_trims_a_tab_a_step_or_a_short_run() {
        let (text, deltas) = reindent("\tone", &IndentUnit::Tab, true);
        assert_eq!(text, "one", "one leading tab leaves");
        assert_eq!(deltas, vec![-1]);
        let (text, deltas) = reindent("    one\n  two\n one\nzero", &IndentUnit::Spaces(2), true);
        assert_eq!(
            text, "  one\ntwo\none\nzero",
            "a step of spaces leaves, a short run leaves whole, bare stays"
        );
        assert_eq!(deltas, vec![-2, -2, -1, 0]);
        let (text, _) = reindent("        deep", &IndentUnit::Tab, true);
        assert_eq!(
            text, "    deep",
            "a tab unit outdents spaces by the conventional four"
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
