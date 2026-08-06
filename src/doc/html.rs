//! XHTML to blocks: the DOM walker book chapters parse through. The
//! walker owns the growing book, blocks and the synthetic source the
//! spans index; it knows nothing of EPUB packaging, so a future `.html`
//! file path can reuse it.
//!
//! The element mapping is the Embedded HTML table applied from a tree:
//! the same tags with the same meanings as the markdown scanner, whose
//! `slug` and `trim_cell` it shares so books and READMEs agree. Anything
//! unmapped strips to its inner text.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::doc::markdown::{slug, trim_cell};
use crate::doc::model::{
    seal_blocks, Block, BlockKind, CodeBody, DetailsGroup, Marker, Span, SpanScript,
};

/// Structural containers with no mapping of their own: a paragraph ends
/// on both sides, their children walk as usual.
const BOUNDARY_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "figcaption",
    "figure",
    "footer",
    "header",
    "main",
    "nav",
    "section",
];

/// Subtrees with no rendered text.
const SKIP_TAGS: &[&str] = &["head", "script", "style", "template", "title"];

struct ListLevel {
    ordered: bool,
    next: u64,
    item_open: bool,
}

#[derive(Default)]
struct Pre {
    language: Option<String>,
    text: String,
}

#[derive(Default)]
struct TableAcc {
    header: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    caption: Option<Vec<Span>>,
    row: Vec<Vec<Span>>,
    row_all_th: bool,
    in_thead: bool,
    in_caption: bool,
    cell_open: bool,
}

pub struct Walker {
    source: String,
    blocks: Vec<Block>,
    details: Vec<DetailsGroup>,
    details_summarized: Vec<bool>,
    details_start: Vec<usize>,
    details_stack: Vec<u16>,
    /// Spans of the paragraph being gathered; text owned until `emit`
    /// appends it to the source and stamps the ranges.
    spans: Vec<Span>,
    /// A whitespace run is pending; the next visible character spends it.
    space: bool,
    quote_depth: u8,
    center: Vec<bool>,
    bold: u16,
    italic: u16,
    strike: u16,
    underline: u16,
    marked: u16,
    coded: u16,
    sub: u16,
    sup: u16,
    small: u16,
    link: Option<String>,
    dt: bool,
    lists: Vec<ListLevel>,
    pre: Option<Pre>,
    table: Option<TableAcc>,
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
            details: Vec::new(),
            details_summarized: Vec::new(),
            details_start: Vec::new(),
            details_stack: Vec::new(),
            spans: Vec::new(),
            space: false,
            quote_depth: 0,
            center: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            underline: 0,
            marked: 0,
            coded: 0,
            sub: 0,
            sup: 0,
            small: 0,
            link: None,
            dt: false,
            lists: Vec::new(),
            pre: None,
            table: None,
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

    /// The seam before spine item `spine`; extra space in layout, a
    /// forced page break in export.
    pub fn chapter_break(&mut self, spine: usize) {
        self.flush();
        self.blocks
            .push(Block::plain(BlockKind::ChapterBreak { spine }));
    }

    /// Seals every span against the finished source and hands the book
    /// over: blocks, source, details groups.
    pub fn finish(mut self) -> (Vec<Block>, String, Vec<DetailsGroup>) {
        self.flush();
        seal_blocks(&mut self.blocks, &self.source);
        (self.blocks, self.source, self.details)
    }

