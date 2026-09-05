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

/// The pair a typed character wraps a selection in: brackets and the
/// two quotes in every file, the emphasis and code marks in markdown
/// alone, where a star in a source file is an operator to type over
/// the selection. None for anything else.
pub fn wrap_pair(typed: &str, markdown: bool) -> Option<(&'static str, &'static str)> {
    match typed {
        "(" => Some(("(", ")")),
        "[" => Some(("[", "]")),
        "{" => Some(("{", "}")),
        "\"" => Some(("\"", "\"")),
        "'" => Some(("'", "'")),
        "*" if markdown => Some(("*", "*")),
        "_" if markdown => Some(("_", "_")),
        "`" if markdown => Some(("`", "`")),
        _ => None,
    }
}

/// One splice that wraps or unwraps a mark: the bytes to replace, the
/// text that goes there, the inner text's range afterwards, and where
/// the caret stands.
#[derive(Debug, PartialEq, Eq)]
pub struct MarkEdit {
    pub replace: std::ops::Range<usize>,
    pub text: String,
    pub inner: std::ops::Range<usize>,
    pub caret: usize,
}

/// Toggles an emphasis or code mark around the selection, or around
/// the word under the caret when nothing is selected. A selection that
/// carries the mark at both ends, or sits just inside a pair of them,
/// loses the pair; anything else gains one. A single star just outside
/// is not a pair when it belongs to a double, so italic inside bold
/// adds its own star. With no selection and no word, an empty pair
/// opens with the caret inside; with a word, the caret keeps its
/// letter. The inner range is what stays selected.
pub fn toggle_mark(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    caret: usize,
    mark: &str,
) -> MarkEdit {
    let m = mark.len();
    let word = |at: usize| -> Option<std::ops::Range<usize>> {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start = source[..at]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(at, |(i, _)| i);
        let end = source[at..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(source.len(), |(i, _)| at + i);
        (end > start).then_some(start..end)
    };
    let (range, pinned) = match selection.filter(|r| !r.is_empty()) {
        Some(r) => (r, None),
        None => match word(caret) {
            Some(r) => (r, Some(caret)),
            None => {
                return MarkEdit {
                    replace: caret..caret,
                    text: format!("{mark}{mark}"),
                    inner: caret + m..caret + m,
                    caret: caret + m,
                };
            }
        },
    };
    let text = &source[range.clone()];
    // The pair around the range, when both sides carry the mark and,
    // for a single star, the star is not half of a double.
    let outside = range.start >= m
        && source[..range.start].ends_with(mark)
        && source[range.end..].starts_with(mark)
        && !(mark == "*"
            && (source[..range.start - m].ends_with('*')
                || source[range.end + m..].starts_with('*')));
    let inside = text.len() >= 2 * m
        && text.starts_with(mark)
        && text.ends_with(mark)
        && !(mark == "*" && (text.starts_with("**") || text.ends_with("**")));
    let ride = |p: usize, from: usize, delta: i64| -> usize {
        if p < from {
            p
        } else {
            (p as i64 + delta) as usize
        }
    };
    if outside {
        let replace = range.start - m..range.end + m;
        let inner = replace.start..replace.start + text.len();
        let caret = pinned.map_or(inner.end, |p| ride(p, range.start, -(m as i64)));
        return MarkEdit {
            replace,
            text: text.to_string(),
            inner,
            caret,
        };
    }
    if inside {
        let stripped = &text[m..text.len() - m];
        let inner = range.start..range.start + stripped.len();
        let caret = pinned.map_or(inner.end, |p| {
            ride(p, range.start, -(m as i64)).min(inner.end)
        });
        return MarkEdit {
            replace: range,
            text: stripped.to_string(),
            inner,
            caret,
        };
    }
    let inner = range.start + m..range.end + m;
    let caret = pinned.map_or(inner.end, |p| ride(p, range.start, m as i64));
    MarkEdit {
        replace: range,
        text: format!("{mark}{text}{mark}"),
        inner,
        caret,
    }
}

/// Ctrl+K: the selection becomes a link's text with the caret in the
/// empty parentheses; a selection that is itself an address becomes
/// the target with the caret in the empty brackets; with no selection
/// an empty link opens with the caret in the brackets.
pub fn link_edit(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    caret: usize,
) -> MarkEdit {
    let (replace, text, caret) = match selection.filter(|r| !r.is_empty()) {
        Some(r) => {
            let inner = &source[r.clone()];
            if is_url(inner) {
                (r.clone(), format!("[]({})", inner.trim()), r.start + 1)
            } else {
                let caret = r.start + inner.len() + 3;
                (r, format!("[{inner}]()"), caret)
            }
        }
        None => (caret..caret, "[]()".to_string(), caret + 1),
    };
    MarkEdit {
        replace,
        text,
        inner: caret..caret,
        caret,
    }
}

/// A pasted address over a selection: the selection becomes the link's
/// text and the address its target, the caret after the link. None
/// when there is no selection or the clipboard is not an address.
pub fn link_paste(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    clipboard: &str,
) -> Option<MarkEdit> {
    let r = selection.filter(|r| !r.is_empty())?;
    if !is_url(clipboard) {
        return None;
    }
    let text = format!("[{}]({})", &source[r.clone()], clipboard.trim());
    let caret = r.start + text.len();
    Some(MarkEdit {
        replace: r,
        text,
        inner: caret..caret,
        caret,
    })
}

/// A web or mail address on its own: a scheme, then at least one byte
/// and no whitespace, once the clipboard's padding is trimmed.
pub fn is_url(text: &str) -> bool {
    let text = text.trim();
    let rest = ["https://", "http://", "ftp://", "mailto:"]
        .iter()
        .find_map(|scheme| text.strip_prefix(scheme));
    rest.is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace))
}

