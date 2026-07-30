//! Maps pulldown-cmark events onto the document model. Every event carries
//! its byte range in the source; spans and blocks keep those ranges so the
//! selection can slice the original markdown back out.

use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::doc::model::{
    seal_blocks, AlertKind, Block, BlockKind, CodeBody, DetailsGroup, Document, Marker, Span,
    SpanImage, SpanScript,
};

pub fn parse(source: impl Into<Arc<str>>) -> Document {
    parse_unless(source, || false).expect("an unconditional parse completes")
}

/// Parses unless `bail` answers true, checked every few thousand events.
/// The parse worker passes its generation check, so a superseded document
/// is never built to the end. A bailed parse answers None.
///
/// The source arrives as (or becomes) an `Arc<str>` the document keeps;
/// the parse allocates no second copy of it, and `seal_blocks` drops
/// every span text the source already carries.
pub fn parse_unless(source: impl Into<Arc<str>>, bail: impl Fn() -> bool) -> Option<Document> {
    let source: Arc<str> = source.into();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_GFM;
    let mut builder = Builder::new(Arc::clone(&source));
    for (count, (event, range)) in Parser::new_ext(&source, options)
        .into_offset_iter()
        .enumerate()
    {
        if count % 4096 == 0 && bail() {
            return None;
        }
        builder.event(event, range);
    }
    builder.finish();
    let mut blocks = builder.blocks;
    seal_blocks(&mut blocks, &source);
    Some(Document {
        blocks,
        source,
        details: builder.details,
    })
}

/// Inline containers the builder can be inside. Paragraphs inside list items
/// or footnote definitions flush as those blocks, not as plain paragraphs.
struct Builder {
    /// The text being parsed; code bodies verify verbatim against it.
    source: Arc<str>,
    blocks: Vec<Block>,
    spans: Vec<Span>,
    quote_depth: u8,
    /// One entry per quote level; the innermost Some wins.
    alerts: Vec<Option<AlertKind>>,
    /// One entry per list level: the next number of an ordered list, or None.
    lists: Vec<Option<u64>>,
    /// Marker of the current item per list level, taken at flush.
    item_markers: Vec<Option<Marker>>,
    heading: Option<u8>,
    code: Option<(Option<String>, String)>,
    /// Source offset of the code body's first text event; the verbatim
    /// check compares the accumulated body against the source there.
    code_start: Option<usize>,
    table: Option<TableAcc>,
    html_table: Option<HtmlTableAcc>,
    /// The `<details>` groups seen so far; mirrors `Document::details`.
    details: Vec<DetailsGroup>,
    /// Open `<details>` group ids, innermost last.
    details_stack: Vec<u16>,
    /// Whether each group has emitted its summary row yet; a group whose
    /// content arrives first gets one synthesized.
    details_summarized: Vec<bool>,
    /// Set between `<summary>` and its close; spans accumulate for the row.
    in_summary: bool,
    /// Where each group's content starts in `blocks`, for synthesizing a
    /// missing summary row in front of it.
    details_start: Vec<usize>,
    /// Open HTML heading level between `<hN>` and its close.
    html_heading: Option<u8>,
    /// Verbatim body accumulating between `<pre>` and its close.
    html_pre: Option<HtmlPre>,
    /// Set between `<dt>` and its close; the term renders bold.
    html_dt: bool,
    /// Open HTML lists, outermost first.
    html_lists: Vec<HtmlList>,
    html_underline: u32,
    html_mark: u32,
    html_small: u32,
    image: Option<(String, String)>,
    footnote: Option<String>,
    in_metadata: bool,
    metadata: Vec<(String, String)>,
    html_block: bool,
    /// Unterminated tag carried between HTML events; pulldown delivers
    /// block HTML line by line, and attributes may wrap.
    html_tail: String,
    /// One entry per open `<p>`/`<div>`, true when it centers its content.
    html_center: Vec<bool>,
    html_code: u32,
    html_sub: u32,
    html_sup: u32,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<String>,
    /// Source byte range of the event being processed, as (start, end).
    current: (usize, usize),
}

#[derive(Default)]
struct TableAcc {
    header: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    row: Vec<Vec<Span>>,
}

/// Accumulates one embedded HTML table; the tag scanner drives it. The
/// header is the `<thead>` rows or a leading all-`<th>` row; a table
/// with neither stays headerless. Nested tables flatten into the open
/// cell, tracked by `nested`.
#[derive(Default)]
struct HtmlTableAcc {
    header: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    row: Vec<Vec<Span>>,
    caption: Option<Vec<Span>>,
    nested: u32,
    in_head: bool,
    row_open: bool,
    row_all_th: bool,
    cell_open: bool,
}

/// One HTML `<pre>` in progress: the language its `<code>` class named
/// and the verbatim body, entities decoded, tags stripped.
#[derive(Default)]
struct HtmlPre {
    language: Option<String>,
    text: String,
}

/// One open HTML list level.
struct HtmlList {
    ordered: bool,
    next: u64,
    /// An `<li>` is accumulating spans at this level.
    item_open: bool,
}

/// The five entities HTML text cannot spell literally. Anything else
/// passes through untouched.
fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Drops the outer whitespace a cell inherits from source formatting.
/// Image spans stay whole; their text is the alt.
fn trim_cell(spans: &mut Vec<Span>) {
    while let Some(first) = spans.first_mut() {
        if first.image.is_some() {
            break;
        }
        let trimmed = first.raw_text().trim_start().to_string();
        if trimmed.is_empty() {
            spans.remove(0);
        } else {
            *first.raw_text_mut() = trimmed;
            break;
        }
    }
    while let Some(last) = spans.last_mut() {
        if last.image.is_some() {
            break;
        }
        let trimmed = last.raw_text().trim_end().to_string();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            *last.raw_text_mut() = trimmed;
            break;
        }
    }
}

impl Builder {
    fn new(source: Arc<str>) -> Builder {
        Builder {
            source,
            blocks: Vec::new(),
            spans: Vec::new(),
            quote_depth: 0,
            alerts: Vec::new(),
            lists: Vec::new(),
            item_markers: Vec::new(),
            heading: None,
            code: None,
            code_start: None,
            table: None,
            html_table: None,
            details: Vec::new(),
            details_stack: Vec::new(),
            details_summarized: Vec::new(),
            in_summary: false,
            details_start: Vec::new(),
            html_heading: None,
            html_pre: None,
            html_dt: false,
            html_lists: Vec::new(),
            html_underline: 0,
            html_mark: 0,
            html_small: 0,
            image: None,
            footnote: None,
            in_metadata: false,
            metadata: Vec::new(),
            html_block: false,
            html_tail: String::new(),
            html_center: Vec::new(),
            html_code: 0,
            html_sub: 0,
            html_sup: 0,
            bold: 0,
            italic: 0,
            strike: 0,
            link: None,
            current: (0, 0),
        }
    }

