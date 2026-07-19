//! Maps pulldown-cmark events onto the document model. Every event carries
//! its byte range in the source; spans and blocks keep those ranges so the
//! selection can slice the original markdown back out.

use std::ops::Range;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::doc::model::{
    AlertKind, Block, BlockKind, Document, Marker, Span, SpanImage, SpanScript,
};
use crate::style::highlight;

pub fn parse(source: &str) -> Document {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_GFM;
    let mut builder = Builder::default();
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        builder.event(event, range);
    }
    Document {
        blocks: builder.blocks,
        source: source.to_string(),
    }
}

/// Inline containers the builder can be inside. Paragraphs inside list items
/// or footnote definitions flush as those blocks, not as plain paragraphs.
#[derive(Default)]
struct Builder {
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
    table: Option<TableAcc>,
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

impl Builder {
    fn event(&mut self, event: Event, range: Range<usize>) {
        self.current = (range.start, range.end);
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(text) => self.push(Span {
                text: text.into_string(),
                code: true,
                ..self.style()
            }),
            Event::InlineMath(tex) => self.push(Span {
                text: tex.into_string(),
                math: true,
                ..self.style()
            }),
            Event::DisplayMath(tex) => {
                self.flush_spans();
                self.emit(BlockKind::MathBlock {
                    tex: tex.into_string(),
                });
            }
            Event::FootnoteReference(label) => self.push(Span {
                text: label.to_string(),
                link: Some(format!("footnote:{label}")),
                ..self.style()
            }),
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
                self.quote_depth += 1;
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
                    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
                    while lines.last().is_some_and(|l| l.is_empty()) {
                        lines.pop();
                    }
                    let highlights = highlight::spans(&lines, language.as_deref());
                    self.emit(BlockKind::CodeBlock {
                        language,
                        lines,
                        highlights,
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
                    span.text = alt;
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
                self.flush_spans();
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some((_, code)) = self.code.as_mut() {
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
        while let Some(start) = rest.find("http://").or_else(|| rest.find("https://")) {
            let (before, from) = rest.split_at(start);
            if !before.is_empty() {
                self.push(Span {
                    text: before.to_string(),
                    range: base + pos..base + pos + before.len(),
                    ..self.style()
                });
            }
            let end = from
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(from.len());
            let (url, after) = from.split_at(end);
            let url = url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
            self.push(Span {
                text: url.to_string(),
                link: Some(url.to_string()),
                range: base + pos + start..base + pos + start + url.len(),
                ..self.style()
            });
            pos += start + url.len();
            rest = &from[url.len()..];
            if rest == after && rest.is_empty() {
                break;
            }
        }
        if !rest.is_empty() {
            self.push(Span {
                text: rest.to_string(),
                range: base + pos..base + pos + rest.len(),
                ..self.style()
            });
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

    /// Text between tags; HTML collapses whitespace runs, newlines included.
    fn html_text(&mut self, text: &str) {
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
        self.text(&collapsed);
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
                self.flush_spans();
                let centered =
                    html_attr(attrs, "align").is_some_and(|a| a.eq_ignore_ascii_case("center"));
                self.html_center.push(centered);
            }
            ("p" | "div", true) => {
                self.flush_spans();
                self.html_center.pop();
            }
            ("a", false) => self.link = html_attr(attrs, "href"),
            ("a", true) => self.link = None,
            ("img", false) => {
                let Some(src) = html_attr(attrs, "src") else {
                    return;
                };
                let mut span = self.style();
                span.text = html_attr(attrs, "alt").unwrap_or_default();
                span.image = Some(SpanImage {
                    src,
                    width: html_attr(attrs, "width").and_then(|v| v.parse().ok()),
                    height: html_attr(attrs, "height").and_then(|v| v.parse().ok()),
                });
                self.spans.push(span);
            }
            ("b" | "strong", false) => self.bold += 1,
            ("b" | "strong", true) => self.bold = self.bold.saturating_sub(1),
            ("i" | "em", false) => self.italic += 1,
            ("i" | "em", true) => self.italic = self.italic.saturating_sub(1),
            ("code" | "kbd", false) => self.html_code += 1,
            ("code" | "kbd", true) => self.html_code = self.html_code.saturating_sub(1),
            ("sub", false) => self.html_sub += 1,
            ("sub", true) => self.html_sub = self.html_sub.saturating_sub(1),
            ("sup", false) => self.html_sup += 1,
            ("sup", true) => self.html_sup = self.html_sup.saturating_sub(1),
            _ => {}
        }
    }

    fn style(&self) -> Span {
        Span {
            text: String::new(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: self.html_code > 0,
            math: false,
            script: if self.html_sub > 0 {
                SpanScript::Sub
            } else if self.html_sup > 0 {
                SpanScript::Sup
            } else {
                SpanScript::None
            },
            link: self.link.clone(),
            image: None,
            range: self.current.0..self.current.1,
        }
    }

    /// Appends a span, merging with the previous one when styles match.
    fn push(&mut self, span: Span) {
        if span.text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            let same_style = last.bold == span.bold
                && last.italic == span.italic
                && last.strike == span.strike
                && last.code == span.code
                && last.math == span.math
                && last.script == span.script
                && last.link == span.link
                && last.image.is_none()
                && span.image.is_none()
                && span.text != "\n"
                && last.text != "\n";
            if same_style {
                last.text.push_str(&span.text);
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
                .all(|s| s.text.trim().is_empty());
            let images: Vec<&Span> = spans.iter().filter(|s| s.image.is_some()).collect();
            if solo && images.len() == 1 {
                let span = images[0];
                let image = span.image.as_ref().expect("image span");
                if span.link.is_none() && image.width.is_none() && image.height.is_none() {
                    let kind = BlockKind::Image {
                        path: image.src.clone(),
                        alt: span.text.clone(),
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
            | BlockKind::FootnoteDef { spans, .. } => extent(spans.iter()),
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
        self.blocks.push(Block {
            quote_depth: self.quote_depth,
            alert: self.alerts.iter().rev().find_map(|a| *a),
            range: self.block_range(&kind),
            centered: self.html_center.iter().any(|&c| c),
            kind,
        });
    }
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
    let mut start = usize::MAX;
    let mut end = 0;
    for span in spans {
        if !span.range.is_empty() {
            start = start.min(span.range.start);
            end = end.max(span.range.end);
        }
    }
    if start == usize::MAX {
        0..0
    } else {
        start..end
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
fn slug(spans: &[Span]) -> String {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
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
        assert_eq!(spans[0].text, "Hello ");
        assert!(spans[1].italic && spans[1].text == "World");
        assert_eq!(anchor, "hello-world");
    }

    #[test]
    fn paragraph_span_styles() {
        let d = parse("plain **bold** *ital* ~~gone~~ `code`");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].text, "plain ");
        assert!(!spans[0].bold && !spans[0].italic && !spans[0].strike && !spans[0].code);
        assert!(spans[1].bold && spans[1].text == "bold");
        assert!(spans[3].italic && spans[3].text == "ital");
        assert!(spans[5].strike && spans[5].text == "gone");
        assert!(spans[7].code && spans[7].text == "code");
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
        assert_eq!(lines, &["fn main() {}", "let x = 1;"]);
        assert_eq!(highlights.len(), lines.len());
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
        assert_eq!(header[0][0].text, "a");
        assert_eq!(rows.len(), 1);
        assert!(rows[0][1][0].bold);
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
        assert_eq!(linked[0].text, "https://example.com");
        assert_eq!(linked[0].link.as_deref(), Some("https://example.com"));
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
        assert_eq!(spans[0].text, "the note");
    }

    #[test]
    fn inline_and_block_math() {
        let d = parse("value $x^2$ here\n\n$$\nE=mc^2\n$$");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let m = spans.iter().find(|s| s.math).unwrap();
        assert_eq!(m.text, "x^2");
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
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
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
            !spans.iter().any(|s| s.text.contains("height")),
            "no leaked attribute text"
        );
    }

    #[test]
    fn inline_html_styles_and_scripts() {
        let d = parse("a <b>bold</b> H<sub>2</sub>O x<sup>9</sup> <kbd>Ctrl</kbd>");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.bold && s.text == "bold"));
        assert!(spans
            .iter()
            .any(|s| s.script == SpanScript::Sub && s.text == "2"));
        assert!(spans
            .iter()
            .any(|s| s.script == SpanScript::Sup && s.text == "9"));
        assert!(spans.iter().any(|s| s.code && s.text == "Ctrl"));
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
        assert_eq!(img.text, "badge");
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
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "before mid after");
    }

    #[test]
    fn br_becomes_line_break_span() {
        let d = parse("line<br>break");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.text == "\n"));
    }

    #[test]
    fn smart_punctuation_applied() {
        let d = parse("\"quote\"");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans[0].text.starts_with('\u{201C}'));
    }

    #[test]
    fn ranges_index_the_source() {
        let src = "# Title\n\npara **bold** end\n\n```rust\nlet x = 1;\n```\n\n> quoted";
        let d = parse(src);
        assert_eq!(d.source, src);
        assert_eq!(&src[d.blocks[0].range.clone()], "Title");
        let BlockKind::Paragraph { spans } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(&src[spans[0].range.clone()], "para ");
        assert_eq!(&src[spans[1].range.clone()], "bold");
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
}