/// Moves a block of whole lines, `block` being its bytes without the
/// trailing newline, one line up or down: the bytes to replace, the
/// text that goes there, and the delta every position inside the block
/// moves by. None at the file's edges; the empty line after a final
/// newline is not a line to swap with.
pub fn move_lines(
    source: &str,
    block: std::ops::Range<usize>,
    up: bool,
) -> Option<(std::ops::Range<usize>, String, i64)> {
    let text = &source[block.clone()];
    if up {
        if block.start == 0 {
            return None;
        }
        let above_start = source[..block.start - 1].rfind('\n').map_or(0, |i| i + 1);
        let above = &source[above_start..block.start - 1];
        Some((
            above_start..block.end,
            format!("{text}\n{above}"),
            -(above.len() as i64 + 1),
        ))
    } else {
        if block.end + 1 >= source.len() {
            return None;
        }
        let below_end = source[block.end + 1..]
            .find('\n')
            .map_or(source.len(), |i| block.end + 1 + i);
        let below = &source[block.end + 1..below_end];
        Some((
            block.start..below_end,
            format!("{below}\n{text}"),
            below.len() as i64 + 1,
        ))
    }
}

/// Duplicates a block of whole lines below itself: the insertion point
/// (the block's end), the text to insert (a newline and the block), and
/// the delta that carries the caret and the selection onto the copy.
pub fn duplicate_lines(
    source: &str,
    block: std::ops::Range<usize>,
) -> (std::ops::Range<usize>, String, i64) {
    let text = format!("\n{}", &source[block.clone()]);
    let delta = text.len() as i64;
    (block.end..block.end, text, delta)
}

/// Deletes a block of whole lines with the newline that follows it, or
/// the one before it when it is the file's last line. Answers the bytes
/// to remove and the landing: the start of the line that follows, in
/// the new coordinates, or the new end of the file when none follows.
pub fn delete_lines(
    source: &str,
    block: std::ops::Range<usize>,
) -> (std::ops::Range<usize>, usize) {
    if block.end < source.len() {
        (block.start..block.end + 1, block.start)
    } else {
        let start = block.start.saturating_sub(1);
        (start..block.end, start)
    }
}