    fn event(&mut self, event: Event, range: Range<usize>) {
        self.current = (range.start, range.end);
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(text) => {
                let mut span = self.style();
                span.set_text(text.into_string());
                span.code = true;
                self.push(span);
            }
            Event::InlineMath(tex) => {
                let mut span = self.style();
                span.set_text(tex.into_string());
                span.math = true;
                self.push(span);
            }
            Event::DisplayMath(tex) => {
                self.flush_spans();
                self.emit(BlockKind::MathBlock {
                    tex: tex.into_string(),
                });
            }
            Event::FootnoteReference(label) => {
                let mut span = self.style();
                span.set_text(label.to_string());
                span.link = Some(format!("footnote:{label}"));
                self.push(span);
            }
            Event::TaskListMarker(checked) => {
                if let Some(slot) = self.item_markers.last_mut() {
                    *slot = Some(Marker::Task { checked });
                }
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.push(Span::plain("\n")),
            Event::Rule => self.emit(BlockKind::Rule),
            Event::Html(html) | Event::InlineHtml(html) => self.html(&html),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => self.heading = Some(heading_level(level)),
            Tag::BlockQuote(kind) => {
                self.quote_depth = self.quote_depth.saturating_add(1);
                self.alerts.push(kind.map(alert_kind));
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(
                        lang.split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    _ => None,
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                // A nested list begins before the enclosing item's text flushed.
                self.flush_spans();
                self.lists.push(start);
            }
            Tag::Item => {
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = Marker::Number(*n);
                        *n += 1;
                        m
                    }
                    _ => Marker::Bullet,
                };
                self.item_markers.push(Some(marker));
            }
            Tag::Table(_) => self.table = Some(TableAcc::default()),
            Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.into_string(), String::new()))
            }
            Tag::FootnoteDefinition(label) => self.footnote = Some(label.into_string()),
            Tag::MetadataBlock(_) => self.in_metadata = true,
            Tag::HtmlBlock => self.html_block = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_spans(),
            TagEnd::Heading(_) => {
                let level = self.heading.take().unwrap_or(1);
                let spans = std::mem::take(&mut self.spans);
                let anchor = slug(&spans);
                self.emit(BlockKind::Heading {
                    level,
                    spans,
                    anchor,
                });
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.alerts.pop();
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    let lines = self.code_body(&text);
                    self.emit(BlockKind::CodeBlock {
                        language,
                        lines,
                        highlights: Vec::new(),
                    });
                }
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_spans();
                self.item_markers.pop();
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.header = std::mem::take(&mut t.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.spans);
                if let Some(t) = self.table.as_mut() {
                    t.row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit(BlockKind::Table {
                        header: t.header,
                        rows: t.rows,
                    });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Image => {
                // Images join the text flow as spans; a paragraph holding
                // nothing else collapses back to a block image at flush.
                if let Some((path, alt)) = self.image.take() {
                    let mut span = self.style();
                    span.set_text(alt);
                    span.image = Some(SpanImage {
                        src: path,
                        width: None,
                        height: None,
                    });
                    self.spans.push(span);
                }
            }
            TagEnd::FootnoteDefinition => self.footnote = None,
            TagEnd::MetadataBlock(_) => {
                self.in_metadata = false;
                let entries = std::mem::take(&mut self.metadata);
                self.emit(BlockKind::Frontmatter { entries });
            }
            TagEnd::HtmlBlock => {
                self.html_block = false;
                self.html_tail.clear();
                // A blank line splits one HTML construct over several
                // blocks; an open accumulator carries across them.
                if !self.html_capturing() {
                    self.flush_spans();
                }
            }
            _ => {}
        }
    }

    /// The accumulated code body as line ranges: into the source when the
    /// body sits there verbatim (fenced blocks), into an owned copy when
    /// parsing normalized it (indented blocks strip their indent).
    fn code_body(&mut self, text: &str) -> CodeBody {
        let start = self.code_start.take().unwrap_or(0);
        let verbatim = self
            .source
            .get(start..start + text.len())
            .is_some_and(|s| s == text);
        if !verbatim {
            return CodeBody::from_text(text);
        }
        let base = text.as_ptr() as usize;
        let mut lines: Vec<Range<u32>> = text
            .lines()
            .map(|line| {
                let at = (start + (line.as_ptr() as usize - base)) as u32;
                at..at + line.len() as u32
            })
            .collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        CodeBody::verbatim(lines)
    }

    fn text(&mut self, text: &str) {
        if let Some((_, code)) = self.code.as_mut() {
            if self.code_start.is_none() {
                self.code_start = Some(self.current.0);
            }
            code.push_str(text);
            return;
        }
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(text);
            return;
        }
        if self.in_metadata {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    self.metadata
                        .push((key.trim().to_string(), value.trim().to_string()));
                }
            }
            return;
        }
        let replaced = replace_emoji(text);
        self.linkified(&replaced);
    }

    /// Splits bare http(s) URLs out of plain text into linked spans. Span
    /// ranges assume text offsets match source offsets; when a transform
    /// broke that, consumers detect the mismatch and fall back.
    fn linkified(&mut self, text: &str) {
        let base = self.current.0;
        let mut pos = 0usize;
        let mut rest = text;
        // Whichever scheme occurs first wins; picking one find over the
        // other would swallow an earlier url of the other scheme.
        let next = |rest: &str| match (rest.find("http://"), rest.find("https://")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        while let Some(start) = next(rest) {
            let (before, from) = rest.split_at(start);
            if !before.is_empty() {
                let mut span = self.style();
                span.set_text(before);
                span.range = (base + pos) as u32..(base + pos + before.len()) as u32;
                self.push(span);
            }
            let end = from
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(from.len());
            let (url, after) = from.split_at(end);
            let url = url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
            let mut span = self.style();
            span.set_text(url);
            span.link = Some(url.to_string());
            span.range = (base + pos + start) as u32..(base + pos + start + url.len()) as u32;
            self.push(span);
            pos += start + url.len();
            rest = &from[url.len()..];
            if rest == after && rest.is_empty() {
                break;
            }
        }
        if !rest.is_empty() {
            let mut span = self.style();
            span.set_text(rest);
            span.range = (base + pos) as u32..(base + pos + rest.len()) as u32;
            self.push(span);
        }
    }

    /// The GitHub README subset of embedded HTML: centered p and div,
    /// sized images, links wrapping images, br, and inline styling tags.
    /// Everything else is stripped with its inner text kept.
    fn html(&mut self, html: &str) {
        let combined = if self.html_tail.is_empty() {
            html.to_string()
        } else {
            std::mem::take(&mut self.html_tail) + " " + html
        };
        let mut rest = combined.as_str();
        while let Some(open) = rest.find('<') {
            let (before, tag_on) = rest.split_at(open);
            // A `<` not starting a tag is ordinary text.
            let tag_like = tag_on[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '/');
            if !tag_like {
                self.html_text(before);
                self.html_text("<");
                rest = &tag_on[1..];
                continue;
            }
            self.html_text(before);
            let Some(close) = tag_on.find('>') else {
                // The tag continues in the next event; keep it whole.
                self.html_tail = tag_on.to_string();
                return;
            };
            self.html_tag(&tag_on[1..close]);
            rest = &tag_on[close + 1..];
        }
        self.html_text(rest);
    }

    /// Text between tags; HTML collapses whitespace runs, newlines
    /// included, except inside `<pre>`, whose body is verbatim.
    fn html_text(&mut self, text: &str) {
        if let Some(pre) = self.html_pre.as_mut() {
            pre.text.push_str(&decode_entities(text));
            return;
        }
        if text.trim().is_empty() {
            if !text.is_empty() && !self.spans.is_empty() {
                self.text(" ");
            }
            return;
        }
        let mut collapsed = String::with_capacity(text.len());
        if text.starts_with(char::is_whitespace) {
            collapsed.push(' ');
        }
        collapsed.push_str(&text.split_whitespace().collect::<Vec<_>>().join(" "));
        if text.ends_with(char::is_whitespace) {
            collapsed.push(' ');
        }
        self.text(&decode_entities(&collapsed));
    }

    fn html_tag(&mut self, tag: &str) {
        let inner = tag.trim().trim_end_matches('/').trim();
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        let attrs = &inner[name_end..];
        match (name.as_str(), closing) {
            ("br", _) => self.push(Span::plain("\n")),
            ("p" | "div", false) => {
                // Block tags inside a capturing construct (table cell,
                // list item, heading, pre, summary) flatten to its text.
                if self.html_capturing() {
                    return;
                }
                self.flush_spans();
                let centered =
                    html_attr(attrs, "align").is_some_and(|a| a.eq_ignore_ascii_case("center"));
                self.html_center.push(centered);
            }
            ("p" | "div", true) => {
                if self.html_capturing() {
                    return;
                }
                self.flush_spans();
                self.html_center.pop();
            }
            ("table", false) => {
                if let Some(t) = self.html_table.as_mut() {
                    t.nested += 1;
                    return;
                }
                self.flush_spans();
                self.html_table = Some(HtmlTableAcc::default());
            }
            ("table", true) => {
                let Some(t) = self.html_table.as_mut() else {
                    return;
                };
                if t.nested > 0 {
                    t.nested -= 1;
                    return;
                }
                self.html_table_close();
            }
            ("thead", _) => {
                if let Some(t) = self.html_table.as_mut() {
                    if t.nested == 0 {
                        t.in_head = !closing;
                    }
                }
            }
            ("tr", false) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    self.html_row_close();
                    self.spans.clear();
                    let t = self.html_table.as_mut().expect("table is open");
                    t.row_open = true;
                    t.row_all_th = true;
                }
            }
            ("tr", true) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    self.html_row_close();
                }
            }
            ("th" | "td", false) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    self.html_cell_close();
                    self.spans.clear();
                    let t = self.html_table.as_mut().expect("table is open");
                    if !t.row_open {
                        t.row_open = true;
                        t.row_all_th = true;
                    }
                    if name == "td" {
                        t.row_all_th = false;
                    }
                    t.cell_open = true;
                }
            }
            ("th" | "td", true) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    self.html_cell_close();
                }
            }
            ("caption", false) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    self.spans.clear();
                }
            }
            ("caption", true) => {
                if self.html_table.as_ref().is_some_and(|t| t.nested == 0) {
                    let mut caption = std::mem::take(&mut self.spans);
                    trim_cell(&mut caption);
                    if !caption.is_empty() {
                        let t = self.html_table.as_mut().expect("table is open");
                        t.caption = Some(caption);
                    }
                }
            }
            ("details", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                let id = self.details.len() as u16;
                self.details.push(DetailsGroup {
                    parent: self.details_stack.last().copied(),
                    open: html_flag(attrs, "open"),
                });
                self.details_summarized.push(false);
                self.details_start.push(self.blocks.len());
                self.details_stack.push(id);
            }
            ("details", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                if let Some(id) = self.details_stack.pop() {
                    self.summarize(id);
                }
            }
            ("summary", false) => {
                if self.html_table.is_some() || self.details_stack.is_empty() {
                    return;
                }
                self.flush_spans();
                self.in_summary = true;
            }
            ("summary", true) => {
                if self.html_table.is_some() || !self.in_summary {
                    return;
                }
                self.in_summary = false;
                let mut spans = std::mem::take(&mut self.spans);
                trim_cell(&mut spans);
                if let Some(&id) = self.details_stack.last() {
                    self.emit(BlockKind::Summary { spans, group: id });
                    self.details_summarized[id as usize] = true;
                }
            }
            ("a", false) => self.link = html_attr(attrs, "href"),
            ("a", true) => self.link = None,
            ("img", false) => {
                let Some(src) = html_attr(attrs, "src") else {
                    return;
                };
                let mut span = self.style();
                span.set_text(html_attr(attrs, "alt").unwrap_or_default());
                span.image = Some(SpanImage {
                    src,
                    width: html_attr(attrs, "width").and_then(|v| v.parse().ok()),
                    height: html_attr(attrs, "height").and_then(|v| v.parse().ok()),
                });
                self.spans.push(span);
            }
            ("b" | "strong", false) => self.bold += 1,
            ("b" | "strong", true) => self.bold = self.bold.saturating_sub(1),
            ("i" | "em" | "cite" | "dfn" | "var", false) => self.italic += 1,
            ("i" | "em" | "cite" | "dfn" | "var", true) => {
                self.italic = self.italic.saturating_sub(1)
            }
            ("code", false) if self.html_pre.is_some() => {
                // The fence language rides the GitHub class convention.
                let pre = self.html_pre.as_mut().expect("pre is open");
                pre.language = html_attr(attrs, "class")
                    .and_then(|c| c.strip_prefix("language-").map(str::to_string));
            }
            ("code", true) if self.html_pre.is_some() => {}
            ("code" | "kbd" | "samp" | "tt", false) => self.html_code += 1,
            ("code" | "kbd" | "samp" | "tt", true) => {
                self.html_code = self.html_code.saturating_sub(1)
            }
            ("sub", false) => self.html_sub += 1,
            ("sub", true) => self.html_sub = self.html_sub.saturating_sub(1),
            ("sup", false) => self.html_sup += 1,
            ("sup", true) => self.html_sup = self.html_sup.saturating_sub(1),
            ("u" | "ins", false) => self.html_underline += 1,
            ("u" | "ins", true) => self.html_underline = self.html_underline.saturating_sub(1),
            ("s" | "del" | "strike", false) => self.strike += 1,
            ("s" | "del" | "strike", true) => self.strike = self.strike.saturating_sub(1),
            ("mark", false) => self.html_mark += 1,
            ("mark", true) => self.html_mark = self.html_mark.saturating_sub(1),
            ("small", false) => self.html_small += 1,
            ("small", true) => self.html_small = self.html_small.saturating_sub(1),
            ("q", false) => self.push_quote_glyph("\u{201C}"),
            ("q", true) => self.push_quote_glyph("\u{201D}"),
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.html_heading = name.as_bytes()[1].checked_sub(b'0');
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.html_heading_close();
            }
            ("blockquote", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.quote_depth = self.quote_depth.saturating_add(1);
            }
            ("blockquote", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            ("pre", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.html_pre = Some(HtmlPre::default());
            }
            ("pre", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.html_pre_close();
            }
            ("hr", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.emit(BlockKind::Rule);
            }
            ("ul" | "ol", false) => {
                if self.html_table.is_some() {
                    return;
                }
                if self.html_lists.is_empty() {
                    self.flush_spans();
                } else {
                    // A nested list opens inside an item: the item's own
                    // text emits first, GitHub's rendering order.
                    self.html_li_close();
                }
                let next = (name == "ol")
                    .then(|| html_attr(attrs, "start").and_then(|s| s.parse().ok()))
                    .flatten()
                    .unwrap_or(1);
                self.html_lists.push(HtmlList {
                    ordered: name == "ol",
                    next,
                    item_open: false,
                });
            }
            ("ul" | "ol", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.html_li_close();
                self.html_lists.pop();
            }
            ("li", false) => {
                if self.html_table.is_some() || self.html_lists.is_empty() {
                    return;
                }
                self.html_li_close();
                self.spans.clear();
                let top = self.html_lists.last_mut().expect("a list is open");
                top.item_open = true;
            }
            ("li", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.html_li_close();
            }
            ("dl", _) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
            }
            ("dt", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.spans.clear();
                self.html_dt = true;
            }
            ("dt", true) => {
                if self.html_table.is_some() {
                    return;
                }
                self.html_dt_close();
            }
            ("dd", false) => {
                if self.html_table.is_some() {
                    return;
                }
                self.flush_spans();
                self.spans.clear();
            }
            ("dd", true) => {
                if self.html_table.is_some() {
                    return;
                }
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
            _ => {}
        }
    }

    /// A styled quotation glyph for `<q>`; owned text, no source range.
    fn push_quote_glyph(&mut self, glyph: &str) {
        let mut span = self.style();
        span.set_text(glyph);
        span.range = 0..0;
        self.push(span);
    }

    /// Emits the accumulated `<hN>` heading with its GitHub slug anchor.
    fn html_heading_close(&mut self) {
        let Some(level) = self.html_heading.take() else {
            return;
        };
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        let anchor = slug(&spans);
        self.emit(BlockKind::Heading {
            level,
            spans,
            anchor,
        });
    }

    /// Emits the accumulated `<pre>` body as a code block; highlighting
    /// arrives from the lazy pipeline like any fence.
    fn html_pre_close(&mut self) {
        let Some(pre) = self.html_pre.take() else {
            return;
        };
        let body = pre.text.strip_prefix('\n').unwrap_or(&pre.text);
        if body.trim().is_empty() {
            return;
        }
        self.emit(BlockKind::CodeBlock {
            language: pre.language,
            lines: CodeBody::from_text(body),
            highlights: Vec::new(),
        });
    }

    /// Emits the open `<li>`'s accumulated spans as its item. An item
    /// that ends empty emits nothing and takes no number.
    fn html_li_close(&mut self) {
        let depth = self.html_lists.len().saturating_sub(1) as u8;
        let Some(top) = self.html_lists.last_mut() else {
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
        let top = self.html_lists.last_mut().expect("a list is open");
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
    fn html_dt_close(&mut self) {
        if !self.html_dt {
            return;
        }
        self.html_dt = false;
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        for span in &mut spans {
            span.bold = true;
        }
        if !spans.is_empty() {
            self.emit(BlockKind::Paragraph { spans });
        }
    }

    /// Closes an open HTML table cell into its row. Text outside any
    /// cell is discarded, the browser hoisting rule reduced to a drop.
    fn html_cell_close(&mut self) {
        let spans = std::mem::take(&mut self.spans);
        let Some(t) = self.html_table.as_mut() else {
            return;
        };
        if !t.cell_open {
            return;
        }
        let mut cell = spans;
        trim_cell(&mut cell);
        t.row.push(cell);
        t.cell_open = false;
    }

    /// Closes an open HTML table row. The first row becomes the header
    /// when it sits in `<thead>` or is all `<th>` cells.
    fn html_row_close(&mut self) {
        self.html_cell_close();
        let Some(t) = self.html_table.as_mut() else {
            return;
        };
        if !t.row_open {
            return;
        }
        t.row_open = false;
        let row = std::mem::take(&mut t.row);
        if row.is_empty() {
            return;
        }
        if (t.in_head || t.row_all_th) && t.header.is_empty() && t.rows.is_empty() {
            t.header = row;
        } else {
            t.rows.push(row);
        }
    }

    /// Emits the accumulated HTML table, its caption first as a centered
    /// paragraph. An empty accumulator emits nothing.
    fn html_table_close(&mut self) {
        self.html_row_close();
        self.spans.clear();
        let Some(t) = self.html_table.take() else {
            return;
        };
        if let Some(caption) = t.caption {
            self.html_center.push(true);
            self.emit(BlockKind::Paragraph { spans: caption });
            self.html_center.pop();
        }
        if t.header.is_empty() && t.rows.is_empty() {
            return;
        }
        self.emit(BlockKind::Table {
            header: t.header,
            rows: t.rows,
        });
    }

    /// A group whose close arrives without a summary row gets one reading
    /// "Details", GitHub's fallback, inserted before the group's content.
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

    /// Whether an HTML construct is accumulating spans of its own, in
    /// which case nothing between its tags may flush as a paragraph.
    fn html_capturing(&self) -> bool {
        self.html_table.is_some()
            || self.html_pre.is_some()
            || self.html_heading.is_some()
            || self.html_dt
            || self.in_summary
            || !self.html_lists.is_empty()
    }

    /// Closes what an unclosed document leaves open, so content never
    /// silently vanishes: tables, headings, pre bodies, list items and
    /// terms emit, details groups fail open.
    fn finish(&mut self) {
        if self.html_table.is_some() {
            self.html_table_close();
        }
        self.html_pre_close();
        self.html_heading_close();
        self.html_dt_close();
        while !self.html_lists.is_empty() {
            self.html_li_close();
            self.html_lists.pop();
        }
        self.flush_spans();
        while let Some(id) = self.details_stack.pop() {
            self.details[id as usize].open = true;
            self.summarize(id);
        }
    }

    fn style(&self) -> Span {
        let mut span = Span::plain("");
        span.bold = self.bold > 0 || self.html_dt;
        span.italic = self.italic > 0;
        span.strike = self.strike > 0;
        span.underline = self.html_underline > 0;
        span.mark = self.html_mark > 0;
        span.code = self.html_code > 0;
        span.script = if self.html_sub > 0 {
            SpanScript::Sub
        } else if self.html_sup > 0 {
            SpanScript::Sup
        } else if self.html_small > 0 {
            SpanScript::Small
        } else {
            SpanScript::None
        };
        span.link = self.link.clone();
        span.range = self.current.0 as u32..self.current.1 as u32;
        span
    }

    /// Appends a span, merging with the previous one when styles match.
    /// Every span still owns its text here; `seal_blocks` decides
    /// borrowing once the document is complete.
    fn push(&mut self, span: Span) {
        if span.raw_text().is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            let same_style = last.bold == span.bold
                && last.italic == span.italic
                && last.strike == span.strike
                && last.underline == span.underline
                && last.mark == span.mark
                && last.code == span.code
                && last.math == span.math
                && last.script == span.script
                && last.link == span.link
                && last.image.is_none()
                && span.image.is_none()
                && span.raw_text() != "\n"
                && last.raw_text() != "\n";
            if same_style {
                last.raw_text_mut().push_str(span.raw_text());
                if last.range.is_empty() {
                    last.range = span.range;
                } else if !span.range.is_empty() {
                    last.range.end = last.range.end.max(span.range.end);
                }
                return;
            }
        }
        self.spans.push(span);
    }

    fn flush_spans(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        if let Some(label) = self.footnote.clone() {
            self.emit(BlockKind::FootnoteDef { label, spans });
            return;
        }
        // A paragraph that is one plain image and whitespace stays a block
        // image; links, size attributes, or centering keep it inline.
        if self.lists.is_empty() && self.html_center.is_empty() {
            let solo = spans
                .iter()
                .filter(|s| s.image.is_none())
                .all(|s| s.raw_text().trim().is_empty());
            let images: Vec<&Span> = spans.iter().filter(|s| s.image.is_some()).collect();
            if solo && images.len() == 1 {
                let span = images[0];
                let image = span.image.as_ref().expect("image span");
                if span.link.is_none() && image.width.is_none() && image.height.is_none() {
                    let kind = BlockKind::Image {
                        path: image.src.clone(),
                        alt: span.raw_text().to_string(),
                    };
                    self.emit(kind);
                    return;
                }
            }
        }
        if !self.lists.is_empty() {
            let depth = (self.lists.len() - 1) as u8;
            let marker = self
                .item_markers
                .last_mut()
                .and_then(Option::take)
                .unwrap_or(Marker::Bullet);
            self.emit(BlockKind::ListItem {
                marker,
                depth,
                spans,
            });
            return;
        }
        self.emit(BlockKind::Paragraph { spans });
    }

    /// Source range of a block: the extent of its spans' ranges where it has
    /// spans, otherwise the range of the event emitting it. Emission happens
    /// on End events, whose pulldown range covers the whole element.
    fn block_range(&self, kind: &BlockKind) -> Range<usize> {
        match kind {
            BlockKind::Heading { spans, .. }
            | BlockKind::Paragraph { spans }
            | BlockKind::ListItem { spans, .. }
            | BlockKind::FootnoteDef { spans, .. }
            | BlockKind::Summary { spans, .. } => extent(spans.iter()),
            BlockKind::Table { header, rows } => extent(
                header
                    .iter()
                    .flatten()
                    .chain(rows.iter().flatten().flatten()),
            ),
            _ => self.current.0..self.current.1,
        }
    }

    fn emit(&mut self, kind: BlockKind) {
        // A summary row belongs to the group enclosing its own; it is
        // the toggle, visible while its group is closed.
        let details = match &kind {
            BlockKind::Summary { group, .. } => self.details[*group as usize].parent,
            _ => self.details_stack.last().copied(),
        };
        self.blocks.push(Block {
            quote_depth: self.quote_depth,
            alert: self.alerts.iter().rev().find_map(|a| *a),
            range: self.block_range(&kind),
            centered: self.html_center.iter().any(|&c| c),
            details,
            kind,
        });
    }
}

