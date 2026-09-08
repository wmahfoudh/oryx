//! Maps pulldown-cmark events onto the document model. Every event carries
//! its byte range in the source; spans and blocks keep those ranges so the
//! selection can slice the original markdown back out.

use std::collections::HashMap;
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
        | Options::ENABLE_GFM
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_DEFINITION_LIST;
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
        abbreviations: builder
            .abbreviations
            .into_iter()
            .map(|(_, expansion)| expansion)
            .collect(),
        ..Document::default()
    })
}

/// The currency gate over dollar-delimited math. pulldown already requires
/// non-space flanks and honors `\$`, which keeps most prices prose; the one
/// reading it shares with prose is a closing dollar glued to a following
/// number, `$5-$10` becoming math between the signs. A digit right after
/// the closing delimiter therefore rejects the event, and the builder
/// downgrades it to the text the author typed.
fn dollar_math_allowed(source: &str, range: &Range<usize>) -> bool {
    !source
        .as_bytes()
        .get(range.end)
        .is_some_and(|b| b.is_ascii_digit())
}

/// GitHub's dollar-backtick form reaches the builder as inline math whose
/// TeX keeps the backticks; they strip to the bare expression. Whitespace
/// trims either way, TeX ignores it.
fn strip_backtick_form(raw: &str) -> String {
    let inner = raw
        .strip_prefix('`')
        .and_then(|s| s.strip_suffix('`'))
        .unwrap_or(raw);
    inner.trim().to_string()
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
    /// How many headings produced each slug so far, for numbering repeats.
    anchors_seen: HashMap<String, usize>,
    /// Verbatim body accumulating between `<pre>` and its close.
    html_pre: Option<HtmlPre>,
    /// Set between a term and its close, `<dt>` or a definition list's
    /// title line; the term renders bold.
    html_dt: bool,
    /// Inside a definition list's definition; its paragraphs flush as
    /// indented items, the `<dd>` form.
    definition: bool,
    /// The `{#id}` of the open heading, its anchor in place of the slug.
    heading_id: Option<String>,
    /// Text events joined until something else arrives; the parser hands
    /// a run over in pieces wherever it tried a delimiter and gave it up,
    /// and a bare URL or an inline mark must read the run whole.
    pending: Option<PendingText>,
    /// The run being flushed: its piece boundaries as (text offset,
    /// source offset) and its source end, for `src`.
    run_map: Vec<(usize, usize)>,
    run_end: usize,
    /// The inline mark of the piece being pushed, set by `marked`.
    mark: Option<Mark>,
    /// The offset of a definition marker that is really the first colon
    /// of a `::text::` highlight line; the text after it takes the colon
    /// back so the pass reads the highlight whole.
    marker_colon: Option<usize>,
    /// The `*[label]: expansion` lines of the whole source, longest label
    /// first, gathered before parsing since a definition usually sits at
    /// the foot and the text above must know it.
    abbreviations: Vec<(String, String)>,
    /// Open HTML lists, outermost first.
    html_lists: Vec<HtmlList>,
    html_underline: u32,
    html_mark: u32,
    html_small: u32,
    image: Option<(String, String)>,
    footnote: Option<FootnoteOpen>,
    /// The number each footnote label took, at its first reference or
    /// definition, whichever came first.
    footnote_numbers: HashMap<String, u32>,
    in_metadata: bool,
    metadata: Vec<(String, String)>,
    html_block: bool,
    /// Unterminated tag carried between HTML events; pulldown delivers
    /// block HTML line by line, and attributes may wrap.
    html_tail: String,
    /// An invisible HTML form was just dropped after text ending in
    /// whitespace; the next text's leading whitespace goes with it, so
    /// the neighbors meet on one space as a browser shows them.
    html_gap: bool,
    /// One entry per open `<p>`/`<div>`, true when it centers its content.
    html_divs: Vec<HtmlDiv>,
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
/// An open `<p>` or `<div>`: whether it centers its content, and
/// whether a page break follows it (`page-break-after`).
struct HtmlDiv {
    centered: bool,
    break_after: bool,
}

/// Which side of an element a page break style puts the break on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakSide {
    Before,
    After,
}

struct HtmlList {
    ordered: bool,
    next: u64,
    /// An `<li>` is accumulating spans at this level.
    item_open: bool,
}

/// The HTML forms a page never shows: a comment, the doctype, a CDATA
/// section, a processing instruction. For text opening one, the byte
/// past its closer, or `Some(None)` when the closer has not arrived;
/// `None` for text opening none. A bare `<` before a space or a digit
/// opens nothing and stays text.
fn html_invisible(text: &str) -> Option<Option<usize>> {
    let (opener, closer) = if text.starts_with("<!--") {
        ("<!--", "-->")
    } else if text.starts_with("<![CDATA[") {
        ("<![CDATA[", "]]>")
    } else if text.starts_with("<?") {
        ("<?", "?>")
    } else if text.starts_with("<!") {
        ("<!", ">")
    } else {
        return None;
    };
    Some(
        text[opener.len()..]
            .find(closer)
            .map(|at| opener.len() + at + closer.len()),
    )
}

/// The named entities HTML text decodes, sorted by name for the
/// search: the five basic ones, the Latin-1 set (`nbsp` through
/// `yuml`, the accented letters included) and the typographic names
/// a README reaches for (dashes, quotes, the ellipsis, the bullet,
/// currency and trade marks, arrows). Anything else stays as typed.
static NAMED_ENTITIES: &[(&str, char)] = &[
    ("AElig", '\u{c6}'),
    ("Aacute", '\u{c1}'),
    ("Acirc", '\u{c2}'),
    ("Agrave", '\u{c0}'),
    ("Aring", '\u{c5}'),
    ("Atilde", '\u{c3}'),
    ("Auml", '\u{c4}'),
    ("Ccedil", '\u{c7}'),
    ("Dagger", '\u{2021}'),
    ("ETH", '\u{d0}'),
    ("Eacute", '\u{c9}'),
    ("Ecirc", '\u{ca}'),
    ("Egrave", '\u{c8}'),
    ("Euml", '\u{cb}'),
    ("Iacute", '\u{cd}'),
    ("Icirc", '\u{ce}'),
    ("Igrave", '\u{cc}'),
    ("Iuml", '\u{cf}'),
    ("Ntilde", '\u{d1}'),
    ("Oacute", '\u{d3}'),
    ("Ocirc", '\u{d4}'),
    ("Ograve", '\u{d2}'),
    ("Oslash", '\u{d8}'),
    ("Otilde", '\u{d5}'),
    ("Ouml", '\u{d6}'),
    ("THORN", '\u{de}'),
    ("Uacute", '\u{da}'),
    ("Ucirc", '\u{db}'),
    ("Ugrave", '\u{d9}'),
    ("Uuml", '\u{dc}'),
    ("Yacute", '\u{dd}'),
    ("aacute", '\u{e1}'),
    ("acirc", '\u{e2}'),
    ("acute", '\u{b4}'),
    ("aelig", '\u{e6}'),
    ("agrave", '\u{e0}'),
    ("amp", '\u{26}'),
    ("apos", '\u{27}'),
    ("aring", '\u{e5}'),
    ("atilde", '\u{e3}'),
    ("auml", '\u{e4}'),
    ("bdquo", '\u{201e}'),
    ("brvbar", '\u{a6}'),
    ("bull", '\u{2022}'),
    ("ccedil", '\u{e7}'),
    ("cedil", '\u{b8}'),
    ("cent", '\u{a2}'),
    ("check", '\u{2713}'),
    ("copy", '\u{a9}'),
    ("curren", '\u{a4}'),
    ("dagger", '\u{2020}'),
    ("darr", '\u{2193}'),
    ("deg", '\u{b0}'),
    ("divide", '\u{f7}'),
    ("eacute", '\u{e9}'),
    ("ecirc", '\u{ea}'),
    ("egrave", '\u{e8}'),
    ("emsp", '\u{2003}'),
    ("ensp", '\u{2002}'),
    ("eth", '\u{f0}'),
    ("euml", '\u{eb}'),
    ("euro", '\u{20ac}'),
    ("frac12", '\u{bd}'),
    ("frac14", '\u{bc}'),
    ("frac34", '\u{be}'),
    ("ge", '\u{2265}'),
    ("gt", '\u{3e}'),
    ("harr", '\u{2194}'),
    ("hearts", '\u{2665}'),
    ("hellip", '\u{2026}'),
    ("iacute", '\u{ed}'),
    ("icirc", '\u{ee}'),
    ("iexcl", '\u{a1}'),
    ("igrave", '\u{ec}'),
    ("infin", '\u{221e}'),
    ("iquest", '\u{bf}'),
    ("iuml", '\u{ef}'),
    ("laquo", '\u{ab}'),
    ("larr", '\u{2190}'),
    ("ldquo", '\u{201c}'),
    ("le", '\u{2264}'),
    ("lsaquo", '\u{2039}'),
    ("lsquo", '\u{2018}'),
    ("lt", '\u{3c}'),
    ("macr", '\u{af}'),
    ("mdash", '\u{2014}'),
    ("micro", '\u{b5}'),
    ("middot", '\u{b7}'),
    ("minus", '\u{2212}'),
    ("nbsp", '\u{a0}'),
    ("ndash", '\u{2013}'),
    ("ne", '\u{2260}'),
    ("not", '\u{ac}'),
    ("ntilde", '\u{f1}'),
    ("oacute", '\u{f3}'),
    ("ocirc", '\u{f4}'),
    ("ograve", '\u{f2}'),
    ("ordf", '\u{aa}'),
    ("ordm", '\u{ba}'),
    ("oslash", '\u{f8}'),
    ("otilde", '\u{f5}'),
    ("ouml", '\u{f6}'),
    ("para", '\u{b6}'),
    ("permil", '\u{2030}'),
    ("plusmn", '\u{b1}'),
    ("pound", '\u{a3}'),
    ("quot", '\u{22}'),
    ("raquo", '\u{bb}'),
    ("rarr", '\u{2192}'),
    ("rdquo", '\u{201d}'),
    ("reg", '\u{ae}'),
    ("rsaquo", '\u{203a}'),
    ("rsquo", '\u{2019}'),
    ("sbquo", '\u{201a}'),
    ("sect", '\u{a7}'),
    ("shy", '\u{ad}'),
    ("sup1", '\u{b9}'),
    ("sup2", '\u{b2}'),
    ("sup3", '\u{b3}'),
    ("szlig", '\u{df}'),
    ("thinsp", '\u{2009}'),
    ("thorn", '\u{fe}'),
    ("times", '\u{d7}'),
    ("trade", '\u{2122}'),
    ("uacute", '\u{fa}'),
    ("uarr", '\u{2191}'),
    ("ucirc", '\u{fb}'),
    ("ugrave", '\u{f9}'),
    ("uml", '\u{a8}'),
    ("uuml", '\u{fc}'),
    ("yacute", '\u{fd}'),
    ("yen", '\u{a5}'),
    ("yuml", '\u{ff}'),
    ("zwj", '\u{200d}'),
    ("zwnj", '\u{200c}'),
];

