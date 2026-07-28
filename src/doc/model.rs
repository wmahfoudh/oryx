//! Document model: the visual-free representation every later stage consumes.

use std::ops::Range;

use crate::style::highlight::SyntaxRole;

#[derive(Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// The text the document was parsed from; block and span ranges index
    /// into it. Markdown copy slices it directly.
    pub source: String,
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
    pub kind: BlockKind,
}

impl Block {
    pub fn plain(kind: BlockKind) -> Self {
        Block {
            quote_depth: 0,
            alert: None,
            range: 0..0,
            centered: false,
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
        lines: Vec<String>,
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
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Marker {
    Bullet,
    Number(u64),
    Task { checked: bool },
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
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub math: bool,
    pub script: SpanScript,
    /// Link target: a URL, a `#anchor`, or `footnote:<label>`.
    pub link: Option<String>,
    /// Set when the span is an inline image flowing with the text.
    pub image: Option<SpanImage>,
    /// Byte range of the span's origin in `Document::source`. The slice may
    /// differ from `text` when parsing transformed it (smart punctuation,
    /// emoji, stripped HTML); empty for synthesized spans.
    pub range: Range<usize>,
}

impl Default for Span {
    fn default() -> Span {
        Span {
            text: String::new(),
            bold: false,
            italic: false,
            strike: false,
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
            text: text.into(),
            ..Span::default()
        }
    }
}
