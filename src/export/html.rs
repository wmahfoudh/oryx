//! The selection as HTML for the clipboard, beside the plain text: a
//! fragment with every style inline, since mail clients strip style
//! sheets, and pictures embedded as data addresses. The walk keeps to
//! the blocks and pieces `selection::plain_text` copies, so the two
//! versions cut at the same places and a program takes the one it reads.
//!
//! The look is neutral: no body font or size, so the email or document
//! keeps its own. Code takes a monospace face on the export theme's
//! code background with its syntax colors, tables thin borders, quotes
//! a bar on the left.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Range;

use base64::Engine;

use crate::doc::images::MediaCache;
use crate::doc::model::{
    AlertKind, Block, BlockKind, ColumnAlign, Document, Marker, Span, SpanScript,
};
use crate::layout::{alert_color, alert_title, layout, math_display, role_color, ViewConfig};
use crate::paint::band;
use crate::style::fonts::FontStore;
use crate::style::highlight::{role_face, SyntaxRole};
use crate::style::theme::{hex_string, Rgba, Theme};
use crate::ui::selection::Selection;

/// An encoded picture for an `<img>`, with the size it shows at in CSS
/// pixels.
#[derive(Clone)]
pub struct Picture {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Where the writer gets its pictures: the app answers from its image
/// cache and its math typesetter, a test answers what it likes.
pub trait Pictures {
    /// The picture an image source names, or None when it cannot load.
    fn image(&mut self, src: &str) -> Option<Picture>;
    /// A formula typeset as a picture; `display` for a block of its
    /// own, as against a formula inside a sentence.
    fn math(&mut self, tex: &str, display: bool) -> Option<Picture>;
}

/// No pictures at all: images fall back to their text, formulas to
/// their plain reading.
pub struct NoPictures;

impl Pictures for NoPictures {
    fn image(&mut self, _src: &str) -> Option<Picture> {
        None
    }

    fn math(&mut self, _tex: &str, _display: bool) -> Option<Picture> {
        None
    }
}

/// The selected range as an HTML fragment. Only the endpoint blocks
/// slice; everything between comes whole, the rule of the plain copy.
/// Empty when nothing is selected.
pub fn selection_html(
    sel: &Selection,
    doc: &Document,
    theme: &Theme,
    pictures: &mut dyn Pictures,
) -> String {
    if sel.is_empty() || doc.blocks.is_empty() {
        return String::new();
    }
    let (a, b) = sel.ordered();
    let mut w = Writer {
        doc,
        theme,
        pictures,
        out: String::new(),
        notes: Vec::new(),
        quotes: Vec::new(),
        lists: Vec::new(),
        seen: HashMap::new(),
    };
    for index in a.block..=b.block.min(doc.blocks.len() - 1) {
        let cut = Cut {
            first: (index == a.block).then_some((a.span, a.byte)),
            last: (index == b.block).then_some((b.span, b.byte)),
        };
        w.block(index, &cut);
    }
    w.close_lists_to(0);
    w.close_quotes_to(0);
    if !w.notes.is_empty() {
        w.notes.sort_by_key(|(number, _)| *number);
        w.out.push_str("<hr><ol>");
        for (number, note) in &w.notes {
            let _ = write!(w.out, "<li value=\"{number}\">{note}</li>");
        }
        w.out.push_str("</ol>");
    }
    w.out
}

/// The part of one block a selection keeps, in the block's own
/// addresses: the first piece and the byte it starts at, the last and
/// the byte it ends at. None on either side means the block's edge.
struct Cut {
    first: Option<(usize, usize)>,
    last: Option<(usize, usize)>,
}

impl Cut {
    /// Whether the piece at `span` lies inside the cut, its bytes aside.
    fn keeps(&self, span: usize) -> bool {
        self.first.is_none_or(|(s, _)| span >= s) && self.last.is_none_or(|(s, _)| span <= s)
    }

