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

/// The bytes Shift+Enter inserts in a markdown source with the caret
/// `col` bytes into `line`: markdown's hard break, two spaces at the
/// line's end, less the spaces already standing before the caret, then
/// the new line opened where the text continues. A paragraph carries
/// its indentation, a quote its marks, and a list item the columns of
/// its marker and task box as spaces, so the break stays inside the
/// item and opens no new one. None when no text stands between the
/// line's prefix and the caret: a break there would end nothing, and
/// two spaces alone on a line are a blank line.
pub fn hard_break(line: &str, col: usize) -> Option<String> {
    let col = col.min(line.len());
    let (_, quote) = marker_seat(line);
    let marker = list_marker(&line[quote..]).map_or(0, |(len, _)| len);
    let prefix = quote + marker;
    if col < prefix || line[prefix..col].trim().is_empty() {
        return None;
    }
    let head = &line[..col];
    let standing = head.len() - head.trim_end_matches(' ').len();
    let mut text = " ".repeat(2usize.saturating_sub(standing));
    text.push('\n');
    text.push_str(&line[..quote]);
    text.push_str(&" ".repeat(marker));
    Some(text)
}

/// The caret's line as Ctrl+C and Ctrl+X take it when nothing is
/// selected.
#[derive(Debug, PartialEq, Eq)]
pub struct LineTake {
    /// The bytes a cut removes.
    pub cut: std::ops::Range<usize>,
    /// The clipboard's text: the line and one newline, so a paste at a
    /// line's start puts a whole line back.
    pub text: String,
    /// Where a cut leaves the caret.
    pub caret: usize,
}

/// The line holding `caret`. A cut removes the line through its own
/// ending, and the caret stands where the next line moved up to. The
/// last line has no ending of its own: it takes the one before it, so
/// no empty line is left behind, and the caret goes to the start of
/// the line that is now last. None in an empty file.
pub fn line_take(source: &str, caret: usize) -> Option<LineTake> {
    if source.is_empty() {
        return None;
    }
    let caret = caret.min(source.len());
    let start = source[..caret].rfind('\n').map_or(0, |i| i + 1);
    let end = source[caret..]
        .find('\n')
        .map_or(source.len(), |i| caret + i);
    let text = format!("{}\n", &source[start..end]);
    let (cut, caret) = if end < source.len() {
        (start..end + 1, start)
    } else if start > 0 {
        let above = source[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        (start - 1..end, above)
    } else {
        (start..end, 0)
    };
    Some(LineTake { cut, text, caret })
}

/// Where a line taken by `line_take` is pasted with the caret at
/// `caret` and nothing selected: at the start of the caret's line,
/// wherever the caret stands in it, as VS Code and Zed do, so a whole
/// line never splits another. Answers the insert position and the
/// caret after it, on the letter it was on.
pub fn line_paste(source: &str, caret: usize, text: &str) -> (usize, usize) {
    let caret = caret.min(source.len());
    let start = source[..caret].rfind('\n').map_or(0, |i| i + 1);
    (start, caret + text.len())
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
/// leaves it. A bare quote line never nests: it holds no item, and
/// four columns after its marks would make its text a code block.
pub fn tab_nests(line: &str, col: usize) -> bool {
    let (_, quote) = marker_seat(line);
    match list_marker(&line[quote..]) {
        Some((len, _)) => col <= quote + len,
        None => false,
    }
}

/// Why Tab leaves a run of markdown list items where they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestRefusal {
    /// No list item stands above to nest under.
    NoItemAbove,
    /// The first item is already a level below the item above it.
    AlreadyNested,
}

impl NestRefusal {
    pub fn message(self) -> &'static str {
        match self {
            NestRefusal::NoItemAbove => "No list item above to nest under",
            NestRefusal::AlreadyNested => "Already nested under the item above",
        }
    }
}

/// The columns a line's leading whitespace spans, a tab reaching the
/// next multiple of four as markdown counts it.
fn indent_columns(indent: &str) -> usize {
    indent.bytes().fold(0, |col, b| match b {
        b'\t' => col + 4 - col % 4,
        _ => col + 1,
    })
}

/// A line's quote marks: how many, and the byte where its own text
/// starts, after each `>` and the one space that belongs to it. Up to
/// three spaces may stand before the first mark; with four columns the
/// line is no quote, and an unquoted line answers zero twice.
fn quote_seat(line: &str) -> (usize, usize) {
    let b = line.as_bytes();
    let lead = b.iter().take_while(|c| **c == b' ').count();
    if lead > 3 {
        return (0, 0);
    }
    let (mut depth, mut i) = (0, lead);
    while b.get(i) == Some(&b'>') {
        depth += 1;
        i += 1;
        if b.get(i) == Some(&b' ') {
            i += 1;
        }
    }
    if depth == 0 {
        (0, 0)
    } else {
        (depth, i)
    }
}

/// The indentation of a list item in columns, given the line's text
/// after its quote marks; None for any other line.
fn item_columns(text: &str) -> Option<usize> {
    let (indent, quote) = marker_seat(text);
    (quote == indent && list_marker(&text[indent..]).is_some())
        .then(|| indent_columns(&text[..indent]))
}

/// The column a list item's own text starts at: its indent, its marker
/// and the space after, the column a nested item has to reach. A
/// bullet's is two, `1.` has three, `10.` four.
fn item_content_column(text: &str) -> Option<usize> {
    let (indent, quote) = marker_seat(text);
    if quote != indent {
        return None;
    }
    let rest = &text[indent..];
    list_marker(rest)?;
    let head = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        rest.bytes().take_while(u8::is_ascii_digit).count() + 1
    };
    let space = rest[head..]
        .bytes()
        .take_while(|c| *c == b' ' || *c == b'\t')
        .count();
    Some(indent_columns(&text[..indent]) + head + space)
}

/// The fence a line opens or closes, given the line's text after its
/// quote marks: a backtick or a tilde run of three at the margin, up to
/// three spaces in.
fn fence_mark(text: &str) -> Option<char> {
    let at_margin = text.trim_start_matches(' ');
    if text.len() - at_margin.len() > 3 {
        return None;
    }
    if at_margin.starts_with("```") {
        Some('`')
    } else if at_margin.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

/// Where a YAML frontmatter ends: the offset past its closing `---`
/// line, for a file that opens with one.
fn frontmatter_end(source: &str) -> Option<usize> {
    let mut at = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let text = line.trim_end_matches(['\n', '\r']);
        if index == 0 {
            if text != "---" {
                return None;
            }
        } else if text == "---" {
            return Some(at + line.len());
        }
        at += line.len();
    }
    None
}

/// How far up the item above is looked for. Past this much text that
/// ended no list there is none, and a Tab held down stays free: the
/// cap `indent_unit` keeps, for the same reason.
const ABOVE_CAP: usize = 64 * 1024;

