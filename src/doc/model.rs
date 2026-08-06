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

    pub fn iter<'a>(&'a self, source: &'a str) -> impl Iterator<Item = &'a str> {
        let base = self.owned.as_deref().unwrap_or(source);
        self.lines.iter().map(move |range| slice(base, range))
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