/// How a language writes a comment: a token that comments the rest of
/// the line, or a pair that wraps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentStyle {
    Line(&'static str),
    Block(&'static str, &'static str),
}

/// The comment style of a detected language, by the token the loader
/// names it with; markdown takes the HTML comment; None where the
/// language has no comment (JSON, diffs) or the file is plain text.
pub fn comment_style(language: Option<&str>, markdown: bool) -> Option<CommentStyle> {
    use CommentStyle::*;
    if markdown {
        return Some(Block("<!--", "-->"));
    }
    Some(match language? {
        "actionscript" | "c" | "cpp" | "csharp" | "d" | "go" | "graphql" | "graphviz"
        | "groovy" | "java" | "javascript" | "kotlin" | "objective-c" | "objective-c++"
        | "pascal" | "php" | "protobuf" | "rust" | "scala" | "swift" | "tsx" | "typescript"
        | "zig" => Line("//"),
        "bash" | "dockerfile" | "makefile" | "perl" | "properties" | "python" | "r" | "ruby"
        | "tcl" | "terraform" | "toml" | "yaml" => Line("#"),
        "applescript" | "haskell" | "lua" | "sql" => Line("--"),
        "clojure" | "ini" | "lisp" => Line(";"),
        "bibtex" | "erlang" | "latex" => Line("%"),
        "batch" => Line("REM"),
        "html" | "jsp" | "xml" => Block("<!--", "-->"),
        "css" => Block("/*", "*/"),
        "ocaml" => Block("(*", "*)"),
        _ => return None,
    })
}

/// Comments or uncomments a region of whole lines, without its
/// trailing newline. A line token goes at the block's shallowest indent
/// on every non-blank line, a space after it; a block pair wraps each
/// non-blank line. When every non-blank line is already commented, the
/// marks come off (the token's one following space with it). Answers
/// the new text and, per line, the byte column of the change and the
/// delta positions on that line move by; a closing mark appended past
/// them is the caller's to count.
pub fn comment_lines(region: &str, style: CommentStyle) -> (String, Vec<(usize, i64)>) {
    let lines: Vec<&str> = region.split('\n').collect();
    let indent_of = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    let blank = |line: &str| line.trim_matches([' ', '\t']).is_empty();
    // The mark a line carries: the bytes it takes at the start and,
    // for a pair, at the end.
    let marked = |line: &str| -> Option<(usize, usize)> {
        let rest = &line[indent_of(line)..];
        match style {
            CommentStyle::Line(token) => {
                let rest = rest.strip_prefix(token)?;
                Some((token.len() + usize::from(rest.starts_with(' ')), 0))
            }
            CommentStyle::Block(open, close) => {
                let rest = rest.strip_prefix(open)?;
                let trimmed = rest.trim_end_matches([' ', '\t']);
                let body = trimmed.strip_suffix(close)?;
                // The space after the opening and the one before the
                // closing are the same byte when the body is one space.
                let lead = usize::from(rest.starts_with(' '));
                let inner = &body[lead.min(body.len())..];
                let head = open.len() + lead;
                let tail =
                    close.len() + usize::from(inner.ends_with(' ')) + (rest.len() - trimmed.len());
                Some((head, tail))
            }
        }
    };
    let items = lines.iter().filter(|line| !blank(line));
    let clearing = items.clone().count() > 0 && items.clone().all(|line| marked(line).is_some());
    let column = items.clone().map(|line| indent_of(line)).min().unwrap_or(0);
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
        if clearing {
            let (head, tail) = marked(line).expect("every line is marked");
            out.push_str(&line[..indent]);
            out.push_str(&line[indent + head..line.len() - tail]);
            edits.push((indent, -(head as i64)));
            continue;
        }
        if marked(line).is_some() {
            out.push_str(line);
            edits.push((indent, 0));
            continue;
        }
        match style {
            CommentStyle::Line(token) => {
                out.push_str(&line[..column]);
                out.push_str(token);
                out.push(' ');
                out.push_str(&line[column..]);
                edits.push((column, token.len() as i64 + 1));
            }
            CommentStyle::Block(open, close) => {
                out.push_str(&line[..indent]);
                out.push_str(open);
                out.push(' ');
                out.push_str(&line[indent..]);
                out.push(' ');
                out.push_str(close);
                edits.push((indent, open.len() as i64 + 1));
            }
        }
    }
    (out, edits)
}

