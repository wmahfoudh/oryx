//! Document model: the visual-free representation every later stage consumes.

use std::ops::Range;
use std::sync::Arc;

use crate::style::highlight::SyntaxRole;

#[derive(Debug, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// The text the document was parsed from; block and span ranges index
    /// into it. Markdown copy slices it directly. Shared with the parse
    /// and highlight workers, which clone the `Arc`, never the text.
    /// For a book this is the synthetic source: the chapters' text in
    /// spine order, built by the walker, no markup in it.
    pub source: Arc<str>,
    /// The `<details>` regions, indexed by the group id blocks carry.
    pub details: Vec<DetailsGroup>,
    /// A book's `dc:title`, shown in the window title; None for files.
    pub title: Option<String>,
    /// A book's anchor map: `path` and `path#id` to source offsets, the
    /// chapters and every element id. Links and the TOC resolve through
    /// it; a delivery replaces it whole. Empty for files.
    pub anchors: std::collections::HashMap<String, usize>,
    /// The key position memory files a book under: `dc:identifier`, or
    /// the canonical path when the metadata has none. None for files.
    pub book_id: Option<String>,
    /// True for code and unknown files, whose single block is the whole
    /// source. Layout shows such a page as code, not as a page
    /// containing code: the block draws without its panel.
    pub code_file: bool,
    /// True for plain text files. Their rows are uniform: blank lines
    /// are real rows inside the blocks, and layout adds no gap between
    /// blocks.
    pub plain_file: bool,
}

impl Default for Document {
    fn default() -> Document {
        Document {
            blocks: Vec::new(),
            source: Arc::from(""),
            details: Vec::new(),
            title: None,
            anchors: std::collections::HashMap::new(),
            book_id: None,
            code_file: false,
            plain_file: false,
        }
    }
}

/// One `<details>` region. Blocks name their innermost group; nesting
/// nests through `parent`.
#[derive(Debug, Clone, PartialEq)]
pub struct DetailsGroup {
    pub parent: Option<u16>,
    pub open: bool,
}

impl Document {
    /// Whether layout shows this block: every enclosing details group
    /// open. A summary row carries its enclosing group, not its own, so
    /// the toggle of a closed group stays visible.
    pub fn block_visible(&self, index: usize) -> bool {
        let mut chain = self.blocks[index].details;
        while let Some(g) = chain {
            let group = &self.details[g as usize];
            if !group.open {
                return false;
            }
            chain = group.parent;
        }
        true
    }

    /// Flips one group's fold state.
    pub fn toggle_details(&mut self, group: u16) {
        let g = &mut self.details[group as usize];
        g.open = !g.open;
    }

    /// The block holding a source offset: the last one starting at or
    /// before it. Book anchors resolve to blocks through this.
    pub fn block_at_offset(&self, offset: usize) -> Option<usize> {
        let after = self.blocks.partition_point(|b| b.range.start <= offset);
        after.checked_sub(1)
    }

    /// Whether justification applies: prose documents, markdown and
    /// books. Code and text files are line-oriented and ignore the
    /// setting everywhere it is read.
    pub fn justifiable(&self) -> bool {
        !self.code_file && !self.plain_file
    }

    /// Opens every closed group enclosing a block, answering whether
    /// anything changed. Navigation into a folded region calls this
    /// before scrolling there.
    pub fn reveal(&mut self, block: usize) -> bool {
        let mut chain = self.blocks[block].details;
        let mut changed = false;
        while let Some(g) = chain {
            let group = &mut self.details[g as usize];
            if !group.open {
                group.open = true;
                changed = true;
            }
            chain = group.parent;
        }
        changed
    }
}

/// Slices a model range out of the source. Model offsets are `u32` bytes
/// created at shaper cluster boundaries or newline splits; the asserts
/// catch any seam that lets one land inside a multi-byte character.
pub(crate) fn slice<'a>(source: &'a str, range: &Range<u32>) -> &'a str {
    let (start, end) = (range.start as usize, range.end as usize);
    debug_assert!(
        source.is_char_boundary(start) && source.is_char_boundary(end),
        "model range {start}..{end} off a char boundary"
    );
    &source[start..end]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// 0 when the block is not inside a blockquote.
    pub quote_depth: u8,
    /// Set when the enclosing quote is a GitHub alert.
    pub alert: Option<AlertKind>,
    /// Byte range of the block's content in `Document::source`. Line-prefix
    /// markers (`#`, `>`, list bullets) may sit before `start` on the same
    /// line; empty when the block has no source form.
    pub range: Range<usize>,
    /// Set inside `<p align="center">` or `<div align="center">`.
    pub centered: bool,
    /// Innermost enclosing `<details>` group; a summary row carries the
    /// group enclosing its own, being the toggle.
    pub details: Option<u16>,
    pub kind: BlockKind,
}

