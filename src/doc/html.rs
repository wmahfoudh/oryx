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
    seal_blocks, Block, BlockKind, CodeBody, DetailsGroup, Marker, Span, SpanImage, SpanScript,
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

/// Subtrees with no rendered text. `<head>` walks normally: its `<style>`
/// feeds the emphasis table and `<title>` is skipped on its own.
const SKIP_TAGS: &[&str] = &["script", "template", "title"];

/// Font families whose presence first in a `font-family` list marks
/// typewriter text; the generic `monospace` anywhere does the same.
const MONO_FACES: &[&str] = &[
    "andale mono",
    "consolas",
    "courier",
    "courier new",
    "courier prime",
    "dejavu sans mono",
    "fira code",
    "fira mono",
    "liberation mono",
    "lucida console",
    "menlo",
    "monaco",
    "roboto mono",
    "source code pro",
    "ubuntu mono",
];

/// The emphasis traits a stylesheet may speak about; everything else in
/// a book's CSS is discarded unread.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmphasisFlags {
    pub italic: bool,
    pub bold: bool,
    pub strike: bool,
    pub underline: bool,
    pub sub: bool,
    pub sup: bool,
    pub mono: bool,
    pub centered: bool,
}

/// What one selector says: traits it turns on and traits it explicitly
/// turns off (`normal`, `none`); an unmentioned trait inherits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Emphasis {
    pub set: EmphasisFlags,
    pub clear: EmphasisFlags,
}

macro_rules! each_trait {
    ($macro:ident) => {
        $macro!(italic);
        $macro!(bold);
        $macro!(strike);
        $macro!(underline);
        $macro!(sub);
        $macro!(sup);
        $macro!(mono);
        $macro!(centered);
    };
}

impl Emphasis {
    /// Later rules win per trait, CSS order semantics without a cascade.
    fn overwrite(&mut self, from: Emphasis) {
        macro_rules! merge {
            ($f:ident) => {
                if from.set.$f || from.clear.$f {
                    self.set.$f = from.set.$f;
                    self.clear.$f = from.clear.$f;
                }
            };
        }
        each_trait!(merge);
    }

    /// Whether any span trait is mentioned; centered is a block trait
    /// and rides its own path.
    fn mentions_inline(&self) -> bool {
        let (s, c) = (&self.set, &self.clear);
        s.italic
            || s.bold
            || s.strike
            || s.underline
            || s.sub
            || s.sup
            || s.mono
            || c.italic
            || c.bold
            || c.strike
            || c.underline
            || c.sub
            || c.sup
            || c.mono
    }

    /// The block trait: Some(true) centered, Some(false) explicitly not,
    /// None unmentioned.
    fn centered(&self) -> Option<bool> {
        if self.set.centered {
            Some(true)
        } else if self.clear.centered {
            Some(false)
        } else {
            None
        }
    }
}

/// The inherited CSS emphasis at a point in the tree: per trait, forced
/// on, forced off, or inherited from the tags.
#[derive(Debug, Default, Clone, Copy)]
struct CssState {
    italic: Option<bool>,
    bold: Option<bool>,
    strike: Option<bool>,
    underline: Option<bool>,
    sub: Option<bool>,
    sup: Option<bool>,
    mono: Option<bool>,
}

impl CssState {
    fn apply(mut self, e: &Emphasis) -> CssState {
        macro_rules! apply {
            ($f:ident) => {
                if e.set.$f {
                    self.$f = Some(true);
                } else if e.clear.$f {
                    self.$f = Some(false);
                }
            };
        }
        apply!(italic);
        apply!(bold);
        apply!(strike);
        apply!(underline);
        apply!(sub);
        apply!(sup);
        apply!(mono);
        self
    }
}

/// The six-property reading of a book's stylesheets: class and element
/// selectors mapped to emphasis, everything else discarded unread. Not
/// a CSS engine; there is no cascade and no specificity beyond element,
/// class, element.class in that order.
#[derive(Default)]
pub struct EmphasisTable {
    rules: std::collections::HashMap<(String, String), Emphasis>,
}