/// One entity at the head of `text`, which starts with `&`: its byte
/// length, `;` included, and the character it stands for. Numeric
/// references decode to any valid scalar value but zero; a name not
/// in the table, a missing `;` or a body longer than any entity
/// answers None and the `&` stays literal.
fn entity(text: &str) -> Option<(usize, char)> {
    const LONGEST: usize = 12;
    let end = text
        .as_bytes()
        .iter()
        .take(LONGEST)
        .position(|&b| b == b';')?;
    let body = &text[1..end];
    let c = match body.strip_prefix('#') {
        Some(number) => {
            let value = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse::<u32>().ok()?,
            };
            if value == 0 {
                return None;
            }
            char::from_u32(value)?
        }
        None => {
            let at = NAMED_ENTITIES
                .binary_search_by_key(&body, |(name, _)| name)
                .ok()?;
            NAMED_ENTITIES[at].1
        }
    };
    Some((end + 1, c))
}

/// Decodes the entities of HTML text in one pass, so `&amp;lt;` reads
/// `&lt;` and never `<`.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let from = &rest[amp..];
        match entity(from) {
            Some((len, c)) => {
                out.push(c);
                rest = &from[len..];
            }
            None => {
                out.push('&');
                rest = &from[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drops the outer whitespace a cell inherits from source formatting,
/// the ASCII kind; a no-break space is content and stays. Image spans
/// stay whole; their text is the alt.
pub(crate) fn trim_cell(spans: &mut Vec<Span>) {
    while let Some(first) = spans.first_mut() {
        if first.image.is_some() {
            break;
        }
        let trimmed = first
            .raw_text()
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .to_string();
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
        let trimmed = last
            .raw_text()
            .trim_end_matches(|c: char| c.is_ascii_whitespace())
            .to_string();
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
        let abbreviations = scan_abbreviations(&source);
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
            anchors_seen: HashMap::new(),
            html_pre: None,
            html_dt: false,
            definition: false,
            heading_id: None,
            pending: None,
            run_map: Vec::new(),
            run_end: 0,
            mark: None,
            marker_colon: None,
            abbreviations,
            html_lists: Vec::new(),
            html_underline: 0,
            html_mark: 0,
            html_small: 0,
            image: None,
            footnote: None,
            footnote_numbers: HashMap::new(),
            in_metadata: false,
            metadata: Vec::new(),
            html_block: false,
            html_tail: String::new(),
            html_gap: false,
            html_divs: Vec::new(),
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
        if !matches!(event, Event::Text(_) | Event::SoftBreak) {
            self.flush_text();
        }
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
                if dollar_math_allowed(&self.source, &range) {
                    let raw = tex.into_string();
                    let mut span = self.style();
                    span.set_text(strip_backtick_form(&raw));
                    span.math = true;
                    self.push(span);
                } else {
                    // The currency downgrade: the reader sees the dollars
                    // and digits the author typed.
                    let mut span = self.style();
                    span.set_text(self.source[range].to_string());
                    self.push(span);
                }
            }
            Event::DisplayMath(tex) => {
                if dollar_math_allowed(&self.source, &range) {
                    self.flush_spans();
                    self.emit(BlockKind::MathBlock {
                        tex: tex.into_string(),
                    });
                } else {
                    let mut span = self.style();
                    span.set_text(self.source[range].to_string());
                    self.push(span);
                }
            }
            Event::FootnoteReference(label) => {
                let number = self.footnote_number(&label);
                let mut span = self.style();
                span.set_text(number.to_string());
                span.link = Some(format!("footnote:{label}"));
                self.push(span);
            }
            Event::TaskListMarker(checked) => {
                if let Some(slot) = self.item_markers.last_mut() {
                    *slot = Some(Marker::Task {
                        checked,
                        marker: range.start,
                    });
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
            Tag::Heading { level, id, .. } => {
                self.heading = Some(heading_level(level));
                self.heading_id = id.map(|id| id.into_string());
            }
            Tag::DefinitionList => self.flush_spans(),
            Tag::DefinitionListTitle => {
                self.flush_spans();
                // The parser takes any colon at a line's start as a
                // definition marker, so a `::text::` highlight line after
                // a paragraph would make the paragraph a term. Such a
                // line keeps the paragraph plain and stays a paragraph.
                let after = &self.source[self.current.1..];
                let next = after.trim_start_matches(['\n', '\r', ' ', '\t']);
                self.html_dt = !next.starts_with("::");
            }
            Tag::DefinitionListDefinition => {
                self.flush_spans();
                let at = self.current.0;
                if self.source[at..].starts_with("::") {
                    self.marker_colon = Some(at);
                } else {
                    self.definition = true;
                }
            }
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
            Tag::FootnoteDefinition(label) => {
                let number = self.footnote_number(&label);
                self.footnote = Some(FootnoteOpen {
                    label: label.into_string(),
                    number,
                    emitted: false,
                });
            }
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
                let anchor = match self.heading_id.take() {
                    Some(id) => self.explicit_anchor(id),
                    None => self.unique_anchor(&spans),
                };
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
            TagEnd::DefinitionListTitle => {
                if self.html_dt {
                    self.html_dt_close();
                } else {
                    self.flush_spans();
                }
            }
            TagEnd::DefinitionListDefinition => {
                self.flush_spans();
                self.definition = false;
                self.marker_colon = None;
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    if language.as_deref() == Some("math") {
                        // GitHub's fenced math notation.
                        self.emit(BlockKind::MathBlock { tex: text });
                    } else {
                        let lines = self.code_body(&text);
                        self.emit(BlockKind::CodeBlock {
                            language,
                            lines,
                            highlights: Vec::new(),
                            exact: 0,
                        });
                    }
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
                    span.image = Some(Box::new(SpanImage {
                        src: path,
                        width: None,
                        height: None,
                    }));
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
        let text = if std::mem::take(&mut self.html_gap) {
            self.flush_text();
            self.close_gap(text)
        } else {
            text
        };
        if let Some(at) = self.marker_colon.take() {
            if self.current.0 == at + 1 && self.pending.is_none() {
                self.pending = Some(PendingText {
                    text: ":".to_string(),
                    boundaries: vec![(0, at)],
                    end: at + 1,
                });
            }
        }
        let (start, end) = self.current;
        let (piece, marks) = replace_emoji(text, start);
        match self.pending.as_mut() {
            Some(run) if run.end == start => {
                let at = run.text.len();
                run.boundaries.push((at, start));
                run.boundaries
                    .extend(marks.into_iter().map(|(t, s)| (at + t, s)));
                run.text.push_str(&piece);
                run.end = end;
            }
            _ => {
                self.flush_text();
                let mut boundaries = vec![(0, start)];
                boundaries.extend(marks);
                self.pending = Some(PendingText {
                    text: piece,
                    boundaries,
                    end,
                });
            }
        }
    }

    /// Runs the text passes over the pending run, bare URLs then the
    /// inline marks, and pushes the spans.
    fn flush_text(&mut self) {
        let Some(run) = self.pending.take() else {
            return;
        };
        let saved = self.current;
        self.current = (run.boundaries[0].1, run.end);
        self.run_map = run.boundaries;
        self.run_end = run.end;
        self.linkified(&run.text);
        self.current = saved;
    }

    /// The source offset of a text offset in the run being flushed:
    /// exact at and after every piece boundary, clamped inside a piece
    /// the parser rewrote to another length (a smart quote, a decoded
    /// entity, a soft break, an emoji).
    fn src(&self, at: usize) -> u32 {
        let i = self
            .run_map
            .iter()
            .rposition(|(t, _)| *t <= at)
            .unwrap_or(0);
        let (t, s) = self.run_map[i];
        let next = self.run_map.get(i + 1).map_or(self.run_end, |(_, s)| *s);
        (s + at.saturating_sub(t)).min(next) as u32
    }

    /// Drops the leading whitespace of text arriving after an invisible
    /// HTML form when the text before it already ends in whitespace.
    /// The source cursor moves past the dropped bytes, so the span's
    /// range keeps matching its text.
    fn close_gap<'a>(&mut self, text: &'a str) -> &'a str {
        let joined = self
            .spans
            .last()
            .is_some_and(|s| s.raw_text().ends_with(char::is_whitespace));
        if !joined {
            return text;
        }
        let trimmed = text.trim_start();
        self.current.0 += text.len() - trimmed.len();
        trimmed
    }

    /// Splits bare http(s) URLs out of plain text into linked spans. Span
    /// ranges assume text offsets match source offsets; when a transform
    /// broke that, consumers detect the mismatch and fall back.
    fn linkified(&mut self, text: &str) {
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
                self.marked(before, pos);
            }
            let end = from
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(from.len());
            let (url, after) = from.split_at(end);
            let url = url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
            let mut span = self.style();
            span.set_text(url);
            span.link = Some(url.to_string());
            span.range = self.src(pos + start)..self.src(pos + start + url.len());
            self.push(span);
            pos += start + url.len();
            rest = &from[url.len()..];
            if rest == after && rest.is_empty() {
                break;
            }
        }
        if !rest.is_empty() {
            self.marked(rest, pos);
        }
    }

    /// Pushes a text run split at the inline marks, each piece in the
    /// style its delimiters give it and ranged over its own content;
    /// `at` is where `text` starts in the run being flushed.
    fn marked(&mut self, text: &str, at: usize) {
        for (range, mark) in marks(text) {
            self.mark = mark;
            self.abbreviated(&text[range.clone()], at + range.start);
        }
        self.mark = None;
    }

    /// Pushes a text piece, every whole word that is a defined
    /// abbreviation carrying its expansion.
    fn abbreviated(&mut self, text: &str, at: usize) {
        let mut plain = 0;
        for (range, which) in abbreviation_hits(text, &self.abbreviations) {
            if plain < range.start {
                let mut span = self.style();
                span.set_text(&text[plain..range.start]);
                span.range = self.src(at + plain)..self.src(at + range.start);
                self.push(span);
            }
            let mut span = self.style();
            span.set_text(&text[range.clone()]);
            span.abbr = std::num::NonZeroU32::new(which as u32 + 1);
            span.range = self.src(at + range.start)..self.src(at + range.end);
            self.push(span);
            plain = range.end;
        }
        if plain < text.len() {
            let mut span = self.style();
            span.set_text(&text[plain..]);
            span.range = self.src(at + plain)..self.src(at + text.len());
            self.push(span);
        }
    }

    /// Whether a paragraph is nothing but abbreviation definitions, read
    /// from its source lines; such a paragraph defines and is not shown.
    fn abbreviation_paragraph(&self, spans: &[Span]) -> bool {
        if self.abbreviations.is_empty() {
            return false;
        }
        let range = extent(spans.iter());
        let Some(text) = self.source.get(range) else {
            return false;
        };
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let mut any = false;
        for line in &mut lines {
            if abbreviation_line(line).is_none() {
                return false;
            }
            any = true;
        }
        any
    }

    /// One HTML event, block or inline, scanned for the GitHub README
    /// subset: text between tags goes to the spans, tags to `html_tag`,
    /// which keeps centered p and div, sized images, links wrapping
    /// images, br and the inline styling tags and strips everything
    /// else with its inner text kept. The invisible forms (comments,
    /// the doctype, CDATA sections, processing instructions) vanish. A
    /// tag or an invisible form cut off at the event's end waits in
    /// `html_tail` for the next event.
    fn html(&mut self, html: &str) {
        let combined = if self.html_tail.is_empty() {
            html.to_string()
        } else {
            std::mem::take(&mut self.html_tail) + " " + html
        };
        let mut rest = combined.as_str();
        while let Some(open) = rest.find('<') {
            let (before, tag_on) = rest.split_at(open);
            if let Some(closed) = html_invisible(tag_on) {
                self.html_text(before);
                let Some(end) = closed else {
                    self.html_tail = tag_on.to_string();
                    return;
                };
                self.flush_text();
                self.html_gap = self
                    .spans
                    .last()
                    .is_some_and(|s| s.raw_text().ends_with(char::is_whitespace));
                rest = &tag_on[end..];
                continue;
            }
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

    /// Text between tags; HTML collapses runs of its own whitespace,
    /// the ASCII kind, newlines included, except inside `<pre>`, whose
    /// body is verbatim. A no-break space is content, not whitespace.
    fn html_text(&mut self, text: &str) {
        if let Some(pre) = self.html_pre.as_mut() {
            pre.text.push_str(&decode_entities(text));
            return;
        }
        let blank = |c: char| c.is_ascii_whitespace();
        if text.chars().all(blank) {
            if !text.is_empty() && (!self.spans.is_empty() || self.pending.is_some()) {
                self.text(" ");
            }
            return;
        }
        let mut collapsed = String::with_capacity(text.len());
        if text.starts_with(blank) {
            collapsed.push(' ');
        }
        collapsed.push_str(
            &text
                .split(blank)
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        );
        if text.ends_with(blank) {
            collapsed.push(' ');
        }
        self.text(&decode_entities(&collapsed));
    }

    fn html_tag(&mut self, tag: &str) {
        self.flush_text();
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
                let side = html_attr(attrs, "style").and_then(|s| page_break_side(&s));
                if side == Some(BreakSide::Before) {
                    self.emit(BlockKind::PageBreak);
                }
                self.html_divs.push(HtmlDiv {
                    centered,
                    break_after: side == Some(BreakSide::After),
                });
            }
            ("p" | "div", true) => {
                if self.html_capturing() {
                    return;
                }
                self.flush_spans();
                if self.html_divs.pop().is_some_and(|d| d.break_after) {
                    self.emit(BlockKind::PageBreak);
                }
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
                span.image = Some(Box::new(SpanImage {
                    src,
                    width: html_attr(attrs, "width").and_then(|v| v.parse().ok()),
                    height: html_attr(attrs, "height").and_then(|v| v.parse().ok()),
                }));
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

    /// A heading's anchor, unique the way GitHub makes it: a repeat of an
    /// earlier heading's slug takes `-1`, the next `-2`, so a link and
    /// the outline reach each occurrence rather than the first.
    fn unique_anchor(&mut self, spans: &[Span]) -> String {
        let base = slug(spans);
        let seen = self.anchors_seen.entry(base.clone()).or_insert(0);
        *seen += 1;
        match *seen {
            1 => base,
            n => format!("{base}-{}", n - 1),
        }
    }

    /// The number a footnote label shows, taken in order of first use:
    /// the first label met, in a reference or a definition, is 1.
    fn footnote_number(&mut self, label: &str) -> u32 {
        let next = self.footnote_numbers.len() as u32 + 1;
        *self
            .footnote_numbers
            .entry(label.to_string())
            .or_insert(next)
    }

    /// The anchor a heading names itself, `{#id}`; the id joins the slug
    /// count, so a later heading whose slug reads the same gets numbered
    /// past it instead of pointing two headings at one anchor.
    fn explicit_anchor(&mut self, id: String) -> String {
        *self.anchors_seen.entry(id.clone()).or_insert(0) += 1;
        id
    }

    /// Emits the accumulated `<hN>` heading with its GitHub slug anchor.
    fn html_heading_close(&mut self) {
        let Some(level) = self.html_heading.take() else {
            return;
        };
        let mut spans = std::mem::take(&mut self.spans);
        trim_cell(&mut spans);
        let anchor = self.unique_anchor(&spans);
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
            exact: 0,
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
            self.html_divs.push(HtmlDiv {
                centered: true,
                break_after: false,
            });
            self.emit(BlockKind::Paragraph { spans: caption });
            self.html_divs.pop();
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
    /// The row takes its neighbor's offset as an empty range, keeping
    /// blocks ordered by source offset for the offset-to-block search.
    fn summarize(&mut self, id: u16) {
        if self.details_summarized[id as usize] {
            return;
        }
        self.details_summarized[id as usize] = true;
        let at = self.details_start[id as usize].min(self.blocks.len());
        let start = match self.blocks.get(at) {
            Some(next) => next.range.start,
            None => self.blocks.last().map(|b| b.range.end).unwrap_or(0),
        };
        self.blocks.insert(
            at,
            Block {
                quote_depth: 0,
                alert: None,
                range: start..start,
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
        self.flush_text();
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
        span.mark = self.html_mark > 0 || self.mark == Some(Mark::Highlight);
        span.code = self.html_code > 0;
        span.script = if self.mark == Some(Mark::Sub) || self.html_sub > 0 {
            SpanScript::Sub
        } else if self.mark == Some(Mark::Sup) || self.html_sup > 0 {
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
                && last.abbr == span.abbr
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
        self.flush_text();
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        if let Some(open) = self.footnote.as_mut() {
            let continued = std::mem::replace(&mut open.emitted, true);
            let (label, number) = (open.label.clone(), open.number);
            self.emit(BlockKind::FootnoteDef {
                label,
                number,
                continued,
                spans,
            });
            return;
        }
        // A paragraph that is nothing but `\newpage` (or its two
        // siblings) is the pandoc spelling of a page break.
        if self.lists.is_empty() && self.html_divs.is_empty() && !self.definition {
            if let Some(range) = page_command(&self.source, &spans) {
                self.emit_at(BlockKind::PageBreak, range);
                return;
            }
        }
        // A paragraph that is one plain image and whitespace stays a block
        // image; links, size attributes, or centering keep it inline.
        if self.lists.is_empty() && self.html_divs.is_empty() {
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
        if self.definition {
            self.emit(BlockKind::ListItem {
                marker: Marker::None,
                depth: 0,
                spans,
            });
            return;
        }
        if self.abbreviation_paragraph(&spans) {
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
            BlockKind::PageBreak => trimmed(&self.source, self.current.0..self.current.1),
            _ => self.current.0..self.current.1,
        }
    }

    fn emit(&mut self, kind: BlockKind) {
        let range = self.block_range(&kind);
        self.emit_at(kind, range);
    }

    fn emit_at(&mut self, kind: BlockKind, range: Range<usize>) {
        // A summary row belongs to the group enclosing its own; it is
        // the toggle, visible while its group is closed.
        let details = match &kind {
            BlockKind::Summary { group, .. } => self.details[*group as usize].parent,
            _ => self.details_stack.last().copied(),
        };
        self.blocks.push(Block {
            quote_depth: self.quote_depth,
            alert: self.alerts.iter().rev().find_map(|a| *a),
            range,
            centered: self.html_divs.iter().any(|d| d.centered),
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
/// The page break a `style` attribute asks for, if any: the CSS 2 names
/// (`page-break-before`, `page-break-after`) with `always`, or the CSS 3
/// names (`break-before`, `break-after`) with `page`; either value on
/// either name is accepted, the intent being plain.
fn page_break_side(style: &str) -> Option<BreakSide> {
    style.split(';').find_map(|declaration| {
        let (name, value) = declaration.split_once(':')?;
        let value = value.trim().to_ascii_lowercase();
        if value != "always" && value != "page" {
            return None;
        }
        match name.trim().to_ascii_lowercase().as_str() {
            "page-break-before" | "break-before" => Some(BreakSide::Before),
            "page-break-after" | "break-after" => Some(BreakSide::After),
            _ => None,
        }
    })
}

/// The source range of a paragraph that is one plain `\newpage`,
/// `\pagebreak` or `\clearpage` and nothing else, the pandoc spelling
/// of a page break; None for any other paragraph.
fn page_command(source: &str, spans: &[Span]) -> Option<Range<usize>> {
    let [span] = spans else {
        return None;
    };
    let plain = !span.bold
        && !span.italic
        && !span.strike
        && !span.underline
        && !span.mark
        && !span.code
        && !span.math
        && span.script == SpanScript::None
        && span.link.is_none()
        && span.image.is_none();
    let command = matches!(
        span.raw_text().trim(),
        "\\newpage" | "\\pagebreak" | "\\clearpage"
    );
    (plain && command).then(|| trimmed(source, span.range.start as usize..span.range.end as usize))
}

/// A range shrunk past the whitespace at both ends.
fn trimmed(source: &str, range: Range<usize>) -> Range<usize> {
    let text = &source[range.clone()];
    let start = range.start + (text.len() - text.trim_start().len());
    let end = range.end - (text.len() - text.trim_end().len());
    start..end.max(start)
}

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
pub(crate) fn slug(spans: &[Span]) -> String {
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

/// The label and expansion of a `*[label]: expansion` line, the
/// abbreviation definition of PHP Markdown Extra; `None` for any other
/// line.
fn abbreviation_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("*[")?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    let expansion = rest[close + 2..].trim();
    (!label.is_empty() && !label.contains(['[', ']']) && !expansion.is_empty())
        .then_some((label, expansion))
}

/// Every abbreviation the source defines, in source order, so the table
/// of a prefix parse is the head of the full parse's and the spans'
/// indices agree across the two; a label defined twice keeps its first
/// expansion. A definition line starts within three spaces of the
/// margin, and the lines of a fenced code block define nothing, since a
/// file showing the syntax must not take it.
fn scan_abbreviations(source: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in source.lines() {
        let lead = line.len() - line.trim_start().len();
        let body = &line[lead..];
        let run = body
            .chars()
            .next()
            .filter(|&c| c == '`' || c == '~')
            .map(|c| (c, body.chars().take_while(|&b| b == c).count()));
        match (fence, run) {
            (Some((c, n)), Some((d, m))) if c == d && m >= n && body[m..].trim().is_empty() => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            (None, Some((c, n))) if n >= 3 && lead <= 3 => {
                fence = Some((c, n));
                continue;
            }
            _ => {}
        }
        if lead > 3 {
            continue;
        }
        if let Some((label, expansion)) = abbreviation_line(body) {
            if !out.iter().any(|(l, _)| l == label) {
                out.push((label.to_string(), expansion.to_string()));
            }
        }
    }
    out
}

/// The whole-word occurrences of the abbreviations in `text`, in order,
/// each with the index of its definition; where two labels match at one
/// place, the longer wins. A word edge is the start, the end, or a
/// character that is not a letter or a digit.
fn abbreviation_hits(text: &str, abbreviations: &[(String, String)]) -> Vec<(Range<usize>, usize)> {
    let mut hits = Vec::new();
    if abbreviations.is_empty() {
        return hits;
    }
    let word = |c: char| c.is_alphanumeric();
    let mut i = 0;
    'scan: while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let at_edge = !text[..i].chars().next_back().is_some_and(word);
        if at_edge {
            let longest = abbreviations
                .iter()
                .enumerate()
                .filter(|(_, (label, _))| {
                    text[i..].starts_with(label.as_str())
                        && !text[i + label.len()..].chars().next().is_some_and(word)
                })
                .max_by_key(|(_, (label, _))| label.len());
            if let Some((which, (label, _))) = longest {
                let end = i + label.len();
                hits.push((i..end, which));
                i = end;
                continue 'scan;
            }
        }
        i += 1;
    }
    hits
}

/// The footnote definition being read: its label, its number, and
/// whether its first block is out, the later ones being continuations.
struct FootnoteOpen {
    label: String,
    number: u32,
    emitted: bool,
}

/// A run of text events joined: the text, the boundary of every piece
/// as (text offset, source offset), and the source end. A piece the
/// parser rewrote (a smart quote, a decoded entity, a soft break) or an
/// emoji shortcode is longer or shorter than its source, and the pieces
/// after it keep their own offsets.
struct PendingText {
    text: String,
    boundaries: Vec<(usize, usize)>,
    end: usize,
}

/// The style an inline mark of the extended syntax gives its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Sub,
    Sup,
    Highlight,
}

/// Splits a text run at the inline marks of the extended syntax. A single
/// tilde or caret around a run holding no whitespace is a subscript or a
/// superscript, `H~2~O` and `X^2^`, Pandoc's rule; a tilde standing
/// between spaces reached the parser first and is strikethrough, as on
/// GitHub. `==` and `::` around a run are a highlight when they sit at
/// word edges, so `std::vector` in prose stays as typed. Returns the
/// pieces in order with their marks, the delimiters dropped.
fn marks(text: &str) -> Vec<(Range<usize>, Option<Mark>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < bytes.len() {
        let hit = match bytes[i] {
            b'~' | b'^' => script_span(text, i),
            b'=' | b':' => highlight_span(text, i),
            _ => None,
        };
        let Some((content, end)) = hit else {
            i += 1;
            continue;
        };
        if plain < i {
            out.push((plain..i, None));
        }
        let mark = match bytes[i] {
            b'~' => Mark::Sub,
            b'^' => Mark::Sup,
            _ => Mark::Highlight,
        };
        out.push((content, Some(mark)));
        plain = end;
        i = end;
    }
    if plain < bytes.len() {
        out.push((plain..bytes.len(), None));
    }
    out
}

/// A subscript or superscript opening at `at`: the content runs to the
/// next same delimiter, holds no whitespace and is not empty. Returns
/// the content's range and the offset past the closer.
fn script_span(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let bytes = text.as_bytes();
    let delim = bytes[at];
    for (j, &b) in bytes.iter().enumerate().skip(at + 1) {
        if b == delim {
            return (j > at + 1).then_some((at + 1..j, j + 1));
        }
        if b.is_ascii_whitespace() {
            return None;
        }
    }
    None
}

/// A highlight opening at `at` on a doubled `=` or `:`: the text before
/// the opener ends on a non-word character or is empty, the content
/// starts on a non-space, and the closing pair follows a non-space and
/// precedes a non-word character or the end. Returns the content's
/// range and the offset past the closer.
fn highlight_span(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let bytes = text.as_bytes();
    let delim = bytes[at];
    if bytes.get(at + 1) != Some(&delim) {
        return None;
    }
    let word = |c: char| c.is_alphanumeric();
    if text[..at].chars().next_back().is_some_and(word) {
        return None;
    }
    let first = text[at + 2..].chars().next()?;
    if first.is_whitespace() || first == delim as char {
        return None;
    }
    let pair = if delim == b'=' { "==" } else { "::" };
    let mut from = at + 2;
    while let Some(found) = text[from..].find(pair) {
        let close = from + found;
        let before = text[..close].chars().next_back();
        let after = text[close + 2..].chars().next();
        let closes = before.is_some_and(|c| !c.is_whitespace() && c != delim as char)
            && !after.is_some_and(word)
            && close > at + 2;
        if closes {
            return Some((at + 2..close, close + 2));
        }
        from = close + 1;
    }
    None
}

/// Replaces `:shortcode:` runs by their emoji. Answers the text and,
/// after each replacement, the boundary as (text offset, source offset
/// from `source_start`), so what follows a shortcode keeps an exact
/// source offset although the emoji and the shortcode differ in length.
fn replace_emoji(text: &str, source_start: usize) -> (String, Vec<(usize, usize)>) {
    if !text.contains(':') {
        return (text.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(text.len());
    let mut marks = Vec::new();
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
                        marks.push((out.len(), source_start + text.len() - rest.len()));
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
    (out, marks)
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
    fn repeated_headings_get_numbered_anchors_like_github() {
        let doc = parse("# Gate\n\ntext\n\n# Gate\n\n## Gate\n\n<h2>Gate</h2>\n\n# Other\n");
        let anchors: Vec<&str> = doc
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Heading { anchor, .. } => Some(anchor.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(anchors, ["gate", "gate-1", "gate-2", "gate-3", "other"]);
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
            ..
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
            marker: Marker::Task { checked, marker },
            depth,
            ..
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert!(*checked);
        assert_eq!(*depth, 0);
        assert_eq!(
            &d.source[*marker..marker + 3],
            "[x]",
            "the marker names its own bytes"
        );
        let d = parse("intro\n\n- [ ] open");
        let BlockKind::ListItem {
            marker: Marker::Task { checked, marker },
            ..
        } = &d.blocks[1].kind
        else {
            panic!()
        };
        assert!(!*checked);
        assert_eq!(&d.source[*marker..marker + 3], "[ ]");
    }

    // The rendered page's one permitted edit: the flip replaces one
    // byte with one byte, so no offset in the model moves.
    #[test]
    fn a_checkbox_flip_moves_nothing() {
        let mut d = parse("# T\n\n- [ ] one\n- [x] two\n\ntail\n");
        let before: Vec<_> = d.blocks.iter().map(|b| b.range.clone()).collect();
        let block = d
            .blocks
            .iter()
            .position(|b| matches!(b.kind, BlockKind::ListItem { .. }))
            .expect("a task item");
        let (splice, text) = d.flip_task(block).expect("a task flips");
        assert_eq!(text, "x");
        assert_eq!(splice.len(), 1);
        assert_eq!(&d.source[splice.start - 1..splice.end + 1], "[x]");
        let after: Vec<_> = d.blocks.iter().map(|b| b.range.clone()).collect();
        assert_eq!(before, after, "no block range moved");
        let BlockKind::ListItem {
            marker: Marker::Task { checked, .. },
            ..
        } = &d.blocks[block].kind
        else {
            panic!()
        };
        assert!(*checked, "the marker flipped in place");
        let (_, text) = d.flip_task(block).expect("and flips back");
        assert_eq!(text, " ");
        assert!(d.flip_task(0).is_none(), "a heading refuses");
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
            ..
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
    fn a_page_break_div_parses_to_the_block() {
        for src in [
            "<div style=\"page-break-after: always\"></div>",
            "<div style=\"page-break-before: always;\"></div>",
            "<div style=\"break-after: page\"></div>",
            "<p style=\"break-before: page\"></p>",
            "<div style=\"PAGE-BREAK-AFTER: Always\"></div>",
            "<div style='margin: 0; page-break-after:always'></div>",
        ] {
            let d = parse(format!("one\n\n{src}\n\ntwo\n"));
            let kinds: Vec<_> = d.blocks.iter().map(|b| &b.kind).collect();
            assert_eq!(d.blocks.len(), 3, "{src}: {kinds:?}");
            assert!(
                matches!(d.blocks[1].kind, BlockKind::PageBreak),
                "{src}: {kinds:?}"
            );
            assert_eq!(
                &d.source[d.blocks[1].range.clone()],
                src,
                "{src}: the block's source"
            );
        }
    }

    #[test]
    fn a_break_after_a_div_with_content_follows_the_content() {
        let d = parse("<div style=\"page-break-after: always\">inside</div>\n\nafter\n");
        let kinds: Vec<_> = d.blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(d.blocks.len(), 3, "{kinds:?}");
        assert!(
            matches!(d.blocks[0].kind, BlockKind::Paragraph { .. }),
            "{kinds:?}"
        );
        assert!(
            matches!(d.blocks[1].kind, BlockKind::PageBreak),
            "{kinds:?}"
        );
        assert!(
            matches!(d.blocks[2].kind, BlockKind::Paragraph { .. }),
            "{kinds:?}"
        );
        let d = parse("<div style=\"page-break-before: always\">inside</div>\n");
        assert!(matches!(d.blocks[0].kind, BlockKind::PageBreak));
        assert!(matches!(d.blocks[1].kind, BlockKind::Paragraph { .. }));
    }

    #[test]
    fn a_div_with_another_style_stays_a_div() {
        let d = parse("<div style=\"color: red; page-break-inside: avoid\">text</div>\n");
        assert_eq!(d.blocks.len(), 1);
        assert!(matches!(d.blocks[0].kind, BlockKind::Paragraph { .. }));
    }

    #[test]
    fn a_bare_tex_page_command_parses_to_the_block() {
        for cmd in ["\\newpage", "\\pagebreak", "\\clearpage", "  \\newpage  "] {
            let d = parse(format!("one\n\n{cmd}\n\ntwo\n"));
            let kinds: Vec<_> = d.blocks.iter().map(|b| &b.kind).collect();
            assert_eq!(d.blocks.len(), 3, "{cmd:?}: {kinds:?}");
            assert!(
                matches!(d.blocks[1].kind, BlockKind::PageBreak),
                "{cmd:?}: {kinds:?}"
            );
            assert_eq!(
                &d.source[d.blocks[1].range.clone()],
                cmd.trim(),
                "{cmd:?}: the block's source"
            );
        }
        let d = parse("say \\newpage here\n");
        assert!(
            matches!(d.blocks[0].kind, BlockKind::Paragraph { .. }),
            "inside a sentence it is text"
        );
        let d = parse("$$\n\\newpage\n$$\n");
        assert!(
            matches!(d.blocks[0].kind, BlockKind::MathBlock { .. }),
            "inside math it is math"
        );
        let d = parse("- \\newpage\n");
        assert!(
            matches!(d.blocks[0].kind, BlockKind::ListItem { .. }),
            "in a list item it is text"
        );
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
    fn a_heading_id_in_braces_becomes_the_anchor() {
        let d = parse("### My Great Heading {#custom-id .cls key=val}\n\n[go](#custom-id)\n");
        let BlockKind::Heading {
            spans,
            anchor,
            level,
        } = &d.blocks[0].kind
        else {
            panic!()
        };
        assert_eq!(*level, 3);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text(&d.source), "My Great Heading");
        assert_eq!(anchor, "custom-id");
        let BlockKind::Paragraph { spans } = &d.blocks[1].kind else {
            panic!()
        };
        assert_eq!(spans[0].link.as_deref(), Some("#custom-id"));
    }

    #[test]
    fn an_explicit_heading_id_counts_with_the_slugs() {
        let d = parse("## Intro {#intro}\n\n## Intro\n\n## Other {#x}\n\n## Other\n");
        let anchors: Vec<&str> = d
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Heading { anchor, .. } => Some(anchor.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(anchors, ["intro", "intro-1", "x", "other"]);
    }

    /// One letter per block: `T` a bold term, `D` an indented definition
    /// item, `P` a plain paragraph, each with its text.
    fn definition_shape(d: &Document) -> Vec<(char, String)> {
        d.blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::Paragraph { spans } if spans.iter().all(|s| s.bold) => {
                    ('T', spans[0].text(&d.source).to_string())
                }
                BlockKind::Paragraph { spans } => ('P', spans[0].text(&d.source).to_string()),
                BlockKind::ListItem {
                    marker: Marker::None,
                    depth: 0,
                    spans,
                } => ('D', spans[0].text(&d.source).to_string()),
                other => panic!("unexpected block {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_definition_list_maps_like_its_html_form() {
        let d = parse(
            "First Term\n: This is the definition.\n\nSecond Term\n: One definition.\n\
             : Another definition.\n\nAfter.\n",
        );
        let shape = definition_shape(&d);
        let expected = [
            ('T', "First Term"),
            ('D', "This is the definition."),
            ('T', "Second Term"),
            ('D', "One definition."),
            ('D', "Another definition."),
            ('P', "After."),
        ];
        assert_eq!(shape.len(), expected.len(), "{shape:?}");
        for (got, want) in shape.iter().zip(expected) {
            assert_eq!((got.0, got.1.as_str()), want);
        }
    }

    #[test]
    fn a_highlight_line_after_a_paragraph_is_no_definition() {
        let d = parse("==twoequals==\n\n::twohypens::\n\nTerm\n: real\n::lit::\n");
        let lit: Vec<(String, bool, bool)> = d
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::Paragraph { spans } => (
                    spans.iter().map(|s| s.text(&d.source)).collect(),
                    spans.iter().any(|s| s.mark),
                    spans.iter().any(|s| s.bold),
                ),
                BlockKind::ListItem { spans, .. } => (
                    spans.iter().map(|s| s.text(&d.source)).collect(),
                    false,
                    false,
                ),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            lit,
            [
                ("twoequals".to_string(), true, false),
                ("twohypens".to_string(), true, false),
                ("Term".to_string(), false, true),
                ("real".to_string(), false, false),
                ("lit".to_string(), true, false),
            ]
        );
    }

    #[test]
    fn a_definition_of_two_paragraphs_keeps_both_indented() {
        let d = parse("Term\n: definition\n\n    more indented\n\nAfter.\n");
        let shape = definition_shape(&d);
        let expected = [
            ('T', "Term"),
            ('D', "definition"),
            ('D', "more indented"),
            ('P', "After."),
        ];
        assert_eq!(shape.len(), expected.len(), "{shape:?}");
        for (got, want) in shape.iter().zip(expected) {
            assert_eq!((got.0, got.1.as_str()), want);
        }
    }

    /// Each span's text with its script and highlight, for the mark tests.
    fn marked_shape(d: &Document) -> Vec<(String, SpanScript, bool)> {
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!("{:?}", d.blocks[0].kind)
        };
        spans
            .iter()
            .map(|s| (s.text(&d.source).to_string(), s.script, s.mark))
            .collect()
    }

    #[test]
    fn intraword_tildes_and_carets_make_sub_and_superscripts() {
        let d = parse("H~2~O and X^2^ and E = mc^2^");
        let plain = |t: &str| (t.to_string(), SpanScript::None, false);
        let sub = |t: &str| (t.to_string(), SpanScript::Sub, false);
        let sup = |t: &str| (t.to_string(), SpanScript::Sup, false);
        assert_eq!(
            marked_shape(&d),
            [
                plain("H"),
                sub("2"),
                plain("O and X"),
                sup("2"),
                plain(" and E = mc"),
                sup("2"),
            ]
        );
    }

    #[test]
    fn a_single_tilde_between_spaces_stays_strikethrough() {
        let d = parse("~one tilde~ and ~word~");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let struck: Vec<&str> = spans
            .iter()
            .filter(|s| s.strike)
            .map(|s| s.text(&d.source))
            .collect();
        assert_eq!(struck, ["one tilde", "word"]);
        assert!(spans.iter().all(|s| s.script == SpanScript::None));
    }

    #[test]
    fn a_script_needs_a_run_without_whitespace() {
        let d = parse("a~b c~d and 2^10 and 3^ and x~ ~y");
        assert_eq!(
            marked_shape(&d),
            [(
                "a~b c~d and 2^10 and 3^ and x~ ~y".to_string(),
                SpanScript::None,
                false
            )]
        );
    }

    #[test]
    fn doubled_equals_and_colons_highlight() {
        let d = parse("==two equals== and ::two colons:: here, (==in brackets==).");
        let plain = |t: &str| (t.to_string(), SpanScript::None, false);
        let lit = |t: &str| (t.to_string(), SpanScript::None, true);
        assert_eq!(
            marked_shape(&d),
            [
                lit("two equals"),
                plain(" and "),
                lit("two colons"),
                plain(" here, ("),
                lit("in brackets"),
                plain(")."),
            ]
        );
    }

    #[test]
    fn a_highlight_needs_word_edges() {
        let text = "std::vector::iterator and a == b == c and x==y==z and == alone ==";
        let d = parse(text);
        assert_eq!(
            marked_shape(&d),
            [(text.to_string(), SpanScript::None, false)]
        );
    }

    #[test]
    fn a_tilde_inside_a_bare_url_is_not_a_delimiter() {
        let d = parse("see https://example.com/~user/~page now");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let link = spans.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(link.text(&d.source), "https://example.com/~user/~page");
        assert!(spans.iter().all(|s| s.script == SpanScript::None));
    }

    #[test]
    fn a_marked_span_ranges_over_its_content() {
        let d = parse("H~2~O ==lit==");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let sub = spans.iter().find(|s| s.script == SpanScript::Sub).unwrap();
        assert_eq!(sub.range, 2..3);
        let lit = spans.iter().find(|s| s.mark).unwrap();
        assert_eq!(lit.range, 8..11);
        assert_eq!(&d.source[8..11], "lit");
    }

    /// A span is the model's most numerous piece: a field added here
    /// shows in the memory table of every markdown fixture, as the
    /// abbreviation did as a string before it became an index.
    #[test]
    fn a_span_stays_small() {
        assert!(
            std::mem::size_of::<Span>() <= 80,
            "a span is {} bytes",
            std::mem::size_of::<Span>()
        );
    }

    #[test]
    fn abbreviation_definitions_vanish_and_mark_their_words() {
        let d = parse(
            "The HTML spec, in HTML5 too.\n\n*[HTML]: Hyper Text Markup Language\n\
             *[W3C]: World Wide Web Consortium\n\nBy the W3C.\n",
        );
        assert_eq!(d.blocks.len(), 2, "{:?}", d.blocks);
        let shape = |i: usize| -> Vec<(String, Option<String>)> {
            let BlockKind::Paragraph { spans } = &d.blocks[i].kind else {
                panic!()
            };
            spans
                .iter()
                .map(|s| (s.text(&d.source).to_string(), s.abbr(&d).map(str::to_owned)))
                .collect()
        };
        let long = Some("Hyper Text Markup Language".to_string());
        assert_eq!(
            shape(0),
            [
                ("The ".to_string(), None),
                ("HTML".to_string(), long),
                (" spec, in HTML5 too.".to_string(), None),
            ]
        );
        assert_eq!(
            shape(1),
            [
                ("By the ".to_string(), None),
                (
                    "W3C".to_string(),
                    Some("World Wide Web Consortium".to_string())
                ),
                (".".to_string(), None),
            ]
        );
    }

    #[test]
    fn a_definition_inside_a_code_fence_defines_nothing() {
        let d = parse(
            "The HTML spec.\n\n```markdown\n*[HTML]: Hyper Text Markup Language\n```\n\n\
             ~~~\n*[W3C]: World Wide Web Consortium\n~~~\n\n    *[CSS]: too deep\n\nW3C and CSS.\n",
        );
        for block in &d.blocks {
            if let BlockKind::Paragraph { spans } = &block.kind {
                assert!(spans.iter().all(|s| s.abbr.is_none()), "{spans:?}");
            }
        }
        let d = parse("   *[CSS]: Cascading Style Sheets\n\nCSS here.\n");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert_eq!(spans[0].abbr(&d), Some("Cascading Style Sheets"));
    }

    #[test]
    fn the_longer_label_wins_and_the_table_keeps_source_order() {
        let d = parse("HTML5 and HTML.\n\n*[HTML]: markup\n*[HTML5]: the fifth\n");
        assert_eq!(d.abbreviations, ["markup", "the fifth"]);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let got: Vec<(&str, Option<&str>)> = spans
            .iter()
            .map(|s| (s.text(&d.source), s.abbr(&d)))
            .collect();
        assert_eq!(
            got,
            [
                ("HTML5", Some("the fifth")),
                (" and ", None),
                ("HTML", Some("markup")),
                (".", None),
            ]
        );
    }

    #[test]
    fn a_paragraph_holding_more_than_definitions_stays() {
        let d = parse("Not a definition\n*[X]: since the paragraph says more\n");
        assert_eq!(d.blocks.len(), 1);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let text: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert_eq!(text, "Not a definition *[X]: since the paragraph says more");
    }

    #[test]
    fn an_abbreviation_span_ranges_over_its_word() {
        let d = parse("See HTML.\n\n*[HTML]: Hyper Text Markup Language\n");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let word = spans.iter().find(|s| s.abbr.is_some()).unwrap();
        assert_eq!(word.range, 4..8);
        assert!(word.is_verbatim());
    }

    #[test]
    fn footnotes_number_in_order_of_first_use() {
        let d = parse(
            "A claim,[^big] another,[^1] the first again.[^big]\n\n\
             [^1]: The one labeled one.\n\n[^big]: The big one.\n",
        );
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let marks: Vec<(&str, &str)> = spans
            .iter()
            .filter_map(|s| s.link.as_deref().map(|l| (s.text(&d.source), l)))
            .collect();
        assert_eq!(
            marks,
            [
                ("1", "footnote:big"),
                ("2", "footnote:1"),
                ("1", "footnote:big")
            ]
        );
        let defs: Vec<(&str, u32, bool)> = d
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::FootnoteDef {
                    label,
                    number,
                    continued,
                    ..
                } => Some((label.as_str(), *number, *continued)),
                _ => None,
            })
            .collect();
        assert_eq!(defs, [("1", 2, false), ("big", 1, false)]);
    }

    #[test]
    fn a_footnote_of_several_paragraphs_continues_after_its_first() {
        let d = parse(
            "Text.[^n]\n\n[^n]: First paragraph.\n\n    Second paragraph.\n\n    \
             `code` third.\n\nAfter.\n",
        );
        let defs: Vec<(u32, bool, String)> = d
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::FootnoteDef {
                    number,
                    continued,
                    spans,
                    ..
                } => Some((*number, *continued, spans[0].text(&d.source).to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(
            defs,
            [
                (1, false, "First paragraph.".to_string()),
                (1, true, "Second paragraph.".to_string()),
                (1, true, "code".to_string()),
            ]
        );
        let BlockKind::Paragraph { spans } = &d.blocks.last().unwrap().kind else {
            panic!()
        };
        assert_eq!(spans[0].text(&d.source), "After.");
    }

    #[test]
    fn a_definition_before_its_reference_takes_the_next_number() {
        let d = parse("[^a]: Defined first.\n\nUsed[^b] then[^a].\n\n[^b]: Defined second.\n");
        let numbers: Vec<(&str, u32)> = d
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::FootnoteDef { label, number, .. } => Some((label.as_str(), *number)),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, [("a", 1), ("b", 2)]);
    }

    /// The parser hands a paragraph over in pieces, and a piece it
    /// rewrote (a smart quote, a decoded entity, a soft break) is longer
    /// or shorter than its source; the pieces after it keep their own
    /// offsets, so a mark, an abbreviation or a URL later in the run
    /// still slices the source exactly.
    #[test]
    fn ranges_stay_exact_across_rewritten_pieces() {
        let source =
            "Don't stop &amp; go\r\nthe ==lit== word :tada: HTML https://x.io/~u done\r\n\r\n\
                      *[HTML]: Hyper Text Markup Language\r\n";
        let d = parse(source);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let slice = |s: &Span| &source[s.range.start as usize..s.range.end as usize];
        let lit = spans.iter().find(|s| s.mark).unwrap();
        assert_eq!(slice(lit), "lit");
        assert!(lit.is_verbatim());
        let abbr = spans.iter().find(|s| s.abbr.is_some()).unwrap();
        assert_eq!(slice(abbr), "HTML");
        assert!(abbr.is_verbatim());
        let url = spans.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(slice(url), "https://x.io/~u");
        assert!(url.is_verbatim());
        let tail = spans.last().unwrap();
        assert_eq!(slice(tail), " done");
        assert!(tail.is_verbatim());
    }

    #[test]
    fn a_list_inside_a_definition_keeps_its_markers() {
        let d = parse("Term\n: - one\n  - two\n\nNext\n: plain\n");
        let markers: Vec<Option<&Marker>> = d
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::ListItem { marker, .. } => Some(marker),
                _ => None,
            })
            .collect();
        assert_eq!(
            markers,
            [
                None,
                Some(&Marker::Bullet),
                Some(&Marker::Bullet),
                None,
                Some(&Marker::None)
            ]
        );
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

    /// A heading's range starts past its `#` marker, so offset zero
    /// sits before every block; it still belongs to the document head,
    /// or a saved or remembered top position resolves to nothing.
    #[test]
    fn an_offset_before_the_first_block_belongs_to_it() {
        let d = parse("# Title\n\nSome prose under it.");
        assert!(d.blocks[0].range.start > 0, "the marker precedes the text");
        assert_eq!(d.block_at_offset(0), Some(0));
    }

    /// The synthesized summary carries its neighbor's offset, keeping
    /// blocks ordered by source offset for the offset-to-block search.
    #[test]
    fn a_synthesized_summary_keeps_blocks_ordered_by_offset() {
        let d = parse("intro\n\n<details>\n\nHidden prose.\n\n</details>");
        let intro = d
            .block_at_offset(2)
            .expect("the offset sits inside the intro");
        assert!(
            matches!(&d.blocks[intro].kind, BlockKind::Paragraph { .. }),
            "got {:?}",
            d.blocks[intro].kind
        );
        let starts: Vec<usize> = d.blocks.iter().map(|b| b.range.start).collect();
        assert!(starts.is_sorted(), "block starts in order, got {starts:?}");
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
        let BlockKind::FootnoteDef { label, spans, .. } = &d.blocks[1].kind else {
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
    fn currency_dollars_stay_prose() {
        // The bug class the gate exists for: pulldown reads these as math,
        // the digit after the closing dollar downgrades them.
        for src in ["$5-$10", "US$100 vs CA$120", "$5 or $6, $7"] {
            let d = parse(src);
            let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
                panic!("{src}")
            };
            assert!(spans.iter().all(|s| !s.math), "{src}");
            let text: String = spans
                .iter()
                .map(|s| s.text(&d.source).to_string())
                .collect();
            assert_eq!(text, src, "the reader sees what was typed");
        }
    }

    #[test]
    fn currency_pins_pulldown_math_events() {
        use pulldown_cmark::{Event, Options, Parser};
        let count = |src: &str| {
            Parser::new_ext(
                src,
                Options::ENABLE_MATH | Options::ENABLE_SMART_PUNCTUATION,
            )
            .filter(|e| matches!(e, Event::InlineMath(_) | Event::DisplayMath(_)))
            .count()
        };
        // These two reach the builder as math today; the gate catches them.
        assert_eq!(count("$5-$10"), 1);
        assert_eq!(count("US$100 vs CA$120"), 1);
        // These never become math events; a change here means pulldown's
        // delimiter rules moved and the gate needs a second look.
        for src in [
            "$5",
            "5$ and 10$",
            "$ x $",
            "\\$50 and \\$60",
            "prices $5, $6, and $7",
        ] {
            assert_eq!(count(src), 0, "{src}");
        }
    }

    #[test]
    fn dollar_backtick_notation_is_math() {
        let d = parse("cost $`x^2`$ here");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let m = spans.iter().find(|s| s.math).expect("math span");
        assert_eq!(m.text(&d.source), "x^2");
    }

    #[test]
    fn math_fence_is_a_math_block() {
        let d = parse("```math\n\\sum_i x_i\n```");
        let BlockKind::MathBlock { tex } = &d.blocks[0].kind else {
            panic!("expected math block, got {:?}", d.blocks[0].kind)
        };
        assert_eq!(tex.trim(), "\\sum_i x_i");
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

    /// The concatenated text of a paragraph block.
    fn paragraph_text(d: &Document, index: usize) -> String {
        let BlockKind::Paragraph { spans } = &d.blocks[index].kind else {
            panic!(
                "block {index} is not a paragraph: {:?}",
                d.blocks[index].kind
            )
        };
        spans.iter().map(|s| s.text(&d.source)).collect()
    }

    #[test]
    fn an_html_comment_block_yields_no_block() {
        let d = parse("<!-- TOC -->\n\ntext");
        assert_eq!(d.blocks.len(), 1, "{:?}", d.blocks);
        assert_eq!(paragraph_text(&d, 0), "text");
    }

    #[test]
    fn an_inline_comment_leaves_one_space_between_its_neighbors() {
        let d = parse("before <!-- note --> after");
        assert_eq!(paragraph_text(&d, 0), "before after");
        let d = parse("glued<!-- note -->together");
        assert_eq!(paragraph_text(&d, 0), "gluedtogether");
    }

    #[test]
    fn comments_holding_a_close_bracket_or_spanning_lines_vanish() {
        let d = parse("<!-- a > b -->\n\n<!--\nline one\nline two\n-->\n\nkept");
        assert_eq!(d.blocks.len(), 1, "{:?}", d.blocks);
        assert_eq!(paragraph_text(&d, 0), "kept");
    }

    #[test]
    fn an_unterminated_comment_hides_the_rest_of_the_file() {
        let d = parse("shown\n\n<!-- never closed\n\n# hidden\n\nhidden too");
        assert_eq!(d.blocks.len(), 1, "{:?}", d.blocks);
        assert_eq!(paragraph_text(&d, 0), "shown");
    }

    #[test]
    fn a_bare_angle_bracket_is_still_text() {
        let d = parse("a < b and c <3 and x<y");
        assert_eq!(paragraph_text(&d, 0), "a < b and c <3 and x<y");
    }

    #[test]
    fn doctype_cdata_and_processing_instructions_vanish() {
        let d = parse("<!DOCTYPE html>\n<?xml version=\"1.0\"?>\n<![CDATA[ raw <b> ]]>\n\nkept");
        assert_eq!(d.blocks.len(), 1, "{:?}", d.blocks);
        assert_eq!(paragraph_text(&d, 0), "kept");
    }

    #[test]
    fn a_commented_out_badge_vanishes() {
        let d = parse("Title <!-- [![b](https://img.tld/b.svg)](https://x.tld) --> here");
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        assert!(spans.iter().all(|s| s.image.is_none()), "no image survives");
        assert_eq!(paragraph_text(&d, 0), "Title here");
    }

    #[test]
    fn html_entities_decode_inside_html_blocks() {
        let d =
            parse("<p align=\"center\">&copy; 2024 &mdash; &#169; &#x1F600; &nbsp;x &amp;lt;</p>");
        assert_eq!(
            paragraph_text(&d, 0),
            "\u{a9} 2024 \u{2014} \u{a9} \u{1F600} \u{a0}x &lt;"
        );
        let d = parse("Inline <b>bold &copy; html</b> here");
        assert_eq!(paragraph_text(&d, 0), "Inline bold \u{a9} html here");
    }

    #[test]
    fn an_nbsp_cell_holds_its_no_break_space() {
        let d = parse("<table><tr><td>&nbsp;</td><td> a&nbsp;&nbsp;b </td></tr></table>");
        let BlockKind::Table { rows, .. } = &d.blocks[0].kind else {
            panic!("{:?}", d.blocks[0].kind)
        };
        assert_eq!(
            rows[0][0][0].text(&d.source),
            "\u{a0}",
            "the cell is not empty"
        );
        assert_eq!(
            rows[0][1][0].text(&d.source),
            "a\u{a0}\u{a0}b",
            "source spaces trim, no-break spaces stay"
        );
    }

    #[test]
    fn unknown_and_invalid_entities_stay_literal() {
        let d = parse("<p>&foo; &#0; &#xD800; &#99999999; &#x110000; & loose &amp</p>");
        assert_eq!(
            paragraph_text(&d, 0),
            "&foo; &#0; &#xD800; &#99999999; &#x110000; & loose &amp"
        );
    }

    #[test]
    fn the_latin_and_typographic_names_decode() {
        let d = parse(
            "<p>&eacute;&uuml;&ntilde;&Ccedil; &laquo;&raquo; &hellip; &euro;&pound;&yen;&cent; \
             &trade;&reg; &times;&divide; &deg;&plusmn; &bull;&middot; &ldquo;&rdquo;&lsquo;&rsquo; \
             &ndash; &apos; &frac12; &rarr;</p>",
        );
        assert_eq!(
            paragraph_text(&d, 0),
            "\u{e9}\u{fc}\u{f1}\u{c7} \u{ab}\u{bb} \u{2026} \u{20ac}\u{a3}\u{a5}\u{a2} \
             \u{2122}\u{ae} \u{d7}\u{f7} \u{b0}\u{b1} \u{2022}\u{b7} \u{201c}\u{201d}\u{2018}\u{2019} \
             \u{2013} ' \u{bd} \u{2192}"
        );
    }

    #[test]
    fn the_entity_table_is_sorted_for_the_search() {
        assert!(super::NAMED_ENTITIES.windows(2).all(|w| w[0].0 < w[1].0));
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