/// Carries a position through a line rewrite. `old` and `new` are the
/// region before and after, the same line count; `edits` names per line
/// the byte column where the line changed and the delta positions past
/// it move by; `p` is relative to the region's start. A position before
/// the column or on it stays, so an inserted marker sits inside the
/// selection; one inside removed bytes clamps to the column, so an
/// outdent never yanks the caret to the line start; one past the
/// column moves by the delta, which leaves it before a closing mark
/// appended after the text; one past the line's end, the start of the
/// line after the region where a selection ends, moves by the line's
/// whole growth. Every line above carries its whole growth down.
pub fn ride_rewrite(old: &str, new: &str, edits: &[(usize, i64)], p: usize) -> usize {
    let mut at = 0;
    let mut shift: i64 = 0;
    for ((old_line, new_line), &(col, delta)) in old.split('\n').zip(new.split('\n')).zip(edits) {
        let end = at + old_line.len();
        if p <= end {
            let col = at + col;
            let moved = if p <= col {
                p as i64
            } else if delta < 0 && p < col + (-delta) as usize {
                col as i64
            } else {
                p as i64 + delta
            };
            return (moved + shift) as usize;
        }
        at = end + 1;
        shift += new_line.len() as i64 - old_line.len() as i64;
    }
    (p as i64 + shift) as usize
}