impl EmphasisTable {
    /// Folds one stylesheet in. Rules under combinators, pseudo-classes,
    /// ids, or attribute selectors are skipped whole; at-rules skip with
    /// their blocks.
    pub fn add_css(&mut self, css: &str) {
        let css = strip_comments(css);
        let mut rest = css.as_str();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            if rest.starts_with('@') {
                rest = skip_at_rule(rest);
                continue;
            }
            let Some(open) = rest.find('{') else {
                break;
            };
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            let selectors = &rest[..open];
            let emphasis = Self::declarations(&rest[open + 1..open + close]);
            if emphasis != Emphasis::default() {
                for selector in selectors.split(',') {
                    if let Some(key) = selector_key(selector) {
                        self.rules.entry(key).or_default().overwrite(emphasis);
                    }
                }
            }
            rest = &rest[open + close + 1..];
        }
    }

    /// Reads one declaration block for the six properties.
    pub fn declarations(text: &str) -> Emphasis {
        let mut out = Emphasis::default();
        for declaration in text.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            let property = property.trim().to_ascii_lowercase();
            let value = value
                .trim()
                .trim_end_matches("!important")
                .trim()
                .to_ascii_lowercase();
            match property.as_str() {
                "font-style" => {
                    if value.contains("italic") || value.contains("oblique") {
                        out.set.italic = true;
                        out.clear.italic = false;
                    } else if value == "normal" {
                        out.set.italic = false;
                        out.clear.italic = true;
                    }
                }
                "font-weight" => {
                    let heavy = value == "bold"
                        || value == "bolder"
                        || value
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .is_some_and(|w| w >= 600);
                    if heavy {
                        out.set.bold = true;
                        out.clear.bold = false;
                    } else if value == "normal"
                        || value == "lighter"
                        || value.parse::<u32>().is_ok()
                    {
                        out.set.bold = false;
                        out.clear.bold = true;
                    }
                }
                "text-decoration" | "text-decoration-line" => {
                    if value.split_whitespace().any(|w| w == "none") {
                        out.set.strike = false;
                        out.clear.strike = true;
                        out.set.underline = false;
                        out.clear.underline = true;
                    } else {
                        if value.split_whitespace().any(|w| w == "line-through") {
                            out.set.strike = true;
                            out.clear.strike = false;
                        }
                        if value.split_whitespace().any(|w| w == "underline") {
                            out.set.underline = true;
                            out.clear.underline = false;
                        }
                    }
                }
                "vertical-align" => match value.as_str() {
                    "sub" => {
                        out.set.sub = true;
                        out.clear.sub = false;
                        out.set.sup = false;
                        out.clear.sup = true;
                    }
                    "super" => {
                        out.set.sup = true;
                        out.clear.sup = false;
                        out.set.sub = false;
                        out.clear.sub = true;
                    }
                    "baseline" => {
                        out.set.sub = false;
                        out.clear.sub = true;
                        out.set.sup = false;
                        out.clear.sup = true;
                    }
                    _ => {}
                },
                "text-align" => match value.as_str() {
                    "center" => {
                        out.set.centered = true;
                        out.clear.centered = false;
                    }
                    "left" | "right" | "justify" | "start" | "end" => {
                        out.set.centered = false;
                        out.clear.centered = true;
                    }
                    _ => {}
                },
                "font-family" => {
                    let mut families = value
                        .split(',')
                        .map(|f| f.trim().trim_matches(['"', '\'']).trim());
                    let generic = families.clone().any(|f| f == "monospace");
                    let first_known = families.next().is_some_and(|f| MONO_FACES.contains(&f));
                    if generic || first_known {
                        out.set.mono = true;
                        out.clear.mono = false;
                    } else {
                        out.set.mono = false;
                        out.clear.mono = true;
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The emphasis for one element: element rule, then class rules,
    /// then element.class rules, later lookups winning per trait.
    pub fn resolve(&self, element: &str, class_attr: &str) -> Emphasis {
        let mut out = Emphasis::default();
        if self.rules.is_empty() {
            return out;
        }
        let element = element.to_ascii_lowercase();
        if let Some(e) = self.rules.get(&(element.clone(), String::new())) {
            out.overwrite(*e);
        }
        for class in class_attr.split_whitespace() {
            if let Some(e) = self.rules.get(&(String::new(), class.to_string())) {
                out.overwrite(*e);
            }
        }
        for class in class_attr.split_whitespace() {
            if let Some(e) = self.rules.get(&(element.clone(), class.to_string())) {
                out.overwrite(*e);
            }
        }
        out
    }
}

/// Raw text of a subtree, tags dropped; the `<style>` body reader.
fn collect_text(node: &Handle, out: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        out.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        collect_text(child, out);
    }
}

/// `element`, `.class`, or `element.class`; anything else is None.
fn selector_key(selector: &str) -> Option<(String, String)> {
    let selector = selector.trim();
    if selector.is_empty()
        || selector
            .chars()
            .any(|c| c.is_whitespace() || ">+~:[#*\"'&()".contains(c))
    {
        return None;
    }
    let mut parts = selector.split('.');
    let element = parts.next().unwrap_or("").to_ascii_lowercase();
    let class = parts.next().unwrap_or("").to_string();
    if parts.next().is_some() || (element.is_empty() && class.is_empty()) {
        return None;
    }
    Some((element, class))
}

/// Joins an href onto a base directory: `../` collapses, `./` drops, a
/// leading `/` starts from the archive root, and percent escapes decode,
/// since hrefs are URLs and archive names are not.
pub(crate) fn join_href(base: &str, href: &str) -> String {
    let href = percent_decode(href);
    let mut parts: Vec<&str> = if href.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|p| !p.is_empty()).collect()
    };
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

pub(crate) fn percent_decode(href: &str) -> std::borrow::Cow<'_, str> {
    if !href.contains('%') {
        return std::borrow::Cow::Borrowed(href);
    }
    let bytes = href.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| u8::from_str_radix(&href[i + 1..i + 3], 16).ok())
            .flatten();
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    std::borrow::Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