    /// Walks a DOM subtree into the model.
    pub fn walk(&mut self, node: &Handle) {
        match &node.data {
            NodeData::Text { contents } => self.text(&contents.borrow()),
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref().to_ascii_lowercase();
                let attr = |key: &str| -> Option<String> {
                    attrs
                        .borrow()
                        .iter()
                        .find(|a| a.name.local.as_ref().eq_ignore_ascii_case(key))
                        .map(|a| a.value.to_string())
                };
                self.element(&tag, &attr, node);
            }
            NodeData::Document => self.children(node),
            _ => {}
        }
    }

    fn children(&mut self, node: &Handle) {
        for child in node.children.borrow().iter() {
            self.walk(child);
        }
    }

    fn element(&mut self, tag: &str, attr: &dyn Fn(&str) -> Option<String>, node: &Handle) {
        if SKIP_TAGS.contains(&tag) {
            return;
        }
        // Inside a table, block structure flattens into the open cell;
        // only the table family and inline styling keep their meaning.
        let in_table = self.table.is_some();
        match tag {
            "br" => self.linebreak(),
            "img" => {
                if let Some(alt) = attr("alt") {
                    self.push_str(&alt);
                }
            }
            "a" => {
                let external =
                    attr("href").filter(|h| h.starts_with("http://") || h.starts_with("https://"));
                let prior = self.link.take();
                self.link = external.or_else(|| prior.clone());
                self.children(node);
                self.link = prior;
            }
            "b" | "strong" => self.styled(&mut |w| &mut w.bold, node),
            "i" | "em" | "cite" | "dfn" | "var" => self.styled(&mut |w| &mut w.italic, node),
            "s" | "del" | "strike" => self.styled(&mut |w| &mut w.strike, node),
            "u" | "ins" => self.styled(&mut |w| &mut w.underline, node),
            "mark" => self.styled(&mut |w| &mut w.marked, node),
            "sub" => self.styled(&mut |w| &mut w.sub, node),
            "sup" => self.styled(&mut |w| &mut w.sup, node),
            "small" => self.styled(&mut |w| &mut w.small, node),
            "code" if self.pre.is_some() => {
                let pre = self.pre.as_mut().expect("pre is open");
                pre.language =
                    attr("class").and_then(|c| c.strip_prefix("language-").map(str::to_string));
                self.children(node);
            }
            "code" | "kbd" | "samp" | "tt" => self.styled(&mut |w| &mut w.coded, node),
            "q" => {
                self.push_glyph("\u{201C}");
                self.children(node);
                self.push_glyph("\u{201D}");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !in_table => {
                let level = tag.as_bytes()[1] - b'0';
                self.flush();
                self.children(node);
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                let anchor = slug(&spans);
                self.emit(BlockKind::Heading {
                    level,
                    spans,
                    anchor,
                });
            }
            "p" | "div" if !in_table => {
                let centered = attr("align").is_some_and(|a| a.eq_ignore_ascii_case("center"));
                self.center.push(centered);
                self.flush();
                self.children(node);
                self.flush();
                self.center.pop();
            }
            "blockquote" if !in_table => {
                self.flush();
                self.quote_depth = self.quote_depth.saturating_add(1);
                self.children(node);
                self.flush();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            "pre" if !in_table => {
                self.flush();
                self.pre = Some(Pre::default());
                self.children(node);
                self.pre_close();
            }
            "hr" if !in_table => {
                self.flush();
                self.emit(BlockKind::Rule);
            }
            "ul" | "ol" if !in_table => {
                if self.lists.is_empty() {
                    self.flush();
                } else {
                    // A nested list opens inside an item: the item's own
                    // text emits first, GitHub's rendering order.
                    self.li_close();
                }
                let next = (tag == "ol")
                    .then(|| attr("start").and_then(|s| s.parse().ok()))
                    .flatten()
                    .unwrap_or(1);
                self.lists.push(ListLevel {
                    ordered: tag == "ol",
                    next,
                    item_open: false,
                });
                self.children(node);
                self.li_close();
                self.lists.pop();
            }
            "li" if !in_table => {
                if self.lists.is_empty() {
                    self.children(node);
                    return;
                }
                self.li_close();
                self.spans.clear();
                let top = self.lists.last_mut().expect("a list is open");
                top.item_open = true;
                self.children(node);
                self.li_close();
            }
            "dl" if !in_table => {
                self.flush();
                self.children(node);
                self.flush();
            }
            "dt" if !in_table => {
                self.flush();
                self.spans.clear();
                self.dt = true;
                self.children(node);
                self.dt_close();
            }
            "dd" if !in_table => {
                self.flush();
                self.spans.clear();
                self.children(node);
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                if !spans.is_empty() {
                    self.emit(BlockKind::ListItem {
                        marker: Marker::None,
                        depth: 0,
                        spans,
                    });
                }
            }
            "details" if !in_table => {
                self.flush();
                let id = self.details.len() as u16;
                self.details.push(DetailsGroup {
                    parent: self.details_stack.last().copied(),
                    open: attr("open").is_some(),
                });
                self.details_summarized.push(false);
                self.details_start.push(self.blocks.len());
                self.details_stack.push(id);
                self.children(node);
                self.flush();
                self.details_stack.pop();
                self.summarize(id);
            }
            "summary" if !in_table => {
                let Some(&id) = self.details_stack.last() else {
                    self.children(node);
                    return;
                };
                self.flush();
                self.spans.clear();
                self.children(node);
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                self.emit(BlockKind::Summary { spans, group: id });
                self.details_summarized[id as usize] = true;
            }
            "table" if !in_table => {
                self.flush();
                self.table = Some(TableAcc::default());
                self.children(node);
                self.table_close();
            }
            "caption" if in_table => {
                let t = self.table.as_mut().expect("a table is open");
                t.in_caption = true;
                self.spans.clear();
                self.children(node);
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                let t = self.table.as_mut().expect("a table is open");
                t.in_caption = false;
                if !spans.is_empty() {
                    t.caption = Some(spans);
                }
            }
            "thead" if in_table => {
                self.table.as_mut().expect("a table is open").in_thead = true;
                self.children(node);
                self.table.as_mut().expect("a table is open").in_thead = false;
            }
            "tr" if in_table => {
                {
                    let t = self.table.as_mut().expect("a table is open");
                    t.row.clear();
                    t.row_all_th = true;
                }
                self.children(node);
                self.row_close();
            }
            "th" | "td" if in_table => {
                {
                    let t = self.table.as_mut().expect("a table is open");
                    t.cell_open = true;
                    t.row_all_th &= tag == "th";
                }
                self.spans.clear();
                self.children(node);
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                let t = self.table.as_mut().expect("a table is open");
                t.cell_open = false;
                t.row.push(spans);
            }
            "picture" => self.children(node),
            _ if BOUNDARY_TAGS.contains(&tag) && !in_table => {
                self.flush();
                self.children(node);
                self.flush();
            }
            // Everything unmapped strips to its inner text.
            _ => self.children(node),
        }
    }

    /// Bumps one style counter around the subtree.
    fn styled(&mut self, field: &mut dyn Fn(&mut Walker) -> &mut u16, node: &Handle) {
        *field(self) += 1;
        self.children(node);
        let counter = field(self);
        *counter = counter.saturating_sub(1);
    }

    /// A span carrying the current inline style and no text yet.
    fn style(&self) -> Span {
        let mut span = Span::plain("");
        span.bold = self.bold > 0 || self.dt;
        span.italic = self.italic > 0;
        span.strike = self.strike > 0;
        span.underline = self.underline > 0;
        span.mark = self.marked > 0;
        span.code = self.coded > 0;
        span.script = if self.sub > 0 {
            SpanScript::Sub
        } else if self.sup > 0 {
            SpanScript::Sup
        } else if self.small > 0 {
            SpanScript::Small
        } else {
            SpanScript::None
        };
        span.link = self.link.clone();
        span
    }

    fn same_style(a: &Span, b: &Span) -> bool {
        a.bold == b.bold
            && a.italic == b.italic
            && a.strike == b.strike
            && a.underline == b.underline
            && a.mark == b.mark
            && a.code == b.code
            && a.script == b.script
            && a.link == b.link
            && a.image.is_none()
            && b.image.is_none()
    }

    /// Appends text in the current style, merging into the last span
    /// when the styles match.
    fn push_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let style = self.style();
        match self.spans.last_mut() {
            Some(last) if Self::same_style(last, &style) => last.raw_text_mut().push_str(text),
            _ => {
                let mut span = style;
                span.set_text(text);
                self.spans.push(span);
            }
        }
    }

    /// A styled quotation glyph for `<q>`.
    fn push_glyph(&mut self, glyph: &str) {
        self.push_str(glyph);
    }

    /// `<br>`: its own newline span, the plain-text path's shape, so
    /// layout starts a fresh line.
    fn linebreak(&mut self) {
        if let Some(pre) = self.pre.as_mut() {
            pre.text.push('\n');
            return;
        }
        self.space = false;
        let mut span = Span::plain("\n");
        span.range = 0..0;
        self.spans.push(span);
    }

    fn text(&mut self, text: &str) {
        if let Some(pre) = self.pre.as_mut() {
            pre.text.push_str(text);
            return;
        }
        if let Some(t) = &self.table {
            // Text outside any cell or caption is discarded, the browser
            // hoisting rule reduced to a drop.
            if !t.cell_open && !t.in_caption {
                return;
            }
        }
        let mut pending = String::new();
        for ch in text.chars() {
            if ch.is_whitespace() {
                self.space = !self.spans.is_empty() || !pending.is_empty();
            } else {
                if std::mem::take(&mut self.space) && !self.ends_with_newline(&pending) {
                    pending.push(' ');
                }
                pending.push(ch);
            }
        }
        let keep_space = self.space;
        self.push_str(&pending);
        self.space = keep_space;
    }

    fn ends_with_newline(&self, pending: &str) -> bool {
        if !pending.is_empty() {
            return pending.ends_with('\n');
        }
        self.spans
            .last()
            .is_some_and(|s| s.raw_text().ends_with('\n'))
    }

    /// Emits the gathered spans as a paragraph.
    fn flush(&mut self) {
        self.space = false;
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        if spans.is_empty() {
            return;
        }
        self.emit(BlockKind::Paragraph { spans });
    }

    /// Emits the open item's accumulated spans. An item that ends empty
    /// emits nothing and takes no number.
    fn li_close(&mut self) {
        let depth = self.lists.len().saturating_sub(1) as u8;
        let Some(top) = self.lists.last_mut() else {
            return;
        };
        if !top.item_open {
            return;
        }
        top.item_open = false;
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        if spans.is_empty() {
            return;
        }
        let top = self.lists.last_mut().expect("a list is open");
        let marker = if top.ordered {
            let n = top.next;
            top.next += 1;
            Marker::Number(n)
        } else {
            Marker::Bullet
        };
        self.emit(BlockKind::ListItem {
            marker,
            depth,
            spans,
        });
    }

    /// Emits the accumulated `<dt>` term as a bold paragraph.
    fn dt_close(&mut self) {
        if !self.dt {
            return;
        }
        self.dt = false;
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        for span in &mut spans {
            span.bold = true;
        }
        if !spans.is_empty() {
            self.emit(BlockKind::Paragraph { spans });
        }
    }

    /// Emits the accumulated `<pre>` body as a code block, its lines
    /// verbatim ranges into the synthetic source.
    fn pre_close(&mut self) {
        let Some(pre) = self.pre.take() else {
            return;
        };
        let body = pre.text.strip_prefix('\n').unwrap_or(&pre.text);
        if body.trim().is_empty() {
            return;
        }
        let block_start = self.source.len();
        let mut lines: Vec<std::ops::Range<u32>> = Vec::new();
        for line in body.split('\n') {
            let start = self.source.len() as u32;
            self.source.push_str(line);
            lines.push(start..self.source.len() as u32);
            self.source.push('\n');
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let block_end = lines.last().map(|l| l.end as usize).unwrap_or(block_start);
        let kind = BlockKind::CodeBlock {
            language: pre.language,
            lines: CodeBody::verbatim(lines),
            highlights: Vec::new(),
        };
        self.emit_placed(kind, block_start..block_end);
    }

    /// Emits the accumulated table, its caption first as a centered
    /// paragraph. An empty accumulator emits nothing.
    fn table_close(&mut self) {
        self.row_close();
        self.spans.clear();
        let Some(mut t) = self.table.take() else {
            return;
        };
        if let Some(caption) = t.caption.take() {
            self.center.push(true);
            self.emit(BlockKind::Paragraph { spans: caption });
            self.center.pop();
        }
        if t.header.is_empty() && t.rows.is_empty() {
            return;
        }
        for cell in t.header.iter_mut().chain(t.rows.iter_mut().flatten()) {
            self.place_spans(cell);
        }
        let start = self.source.len();
        self.emit_placed(
            BlockKind::Table {
                header: t.header,
                rows: t.rows,
            },
            start..start,
        );
    }

    /// Closes the current row: the first `<thead>` or all-`<th>` row
    /// becomes the header, everything after lands in the body.
    fn row_close(&mut self) {
        let Some(t) = self.table.as_mut() else {
            return;
        };
        if t.row.is_empty() {
            return;
        }
        let row = std::mem::take(&mut t.row);
        let leading = t.header.is_empty() && t.rows.is_empty();
        if leading && (t.in_thead || t.row_all_th) {
            t.header = row;
        } else {
            t.rows.push(row);
        }
    }

    /// A group whose close arrives without a summary row gets one
    /// reading "Details", the markdown scanner's fallback.
    fn summarize(&mut self, id: u16) {
        if self.details_summarized[id as usize] {
            return;
        }
        self.details_summarized[id as usize] = true;
        let at = self.details_start[id as usize].min(self.blocks.len());
        self.blocks.insert(
            at,
            Block {
                quote_depth: 0,
                alert: None,
                range: 0..0,
                centered: false,
                details: self.details[id as usize].parent,
                kind: BlockKind::Summary {
                    spans: vec![Span::plain("Details")],
                    group: id,
                },
            },
        );
    }

    /// Appends each span's text to the source and stamps its range; the
    /// synthetic source is exactly the book's text in reading order.
    fn place_spans(&mut self, spans: &mut [Span]) -> std::ops::Range<usize> {
        let start = self.source.len();
        for span in spans.iter_mut() {
            let text = span.raw_text();
            if text.is_empty() {
                continue;
            }
            let at = self.source.len() as u32;
            self.source.push_str(text);
            span.range = at..self.source.len() as u32;
        }
        let end = self.source.len();
        if end > start {
            self.source.push('\n');
        }
        start..end
    }

    /// Places the kind's spans into the source, then emits the block
    /// with the walker's block context stamped on.
    fn emit(&mut self, mut kind: BlockKind) {
        let range = match &mut kind {
            BlockKind::Heading { spans, .. }
            | BlockKind::Paragraph { spans }
            | BlockKind::ListItem { spans, .. }
            | BlockKind::Summary { spans, .. } => self.place_spans(spans),
            _ => {
                let at = self.source.len();
                at..at
            }
        };
        self.emit_placed(kind, range);
    }

    fn emit_placed(&mut self, kind: BlockKind, range: std::ops::Range<usize>) {
        // A summary row belongs to the group enclosing its own; it is
        // the toggle, visible while its group is closed.
        let details = match &kind {
            BlockKind::Summary { group, .. } => self.details[*group as usize].parent,
            _ => self.details_stack.last().copied(),
        };
        self.blocks.push(Block {
            quote_depth: self.quote_depth,
            alert: None,
            range,
            centered: self.center.iter().any(|&c| c),
            details,
            kind,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::model::{DetailsGroup, Marker, SpanScript};

    fn walk(xhtml: &str) -> (Vec<Block>, String, Vec<DetailsGroup>) {
        let mut walker = Walker::new();
        walker.walk_chapter(xhtml);
        walker.finish()
    }

    fn text_of(spans: &[Span], source: &str) -> String {
        spans.iter().map(|s| s.text(source)).collect()
    }

    #[test]
    fn h2_yields_a_level_two_heading_with_anchor() {
        let (blocks, source, _) = walk("<html><body><h2>Down the Rabbit-Hole</h2></body></html>");
        let BlockKind::Heading {
            level,
            spans,
            anchor,
        } = &blocks[0].kind
        else {
            panic!("expected a heading, got {:?}", blocks[0].kind);
        };
        assert_eq!(*level, 2);
        assert_eq!(text_of(spans, &source), "Down the Rabbit-Hole");
        assert_eq!(anchor, "down-the-rabbit-hole");
    }

    #[test]
    fn nested_lists_carry_kinds_and_depths() {
        let (blocks, source, _) = walk(
            "<html><body><ol start=\"3\"><li>alpha</li><li>beta<ul><li>inner</li></ul></li></ol></body></html>",
        );
        let items: Vec<(Marker, u8, String)> = blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::ListItem {
                    marker,
                    depth,
                    spans,
                } => Some((*marker, *depth, text_of(spans, &source))),
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            [
                (Marker::Number(3), 0, "alpha".to_string()),
                (Marker::Number(4), 0, "beta".to_string()),
                (Marker::Bullet, 1, "inner".to_string()),
            ]
        );
    }

    #[test]
    fn double_blockquote_carries_depth_two() {
        let (blocks, _, _) =
            walk("<html><body><blockquote><blockquote><p>deep</p></blockquote></blockquote></body></html>");
        assert_eq!(blocks[0].quote_depth, 2);
    }

    #[test]
    fn pre_code_yields_a_language_tagged_code_block() {
        let (blocks, source, _) = walk(
            "<html><body><pre><code class=\"language-rust\">fn main() {}\nlet x = 1;\n</code></pre></body></html>",
        );
        let BlockKind::CodeBlock {
            language, lines, ..
        } = &blocks[0].kind
        else {
            panic!("expected a code block, got {:?}", blocks[0].kind);
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.line(&source, 0), "fn main() {}");
        assert_eq!(lines.line(&source, 1), "let x = 1;");
        assert!(lines.is_verbatim(), "code lines should slice the source");
    }

    #[test]
    fn thead_table_maps_header_and_rows() {
        let (blocks, source, _) = walk(
            "<html><body><table><thead><tr><th>K</th><th>V</th></tr></thead><tbody><tr><td>a</td><td>1</td></tr></tbody></table></body></html>",
        );
        let BlockKind::Table { header, rows } = &blocks[0].kind else {
            panic!("expected a table, got {:?}", blocks[0].kind);
        };
        assert_eq!(header.len(), 2);
        assert_eq!(text_of(&header[0], &source), "K");
        assert_eq!(rows.len(), 1);
        assert_eq!(text_of(&rows[0][1], &source), "1");
    }

    #[test]
    fn headerless_table_keeps_an_empty_header() {
        let (blocks, source, _) =
            walk("<html><body><table><tr><td>a</td><td>b</td></tr></table></body></html>");
        let BlockKind::Table { header, rows } = &blocks[0].kind else {
            panic!("expected a table, got {:?}", blocks[0].kind);
        };
        assert!(header.is_empty());
        assert_eq!(text_of(&rows[0][0], &source), "a");
    }

    #[test]
    fn dl_maps_bold_terms_and_indented_definitions() {
        let (blocks, source, _) =
            walk("<html><body><dl><dt>term</dt><dd>meaning</dd></dl></body></html>");
        let BlockKind::Paragraph { spans } = &blocks[0].kind else {
            panic!("expected the term paragraph, got {:?}", blocks[0].kind);
        };
        assert!(spans.iter().all(|s| s.bold));
        assert_eq!(text_of(spans, &source), "term");
        let BlockKind::ListItem {
            marker,
            depth,
            spans,
        } = &blocks[1].kind
        else {
            panic!("expected the definition item, got {:?}", blocks[1].kind);
        };
        assert_eq!(*marker, Marker::None);
        assert_eq!(*depth, 0);
        assert_eq!(text_of(spans, &source), "meaning");
    }

    #[test]
    fn details_fold_state_follows_the_open_attribute() {
        let (blocks, _, details) = walk(
            "<html><body>\
             <details open><summary>Shown</summary><p>a</p></details>\
             <details><summary>Hidden</summary><p>b</p></details>\
             </body></html>",
        );
        assert_eq!(details.len(), 2);
        assert!(details[0].open);
        assert!(!details[1].open);
        let summaries = blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Summary { .. }))
            .count();
        assert_eq!(summaries, 2);
        let inner = blocks
            .iter()
            .find(|b| matches!(&b.kind, BlockKind::Paragraph { .. }))
            .unwrap();
        assert_eq!(inner.details, Some(0));
    }

    #[test]
    fn inline_tags_carry_their_styles() {
        let (blocks, source, _) = walk(
            "<html><body><p><b>b</b><em>i</em><code>c</code><sub>s</sub><sup>S</sup><small>m</small><u>u</u><del>d</del><mark>k</mark><q>q</q></p></body></html>",
        );
        let BlockKind::Paragraph { spans } = &blocks[0].kind else {
            panic!("expected a paragraph, got {:?}", blocks[0].kind);
        };
        let find = |t: &str| {
            spans
                .iter()
                .find(|s| s.text(&source) == t)
                .unwrap_or_else(|| panic!("no span with text {t:?}"))
                .clone()
        };
        assert!(find("b").bold);
        assert!(find("i").italic);
        assert!(find("c").code);
        assert_eq!(find("s").script, SpanScript::Sub);
        assert_eq!(find("S").script, SpanScript::Sup);
        assert_eq!(find("m").script, SpanScript::Small);
        assert!(find("u").underline);
        assert!(find("d").strike);
        assert!(find("k").mark);
        let joined = text_of(spans, &source);
        assert!(
            joined.contains("\u{201C}q\u{201D}"),
            "q should gain quotation marks: {joined:?}"
        );
    }

    #[test]
    fn picture_reduces_to_its_alt_text() {
        let (blocks, source, _) = walk(
            "<html><body><p><picture><source srcset=\"x.webp\"><img src=\"x.png\" alt=\"a duchess\"></picture></p></body></html>",
        );
        let BlockKind::Paragraph { spans } = &blocks[0].kind else {
            panic!("expected a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(text_of(spans, &source), "a duchess");
    }

    #[test]
    fn http_links_span_where_relative_hrefs_stay_plain() {
        let (blocks, source, _) = walk(
            "<html><body><p><a href=\"https://x.tld\">out</a> and <a href=\"chapter-2.xhtml#f1\">in</a></p></body></html>",
        );
        let BlockKind::Paragraph { spans } = &blocks[0].kind else {
            panic!("expected a paragraph, got {:?}", blocks[0].kind);
        };
        let out = spans.iter().find(|s| s.text(&source) == "out").unwrap();
        assert_eq!(out.link.as_deref(), Some("https://x.tld"));
        assert!(
            spans
                .iter()
                .all(|s| s.link.is_none() || s.text(&source) == "out"),
            "the relative href must stay plain text"
        );
        assert_eq!(text_of(spans, &source), "out and in");
    }

    #[test]
    fn mathml_leaves_only_its_text() {
        let (blocks, source, _) =
            walk("<html><body><p><math><mi>x</mi><mo>=</mo><mn>1</mn></math></p></body></html>");
        let BlockKind::Paragraph { spans } = &blocks[0].kind else {
            panic!("expected a paragraph, got {:?}", blocks[0].kind);
        };
        assert_eq!(text_of(spans, &source), "x=1");
    }

    #[test]
    fn hr_is_a_rule() {
        let (blocks, _, _) = walk("<html><body><p>a</p><hr/><p>b</p></body></html>");
        assert!(matches!(blocks[1].kind, BlockKind::Rule));
    }

    #[test]
    fn centered_paragraph_carries_the_flag() {
        let (blocks, _, _) = walk("<html><body><p align=\"center\">centered</p></body></html>");
        assert!(blocks[0].centered);
    }

    #[test]
    fn malformed_markup_still_yields_its_text() {
        let (blocks, source, _) = walk("<p>Un<closed <b>and</p> stray > brackets");
        assert!(!blocks.is_empty());
        let all: String = blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Paragraph { spans } => Some(text_of(spans, &source)),
                _ => None,
            })
            .collect();
        assert!(all.contains("Un"), "text should survive: {all:?}");
    }
}
