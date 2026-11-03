//! The HTML copy read back through html5ever, so the assertions are what
//! a mail client or a word processor receives rather than what we wrote.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use oryx::doc::markdown;
use oryx::doc::model::{Block, BlockKind, CodeBody, Document};
use oryx::export::html::{selection_html, NoPictures, Picture, Pictures};
use oryx::style::highlight;
use oryx::style::theme::Theme;
use oryx::ui::selection::{self, ModelPos, Selection};

/// A copy read back: the HTML and its parsed tree, held together since
/// dropping the tree empties every node.
struct Page {
    html: String,
    dom: RcDom,
}

impl std::fmt::Display for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.html)
    }
}

impl Page {
    fn parse(html: String) -> Page {
        let dom = html5ever::parse_fragment(
            RcDom::default(),
            Default::default(),
            html5ever::QualName::new(None, html5ever::ns!(html), html5ever::local_name!("body")),
            Vec::new(),
            false,
        )
        .one(html.as_str());
        Page { html, dom }
    }

    fn elements(&self, tag: &str) -> Vec<Handle> {
        let mut all = Vec::new();
        walk(&self.dom.document, &mut all);
        all.into_iter()
            .filter(|n| tag_of(n).as_deref() == Some(tag))
            .collect()
    }

    fn texts(&self, tag: &str) -> Vec<String> {
        self.elements(tag).iter().map(text_of).collect()
    }

    fn whole_text(&self) -> String {
        text_of(&self.dom.document)
    }

    fn contains(&self, needle: &str) -> bool {
        self.html.contains(needle)
    }
}

fn html_of(doc: &Document) -> Page {
    let sel = selection::all(doc).expect("something to select");
    Page::parse(selection_html(
        &sel,
        doc,
        &Theme::default_dark(),
        &mut NoPictures,
    ))
}

fn html_of_markdown(md: &str) -> Page {
    html_of(&markdown::parse(md))
}

/// Answers every picture and remembers what it was asked for.
struct Counting {
    asked: Vec<String>,
}

impl Pictures for Counting {
    fn image(&mut self, src: &str) -> Option<Picture> {
        self.asked.push(src.to_string());
        Some(Picture {
            mime: "image/png",
            bytes: vec![1, 2, 3],
            width: 4,
            height: 4,
        })
    }

    fn math(&mut self, _tex: &str, _display: bool) -> Option<Picture> {
        None
    }
}

fn walk(node: &Handle, out: &mut Vec<Handle>) {
    for child in node.children.borrow().iter() {
        out.push(child.clone());
        walk(child, out);
    }
}

fn tag_of(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

fn text_of(node: &Handle) -> String {
    let mut out = String::new();
    fn push(node: &Handle, out: &mut String) {
        match &node.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            _ => {
                for child in node.children.borrow().iter() {
                    push(child, out);
                }
            }
        }
    }
    push(node, &mut out);
    out
}

fn attr(node: &Handle, name: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| &*a.name.local == name)
            .map(|a| a.value.to_string()),
        _ => None,
    }
}

fn children_tags(node: &Handle) -> Vec<String> {
    node.children.borrow().iter().filter_map(tag_of).collect()
}

#[test]
fn headings_and_paragraphs_take_their_tags() {
    let html = html_of_markdown("# Title\n\n## Second\n\nA paragraph.\n\nAnother one.\n");
    assert_eq!(html.texts("h1"), ["Title"]);
    assert_eq!(html.texts("h2"), ["Second"]);
    assert_eq!(html.texts("p"), ["A paragraph.", "Another one."]);
}