    /// The bytes of a piece the selection keeps. None when the piece is
    /// outside the cut, or when the selection starts at its end or ends
    /// at its start, the cases the plain copy skips as well; an empty
    /// piece strictly inside comes back empty, the blank line of a code
    /// block.
    fn slice<'t>(&self, span: usize, text: &'t str) -> Option<&'t str> {
        let mut from = 0usize;
        let mut to = text.len();
        let mut at_edge = false;
        if let Some((s, byte)) = self.first {
            match span.cmp(&s) {
                Ordering::Less => return None,
                Ordering::Equal => {
                    from = byte.min(text.len());
                    at_edge = true;
                }
                Ordering::Greater => {}
            }
        }
        if let Some((s, byte)) = self.last {
            match span.cmp(&s) {
                Ordering::Greater => return None,
                Ordering::Equal => {
                    to = byte.min(text.len());
                    at_edge = true;
                }
                Ordering::Less => {}
            }
        }
        if from >= to {
            return (!at_edge).then_some("");
        }
        Some(&text[floor_boundary(text, from)..floor_boundary(text, to)])
    }
}

fn floor_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// One open list: its depth in the model, its tag, and whether an item
/// is open inside it, so a nested list lands inside the item.
struct ListLevel {
    depth: u8,
    tag: &'static str,
    item_open: bool,
}

struct Writer<'a> {
    doc: &'a Document,
    theme: &'a Theme,
    pictures: &'a mut dyn Pictures,
    out: String,
    /// The footnote definitions by number, gathered at the end in the
    /// page's order, whatever order the source declared them in.
    notes: Vec<(u32, String)>,
    /// The open blockquotes, outermost first, each with the alert it
    /// opened for.
    quotes: Vec<Option<AlertKind>>,
    lists: Vec<ListLevel>,
    /// The pictures asked for so far, by source: a picture a page
    /// shows twice is read and encoded once.
    seen: HashMap<String, Option<Picture>>,
}