/// Whether an attribute is present at all, valued or bare: `open`,
/// `open=""`, `open="open"`.
fn html_flag(attrs: &str, name: &str) -> bool {
    if html_attr(attrs, name).is_some() {
        return true;
    }
    attrs
        .to_ascii_lowercase()
        .split_whitespace()
        .any(|word| word == name)
}

/// One attribute's value from a tag's attribute text: `name="v"`, `name='v'`,
/// or unquoted `name=v`.
fn html_attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let start = from + at;
        let before_ok = start == 0
            || lower[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        let after = &attrs[start + name.len()..];
        let after_trim = after.trim_start();
        if before_ok && after_trim.starts_with('=') {
            let value = after_trim[1..].trim_start();
            return Some(match value.chars().next() {
                Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or("").to_string(),
                _ => value
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
                    .to_string(),
            });
        }
        from = start + name.len();
    }
    None
}

/// Smallest range covering every nonempty span range.
fn extent<'a>(spans: impl Iterator<Item = &'a Span>) -> Range<usize> {
    let mut start = u32::MAX;
    let mut end = 0;
    for span in spans {
        if !span.range.is_empty() {
            start = start.min(span.range.start);
            end = end.max(span.range.end);
        }
    }
    if start == u32::MAX {
        0..0
    } else {
        start as usize..end as usize
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn alert_kind(kind: BlockQuoteKind) -> AlertKind {
    match kind {
        BlockQuoteKind::Note => AlertKind::Note,
        BlockQuoteKind::Tip => AlertKind::Tip,
        BlockQuoteKind::Important => AlertKind::Important,
        BlockQuoteKind::Warning => AlertKind::Warning,
        BlockQuoteKind::Caution => AlertKind::Caution,
    }
}

/// GitHub-style slug: lowercase, alphanumerics kept, spaces to hyphens.
/// Runs at heading end, before sealing, so every span still owns its text.
fn slug(spans: &[Span]) -> String {
    let text: String = spans.iter().map(|s| s.raw_text()).collect();
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn replace_emoji(text: &str) -> String {
    if !text.contains(':') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        let (before, from) = rest.split_at(start);
        out.push_str(before);
        match from[1..].find(':') {
            Some(len) => {
                let code = &from[1..1 + len];
                let valid = !code.is_empty()
                    && code
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_+-".contains(c));
                match valid.then(|| lookup_emoji(code)).flatten() {
                    Some(emoji) => {
                        out.push_str(emoji);
                        rest = &from[len + 2..];
                    }
                    None => {
                        out.push(':');
                        rest = &from[1..];
                    }
                }
            }
            None => {
                out.push_str(from);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn lookup_emoji(code: &str) -> Option<&'static str> {
    EMOJI
        .binary_search_by_key(&code, |(k, _)| k)
        .ok()
        .map(|i| EMOJI[i].1)
}

/// Common GitHub shortcodes, sorted by name for binary search.
static EMOJI: &[(&str, &str)] = &[
    ("+1", "\u{1F44D}"),
    ("-1", "\u{1F44E}"),
    ("100", "\u{1F4AF}"),
    ("airplane", "\u{2708}\u{FE0F}"),
    ("alarm_clock", "\u{23F0}"),
    ("angry", "\u{1F620}"),
    ("art", "\u{1F3A8}"),
    ("bell", "\u{1F514}"),
    ("bike", "\u{1F6B2}"),
    ("bird", "\u{1F426}"),
    ("blush", "\u{1F60A}"),
    ("book", "\u{1F4D6}"),
    ("books", "\u{1F4DA}"),
    ("bug", "\u{1F41B}"),
    ("bulb", "\u{1F4A1}"),
    ("cake", "\u{1F370}"),
    ("calendar", "\u{1F4C5}"),
    ("car", "\u{1F697}"),
    ("cat", "\u{1F431}"),
    ("chart_with_downwards_trend", "\u{1F4C9}"),
    ("chart_with_upwards_trend", "\u{1F4C8}"),
    ("clap", "\u{1F44F}"),
    ("cloud", "\u{2601}\u{FE0F}"),
    ("coffee", "\u{2615}"),
    ("construction", "\u{1F6A7}"),
    ("cry", "\u{1F622}"),
    ("dog", "\u{1F436}"),
    ("eyes", "\u{1F440}"),
    ("fire", "\u{1F525}"),
    ("fish", "\u{1F41F}"),
    ("gear", "\u{2699}\u{FE0F}"),
    ("ghost", "\u{1F47B}"),
    ("gift", "\u{1F381}"),
    ("grin", "\u{1F601}"),
    ("hammer", "\u{1F528}"),
    ("heart", "\u{2764}\u{FE0F}"),
    ("hourglass", "\u{231B}"),
    ("house", "\u{1F3E0}"),
    ("joy", "\u{1F602}"),
    ("key", "\u{1F511}"),
    ("link", "\u{1F517}"),
    ("lock", "\u{1F512}"),
    ("mag", "\u{1F50D}"),
    ("mega", "\u{1F4E3}"),
    ("memo", "\u{1F4DD}"),
    ("moneybag", "\u{1F4B0}"),
    ("moon", "\u{1F319}"),
    ("muscle", "\u{1F4AA}"),
    ("package", "\u{1F4E6}"),
    ("pencil2", "\u{270F}\u{FE0F}"),
    ("penguin", "\u{1F427}"),
    ("pizza", "\u{1F355}"),
    ("pray", "\u{1F64F}"),
    ("question", "\u{2753}"),
    ("rage", "\u{1F621}"),
    ("rainbow", "\u{1F308}"),
    ("robot", "\u{1F916}"),
    ("rocket", "\u{1F680}"),
    ("rofl", "\u{1F923}"),
    ("skull", "\u{1F480}"),
    ("smile", "\u{1F604}"),
    ("snowflake", "\u{2744}\u{FE0F}"),
    ("sob", "\u{1F62D}"),
    ("sparkles", "\u{2728}"),
    ("star", "\u{2B50}"),
    ("sunny", "\u{2600}\u{FE0F}"),
    ("tada", "\u{1F389}"),
    ("thinking", "\u{1F914}"),
    ("thumbsdown", "\u{1F44E}"),
    ("thumbsup", "\u{1F44D}"),
    ("truck", "\u{1F69A}"),
    ("turtle", "\u{1F422}"),
    ("warning", "\u{26A0}\u{FE0F}"),
    ("wave", "\u{1F44B}"),
    ("white_check_mark", "\u{2705}"),
    ("wink", "\u{1F609}"),
    ("wrench", "\u{1F527}"),
    ("x", "\u{274C}"),
    ("zap", "\u{26A1}"),
];

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::doc::model::*;

    fn to_usize(range: &std::ops::Range<u32>) -> std::ops::Range<usize> {
        range.start as usize..range.end as usize
    }

    #[test]
    fn heading_maps_level_spans_anchor() {
        let d = parse("## Hello *World*");
        let BlockKind::Heading {
            level,
            spans,
            anchor,
        } = &d.blocks[0].kind
        else {
            panic!("expected heading, got {:?}", d.blocks)
        };
        assert_eq!(*level, 2);
        assert_eq!(spans[0].text(&d.source), "Hello ");
        assert!(spans[1].italic && spans[1].text(&d.source) == "World");
        assert_eq!(anchor, "hello-world");
    }

    #[test]
    fn paragraph_span_styles() {
        let d = parse("plain **bold** *ital* ~~gone~~ `code`");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "plain ");
        assert!(!spans[0].bold && !spans[0].italic && !spans[0].strike && !spans[0].code);
        assert!(spans[1].bold && spans[1].text(&d.source) == "bold");
        assert!(spans[3].italic && spans[3].text(&d.source) == "ital");
        assert!(spans[5].strike && spans[5].text(&d.source) == "gone");
        assert!(spans[7].code && spans[7].text(&d.source) == "code");
    }

    #[test]
    fn nested_quote_sets_depth() {
        let d = parse("> > deep");
        assert_eq!(d.blocks[0].quote_depth, 2);
    }

    #[test]
    fn alert_marker_classifies_quote() {
        let d = parse("> [!WARNING]\n> careful");
        assert!(matches!(d.blocks[0].alert, Some(AlertKind::Warning)));
        assert_eq!(d.blocks[0].quote_depth, 1);
    }

    #[test]
    fn fenced_code_language_and_lines() {
        let d = parse("```rust\nfn main() {}\nlet x = 1;\n```");
        let BlockKind::CodeBlock {
            language,
            lines,
            highlights,
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(
            lines.iter(&d.source).collect::<Vec<_>>(),
            ["fn main() {}", "let x = 1;"]
        );
        // Highlights come from the load budget pass, never from parse.
        assert!(highlights.is_empty());
    }

    #[test]
    fn task_list_item() {
        let d = parse("- [x] done");
        let BlockKind::ListItem {
            marker: Marker::Task { checked },
            depth,
            ..
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert!(*checked);
        assert_eq!(*depth, 0);
    }

    #[test]
    fn nested_and_ordered_lists() {
        let d = parse("- a\n  - b\n\n1. one\n2. two");
        let BlockKind::ListItem { depth: d0, .. } = &d.blocks[0].kind else {
            panic!()
        };
        let BlockKind::ListItem { depth: d1, .. } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!((*d0, *d1), (0, 1));
        let BlockKind::ListItem {
            marker: Marker::Number(n2),
            ..
        } = &d.blocks[3].kind
        else {
            panic!()
        };
        assert_eq!(*n2, 2);
    }

    #[test]
    fn table_header_and_rows() {
        let d = parse("|a|b|\n|-|-|\n|c|**d**|");
        let BlockKind::Table { header, rows } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(header.len(), 2);
        assert_eq!(header[0][0].text(&d.source), "a");
        assert_eq!(rows.len(), 1);
        assert!(rows[0][1][0].bold);
    }

    #[test]
    fn html_table_with_thead_maps_header_and_rows() {
        let d = parse(
            "<table>\n<thead><tr><th>Name</th><th>Size</th></tr></thead>\n\
             <tbody><tr><td>alpha</td><td> 12 </td></tr></tbody>\n</table>",
        );
        let BlockKind::Table { header, rows } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(header.len(), 2);
        assert_eq!(header[0][0].text(&d.source), "Name");
        assert_eq!(header[1][0].text(&d.source), "Size");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0][0].text(&d.source), "alpha");
        assert_eq!(rows[0][1][0].text(&d.source), "12", "cells trim padding");
    }

    #[test]
    fn html_table_leading_th_row_is_the_header() {
        let d = parse(
            "<table><tr><th>K</th><th>V</th></tr>\
             <tr><td>a</td><td>1</td></tr></table>",
        );
        let BlockKind::Table { header, rows } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(header[0][0].text(&d.source), "K");
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn html_table_without_header_renders_all_rows_as_body() {
        let d = parse(
            "<table><tr><td>a</td><td>1</td></tr>\
             <tr><td>b</td><td>2</td></tr></table>",
        );
        let BlockKind::Table { header, rows } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(header.is_empty());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][0][0].text(&d.source), "b");
    }

    #[test]
    fn html_table_cell_carries_an_inline_image() {
        let d = parse(
            "<table><tr><td>\
             <img src=\"badge.svg\" alt=\"ci\" width=\"90\">\
             </td></tr></table>",
        );
        let BlockKind::Table { rows, .. } = &d.blocks[0].kind else {
            panic!()
        };
        let image = rows[0][0]
            .iter()
            .find_map(|s| s.image.as_ref())
            .expect("cell keeps its image span");
        assert_eq!(image.src, "badge.svg");
        assert_eq!(image.width, Some(90));
    }

    #[test]
    fn html_table_caption_precedes_as_centered_paragraph() {
        let d = parse("<table><caption>Release sizes</caption><tr><td>a</td></tr></table>");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "Release sizes");
        assert!(d.blocks[0].centered);
        assert!(matches!(d.blocks[1].kind, BlockKind::Table { .. }));
    }

    #[test]
    fn html_table_colspan_occupies_one_slot() {
        let d = parse("<table><tr><td colspan=\"2\">wide</td><td>x</td></tr></table>");
        let BlockKind::Table { rows, .. } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn html_block_tags_inside_cells_flatten_to_text() {
        let d = parse(
            "<table><tr><td><ul><li>one</li><li>two</li></ul></td>\
             <td><p>x</p>y</td></tr></table>",
        );
        assert_eq!(d.blocks.len(), 1, "nothing escapes the table");
        let BlockKind::Table { rows, .. } = &d.blocks[0].kind else {
            panic!()
        };
        let first: String = rows[0][0].iter().map(|s| s.text(&d.source)).collect();
        assert!(first.contains("one") && first.contains("two"));
        let second: String = rows[0][1].iter().map(|s| s.text(&d.source)).collect();
        assert!(second.contains('x') && second.contains('y'));
    }

    #[test]
    fn html_headings_map_with_slug_anchors() {
        let d = parse("<h2>Deep Dive</h2>\n<h4>Sub Part</h4>");
        let BlockKind::Heading {
            level,
            spans,
            anchor,
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert_eq!(*level, 2);
        assert_eq!(spans[0].text(&d.source), "Deep Dive");
        assert_eq!(anchor, "deep-dive");
        let BlockKind::Heading { level, .. } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(*level, 4);
    }

    #[test]
    fn html_lists_nest_and_number() {
        let d = parse("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>");
        let items: Vec<(u8, String)> = d
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::ListItem { depth, spans, .. } => {
                    (*depth, spans.iter().map(|s| s.text(&d.source)).collect())
                }
                other => panic!("not a list item: {other:?}"),
            })
            .collect();
        assert_eq!(
            items,
            vec![
                (0, "a".to_string()),
                (1, "b".to_string()),
                (0, "c".to_string())
            ]
        );

        let d = parse("<ol start=\"3\"><li>x</li><li>y</li></ol>");
        let numbers: Vec<u64> = d
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::ListItem {
                    marker: Marker::Number(n),
                    ..
                } => *n,
                other => panic!("not a numbered item: {other:?}"),
            })
            .collect();
        assert_eq!(numbers, vec![3, 4]);
    }

    #[test]
    fn html_blockquotes_stack_depth() {
        let d =
            parse("<blockquote>\n\nouter\n\n<blockquote>\n\ninner\n\n</blockquote>\n</blockquote>");
        assert_eq!(d.blocks[0].quote_depth, 1);
        assert_eq!(d.blocks[1].quote_depth, 2);
    }

    #[test]
    fn html_pre_code_becomes_a_code_block() {
        let d =
            parse("<pre><code class=\"language-rust\">fn main() {}\n&lt;tag&gt;\n</code></pre>");
        let BlockKind::CodeBlock {
            language,
            lines,
            highlights,
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.line(&d.source, 0), "fn main() {}");
        assert_eq!(lines.line(&d.source, 1), "<tag>", "entities decode");
        assert!(
            highlights.is_empty(),
            "colors arrive from the lazy pipeline"
        );
    }

    #[test]
    fn html_hr_is_a_rule() {
        let d = parse("before\n\n<hr>\n\nafter");
        assert!(d.blocks.iter().any(|b| matches!(b.kind, BlockKind::Rule)));
    }

    #[test]
    fn html_dl_maps_terms_and_definitions() {
        let d = parse("<dl><dt>Term</dt><dd>Its definition</dd></dl>");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "Term");
        assert!(spans[0].bold, "terms read bold");
        let BlockKind::ListItem {
            marker: Marker::None,
            depth: 0,
            spans,
        } = &d.blocks[1].kind
        else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "Its definition");
    }

    #[test]
    fn html_inline_set_maps_to_span_styles() {
        let d = parse(
            "<u>under</u> a <ins>inserted</ins> b <s>gone</s> c <mark>lit</mark> \
             d <small>fine</small> e <q>quoted</q> f <cite>cited</cite> g <var>x</var> \
             h <samp>out</samp> i <tt>tele</tt>",
        );
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let by_text = |t: &str| {
            spans
                .iter()
                .find(|s| s.text(&d.source) == t)
                .unwrap_or_else(|| panic!("no span {t:?}"))
        };
        assert!(by_text("under").underline);
        assert!(by_text("inserted").underline);
        assert!(by_text("gone").strike);
        assert!(by_text("lit").mark);
        assert_eq!(by_text("fine").script, SpanScript::Small);
        assert!(by_text("cited").italic);
        assert!(by_text("x").italic);
        assert!(by_text("out").code);
        assert!(by_text("tele").code);
        let joined: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert!(
            joined.contains("\u{201C}quoted\u{201D}"),
            "q wraps in typographic quotes: {joined:?}"
        );
    }

    #[test]
    fn html_picture_reduces_to_its_img() {
        let d = parse(
            "<picture><source srcset=\"x.webp\">\
             <img src=\"logo.png\" alt=\"logo\"></picture>",
        );
        let BlockKind::Image { path, alt } = &d.blocks[0].kind else {
            panic!("picture yields its image: {:?}", d.blocks[0].kind)
        };
        assert_eq!(path, "logo.png");
        assert_eq!(alt, "logo");
    }

    #[test]
    fn html_checkbox_input_degrades_to_text() {
        let d = parse("<input type=\"checkbox\" disabled> pick me");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let joined: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert_eq!(joined.trim(), "pick me");
    }

    #[test]
    fn details_groups_nest_and_stamp_their_blocks() {
        let d = parse(
            "<details>\n<summary>Outer</summary>\n\nText inside.\n\n\
             <details open>\n<summary>Inner</summary>\n\nDeep text.\n\n</details>\n</details>",
        );
        assert_eq!(d.details.len(), 2);
        assert_eq!(d.details[0].parent, None);
        assert!(!d.details[0].open, "closed without the open attribute");
        assert_eq!(d.details[1].parent, Some(0));
        assert!(d.details[1].open);
        let BlockKind::Summary { spans, group } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "Outer");
        assert_eq!(*group, 0);
        assert_eq!(
            d.blocks[0].details, None,
            "a summary sits outside its own group"
        );
        assert_eq!(d.blocks[1].details, Some(0));
        let BlockKind::Summary { group, .. } = &d.blocks[2].kind else {
            panic!()
        };
        assert_eq!(*group, 1);
        assert_eq!(d.blocks[2].details, Some(0));
        assert_eq!(d.blocks[3].details, Some(1));
    }

    #[test]
    fn a_details_without_summary_synthesizes_one() {
        let d = parse("<details>\n\nHidden prose.\n\n</details>");
        let BlockKind::Summary { spans, group } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "Details");
        assert_eq!(*group, 0);
        assert_eq!(d.blocks[1].details, Some(0));
    }

    #[test]
    fn an_unclosed_details_stays_open() {
        let d = parse("<details>\n<summary>Broken</summary>\n\nStill visible.");
        assert!(d.details[0].open, "a broken document fails visible");
        assert!(d.block_visible(1));
    }

    #[test]
    fn block_visibility_walks_the_details_chain() {
        let d = parse(
            "<details>\n<summary>Outer</summary>\n\nBody.\n\n\
             <details open>\n<summary>Inner</summary>\n\nDeep.\n\n</details>\n</details>",
        );
        assert!(d.block_visible(0), "the toggle row of a closed group shows");
        assert!(!d.block_visible(1));
        assert!(
            !d.block_visible(2),
            "an inner summary hides with its parent"
        );
        assert!(
            !d.block_visible(3),
            "an open inner group inside a closed outer stays hidden"
        );
        let mut open = d;
        open.toggle_details(0);
        assert!(open.block_visible(1) && open.block_visible(3));
    }

    #[test]
    fn reveal_chain_opens_every_closed_ancestor() {
        let mut d = parse(
            "<details>\n<summary>Outer</summary>\n\n\
             <details>\n<summary>Inner</summary>\n\nDeep.\n\n</details>\n</details>",
        );
        let deep = d
            .blocks
            .iter()
            .position(|b| b.details == Some(1))
            .expect("the deep paragraph");
        assert!(d.reveal(deep));
        assert!(d.details[0].open && d.details[1].open);
        assert!(d.block_visible(deep));
        assert!(!d.reveal(deep), "already visible changes nothing");
    }

    #[test]
    fn an_unclosed_html_table_still_emits() {
        let d = parse("<table><tr><td>alpha</td>");
        let BlockKind::Table { header, rows } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(header.is_empty());
        assert_eq!(rows[0][0][0].text(&d.source), "alpha");
    }

    #[test]
    fn rule_and_image() {
        let d = parse("***\n\n![alt text](img/logo.png)");
        assert!(matches!(d.blocks[0].kind, BlockKind::Rule));
        let BlockKind::Image { path, alt } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(path, "img/logo.png");
        assert_eq!(alt, "alt text");
    }

    #[test]
    fn links_and_heading_anchors() {
        let d = parse("[text](https://a.tld) and [sec](#my-section)");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].link.as_deref(), Some("https://a.tld"));
        assert_eq!(spans[2].link.as_deref(), Some("#my-section"));
    }

    #[test]
    fn bare_url_autolinks() {
        let d = parse("visit https://example.com now");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let linked: Vec<_> = spans.iter().filter(|s| s.link.is_some()).collect();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].text(&d.source), "https://example.com");
        assert_eq!(linked[0].link.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn bare_urls_link_in_order_whichever_scheme_comes_first() {
        for src in [
            "see https://a.tld then http://b.tld",
            "see http://a.tld then https://b.tld",
        ] {
            let d = parse(src);
            let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
                panic!()
            };
            let linked: Vec<_> = spans.iter().filter(|s| s.link.is_some()).collect();
            assert_eq!(linked.len(), 2, "both urls link in {src:?}");
            assert!(
                linked[0].text(&d.source).contains("a.tld"),
                "in source order"
            );
            assert!(
                linked[1].text(&d.source).contains("b.tld"),
                "in source order"
            );
        }
    }

    #[test]
    fn deep_quote_nesting_never_panics() {
        let src = "> ".repeat(300) + "text";
        let d = parse(src.as_str());
        assert!(!d.blocks.is_empty());
    }

    #[test]
    fn footnote_reference_and_definition() {
        let d = parse("text[^1]\n\n[^1]: the note");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let fr = spans.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(fr.link.as_deref(), Some("footnote:1"));
        let BlockKind::FootnoteDef { label, spans } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(label, "1");
        assert_eq!(spans[0].text(&d.source), "the note");
    }

    #[test]
    fn inline_and_block_math() {
        let d = parse("value $x^2$ here\n\n$$\nE=mc^2\n$$");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let m = spans.iter().find(|s| s.math).unwrap();
        assert_eq!(m.text(&d.source), "x^2");
        let BlockKind::MathBlock { tex } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(tex.trim(), "E=mc^2");
    }

    #[test]
    fn frontmatter_entries() {
        let d = parse("---\ntitle: Test\ntags: a b\n---\n\n# Hi");
        let BlockKind::Frontmatter { entries } = &d.blocks[0].kind else {
            panic!("expected frontmatter, got {:?}", d.blocks)
        };
        assert_eq!(entries[0], ("title".into(), "Test".into()));
        assert_eq!(entries[1], ("tags".into(), "a b".into()));
        assert!(matches!(d.blocks[1].kind, BlockKind::Heading { .. }));
    }

    #[test]
    fn emoji_shortcodes_replaced() {
        let d = parse("ship it :tada: :rocket:");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let text: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert!(text.contains('\u{1F389}'), "tada missing in {text:?}");
        assert!(text.contains('\u{1F680}'), "rocket missing in {text:?}");
    }

    #[test]
    fn centered_html_block_with_linked_sized_images() {
        let d = parse(
            "<p align=\"center\">\n<a href=\"https://x.tld\"><img src=\"https://img.tld/a.svg\" height=\"20\"></a>\n<img src=\"b.png\" width=\"64\" height=\"32\">\n</p>",
        );
        let b = &d.blocks[0];
        assert!(b.centered, "block centered");
        let BlockKind::Paragraph { spans } = &b.kind else {
            panic!("expected paragraph, got {:?}", b.kind)
        };
        let images: Vec<&Span> = spans.iter().filter(|s| s.image.is_some()).collect();
        assert_eq!(images.len(), 2);
        let first = images[0].image.as_ref().unwrap();
        assert_eq!(first.src, "https://img.tld/a.svg");
        assert_eq!(first.height, Some(20));
        assert_eq!(images[0].link.as_deref(), Some("https://x.tld"));
        let second = images[1].image.as_ref().unwrap();
        assert_eq!((second.width, second.height), (Some(64), Some(32)));
        assert!(images[1].link.is_none());
    }

    #[test]
    fn html_tags_split_across_lines_reassemble() {
        let d = parse(
            "<p align=\"center\">\n<img src=\"logo.svg\"\n    height=\"130\">\n<a href=\"https://x.tld\">\n<img src=\"b.svg\"\n alt=\"B\"></a>\n</p>",
        );
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!("{:?}", d.blocks[0].kind)
        };
        let images: Vec<&Span> = spans.iter().filter(|s| s.image.is_some()).collect();
        assert_eq!(images.len(), 2, "both split images parsed");
        assert_eq!(images[0].image.as_ref().unwrap().height, Some(130));
        assert_eq!(images[1].link.as_deref(), Some("https://x.tld"));
        assert!(
            !spans.iter().any(|s| s.text(&d.source).contains("height")),
            "no leaked attribute text"
        );
    }

    #[test]
    fn inline_html_styles_and_scripts() {
        let d = parse("a <b>bold</b> H<sub>2</sub>O x<sup>9</sup> <kbd>Ctrl</kbd>");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.bold && s.text(&d.source) == "bold"));
        assert!(spans
            .iter()
            .any(|s| s.script == SpanScript::Sub && s.text(&d.source) == "2"));
        assert!(spans
            .iter()
            .any(|s| s.script == SpanScript::Sup && s.text(&d.source) == "9"));
        assert!(spans.iter().any(|s| s.code && s.text(&d.source) == "Ctrl"));
    }

    #[test]
    fn markdown_image_beside_text_stays_inline() {
        let d = parse("- coverage: ![badge](https://img.tld/c.svg)");
        let BlockKind::ListItem { spans, .. } = &d.blocks[0].kind else {
            panic!("{:?}", d.blocks[0].kind)
        };
        let img = spans
            .iter()
            .find(|s| s.image.is_some())
            .expect("inline image span");
        assert_eq!(img.image.as_ref().unwrap().src, "https://img.tld/c.svg");
        assert_eq!(img.text(&d.source), "badge");
    }

    #[test]
    fn bare_and_linked_image_paragraphs() {
        let bare = parse("![alt text](pic.png)");
        assert!(matches!(&bare.blocks[0].kind, BlockKind::Image { .. }));
        // A linked badge alone in a paragraph stays inline so it is
        // clickable.
        let linked = parse("[![b](https://img.tld/b.svg)](https://x.tld)");
        let BlockKind::Paragraph { spans } = &linked.blocks[0].kind else {
            panic!("{:?}", linked.blocks[0].kind)
        };
        assert_eq!(spans[0].link.as_deref(), Some("https://x.tld"));
        assert!(spans[0].image.is_some());
    }

    #[test]
    fn html_stripped_inner_text_kept() {
        let d = parse("before <b>mid</b> after");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let text: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert_eq!(text, "before mid after");
    }

    #[test]
    fn br_becomes_line_break_span() {
        let d = parse("line<br>break");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.text(&d.source) == "\n"));
    }

    #[test]
    fn smart_punctuation_applied() {
        let d = parse("\"quote\"");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans[0].text(&d.source).starts_with('\u{201C}'));
    }

    #[test]
    fn ranges_index_the_source() {
        let src = "# Title\n\npara **bold** end\n\n```rust\nlet x = 1;\n```\n\n> quoted";
        let d = parse(src);
        assert_eq!(&*d.source, src);
        assert_eq!(&src[d.blocks[0].range.clone()], "Title");
        let BlockKind::Paragraph { spans } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(&src[to_usize(&spans[0].range)], "para ");
        assert_eq!(&src[to_usize(&spans[1].range)], "bold");
        let code = &src[d.blocks[2].range.clone()];
        assert!(code.starts_with("```rust"), "code range was {code:?}");
        assert!(code.trim_end().ends_with("```"), "code range was {code:?}");
        assert_eq!(&src[d.blocks[3].range.clone()], "quoted");
    }

    #[test]
    fn unclosed_markup_never_panics() {
        parse("**bold *ital ~~strike `code $math");
        parse("| broken | table\n|---|\n| x");
        parse("> [!BOGUS]\n> text");
    }

    #[test]
    fn empty_input_is_empty_document() {
        assert_eq!(parse("").blocks.len(), 0);
    }

    fn para_spans(d: &Document) -> &[Span] {
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!("expected paragraph, got {:?}", d.blocks)
        };
        spans
    }

    #[test]
    fn untransformed_spans_borrow_the_source() {
        let d = parse("plain **bold** and *italic* text");
        for span in para_spans(&d) {
            assert!(
                span.is_verbatim(),
                "span {:?} should borrow",
                span.text(&d.source)
            );
        }
        assert_eq!(para_spans(&d)[0].text(&d.source), "plain ");
        assert_eq!(para_spans(&d)[1].text(&d.source), "bold");
    }

    #[test]
    fn bare_urls_borrow_around_the_link() {
        let d = parse("see https://a.example now");
        let spans = para_spans(&d);
        assert_eq!(spans.len(), 3);
        for span in spans {
            assert!(span.is_verbatim(), "{:?}", span.text(&d.source));
        }
        assert_eq!(spans[1].text(&d.source), "https://a.example");
        assert_eq!(spans[1].link.as_deref(), Some("https://a.example"));
    }

    #[test]
    fn multibyte_spans_borrow_and_slice_cleanly() {
        let d = parse("# 你好 🚀 world");
        let BlockKind::Heading { spans, .. } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans[0].is_verbatim());
        assert_eq!(spans[0].text(&d.source), "你好 🚀 world");
    }

    #[test]
    fn smart_punctuation_owns_its_text() {
        let d = parse("\"quoted\" words");
        let spans = para_spans(&d);
        assert!(!spans[0].is_verbatim(), "smart quotes rewrite the text");
        assert_eq!(spans[0].text(&d.source), "\u{201C}quoted\u{201D} words");
    }

    #[test]
    fn decoded_entities_own_their_text() {
        let d = parse("AT&amp;T works");
        let spans = para_spans(&d);
        assert!(!spans[0].is_verbatim(), "the entity decodes away");
        assert_eq!(spans[0].text(&d.source), "AT&T works");
    }

    #[test]
    fn emoji_shortcodes_own_their_text() {
        let d = parse("ship it :tada: today");
        let spans = para_spans(&d);
        assert!(!spans[0].is_verbatim());
        assert_eq!(spans[0].text(&d.source), "ship it \u{1F389} today");
    }

    #[test]
    fn merge_across_a_soft_break_owns() {
        let d = parse("first line\nsecond line");
        let spans = para_spans(&d);
        assert_eq!(spans[0].text(&d.source), "first line second line");
        assert!(!spans[0].is_verbatim(), "the newline became a space");
    }

    #[test]
    fn contiguous_verbatim_text_borrows_whole() {
        // 2 * 3 never opens emphasis, so however pulldown splits the
        // events, the merged text equals the source slice and borrows.
        let d = parse("the product 2 * 3 * 4 stands");
        let spans = para_spans(&d);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].is_verbatim());
        assert_eq!(spans[0].text(&d.source), "the product 2 * 3 * 4 stands");
    }

    #[test]
    fn fenced_code_lines_borrow_the_source() {
        let d = parse("```rust\nfn main() {}\n// emoji 🚀 CJK 你好\n```");
        let BlockKind::CodeBlock { lines, .. } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(lines.is_verbatim(), "fence bodies are verbatim");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.line(&d.source, 0), "fn main() {}");
        assert_eq!(lines.line(&d.source, 1), "// emoji 🚀 CJK 你好");
    }

    #[test]
    fn indented_code_owns_its_body() {
        let d = parse("    let x = 1;\n    let y = 2;\n");
        let BlockKind::CodeBlock { lines, .. } = &d.blocks[0].kind else {
            panic!("expected code block, got {:?}", d.blocks)
        };
        assert!(!lines.is_verbatim(), "the indent is stripped");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.line(&d.source, 0), "let x = 1;");
        assert_eq!(lines.line(&d.source, 1), "let y = 2;");
    }

    #[test]
    fn synthesized_spans_stay_owned() {
        let d = parse("a  \nb");
        let spans = para_spans(&d);
        let brk = spans
            .iter()
            .find(|s| s.text(&d.source) == "\n")
            .expect("hard break span");
        assert!(!brk.is_verbatim());
    }
}