#[test]
fn inline_styles_map_to_their_tags() {
    let html = html_of_markdown(
        "**bold** *italic* ~~struck~~ `code` <u>under</u> <mark>lit</mark> H~2~O x^2^ <small>tiny</small>\n",
    );
    assert_eq!(html.texts("strong"), ["bold"]);
    assert_eq!(html.texts("em"), ["italic"]);
    assert_eq!(html.texts("s"), ["struck"]);
    assert_eq!(html.texts("code"), ["code"]);
    assert_eq!(html.texts("u"), ["under"]);
    assert_eq!(html.texts("mark"), ["lit"]);
    assert_eq!(html.texts("sub"), ["2"]);
    assert_eq!(html.texts("sup"), ["2"]);
    assert_eq!(html.texts("small"), ["tiny"]);
    assert_eq!(
        html.whole_text(),
        "bold italic struck code under lit H2O x2 tiny"
    );
}

#[test]
fn inline_code_carries_its_face_and_background() {
    let html = html_of_markdown("a `word` here\n");
    let code = &html.elements("code")[0];
    let style = attr(code, "style").expect("a style");
    assert!(style.contains("monospace"), "{style}");
    assert!(style.contains("background"), "{style}");
}

#[test]
fn links_keep_a_web_address_and_drop_the_rest() {
    let html = html_of_markdown(
        "[web](https://example.com) [section](#top) [file](README.md) <someone@example.com>\n",
    );
    let links: Vec<(String, String)> = html
        .elements("a")
        .iter()
        .map(|a| (text_of(a), attr(a, "href").unwrap_or_default()))
        .collect();
    assert_eq!(
        links,
        [
            ("web".to_string(), "https://example.com".to_string()),
            (
                "someone@example.com".to_string(),
                "mailto:someone@example.com".to_string()
            ),
        ]
    );
    assert_eq!(html.whole_text(), "web section file someone@example.com");
}

#[test]
fn text_is_escaped() {
    let html = html_of_markdown("a < b & c > d \"quoted\"\n");
    assert!(html.contains("a &lt; b &amp; c &gt; d"), "{html}");
    assert_eq!(html.whole_text(), "a < b & c > d \u{201c}quoted\u{201d}");
}