/// An inline `<svg>` subtree awaiting rasterization: its serialized
/// markup and the raw hrefs its `<image>` elements reference, for the
/// caller to inline before decoding.
pub struct PendingSvg {
    pub key: String,
    pub markup: String,
    pub refs: Vec<String>,
}

fn serialize_subtree(node: &Handle) -> String {
    let mut out = Vec::new();
    let handle: markup5ever_rcdom::SerializableHandle = node.clone().into();
    let opts = html5ever::serialize::SerializeOpts {
        traversal_scope: html5ever::serialize::TraversalScope::IncludeNode,
        ..Default::default()
    };
    let _ = html5ever::serialize::serialize(&mut out, &handle, opts);
    String::from_utf8_lossy(&out).into_owned()
}

/// Archive hrefs referenced by `<image>` elements in an svg subtree;
/// data URIs and remote URLs stay as they are.
fn svg_refs(node: &Handle, out: &mut Vec<String>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        if name.local.as_ref().eq_ignore_ascii_case("image") {
            for a in attrs.borrow().iter() {
                if a.name.local.as_ref() == "href" {
                    let value = a.value.to_string();
                    if !value.starts_with("data:") && !value.starts_with("http") {
                        out.push(value);
                    }
                }
            }
        }
    }
    for child in node.children.borrow().iter() {
        svg_refs(child, out);
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start..].find("*/") {
            Some(end) => rest = &rest[start + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Skips an at-rule: to its semicolon, or over its balanced block.
fn skip_at_rule(rest: &str) -> &str {
    let stop = rest.find(['{', ';']);
    match stop {
        Some(i) if rest.as_bytes()[i] == b';' => &rest[i + 1..],
        Some(mut i) => {
            let bytes = rest.as_bytes();
            let mut depth = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return &rest[i + 1..];
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            ""
        }
        None => "",
    }
}

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
    emphasis: EmphasisTable,
    /// Inherited CSS emphasis down the tree; the root is all-inherit.
    css: Vec<CssState>,
    /// The current chapter's directory inside the archive; relative
    /// image sources resolve against it.
    base: String,
    /// Image sources the current chapter referenced, for extraction.
    images: Vec<String>,
    /// Inline `<svg>` subtrees the current chapter held.
    svgs: Vec<PendingSvg>,
    svg_serial: u32,
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
            emphasis: EmphasisTable::default(),
            css: vec![CssState::default()],
            base: String::new(),
            images: Vec::new(),
            svgs: Vec::new(),
            svg_serial: 0,
        }
    }

    /// The book's stylesheet reading; chapters' own `<style>` elements
    /// fold in as they walk.
    pub fn set_emphasis(&mut self, table: EmphasisTable) {
        self.emphasis = table;
    }

    /// The archive directory of the chapter about to walk.
    pub fn set_chapter_base(&mut self, base: &str) {
        self.base = base.to_string();
    }

    /// Image sources referenced since the last take, resolved.
    pub fn take_images(&mut self) -> Vec<String> {
        std::mem::take(&mut self.images)
    }

    /// Inline svgs gathered since the last take.
    pub fn take_svgs(&mut self) -> Vec<PendingSvg> {
        std::mem::take(&mut self.svgs)
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

    /// A sealed copy of the book so far, the walker left to continue;
    /// the prefix document at open. Sealing decides by text and offset,
    /// so the copy equals the same blocks inside a later `finish`.
    pub fn snapshot(&self) -> (Vec<Block>, String, Vec<DetailsGroup>) {
        let mut blocks = self.blocks.clone();
        seal_blocks(&mut blocks, &self.source);
        (blocks, self.source.clone(), self.details.clone())
    }

    /// Bytes of book text walked so far; the prefix cut reads it.
    pub fn source_len(&self) -> usize {
        self.source.len()
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
        let class_attr = attr("class").unwrap_or_default();
        let mut emphasis = self.emphasis.resolve(tag, &class_attr);
        if let Some(style) = attr("style") {
            emphasis.overwrite(EmphasisTable::declarations(&style));
        }
        let pushed = emphasis.mentions_inline();
        if pushed {
            let top = *self.css.last().expect("the root css state");
            self.css.push(top.apply(&emphasis));
        }
        self.dispatch(tag, attr, node, &emphasis);
        if pushed {
            self.css.pop();
        }
    }

    fn dispatch(
        &mut self,
        tag: &str,
        attr: &dyn Fn(&str) -> Option<String>,
        node: &Handle,
        emphasis: &Emphasis,
    ) {
        // Inside a table, block structure flattens into the open cell;
        // only the table family and inline styling keep their meaning.
        let in_table = self.table.is_some();
        match tag {
            "br" => self.linebreak(),
            "style" => {
                let mut css = String::new();
                collect_text(node, &mut css);
                self.emphasis.add_css(&css);
            }
            "img" => {
                let alt = attr("alt").unwrap_or_default();
                let src = attr("src")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                match src {
                    Some(src) => {
                        let src = if src.starts_with("http://") || src.starts_with("https://") {
                            src
                        } else {
                            join_href(&self.base, &src)
                        };
                        let mut span = self.style();
                        span.set_text(alt);
                        span.image = Some(SpanImage {
                            src: src.clone(),
                            width: attr("width").and_then(|v| v.parse().ok()),
                            height: attr("height").and_then(|v| v.parse().ok()),
                        });
                        self.spans.push(span);
                        self.images.push(src);
                    }
                    None => self.push_str(&alt),
                }
            }
            "svg" => {
                let key = format!("svg:{}:{}", self.base, self.svg_serial);
                self.svg_serial += 1;
                let markup = serialize_subtree(node);
                let mut refs = Vec::new();
                svg_refs(node, &mut refs);
                let mut span = self.style();
                span.set_text("");
                span.image = Some(SpanImage {
                    src: key.clone(),
                    width: attr("width").and_then(|v| v.parse().ok()),
                    height: attr("height").and_then(|v| v.parse().ok()),
                });
                self.spans.push(span);
                self.svgs.push(PendingSvg { key, markup, refs });
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
                let centered = attr("align").is_some_and(|a| a.eq_ignore_ascii_case("center"))
                    || emphasis.centered() == Some(true);
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

    /// A span carrying the current inline style and no text yet. The
    /// CSS state decides a mentioned trait either way; the tag counters
    /// decide the rest.
    fn style(&self) -> Span {
        let css = self.css.last().copied().unwrap_or_default();
        let mut span = Span::plain("");
        span.bold = css.bold.unwrap_or(self.bold > 0 || self.dt);
        span.italic = css.italic.unwrap_or(self.italic > 0);
        span.strike = css.strike.unwrap_or(self.strike > 0);
        span.underline = css.underline.unwrap_or(self.underline > 0);
        span.mark = self.marked > 0;
        span.code = css.mono.unwrap_or(self.coded > 0);
        span.script = if css.sub.unwrap_or(self.sub > 0) {
            SpanScript::Sub
        } else if css.sup.unwrap_or(self.sup > 0) {
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
        walk_styled("", xhtml)
    }

    fn walk_styled(css: &str, xhtml: &str) -> (Vec<Block>, String, Vec<DetailsGroup>) {
        let mut walker = Walker::new();
        let mut table = EmphasisTable::default();
        table.add_css(css);
        walker.set_emphasis(table);
        walker.walk_chapter(xhtml);
        walker.finish()
    }

    fn spans_of(block: &Block) -> &[Span] {
        match &block.kind {
            BlockKind::Paragraph { spans } => spans,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn a_class_italicizes_its_span() {
        let (blocks, source, _) = walk_styled(
            ".i { font-style: italic }",
            "<html><body><p><span class=\"i\">soft</span> hard</p></body></html>",
        );
        let spans = spans_of(&blocks[0]);
        assert!(
            spans
                .iter()
                .find(|s| s.text(&source) == "soft")
                .unwrap()
                .italic
        );
        assert!(
            !spans
                .iter()
                .find(|s| s.text(&source) == " hard")
                .unwrap()
                .italic
        );
    }

    #[test]
    fn an_element_scoped_class_applies_only_there() {
        let (blocks, _, _) = walk_styled(
            "p.si { font-style: italic }",
            "<html><body><p class=\"si\">a</p><div class=\"si\">b</div></body></html>",
        );
        assert!(spans_of(&blocks[0])[0].italic);
        assert!(!spans_of(&blocks[1])[0].italic);
    }

    #[test]
    fn normal_clears_inherited_italic_from_class_and_tag() {
        let (blocks, source, _) = walk_styled(
            ".it { font-style: italic } .up { font-style: normal }",
            "<html><body><p class=\"it\">one <span class=\"up\">two</span> three</p>\
             <p><i>a <span class=\"up\">b</span></i></p></body></html>",
        );
        let first = spans_of(&blocks[0]);
        assert!(
            first
                .iter()
                .find(|s| s.text(&source) == "one")
                .unwrap()
                .italic
        );
        assert!(
            !first
                .iter()
                .find(|s| s.text(&source) == " two")
                .unwrap()
                .italic
        );
        assert!(
            first
                .iter()
                .find(|s| s.text(&source) == " three")
                .unwrap()
                .italic
        );
        let second = spans_of(&blocks[1]);
        assert!(
            second
                .iter()
                .find(|s| s.text(&source) == "a")
                .unwrap()
                .italic
        );
        assert!(
            !second
                .iter()
                .find(|s| s.text(&source) == " b")
                .unwrap()
                .italic
        );
    }

    #[test]
    fn numeric_weight_bolds() {
        let (blocks, _, _) = walk_styled(
            ".b { font-weight: 700 }",
            "<html><body><p class=\"b\">heavy</p></body></html>",
        );
        assert!(spans_of(&blocks[0])[0].bold);
    }

    #[test]
    fn decorations_strike_underline_and_clear() {
        let (blocks, source, _) = walk_styled(
            ".s { text-decoration: line-through } .u { text-decoration: underline } .n { text-decoration: none }",
            "<html><body><p><span class=\"s\">gone</span><span class=\"u\">under</span></p>\
             <p><u>x <span class=\"n\">y</span></u></p></body></html>",
        );
        let first = spans_of(&blocks[0]);
        assert!(
            first
                .iter()
                .find(|s| s.text(&source) == "gone")
                .unwrap()
                .strike
        );
        assert!(
            first
                .iter()
                .find(|s| s.text(&source) == "under")
                .unwrap()
                .underline
        );
        let second = spans_of(&blocks[1]);
        assert!(
            second
                .iter()
                .find(|s| s.text(&source) == "x")
                .unwrap()
                .underline
        );
        assert!(
            !second
                .iter()
                .find(|s| s.text(&source) == " y")
                .unwrap()
                .underline
        );
    }

    #[test]
    fn vertical_align_reaches_the_script_mechanism() {
        let (blocks, source, _) = walk_styled(
            ".sb { vertical-align: sub } .sp { vertical-align: super }",
            "<html><body><p>a<span class=\"sb\">1</span><span class=\"sp\">2</span></p></body></html>",
        );
        let spans = spans_of(&blocks[0]);
        assert_eq!(
            spans
                .iter()
                .find(|s| s.text(&source) == "1")
                .unwrap()
                .script,
            SpanScript::Sub
        );
        assert_eq!(
            spans
                .iter()
                .find(|s| s.text(&source) == "2")
                .unwrap()
                .script,
            SpanScript::Sup
        );
    }

    #[test]
    fn text_align_center_centers_the_block() {
        let (blocks, _, _) = walk_styled(
            ".tb { text-align: center }",
            "<html><body><p class=\"tb\">* * *</p></body></html>",
        );
        assert!(blocks[0].centered);
    }

    #[test]
    fn a_monospace_family_renders_as_inline_code() {
        let (blocks, _, _) = walk_styled(
            ".mono { font-family: \"Courier New\", monospace }",
            "<html><body><p class=\"mono\">STOP</p></body></html>",
        );
        assert!(spans_of(&blocks[0])[0].code);
    }

    #[test]
    fn other_properties_and_combinators_are_ignored() {
        let (blocks, _, _) = walk_styled(
            ".c { color: red; font-size: 30px } div > p { font-style: italic }",
            "<html><body><div><p class=\"c\">plain</p></div></body></html>",
        );
        let span = &spans_of(&blocks[0])[0];
        assert!(!span.italic && !span.bold && !span.code);
    }

    #[test]
    fn a_chapter_style_element_styles_that_chapter() {
        let (blocks, _, _) = walk(
            "<html><head><style>.i { font-style: italic }</style></head>\
             <body><p class=\"i\">styled</p></body></html>",
        );
        assert!(spans_of(&blocks[0])[0].italic);
    }

    #[test]
    fn a_style_attribute_styles_its_element() {
        let (blocks, source, _) = walk(
            "<html><body><p style=\"font-style: italic\">a <span style=\"font-style: normal\">b</span></p></body></html>",
        );
        let spans = spans_of(&blocks[0]);
        assert!(
            spans
                .iter()
                .find(|s| s.text(&source) == "a")
                .unwrap()
                .italic
        );
        assert!(
            !spans
                .iter()
                .find(|s| s.text(&source) == " b")
                .unwrap()
                .italic
        );
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
    fn img_maps_to_an_inline_image_span_with_size() {
        let mut walker = Walker::new();
        walker.set_chapter_base("OEBPS/text");
        walker.walk_chapter(
            "<html><body><p><img src=\"pic.png\" width=\"64\" height=\"32\" alt=\"B\"/></p></body></html>",
        );
        let (blocks, source, _) = walker.finish();
        let spans = spans_of(&blocks[0]);
        let image = spans[0].image.as_ref().expect("an image span");
        assert_eq!(image.src, "OEBPS/text/pic.png");
        assert_eq!(image.width, Some(64));
        assert_eq!(image.height, Some(32));
        assert_eq!(spans[0].text(&source), "B");
    }

    #[test]
    fn join_href_resolves_dots_root_and_escapes() {
        assert_eq!(
            join_href("OEBPS/text", "../images/a%20b.png"),
            "OEBPS/images/a b.png"
        );
        assert_eq!(join_href("OEBPS/text", "/images/x.png"), "images/x.png");
        assert_eq!(join_href("", "x.png"), "x.png");
        assert_eq!(join_href("OEBPS/text", "pic.png"), "OEBPS/text/pic.png");
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