impl Writer<'_> {
    fn picture(&mut self, src: &str) -> Option<Picture> {
        if let Some(known) = self.seen.get(src) {
            return known.clone();
        }
        let picture = self.pictures.image(src);
        self.seen.insert(src.to_string(), picture.clone());
        picture
    }

    fn block(&mut self, index: usize, cut: &Cut) {
        let doc = self.doc;
        let block = &doc.blocks[index];
        let source = &*doc.source;
        match &block.kind {
            BlockKind::Heading { level, spans, .. } => {
                let Some(inner) = self.spans_html(spans, 0, cut) else {
                    return;
                };
                self.enter(block);
                let level = (*level).clamp(1, 6);
                let _ = write!(self.out, "<h{level}{}>{inner}</h{level}>", align(block));
            }
            BlockKind::Paragraph { spans } => {
                let Some(inner) = self.spans_html(spans, 0, cut) else {
                    return;
                };
                self.enter(block);
                let _ = write!(self.out, "<p{}>{inner}</p>", align(block));
            }
            BlockKind::Summary { spans, .. } => {
                let Some(inner) = self.spans_html(spans, 0, cut) else {
                    return;
                };
                self.enter(block);
                let _ = write!(self.out, "<p><strong>{inner}</strong></p>");
            }
            BlockKind::ListItem {
                marker,
                depth,
                spans,
            } => {
                let Some(inner) = self.spans_html(spans, 0, cut) else {
                    return;
                };
                self.enter_quotes(block);
                self.enter_list(*depth, marker);
                let prefix = match marker {
                    Marker::Task { checked: true, .. } => "\u{2611} ",
                    Marker::Task { checked: false, .. } => "\u{2610} ",
                    _ => "",
                };
                let style = match marker {
                    Marker::Bullet | Marker::Number(_) => "",
                    Marker::Task { .. } | Marker::None => " style=\"list-style-type:none\"",
                };
                let _ = write!(self.out, "<li{style}>{prefix}{inner}");
            }
            BlockKind::FootnoteDef { number, spans, .. } => {
                let Some(inner) = self.spans_html(spans, 0, cut) else {
                    return;
                };
                let note = match self.notes.iter_mut().find(|(n, _)| n == number) {
                    Some((_, note)) => note,
                    None => {
                        self.notes.push((*number, String::new()));
                        &mut self.notes.last_mut().expect("just pushed").1
                    }
                };
                let _ = write!(note, "<p>{inner}</p>");
            }
            BlockKind::CodeBlock {
                lines, highlights, ..
            } => {
                let mut body = String::new();
                let mut any = false;
                for i in 0..lines.len() {
                    let line = lines.line(source, i);
                    let Some(kept) = cut.slice(i, line) else {
                        continue;
                    };
                    if any {
                        body.push('\n');
                    }
                    any = true;
                    // A blank line strictly inside the cut comes back as
                    // an empty piece of its own, not a slice of the line.
                    let start = if kept.is_empty() {
                        0
                    } else {
                        kept.as_ptr() as usize - line.as_ptr() as usize
                    };
                    let segments = highlights.get(i).map_or(&[][..], Vec::as_slice);
                    self.code_line(&mut body, kept, start, segments);
                }
                if !any {
                    return;
                }
                self.enter(block);
                let _ = write!(
                    self.out,
                    "<pre style=\"font-family:monospace;background:{};color:{};border:1px solid {};border-radius:4px;padding:8px 12px;white-space:pre-wrap\">{body}</pre>",
                    self.css(self.theme.blocks.code_bg),
                    self.css(self.theme.surface.foreground),
                    self.css(self.theme.blocks.code_border),
                );
            }
            BlockKind::Table {
                header,
                rows,
                aligns,
            } => {
                let border = self.css(self.theme.blocks.table_border);
                let mut chain = 0usize;
                let mut head = String::new();
                let mut body = String::new();
                for (r, row) in std::iter::once(header).chain(rows.iter()).enumerate() {
                    let first = chain;
                    let count: usize = row.iter().map(Vec::len).sum();
                    chain += count;
                    if row.is_empty() || !(first..first + count).any(|c| cut.keeps(c)) {
                        continue;
                    }
                    let tag = if r == 0 { "th" } else { "td" };
                    let out = if r == 0 { &mut head } else { &mut body };
                    out.push_str("<tr>");
                    let mut base = first;
                    for (c, cell) in row.iter().enumerate() {
                        let inner = self.spans_html(cell, base, cut).unwrap_or_default();
                        base += cell.len();
                        // A header cell without a mark stays at the left,
                        // where a browser would center it.
                        let align = match aligns.get(c) {
                            Some(ColumnAlign::Left) => ";text-align:left",
                            Some(ColumnAlign::Center) => ";text-align:center",
                            Some(ColumnAlign::Right) => ";text-align:right",
                            _ if r == 0 => ";text-align:left",
                            _ => "",
                        };
                        let _ = write!(
                            out,
                            "<{tag} style=\"border:1px solid {border};padding:4px 8px{align}\">{inner}</{tag}>"
                        );
                    }
                    out.push_str("</tr>");
                }
                if head.is_empty() && body.is_empty() {
                    return;
                }
                self.enter(block);
                self.out
                    .push_str("<table style=\"border-collapse:collapse\">");
                if !head.is_empty() {
                    let _ = write!(self.out, "<thead>{head}</thead>");
                }
                if !body.is_empty() {
                    let _ = write!(self.out, "<tbody>{body}</tbody>");
                }
                self.out.push_str("</table>");
            }
            BlockKind::MathBlock { tex } => {
                let text = math_display(tex);
                if cut.slice(0, &text).is_none() {
                    return;
                }
                self.enter(block);
                match self.pictures.math(tex, true) {
                    Some(picture) => {
                        let size = (Some(picture.width), Some(picture.height));
                        let _ = write!(self.out, "<p>{}</p>", img(&picture, tex.trim(), "", size));
                    }
                    None => {
                        let _ = write!(self.out, "<p><i>{}</i></p>", escape(&text));
                    }
                }
            }
            BlockKind::Frontmatter { entries } => {
                let mut rows = String::new();
                for (i, (key, value)) in entries.iter().enumerate() {
                    if !cut.keeps(i) {
                        continue;
                    }
                    let _ = write!(
                        rows,
                        "<tr><td style=\"padding:2px 8px\"><strong>{}</strong></td><td style=\"padding:2px 8px\">{}</td></tr>",
                        escape(key),
                        escape(value)
                    );
                }
                if rows.is_empty() {
                    return;
                }
                self.enter(block);
                let _ = write!(
                    self.out,
                    "<table style=\"border-collapse:collapse\"><tbody>{rows}</tbody></table>"
                );
            }
            BlockKind::Image { path, alt } => {
                self.enter(block);
                match self.picture(path) {
                    Some(picture) => {
                        let _ = write!(self.out, "<p>{}</p>", img(&picture, alt, "", (None, None)));
                    }
                    None => {
                        let _ = write!(self.out, "<p><em>{}</em></p>", escape(alt));
                    }
                }
            }
            BlockKind::Rule => {
                self.enter(block);
                self.out.push_str("<hr>");
            }
            BlockKind::ChapterBreak { .. } | BlockKind::PageBreak => {}
        }
    }

    /// Closes what a block outside any list ends, and opens the quotes
    /// it sits in.
    fn enter(&mut self, block: &Block) {
        self.close_lists_to(0);
        self.enter_quotes(block);
    }

    /// Opens and closes blockquotes until the open ones are the block's:
    /// its depth, and its alert, an alert of another kind at the same
    /// depth closing the quote before it as the page starts a new panel.
    fn enter_quotes(&mut self, block: &Block) {
        let depth = usize::from(block.quote_depth);
        let reopen =
            depth > 0 && self.quotes.len() >= depth && self.quotes[depth - 1] != block.alert;
        if self.quotes.len() != depth || reopen {
            self.close_lists_to(0);
        }
        if reopen {
            self.close_quotes_to(depth - 1);
        }
        self.close_quotes_to(depth);
        while self.quotes.len() < depth {
            // The alert is the innermost quote's: a `> > [!NOTE]` opens
            // a plain quote around a note, as the page draws it.
            let alert = block.alert.filter(|_| self.quotes.len() + 1 == depth);
            let titled = alert.filter(|_| self.quotes.last().is_none_or(|a| *a != alert));
            let bar = match alert {
                Some(kind) => alert_color(self.theme, kind),
                None => self.theme.blocks.quote_bar,
            };
            let _ = write!(
                self.out,
                "<blockquote style=\"margin:8px 0;padding:2px 0 2px 12px;border-left:3px solid {}\">",
                self.css(bar)
            );
            if let Some(kind) = titled {
                let _ = write!(
                    self.out,
                    "<p style=\"color:{}\"><strong>{}</strong></p>",
                    self.css(alert_color(self.theme, kind)),
                    alert_title(kind)
                );
            }
            self.quotes.push(alert);
        }
    }

    fn close_quotes_to(&mut self, depth: usize) {
        while self.quotes.len() > depth {
            self.out.push_str("</blockquote>");
            self.quotes.pop();
        }
    }

    /// Opens or closes lists until one of the item's depth and kind is
    /// open, with its previous item closed. A deeper list opens inside
    /// the item before it, so the nesting reads as on the page.
    fn enter_list(&mut self, depth: u8, marker: &Marker) {
        let tag = match marker {
            Marker::Number(_) => "ol",
            _ => "ul",
        };
        while self
            .lists
            .last()
            .is_some_and(|l| l.depth > depth || (l.depth == depth && l.tag != tag))
        {
            self.close_list();
        }
        match self.lists.last_mut() {
            Some(level) if level.depth == depth => {
                if level.item_open {
                    self.out.push_str("</li>");
                }
                level.item_open = true;
            }
            _ => {
                let start = match marker {
                    Marker::Number(n) if *n != 1 => format!(" start=\"{n}\""),
                    _ => String::new(),
                };
                let _ = write!(self.out, "<{tag}{start}>");
                self.lists.push(ListLevel {
                    depth,
                    tag,
                    item_open: true,
                });
            }
        }
    }

    fn close_list(&mut self) {
        if let Some(level) = self.lists.pop() {
            if level.item_open {
                self.out.push_str("</li>");
            }
            let _ = write!(self.out, "</{}>", level.tag);
        }
    }

    fn close_lists_to(&mut self, count: usize) {
        while self.lists.len() > count {
            self.close_list();
        }
    }

    /// Inline spans as HTML, `base` being the address of the first one.
    /// None when the cut keeps none of them.
    fn spans_html(&mut self, spans: &[Span], base: usize, cut: &Cut) -> Option<String> {
        let source = &*self.doc.source;
        let mut out = String::new();
        let mut any = false;
        for (i, span) in spans.iter().enumerate() {
            let Some(text) = cut.slice(base + i, span.text(source)) else {
                continue;
            };
            any = true;
            self.span_html(&mut out, span, text);
        }
        any.then_some(out)
    }

    fn span_html(&mut self, out: &mut String, span: &Span, text: &str) {
        if let Some(image) = &span.image {
            let link = web_link(span.link.as_deref());
            if let Some(href) = &link {
                let _ = write!(out, "<a href=\"{}\">", escape(href));
            }
            match self.picture(&image.src) {
                Some(picture) => {
                    out.push_str(&img(&picture, text, "", (image.width, image.height)));
                }
                None => {
                    let _ = write!(out, "<em>{}</em>", escape(text));
                }
            }
            if link.is_some() {
                out.push_str("</a>");
            }
            return;
        }
        if span.math {
            match self.pictures.math(text, false) {
                Some(picture) => {
                    let size = (Some(picture.width), Some(picture.height));
                    out.push_str(&img(&picture, text, "vertical-align:middle", size));
                }
                None => {
                    let _ = write!(out, "<i>{}</i>", escape(text));
                }
            }
            return;
        }
        if span
            .link
            .as_deref()
            .is_some_and(|l| l.starts_with("footnote:"))
        {
            let _ = write!(out, "<sup>{}</sup>", escape(text));
            return;
        }
        let mut open: Vec<&'static str> = Vec::new();
        if let Some(href) = web_link(span.link.as_deref()) {
            let _ = write!(out, "<a href=\"{}\">", escape(&href));
            open.push("a");
        }
        if let Some(abbr) = span
            .abbr
            .and_then(|i| self.doc.abbreviations.get(i.get() as usize - 1))
        {
            let _ = write!(out, "<abbr title=\"{}\">", escape(abbr));
            open.push("abbr");
        }
        if span.bold {
            out.push_str("<strong>");
            open.push("strong");
        }
        if span.italic {
            out.push_str("<em>");
            open.push("em");
        }
        if span.strike {
            out.push_str("<s>");
            open.push("s");
        }
        if span.underline {
            out.push_str("<u>");
            open.push("u");
        }
        if span.mark {
            out.push_str("<mark>");
            open.push("mark");
        }
        match span.script {
            SpanScript::Sub => {
                out.push_str("<sub>");
                open.push("sub");
            }
            SpanScript::Sup => {
                out.push_str("<sup>");
                open.push("sup");
            }
            SpanScript::Small => {
                out.push_str("<small>");
                open.push("small");
            }
            SpanScript::None => {}
        }
        if span.code {
            let _ = write!(
                out,
                "<code style=\"font-family:monospace;background:{};color:{};padding:1px 4px;border-radius:3px\">",
                self.css(self.theme.text.inline_code_bg),
                self.css(self.theme.text.inline_code)
            );
            open.push("code");
        }
        // A line break inside a paragraph is a hard break the author
        // asked for, two spaces or a backslash at the line's end.
        out.push_str(&escape(text).replace('\n', "<br>"));
        for tag in open.iter().rev() {
            let _ = write!(out, "</{tag}>");
        }
    }

    /// One code line, or the kept part of it starting at `start`, with
    /// its colored segments as spans; the plain role takes the box's
    /// color.
    fn code_line(
        &self,
        out: &mut String,
        kept: &str,
        start: usize,
        segments: &[(Range<usize>, SyntaxRole)],
    ) {
        let end = start + kept.len();
        let mut at = start;
        for (range, role) in segments {
            let from = range.start.max(at);
            let to = range.end.min(end);
            if from >= to {
                continue;
            }
            if from > at {
                out.push_str(&escape(&kept[at - start..from - start]));
            }
            let piece = escape(&kept[from - start..to - start]);
            let (bold, italic) = role_face(*role);
            let color = role_color(self.theme, *role);
            if *role == SyntaxRole::Plain
                || (color == self.theme.surface.foreground && !bold && !italic)
            {
                out.push_str(&piece);
            } else {
                let mut style = format!("color:{}", self.css(color));
                if bold {
                    style.push_str(";font-weight:bold");
                }
                if italic {
                    style.push_str(";font-style:italic");
                }
                let _ = write!(out, "<span style=\"{style}\">{piece}</span>");
            }
            at = to;
        }
        if at < end {
            out.push_str(&escape(&kept[at - start..]));
        }
    }

    /// A color as CSS. A translucent role is laid over the page, the
    /// color the eye sees on screen, since a word processor reads no
    /// alpha.
    fn css(&self, color: Rgba) -> String {
        if color.a == 255 {
            return hex_string(color);
        }
        let bg = self.theme.surface.background;
        let mix = |c: u8, b: u8| {
            let a = f32::from(color.a) / 255.0;
            (f32::from(c) * a + f32::from(b) * (1.0 - a)).round() as u8
        };
        hex_string(Rgba {
            r: mix(color.r, bg.r),
            g: mix(color.g, bg.g),
            b: mix(color.b, bg.b),
            a: 255,
        })
    }
}

