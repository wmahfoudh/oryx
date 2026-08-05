//! XHTML to blocks: the DOM walker book chapters parse through. The
//! walker owns the growing book, blocks and the synthetic source the
//! spans index; it knows nothing of EPUB packaging, so a future `.html`
//! file path can reuse it.
//!
//! This is the base pass: block-level elements become paragraphs of
//! collapsed text, and every tag strips to its inner text. The element
//! mapping and the emphasis table build on it.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::doc::model::{seal_blocks, Block, BlockKind, Span};

/// Elements that end the paragraph being gathered on both sides.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "td",
    "th",
    "tr",
    "ul",
];

/// Subtrees with no rendered text.
const SKIP_TAGS: &[&str] = &["head", "script", "style", "template"];

pub struct Walker {
    source: String,
    blocks: Vec<Block>,
    /// Text of the paragraph being gathered, whitespace collapsed.
    gather: String,
    /// A whitespace run is pending; the next visible character spends it.
    space: bool,
}

impl Default for Walker {
    fn default() -> Self {
        Walker::new()
    }
}

impl Walker {
    pub fn new() -> Walker {
        Walker {
            source: String::new(),
            blocks: Vec::new(),
            gather: String::new(),
            space: false,
        }
    }

    /// Parses one chapter and walks it. html5ever never fails; malformed
    /// markup yields whatever text it holds.
    pub fn walk_chapter(&mut self, xhtml: &str) {
        let dom = html5ever::parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .one(xhtml.as_bytes());
        self.walk(&dom.document);
        self.flush();
    }

    /// Walks a DOM subtree into the model.
    pub fn walk(&mut self, node: &Handle) {
        match &node.data {
            NodeData::Text { contents } => self.text(&contents.borrow()),
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                if SKIP_TAGS.contains(&tag) {
                    return;
                }
                if tag == "br" {
                    self.space = false;
                    self.gather.push('\n');
                    return;
                }
                let boundary = BLOCK_TAGS.contains(&tag);
                if boundary {
                    self.flush();
                }
                for child in node.children.borrow().iter() {
                    self.walk(child);
                }
                if boundary {
                    self.flush();
                }
            }
            NodeData::Document => {
                for child in node.children.borrow().iter() {
                    self.walk(child);
                }
            }
            _ => {}
        }
    }

    /// The seam before spine item `spine`; extra space in layout, a
    /// forced page break in export.
    pub fn chapter_break(&mut self, spine: usize) {
        self.flush();
        self.blocks
            .push(Block::plain(BlockKind::ChapterBreak { spine }));
    }

    /// Seals every span against the finished source and hands both over.
    pub fn finish(mut self) -> (Vec<Block>, String) {
        self.flush();
        seal_blocks(&mut self.blocks, &self.source);
        (self.blocks, self.source)
    }

    fn text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_whitespace() {
                self.space = !self.gather.is_empty();
            } else {
                if std::mem::take(&mut self.space) && !self.gather.ends_with('\n') {
                    self.gather.push(' ');
                }
                self.gather.push(ch);
            }
        }
    }

    /// Ends the gathered paragraph: its text appends to the source and
    /// one span points at exactly that appendix.
    fn flush(&mut self) {
        self.space = false;
        if self.gather.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.gather);
        let start = self.source.len();
        self.source.push_str(&text);
        let end = self.source.len();
        self.source.push('\n');

        let mut span = Span::plain(text);
        span.range = start as u32..end as u32;
        let mut block = Block::plain(BlockKind::Paragraph { spans: vec![span] });
        block.range = start..end;
        self.blocks.push(block);
    }
}