#[test]
fn bullet_lists_nest_by_depth() {
    let html = html_of_markdown("- one\n- two\n  - inner\n    - deeper\n- three\n");
    let top = html.elements("ul");
    assert_eq!(top.len(), 3, "{html}");
    let outer = &top[0];
    let items = outer
        .children
        .borrow()
        .iter()
        .filter(|n| tag_of(n).as_deref() == Some("li"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 3, "{html}");
    assert_eq!(children_tags(&items[1]), ["ul"], "{html}");
    let inner = &items[1].children.borrow()[1];
    assert_eq!(text_of(inner).trim(), "innerdeeper");
    assert_eq!(text_of(&items[2]), "three");
}

#[test]
fn numbered_lists_start_where_the_source_does() {
    let html = html_of_markdown("7. seven\n8. eight\n\ntext\n\n1. one\n");
    let lists = html.elements("ol");
    assert_eq!(lists.len(), 2, "{html}");
    assert_eq!(attr(&lists[0], "start").as_deref(), Some("7"));
    assert_eq!(attr(&lists[1], "start"), None);
    assert_eq!(html.texts("li"), ["seven", "eight", "one"]);
}

#[test]
fn task_items_show_a_box() {
    let html = html_of_markdown("- [ ] open\n- [x] done\n");
    assert_eq!(html.texts("li"), ["\u{2610} open", "\u{2611} done"]);
    for li in html.elements("li") {
        assert!(attr(&li, "style")
            .unwrap_or_default()
            .contains("list-style"));
    }
}

#[test]
fn definitions_indent_under_their_term() {
    let html = html_of_markdown("Oryx\n: A viewer.\n: An editor.\n");
    assert_eq!(html.texts("p"), ["Oryx"]);
    assert_eq!(html.texts("strong"), ["Oryx"]);
    assert_eq!(html.texts("li"), ["A viewer.", "An editor."]);
    for li in html.elements("li") {
        assert!(attr(&li, "style")
            .unwrap_or_default()
            .contains("list-style"));
    }
}

#[test]
fn code_blocks_keep_their_lines_and_their_colors() {
    let mut doc = markdown::parse("```rust\nfn main() {\n    let x = 1;\n}\n```\n");
    let source = doc.source.clone();
    if let BlockKind::CodeBlock {
        lines, highlights, ..
    } = &mut doc.blocks[0].kind
    {
        *highlights = highlight::spans(&source, lines, Some("rust"));
    } else {
        panic!("a code block");
    }
    let html = html_of(&doc);
    let pre = &html.elements("pre")[0];
    assert_eq!(text_of(pre), "fn main() {\n    let x = 1;\n}");
    let style = attr(pre, "style").expect("a style");
    assert!(style.contains("monospace"), "{style}");
    assert!(style.contains("background"), "{style}");
    let theme = Theme::default_dark();
    let keyword = oryx::style::theme::hex_string(theme.syntax.keyword).to_ascii_lowercase();
    let colored: Vec<(String, String)> = html
        .elements("span")
        .iter()
        .map(|s| {
            (
                text_of(s),
                attr(s, "style").unwrap_or_default().to_ascii_lowercase(),
            )
        })
        .collect();
    assert!(
        colored
            .iter()
            .any(|(text, style)| text == "fn" && style.contains(&keyword)),
        "{colored:?}"
    );
}

#[test]
fn tables_have_a_header_row_and_bordered_cells() {
    let html = html_of_markdown("| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n");
    assert_eq!(html.texts("th"), ["A", "B"]);
    assert_eq!(html.texts("td"), ["1", "2", "3", "4"]);
    assert_eq!(html.elements("tr").len(), 3);
    assert_eq!(html.elements("thead").len(), 1);
    for cell in html.elements("td") {
        assert!(attr(&cell, "style").unwrap_or_default().contains("border"));
    }
    let table = &html.elements("table")[0];
    assert!(attr(table, "style")
        .unwrap_or_default()
        .contains("border-collapse"));
}

/// Each cell of a marked column says its alignment, the header's too; a
/// column without a mark says nothing, and its header stays at the left.
#[test]
fn table_cells_carry_the_alignment_of_their_column() {
    let html = html_of_markdown("| L | C | R | P |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n");
    let cells: Vec<String> = html
        .elements("td")
        .iter()
        .map(|c| attr(c, "style").unwrap_or_default())
        .collect();
    assert!(cells[0].contains("text-align:left"), "{}", cells[0]);
    assert!(cells[1].contains("text-align:center"), "{}", cells[1]);
    assert!(cells[2].contains("text-align:right"), "{}", cells[2]);
    assert!(!cells[3].contains("text-align"), "{}", cells[3]);
    let heads: Vec<String> = html
        .elements("th")
        .iter()
        .map(|c| attr(c, "style").unwrap_or_default())
        .collect();
    assert!(heads[1].contains("text-align:center"), "{}", heads[1]);
    assert!(heads[2].contains("text-align:right"), "{}", heads[2]);
    assert!(heads[3].contains("text-align:left"), "{}", heads[3]);
}

#[test]
fn a_table_without_a_header_has_no_thead() {
    let html = html_of_markdown("<table><tr><td>a</td><td>b</td></tr></table>\n");
    assert_eq!(html.elements("thead").len(), 0);
    assert_eq!(html.texts("td"), ["a", "b"]);
}

#[test]
fn quotes_nest_in_blockquotes_with_a_bar() {
    let html = html_of_markdown("> outer\n>\n> > inner\n\nafter\n");
    let quotes = html.elements("blockquote");
    assert_eq!(quotes.len(), 2, "{html}");
    assert!(attr(&quotes[0], "style")
        .unwrap_or_default()
        .contains("border-left"));
    assert_eq!(children_tags(&quotes[0]), ["p", "blockquote"]);
    assert_eq!(text_of(&quotes[1]), "inner");
    assert_eq!(html.texts("p"), ["outer", "inner", "after"]);
}

#[test]
fn alerts_carry_their_title() {
    let html = html_of_markdown("> [!NOTE]\n> Mind this.\n\n> [!WARNING]\n> Careful.\n");
    let quotes = html.elements("blockquote");
    assert_eq!(quotes.len(), 2, "{html}");
    assert_eq!(html.texts("strong"), ["Note", "Warning"]);
    assert_eq!(
        html.texts("p"),
        ["Note", "Mind this.", "Warning", "Careful."]
    );
    let theme = Theme::default_dark();
    let note = oryx::style::theme::hex_string(theme.alerts.note).to_ascii_lowercase();
    assert!(attr(&quotes[0], "style")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains(&note));
}

#[test]
fn footnotes_gather_at_the_end() {
    let html = html_of_markdown(
        "A claim.[^a] More.\n\n[^a]: The note.\n\n    Its second paragraph.\n\nLast.\n",
    );
    assert_eq!(html.texts("sup"), ["1"]);
    assert_eq!(
        html.texts("p")[..3],
        ["A claim.1 More.", "Last.", "The note."]
    );
    let notes = html.elements("ol");
    assert_eq!(notes.len(), 1, "{html}");
    let items = html.texts("li");
    assert_eq!(items.len(), 1, "{html}");
    assert_eq!(items[0].trim(), "The note.Its second paragraph.");
    assert!(html.html.find("<hr").unwrap() < html.html.find("<ol").unwrap());
}

#[test]
fn footnotes_come_out_in_the_order_they_are_used() {
    // The second label is used first: it is note 1, and the page lists
    // it first, wherever the source defined it.
    let html =
        html_of_markdown("First,[^b] then.[^a]\n\n[^a]: Defined first.\n\n[^b]: Used first.\n");
    assert_eq!(html.texts("sup"), ["1", "2"]);
    assert_eq!(html.texts("li"), ["Used first.", "Defined first."]);
    let values: Vec<Option<String>> = html
        .elements("li")
        .iter()
        .map(|li| attr(li, "value"))
        .collect();
    assert_eq!(values, [Some("1".to_string()), Some("2".to_string())]);
}

#[test]
fn a_hard_break_becomes_a_line_break_tag() {
    let html = html_of_markdown("one  \ntwo\\\nthree\nfour\n");
    assert_eq!(html.elements("br").len(), 2, "{html}");
    assert_eq!(html.texts("p"), ["onetwothree four"]);
}

#[test]
fn abbreviations_carry_their_expansion() {
    let html = html_of_markdown("The W3C decides.\n\n*[W3C]: World Wide Web Consortium\n");
    let abbr = &html.elements("abbr")[0];
    assert_eq!(text_of(abbr), "W3C");
    assert_eq!(
        attr(abbr, "title").as_deref(),
        Some("World Wide Web Consortium")
    );
}

#[test]
fn rules_come_through_and_page_breaks_do_not() {
    let html = html_of_markdown(
        "above\n\n---\n\nbetween\n\n<div style=\"page-break-after: always\"></div>\n\nbelow\n",
    );
    assert_eq!(html.elements("hr").len(), 1);
    assert_eq!(html.texts("p"), ["above", "between", "below"]);
}

#[test]
fn alignment_travels_as_a_style() {
    let html = html_of_markdown(
        "<p align=\"center\">middle</p>\n\n<p align=\"right\">edge</p>\n\nplain\n",
    );
    let styles: Vec<Option<String>> = html
        .elements("p")
        .iter()
        .map(|p| attr(p, "style"))
        .collect();
    assert!(styles[0]
        .as_deref()
        .unwrap_or("")
        .contains("text-align:center"));
    assert!(styles[1]
        .as_deref()
        .unwrap_or("")
        .contains("text-align:right"));
    assert_eq!(styles[2], None);
}

#[test]
fn a_summary_reads_as_a_bold_line() {
    let html =
        html_of_markdown("<details open>\n<summary>Open me</summary>\n\nInside.\n\n</details>\n");
    assert_eq!(html.texts("strong"), ["Open me"]);
    assert_eq!(html.texts("p"), ["Open me", "Inside."]);
}

#[test]
fn frontmatter_becomes_a_small_table() {
    let html = html_of_markdown("---\ntitle: Notes\nauthor: Me\n---\n\nBody.\n");
    assert_eq!(html.texts("td"), ["title", "Notes", "author", "Me"]);
    assert_eq!(html.texts("p"), ["Body."]);
}

struct OnePixel;

impl Pictures for OnePixel {
    fn image(&mut self, src: &str) -> Option<Picture> {
        (src != "missing.png").then(|| Picture {
            mime: "image/png",
            bytes: vec![0x89, b'P', b'N', b'G'],
            width: 1,
            height: 1,
        })
    }

    fn math(&mut self, _tex: &str, _display: bool) -> Option<Picture> {
        Some(Picture {
            mime: "image/png",
            bytes: vec![1, 2, 3],
            width: 30,
            height: 12,
        })
    }
}

#[test]
fn images_embed_as_data_addresses() {
    let doc = markdown::parse(
        "![a picture](pic.png)\n\ninline <img src=\"pic.png\" width=\"48\"> here\n\n![gone](missing.png)\n",
    );
    let sel = selection::all(&doc).unwrap();
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut OnePixel,
    ));
    let images = html.elements("img");
    assert_eq!(images.len(), 2, "{html}");
    assert_eq!(
        attr(&images[0], "src").as_deref(),
        Some("data:image/png;base64,iVBORw==")
    );
    assert_eq!(attr(&images[0], "alt").as_deref(), Some("a picture"));
    assert_eq!(attr(&images[1], "width").as_deref(), Some("48"));
    assert_eq!(html.texts("p"), ["", "inline  here", "gone"]);
}