impl Block {
    pub fn plain(kind: BlockKind) -> Self {
        Block {
            quote_depth: 0,
            alert: None,
            range: 0..0,
            centered: false,
            details: None,
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        spans: Vec<Span>,
        anchor: String,
    },
    Paragraph {
        spans: Vec<Span>,
    },
    CodeBlock {
        language: Option<String>,
        lines: CodeBody,
        /// One vector of styled ranges per line, computed at load.
        highlights: Vec<Vec<(Range<usize>, SyntaxRole)>>,
    },
    ListItem {
        marker: Marker,
        depth: u8,
        spans: Vec<Span>,
    },
    Table {
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Rule,
    Image {
        path: String,
        alt: String,
    },
    FootnoteDef {
        label: String,
        spans: Vec<Span>,
    },
    MathBlock {
        tex: String,
    },
    Frontmatter {
        entries: Vec<(String, String)>,
    },
    /// The visible toggle row of a `<details>` region.
    Summary {
        spans: Vec<Span>,
        group: u16,
    },
    /// The seam before a book chapter: extra space in layout, a forced
    /// page break in export. `spine` is the index of the chapter that
    /// follows; the first chapter carries no marker.
    ChapterBreak {
        spine: usize,
    },
}

/// A code block's lines as byte ranges. The ranges index the document
/// source when the body survived parsing verbatim, which fenced blocks
/// and code files always do; an indented block strips its indent, so the
/// normalized body is owned and the ranges index it instead.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodeBody {
    owned: Option<Box<str>>,
    lines: Vec<Range<u32>>,
}

impl CodeBody {
    /// Lines slicing the source directly.
    pub(crate) fn verbatim(lines: Vec<Range<u32>>) -> CodeBody {
        CodeBody { owned: None, lines }
    }