/// The byte offset `column` bytes into `line`, brought back to the
/// character boundary before it when it falls inside a character, or
/// to the line's end when the line is shorter.
pub fn seat_at_column(line: &str, column: usize) -> usize {
    let mut at = column.min(line.len());
    while !line.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// After Enter continues a numbered item at `at` with `continuation`
/// (a newline, the indent and the next number), the items below that
/// belong to the same list, same indent and same delimiter with blank
/// lines allowed between, count on from it. Answers the end of the
/// last renumbered item and the text for `at..end` minus the
/// continuation: the rest of the split line, then the renumbered
/// items. None when no item below needs a new number.
pub fn renumber_tail(source: &str, at: usize, continuation: &str) -> Option<(usize, String)> {
    let cont = continuation.strip_prefix('\n')?;
    let indent = cont.len() - cont.trim_start_matches([' ', '\t']).len();
    let digits = cont[indent..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits == 0 {
        return None;
    }
    let delim = *cont.as_bytes().get(indent + digits)?;
    let mut next: u64 = cont[indent..indent + digits].parse().ok()?;
    let line_end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    let mut out = source[at..line_end].to_string();
    let mut end = line_end;
    let mut pos = line_end;
    let mut pending = String::new();
    while pos < source.len() {
        let start = pos + 1;
        let stop = source[start..]
            .find('\n')
            .map_or(source.len(), |i| start + i);
        let line = &source[start..stop];
        if line.trim_matches([' ', '\t']).is_empty() {
            pending.push('\n');
            pending.push_str(line);
            pos = stop;
            continue;
        }
        let line_indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = &line[line_indent..];
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        let item = line_indent == indent
            && n > 0
            && rest.as_bytes().get(n) == Some(&delim)
            && matches!(rest.as_bytes().get(n + 1), Some(b' ' | b'\t'));
        if !item {
            break;
        }
        next += 1;
        out.push_str(&pending);
        pending.clear();
        out.push('\n');
        out.push_str(&line[..line_indent]);
        out.push_str(&next.to_string());
        out.push_str(&rest[n..]);
        end = stop;
        pos = stop;
    }
    (end > line_end && out != source[at..end]).then_some((end, out))
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
    fn wrap_pairs_are_brackets_everywhere_and_marks_in_markdown() {
        assert_eq!(wrap_pair("(", false), Some(("(", ")")));
        assert_eq!(wrap_pair("[", false), Some(("[", "]")));
        assert_eq!(wrap_pair("{", true), Some(("{", "}")));
        assert_eq!(wrap_pair("\"", false), Some(("\"", "\"")));
        assert_eq!(wrap_pair("'", false), Some(("'", "'")));
        assert_eq!(wrap_pair("*", true), Some(("*", "*")));
        assert_eq!(wrap_pair("_", true), Some(("_", "_")));
        assert_eq!(wrap_pair("`", true), Some(("`", "`")));
        assert_eq!(wrap_pair("*", false), None, "a star in code is an operator");
        assert_eq!(wrap_pair("`", false), None);
        assert_eq!(wrap_pair(")", true), None, "a closing bracket types");
        assert_eq!(wrap_pair("a", true), None);
    }

    #[test]
    fn toggle_mark_wraps_and_unwraps_a_selection() {
        let e = |replace: std::ops::Range<usize>,
                 text: &str,
                 inner: std::ops::Range<usize>,
                 caret: usize| MarkEdit {
            replace,
            text: text.to_string(),
            inner,
            caret,
        };
        assert_eq!(
            toggle_mark("a word b", Some(2..6), 6, "**"),
            e(2..6, "**word**", 4..8, 8)
        );
        assert_eq!(
            toggle_mark("a **word** b", Some(2..10), 10, "**"),
            e(2..10, "word", 2..6, 6),
            "the marks inside the selection come off"
        );
        assert_eq!(
            toggle_mark("a **word** b", Some(4..8), 8, "**"),
            e(2..10, "word", 2..6, 6),
            "the marks just outside the selection come off"
        );
        assert_eq!(
            toggle_mark("a **word** b", Some(4..8), 8, "*"),
            e(4..8, "*word*", 5..9, 9),
            "italic inside bold adds a third star, the double is not a single"
        );
        assert_eq!(
            toggle_mark("a `x` b", Some(3..4), 4, "`"),
            e(2..5, "x", 2..3, 3)
        );
        assert_eq!(
            toggle_mark("one\ntwo", Some(0..7), 7, "**"),
            e(0..7, "**one\ntwo**", 2..9, 9),
            "a selection over lines wraps whole"
        );
    }

    #[test]
    fn toggle_mark_takes_the_word_under_the_caret() {
        let e = |replace: std::ops::Range<usize>,
                 text: &str,
                 inner: std::ops::Range<usize>,
                 caret: usize| MarkEdit {
            replace,
            text: text.to_string(),
            inner,
            caret,
        };
        assert_eq!(
            toggle_mark("a word b", None, 4, "**"),
            e(2..6, "**word**", 4..8, 6),
            "the caret rides its letter"
        );
        assert_eq!(
            toggle_mark("a word b", None, 6, "**"),
            e(2..6, "**word**", 4..8, 8),
            "at the word's end"
        );
        assert_eq!(
            toggle_mark("a word b", None, 2, "*"),
            e(2..6, "*word*", 3..7, 3),
            "at the word's start"
        );
        assert_eq!(
            toggle_mark("a **word** b", None, 5, "**"),
            e(2..10, "word", 2..6, 3),
            "unwrapped, the caret rides back"
        );
        assert_eq!(
            toggle_mark("a  b", None, 2, "**"),
            e(2..2, "****", 4..4, 4),
            "no word: an empty pair, the caret inside"
        );
        assert_eq!(toggle_mark("", None, 0, "`"), e(0..0, "``", 1..1, 1));
        assert_eq!(
            toggle_mark("état", None, 2, "_"),
            e(0..5, "_état_", 1..6, 3),
            "a word is any run of letters"
        );
    }

    #[test]
    fn a_link_wraps_the_selection_or_opens_empty() {
        let e = |replace: std::ops::Range<usize>, text: &str, caret: usize| MarkEdit {
            replace,
            text: text.to_string(),
            inner: caret..caret,
            caret,
        };
        assert_eq!(
            link_edit("a word b", Some(2..6), 6),
            e(2..6, "[word]()", 9),
            "the caret in the parentheses"
        );
        assert_eq!(
            link_edit("ab", None, 1),
            e(1..1, "[]()", 2),
            "the caret in the brackets"
        );
        assert_eq!(
            link_edit("see https://x.y now", Some(4..15), 15),
            e(4..15, "[](https://x.y)", 5),
            "a selected address becomes the target, the caret in the brackets"
        );
    }

    #[test]
    fn a_pasted_address_over_a_selection_makes_a_link() {
        assert!(is_url("https://x.y/z?q=1"));
        assert!(is_url("http://a"));
        assert!(is_url("mailto:a@b.c"));
        assert!(is_url("  https://x.y\n"), "clipboard padding is trimmed");
        assert!(!is_url("hello"));
        assert!(!is_url("https://a b"));
        assert!(!is_url("https://"));
        assert_eq!(
            link_paste("a word b", Some(2..6), "https://x.y\n"),
            Some(MarkEdit {
                replace: 2..6,
                text: "[word](https://x.y)".to_string(),
                inner: 21..21,
                caret: 21,
            })
        );
        assert_eq!(link_paste("a word b", Some(2..6), "plain"), None);
        assert_eq!(
            link_paste("a word b", None, "https://x.y"),
            None,
            "no selection: a plain paste"
        );
    }

    #[test]
    fn move_lines_swaps_the_block_with_its_neighbor() {
        let m = |replace: std::ops::Range<usize>, text: &str, delta: i64| {
            Some((replace, text.to_string(), delta))
        };
        assert_eq!(move_lines("a\nb\nc", 2..3, true), m(0..3, "b\na", -2));
        assert_eq!(move_lines("a\nb\nc", 2..3, false), m(2..5, "c\nb", 2));
        assert_eq!(
            move_lines("a\nb\nc", 0..1, true),
            None,
            "nothing above the first line"
        );
        assert_eq!(
            move_lines("a\nb\nc", 4..5, false),
            None,
            "nothing below the last"
        );
        assert_eq!(
            move_lines("a\nb\n", 2..3, false),
            None,
            "the file's final newline is not a line to swap with"
        );
        assert_eq!(
            move_lines("a\nb\nc\nd", 2..5, true),
            m(0..5, "b\nc\na", -2),
            "a block of lines"
        );
        assert_eq!(move_lines("a\nb\nc\nd", 2..5, false), m(2..7, "d\nb\nc", 2));
        assert_eq!(
            move_lines("a\n\nc", 2..2, true),
            m(0..2, "\na", -2),
            "an empty line moves too"
        );
        assert_eq!(
            move_lines("a\nbb\nc", 0..1, false),
            m(0..4, "bb\na", 3),
            "the delta is the neighbor's length plus its newline"
        );
    }

    #[test]
    fn duplicate_lines_copies_the_block_below_it() {
        assert_eq!(
            duplicate_lines("a\nb\nc", 2..3),
            (3..3, "\nb".to_string(), 2)
        );
        assert_eq!(
            duplicate_lines("a\nb", 2..3),
            (3..3, "\nb".to_string(), 2),
            "the last line too"
        );
        assert_eq!(
            duplicate_lines("a\nb\nc\nd", 2..5),
            (5..5, "\nb\nc".to_string(), 4),
            "a block"
        );
        assert_eq!(
            duplicate_lines("a\n\nb", 2..2),
            (2..2, "\n".to_string(), 1),
            "an empty line"
        );
    }

    #[test]
    fn delete_lines_removes_the_block_and_names_the_landing() {
        assert_eq!(
            delete_lines("a\nb\nc", 2..3),
            (2..4, 2),
            "the line and its newline; the next line lands at the same start"
        );
        assert_eq!(
            delete_lines("a\nb\nc", 4..5),
            (3..5, 3),
            "the last line takes the newline before it; the landing is the new end"
        );
        assert_eq!(delete_lines("a\nb\nc\nd", 2..5), (2..6, 2), "a block");
        assert_eq!(delete_lines("a", 0..1), (0..1, 0), "the only line");
        assert_eq!(
            delete_lines("a\nb\n", 2..3),
            (2..4, 2),
            "a last line followed by the final newline keeps that newline"
        );
    }

    #[test]
    fn comment_styles_follow_the_language() {
        use CommentStyle::*;
        assert_eq!(comment_style(Some("rust"), false), Some(Line("//")));
        assert_eq!(comment_style(Some("python"), false), Some(Line("#")));
        assert_eq!(comment_style(Some("sql"), false), Some(Line("--")));
        assert_eq!(comment_style(Some("css"), false), Some(Block("/*", "*/")));
        assert_eq!(
            comment_style(Some("html"), false),
            Some(Block("<!--", "-->"))
        );
        assert_eq!(
            comment_style(Some("json"), false),
            None,
            "no comments in JSON"
        );
        assert_eq!(
            comment_style(None, true),
            Some(Block("<!--", "-->")),
            "markdown"
        );
        assert_eq!(comment_style(None, false), None, "plain text");
    }

    #[test]
    fn comment_lines_toggles_line_and_block_comments() {
        use CommentStyle::*;
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        assert_eq!(
            comment_lines("a\n  b", Line("//")),
            r("// a\n//   b", vec![(0, 3), (0, 3)]),
            "the token sits at the block's shallowest indent"
        );
        assert_eq!(
            comment_lines("  a\n  b", Line("#")),
            r("  # a\n  # b", vec![(2, 2), (2, 2)])
        );
        assert_eq!(
            comment_lines("// a\n//b\n\n  // c", Line("//")),
            r("a\nb\n\n  c", vec![(0, -3), (0, -2), (0, 0), (2, -3)]),
            "every line commented: the token and one space come off, a blank skipped"
        );
        assert_eq!(
            comment_lines("// a\nb", Line("//")),
            r("// a\n// b", vec![(0, 0), (0, 3)]),
            "mixed: every line commented, the commented one untouched"
        );
        assert_eq!(
            comment_lines("a\n\nb", Block("<!--", "-->")),
            r("<!-- a -->\n\n<!-- b -->", vec![(0, 5), (0, 0), (0, 5)]),
            "a block style wraps each line; the delta is the opening's"
        );
        assert_eq!(
            comment_lines("<!-- a -->\n  <!-- b -->", Block("<!--", "-->")),
            r("a\n  b", vec![(0, -5), (2, -5)])
        );
        assert_eq!(comment_lines("", Line("#")), r("", vec![(0, 0)]));
    }

    /// A pair around one space, or around nothing, is the shortest
    /// block comment: uncommenting it leaves the line empty. The one
    /// space is the opening's and the closing's at once.
    #[test]
    fn the_shortest_block_comments_uncomment_to_nothing() {
        use CommentStyle::*;
        let r = |text: &str, edits: Vec<(usize, i64)>| (text.to_string(), edits);
        let html = Block("<!--", "-->");
        assert_eq!(comment_lines("<!-- -->", html), r("", vec![(0, -5)]));
        assert_eq!(comment_lines("<!---->", html), r("", vec![(0, -4)]));
        assert_eq!(
            comment_lines("<!--  -->", html),
            r("", vec![(0, -5)]),
            "two spaces: one each side"
        );
        assert_eq!(
            comment_lines("  /*  */", Block("/*", "*/")),
            r("  ", vec![(2, -3)])
        );
    }

    #[test]
    fn a_position_rides_a_rewrite_by_its_line() {
        // A bullet on two lines: the marker sits inside the selection.
        let (old, new, edits) = ("a\nb", "- a\n- b", vec![(0, 2), (0, 2)]);
        let ride = |p| ride_rewrite(old, new, &edits, p);
        assert_eq!(ride(0), 0, "hugging the column stays");
        assert_eq!(ride(1), 3);
        assert_eq!(ride(2), 4, "the second line's start");
        assert_eq!(ride(3), 7);
        assert_eq!(ride(4), 8, "the line after the region");
        // An outdent: a position inside the removed bytes clamps.
        let (old, new, edits) = ("    a\n  b", "  a\nb", vec![(0, -2), (0, -2)]);
        let ride = |p| ride_rewrite(old, new, &edits, p);
        assert_eq!(ride(1), 0);
        assert_eq!(ride(5), 3);
        assert_eq!(ride(7), 4, "inside the second line's removed bytes");
        assert_eq!(ride(9), 5);
        // A block comment: the closing mark counts for the line after.
        let (old, new, edits) = ("abc", "<!-- abc -->", vec![(0, 5)]);
        let ride = |p| ride_rewrite(old, new, &edits, p);
        assert_eq!(ride(3), 8, "the text's end stays before the closing mark");
        assert_eq!(ride(4), 13, "the next line's start rides the whole growth");
    }

    #[test]
    fn a_column_seats_on_a_character_boundary() {
        assert_eq!(seat_at_column("héllo", 1), 1);
        assert_eq!(
            seat_at_column("héllo", 2),
            1,
            "inside the accent: before it"
        );
        assert_eq!(seat_at_column("héllo", 3), 3);
        assert_eq!(seat_at_column("ab", 5), 2, "a shorter line: its end");
        assert_eq!(seat_at_column("", 3), 0);
    }

    #[test]
    fn the_items_below_a_new_numbered_item_count_on() {
        let s = |end: usize, tail: &str| Some((end, tail.to_string()));
        assert_eq!(
            renumber_tail("1. a\n2. b\n3. c", 4, "\n2. "),
            s(14, "\n3. b\n4. c")
        );
        assert_eq!(
            renumber_tail("1. a b\n2. c", 4, "\n2. "),
            s(11, " b\n3. c"),
            "a split mid-item carries the rest of the line"
        );
        assert_eq!(
            renumber_tail("1. a\ntext", 4, "\n2. "),
            None,
            "nothing numbered below"
        );
        assert_eq!(
            renumber_tail("1. a\n\n2. b", 4, "\n2. "),
            s(10, "\n\n3. b"),
            "a loose list"
        );
        assert_eq!(
            renumber_tail("1. a\n   2. b", 4, "\n2. "),
            None,
            "a nested list is another list"
        );
        assert_eq!(
            renumber_tail("  1. a\n  2. b", 6, "\n  2. "),
            s(13, "\n  3. b"),
            "the indent must match"
        );
        assert_eq!(renumber_tail("1) a\n2) b", 4, "\n2) "), s(9, "\n3) b"));
        assert_eq!(
            renumber_tail("1. a\n2) b", 4, "\n2. "),
            None,
            "another delimiter is another list"
        );
        assert_eq!(
            renumber_tail("- a\n- b", 3, "\n- "),
            None,
            "bullets have no numbers"
        );
        assert_eq!(
            renumber_tail("1. a\n2. b\n\ntext\n3. c", 4, "\n2. "),
            s(9, "\n3. b"),
            "the list ends at the first line that is not an item"
        );
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