/// The outliner's rule, which is markdown's too: a list item sits at
/// most one level below the item above it. Indented any further, or
/// with no item above, it stops being a list item: after a blank line
/// four columns make an indented code block, and under a paragraph the
/// line joins it as text. Answers the refusal for one more indent over
/// the lines `start..end` when every one of them is a list item under
/// the same quote marks, none included; the first decides for the run.
/// Any other selection indents freely, four columns being how a code
/// block is asked for.
///
/// The item above is the nearest one going up under the same quote
/// marks, across blank lines, the indented lines of an item's own
/// text, and margin lines that reach an item without a blank line
/// between, its lazy continuation. A margin line cut off from every
/// item by a blank line is a paragraph of the document, and it ended
/// the list. A line under other quote marks counts as a margin line,
/// and a blank one under fewer marks ended the quote, the list with it.
pub fn nest_refusal(source: &str, start: usize, end: usize) -> Option<NestRefusal> {
    let mut lines = source[start..end]
        .split('\n')
        .map(|line| {
            let (depth, seat) = quote_seat(line);
            (depth, &line[seat..])
        })
        .filter(|(_, text)| !text.trim().is_empty());
    let (depth, first) = lines.next()?;
    let current = item_columns(first)?;
    if !lines.all(|(d, text)| d == depth && item_columns(text).is_some()) {
        return None;
    }
    let mut margin_run = false;
    let frontmatter = frontmatter_end(source);
    let head = &source[..start];
    let mut cursor = head.len();
    let mut fence = None;
    for raw in head.rsplit('\n') {
        let line_start = cursor - raw.len();
        cursor = line_start.saturating_sub(1);
        if line_start == start {
            continue;
        }
        // The frontmatter is the document's own, like a paragraph at
        // the margin: nothing in it is the item above.
        if start - line_start > ABOVE_CAP || frontmatter.is_some_and(|end| line_start < end) {
            return Some(NestRefusal::NoItemAbove);
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let (d, seat) = quote_seat(line);
        let text = &line[seat..];
        // A fenced code block is skipped whole, whatever its lines look
        // like; going up, its closing fence comes first. It ended the
        // list above it as a paragraph at the margin does.
        if let Some(mark) = fence_mark(text) {
            fence = match fence {
                Some(open) if open == mark => None,
                Some(open) => Some(open),
                None => Some(mark),
            };
            margin_run = true;
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let blank = text.trim().is_empty();
        if d != depth {
            if blank && d < depth {
                return Some(NestRefusal::NoItemAbove);
            }
            margin_run = true;
        } else if blank {
            if margin_run {
                return Some(NestRefusal::NoItemAbove);
            }
        } else if let Some(above) = item_content_column(text) {
            // Nested once the item's indent reaches the text of the
            // item above: two columns under a bullet, three under `1.`.
            return (current >= above).then_some(NestRefusal::AlreadyNested);
        } else if !text.starts_with([' ', '\t']) {
            margin_run = true;
        }
    }
    Some(NestRefusal::NoItemAbove)
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
/// dominant space step where space-led lines do. A file that offers no
/// evidence, every new one, answers by its kind: four spaces in
/// markdown, where a hard tab is flagged by the linters and four
/// columns nest under every list marker (a line under `1. ` needs
/// three), a tab elsewhere, which Go and Makefiles require.
/// The step is the most common leading-width
/// difference between a line and the nearest less-indented line above,
/// kept inside 2..=8; ties go to the smaller step. The scan caps at
/// the first 64KB, which carries any real file's indentation habits:
/// the full pass read 9.5ms on the 8MB fixture, too slow for a held
/// key, and the capped one is free at human rate.
pub fn indent_unit(source: &str, markdown: bool) -> IndentUnit {
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
    if tabs == 0 && spaces == 0 && markdown {
        return IndentUnit::Spaces(4);
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

/// `reindent` for a markdown source. A quoted line keeps its marks at
/// the margin and takes or gives the indent after them: in front of
/// the `>`, four columns would end the quote and leave `> - item` as
/// text or code. Answers, per line, the byte column where the line
/// changed beside the delta, `ride_rewrite`'s form.
pub fn reindent_markdown(
    region: &str,
    unit: &IndentUnit,
    outdent: bool,
) -> (String, Vec<(usize, i64)>) {
    let mut out = String::with_capacity(region.len() + 64);
    let mut edits = Vec::with_capacity(8);
    for (i, line) in region.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let (_, seat) = quote_seat(line);
        let (marks, text) = line.split_at(seat);
        out.push_str(marks);
        let delta = if outdent {
            let cut = outdent_cut(text, unit);
            out.push_str(&text[cut..]);
            -(cut as i64)
        } else if text.trim().is_empty() {
            out.push_str(text);
            0
        } else {
            let ins = unit.text();
            out.push_str(&ins);
            out.push_str(text);
            ins.len() as i64
        };
        edits.push((seat, delta));
    }
    (out, edits)
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

/// The word the caret stands in or at an end of: a run of letters,
/// digits, underscores and combining marks (`caret::word_char`). None
/// between two spaces or in an empty file.
fn word_at(source: &str, at: usize) -> Option<std::ops::Range<usize>> {
    let is_word = super::caret::word_char;
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
}

/// The run of non-blank characters the caret stands in or at an end
/// of, when it is an address.
fn address_at(source: &str, at: usize) -> Option<std::ops::Range<usize>> {
    let start = source[..at].rfind(char::is_whitespace).map_or(0, |i| {
        i + source[i..].chars().next().map_or(1, char::len_utf8)
    });
    let end = source[at..]
        .find(char::is_whitespace)
        .map_or(source.len(), |i| at + i);
    (end > start && is_url(&source[start..end])).then_some(start..end)
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
/// adds its own star. With no selection the caret's place among the
/// line's marks decides first (`toggle_at_caret`); then, with no word,
/// an empty pair opens with the caret inside, and with a word the
/// caret keeps its letter, or goes past the closing mark from the
/// word's end. The inner range is what stays selected.
pub fn toggle_mark(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    caret: usize,
    mark: &str,
) -> MarkEdit {
    let m = mark.len();
    let word = |at: usize| word_at(source, at);
    let selection = selection.filter(|r| !r.is_empty());
    if selection.is_none() {
        if let Some(edit) = toggle_at_caret(source, caret, mark) {
            return edit;
        }
    }
    let (range, pinned) = match selection {
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
    // A caret at the word's end is someone writing on: it goes past the
    // closing mark, so the next letters land outside the mark.
    let caret = pinned.map_or(inner.end, |p| {
        if p == range.end {
            inner.end + m
        } else {
            ride(p, range.start, m as i64)
        }
    });
    MarkEdit {
        replace: range,
        text: format!("{mark}{text}{mark}"),
        inner,
        caret,
    }
}

/// What the key does by where the caret stands among the marks of its
/// own line, nothing selected. On an empty pair the pair goes. Right
/// before a run's closing mark the caret steps over it and the text
/// stays, the word processor's "off for what comes next". Anywhere
/// else inside a run the run loses its marks, however many words it
/// holds. None everywhere else, and on a line whose marks do not pair,
/// where the word under the caret decides as before.
fn toggle_at_caret(source: &str, caret: usize, mark: &str) -> Option<MarkEdit> {
    let m = mark.len();
    let sign = mark.as_bytes()[0];
    let line_start = source[..caret].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[caret..]
        .find('\n')
        .map_or(source.len(), |i| caret + i);
    let line = &source[line_start..line_end];
    let at = caret - line_start;
    let b = line.as_bytes();

    // The empty pair: the mark on both sides and no more of its sign
    // around it, with no word pressed against the closing mark, which
    // would make that mark an opening one.
    if at >= m
        && line[..at].ends_with(mark)
        && line[at..].starts_with(mark)
        && (at == m || b[at - m - 1] != sign)
        && b.get(at + m)
            .is_none_or(|c| *c != sign && !c.is_ascii_alphanumeric())
    {
        let open = caret - m;
        return Some(MarkEdit {
            replace: open..caret + m,
            text: String::new(),
            inner: open..open,
            caret: open,
        });
    }

    let (open, close) = marked_runs(line, mark)?
        .into_iter()
        .find(|(open, close)| open + m <= at && at <= *close)?;
    if at == close {
        return Some(MarkEdit {
            replace: caret..caret,
            text: String::new(),
            inner: caret..caret,
            caret: caret + m,
        });
    }
    let text = &line[open + m..close];
    let start = line_start + open;
    Some(MarkEdit {
        replace: start..line_start + close + m,
        text: text.to_string(),
        inner: start..start + text.len(),
        caret: caret - m,
    })
}

/// The runs `mark` wraps on one line, as the byte positions of each
/// opening and closing mark. A run of the mark's sign is read the way
/// markdown pairs it: two stars a bold mark, one an italic mark, three
/// one of each; one backtick a code mark. A sign after a backslash, a
/// star inside inline code, and a run with whitespace on both sides (a
/// bullet, a product) are text. None when the marks do not pair, an
/// odd count or a mark facing the wrong way.
fn marked_runs(line: &str, mark: &str) -> Option<Vec<(usize, usize)>> {
    let sign = mark.as_bytes()[0];
    let code = if sign == b'`' {
        Vec::new()
    } else {
        marked_runs(line, "`").unwrap_or_default()
    };
    let b = line.as_bytes();
    // Each mark found: its position, whether it may open, may close.
    let mut found: Vec<(usize, bool, bool)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != sign {
            i += 1;
            continue;
        }
        let mut start = i;
        while i < b.len() && b[i] == sign {
            i += 1;
        }
        if start > 0 && b[start - 1] == b'\\' {
            start += 1;
        }
        let len = i - start;
        if code
            .iter()
            .any(|(open, close)| *open < start && start < *close)
        {
            continue;
        }
        let opens = b.get(i).is_some_and(|c| !c.is_ascii_whitespace());
        let closes = start > 0 && !b[start - 1].is_ascii_whitespace();
        if !opens && !closes {
            continue;
        }
        match mark {
            "**" => found.extend((0..len / 2).map(|k| (start + 2 * k, opens, closes))),
            "*" if len % 2 == 1 => found.push((start + len - 1, opens, closes)),
            "`" if len == 1 => found.push((start, true, true)),
            _ => {}
        }
    }
    if found.len() % 2 == 1 {
        return None;
    }
    found
        .chunks(2)
        .map(|pair| (pair[0].1 && pair[1].2).then_some((pair[0].0, pair[1].0)))
        .collect()
}

/// Ctrl+K: the selection becomes a link's text with the caret in the
/// empty parentheses; a selection that is itself an address becomes
/// the target with the caret in the empty brackets. With no selection
/// the address under the caret, else the word under it, stands for the
/// selection, as the word does for the bold key; with neither, an
/// empty link opens with the caret in the brackets.
pub fn link_edit(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    caret: usize,
) -> MarkEdit {
    let taken = selection
        .filter(|r| !r.is_empty())
        .or_else(|| address_at(source, caret))
        .or_else(|| word_at(source, caret));
    let (replace, text, caret) = match taken {
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

/// The forms the case key cycles through, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseForm {
    Lower,
    Title,
    Upper,
}

const CASE_CYCLE: [CaseForm; 3] = [CaseForm::Lower, CaseForm::Title, CaseForm::Upper];

/// The stretches of a text the parser reads as one paragraph, each
/// with the line breaks it holds: between blank lines, and cut before
/// a line that opens a block of its own, which the parser lets
/// interrupt a paragraph (`starts_block`), so that a `$` above such a
/// line pairs with none below it. A quote's own lines, each with its
/// marker, stay one paragraph.
fn paragraphs(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    let mut at = 0;
    // What the paragraph began as: a block quote's, which its unquoted
    // lines continue lazily until one opens a block; an item of a list,
    // where the next item with any number begins a block; indented
    // code, which ends at the first line indented less.
    let (mut in_quote, mut in_list, mut in_code) = (false, false, false);
    for line in text.split_inclusive('\n') {
        let body = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - body.len();
        let blank = body.trim().is_empty();
        let quoted = body.starts_with('>');
        if let Some(s) = start {
            let lazy = in_quote && !quoted;
            if blank
                || starts_block(indent, body, lazy)
                || (quoted && !in_quote)
                || (lazy && (is_item(line) || marker_alone(body)))
                || (in_list && is_item(line))
                || (in_code && !indented_code(line))
            {
                out.push(s..at);
                start = None;
            }
        }
        if !blank && start.is_none() {
            start = Some(at);
            in_quote = quoted;
            in_list = is_item(line);
            in_code = indented_code(line);
        }
        at += line.len();
    }
    if let Some(s) = start {
        out.push(s..text.len());
    }
    out
}

/// Whether a line is a list item with text: a bullet, or a number of
/// up to nine digits with `.` or `)`, then whitespace and text, indented
/// three spaces at most.
fn is_item(line: &str) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let l = &line[indent..];
    let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let marker = if digits == 0 {
        usize::from(l.starts_with(['-', '*', '+']))
    } else if (1..=9).contains(&digits) && l[digits..].starts_with(['.', ')']) {
        digits + 1
    } else {
        0
    };
    indent <= 3
        && marker > 0
        && l[marker..].starts_with([' ', '\t'])
        && !l[marker..].trim().is_empty()
}

/// Whether a line is a list marker and nothing else: a bullet, or a
/// number with `.` or `)`. After a block quote's line, the parser ends
/// the quote at such a line and opens an empty item; after a plain
/// paragraph's line it reads the marker as the paragraph's text.
fn marker_alone(body: &str) -> bool {
    let l = body.trim_end();
    let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    matches!(l, "*" | "+" | "-")
        || ((1..=9).contains(&digits) && l.len() == digits + 1 && l.ends_with(['.', ')']))
}

/// Whether a line is indented as code: four spaces or a tab, with text
/// after them. The page shows such a block verbatim.
fn indented_code(line: &str) -> bool {
    (line.starts_with("    ") || line.starts_with('\t')) && !line.trim().is_empty()
}

/// Whether a line opens a block that interrupts a paragraph, or
/// underlines one into a heading, as the parser reads it: a heading, a
/// fence, a thematic break, a setext underline of `=` or `-`, or a
/// list item with text (a bullet, or `1.` and `1)`), indented three
/// spaces at most. `body` is the line past its indent of `indent`
/// bytes. An unquoted line of a block quote's paragraph (`lazy`) that
/// is `=` or two `-` is the quote's own text, not an underline, as the
/// parser reads it; the quote itself is read by `paragraphs`. The
/// first byte settles most lines at once: a press on two hundred
/// thousand rows reads each line once.
fn starts_block(indent: usize, body: &str, lazy: bool) -> bool {
    let Some(first) = body.bytes().next() else {
        return false;
    };
    let l = body.trim_end();
    if indent > 3
        || !matches!(
            first,
            b'#' | b'`' | b'~' | b'*' | b'-' | b'_' | b'+' | b'1' | b'='
        )
    {
        return false;
    }
    let item_after =
        |marker: usize| l[marker..].starts_with([' ', '\t']) && !l[marker..].trim().is_empty();
    match first {
        b'#' => {
            let signs = l.len() - l.trim_start_matches('#').len();
            signs <= 6 && (l.len() == signs || l[signs..].starts_with([' ', '\t']))
        }
        b'`' => l.starts_with("```"),
        b'~' => l.starts_with("~~~"),
        b'=' => !lazy && l.bytes().all(|b| b == b'='),
        b'1' => (l.starts_with("1.") || l.starts_with("1)")) && item_after(2),
        b'+' => item_after(1),
        _ => {
            // `*`, `-` or `_`: a thematic break of three or more, a
            // setext underline of `-`, or a bullet with text.
            let (mut count, mut same) = (0, true);
            for b in l.bytes() {
                if b == first {
                    count += 1;
                } else if b != b' ' && b != b'\t' {
                    same = false;
                    break;
                }
            }
            (same && (count >= 3 || (first == b'-' && !lazy))) || (first != b'_' && item_after(1))
        }
    }
}

/// The stretch from the start of the paragraph that holds a range's
/// start to the end of the one that holds its end.
fn paragraphs_around(source: &str, range: &std::ops::Range<usize>) -> std::ops::Range<usize> {
    let mut start = source[..range.start].rfind('\n').map_or(0, |i| i + 1);
    while start > 0 {
        let line_end = start - 1;
        let line_start = source[..line_end].rfind('\n').map_or(0, |i| i + 1);
        if source[line_start..line_end].trim().is_empty() {
            break;
        }
        start = line_start;
    }
    let mut end = source[range.end..]
        .find('\n')
        .map_or(source.len(), |i| range.end + i + 1);
    while end < source.len() {
        let line_end = source[end..].find('\n').map_or(source.len(), |i| end + i);
        if source[end..line_end].trim().is_empty() {
            break;
        }
        end = (line_end + 1).min(source.len());
    }
    start..end
}

/// The ranges of `text` the case key keeps as they are: the pieces the
/// page shows as something other than its text. An indented code
/// block is kept whole. Inline code runs to a closing run of as many
/// backticks on its paragraph; a link's address is what the parser
/// reads after a `](` whose `]` closes a `[` (`address_end`); a
/// reference definition's address; an autolink holds no whitespace; a
/// tag is what CommonMark reads as one (`html_tag_len`). A formula is
/// what the page shows as one: the parser's pairing (`formula_end`),
/// then the page's currency rule, by which a paired formula whose
/// closer precedes a digit is text. An escaped dollar sign is text. A
/// press costs about the text's length: a length of backtick run found
/// unclosed is not searched again in its paragraph, each `$` is visited
/// once (the signs between an opener found to be text and the next `$`
/// that can open are stepped over, each examined by the scan), the next
/// `>` of an autolink candidate is searched once per region, and a tag
/// candidate is refused at its first byte off the grammar.
fn kept_pieces(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut kept = Vec::new();
    for para in paragraphs(text) {
        if indented_code(&text[para.clone()]) {
            kept.push(para);
            continue;
        }
        let mut unclosed: Vec<usize> = Vec::new();
        // The `$` signs before this offset were examined by a scan and
        // cannot open: stepped over.
        let mut skip_dollars_until = para.start;
        // The next `>` from a position, remembered until the scan passes
        // it: None once no `>` stands before the paragraph's end.
        let mut next_gt: Option<Option<usize>> = None;
        // The `[` open before the position: a `](` whose `]` closes
        // none is text, not a link's address.
        let mut brackets = 0usize;
        let mut line_start = true;
        let mut i = para.start;
        while i < para.end {
            let rest = &text[i..para.end];
            if line_start {
                if let Some(address) = definition_address(rest) {
                    kept.push(i + address.start..i + address.end);
                    i += address.end;
                    line_start = false;
                    continue;
                }
            }
            line_start = false;
            let c = rest.chars().next().expect("inside the text");
            if c == '\n' {
                line_start = true;
                i += 1;
            } else if c == '\\' {
                // An escaped character is text, a backtick or a dollar
                // sign among them.
                i += 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
            } else if c == '`' {
                let ticks = rest.len() - rest.trim_start_matches('`').len();
                let close = if unclosed.contains(&ticks) {
                    None
                } else {
                    closing_ticks(&rest[ticks..], ticks)
                };
                match close {
                    Some(at) => {
                        let len = ticks + at + ticks;
                        kept.push(i..i + len);
                        i += len;
                    }
                    None => {
                        if !unclosed.contains(&ticks) {
                            unclosed.push(ticks);
                        }
                        i += ticks;
                    }
                }
            } else if c == '$' {
                if i < skip_dollars_until {
                    i += 1;
                    continue;
                }
                let display = rest.starts_with("$$");
                let signs = if display { 2 } else { 1 };
                // A `$` opens when no whitespace follows it; a `$$`
                // always, its first sign being followed by the second.
                let opens = display
                    || rest[1..]
                        .chars()
                        .next()
                        .is_some_and(|next| !next.is_ascii_whitespace());
                if !opens {
                    i += 1;
                    continue;
                }
                match formula_end(&rest[signs..], display) {
                    Formula::Closed(len) => {
                        let end = i + signs + len;
                        // The page's currency rule: a formula whose
                        // closer precedes a digit is the text typed.
                        if !text.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                            kept.push(i..end);
                        }
                        i = end;
                    }
                    // A `$$` found to be text leaves its second sign to
                    // open an inline formula of its own, as the parser
                    // tries it next; a `$` found to be text is stepped
                    // over with every `$` before the one that can open.
                    Formula::Text { .. } if display => i += 1,
                    Formula::Text { resume } => {
                        skip_dollars_until = resume.map_or(para.end, |at| i + 1 + at);
                        i += 1;
                    }
                }
            } else if c == '[' {
                brackets += 1;
                i += 1;
            } else if let Some(after) = rest.strip_prefix("](") {
                match address_end(after).filter(|_| brackets > 0) {
                    Some(len) => {
                        kept.push(i..i + 2 + len);
                        i += 2 + len;
                        brackets -= 1;
                    }
                    // No link: the `]` still closes its `[`, as the
                    // parser drops the bracket it could not pair.
                    None => {
                        brackets = brackets.saturating_sub(1);
                        i += 1;
                    }
                }
            } else if c == ']' {
                brackets = brackets.saturating_sub(1);
                i += 1;
            } else if c == '<' {
                if let Some(len) = html_tag_len(rest) {
                    kept.push(i..i + len);
                    i += len;
                    continue;
                }
                let gt = match next_gt {
                    Some(Some(at)) if at > i => Some(at),
                    Some(None) => None,
                    _ => {
                        let found = text[i + 1..para.end].find('>').map(|k| i + 1 + k);
                        next_gt = Some(found);
                        found
                    }
                };
                match gt
                    .map(|at| &text[i + 1..at])
                    .filter(|inner| autolink_inner(inner))
                {
                    Some(inner) => {
                        let len = 1 + inner.len() + 1;
                        kept.push(i..i + len);
                        i += len;
                    }
                    None => i += 1,
                }
            } else {
                i += c.len_utf8();
            }
        }
    }
    kept
}

/// How the text after an opening `$` or `$$` reads.
enum Formula {
    /// A formula: the length from after the opener to the end of its
    /// closer.
    Closed(usize),
    /// The opener is text. `resume` is the offset of the first later `$`
    /// that can open a formula of its own, where the scan goes on; None
    /// when no later `$` of the paragraph can open.
    Text { resume: Option<usize> },
}

/// Where the formula opened by a `$` or a `$$` ends, as the markdown
/// parser reads it (pulldown-cmark 0.13.4, `src/firstpass.rs` and
/// `src/parse.rs`, its spec tests 10 and 21): braces are counted from
/// the opener, and only a `$` at the opener's level can pair with it.
/// An inline formula closes at the first such `$` whose previous
/// character is not whitespace; one that cannot close makes the opener
/// text, since a formula cannot hold a `$`. A display formula closes at
/// the first `$$` at its level, and a lone `$` there makes the opener
/// text. A `}` below the opener's level makes it text too. A `$` at
/// the start of a line, past the quote markers, has no character
/// before it and cannot close. Once the opener is text, the first `$`
/// after it that can open, at any level, an inner one included, is
/// where the scan resumes.
fn formula_end(after: &str, display: bool) -> Formula {
    let mut depth = 0i32;
    let mut text = false;
    let mut first_open: Option<usize> = None;
    let mut previous: Option<char> = None;
    // A line's quote markers and the blanks among them are not its
    // text: a `$` right after them has no character before it.
    let mut in_prefix = false;
    let mut chars = after.char_indices();
    while let Some((i, c)) = chars.next() {
        if in_prefix && matches!(c, '>' | ' ' | '\t') {
            previous = None;
            continue;
        }
        in_prefix = false;
        match c {
            '\n' => {
                previous = None;
                in_prefix = true;
                continue;
            }
            '\\' => {
                previous = chars.next().map(|(_, escaped)| escaped);
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    text = true;
                }
            }
            '$' => {
                let next = after[i + 1..].chars().next();
                if !text && depth == 0 {
                    if display {
                        if next == Some('$') {
                            return Formula::Closed(i + 2);
                        }
                    } else if previous.is_some_and(|p| !p.is_ascii_whitespace()) {
                        return Formula::Closed(i + 1);
                    }
                    text = true;
                }
                if first_open.is_none() && next.is_some_and(|n| !n.is_ascii_whitespace()) {
                    first_open = Some(i);
                }
            }
            _ => {}
        }
        if text && first_open.is_some() {
            return Formula::Text { resume: first_open };
        }
        previous = Some(c);
    }
    Formula::Text { resume: first_open }
}

/// The offset in `after` of a run of exactly `ticks` backticks, the
/// closer of a code span.
fn closing_ticks(after: &str, ticks: usize) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = after[from..].find('`') {
        let run_start = from + at;
        let run = after[run_start..].len() - after[run_start..].trim_start_matches('`').len();
        if run == ticks {
            return Some(run_start);
        }
        from = run_start + run;
    }
    None
}

/// The length of a link's address from after its `](` to its closing
/// parenthesis, as the parser reads it: a destination between angle
/// brackets, or one without whitespace and with its parentheses
/// balanced; then an optional title in quotes or parentheses after
/// whitespace; then the closing parenthesis. Whitespace, with one line
/// break at most, may stand before and after the destination. None
/// when the shape is not a link's, which the page then shows as text.
fn address_end(after: &str) -> Option<usize> {
    let b = after.as_bytes();
    let space = |at: &mut usize| {
        let mut breaks = 0;
        while let Some(&c) = b.get(*at) {
            match c {
                b' ' | b'\t' => {}
                b'\n' if breaks == 0 => breaks += 1,
                _ => break,
            }
            *at += 1;
        }
    };
    let mut at = 0;
    space(&mut at);
    if b.get(at) == Some(&b'<') {
        let close = after[at + 1..].find(['>', '<', '\n'])?;
        if b[at + 1 + close] != b'>' {
            return None;
        }
        at += close + 2;
    } else {
        let mut depth = 0u32;
        while let Some(&c) = b.get(at) {
            match c {
                b'\\' => at += 1,
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                b' ' | b'\t' | b'\n' => break,
                c if c < 0x20 => return None,
                _ => {}
            }
            at += 1;
        }
        if depth != 0 {
            return None;
        }
    }
    let dest_end = at;
    space(&mut at);
    let close = match b.get(at) {
        Some(b'"') if at > dest_end => Some('"'),
        Some(b'\'') if at > dest_end => Some('\''),
        Some(b'(') if at > dest_end => Some(')'),
        _ => None,
    };
    if let Some(close) = close {
        let end = after[at + 1..].find([close, '\n'])?;
        if after.as_bytes()[at + 1 + end] != close as u8 {
            return None;
        }
        at += end + 2;
        space(&mut at);
    }
    (b.get(at) == Some(&b')')).then_some(at + 1)
}

/// Whether the text between a `<` and the next `>` is an autolink: an
/// address with a scheme or an at sign and no whitespace. The candidate
/// is refused on its first whitespace or `<` before any other look,
/// and the scheme is sought in its first 32 bytes, the longest the
/// markdown rules allow: on a paragraph of many `<` with a `>` near
/// its end, each candidate runs to that `>`, and a scan of each to its
/// end cost the square of the paragraph's length.
fn autolink_inner(inner: &str) -> bool {
    if inner.is_empty() || inner.contains(|c: char| c.is_whitespace() || c == '<') {
        return false;
    }
    let scheme = inner
        .bytes()
        .take(33)
        .position(|b| b == b':')
        .is_some_and(|colon| {
            let s = &inner[..colon];
            s.len() >= 2
                && s.starts_with(|c: char| c.is_ascii_alphabetic())
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        });
    scheme || inner.contains('@')
}

/// The length of the raw HTML that CommonMark reads at the start of
/// `rest`, which begins with `<`, on its line: an open tag (a name of
/// a letter then letters, digits and hyphens; attributes, each after
/// whitespace, a name of a letter, `_` or `:` then letters, digits,
/// `_`, `.`, `:` and `-`, with an optional `=` and a value, bare or
/// quoted; optional whitespace and `/`; then `>`), a closing tag, a
/// comment, a processing instruction, a declaration or a CDATA section.
/// None for anything else between angle brackets, which the page shows
/// as text. The scan stops at the first byte off the grammar, so a
/// `<` that starts no tag costs a byte or two.
fn html_tag_len(rest: &str) -> Option<usize> {
    if !rest
        .as_bytes()
        .get(1)
        .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?'))
    {
        return None;
    }
    let line = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let b = line.as_bytes();
    let delimited = |open: &str, close: &str| {
        line.strip_prefix(open)
            .and_then(|inner| inner.find(close))
            .map(|k| open.len() + k + close.len())
    };
    if let Some(len) = delimited("<!--", "-->")
        .or_else(|| delimited("<?", "?>"))
        .or_else(|| delimited("<![CDATA[", "]]>"))
    {
        return Some(len);
    }
    if line.starts_with("<!") && b.get(2).is_some_and(u8::is_ascii_alphabetic) {
        return line.find('>').map(|k| k + 1);
    }
    let space = |at: usize| b.get(at).is_some_and(|c| *c == b' ' || *c == b'\t');
    let closing = line.starts_with("</");
    let mut at = if closing { 2 } else { 1 };
    if !b.get(at).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    at += 1;
    while b
        .get(at)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-')
    {
        at += 1;
    }
    if closing {
        while space(at) {
            at += 1;
        }
        return (b.get(at) == Some(&b'>')).then_some(at + 1);
    }
    loop {
        let before_space = at;
        while space(at) {
            at += 1;
        }
        match b.get(at) {
            Some(b'>') => return Some(at + 1),
            Some(b'/') => return (b.get(at + 1) == Some(&b'>')).then_some(at + 2),
            Some(c)
                if at > before_space && (c.is_ascii_alphabetic() || *c == b'_' || *c == b':') =>
            {
                at += 1;
                while b
                    .get(at)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(c))
                {
                    at += 1;
                }
                let mut probe = at;
                while space(probe) {
                    probe += 1;
                }
                if b.get(probe) != Some(&b'=') {
                    continue;
                }
                probe += 1;
                while space(probe) {
                    probe += 1;
                }
                let bare = |c: &u8| !b" \t\"'=<>`".contains(c);
                match b.get(probe) {
                    Some(&q) if q == b'"' || q == b'\'' => {
                        let close = line[probe + 1..].find(q as char)?;
                        at = probe + 1 + close + 1;
                    }
                    Some(c) if bare(c) => {
                        while b.get(probe).is_some_and(bare) {
                            probe += 1;
                        }
                        at = probe;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

/// The address of a reference definition at the start of a line,
/// `[label]: address`, as its range in the line. None for any other
/// line.
fn definition_address(line: &str) -> Option<std::ops::Range<usize>> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 || !line[indent..].starts_with('[') {
        return None;
    }
    let label_end = indent + 1 + line[indent + 1..].find([']', '\n'])?;
    if label_end == indent + 1 || !line[label_end..].starts_with("]:") {
        return None;
    }
    let after = label_end + 2;
    let start = after + line[after..].len() - line[after..].trim_start_matches([' ', '\t']).len();
    let len = line[start..]
        .find(char::is_whitespace)
        .unwrap_or(line.len() - start);
    (len > 0).then(|| start..start + len)
}

/// The text of a range cut into the pieces the case key changes and
/// the pieces that keep their letters, from the kept ranges of the
/// source. True marks a kept piece.
fn pieces_of<'s>(
    source: &'s str,
    range: &std::ops::Range<usize>,
    kept: &[std::ops::Range<usize>],
) -> Vec<(&'s str, bool)> {
    let mut pieces = Vec::new();
    let mut at = range.start;
    for k in kept {
        if k.end <= at {
            continue;
        }
        if k.start >= range.end {
            break;
        }
        let (from, to) = (k.start.max(at), k.end.min(range.end));
        if at < from {
            pieces.push((&source[at..from], false));
        }
        pieces.push((&source[from..to], true));
        at = to;
    }
    if at < range.end {
        pieces.push((&source[at..range.end], false));
    }
    pieces
}

/// The pieces in a form, the kept ones as they are. A capital on each
/// word goes on the first letter or digit after whitespace, the
/// punctuation before it left alone, and the rest of the word is
/// lowered. A tag is not on the page, so the text after it starts a
/// word; inline code and an address are, and a word goes on after
/// them. The letters change as strings, so a Greek sigma at the end of
/// a word takes its final form.
fn in_case(pieces: &[(&str, bool)], form: CaseForm) -> String {
    let mut out = String::new();
    let mut word_start = true;
    for (piece, kept) in pieces {
        if *kept {
            out.push_str(piece);
            // A tag is not on the page, and a kept block ends with its
            // line: the text after either starts a word.
            word_start = (piece.starts_with('<') && piece.ends_with('>'))
                || piece.ends_with(char::is_whitespace);
            continue;
        }
        match form {
            CaseForm::Lower => out.push_str(&piece.to_lowercase()),
            CaseForm::Upper => out.push_str(&piece.to_uppercase()),
            CaseForm::Title => title_case(piece, &mut word_start, &mut out),
        }
    }
    out
}

/// A text with a capital on each word, appended to `out`. `word_start`
/// says whether the text begins a word, and says at the end whether a
/// word begins after it.
fn title_case(text: &str, word_start: &mut bool, out: &mut String) {
    let mut rest = text;
    while !rest.is_empty() {
        let blank = rest.len() - rest.trim_start().len();
        if blank > 0 {
            out.push_str(&rest[..blank]);
            *word_start = true;
            rest = &rest[blank..];
            continue;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..len];
        let first = (*word_start)
            .then(|| word.char_indices().find(|(_, c)| c.is_alphanumeric()))
            .flatten();
        match first {
            Some((i, c)) => {
                out.push_str(&word[..i]);
                out.extend(c.to_uppercase());
                out.push_str(&word[i + c.len_utf8()..].to_lowercase());
                *word_start = false;
            }
            None if *word_start => out.push_str(word),
            None => out.push_str(&word.to_lowercase()),
        }
        rest = &rest[len..];
    }
}

/// The next form of the cycle and the pieces in it, from the form they
/// are in: the first form after it that reads differently, so that a
/// single capital letter, both a capital on each word and upper case,
/// goes to lower case. None when no letter the key changes is there.
fn next_case(pieces: &[(&str, bool)]) -> Option<(CaseForm, String)> {
    let has_letter = pieces
        .iter()
        .any(|(piece, kept)| !kept && piece.chars().any(char::is_alphabetic));
    if !has_letter {
        return None;
    }
    let text: String = pieces.iter().map(|(piece, _)| *piece).collect();
    let current = CASE_CYCLE.iter().position(|f| in_case(pieces, *f) == text);
    let first = current.map_or(0, |i| i + 1);
    (0..CASE_CYCLE.len())
        .map(|k| CASE_CYCLE[(first + k) % CASE_CYCLE.len()])
        .map(|form| (form, in_case(pieces, form)))
        .find(|(_, t)| *t != text)
}

/// The next case of the selection, or of the word under the caret:
/// lower case, then a capital on each word, then upper case, then
/// lower case again, from the form the text is in. The new text stays
/// selected, so the key cycles on; with no selection the caret keeps
/// its place in the word. In a markdown file the pieces that keep
/// their letters (`kept_pieces`) are found on the paragraphs the text
/// touches, and the word under the caret inside one is not the key's
/// to change. None when nothing would change.
pub fn case_edit(
    source: &str,
    selection: Option<std::ops::Range<usize>>,
    caret: usize,
    markdown: bool,
) -> Option<MarkEdit> {
    let selection = selection.filter(|r| !r.is_empty());
    let had_selection = selection.is_some();
    let range = selection.or_else(|| word_at(source, caret))?;
    let kept = if markdown {
        let around = paragraphs_around(source, &range);
        kept_pieces(&source[around.clone()])
            .into_iter()
            .map(|k| around.start + k.start..around.start + k.end)
            .collect()
    } else {
        Vec::new()
    };
    let pieces = pieces_of(source, &range, &kept);
    if !had_selection && pieces.iter().any(|(_, kept)| *kept) {
        return None;
    }
    let (form, text) = next_case(&pieces)?;
    let inner = range.start..range.start + text.len();
    let caret = if had_selection {
        inner.end
    } else {
        // The caret keeps its place by its letters, not by its byte
        // offset: a letter before it may change length in the new form
        // (a dotless i to I, a dotted I to i with a combining dot), and
        // the old offset would fall inside a character.
        let before = &source[range.start..caret.clamp(range.start, range.end)];
        inner.start + in_case(&[(before, false)], form).len()
    };
    Some(MarkEdit {
        replace: range,
        text,
        inner,
        caret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hard_break_ends_the_line_with_two_spaces() {
        assert_eq!(hard_break("one two", 7).as_deref(), Some("  \n"));
        assert_eq!(
            hard_break("one two", 3).as_deref(),
            Some("  \n"),
            "in the middle of a line"
        );
        assert_eq!(
            hard_break("  indented text", 15).as_deref(),
            Some("  \n  "),
            "the line's own indentation carries"
        );
    }

    #[test]
    fn a_hard_break_adds_only_the_spaces_that_are_missing() {
        assert_eq!(hard_break("one ", 4).as_deref(), Some(" \n"));
        assert_eq!(hard_break("one  ", 5).as_deref(), Some("\n"));
        assert_eq!(hard_break("one   ", 6).as_deref(), Some("\n"));
    }

    #[test]
    fn a_hard_break_stays_inside_its_item_and_its_quote() {
        assert_eq!(
            hard_break("- item", 6).as_deref(),
            Some("  \n  "),
            "under the item's text, no marker"
        );
        assert_eq!(hard_break("10. item", 8).as_deref(), Some("  \n    "));
        assert_eq!(
            hard_break("  - [ ] task", 12).as_deref(),
            Some("  \n        "),
            "under the task's text, past the box"
        );
        assert_eq!(hard_break("> quoted", 8).as_deref(), Some("  \n> "));
        assert_eq!(hard_break("> - item", 8).as_deref(), Some("  \n>   "));
    }

    #[test]
    fn a_hard_break_needs_text_before_it() {
        assert_eq!(hard_break("", 0), None);
        assert_eq!(hard_break("text", 0), None);
        assert_eq!(hard_break("   ", 3), None, "spaces alone are no text");
        assert_eq!(hard_break("- ", 2), None, "an empty item");
        assert_eq!(hard_break("- item", 1), None, "inside the marker");
        assert_eq!(hard_break("> ", 2), None);
    }

    #[test]
    fn a_hard_break_renders_as_a_line_break_in_the_item() {
        let line = "- one two";
        let text = hard_break(line, 5).expect("text stands before the caret");
        let source = format!("{}{}{}\n", &line[..5], text, &line[5..]);
        assert_eq!(source, "- one  \n   two\n");
        let doc = crate::doc::markdown::parse(source);
        assert_eq!(doc.blocks.len(), 1, "one item still");
    }

    #[test]
    fn a_line_is_taken_with_its_ending() {
        let take = line_take("a\nbee\nc", 4).expect("a line");
        assert_eq!(
            (take.cut.clone(), take.text.as_str(), take.caret),
            (2..6, "bee\n", 2)
        );
        let take = line_take("a\nb", 0).expect("the first line");
        assert_eq!(
            (take.cut.clone(), take.text.as_str(), take.caret),
            (0..2, "a\n", 0)
        );
        let take = line_take("a\n\nb", 2).expect("an empty line");
        assert_eq!(
            (take.cut.clone(), take.text.as_str(), take.caret),
            (2..3, "\n", 2)
        );
    }

    #[test]
    fn the_last_line_takes_the_ending_before_it() {
        let take = line_take("a\nb", 3).expect("the last line");
        assert_eq!(take.text, "b\n", "the clipboard always gets a whole line");
        assert_eq!(take.cut, 1..3, "no empty line is left behind");
        assert_eq!(take.caret, 0, "the start of the line that is now last");

        let take = line_take("one\ntwo\nthree", 9).expect("the last of three");
        assert_eq!((take.cut.clone(), take.caret), (7..13, 4));

        let take = line_take("a\nb\n", 2).expect("a last line with its own ending");
        assert_eq!(
            (take.cut.clone(), take.text.as_str(), take.caret),
            (2..4, "b\n", 2)
        );
    }

    #[test]
    fn a_taken_line_is_pasted_above_the_carets_line() {
        assert_eq!(
            line_paste("one\ntwo", 5, "one\n"),
            (4, 9),
            "from the middle of two: in front of it, the caret on the same letter"
        );
        assert_eq!(line_paste("one\ntwo", 0, "x\n"), (0, 2), "the first line");
        assert_eq!(line_paste("one\n", 4, "x\n"), (4, 6), "the empty last row");
    }

    #[test]
    fn a_lone_line_and_an_empty_file() {
        let take = line_take("abc", 1).expect("the only line");
        assert_eq!(
            (take.cut.clone(), take.text.as_str(), take.caret),
            (0..3, "abc\n", 0)
        );
        assert_eq!(line_take("", 0), None, "nothing to take");
    }

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
            e(2..6, "**word**", 4..8, 10),
            "at the word's end the caret goes past the closing mark, to write on"
        );
        assert_eq!(
            toggle_mark("a word b", None, 6, "*"),
            e(2..6, "*word*", 3..7, 8),
            "italic the same"
        );
        assert_eq!(
            toggle_mark("a word b", None, 6, "`"),
            e(2..6, "`word`", 3..7, 8),
            "inline code the same"
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

    /// A `MarkEdit`, written short.
    fn mark_edit(
        replace: std::ops::Range<usize>,
        text: &str,
        inner: std::ops::Range<usize>,
        caret: usize,
    ) -> MarkEdit {
        MarkEdit {
            replace,
            text: text.to_string(),
            inner,
            caret,
        }
    }

    #[test]
    fn the_key_again_before_the_closing_mark_steps_over_it() {
        assert_eq!(
            toggle_mark("**two words**", None, 11, "**"),
            mark_edit(11..11, "", 11..11, 13),
            "bold off for what comes next, the text untouched"
        );
        assert_eq!(
            toggle_mark("an *old tale* here", None, 12, "*"),
            mark_edit(12..12, "", 12..12, 13)
        );
        assert_eq!(
            toggle_mark("a `x y` b", None, 6, "`"),
            mark_edit(6..6, "", 6..6, 7)
        );
    }

    #[test]
    fn the_key_again_on_an_empty_pair_removes_it() {
        assert_eq!(
            toggle_mark("a **** b", None, 4, "**"),
            mark_edit(2..6, "", 2..2, 2)
        );
        assert_eq!(
            toggle_mark("a ** b", None, 3, "*"),
            mark_edit(2..4, "", 2..2, 2)
        );
        assert_eq!(
            toggle_mark("``", None, 1, "`"),
            mark_edit(0..2, "", 0..0, 0)
        );
        assert_eq!(
            toggle_mark("**word**", None, 1, "*"),
            mark_edit(1..1, "**", 2..2, 2),
            "between the stars of a bold mark is no empty italic pair"
        );
    }

    #[test]
    fn the_key_inside_a_marked_run_removes_its_marks() {
        assert_eq!(
            toggle_mark("**two words**", None, 9, "**"),
            mark_edit(0..13, "two words", 0..9, 7),
            "a run of two words, the caret inside the second"
        );
        assert_eq!(
            toggle_mark("**two words**", None, 2, "**"),
            mark_edit(0..13, "two words", 0..9, 0),
            "right after the opening mark"
        );
        assert_eq!(
            toggle_mark("say `let x = 1` now", None, 9, "`"),
            mark_edit(4..15, "let x = 1", 4..13, 8)
        );
        assert_eq!(
            toggle_mark("`a ** b` and **c d**", None, 17, "**"),
            mark_edit(13..20, "c d", 13..16, 15),
            "stars inside code are text"
        );
    }

    #[test]
    fn a_line_that_is_unclear_keeps_the_word_behavior() {
        assert_eq!(
            toggle_mark("**a word b**", None, 6, "*"),
            mark_edit(4..8, "*word*", 5..9, 7),
            "italic inside bold: no italic run here, the word is wrapped"
        );
        assert_eq!(
            toggle_mark("* item one", None, 8, "*"),
            mark_edit(7..10, "*one*", 8..11, 9),
            "a bullet's star is no mark"
        );
        assert_eq!(
            toggle_mark("2 * 3 and a*b*c", None, 12, "*"),
            mark_edit(11..14, "b", 11..12, 11),
            "a star between spaces is text; the pair around b is a run"
        );
        assert_eq!(
            toggle_mark("a*b **x y**", None, 8, "*"),
            mark_edit(8..9, "*y*", 9..10, 9),
            "one star alone: unclear, the word is wrapped"
        );
        assert_eq!(
            toggle_mark("**open and no end", None, 8, "**"),
            mark_edit(7..10, "**and**", 9..12, 10),
            "an opening mark alone"
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
            link_edit("a  b", None, 2),
            e(2..2, "[]()", 3),
            "no word: an empty link, the caret in the brackets"
        );
        assert_eq!(link_edit("", None, 0), e(0..0, "[]()", 1));
        assert_eq!(
            link_edit("see https://x.y now", Some(4..15), 15),
            e(4..15, "[](https://x.y)", 5),
            "a selected address becomes the target, the caret in the brackets"
        );
    }

    #[test]
    fn a_link_takes_the_word_or_the_address_under_the_caret() {
        let e = |replace: std::ops::Range<usize>, text: &str, caret: usize| MarkEdit {
            replace,
            text: text.to_string(),
            inner: caret..caret,
            caret,
        };
        assert_eq!(
            link_edit("see link now", None, 6),
            e(4..8, "[link]()", 11),
            "inside the word: its text, the caret in the parentheses"
        );
        assert_eq!(
            link_edit("see link", None, 8),
            e(4..8, "[link]()", 11),
            "at the word's end"
        );
        assert_eq!(link_edit("ab", None, 1), e(0..2, "[ab]()", 5));
        assert_eq!(
            link_edit("see https://x.y now", None, 9),
            e(4..15, "[](https://x.y)", 5),
            "on an address: the target, the caret in the brackets"
        );
        assert_eq!(
            link_edit("see https://x.y now", None, 15),
            e(4..15, "[](https://x.y)", 5),
            "at the address's end"
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

    /// The refusal for a Tab over the whole of `region`, found in
    /// `source` by its text.
    fn refusal_in(source: &str, region: &str) -> Option<NestRefusal> {
        let start = source.find(region).expect("the region is in the source");
        nest_refusal(source, start, start + region.len())
    }

    #[test]
    fn a_numbered_item_two_columns_in_is_a_sibling_and_may_nest() {
        assert_eq!(
            refusal_in("1. a\n  1. b\n", "  1. b"),
            None,
            "`1.` has three columns"
        );
        assert_eq!(
            refusal_in("1. a\n   1. b\n", "   1. b"),
            Some(NestRefusal::AlreadyNested)
        );
        assert_eq!(
            refusal_in("- a\n  - b\n", "  - b"),
            Some(NestRefusal::AlreadyNested)
        );
        assert_eq!(
            refusal_in("- [ ] a\n  - [ ] b\n", "  - [ ] b"),
            Some(NestRefusal::AlreadyNested),
            "the task box is the item's text"
        );
        assert_eq!(
            refusal_in("10. a\n   10. b\n", "   10. b"),
            None,
            "`10.` has four"
        );
    }

    #[test]
    fn a_fenced_block_and_the_frontmatter_hold_no_item_above() {
        assert_eq!(
            refusal_in("a\n\n```\n- one\n- two\n```\n\n- real\n", "- real"),
            Some(NestRefusal::NoItemAbove),
            "a markdown file showing markdown"
        );
        assert_eq!(
            refusal_in("---\ntags:\n  - a\n  - b\n---\n\n- item\n", "- item"),
            Some(NestRefusal::NoItemAbove),
            "a YAML frontmatter"
        );
        assert_eq!(
            refusal_in("- a\n\n~~~\ntext\n~~~\n\n- b\n", "- b"),
            Some(NestRefusal::NoItemAbove),
            "a fence between two items ended the first list"
        );
        assert_eq!(
            refusal_in("- a\n  ```\n  - not one\n  ```\n- b\n", "- b"),
            None,
            "a fence inside the item above is the item's own text"
        );
    }

    #[test]
    fn the_look_for_the_item_above_stops_after_sixty_four_kilobytes() {
        let quoted = "> text\n".repeat(20);
        assert_eq!(refusal_in(&format!("- a\n{quoted}- b\n"), "- b"), None);
        let quoted = "> text\n".repeat(ABOVE_CAP / 7 + 2);
        assert_eq!(
            refusal_in(&format!("- a\n{quoted}- b\n"), "- b"),
            Some(NestRefusal::NoItemAbove)
        );
    }

    #[test]
    fn an_item_nests_under_the_item_above_it() {
        assert_eq!(refusal_in("- a\n- b\n", "- b"), None);
        assert_eq!(refusal_in("1. a\n1. b\n", "1. b"), None);
        assert_eq!(refusal_in("- a\n\n- b\n", "- b"), None, "a loose list");
        assert_eq!(
            refusal_in("- a\n  more of a\n- b\n", "- b"),
            None,
            "the item's own text stands between"
        );
        assert_eq!(
            refusal_in("- a\nlazy text of a\n- b\n", "- b"),
            None,
            "a lazy line still belongs to the item"
        );
        assert_eq!(
            refusal_in("- a\n    - b\n- c\n", "- c"),
            None,
            "under a deeper item, one level below it"
        );
        assert_eq!(
            refusal_in("- a\n- b\n- c\n", "- b\n- c"),
            None,
            "a selection"
        );
    }

    #[test]
    fn an_item_with_nothing_above_does_not_nest() {
        let no = Some(NestRefusal::NoItemAbove);
        assert_eq!(
            refusal_in("- a\n- b\n", "- a\n- b"),
            no,
            "the file's first lines"
        );
        assert_eq!(
            refusal_in("Why a code block:\n\n- a\n- b\n- c\n", "- a\n- b\n- c"),
            no,
            "the user's file of 21/09/2026: four spaces after a blank line are code"
        );
        assert_eq!(
            refusal_in("A paragraph.\n- a\n", "- a"),
            no,
            "indented, the item would join the paragraph as text"
        );
        assert_eq!(
            refusal_in("- a\n\nA paragraph.\n\n- b\n", "- b"),
            no,
            "a paragraph at the margin ended the list above"
        );
    }

    #[test]
    fn an_item_already_a_level_down_does_not_nest_again() {
        assert_eq!(
            refusal_in("- a\n    - b\n", "    - b"),
            Some(NestRefusal::AlreadyNested)
        );
        assert_eq!(
            refusal_in("- a\n\t- b\n", "\t- b"),
            Some(NestRefusal::AlreadyNested),
            "a tab is a level too"
        );
    }

    #[test]
    fn the_rule_holds_inside_a_quote() {
        assert_eq!(refusal_in("> - a\n> - b\n", "> - b"), None);
        assert_eq!(
            refusal_in("> - a\n>\n> - b\n", "> - b"),
            None,
            "a loose list inside the quote"
        );
        assert_eq!(
            refusal_in("> - a\n", "> - a"),
            Some(NestRefusal::NoItemAbove)
        );
        assert_eq!(
            refusal_in("> - a\n\n> - b\n", "> - b"),
            Some(NestRefusal::NoItemAbove),
            "a bare blank line ended the quote above"
        );
        assert_eq!(
            refusal_in("- a\n> - b\n", "> - b"),
            Some(NestRefusal::NoItemAbove),
            "an item outside the quote is not above it"
        );
        assert_eq!(
            refusal_in("> - a\n>     - b\n", ">     - b"),
            Some(NestRefusal::AlreadyNested)
        );
    }

    #[test]
    fn a_quoted_line_takes_its_indent_after_the_quote_marks() {
        let four = IndentUnit::Spaces(4);
        let (text, edits) = reindent_markdown("> - b", &four, false);
        assert_eq!((text.as_str(), edits), (">     - b", vec![(2, 4)]));
        let (text, edits) = reindent_markdown("> > - b", &four, false);
        assert_eq!((text.as_str(), edits), ("> >     - b", vec![(4, 4)]));
        let (text, edits) = reindent_markdown("- b\n\n> c\n>", &four, false);
        assert_eq!(
            text, "    - b\n\n>     c\n>",
            "blank lines stay, quoted or not"
        );
        assert_eq!(edits, vec![(0, 4), (0, 0), (2, 4), (1, 0)]);

        let depths: Vec<u8> = crate::doc::markdown::parse("> - a\n>     - b\n")
            .blocks
            .iter()
            .filter_map(|b| match b.kind {
                crate::doc::model::BlockKind::ListItem { depth, .. } => Some(depth),
                _ => None,
            })
            .collect();
        assert_eq!(depths.len(), 2);
        assert!(depths[1] > depths[0], "the item nests inside the quote");
    }

    #[test]
    fn a_quoted_line_gives_its_indent_back_and_keeps_its_own_space() {
        let four = IndentUnit::Spaces(4);
        let (text, edits) = reindent_markdown(">     - b", &four, true);
        assert_eq!((text.as_str(), edits), ("> - b", vec![(2, -4)]));
        let (text, edits) = reindent_markdown("> - b", &four, true);
        assert_eq!((text.as_str(), edits), ("> - b", vec![(2, 0)]));
        let (text, _) = reindent_markdown("    > - b", &four, true);
        assert_eq!(
            text, "> - b",
            "four columns before the mark are no quote: cut at the margin"
        );
    }

    #[test]
    fn lines_that_are_not_all_items_indent_freely() {
        assert_eq!(refusal_in("text\n- a\n", "text\n- a"), None);
        assert_eq!(refusal_in("plain\n", "plain"), None);
        assert_eq!(refusal_in("\n\n", "\n"), None, "nothing to indent");
    }

    #[test]
    fn the_unit_follows_the_dominant_indentation() {
        assert_eq!(
            indent_unit("all:\n\tcc -o all main.c\n\tstrip all\n", false),
            IndentUnit::Tab,
            "tab-led lines dominate a Makefile"
        );
        assert_eq!(
            indent_unit("- a\n  - b\n  - c\n", false),
            IndentUnit::Spaces(2),
            "two-space nesting reads as a two-space step"
        );
        assert_eq!(
            indent_unit("fn main() {\n    if x {\n        y();\n    }\n}\n", false),
            IndentUnit::Spaces(4),
            "four-space blocks read as a four-space step"
        );
        assert_eq!(
            indent_unit("\ta\n\tb\n  c\n", false),
            IndentUnit::Tab,
            "tabs outnumber spaces"
        );
        assert_eq!(
            indent_unit("plain\nlines\n", false),
            IndentUnit::Tab,
            "no evidence answers a tab"
        );
        assert_eq!(indent_unit("", false), IndentUnit::Tab);
    }

    #[test]
    fn a_markdown_file_without_evidence_indents_with_four_spaces() {
        assert_eq!(indent_unit("", true), IndentUnit::Spaces(4), "a new file");
        assert_eq!(
            indent_unit("# Title\n\nA paragraph.\n\n- a\n- b\n", true),
            IndentUnit::Spaces(4),
            "text at the margin is no evidence"
        );
    }

    #[test]
    fn a_markdown_file_keeps_the_indent_it_already_has() {
        assert_eq!(
            indent_unit("- a\n  - b\n  - c\n", true),
            IndentUnit::Spaces(2),
            "two-space nesting stays two"
        );
        assert_eq!(
            indent_unit("- a\n\t- b\n\t- c\n", true),
            IndentUnit::Tab,
            "a file nested with tabs stays on tabs"
        );
    }

    #[test]
    fn four_spaces_nest_under_a_numbered_item() {
        let (text, _) = reindent("1. b", &indent_unit("1. a\n1. b\n", true), false);
        assert_eq!(text, "    1. b");
        let depths = |source: String| -> Vec<u8> {
            crate::doc::markdown::parse(source)
                .blocks
                .iter()
                .filter_map(|b| match b.kind {
                    crate::doc::model::BlockKind::ListItem { depth, .. } => Some(depth),
                    _ => None,
                })
                .collect()
        };
        let nested = depths(format!("1. a\n{text}\n"));
        assert_eq!(nested.len(), 2);
        assert!(nested[1] > nested[0], "four spaces nest the item");
        let flat = depths("1. a\n  1. b\n".to_string());
        assert_eq!(flat[0], flat[1], "two would not, which is why four");
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

    fn case_of(text: &str) -> String {
        let edit = case_edit(text, Some(0..text.len()), text.len(), true).expect("a change");
        assert_eq!(edit.replace, 0..text.len());
        assert_eq!(
            edit.inner,
            0..edit.text.len(),
            "the new text stays selected"
        );
        assert_eq!(
            edit.caret,
            edit.text.len(),
            "the caret at the selection's end"
        );
        edit.text
    }

    #[test]
    fn the_case_key_cycles_a_word() {
        assert_eq!(case_of("hello"), "Hello");
        assert_eq!(case_of("Hello"), "HELLO");
        assert_eq!(case_of("HELLO"), "hello");
        assert_eq!(case_of("hElLo"), "hello", "a mixed word goes to lower case");
    }

    #[test]
    fn the_case_key_cycles_a_sentence() {
        assert_eq!(case_of("the quick brown fox"), "The Quick Brown Fox");
        assert_eq!(case_of("The Quick Brown Fox"), "THE QUICK BROWN FOX");
        assert_eq!(case_of("THE QUICK BROWN FOX"), "the quick brown fox");
        assert_eq!(case_of("Hello world"), "hello world");
        assert_eq!(case_of("well-known, don't"), "Well-known, Don't");
    }

    #[test]
    fn the_case_key_changes_each_line_of_a_selection() {
        assert_eq!(case_of("one two\nthree four"), "One Two\nThree Four");
        assert_eq!(
            case_of("One Two\nthree four"),
            "one two\nthree four",
            "the selection as a whole decides the form"
        );
    }

    #[test]
    fn the_case_key_spares_links_and_inline_code() {
        assert_eq!(
            case_of("see [the page](https://x.y/Path) and `fooBar` now"),
            "See [The Page](https://x.y/Path) And `fooBar` Now"
        );
        assert_eq!(
            case_of("See [The Page](https://x.y/Path) And `fooBar` Now"),
            "SEE [THE PAGE](https://x.y/Path) AND `fooBar` NOW"
        );
    }

    /// An HTML tag keeps its letters, its attributes with it, and so
    /// does a formula as the markdown parser reads one; the dollar sign
    /// of a price, an escaped one, a dollar with no closer and a lone
    /// `<` are text.
    #[test]
    fn the_case_key_spares_tags_and_formulas() {
        assert_eq!(
            case_of("see <img src=\"images/Photo.png\"> here"),
            "See <img src=\"images/Photo.png\"> Here"
        );
        assert_eq!(case_of("<b>bold</b> text"), "<b>Bold</b> Text");
        assert_eq!(
            case_of("the formula $\\Delta x$ holds"),
            "The Formula $\\Delta x$ Holds"
        );
        assert_eq!(case_of("block $$\\Delta$$ done"), "Block $$\\Delta$$ Done");
        assert_eq!(case_of("price $5-$10 each"), "Price $5-$10 Each");
        assert_eq!(case_of("a < b and c > d"), "A < B And C > D");
        assert_eq!(case_of("cost \\$x and $y"), "Cost \\$X And $Y");
    }

    /// A `$` that opens but meets no closer is text, and the search
    /// goes on from the `$` that could not close, which may open: the
    /// page reads the later formula, so the key keeps it. A kind of
    /// dollar sign is given up only when none is left on the paragraph.
    #[test]
    fn a_formula_after_a_dollar_that_cannot_close_is_kept() {
        assert_eq!(
            case_of("spend $a then $delta x$ here"),
            "Spend $A Then $delta x$ Here"
        );
        assert_eq!(
            case_of("pay $5-$10 for $mass x$ now"),
            "Pay $5-$10 For $mass x$ Now"
        );
        assert_eq!(
            case_of("prices $5, $6, and $big x$ today"),
            "Prices $5, $6, And $big x$ Today"
        );
        assert_eq!(
            case_of("a $x$5 and $big y$ here"),
            "A $X$5 And $big y$ Here"
        );
        assert_eq!(case_of("see $x and $y$ now"), "See $X And $y$ Now");
        assert_eq!(
            case_of("one $x then $ and y$ end"),
            "One $X Then $ And Y$ End",
            "a $ that cannot close makes the opener text, as the parser reads it"
        );
        assert_eq!(case_of("none $x and $y and z"), "None $X And $Y And Z");
    }

    /// The hunt's claim 8: a display formula in its usual form, on its
    /// own lines, opens with a line break after its signs, as the
    /// parser reads it, and with a space after them too. A lone `$`
    /// inside makes the opener text (the parser's spec test 10).
    #[test]
    fn a_display_formula_opens_with_whitespace_after_its_signs() {
        assert_eq!(case_of("see $$\nE=mc^2\n$$ now"), "See $$\nE=mc^2\n$$ Now");
        assert_eq!(case_of("see $$ x $$ now"), "See $$ x $$ Now");
        assert_eq!(case_of("a $$ b $ c $$ d"), "A $$ B $ C $$ D");
    }

    /// The hunt's claim 9: an opener that is text hides nothing up to
    /// the next `$`: a code span, an address or a tag between two
    /// prices is kept, and the dollar signs between, each unable to
    /// open, are stepped over.
    #[test]
    fn the_text_between_two_prices_is_visited() {
        assert_eq!(
            case_of("costs $5 or `foo bar` or $6 ok"),
            "Costs $5 Or `foo bar` Or $6 Ok"
        );
        assert_eq!(
            case_of("pay $5 at [the shop](http://shop.example) or $6"),
            "Pay $5 At [The Shop](http://shop.example) Or $6"
        );
        assert_eq!(
            case_of("pay $5 <b>now</b> or $6"),
            "Pay $5 <b>Now</b> Or $6"
        );
    }

    /// The inline rules are the parser's (pulldown-cmark 0.13.4, its
    /// spec tests 10 and 21): a `$` at the opener's brace level that
    /// cannot close makes the opener text, so what the page shows as
    /// text changes; braces hold an inner `$`; a `}` below the opener's
    /// level ends it; a formula the parser pairs whose closer precedes
    /// a digit is the page's currency text, the scan going on after its
    /// closer.
    #[test]
    fn the_inline_rules_follow_the_parser() {
        assert_eq!(case_of("$x $ x$"), "$X $ X$", "the parser's spec test 10");
        assert_eq!(
            case_of("see $\\text{Hello $x$ there!}$ now"),
            "See $\\text{Hello $x$ there!}$ Now",
            "the parser's spec test 21"
        );
        assert_eq!(
            case_of("see $$\n\\text{Hello $x^2$}\n$$ now"),
            "See $$\n\\text{Hello $x^2$}\n$$ Now"
        );
        assert_eq!(
            case_of("$a } b$ c"),
            "$A } B$ C",
            "a brace below the opener's level"
        );
        assert_eq!(
            case_of("from $5-$10 to x$ here"),
            "From $5-$10 To X$ Here",
            "the currency text, then a $ that cannot open"
        );
        assert_eq!(
            case_of("a $x {$y$} z$ b"),
            "A $x {$y$} z$ B",
            "braces inside a formula"
        );
        assert_eq!(
            case_of("a $x {$y$} z b"),
            "A $X {$y$} Z B",
            "the inner pair stands when the outer does not close"
        );
    }

    /// A tag is what CommonMark reads as one: a name, attributes, a
    /// closing tag, a comment. Text between angle brackets that is none
    /// of these is text on the page, and its words change.
    #[test]
    fn text_between_angle_brackets_is_kept_only_as_a_tag() {
        assert_eq!(
            case_of("say <hello, world!> now"),
            "Say <Hello, World!> Now"
        );
        assert_eq!(case_of("say <hello> now"), "Say <hello> Now");
        assert_eq!(
            case_of("a <span class=\"x\" data-k=v> b"),
            "A <span class=\"x\" data-k=v> B"
        );
        assert_eq!(case_of("a <br/> b"), "A <br/> B");
        assert_eq!(case_of("a </div > b"), "A </div > B");
        assert_eq!(case_of("a <!-- a note --> b"), "A <!-- a note --> B");
        assert_eq!(case_of("a <3 b"), "A <3 B");
        assert_eq!(
            case_of("a <x y=\"z w> b"),
            "A <X Y=\"z W> B",
            "an unterminated value is no tag"
        );
    }

    /// The key cuts its paragraphs where the parser cuts its blocks: a
    /// line that starts a block quote, a heading, a list item, a fence
    /// or a thematic break ends the paragraph before it, so a `$` above
    /// such a line pairs with none below it. A quote's own lines stay
    /// one paragraph.
    #[test]
    fn a_paragraph_ends_where_a_block_begins() {
        assert_eq!(case_of("$0\n>a$<"), "$0\n>A$<");
        assert_eq!(case_of("x $a\n- b$ c"), "X $A\n- B$ C");
        assert_eq!(case_of("x $a\n# b$ c"), "X $A\n# B$ C");
        assert_eq!(case_of("x $a\n---\nb$ c"), "X $A\n---\nB$ C");
        assert_eq!(
            case_of("> $a\n> b$ c"),
            "> $a\n> b$ C",
            "a quote's lines are one paragraph"
        );
        assert_eq!(
            case_of("- $a\n  b$ c"),
            "- $a\n  b$ C",
            "an item's indented continuation"
        );
    }

    /// A link's address is the parser's: after a `](` whose `]` closes
    /// a `[`, a destination without whitespace or between angle
    /// brackets, an optional title; anything else is text and changes.
    #[test]
    fn a_link_address_has_the_parser_s_shape() {
        assert_eq!(case_of("a](b c)"), "A](b C)", "no bracket before it");
        assert_eq!(case_of("[]](a)"), "[]](A)", "the bracket closed before it");
        assert_eq!(case_of("[[a]](b)"), "[[A]](b)", "nested brackets");
        assert_eq!(
            case_of("[a](b c)"),
            "[A](b C)",
            "a space in the destination"
        );
        assert_eq!(
            case_of("[a](b\n) c"),
            "[A](b\n) C",
            "a line break before the parenthesis"
        );
        assert_eq!(
            case_of("[a](\n b) c"),
            "[A](\n b) C",
            "whitespace before the destination"
        );
        assert_eq!(case_of("[a]() b"), "[A]() B", "an empty destination");
        assert_eq!(
            case_of("-[](](a b)"),
            "-[](](A B)",
            "a ]( that is no link closes its bracket"
        );
        assert_eq!(case_of("[a](b \"t x\")"), "[A](b \"t x\")", "a title");
        assert_eq!(
            case_of("[a](<b c>)"),
            "[A](<b c>)",
            "a destination in brackets"
        );
        assert_eq!(case_of("![alt](p.png)"), "![Alt](p.png)");
    }

    /// The key cuts its paragraphs where the parser cuts its blocks,
    /// once more: a list's next item, whatever its number, begins a
    /// block, a numbered line after a paragraph's line does not (it is
    /// read as the paragraph's), and an indented code block is kept
    /// whole.
    #[test]
    fn a_list_s_next_item_and_an_indented_block_are_read_as_the_page_reads_them() {
        assert_eq!(case_of("1. $a\n2. b$ c"), "1. $A\n2. B$ C");
        assert_eq!(case_of("x $a\n2. b$ c"), "X $a\n2. b$ C", "a lazy line");
        assert_eq!(
            case_of("x\n\n    code here\n\ntext"),
            "X\n\n    code here\n\nText"
        );
        assert_eq!(
            case_of("    code\ntext"),
            "    code\nText",
            "the block ends before the line"
        );
        assert_eq!(
            case_of("x $a\n*\nb$ c"),
            "X $a\n*\nb$ C",
            "a bare marker after a paragraph's line is its text"
        );
        assert_eq!(
            case_of("> $a\n*\nb$ c"),
            "> $A\n*\nB$ C",
            "after a quote's line it ends the quote"
        );
        assert_eq!(
            case_of(">$a\n>$[ b"),
            ">$A\n>$[ B",
            "a $ past the quote marker has nothing before it"
        );
        assert_eq!(
            case_of("> $a\n=\nb$ c"),
            "> $a\n=\nb$ C",
            "a line of = after a quote's line is the quote's text"
        );
        assert_eq!(case_of("> $a\n--\nb$ c"), "> $a\n--\nb$ C");
        assert_eq!(
            case_of("> $a\n2. b$ c"),
            "> $A\n2. B$ C",
            "any numbered item ends the quote"
        );
        assert_eq!(
            case_of("> x\n$a\n> b$ c"),
            "> X\n$a\n> b$ C",
            "a quoted line after a lazy one is the same paragraph"
        );
    }

    /// A press on a table of two hundred thousand rows holding a `<`
    /// costs the paragraph's length, not its square: the closing `>` is
    /// searched once per region. The bound is wide for a debug build;
    /// the scan that searched from each `<` to the paragraph's end took
    /// seconds here.
    #[test]
    fn the_case_key_on_a_long_table_with_lone_angle_brackets_costs_its_length() {
        let source = "| a < b |\n".repeat(200_000);
        let started = std::time::Instant::now();
        let edit = case_edit(&source, None, 2, true);
        let took = started.elapsed();
        assert!(edit.is_some(), "the word changes");
        assert!(took.as_secs_f32() < 1.0, "one press took {took:?}");
        // A `>` near the end makes each `<` a candidate up to it: the
        // candidate is refused on its first space, not scanned whole.
        let source = format!("{source}| c > d |\n");
        let started = std::time::Instant::now();
        let edit = case_edit(&source, None, 2, true);
        let took = started.elapsed();
        assert!(edit.is_some(), "the word changes");
        assert!(
            took.as_secs_f32() < 1.0,
            "one press with a `>` took {took:?}"
        );
    }

    #[test]
    fn the_case_key_takes_the_word_at_the_caret() {
        let edit = case_edit("say hello there", None, 6, true).expect("the word");
        assert_eq!(edit.replace, 4..9);
        assert_eq!(edit.text, "Hello");
        assert_eq!(edit.inner, 4..9);
        assert_eq!(edit.caret, 6, "the caret keeps its place in the word");
        assert!(
            case_edit("say  there", None, 4, true).is_none(),
            "between two spaces there is no word"
        );
    }

    #[test]
    fn the_word_at_the_caret_is_whole_across_a_joiner() {
        let source = "\u{645}\u{6cc}\u{200c}\u{631}\u{648}\u{645} x";
        assert_eq!(word_at(source, 9), Some(0..13), "from the second half");
        assert_eq!(word_at(source, 2), Some(0..13), "from the first half");
    }

    #[test]
    fn the_caret_stays_after_the_same_letters_when_their_bytes_change() {
        // The dotless i is two bytes and its capital I one.
        let edit = case_edit("ışık", None, 2, false).expect("the word");
        assert_eq!(edit.text, "Işık");
        assert!(edit.text.is_char_boundary(edit.caret));
        assert_eq!(edit.caret, 1, "after the I");
        // The dotted capital I lowers to an i with a combining dot, three bytes.
        let edit = case_edit("İSTANBUL", None, 2, false).expect("the word");
        assert_eq!(edit.text, "i\u{307}stanbul");
        assert!(edit.text.is_char_boundary(edit.caret));
        assert_eq!(edit.caret, 3, "after the i and its dot");
        // A sharp s becomes two letters in upper case.
        let edit = case_edit("Straße", None, 6, false).expect("the word");
        assert_eq!(edit.text, "STRASSE");
        assert_eq!(edit.caret, 6, "after both letters");
    }

    #[test]
    fn a_single_capital_letter_cycles_with_its_lower_case() {
        assert_eq!(case_of("A"), "a");
        assert_eq!(case_of("a"), "A");
        assert!(
            case_edit("123", Some(0..3), 3, true).is_none(),
            "no letter to change"
        );
    }

    #[test]
    fn the_caret_inside_inline_code_or_an_address_changes_nothing() {
        assert!(
            case_edit("in `fooBar` code", None, 7, true).is_none(),
            "the word at the caret is inline code"
        );
        assert!(
            case_edit("a [link](https://x.y/Path) here", None, 23, true).is_none(),
            "the word at the caret is in the address"
        );
        let edit =
            case_edit("in `fooBar` code", None, 7, false).expect("a code file spares nothing");
        assert_eq!(edit.text, "foobar");
    }

    #[test]
    fn an_autolink_and_a_reference_definition_keep_their_address() {
        assert_eq!(
            case_of("see <https://x.y/Path> now"),
            "See <https://x.y/Path> Now"
        );
        assert_eq!(
            case_of("[Ref]: https://x.y/Path"),
            "[REF]: https://x.y/Path"
        );
    }

    #[test]
    fn an_address_with_balanced_parentheses_is_kept_whole() {
        assert_eq!(
            case_of("a [w](https://en.wikipedia.org/wiki/Foo_(Bar)_Baz) b"),
            "A [W](https://en.wikipedia.org/wiki/Foo_(Bar)_Baz) B"
        );
    }

    #[test]
    fn a_selection_begun_inside_the_address_spares_it() {
        let source = "see [x](https://x.y/Path) then\n";
        let from = source.find("x.y").unwrap();
        let to = source.len() - 1;
        let edit = case_edit(source, Some(from..to), to, true).expect("a change");
        assert_eq!(edit.text, "x.y/Path) Then");
    }

    #[test]
    fn a_backtick_pairs_in_its_paragraph_only() {
        assert_eq!(
            case_of("one `Two\n\nThree` Four"),
            "one `two\n\nthree` four",
            "a lone backtick in each paragraph is no code"
        );
        assert_eq!(
            case_of("esc \\`not Code\\` Text"),
            "esc \\`not code\\` text",
            "an escaped backtick is no mark"
        );
        assert_eq!(
            case_of("fn Main() {}\n```\nSome `x` Text"),
            "fn main() {}\n```\nsome `x` text",
            "a fence with no closer in the paragraph is no code, the span after it is"
        );
    }

    #[test]
    fn a_code_file_spares_nothing() {
        let source = "let s = \"`\"; let X = 1; let t = \"`\";";
        let edit = case_edit(source, Some(0..source.len()), source.len(), false).expect("a change");
        assert_eq!(edit.text, "let s = \"`\"; let x = 1; let t = \"`\";");
    }

    #[test]
    fn the_final_sigma_and_a_decomposed_accent() {
        assert_eq!(case_of("ΟΔΟΣ ΚΑΙ ΣΟΦΟΣ"), "οδος και σοφος");
        assert_eq!(case_of("οδος και σοφος"), "Οδος Και Σοφος");
        let source = "an e\u{301}cole here";
        let at = source.find("le").unwrap();
        let edit = case_edit(source, None, at, true).expect("the word");
        assert_eq!(
            edit.replace,
            3..3 + "e\u{301}cole".len(),
            "the accent is part of the word"
        );
        assert_eq!(edit.text, "E\u{301}cole");
    }

    #[test]
    fn an_unclosed_address_costs_its_line_only() {
        let source = "see [x](y and more text\n".repeat(40_000);
        let start = std::time::Instant::now();
        let edit = case_edit(&source, Some(0..source.len()), source.len(), true).expect("a change");
        assert!(edit.text.starts_with("See [X](y And More Text\n"));
        let took = start.elapsed();
        println!("the case key on 960 KB of unclosed links: {took:?}");
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }
}