    /// Lines over an owned body; ranges index the body.
    pub fn from_text(text: &str) -> CodeBody {
        let base = text.as_ptr() as usize;
        let mut lines: Vec<Range<u32>> = text
            .lines()
            .map(|line| {
                let start = (line.as_ptr() as usize - base) as u32;
                start..start + line.len() as u32
            })
            .collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        CodeBody {
            owned: Some(text.into()),
            lines,
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn is_verbatim(&self) -> bool {
        self.owned.is_none()
    }

    pub fn line<'a>(&'a self, source: &'a str, index: usize) -> &'a str {
        let base = self.owned.as_deref().unwrap_or(source);
        slice(base, &self.lines[index])
    }

    /// The source byte range of a line, only when the body slices the
    /// source directly; an owned body has no source coordinates.
    pub fn line_range(&self, index: usize) -> Option<Range<usize>> {
        if self.owned.is_some() {
            return None;
        }
        let range = &self.lines[index];
        Some(range.start as usize..range.end as usize)
    }

    pub fn iter<'a>(&'a self, source: &'a str) -> impl Iterator<Item = &'a str> {
        let base = self.owned.as_deref().unwrap_or(source);
        self.lines.iter().map(move |range| slice(base, range))
    }

    /// Splices a verbatim body after an edit: `old_lines` of the body
    /// were replaced by `new_lines` of the already-edited `source`, and
    /// the bytes past the touched region shifted by `delta`. Touched
    /// entries rebuild from the source, entries after shift; an edit
    /// reaching past the last entry rebuilds the suffix from the first
    /// touched line, where `keep_trailing` decides whether trailing
    /// blank lines stay as rows (text files) or pop (code files).
    /// Declines on an owned body, whose ranges have no source
    /// coordinates to splice.
    pub fn splice(
        &mut self,
        source: &str,
        old_lines: Range<usize>,
        new_lines: Range<usize>,
        delta: isize,
        keep_trailing: bool,
    ) -> bool {
        if self.owned.is_some() {
            return false;
        }
        let len = self.lines.len();
        // Bytes before the first touched line are untouched by the
        // edit, so its recorded start still holds: one byte past the
        // previous entry's terminator.
        let from = old_lines.start.min(len);
        let start = if from == 0 {
            0
        } else {
            self.lines[from - 1].end as usize + 1
        };
        let start = start.min(source.len());
        // The pieces between terminators from `start`: `split` keeps
        // empty lines as real entries, which the region form needs.
        let scan = |region: &str| -> Vec<Range<u32>> {
            let mut at = start as u32;
            region
                .split('\n')
                .map(|piece| {
                    let range = at..at + piece.len() as u32;
                    at = range.end + 1;
                    range
                })
                .collect()
        };
        if old_lines.end >= len {
            // The edit reaches the last entry or past it (the phantom
            // line after a trailing terminator, a code file's popped
            // tail): rebuild the suffix. The final piece after a
            // closing terminator is that phantom, never a row.
            let mut tail = scan(&source[start..]);
            if source.ends_with('\n') || start == source.len() {
                tail.pop();
            }
            if !keep_trailing {
                while tail.last().is_some_and(|l| l.is_empty()) {
                    tail.pop();
                }
            }
            self.lines.truncate(from);
            self.lines.append(&mut tail);
            return true;
        }
        // Mid-file: the touched region ends one terminator before the
        // next preserved entry's shifted start, so its pieces are
        // exactly the new touched lines.
        let next = (self.lines[old_lines.end].start as isize + delta) as usize;
        let pieces = scan(&source[start..next - 1]);
        debug_assert_eq!(
            pieces.len(),
            new_lines.len(),
            "the region holds the touched lines"
        );
        for range in &mut self.lines[old_lines.end..] {
            range.start = range.start.wrapping_add_signed(delta as i32);
            range.end = range.end.wrapping_add_signed(delta as i32);
        }
        self.lines.splice(old_lines, pieces);
        true
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Marker {
    Bullet,
    Number(u64),
    Task {
        checked: bool,
    },
    /// Indented like an item but drawing no marker; definition bodies.
    None,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// Inline image carried by a span; the span text is the alt fallback.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SpanImage {
    pub src: String,
    /// Pixel size from HTML attributes; None uses the natural size.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Vertical script position of a span.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum SpanScript {
    #[default]
    None,
    Sub,
    Sup,
    /// Reduced size on the unmoved baseline; `<small>`.
    Small,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Span {
    /// Owned text when parsing transformed it away from the source slice
    /// (smart punctuation, entities, emoji shortcodes, stripped HTML,
    /// merges over gaps) and for synthesized spans; `None` when `range`
    /// slices the text out of the source, decided once by `seal`. Read
    /// through `text`.
    owned: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    /// Drawn under the baseline; `<u>` and `<ins>`.
    pub underline: bool,
    /// Highlight background behind the run; `<mark>`.
    pub mark: bool,
    pub code: bool,
    pub math: bool,
    pub script: SpanScript,
    /// Link target: a URL, a `#anchor`, or `footnote:<label>`.
    pub link: Option<String>,
    /// Set when the span is an inline image flowing with the text.
    pub image: Option<SpanImage>,
    /// Byte range of the span's origin in `Document::source`. The slice may
    /// differ from the text when parsing transformed it; empty for
    /// synthesized spans.
    pub range: Range<u32>,
}

impl Default for Span {
    fn default() -> Span {
        Span {
            owned: Some(String::new()),
            bold: false,
            italic: false,
            strike: false,
            underline: false,
            mark: false,
            code: false,
            math: false,
            script: SpanScript::None,
            link: None,
            image: None,
            range: 0..0,
        }
    }
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Span {
            owned: Some(text.into()),
            ..Span::default()
        }
    }

    /// The display text: the source slice for a verbatim span, the owned
    /// text otherwise.
    pub fn text<'a>(&'a self, source: &'a str) -> &'a str {
        match &self.owned {
            Some(text) => text,
            None => slice(source, &self.range),
        }
    }

    /// True when `range` slices the display text out of the source.
    pub fn is_verbatim(&self) -> bool {
        self.owned.is_none()
    }

    pub(crate) fn set_text(&mut self, text: impl Into<String>) {
        self.owned = Some(text.into());
    }

    pub(crate) fn clear_text(&mut self) {
        self.owned = Some(String::new());
    }

    /// The text before `seal` decided ownership; the builder reads and
    /// merges through this, and every span still owns its text then.
    pub(crate) fn raw_text(&self) -> &str {
        self.owned.as_deref().unwrap_or_default()
    }

    pub(crate) fn raw_text_mut(&mut self) -> &mut String {
        self.owned.get_or_insert_with(String::new)
    }

    /// Drops the owned text when the source slice already carries it.
    /// One byte compare per span, once, when the document is complete.
    pub(crate) fn seal(&mut self, source: &str) {
        let sealed = self.owned.as_ref().is_some_and(|text| {
            !self.range.is_empty()
                && source
                    .get(self.range.start as usize..self.range.end as usize)
                    .is_some_and(|slice| slice == text.as_str())
        });
        if sealed {
            self.owned = None;
        }
    }
}

/// Seals every span of every block against the source.
pub(crate) fn seal_blocks(blocks: &mut [Block], source: &str) {
    for block in blocks {
        match &mut block.kind {
            BlockKind::Heading { spans, .. }
            | BlockKind::Paragraph { spans }
            | BlockKind::ListItem { spans, .. }
            | BlockKind::FootnoteDef { spans, .. }
            | BlockKind::Summary { spans, .. } => {
                for span in spans {
                    span.seal(source);
                }
            }
            BlockKind::Table { header, rows } => {
                for span in header
                    .iter_mut()
                    .flatten()
                    .chain(rows.iter_mut().flatten().flatten())
                {
                    span.seal(source);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loader's construction over the edited source, the splice's
    /// referee: `str::lines` ranges, trailing blank lines popped unless
    /// kept.
    fn fresh(source: &str, keep_trailing: bool) -> Vec<Range<u32>> {
        let base = source.as_ptr() as usize;
        let mut lines: Vec<Range<u32>> = source
            .lines()
            .map(|line| {
                let start = (line.as_ptr() as usize - base) as u32;
                start..start + line.len() as u32
            })
            .collect();
        if !keep_trailing {
            while lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop();
            }
        }
        lines
    }

    fn body(source: &str, keep_trailing: bool) -> CodeBody {
        CodeBody::verbatim(fresh(source, keep_trailing))
    }

    fn ranges(body: &CodeBody, source: &str) -> Vec<Range<u32>> {
        (0..body.len())
            .map(|i| {
                let r = body.line_range(i).expect("verbatim body");
                assert!(r.end <= source.len(), "range within the source");
                r.start as u32..r.end as u32
            })
            .collect()
    }

    #[test]
    fn justification_applies_to_prose_documents() {
        let md = crate::doc::markdown::parse("a paragraph of prose\n");
        assert!(md.justifiable(), "markdown prose justifies");
        let book = Document {
            book_id: Some("book".into()),
            ..Document::default()
        };
        assert!(book.justifiable(), "book prose justifies");
        let code = crate::doc::load::code_document(Some("rust"), "let a = 1;\n");
        assert!(!code.justifiable(), "a code file ignores the setting");
        let text = crate::doc::load::text_document("plain lines\n");
        assert!(!text.justifiable(), "a text file ignores the setting");
    }

    #[test]
    fn a_mid_line_edit_rebuilds_its_line_and_shifts_the_tail() {
        let mut b = body("aaa\nbbb\nccc\n", true);
        let edited = "aaa\nbxxbb\nccc\n";
        assert!(b.splice(edited, 1..2, 1..2, 2, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn a_split_adds_a_line_in_place() {
        let mut b = body("aaa\nbbb\nccc\n", true);
        let edited = "aaa\nb\nbb\nccc\n";
        assert!(b.splice(edited, 1..2, 1..3, 1, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn a_join_removes_a_line_in_place() {
        let mut b = body("aaa\nbbb\nccc\n", true);
        let edited = "aaabbb\nccc\n";
        assert!(b.splice(edited, 0..2, 0..1, -1, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn an_edit_on_the_last_line_rebuilds_the_suffix() {
        let mut b = body("aaa\nbbb", true);
        let edited = "aaa\nbbbxx";
        assert!(b.splice(edited, 1..2, 1..2, 2, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn typing_past_the_terminator_grows_a_row() {
        // The caret after a trailing newline stands on a line no entry
        // holds yet; the edit reaches past the vector and the suffix
        // rebuild adds the row.
        let mut b = body("aaa\n", true);
        let edited = "aaa\nbbb";
        assert!(b.splice(edited, 1..2, 1..2, 3, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn a_trailing_blank_keeps_or_pops_by_kind() {
        let mut text = body("aaa\n", true);
        let edited = "aaa\n\n";
        assert!(text.splice(edited, 1..2, 1..3, 1, true));
        assert_eq!(
            ranges(&text, edited),
            fresh(edited, true),
            "text keeps the row"
        );
        let mut code = body("aaa\n", false);
        assert!(code.splice(edited, 1..2, 1..3, 1, false));
        assert_eq!(ranges(&code, edited), fresh(edited, false), "code pops it");
    }

    #[test]
    fn blank_rows_shift_through_an_edit_above_them() {
        let mut b = body("aaa\n\n\nccc\n\n", true);
        let edited = "aaaxx\n\n\nccc\n\n";
        assert!(b.splice(edited, 0..1, 0..1, 2, true));
        assert_eq!(ranges(&b, edited), fresh(edited, true));
    }

    #[test]
    fn an_edit_beyond_a_popped_tail_rebuilds_the_suffix() {
        // A code file's popped trailing blanks leave source bytes past
        // the last entry; an edit landing there must still resolve.
        let mut b = body("aaa\n\n\n", false);
        assert_eq!(b.len(), 1, "the popped body holds one row");
        let edited = "aaa\n\nx\n";
        assert!(b.splice(edited, 2..3, 2..3, 1, false));
        assert_eq!(ranges(&b, edited), fresh(edited, false));
    }

    #[test]
    fn an_owned_body_declines_the_splice() {
        let mut b = CodeBody::from_text("aaa\nbbb\n");
        assert!(!b.splice("aaa\nbxbb\n", 1..2, 1..2, 1, true));
    }
}