#[test]
fn math_becomes_a_picture_or_stays_text() {
    let doc = markdown::parse("Inline $x^2$ here.\n\n$$\n\\sum_{n=1}^{\\infty} n\n$$\n");
    let sel = selection::all(&doc).unwrap();
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut OnePixel,
    ));
    let images = html.elements("img");
    assert_eq!(images.len(), 2, "{html}");
    assert_eq!(attr(&images[0], "alt").as_deref(), Some("x^2"));
    assert!(attr(&images[0], "style")
        .unwrap_or_default()
        .contains("vertical-align"));
    assert_eq!(attr(&images[1], "width").as_deref(), Some("30"));

    // Without a picture the copy reads as the plain one does: the
    // formula as typed inside a sentence, its plain reading as a block.
    let plain = html_of(&doc);
    assert_eq!(plain.elements("img").len(), 0);
    assert_eq!(
        plain.texts("p"),
        [
            "Inline x^2 here.".to_string(),
            oryx::layout::math_display("\\sum_{n=1}^{\\infty} n")
        ]
    );
}

/// The plain reading of a display formula shows an escaped reserved
/// character as the page does.
#[test]
fn the_copy_of_a_display_formula_reads_its_escaped_characters() {
    let doc = markdown::parse("$$\n15\\% \\# x\\_1\n$$\n");
    let plain = html_of(&doc);
    assert_eq!(plain.texts("p"), ["15% # x_1".to_string()]);
}