/// A link a mail client or a document can follow: a web address, or
/// an email address, which the page shows bare and a link needs as
/// `mailto:`. A section anchor or a file beside the note means nothing
/// once pasted elsewhere.
fn web_link(link: Option<&str>) -> Option<String> {
    let link = link?;
    if link.starts_with("http://") || link.starts_with("https://") || link.starts_with("mailto:") {
        return Some(link.to_string());
    }
    let (user, host) = link.split_once('@')?;
    let plain = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    };
    (plain(user) && plain(host) && host.contains('.')).then(|| format!("mailto:{link}"))
}

fn align(block: &Block) -> &'static str {
    if block.centered {
        " style=\"text-align:center\""
    } else if block.right {
        " style=\"text-align:right\""
    } else if block.left {
        " style=\"text-align:left\""
    } else {
        ""
    }
}

/// An `<img>` over an embedded picture. The size is the writer's to
/// give: a formula's picture is shown at the size it was typeset for,
/// a file's at its own unless the source gave one.
fn img(picture: &Picture, alt: &str, style: &str, size: (Option<u32>, Option<u32>)) -> String {
    let mut out = format!(
        "<img src=\"data:{};base64,{}\" alt=\"{}\"",
        picture.mime,
        base64::engine::general_purpose::STANDARD.encode(&picture.bytes),
        escape(alt)
    );
    if let Some(w) = size.0 {
        let _ = write!(out, " width=\"{w}\"");
    }
    if let Some(h) = size.1 {
        let _ = write!(out, " height=\"{h}\"");
    }
    if !style.is_empty() {
        let _ = write!(out, " style=\"{style}\"");
    }
    out.push('>');
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The page's own pictures: an image as the cache holds it, its stored
/// bytes when they are a picture a mail client shows, its pixels as a
/// PNG otherwise; a formula typeset by the layout and painted the way
/// the screen paints it, at twice the size for a crisp paste, its paper
/// made transparent.
pub struct PagePictures<'a> {
    pub theme: &'a Theme,
    pub cfg: &'a ViewConfig,
    pub fonts: &'a mut FontStore,
    pub media: &'a mut MediaCache,
}

