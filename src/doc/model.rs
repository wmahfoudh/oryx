//! Document model: the visual-free representation every later stage consumes.

use std::ops::Range;

use crate::style::highlight::SyntaxRole;

#[derive(Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, PartialEq)]
pub struct Block {
    /// 0 when the block is not inside a blockquote.
    pub quote_depth: u8,
    /// Set when the enclosing quote is a GitHub alert.
    pub alert: Option<AlertKind>,
    pub kind: BlockKind,
}

impl Block {
    pub fn plain(kind: BlockKind) -> Self {
        Block {
            quote_depth: 0,
            alert: None,
            kind,
        }
    }
}

#[derive(Debug, PartialEq)]
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

#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub math: bool,
    /// Link target: a URL, a `#anchor`, or `footnote:<label>`.
    pub link: Option<String>,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }
}