/// The plain reading of a display formula takes a control word as a
/// script whole (claim 10 of the hunt of Task 328).
#[test]
fn the_copy_of_a_display_formula_reads_a_control_word_as_its_script() {
    let doc = markdown::parse("$$\n\\int_0^\\infty e^\\pi\n$$\n");
    let plain = html_of(&doc);
    assert_eq!(
        plain.texts("p"),
        ["\u{222B}0\u{221E} e\u{03C0}".to_string()]
    );
}

#[test]
fn a_cut_selection_slices_only_the_end_blocks() {
    let doc = markdown::parse("first paragraph\n\nmiddle one\n\nlast paragraph\n");
    let sel = Selection {
        start: ModelPos {
            block: 0,
            span: 0,
            byte: 6,
        },
        end: ModelPos {
            block: 2,
            span: 0,
            byte: 4,
        },
    };
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut NoPictures,
    ));
    assert_eq!(html.texts("p"), ["paragraph", "middle one", "last"]);
    assert_eq!(
        selection::plain_text(&sel, &doc),
        "paragraph\n\nmiddle one\n\nlast"
    );
}

#[test]
fn a_cut_inside_a_code_block_keeps_the_selected_lines() {
    let doc = markdown::parse("```\none\ntwo\nthree\nfour\n```\n");
    let sel = Selection {
        start: ModelPos {
            block: 0,
            span: 1,
            byte: 1,
        },
        end: ModelPos {
            block: 0,
            span: 2,
            byte: 3,
        },
    };
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut NoPictures,
    ));
    assert_eq!(html.texts("pre"), ["wo\nthr"]);
    assert_eq!(selection::plain_text(&sel, &doc), "wo\nthr");
}