/// The scale a formula is painted at; its `<img>` shows it at one.
const MATH_SCALE: f32 = 2.0;
/// The width a formula is laid out in, at that scale.
const MATH_WIDTH: f32 = 1600.0;
/// Transparent room around a formula's ink.
const MATH_PAD: u32 = 2;

impl Pictures for PagePictures<'_> {
    fn image(&mut self, src: &str) -> Option<Picture> {
        if let Some(bytes) = self.media.stored_bytes(src) {
            if let Some(mime) = picture_mime(&bytes) {
                return Some(Picture {
                    mime,
                    bytes,
                    width: 0,
                    height: 0,
                });
            }
        }
        self.media.warm(src);
        let (width, height) = self.media.dimensions(src)?;
        let pixels = self.media.scaled(src, width, height)?.to_vec();
        let bytes = png_bytes(width, height, pixels)?;
        Some(Picture {
            mime: "image/png",
            bytes,
            width,
            height,
        })
    }

    fn math(&mut self, tex: &str, display: bool) -> Option<Picture> {
        let kind = if display {
            BlockKind::MathBlock {
                tex: tex.to_string(),
            }
        } else {
            let mut span = Span::plain(tex);
            span.math = true;
            BlockKind::Paragraph { spans: vec![span] }
        };
        let doc = Document {
            blocks: vec![Block::plain(kind)],
            ..Document::default()
        };
        let cfg = ViewConfig {
            zoom: MATH_SCALE,
            gutter: 0.0,
            print: true,
            ..self.cfg.clone()
        };
        let laid = layout(&doc, self.theme, self.fonts, self.media, &cfg, MATH_WIDTH);
        let width = MATH_WIDTH as u32;
        let height = laid.height.ceil().max(1.0) as u32;
        // Painted twice, over a black and a white paper: what the two
        // papers change in a pixel is the paper showing through, the rest
        // is ink, whatever its color (`\color` sets some), read back from
        // the black paper where it stands alone.
        let mut on_black = self.theme.clone();
        on_black.surface.background = crate::style::theme::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        };
        let mut on_white = self.theme.clone();
        on_white.surface.background = crate::style::theme::Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        let black = band(
            &laid,
            &doc,
            &on_black,
            self.fonts,
            self.media,
            &[],
            0.0,
            width,
            height,
        );
        let white = band(
            &laid,
            &doc,
            &on_white,
            self.fonts,
            self.media,
            &[],
            0.0,
            width,
            height,
        );
        let channel = |px: u32, shift: u32| ((px >> shift) & 0xFF) as i32;
        let alpha_of = |b: u32, w: u32| {
            [16u32, 8, 0]
                .into_iter()
                .map(|s| 255 - (channel(w, s) - channel(b, s)).clamp(0, 255))
                .max()
                .unwrap_or(0) as u8
        };
        let mut x0 = u32::MAX;
        let mut y0 = u32::MAX;
        let mut x1 = 0u32;
        let mut y1 = 0u32;
        for (i, (b, w)) in black.iter().zip(&white).enumerate() {
            if alpha_of(*b, *w) == 0 {
                continue;
            }
            let x = i as u32 % width;
            let y = i as u32 / width;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x0 > x1 {
            return None;
        }
        let x0 = x0.saturating_sub(MATH_PAD);
        let y0 = y0.saturating_sub(MATH_PAD);
        let x1 = (x1 + MATH_PAD).min(width - 1);
        let y1 = (y1 + MATH_PAD).min(height - 1);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = (y * width + x) as usize;
                let alpha = alpha_of(black[i], white[i]);
                let color = |s: u32| {
                    if alpha == 0 {
                        0
                    } else {
                        (channel(black[i], s) * 255 / i32::from(alpha)).min(255) as u8
                    }
                };
                rgba.extend_from_slice(&[color(16), color(8), color(0), alpha]);
            }
        }
        let bytes = png_bytes(w, h, rgba)?;
        Some(Picture {
            mime: "image/png",
            bytes,
            width: (w as f32 / MATH_SCALE).ceil() as u32,
            height: (h as f32 / MATH_SCALE).ceil() as u32,
        })
    }
}

/// The type of an image file by its first bytes, among the kinds a
/// mail client or a word processor shows inline.
fn picture_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn png_bytes(width: u32, height: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