#[test]
fn a_cut_inside_a_table_keeps_the_rows_touched() {
    let doc = markdown::parse("| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n");
    // From cell "2" (chain 3) to cell "3" (chain 4).
    let sel = Selection {
        start: ModelPos {
            block: 0,
            span: 3,
            byte: 0,
        },
        end: ModelPos {
            block: 0,
            span: 4,
            byte: 1,
        },
    };
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut NoPictures,
    ));
    assert_eq!(html.elements("tr").len(), 2, "{html}");
    assert_eq!(html.texts("td"), ["", "2", "3", ""]);
    assert_eq!(html.elements("th").len(), 0);
}

#[test]
fn an_empty_selection_gives_nothing() {
    let doc = markdown::parse("text\n");
    let pos = ModelPos {
        block: 0,
        span: 0,
        byte: 2,
    };
    let sel = Selection {
        start: pos,
        end: pos,
    };
    assert_eq!(
        selection_html(&sel, &doc, &Theme::default_dark(), &mut NoPictures),
        ""
    );
}

#[test]
fn a_code_file_copies_as_one_code_box() {
    let text = "def f():\n    return 1\n";
    let doc = Document {
        blocks: vec![Block::plain(BlockKind::CodeBlock {
            language: Some("py".to_string()),
            lines: CodeBody::from_text(text),
            highlights: Vec::new(),
            exact: 0,
        })],
        source: text.into(),
        code_file: true,
        ..Document::default()
    };
    let html = html_of(&doc);
    assert_eq!(html.texts("pre"), ["def f():\n    return 1"]);
}

#[test]
fn every_word_of_the_fixture_reaches_the_html() {
    let source = std::fs::read_to_string("tests/fixtures/copy-html.md").unwrap();
    let doc = markdown::parse(source);
    let sel = selection::all(&doc).unwrap();
    let plain = selection::plain_text(&sel, &doc);
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut NoPictures,
    ));
    let text = html.whole_text();
    // A footnote's label is the plain copy's own ("2." before the
    // definition); the HTML numbers the list. A frontmatter key reads
    // "title:" in the plain copy and sits in a cell of its own here.
    let mut missing = Vec::new();
    for word in plain.split_whitespace() {
        let word = word.trim_end_matches(':');
        let label =
            word.ends_with('.') && word[..word.len() - 1].bytes().all(|b| b.is_ascii_digit());
        if !label && !text.contains(word) {
            missing.push(word);
        }
    }
    assert!(missing.is_empty(), "missing from the HTML: {missing:?}");
}

fn page_pictures<'a>(
    theme: &'a Theme,
    cfg: &'a oryx::layout::ViewConfig,
    fonts: &'a mut oryx::style::fonts::FontStore,
    media: &'a mut oryx::doc::images::MediaCache,
) -> oryx::export::html::PagePictures<'a> {
    oryx::export::html::PagePictures {
        theme,
        cfg,
        fonts,
        media,
    }
}

#[test]
fn a_formula_paints_as_a_transparent_picture() {
    let theme = Theme::default_dark();
    let cfg = oryx::layout::ViewConfig::default();
    let mut fonts = oryx::style::fonts::FontStore::new();
    let mut media = oryx::doc::images::MediaCache::offline("tests/fixtures".into());
    let mut pictures = page_pictures(&theme, &cfg, &mut fonts, &mut media);
    let picture = pictures.math("x^2 + y^2", false).expect("a picture");
    assert_eq!(picture.mime, "image/png");
    let decoded = image::load_from_memory(&picture.bytes)
        .expect("a PNG")
        .into_rgba8();
    // Shown at half the painted size, and never empty.
    assert!(picture.width > 0 && picture.height > 0);
    assert_eq!(picture.width, decoded.width().div_ceil(2));
    assert_eq!(picture.height, decoded.height().div_ceil(2));
    // The paper is gone: the corners are clear, the ink is there in
    // the page's foreground, the color formulas are set in.
    assert_eq!(decoded.get_pixel(0, 0)[3], 0);
    let ink = theme.surface.foreground;
    assert!(decoded
        .pixels()
        .any(|p| p[3] == 255 && p[0] == ink.r && p[1] == ink.g && p[2] == ink.b));
    // A block formula is taller than the same one inline: display
    // style sets the limits over and under the big operators.
    let inline = pictures.math("\\sum_{n=1}^{\\infty} n", false).unwrap();
    let block = pictures.math("\\sum_{n=1}^{\\infty} n", true).unwrap();
    assert!(
        block.height > inline.height,
        "{} vs {}",
        block.height,
        inline.height
    );
}

#[test]
fn a_local_picture_travels_as_its_own_bytes() {
    let theme = Theme::default_dark();
    let cfg = oryx::layout::ViewConfig::default();
    let mut fonts = oryx::style::fonts::FontStore::new();
    let mut media = oryx::doc::images::MediaCache::offline("tests/fixtures".into());
    let mut pictures = page_pictures(&theme, &cfg, &mut fonts, &mut media);
    let picture = pictures
        .image("../../examples/oryx-test.png")
        .expect("a picture");
    assert_eq!(picture.mime, "image/png");
    assert_eq!(
        picture.bytes,
        std::fs::read("examples/oryx-test.png").unwrap()
    );
    assert!(pictures.image("missing.png").is_none());
}

#[test]
fn a_blank_line_inside_a_selected_code_block_is_kept() {
    let html = html_of_markdown("```\none\n\nthree\n```\n");
    assert_eq!(html.texts("pre"), ["one\n\nthree"]);
}

#[test]
fn a_picture_shown_twice_is_asked_for_once() {
    let doc = markdown::parse("![one](pic.png)\n\ntext\n\n![two](pic.png)\n");
    let sel = selection::all(&doc).expect("something to select");
    let mut pictures = Counting { asked: Vec::new() };
    let html = Page::parse(selection_html(
        &sel,
        &doc,
        &Theme::default_dark(),
        &mut pictures,
    ));
    assert_eq!(html.elements("img").len(), 2);
    assert_eq!(pictures.asked, ["pic.png"]);
}

#[test]
fn a_nested_alert_marks_the_inner_quote() {
    let html = html_of_markdown("> > [!NOTE]\n> > careful\n");
    let quotes = html.elements("blockquote");
    assert_eq!(quotes.len(), 2, "{html}");
    assert_eq!(
        children_tags(&quotes[0]),
        ["blockquote"],
        "the outer quote holds only the inner one: {html}"
    );
    assert_eq!(html.texts("strong"), ["Note"]);
}
