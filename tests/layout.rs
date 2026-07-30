use std::path::PathBuf;
use std::time::{Duration, Instant};

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::markdown;
use oryx::doc::model::{BlockKind, Document, SpanScript};
use oryx::doc::stream::{self, Swap};
use oryx::layout::{
    layout, layout_begin, layout_extend, layout_more, layout_step, metrics, recolor_batch,
    recolor_code_lines, window_to, LayoutDoc, ShapePool, TextRun, ViewConfig,
};
use oryx::style::fonts::{FontStore, CODE_FAMILY};
use oryx::style::highlight::{self, Arrival};
use oryx::style::theme::Theme;

fn fonts() -> FontStore {
    FontStore::new()
}

fn cfg() -> ViewConfig {
    ViewConfig::default()
}

fn lay_doc(doc: &Document, width: f32, fonts: &mut FontStore) -> LayoutDoc {
    let mut media = MediaCache::new(PathBuf::from("."));
    layout(
        doc,
        &Theme::default_dark(),
        fonts,
        &mut media,
        &cfg(),
        width,
    )
}

fn lay(source: &str, width: f32) -> LayoutDoc {
    lay_doc(&markdown::parse(source), width, &mut fonts())
}

/// As `lay`, keeping the document so tests can read run text, family
/// and link through the accessors.
fn lay2(source: &str, width: f32) -> (Document, LayoutDoc) {
    let doc = markdown::parse(source);
    let l = lay_doc(&doc, width, &mut fonts());
    (doc, l)
}

fn find_text<'a>(l: &'a LayoutDoc, doc: &'a Document, text: &str) -> &'a TextRun {
    l.runs
        .iter()
        .find(|r| l.run_text(doc, r) == text)
        .unwrap_or_else(|| panic!("no run with text {text:?}"))
}

/// Folds full highlights into every code block, as the budget pass or
/// the worker would.
fn highlight_all(doc: &mut Document) {
    let source = std::sync::Arc::clone(&doc.source);
    for i in 0..doc.blocks.len() {
        let BlockKind::CodeBlock {
            language, lines, ..
        } = &doc.blocks[i].kind
        else {
            continue;
        };
        let spans = highlight::spans(&source, lines, language.as_deref());
        load::fold(
            doc,
            &Arrival {
                block: i,
                start_line: 0,
                spans,
            },
        );
    }
}

fn body_run(l: &LayoutDoc) -> &TextRun {
    l.runs.iter().find(|r| r.size == 22.0).expect("no body run")
}

#[test]
fn h1_larger_and_spaced() {
    let l = lay("# Title\n\nBody text", 800.0);
    let title = &l.runs[0];
    assert_eq!(title.size, 44.0);
    assert_eq!(title.weight, 700);
    let body = body_run(&l);
    assert!(title.y < body.y);
}

#[test]
fn wrapping_respects_margins() {
    let l = lay(&"word ".repeat(200), 500.0);
    assert!(l.runs.len() > 1, "expected wrapped lines");
    for r in &l.runs {
        assert!(r.x >= 40.0 - 0.5, "run starts left of margin: {}", r.x);
        assert!(
            r.x + r.width <= 460.0 + 0.5,
            "run exceeds right margin: {}",
            r.x + r.width
        );
    }
}

#[test]
fn empty_document_has_zero_height() {
    let l = lay("", 800.0);
    assert_eq!(l.height, 0.0);
    assert!(l.runs.is_empty());
}

#[test]
fn zoom_doubles_sizes_and_grows_height() {
    let source = "# Title\n\nSome body text here";
    let normal = lay(source, 800.0);
    let doc = markdown::parse(source);
    let mut config = cfg();
    config.zoom = 2.0;
    let mut media = MediaCache::new(PathBuf::from("."));
    let zoomed = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts(),
        &mut media,
        &config,
        800.0,
    );
    assert_eq!(zoomed.runs[0].size, 88.0);
    assert!(zoomed.height > normal.height);
}

#[test]
fn paragraphs_separated_lines_flush() {
    let l = lay(
        "alpha beta gamma delta epsilon zeta eta theta\n\nsecond paragraph",
        320.0,
    );
    let tops: Vec<f32> = l.runs.iter().map(|r| r.y).collect();
    let mut gaps: Vec<f32> = Vec::new();
    for pair in tops.windows(2) {
        if pair[1] > pair[0] {
            gaps.push(pair[1] - pair[0]);
        }
    }
    assert!(gaps.len() >= 2, "expected several line transitions");
    let line_height = 22.0 * 1.5;
    let flush = gaps
        .iter()
        .filter(|g| (**g - line_height).abs() < 0.5)
        .count();
    let separated = gaps.iter().filter(|g| **g > line_height + 1.0).count();
    assert!(flush >= 1, "no flush line transition found in {gaps:?}");
    assert_eq!(separated, 1, "expected one paragraph gap in {gaps:?}");
}

#[test]
fn anchors_carry_heading_positions() {
    let l = lay("# One\n\ntext\n\n## Two More\n\ntext", 800.0);
    assert_eq!(l.anchors.len(), 2);
    assert_eq!(l.anchors[0].0, "one");
    assert_eq!(l.anchors[1].0, "two-more");
    assert!(l.anchors[1].1 > l.anchors[0].1);
    let h2 = l
        .runs
        .iter()
        .find(|r| (r.size - 22.0 * 1.6).abs() < 0.01)
        .unwrap();
    assert!((l.anchors[1].1 - h2.y).abs() < 0.5);
}

#[test]
fn inline_code_uses_code_font() {
    let (doc, l) = lay2("body `mono` body", 800.0);
    let code = l
        .runs
        .iter()
        .find(|r| l.run_family(r) == CODE_FAMILY)
        .unwrap();
    assert_eq!(l.run_text(&doc, code), "mono");
    assert_eq!(code.size, 20.0);
}

#[test]
fn link_color_and_target() {
    let (doc, l) = lay2("[click](https://a.tld)", 800.0);
    let t = Theme::default_dark();
    let link = l
        .runs
        .iter()
        .find(|r| l.run_link(&doc, r).is_some())
        .unwrap();
    assert_eq!(link.color, t.text.link);
    assert_eq!(l.run_link(&doc, link), Some("https://a.tld"));
}

#[test]
fn code_block_panel_and_highlighting() {
    let mut doc = markdown::parse("```rust\nfn main() {\n    let s = \"hi\";\n}\n```");
    highlight_all(&mut doc);
    let l = lay_doc(&doc, 800.0, &mut fonts());
    let t = Theme::default_dark();
    assert!(l.rects.iter().any(|r| r.color == t.blocks.code_bg));
    assert!(l.rects.iter().any(|r| r.color == t.blocks.code_border));
    let kw = l
        .runs
        .iter()
        .find(|r| r.color == t.syntax.keyword)
        .expect("keyword-colored run");
    assert_eq!(l.run_family(kw), CODE_FAMILY);
    assert!(l.runs.iter().any(|r| r.color == t.syntax.string));
    let rows: std::collections::BTreeSet<i64> = l.runs.iter().map(|r| r.y as i64).collect();
    assert!(rows.len() >= 3, "one row per source line, got {rows:?}");
}

#[test]
fn unhighlighted_tail_renders_in_foreground() {
    let mut doc = markdown::parse("```rust\nfn main() {}\nlet x = 1;\n```");
    let source = std::sync::Arc::clone(&doc.source);
    let BlockKind::CodeBlock {
        language, lines, ..
    } = &doc.blocks[0].kind
    else {
        panic!("expected code block")
    };
    let spans = highlight::spans(&source, lines, language.as_deref());
    load::fold(
        &mut doc,
        &Arrival {
            block: 0,
            start_line: 0,
            spans: spans[0..1].to_vec(),
        },
    );
    let l = lay_doc(&doc, 800.0, &mut fonts());
    let t = Theme::default_dark();
    assert!(l.runs.iter().any(|r| r.color == t.syntax.keyword));
    let tail_y = l.runs.iter().map(|r| r.y as i64).max().unwrap();
    assert!(l
        .runs
        .iter()
        .filter(|r| r.y as i64 == tail_y)
        .all(|r| r.color == t.surface.foreground));
}

#[test]
fn recolor_in_place_matches_full_relayout() {
    let source = "# Title\n\nintro paragraph\n\n\
        ```rust\nfn main() {\n    let s = \"hi\";\n}\n```\n\ntail paragraph";
    let mut doc = markdown::parse(source);
    let mut store = fonts();
    let mut lazy = lay_doc(&doc, 800.0, &mut store);
    highlight_all(&mut doc);
    let block = doc
        .blocks
        .iter()
        .position(|b| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .unwrap();
    recolor_code_lines(
        &mut lazy,
        &doc,
        &Theme::default_dark(),
        &mut store,
        &cfg(),
        block,
        0..3,
    );
    let full = lay_doc(&doc, 800.0, &mut store);
    assert_eq!(lazy.runs, full.runs);
    assert_eq!(lazy.height, full.height);
}

#[test]
fn recolor_in_chunks_matches_full_relayout() {
    let mut source = String::from("```rust\n");
    for i in 0..11 {
        source.push_str(&format!("let value_{i} = {i}; // note {i}\n"));
    }
    source.push_str(&format!(
        "let long = \"{}\"; // wraps\n```",
        "x".repeat(200)
    ));
    let mut doc = markdown::parse(source.as_str());
    let mut store = fonts();
    let mut lazy = lay_doc(&doc, 500.0, &mut store);
    highlight_all(&mut doc);
    let theme = Theme::default_dark();
    recolor_code_lines(&mut lazy, &doc, &theme, &mut store, &cfg(), 0, 0..5);
    recolor_code_lines(&mut lazy, &doc, &theme, &mut store, &cfg(), 0, 5..12);
    let full = lay_doc(&doc, 500.0, &mut store);
    assert_eq!(lazy.runs, full.runs);
}

#[test]
fn code_lines_wrap_inside_the_panel() {
    let src = "```rust\n// this comment is long enough that it must wrap into more than one visual line at a narrow width\nlet x = 1;\n```";
    let (doc, l) = lay2(src, 420.0);
    let t = Theme::default_dark();
    let panel = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.code_bg)
        .unwrap();
    let right = panel.x + panel.width;
    for r in l.runs.iter().filter(|r| l.run_family(r) == CODE_FAMILY) {
        assert!(
            r.x + r.width <= right + 0.5,
            "run overflows the panel: {:?}",
            l.run_text(&doc, r)
        );
    }
    let rows: std::collections::BTreeSet<i64> = l.runs.iter().map(|r| r.y as i64).collect();
    assert!(rows.len() >= 3, "expected wrapped rows, got {rows:?}");
    let bottom = l
        .runs
        .iter()
        .map(|r| r.y + 1.5 * r.size)
        .fold(0.0, f32::max);
    assert!(
        panel.y + panel.height >= bottom - 0.5,
        "panel covers the wrapped lines"
    );
}

#[test]
fn inline_code_gets_pill() {
    let l = lay("with `mono` inside", 800.0);
    let t = Theme::default_dark();
    let run = l
        .runs
        .iter()
        .find(|r| l.run_family(r) == CODE_FAMILY)
        .unwrap();
    let pill = l
        .rects
        .iter()
        .find(|r| r.color == t.text.inline_code_bg)
        .expect("pill rect");
    assert!(pill.x <= run.x && pill.x + pill.width >= run.x + run.width);
}

#[test]
fn quote_indents_with_bar_and_panel() {
    let plain = lay("text", 800.0);
    let quoted = lay("> text", 800.0);
    let deep = lay("> > text", 800.0);
    let t = Theme::default_dark();
    let px = plain.runs[0].x;
    assert!(quoted.runs[0].x >= px + 24.0);
    assert!(deep.runs[0].x >= px + 48.0);
    assert!(quoted.rects.iter().any(|r| r.color == t.blocks.quote_bar));
    assert!(quoted.rects.iter().any(|r| r.color == t.blocks.quote_bg));
    assert_eq!(
        deep.rects
            .iter()
            .filter(|r| r.color == t.blocks.quote_bar)
            .count(),
        2
    );
}

#[test]
fn task_items_draw_checkboxes() {
    let l = lay("- [x] done\n- [ ] todo", 800.0);
    let t = Theme::default_dark();
    assert!(
        l.rects.iter().any(|r| r.color == t.text.link),
        "checked box fill"
    );
    assert!(
        l.rects
            .iter()
            .any(|r| r.color == t.blocks.rule && r.stroke > 0.0),
        "unchecked box outline"
    );
}

#[test]
fn ordered_markers_number_text() {
    let (doc, l) = lay2("1. one\n2. two\n3. three", 800.0);
    assert!(l.runs.iter().any(|r| l.run_text(&doc, r) == "3."));
    let one = find_text(&l, &doc, "one");
    let marker = find_text(&l, &doc, "1.");
    assert!(marker.x < one.x);
}

#[test]
fn nested_items_indent_deeper() {
    let (doc, l) = lay2("- outer\n  - inner", 800.0);
    let outer = find_text(&l, &doc, "outer");
    let inner = find_text(&l, &doc, "inner");
    assert!(inner.x >= outer.x + 24.0 - 0.5);
}

#[test]
fn rule_spans_content_width() {
    let l = lay("above\n\n***\n\nbelow", 800.0);
    let t = Theme::default_dark();
    let rule = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.rule)
        .expect("rule rect");
    assert!((rule.width - 672.0).abs() < 1.0);
}

#[test]
fn closed_details_fold_and_reopen_exactly() {
    let src_closed = "<details>\n<summary>More</summary>\n\n### Hidden Head\n\n\
                      Hidden body text.\n\n</details>\n\nAfter.";
    let mut doc = markdown::parse(src_closed);
    let mut fs = fonts();
    let closed = lay_doc(&doc, 800.0, &mut fs);
    let all: String = closed
        .runs
        .iter()
        .map(|r| closed.run_text(&doc, r))
        .collect();
    assert!(all.contains("More") && all.contains("After."));
    assert!(!all.contains("Hidden"), "closed content emits nothing");
    find_text(&closed, &doc, "\u{25B8}");
    assert!(
        closed.anchors.iter().all(|(a, _)| a != "hidden-head"),
        "a folded heading records no anchor"
    );

    doc.toggle_details(0);
    let reopened = lay_doc(&doc, 800.0, &mut fs);
    let twin = markdown::parse(&*src_closed.replace("<details>", "<details open>"));
    let open = lay_doc(&twin, 800.0, &mut fs);
    assert_eq!(reopened.runs.len(), open.runs.len());
    assert_eq!(reopened.rects.len(), open.rects.len());
    assert!((reopened.height - open.height).abs() < 0.5);
    assert!(reopened.height > closed.height);
    find_text(&reopened, &doc, "\u{25BE}");
    assert!(
        reopened.anchors.iter().any(|(a, _)| a == "hidden-head"),
        "reopening places the anchor"
    );
}

#[test]
fn headerless_html_table_draws_no_header_band() {
    let (doc, l) = lay2(
        "<table><tr><td>alpha</td><td>beta</td></tr>\
         <tr><td>gamma</td><td>delta</td></tr></table>",
        800.0,
    );
    let t = Theme::default_dark();
    assert!(
        l.rects.iter().all(|r| r.color != t.blocks.table_header_bg),
        "no header band"
    );
    assert!(
        l.rects.iter().any(|r| r.color == t.blocks.table_row_alt_bg),
        "second body row keeps its stripe"
    );
    let alpha = find_text(&l, &doc, "alpha");
    let beta = find_text(&l, &doc, "beta");
    assert!(alpha.x < beta.x, "columns increase");
    assert_eq!(alpha.weight, 400, "first row is body, not header");
}

#[test]
fn table_columns_header_and_stripes() {
    let (doc, l) = lay2("|alpha|beta|gamma|\n|-|-|-|\n|a|b|c|\n|d|e|f|", 800.0);
    let t = Theme::default_dark();
    let alpha = find_text(&l, &doc, "alpha");
    let beta = find_text(&l, &doc, "beta");
    let gamma = find_text(&l, &doc, "gamma");
    assert!(alpha.x < beta.x && beta.x < gamma.x, "columns increase");
    assert_eq!(alpha.weight, 700, "header bold");
    assert!(l.rects.iter().any(|r| r.color == t.blocks.table_header_bg));
    assert!(l.rects.iter().any(|r| r.color == t.blocks.table_row_alt_bg));
    assert!(
        l.rects
            .iter()
            .filter(|r| r.color == t.blocks.table_border)
            .count()
            >= 5,
        "grid lines"
    );
    let outline = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.table_border && r.stroke > 0.0)
        .expect("rounded outline");
    assert!(outline.radius_top > 0.0 && outline.radius_bottom > 0.0);
    let header_bg = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.table_header_bg)
        .unwrap();
    assert!(header_bg.radius_top > 0.0, "header stripe rounds on top");
    let a = find_text(&l, &doc, "a");
    assert!(a.y > alpha.y, "body row below header");
    assert!((a.x - alpha.x).abs() < 1.0, "same column aligns");
}

#[test]
fn table_cells_wrap_within_capped_columns() {
    let (doc, l) = lay2(
        "|short|this long cell has quite a lot of text and must wrap|\n|-|-|\n|x|y|",
        500.0,
    );
    for r in &l.runs {
        assert!(
            r.x + r.width <= 460.5,
            "run exceeds table bounds: {} {}",
            l.run_text(&doc, r),
            r.x + r.width
        );
    }
    let header_rows: std::collections::BTreeSet<i64> = l
        .runs
        .iter()
        .filter(|r| r.weight == 700)
        .map(|r| r.y as i64)
        .collect();
    assert!(
        header_rows.len() >= 2,
        "long header cell should wrap onto multiple lines: {header_rows:?}"
    );
    let header_cells = l.runs.iter().filter(|r| r.weight == 700).count();
    assert!(header_cells >= 2, "both header cells present");
}

#[test]
fn compact_table_stays_narrow() {
    let l = lay("|a|b|\n|-|-|\n|1|2|", 800.0);
    let t = Theme::default_dark();
    let outline = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.table_border && r.stroke > 0.0)
        .unwrap();
    assert!(
        outline.width < 250.0,
        "tiny table should not stretch: {}",
        outline.width
    );
}

#[test]
fn dominant_column_grows_into_leftover() {
    let long = "one dominant column with a long descriptive sentence that wants space";
    let l = lay(&format!("|id|note|\n|-|-|\n|1|{long}|"), 800.0);
    let t = Theme::default_dark();
    let outline = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.table_border && r.stroke > 0.0)
        .unwrap();
    assert!(
        outline.width > 672.0 * 0.8,
        "table with one wordy column should use most of the width: {}",
        outline.width
    );
}

fn lay_with_images(source: &str, width: f32, dir: PathBuf) -> LayoutDoc {
    let doc = markdown::parse(source);
    let mut media = MediaCache::new(dir);
    layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts(),
        &mut media,
        &cfg(),
        width,
    )
}

#[test]
fn oversized_image_scales_to_content_width() {
    let dir = std::env::temp_dir().join(format!("oryx-laytest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(2000, 1000, image::Rgba([10, 20, 30, 255]));
    img.save(dir.join("wide.png")).unwrap();
    let l = lay_with_images("![big](wide.png)", 800.0, dir);
    let place = &l.images[0];
    assert!((place.width - 672.0).abs() < 0.5, "fits content width");
    assert!(
        (place.height - 336.0).abs() < 0.5,
        "aspect preserved: {}",
        place.height
    );
}

#[test]
fn small_image_keeps_natural_size() {
    let dir = std::env::temp_dir().join(format!("oryx-laytest2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(100, 50, image::Rgba([10, 20, 30, 255]));
    img.save(dir.join("small.png")).unwrap();
    let l = lay_with_images("![small](small.png)", 800.0, dir);
    assert!((l.images[0].width - 100.0).abs() < 0.5, "no upscaling");
}

#[test]
fn missing_image_becomes_placeholder() {
    let dir = std::env::temp_dir().join("oryx-laytest-none");
    std::fs::create_dir_all(&dir).unwrap();
    let doc = markdown::parse("![the alt text](gone.png)");
    let mut media = MediaCache::new(dir);
    let l = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts(),
        &mut media,
        &cfg(),
        800.0,
    );
    let t = Theme::default_dark();
    assert!(l.images.is_empty());
    assert!(
        l.rects
            .iter()
            .any(|r| r.color == t.blocks.code_border && r.stroke > 0.0),
        "placeholder outline"
    );
    assert!(
        l.runs
            .iter()
            .any(|r| l.run_text(&doc, r).contains("the alt text")),
        "alt text shown"
    );
}

#[test]
fn strike_emits_line_rect() {
    let l = lay("~~gone~~", 800.0);
    let run = &l.runs[0];
    let rect = l
        .rects
        .iter()
        .find(|r| r.y > run.y && r.y < run.y + 33.0)
        .expect("no strike rect");
    assert!((rect.width - run.width).abs() < 1.0);
}

#[test]
fn link_at_returns_target_inside_run() {
    let (doc, l) = lay2("intro [click here](https://a.tld) outro", 800.0);
    let link = l
        .runs
        .iter()
        .find(|r| l.run_link(&doc, r).is_some())
        .unwrap();
    let hit = l.link_at(&doc, link.x + link.width / 2.0, link.y + link.size / 2.0);
    assert_eq!(hit, Some("https://a.tld"));
}

#[test]
fn link_at_returns_none_outside_links() {
    let (doc, l) = lay2("intro [click here](https://a.tld) outro", 800.0);
    let plain = l
        .runs
        .iter()
        .find(|r| l.run_link(&doc, r).is_none())
        .unwrap();
    assert_eq!(l.link_at(&doc, plain.x + 1.0, plain.y + 1.0), None);
    assert_eq!(l.link_at(&doc, -10.0, -10.0), None);
}

#[test]
fn anchor_target_resolves_to_heading_y() {
    let l = lay("# One\n\n[jump](#two-more)\n\n## Two More\n\ntext", 800.0);
    let heading_y = l.anchors.iter().find(|(s, _)| s == "two-more").unwrap().1;
    assert_eq!(l.anchor_y("#two-more"), Some(heading_y));
    assert_eq!(l.anchor_y("#absent"), None);
}

#[test]
fn consecutive_quoted_blocks_tile_without_gap() {
    let t = Theme::default_dark();
    for source in ["> one\n>\n> two", "> [!CAUTION]\n> one\n>\n> two"] {
        let l = lay(source, 800.0);
        let mut panels: Vec<_> = l
            .rects
            .iter()
            .filter(|r| r.color == t.blocks.quote_bg)
            .collect();
        panels.sort_by(|a, b| a.y.total_cmp(&b.y));
        assert_eq!(panels.len(), 2, "{source}: expected two quote panels");
        assert!(
            panels[0].y + panels[0].height >= panels[1].y - 0.01,
            "{source}: gap between panels: first ends {}, second starts {}",
            panels[0].y + panels[0].height,
            panels[1].y
        );
    }
}

#[test]
fn badge_row_centers_and_shares_a_line() {
    let l = lay(
        "<p align=\"center\"><img src=\"a.png\" width=\"40\" height=\"20\"> <img src=\"b.png\" width=\"40\" height=\"20\"></p>",
        800.0,
    );
    assert_eq!(l.images.len(), 2, "two inline images placed");
    assert_eq!(l.images[0].y, l.images[1].y, "badges share a row");
    let left = l.images[0].x;
    let right = l.images[1].x + l.images[1].width;
    let mid = (left + right) / 2.0;
    assert!((mid - 400.0).abs() < 20.0, "row centered, mid {mid}");
}

#[test]
fn inline_badge_joins_the_text_line() {
    let (doc, l) = lay2(
        "coverage: <img src=\"c.png\" width=\"40\" height=\"20\">",
        800.0,
    );
    let text = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r).contains("coverage"))
        .unwrap();
    let img = &l.images[0];
    assert!(img.x >= text.x + text.width - 1.0, "badge after the text");
    assert!(
        img.y >= text.y - 1.0 && img.y + img.height <= text.y + 34.0,
        "badge inside the text line box"
    );
}

#[test]
fn linked_inline_image_is_clickable() {
    let (doc, l) = lay2(
        "<p align=\"center\"><a href=\"https://z.tld\"><img src=\"d.png\" width=\"40\" height=\"20\"></a></p>",
        800.0,
    );
    let img = &l.images[0];
    assert_eq!(
        l.link_at(&doc, img.x + 5.0, img.y + 5.0),
        Some("https://z.tld"),
        "image hit box carries the link"
    );
}

#[test]
fn footnote_reference_superscript_and_definitions_last() {
    let t = Theme::default_dark();
    // The definition sits mid-document; layout must still render it last.
    let (doc, l) = lay2("body text[^n]\n\n[^n]: the note itself\n\nmore", 800.0);
    let reference = l
        .runs
        .iter()
        .find(|r| l.run_link(&doc, r) == Some("footnote:n"))
        .expect("reference run");
    assert!(
        (reference.size - 22.0 * 0.7).abs() < 0.5,
        "superscript size"
    );
    assert_eq!(reference.color, t.text.link);
    let body = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r).contains("body"))
        .unwrap();
    assert!(
        reference.baseline < body.baseline - 2.0,
        "reference baseline raised"
    );
    let note = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r).contains("the note itself"))
        .expect("definition text");
    let more = find_text(&l, &doc, "more");
    assert!(note.y > more.y, "definitions collect at the end");
    let anchor = l.anchor_y("footnote:n").expect("footnote anchor");
    assert!((anchor - note.y).abs() < 60.0);
    assert!(
        l.rects
            .iter()
            .any(|r| r.color == t.blocks.rule && r.y > more.y && r.y < note.y),
        "rule above the definitions"
    );
}

#[test]
fn math_spans_style_and_scripts() {
    let t = Theme::default_dark();
    let (doc, l) = lay2("energy $E=mc^2$ inline", 800.0);
    let m = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r).contains("E=mc"))
        .expect("math run");
    assert_eq!(m.color, t.text.math);
    assert_eq!(l.run_family(m), CODE_FAMILY);
    assert!(m.italic);
    let sup = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r) == "2" && r.size < m.size)
        .expect("superscript run");
    assert!(sup.baseline < m.baseline - 1.0, "superscript raised");
    assert_eq!(sup.color, t.text.math);
}

#[test]
fn math_block_centers_in_panel() {
    let t = Theme::default_dark();
    let l = lay("$$E=mc^2$$", 800.0);
    let panel = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.code_bg)
        .expect("math panel");
    let min_x = l.runs.iter().map(|r| r.x).fold(f32::MAX, f32::min);
    let max_x = l.runs.iter().map(|r| r.x + r.width).fold(0.0, f32::max);
    let mid = (min_x + max_x) / 2.0;
    assert!((mid - 400.0).abs() < 30.0, "centered, mid={mid}");
    assert!(panel.width > max_x - min_x, "panel wraps the formula");
}

#[test]
fn quote_region_paints_without_seam() {
    let t = Theme::default_dark();
    // Fractional zooms move the panel junction across sub-pixel positions;
    // independent edge rounding used to leave an uncovered row at some of
    // them, so the whole sweep must paint solid.
    for zoom in [1.0, 1.03, 1.07, 1.13, 1.17, 1.23, 1.29] {
        let doc = markdown::parse("# Head\n\ntext\n\n> [!CAUTION]\n> one\n>\n> two paragraphs");
        let mut config = cfg();
        config.zoom = zoom;
        let mut media = MediaCache::new(PathBuf::from("."));
        let l = layout(&doc, &t, &mut fonts(), &mut media, &config, 800.0);
        let panels: Vec<_> = l
            .rects
            .iter()
            .filter(|r| r.color == t.blocks.quote_bg)
            .collect();
        assert_eq!(panels.len(), 2, "zoom {zoom}");
        let top = panels.iter().map(|r| r.y).fold(f32::MAX, f32::min);
        let bottom = panels.iter().map(|r| r.y + r.height).fold(0.0, f32::max);
        let pixels = oryx::paint::band(&l, &doc, &t, &mut fonts(), &mut media, &[], 0.0, 800, 900);
        let bg = t.surface.background;
        let bgpx = ((bg.r as u32) << 16) | ((bg.g as u32) << 8) | bg.b as u32;
        let x = (0.08 * 800.0) as usize + 30;
        for row in (top.ceil() as usize + 1)..(bottom.floor() as usize - 1) {
            assert_ne!(
                pixels[row * 800 + x],
                bgpx,
                "background shows through at zoom {zoom}, row {row}"
            );
        }
    }
}

#[test]
fn alert_titles_bold_and_colored_per_kind() {
    let theme = Theme::default_dark();
    for (tag, title_text, color) in [
        ("NOTE", "Note", theme.alerts.note),
        ("TIP", "Tip", theme.alerts.tip),
        ("IMPORTANT", "Important", theme.alerts.important),
        ("WARNING", "Warning", theme.alerts.warning),
        ("CAUTION", "Caution", theme.alerts.caution),
    ] {
        let (doc, l) = lay2(&format!("> [!{tag}]\n> Body here."), 800.0);
        let title = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r) == title_text)
            .unwrap_or_else(|| panic!("{tag}: no title run"));
        assert_eq!(title.weight, 700, "{tag}");
        assert_eq!(title.color, color, "{tag}");
        let body = l
            .runs
            .iter()
            .find(|r| l.run_text(&doc, r).contains("Body here"))
            .unwrap();
        assert!(title.y < body.y, "{tag}: title above the body");
        assert!(
            l.rects
                .iter()
                .any(|r| r.color == color && r.width <= 4.0 && r.height > 0.0),
            "{tag}: no bar in the alert color"
        );
    }
}

#[test]
fn frontmatter_panel_precedes_all_blocks() {
    let theme = Theme::default_dark();
    let (doc, l) = lay2("---\ntitle: Oryx\ntags: docs\n---\n\n# Head\n\nBody", 800.0);
    let panel = l
        .rects
        .iter()
        .find(|r| r.color == theme.blocks.frontmatter_bg)
        .expect("no frontmatter panel");
    let meta = l
        .runs
        .iter()
        .find(|r| l.run_text(&doc, r).contains("title: Oryx"))
        .expect("no metadata line");
    assert_eq!(meta.color, theme.blocks.frontmatter_fg);
    let heading = find_text(&l, &doc, "Head");
    assert!(panel.y < heading.y);
    assert!(panel.y + panel.height <= heading.y);
    assert!(meta.y < heading.y);
}

// A resumable pass must place exactly what one pass places, wherever the
// slice boundaries fall.

/// One block of every kind, so a boundary can land between any two of
/// them and every field carried across a boundary is exercised.
const ONE_OF_EACH: &str = r#"---
title: Sweep
tags: layout
---

# Heading

A paragraph with **bold**, *italic*, `code` and a [link](https://example.com).

> Quoted first.
>
> Quoted second.

> [!NOTE]
> An alert body.

- First item
- Second item
  - Nested item
- [ ] Task item

1. Ordered one
2. Ordered two

| Column | Other |
|---|---:|
| a | b |
| c | d |

```rust
fn main() {
    let x = 1;
    println!("{x}");
}
```

---

![missing](nope.png)

$$
x^2 + y_i
$$

Text with a footnote[^1].

[^1]: The definition.
"#;

/// Consecutive quoted blocks and an alert: the panel top is derived from
/// the previous block's trailing space and gap.
const QUOTE_REGION: &str = "Lead in.\n\n> Quoted first.\n>\n> Quoted second.\n\n\
     > [!TIP]\n> Alert body.\n\nTail.";

/// Consecutive list items take the tight gap, not the paragraph gap.
const TIGHT_LIST: &str = "- One\n- Two\n- Three\n\nAfter.";

/// Footnote definitions move to the end under a separator rule.
const FOOTNOTES: &str = "Body[^a] and more[^b].\n\n[^a]: First note.\n\n[^b]: Second note.";

/// A code block long enough that a boundary falls between its lines.
const CODE_LINES: &str =
    "Intro.\n\n```rust\nfn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n```\n\nOutro.";

fn theme() -> Theme {
    Theme::default_dark()
}

/// Runs `k` steps, then finishes the pass without a deadline.
fn lay_in_two(doc: &Document, width: f32, fonts: &mut FontStore, k: usize) -> LayoutDoc {
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(doc, &cfg(), width);
    for _ in 0..k {
        if layout_step(
            doc,
            &theme(),
            fonts,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        ) {
            break;
        }
    }
    layout_more(
        doc,
        &theme(),
        fonts,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        None,
    );
    out
}

/// Steps a complete pass takes, which is the number of boundaries to sweep.
fn step_count(doc: &Document, width: f32, fonts: &mut FontStore) -> usize {
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(doc, &cfg(), width);
    let mut steps = 1;
    while !layout_step(
        doc,
        &theme(),
        fonts,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
    ) {
        steps += 1;
    }
    steps
}

fn assert_same(split: &LayoutDoc, whole: &LayoutDoc, at: usize) {
    assert_eq!(split.height, whole.height, "height, boundary after {at}");
    assert_eq!(split.runs, whole.runs, "runs, boundary after {at}");
    assert_eq!(split.rects, whole.rects, "rects, boundary after {at}");
    assert_eq!(split.images, whole.images, "images, boundary after {at}");
    assert_eq!(split.anchors, whole.anchors, "anchors, boundary after {at}");
    assert_eq!(
        split.code_lines, whole.code_lines,
        "code lines, boundary after {at}"
    );
}

/// Every boundary in the document produces the same layout as one pass.
fn assert_sweep(source: &str) {
    let doc = markdown::parse(source);
    let mut fonts = fonts();
    let whole = lay_doc(&doc, 800.0, &mut fonts);
    let steps = step_count(&doc, 800.0, &mut fonts);
    // A sweep over a fixture that takes one step would pass while testing
    // nothing, so the step count is part of the assertion.
    assert!(steps > 2, "fixture sweeps only {steps} boundaries");
    for k in 0..=steps {
        let split = lay_in_two(&doc, 800.0, &mut fonts, k);
        assert_same(&split, &whole, k);
    }
}

#[test]
fn an_unbounded_pass_matches_layout() {
    let doc = markdown::parse(ONE_OF_EACH);
    let mut fonts = fonts();
    let whole = lay_doc(&doc, 800.0, &mut fonts);
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    let done = layout_more(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        None,
    );
    assert!(done, "an unbounded pass completes");
    assert_same(&out, &whole, 0);
}

#[test]
fn sweep_over_one_of_each() {
    assert_sweep(ONE_OF_EACH);
}

#[test]
fn sweep_over_a_quote_region() {
    assert_sweep(QUOTE_REGION);
}

#[test]
fn sweep_over_a_tight_list() {
    assert_sweep(TIGHT_LIST);
}

#[test]
fn sweep_over_footnotes() {
    assert_sweep(FOOTNOTES);
}

#[test]
fn sweep_over_code_lines() {
    assert_sweep(CODE_LINES);
}

#[test]
fn a_past_deadline_places_nothing() {
    let doc = markdown::parse(ONE_OF_EACH);
    let mut fonts = fonts();
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    let done = layout_more(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        Some(Instant::now() - Duration::from_millis(1)),
    );
    assert!(!done, "a past deadline cannot complete the document");
    assert_eq!(out.height, 0.0);
    assert!(out.runs.is_empty());
    assert!(out.rects.is_empty());
    assert!(out.anchors.is_empty());
}

#[test]
fn a_partial_pass_is_a_prefix_of_the_complete_one() {
    let doc = markdown::parse(QUOTE_REGION);
    let mut fonts = fonts();
    let whole = lay_doc(&doc, 800.0, &mut fonts);
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    for k in 0..step_count(&doc, 800.0, &mut fonts) {
        layout_step(
            &doc,
            &theme(),
            &mut fonts,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        );
        assert_eq!(out.runs, whole.runs[..out.runs.len()], "runs after {k}");
        assert_eq!(out.rects, whole.rects[..out.rects.len()], "rects after {k}");
        assert_eq!(
            out.anchors,
            whole.anchors[..out.anchors.len()],
            "anchors after {k}"
        );
    }
}

#[test]
fn height_grows_to_the_complete_height() {
    let doc = markdown::parse(ONE_OF_EACH);
    let mut fonts = fonts();
    let whole = lay_doc(&doc, 800.0, &mut fonts);
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    let mut previous = 0.0_f32;
    loop {
        let done = layout_step(
            &doc,
            &theme(),
            &mut fonts,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        );
        assert!(out.height >= previous, "height shrank to {}", out.height);
        assert!(out.height <= whole.height, "height passed the complete one");
        previous = out.height;
        if done {
            break;
        }
    }
    assert_eq!(out.height, whole.height);
}

#[test]
fn a_partial_code_panel_covers_the_placed_lines() {
    let doc = markdown::parse(CODE_LINES);
    let mut fonts = fonts();
    let theme = theme();
    let whole = lay_doc(&doc, 800.0, &mut fonts);
    let complete = whole
        .rects
        .iter()
        .find(|r| r.color == theme.blocks.code_bg)
        .expect("no code panel");
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    // The intro paragraph, then two of the four code lines.
    for _ in 0..3 {
        layout_step(
            &doc,
            &theme,
            &mut fonts,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        );
    }
    let panel = out
        .rects
        .iter()
        .find(|r| r.color == theme.blocks.code_bg)
        .expect("no partial code panel");
    let last = out.runs.last().expect("no placed run");
    let bottom = last.y + metrics::LINE_HEIGHT * last.size;
    assert!(
        panel.y + panel.height >= bottom,
        "panel stops above the lines it holds"
    );
    assert!(
        panel.height < complete.height,
        "a partial panel is already at full height"
    );
}

#[test]
fn a_table_grants_each_column_more_than_its_text_needs() {
    // The measuring pass shapes a cell with no wrap width; the layout pass
    // shapes it inside the column. Font fallback can resolve differently
    // between the two, so a column granted exactly the measured width wraps
    // its cell in a table with room to spare.
    let (doc, l) = lay2(
        "| Shortcut | Action | Notes |\n|---|---|---|\n\
         | Ctrl+F | Find in document | Smart case matching |\n",
        1200.0,
    );
    let pad = 8.0;
    let cell = find_text(&l, &doc, "Find in document");
    let next_column = l
        .runs
        .iter()
        .filter(|r| r.x > cell.x + cell.width)
        .map(|r| r.x)
        .fold(f32::MAX, f32::min);
    let inner = next_column - pad - cell.x;
    assert!(
        inner > cell.width,
        "column holds {inner} for text needing {}",
        cell.width
    );
}

#[test]
fn table_rows_record_their_bands() {
    let doc = markdown::parse("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
    let l = lay_doc(&doc, 800.0, &mut fonts());
    assert_eq!(l.table_rows.len(), 3, "header and two body rows");
    for pair in l.table_rows.windows(2) {
        assert!(pair[0].bottom <= pair[1].top + 0.01, "bands do not overlap");
        assert!(pair[0].bottom > pair[0].top, "a band has height");
    }
}

#[test]
fn an_image_scales_with_the_reading_size() {
    let doc = markdown::parse("![logo](oryx-test.png)");
    let laid = |body: f32| {
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let cfg = ViewConfig {
            body_size: body,
            ..cfg()
        };
        layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts(),
            &mut media,
            &cfg,
            2000.0,
        )
    };
    let reference = laid(oryx::layout::metrics::REFERENCE_BODY);
    let half = laid(oryx::layout::metrics::REFERENCE_BODY / 2.0);
    assert!(!reference.images.is_empty(), "the fixture has an image");
    assert!(
        (half.images[0].width - reference.images[0].width / 2.0).abs() < 0.5,
        "halving the text halves the image"
    );
    assert!(
        (half.images[0].height - reference.images[0].height / 2.0).abs() < 0.5,
        "and keeps its aspect"
    );
}

/// The index answers with a superset of the linear scan and stays a
/// search rather than a scan once the document is indexed.
#[test]
fn the_y_index_never_misses_an_element() {
    let source = std::fs::read_to_string("tests/fixtures/tour.md").unwrap();
    let mut fonts = fonts();
    for width in [420.0, 640.0, 900.0] {
        let mut lay = lay_doc(&markdown::parse(source.as_str()), width, &mut fonts);
        lay.index_more();
        assert!(lay.height > 2000.0, "the tour is tall enough to matter");
        let slices = 37;
        let step = lay.height / slices as f32;
        for i in 0..slices {
            let y0 = step * i as f32;
            let y1 = y0 + 300.0;
            let (head, tail) = lay.runs_in(y0, y1);
            for (index, run) in lay.runs.iter().enumerate() {
                let touches = run.y <= y1 && run.y + metrics::LINE_HEIGHT * run.size >= y0;
                assert!(
                    !touches || head.contains(&index) || tail.contains(&index),
                    "run {index} missed at width {width} slice {i}"
                );
            }
            assert!(tail.is_empty(), "everything is indexed after index_more");
            assert!(
                head.len() <= lay.runs.len() / 3 + 64,
                "a slice answers {} of {} runs at width {width}",
                head.len(),
                lay.runs.len()
            );
            let (rect_head, rect_tail) = lay.rects_in(y0, y1);
            for (index, rect) in lay.rects.iter().enumerate() {
                let touches = rect.y <= y1 && rect.y + rect.height >= y0;
                assert!(
                    !touches || rect_head.contains(&index) || rect_tail.contains(&index),
                    "rect {index} missed at width {width} slice {i}"
                );
            }
            let (image_head, image_tail) = lay.images_in(y0, y1);
            for (index, image) in lay.images.iter().enumerate() {
                let touches = image.y <= y1 && image.y + image.height >= y0;
                assert!(
                    !touches || image_head.contains(&index) || image_tail.contains(&index),
                    "image {index} missed at width {width} slice {i}"
                );
            }
        }
    }
}

/// The unindexed tail keeps queries honest between index calls while a
/// resumable pass appends.
#[test]
fn the_y_index_stays_honest_while_a_pass_grows() {
    let source = std::fs::read_to_string("tests/fixtures/tour.md").unwrap();
    let doc = markdown::parse(source.as_str());
    let mut fonts = fonts();
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 640.0);
    let mut steps = 0usize;
    loop {
        let done = layout_step(
            &doc,
            &theme(),
            &mut fonts,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        );
        steps += 1;
        // Index on a stride so some checks run against a stale index
        // with a real tail.
        if steps % 11 == 0 {
            out.index_more();
        }
        if steps % 5 == 0 || done {
            let (head, tail) = out.runs_in(0.0, out.height);
            for (index, _) in out.runs.iter().enumerate() {
                assert!(
                    head.contains(&index) || tail.contains(&index),
                    "run {index} missed after {steps} steps"
                );
            }
            if out.height > 1500.0 && steps % 11 == 0 {
                let (head, tail) = out.runs_in(0.0, 200.0);
                assert!(
                    head.len() + tail.len() < out.runs.len(),
                    "a slice query mid-pass is not a full scan"
                );
            }
        }
        if done {
            break;
        }
    }
}

/// Lays out a prefix of the source, splices the parse worker's tail the
/// way the app does, and hands back the extended layout with its pass.
fn splice_at(
    source: &str,
    cut: usize,
    width: f32,
    store: &mut FontStore,
) -> (Document, LayoutDoc, oryx::layout::LayoutPass) {
    let mut doc = markdown::parse(&source[..cut]);
    doc.source = std::sync::Arc::from(source);
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut lazy, mut pass) = layout_begin(&doc, &cfg(), width);
    layout_more(
        &doc,
        &Theme::default_dark(),
        store,
        &mut media,
        &cfg(),
        &mut lazy,
        &mut pass,
        None,
    );
    assert!(pass.is_complete(), "the prefix lays out whole");
    let full = markdown::parse(source);
    let Swap::Splice(tail) = stream::swap(&doc.blocks, full.blocks) else {
        panic!("the fixture cut splices")
    };
    doc.blocks.extend(tail);
    assert!(layout_extend(&doc, &mut pass), "a note-free prefix extends");
    layout_more(
        &doc,
        &Theme::default_dark(),
        store,
        &mut media,
        &cfg(),
        &mut lazy,
        &mut pass,
        None,
    );
    (doc, lazy, pass)
}

#[test]
fn an_extended_pass_matches_a_from_scratch_layout() {
    let source = "# Title\n\nintro paragraph\n\n```rust\nfn a() {}\n```\n\n\
        - item one\n- item two\n\ntail paragraph\n\n> a closing quote\n";
    let cut = source.find("- item").expect("the fixture holds a list");
    let mut store = fonts();
    let (doc, lazy, pass) = splice_at(source, cut, 800.0, &mut store);
    assert!(pass.is_complete());
    let scratch = lay_doc(&doc, 800.0, &mut store);
    assert_eq!(lazy.runs, scratch.runs);
    assert_eq!(lazy.rects, scratch.rects);
    assert_eq!(lazy.height, scratch.height);
}

#[test]
fn tail_footnotes_extend_into_the_note_section() {
    let source = "intro paragraph\n\nbody with a note[^1]\n\n[^1]: the note text\n";
    let cut = source.find("body").expect("the fixture holds a body");
    let mut store = fonts();
    let (doc, lazy, _) = splice_at(source, cut, 800.0, &mut store);
    let scratch = lay_doc(&doc, 800.0, &mut store);
    assert_eq!(lazy.runs, scratch.runs);
    assert_eq!(lazy.rects, scratch.rects);
    assert_eq!(lazy.height, scratch.height);
}

#[test]
fn a_prefix_with_placed_footnotes_refuses_extension() {
    let source = "a paragraph with a note[^1]\n\n[^1]: placed early\n\nlate paragraph\n";
    let cut = source.find("late").expect("the fixture holds a tail");
    let mut doc = markdown::parse(&source[..cut]);
    doc.source = std::sync::Arc::from(source);
    let mut store = fonts();
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut lazy, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    layout_more(
        &doc,
        &Theme::default_dark(),
        &mut store,
        &mut media,
        &cfg(),
        &mut lazy,
        &mut pass,
        None,
    );
    let full = markdown::parse(source);
    let Swap::Splice(tail) = stream::swap(&doc.blocks, full.blocks) else {
        panic!("the fixture cut splices")
    };
    doc.blocks.extend(tail);
    assert!(
        !layout_extend(&doc, &mut pass),
        "a placed note section cannot take body blocks after it"
    );
}

#[test]
fn a_mid_pass_extension_matches_a_from_scratch_layout() {
    let source = "# Title\n\none paragraph\n\nanother paragraph\n\nlast paragraph\n";
    let cut = source.find("another").expect("the fixture holds a middle");
    let mut doc = markdown::parse(&source[..cut]);
    doc.source = std::sync::Arc::from(source);
    let mut store = fonts();
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut lazy, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    layout_step(
        &doc,
        &Theme::default_dark(),
        &mut store,
        &mut media,
        &cfg(),
        &mut lazy,
        &mut pass,
    );
    assert!(!pass.is_complete(), "the prefix is mid-pass");
    let full = markdown::parse(source);
    let Swap::Splice(tail) = stream::swap(&doc.blocks, full.blocks) else {
        panic!("the fixture cut splices")
    };
    doc.blocks.extend(tail);
    assert!(layout_extend(&doc, &mut pass));
    layout_more(
        &doc,
        &Theme::default_dark(),
        &mut store,
        &mut media,
        &cfg(),
        &mut lazy,
        &mut pass,
        None,
    );
    let scratch = lay_doc(&doc, 800.0, &mut store);
    assert_eq!(lazy.runs, scratch.runs);
    assert_eq!(lazy.height, scratch.height);
}

#[test]
fn splice_offsets_positions_and_carried_indices() {
    let mut store = fonts();
    // The target holds unrelated content first, so the splice must shift
    // the carried record indices by the existing lengths, not only the
    // positions.
    let doc_a = markdown::parse("# Alpha\n\na paragraph without code\n\n> quoted\n");
    let mut target = lay_doc(&doc_a, 800.0, &mut store);
    let base_runs = target.runs.len();
    let base_rects = target.rects.len();
    let base_anchors = target.anchors.len();
    let height_a = target.height;

    let mut doc_b = markdown::parse(
        "second heading text\n-------\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\n\
         |a|b|\n|-|-|\n|1|2|\n",
    );
    let mut reference = lay_doc(&doc_b, 800.0, &mut store);
    let mut scratch = lay_doc(&doc_b, 800.0, &mut store);
    let top = 4096.0;
    target.splice(&mut scratch, top);

    assert_eq!(target.height, height_a, "the caller owns the height");
    assert!(scratch.runs.is_empty(), "the scratch drains for reuse");
    assert_eq!(target.runs.len(), base_runs + reference.runs.len());
    for (spliced, direct) in target.runs[base_runs..].iter().zip(&reference.runs) {
        assert_eq!(spliced.text, direct.text);
        assert_eq!(spliced.x, direct.x);
        assert_eq!(spliced.y, direct.y + top);
        assert_eq!(spliced.baseline, direct.baseline + top);
    }
    assert_eq!(target.rects.len(), base_rects + reference.rects.len());
    for (spliced, direct) in target.rects[base_rects..].iter().zip(&reference.rects) {
        assert_eq!(spliced.x, direct.x);
        assert_eq!(spliced.y, direct.y + top);
        assert_eq!(spliced.height, direct.height);
    }
    for (spliced, direct) in target.anchors[base_anchors..]
        .iter()
        .zip(&reference.anchors)
    {
        assert_eq!(spliced.0, direct.0);
        assert_eq!(spliced.1, direct.1 + top);
    }
    assert_eq!(target.table_rows.len(), reference.table_rows.len());
    for (spliced, direct) in target.table_rows.iter().zip(&reference.table_rows) {
        assert_eq!(spliced.top, direct.top + top);
        assert_eq!(spliced.bottom, direct.bottom + top);
    }

    // The code line records must re-shape at the spliced position: a
    // recolor through them lands on the appended runs, moves nothing,
    // and leaves the head untouched.
    highlight_all(&mut doc_b);
    let block = doc_b
        .blocks
        .iter()
        .position(|b| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .expect("the fixture holds code");
    let theme = Theme::default_dark();
    recolor_code_lines(
        &mut reference,
        &doc_b,
        &theme,
        &mut store,
        &cfg(),
        block,
        0..2,
    );
    recolor_code_lines(&mut target, &doc_b, &theme, &mut store, &cfg(), block, 0..2);
    assert_eq!(target.runs.len(), base_runs + reference.runs.len());
    for (spliced, direct) in target.runs[base_runs..].iter().zip(&reference.runs) {
        assert_eq!(spliced.color, direct.color);
        assert_eq!(spliced.text, direct.text);
        assert_eq!(spliced.y, direct.y + top);
    }
    let a_reference = lay_doc(&doc_a, 800.0, &mut store);
    for (kept, direct) in target.runs[..base_runs].iter().zip(&a_reference.runs) {
        assert_eq!(kept.text, direct.text);
        assert_eq!(kept.y, direct.y);
    }
}

/// Three code blocks with prose between them, highlighted in full, for
/// the batch recolor equivalences.
fn batch_fixture() -> (Document, String) {
    let source = "intro paragraph\n\n\
        ```rust\nfn one() {}\nlet a = \"s\";\n```\n\n\
        middle paragraph\n\n\
        ```rust\nfn two() {}\nlet b = 1;\n```\n\n\
        ```python\ndef three():\n    return 3\n```\n\n\
        closing paragraph\n"
        .to_string();
    (markdown::parse(source.as_str()), source)
}

#[test]
fn a_multi_patch_batch_matches_the_sequential_path() {
    let (mut doc, _) = batch_fixture();
    let mut store = fonts();
    let mut batched = lay_doc(&doc, 800.0, &mut store);
    let mut sequential = lay_doc(&doc, 800.0, &mut store);
    highlight_all(&mut doc);
    let blocks: Vec<usize> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .map(|(i, _)| i)
        .collect();
    let theme = Theme::default_dark();
    let patches: Vec<(usize, std::ops::Range<usize>)> = blocks.iter().map(|&b| (b, 0..2)).collect();
    recolor_batch(&mut batched, &doc, &theme, &mut store, &cfg(), &patches);
    for &b in &blocks {
        recolor_code_lines(&mut sequential, &doc, &theme, &mut store, &cfg(), b, 0..2);
    }
    assert_eq!(batched.runs, sequential.runs);
    let scratch = lay_doc(&doc, 800.0, &mut store);
    assert_eq!(batched.runs, scratch.runs, "and both match a fresh layout");
}

#[test]
fn a_middle_patch_shifts_later_records_like_the_sequential_path() {
    let (mut doc, _) = batch_fixture();
    let mut store = fonts();
    let mut batched = lay_doc(&doc, 800.0, &mut store);
    let mut sequential = lay_doc(&doc, 800.0, &mut store);
    highlight_all(&mut doc);
    let blocks: Vec<usize> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .map(|(i, _)| i)
        .collect();
    let middle = blocks[1];
    let theme = Theme::default_dark();
    recolor_batch(
        &mut batched,
        &doc,
        &theme,
        &mut store,
        &cfg(),
        &[(middle, 0..2)],
    );
    recolor_code_lines(
        &mut sequential,
        &doc,
        &theme,
        &mut store,
        &cfg(),
        middle,
        0..2,
    );
    assert_eq!(batched.runs, sequential.runs);
    // The record shift is observable through a later recolor: it must
    // land on the last block's runs in both.
    recolor_code_lines(
        &mut batched,
        &doc,
        &theme,
        &mut store,
        &cfg(),
        blocks[2],
        0..2,
    );
    recolor_code_lines(
        &mut sequential,
        &doc,
        &theme,
        &mut store,
        &cfg(),
        blocks[2],
        0..2,
    );
    assert_eq!(batched.runs, sequential.runs);
}

#[test]
fn an_empty_batch_is_a_no_op() {
    let (doc, _) = batch_fixture();
    let mut store = fonts();
    let mut lay = lay_doc(&doc, 800.0, &mut store);
    let before = lay.runs.clone();
    recolor_batch(
        &mut lay,
        &doc,
        &Theme::default_dark(),
        &mut store,
        &cfg(),
        &[],
    );
    assert_eq!(lay.runs, before);
}

#[test]
fn a_pooled_store_lays_out_identically() {
    let (mut doc, _) = batch_fixture();
    highlight_all(&mut doc);
    let mut template = fonts();
    let seed = template.seed();
    let direct = lay_doc(&doc, 800.0, &mut template);
    let mut pooled = FontStore::pooled(&seed);
    let clone = lay_doc(&doc, 800.0, &mut pooled);
    assert_eq!(direct.runs, clone.runs);
    assert_eq!(direct.rects, clone.rects);
    assert_eq!(direct.height, clone.height);
}

/// Lays out with the shaping pool attached, the way the app will.
fn lay_pooled(
    doc: &Document,
    width: f32,
    store: &mut FontStore,
    pool: &std::sync::Arc<ShapePool>,
) -> LayoutDoc {
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(doc, &cfg(), width);
    pass.attach_pool(std::sync::Arc::clone(pool));
    // Seed with an expired slice, then give the workers a moment, so the
    // pass provably consumes pooled steps instead of racing them.
    layout_more(
        doc,
        &Theme::default_dark(),
        store,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        Some(Instant::now()),
    );
    let seeded = Instant::now();
    while pool.completed() == 0 && seeded.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(1));
    }
    layout_more(
        doc,
        &Theme::default_dark(),
        store,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        None,
    );
    out
}

#[test]
fn a_pooled_pass_matches_the_serial_pass() {
    let (mut doc, _) = batch_fixture();
    highlight_all(&mut doc);
    let mut store = fonts();
    let serial = lay_doc(&doc, 800.0, &mut store);
    let pool = std::sync::Arc::new(ShapePool::new(2, &store.seed()));
    let pooled = lay_pooled(&doc, 800.0, &mut store, &pool);
    assert!(pool.completed() > 0, "the pool did real work");
    assert_eq!(serial.runs, pooled.runs);
    assert_eq!(serial.rects, pooled.rects);
    assert_eq!(serial.anchors, pooled.anchors);
    assert_eq!(serial.table_rows, pooled.table_rows);
    assert_eq!(serial.height, pooled.height);
}

#[test]
fn a_pooled_code_file_matches_the_serial_pass() {
    let mut source = String::from("```rust\n");
    for i in 0..120 {
        source.push_str(&format!("let value_{i} = compute({i}); // line {i}\n"));
    }
    source.push_str("```\n");
    let mut doc = markdown::parse(source.as_str());
    highlight_all(&mut doc);
    let mut store = fonts();
    let serial = lay_doc(&doc, 700.0, &mut store);
    let pool = std::sync::Arc::new(ShapePool::new(3, &store.seed()));
    let pooled = lay_pooled(&doc, 700.0, &mut store, &pool);
    assert!(pool.completed() > 0, "the pool shaped code lines");
    assert_eq!(serial.runs, pooled.runs);
    assert_eq!(serial.rects, pooled.rects);
    assert_eq!(serial.height, pooled.height);
}

#[test]
fn an_image_and_alert_document_matches_serial_under_the_pool() {
    // Blocks the pool must refuse: an image block and an alert region.
    // The assembler shapes them itself and the result stays identical.
    let source = "before paragraph\n\n\
        ![a missing image](nowhere.png)\n\n\
        > [!NOTE]\n> the alert body\n\n\
        after paragraph\n";
    let doc = markdown::parse(source);
    let mut store = fonts();
    let serial = lay_doc(&doc, 800.0, &mut store);
    let pool = std::sync::Arc::new(ShapePool::new(2, &store.seed()));
    let pooled = lay_pooled(&doc, 800.0, &mut store, &pool);
    assert_eq!(serial.runs, pooled.runs);
    assert_eq!(serial.rects, pooled.rects);
    assert_eq!(serial.height, pooled.height);
}

#[test]
fn a_stale_pool_generation_reseeds_and_completes() {
    let (mut doc, _) = batch_fixture();
    highlight_all(&mut doc);
    let mut store = fonts();
    let serial = lay_doc(&doc, 800.0, &mut store);
    let pool = std::sync::Arc::new(ShapePool::new(2, &store.seed()));
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    pass.attach_pool(std::sync::Arc::clone(&pool));
    for _ in 0..3 {
        layout_step(
            &doc,
            &Theme::default_dark(),
            &mut store,
            &mut media,
            &cfg(),
            &mut out,
            &mut pass,
        );
    }
    // Another pass claimed the pool in between, the export scenario.
    pool.begin();
    layout_more(
        &doc,
        &Theme::default_dark(),
        &mut store,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        None,
    );
    assert_eq!(serial.runs, out.runs);
    assert_eq!(serial.height, out.height);
}

#[test]
fn recolor_reports_the_spliced_range() {
    let (mut doc, _) = batch_fixture();
    let mut store = fonts();
    let mut lay = lay_doc(&doc, 800.0, &mut store);
    highlight_all(&mut doc);
    let middle = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .map(|(i, _)| i)
        .nth(1)
        .expect("the fixture holds three code blocks");
    let snapshot = lay.runs.clone();
    let theme = Theme::default_dark();
    let (lo, hi, delta) =
        recolor_code_lines(&mut lay, &doc, &theme, &mut store, &cfg(), middle, 0..2)
            .expect("a recolor that changed runs reports its splice");
    assert_eq!(
        lay.runs.len(),
        snapshot.len().wrapping_add_signed(delta),
        "the delta accounts for the length change"
    );
    assert!(lo < hi && hi <= snapshot.len());
    assert_eq!(lay.runs[..lo], snapshot[..lo], "the head is untouched");
    assert_eq!(
        lay.runs[hi.wrapping_add_signed(delta)..],
        snapshot[hi..],
        "the tail moved whole"
    );
    let missing = recolor_code_lines(&mut lay, &doc, &theme, &mut store, &cfg(), 9999, 0..2);
    assert!(missing.is_none(), "a no-op recolor reports nothing");
}

/// The field scenario behind the coverage walk in markdown copy: the
/// showcase collection carries footnote definitions three files in, the
/// notes section lays out last, and a select-all copy must still cover
/// the source tail.
#[test]
fn showcase_select_all_markdown_copy_covers_the_whole_source() {
    let mut names: Vec<_> = std::fs::read_dir("tests/showcase")
        .expect("showcase directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    names.sort();
    let mut source = String::new();
    for path in names {
        source.push_str(&std::fs::read_to_string(path).expect("showcase file"));
    }
    let doc = markdown::parse(source.as_str());
    let sel = oryx::ui::selection::all(&doc).expect("the document selects");
    let md = oryx::ui::selection::markdown(&sel, &doc);
    assert!(
        md.len() >= source.trim_end().len(),
        "the copy dropped {} of {} source bytes",
        source.len() - md.len(),
        source.len()
    );
}

/// The accessor pins for runs without text: whatever representation a
/// run carries, these answers must not move.
#[test]
fn run_accessors_answer_text_family_and_link() {
    let source = "# Head\n\npara with [tied](https://a.example/x) and `pill` here\n\n```rust\nlet a = 1;\n```\n\n- item one\n\nfoot[^n]\n\n[^n]: note body\n";
    let doc = markdown::parse(source);
    let mut fonts = FontStore::new();
    let lay = lay_doc(&doc, 900.0, &mut fonts);

    let texts: Vec<&str> = lay.runs.iter().map(|r| lay.run_text(&doc, r)).collect();
    let joined = texts.concat();
    assert!(joined.contains("para with "), "got {joined:?}");
    assert!(joined.contains("tied"), "got {joined:?}");
    assert!(joined.contains("let a = 1;"), "got {joined:?}");
    assert!(joined.contains("item one"), "got {joined:?}");

    let link_run = lay
        .runs
        .iter()
        .find(|r| lay.run_text(&doc, r) == "tied")
        .expect("the link run exists");
    assert_eq!(
        lay.run_link(&doc, link_run),
        Some("https://a.example/x"),
        "the link answers through the run"
    );

    let code_run = lay
        .runs
        .iter()
        .find(|r| lay.run_text(&doc, r).contains("let a"))
        .expect("the code run exists");
    assert_eq!(lay.run_family(code_run), CODE_FAMILY);

    let foot_run = lay
        .runs
        .iter()
        .find(|r| lay.run_text(&doc, r) == "n" && lay.run_link(&doc, r).is_some())
        .expect("the footnote reference run exists");
    assert_eq!(lay.run_link(&doc, foot_run), Some("footnote:n"));

    let marker = lay
        .runs
        .iter()
        .find(|r| r.span == oryx::ui::selection::MARKER_SPAN)
        .expect("the list marker run exists");
    assert!(!lay.run_text(&doc, marker).is_empty(), "markers keep text");
}

/// Table cells and math expansion have no flat span list; their runs
/// must still answer text and links through the accessors.
#[test]
fn run_accessors_cover_tables_and_math() {
    let source = "| a | b |\n|---|---|\n| one | [t](https://t.example) |\n\n$$x^2 + y$$\n";
    let doc = markdown::parse(source);
    let mut fonts = FontStore::new();
    let lay = lay_doc(&doc, 900.0, &mut fonts);
    let joined: String = lay
        .runs
        .iter()
        .map(|r| lay.run_text(&doc, r))
        .collect::<Vec<_>>()
        .concat();
    assert!(joined.contains("one"), "got {joined:?}");
    let cell_link = lay
        .runs
        .iter()
        .find(|r| lay.run_text(&doc, r) == "t")
        .expect("the cell link run exists");
    assert_eq!(lay.run_link(&doc, cell_link), Some("https://t.example"));
    assert!(
        joined.contains('x') && joined.contains('y'),
        "math runs resolve, got {joined:?}"
    );
}

/// Recoloring rebuilds code runs; their accessor texts must survive.
#[test]
fn recolor_preserves_accessor_texts() {
    let mut doc = markdown::parse("```rust\nfn main() {}\nlet x = 1;\n```");
    let mut fonts = FontStore::new();
    let source = std::sync::Arc::clone(&doc.source);
    let BlockKind::CodeBlock {
        language, lines, ..
    } = &doc.blocks[0].kind
    else {
        panic!()
    };
    let spans = highlight::spans(&source, lines, language.as_deref());
    let mut lay = lay_doc(&doc, 900.0, &mut fonts);
    let before: Vec<String> = lay
        .runs
        .iter()
        .map(|r| lay.run_text(&doc, r).to_string())
        .collect();
    let BlockKind::CodeBlock { highlights, .. } = &mut doc.blocks[0].kind else {
        panic!()
    };
    *highlights = spans;
    recolor_batch(
        &mut lay,
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &cfg(),
        &[(0, 0..2)],
    );
    let after: String = lay
        .runs
        .iter()
        .map(|r| lay.run_text(&doc, r))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(before.concat(), after, "texts survive the rebuild");
}

/// Task 51 pins: selection and search anchor on the model, so neither
/// needs the layout. The copy separators are the display rules: blocks
/// join with a blank line, table cells with a tab, rows and code lines
/// with newlines, footnote definitions carry their label.
#[test]
fn select_all_and_copy_need_no_layout() {
    let source = "# Title\n\npara one **bold** tail\n\n- item `code`\n\n```rust\nlet a = 1;\n\nlet b = 2;\n```\n\n|h1|h2|\n|-|-|\n|c1|c2|\n\nline one  \nline two\n\n> [!NOTE]\n> alert body\n\nfoot[^n]\n\n[^n]: note text\n";
    let doc = markdown::parse(source);
    let sel = oryx::ui::selection::all(&doc).expect("the document selects");
    let plain = oryx::ui::selection::plain_text(&sel, &doc);
    assert_eq!(
        plain,
        "Title\n\npara one bold tail\n\nitem code\n\nlet a = 1;\n\nlet b = 2;\n\nh1\th2\nc1\tc2\n\nline one\nline two\n\nalert body\n\nfoot n\n\nn.\tnote text"
    );
    let md = oryx::ui::selection::markdown(&sel, &doc);
    assert!(md.starts_with("# Title"), "got {md:?}");
    assert!(md.ends_with("[^n]: note text"), "got {md:?}");
}

#[test]
fn search_matches_need_no_layout() {
    let source = "one two\n\ntwo three\n\n|ab|cd|\n|-|-|\n|two|x|\n";
    let doc = markdown::parse(source);
    let found = oryx::ui::search::matches(&doc, "two");
    assert_eq!(found.len(), 3);
    assert!(oryx::ui::search::matches(&doc, "onetwo").is_empty());
    assert!(
        oryx::ui::search::matches(&doc, "abcd").is_empty(),
        "cells never join"
    );
}

// Task 52: the layout window. The pass measures the whole document but
// retains geometry only for a band around the scroll position; a block
// table records y and height for everything else, and re-entering a
// cold region re-shapes it at the recorded positions bit for bit.

/// The paint band at a scroll position: the viewport plus two viewport
/// heights each side, clamped the way the band cache clamps it.
fn band_at(scroll: f32, vh: f32, height: f32) -> (f32, f32) {
    let band_h = 5.0 * vh;
    let top = (scroll - 2.0 * vh).clamp(0.0, (height - band_h).max(0.0));
    (top, (top + band_h).min(height))
}

/// A complete pass with retention bounded around `scroll`.
fn windowed_doc(
    doc: &Document,
    width: f32,
    fonts: &mut FontStore,
    scroll: f32,
    vh: f32,
) -> LayoutDoc {
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut out, mut pass) = layout_begin(doc, &cfg(), width);
    pass.retain_around(scroll, vh);
    layout_more(
        doc,
        &theme(),
        fonts,
        &mut media,
        &cfg(),
        &mut out,
        &mut pass,
        None,
    );
    out
}

fn slide_to(doc: &Document, fonts: &mut FontStore, lay: &mut LayoutDoc, scroll: f32, vh: f32) {
    let mut media = MediaCache::new(PathBuf::from("."));
    window_to(
        doc,
        &theme(),
        fonts,
        &mut media,
        &cfg(),
        lay,
        None,
        scroll,
        vh,
        true,
    );
}

/// Every field of a run with its references resolved, so two layouts
/// compare across their private side buffers and family tables.
type RunKey = (
    u32,
    u32,
    u32,
    u32,
    u32,
    u16,
    bool,
    (u8, u8, u8, u8),
    usize,
    usize,
    String,
    String,
);

fn run_keys_in(lay: &LayoutDoc, doc: &Document, y0: f32, y1: f32) -> Vec<RunKey> {
    let mut keys: Vec<RunKey> = lay
        .runs
        .iter()
        .filter(|r| r.y >= y0 && r.y <= y1)
        .map(|r| {
            (
                r.y.to_bits(),
                r.x.to_bits(),
                r.width.to_bits(),
                r.baseline.to_bits(),
                r.size.to_bits(),
                r.weight,
                r.italic,
                (r.color.r, r.color.g, r.color.b, r.color.a),
                r.block,
                r.span,
                lay.run_text(doc, r).to_string(),
                lay.run_family(r).to_string(),
            )
        })
        .collect();
    keys.sort();
    keys
}

type RectKey = (u32, u32, u32, u32, (u8, u8, u8, u8), u32, u32, u32);

fn rect_keys_in(lay: &LayoutDoc, y0: f32, y1: f32) -> Vec<RectKey> {
    let mut keys: Vec<RectKey> = lay
        .rects
        .iter()
        .filter(|r| r.y < y1 && r.y + r.height > y0)
        .map(|r| {
            (
                r.y.to_bits(),
                r.x.to_bits(),
                r.width.to_bits(),
                r.height.to_bits(),
                (r.color.r, r.color.g, r.color.b, r.color.a),
                r.radius_top.to_bits(),
                r.radius_bottom.to_bits(),
                r.stroke.to_bits(),
            )
        })
        .collect();
    keys.sort();
    keys
}

fn image_keys_in(lay: &LayoutDoc, y0: f32, y1: f32) -> Vec<(String, u32, u32, u32, u32)> {
    let mut keys: Vec<(String, u32, u32, u32, u32)> = lay
        .images
        .iter()
        .filter(|i| i.y < y1 && i.y + i.height > y0)
        .map(|i| {
            (
                i.src.clone(),
                i.x.to_bits(),
                i.y.to_bits(),
                i.width.to_bits(),
                i.height.to_bits(),
            )
        })
        .collect();
    keys.sort();
    keys
}

/// The windowed layout holds exactly the full layout's elements over the
/// band at `scroll`, bit for bit.
fn assert_band_covered(
    windowed: &LayoutDoc,
    full: &LayoutDoc,
    doc: &Document,
    scroll: f32,
    vh: f32,
    what: &str,
) {
    let (y0, y1) = band_at(scroll, vh, full.height);
    let got = run_keys_in(windowed, doc, y0, y1);
    let want = run_keys_in(full, doc, y0, y1);
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: {} runs over the band, the full layout has {}",
        got.len(),
        want.len()
    );
    assert_eq!(got, want, "{what}: runs over the band");
    let got = rect_keys_in(windowed, y0, y1);
    let want = rect_keys_in(full, y0, y1);
    assert_eq!(got, want, "{what}: rects over the band");
    let got = image_keys_in(windowed, y0, y1);
    let want = image_keys_in(full, y0, y1);
    assert_eq!(got, want, "{what}: images over the band");
}

fn tour_doc() -> Document {
    let source = std::fs::read_to_string("tests/fixtures/tour.md").expect("the tour fixture");
    markdown::parse(source)
}

#[test]
fn a_windowed_pass_covers_the_band_bit_for_bit() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let full = lay_doc(&doc, 800.0, &mut fonts);
    let vh = full.height / 30.0;
    for frac in [0.0_f32, 0.4, 0.8] {
        let scroll = (full.height * frac).min(full.height - vh);
        let windowed = windowed_doc(&doc, 800.0, &mut fonts, scroll, vh);
        assert_eq!(
            windowed.height, full.height,
            "height with the window at {frac}"
        );
        assert_eq!(
            windowed.anchors, full.anchors,
            "anchors with the window at {frac}"
        );
        assert!(
            windowed.window_span().is_some(),
            "retention is bounded at {frac}"
        );
        assert!(
            windowed.runs.len() * 2 < full.runs.len(),
            "at {frac} the window keeps {} of {} runs",
            windowed.runs.len(),
            full.runs.len()
        );
        assert_band_covered(
            &windowed,
            &full,
            &doc,
            scroll,
            vh,
            &format!("window at {frac}"),
        );
    }
}

#[test]
fn sliding_the_window_reproduces_evicted_geometry() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let full = lay_doc(&doc, 800.0, &mut fonts);
    let vh = full.height / 30.0;
    let mut windowed = windowed_doc(&doc, 800.0, &mut fonts, 0.0, vh);
    let (top0, top1) = band_at(0.0, vh, full.height);
    let before = run_keys_in(&windowed, &doc, top0, top1);
    assert!(!before.is_empty(), "the top band is materialized");

    let bottom = full.height - vh;
    slide_to(&doc, &mut fonts, &mut windowed, bottom, vh);
    assert_band_covered(&windowed, &full, &doc, bottom, vh, "after the slide down");
    assert!(
        run_keys_in(&windowed, &doc, top0, top1).is_empty(),
        "the top band evicted"
    );

    slide_to(&doc, &mut fonts, &mut windowed, 0.0, vh);
    assert_eq!(
        run_keys_in(&windowed, &doc, top0, top1),
        before,
        "re-entry reproduces the evicted geometry"
    );
    assert_band_covered(&windowed, &full, &doc, 0.0, vh, "after the return");
}

#[test]
fn height_and_anchors_hold_wherever_the_window_sits() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let full = lay_doc(&doc, 800.0, &mut fonts);
    let vh = full.height / 30.0;
    let mut windowed = windowed_doc(&doc, 800.0, &mut fonts, 0.0, vh);
    for frac in [0.15_f32, 0.35, 0.55, 0.75, 0.95] {
        let scroll = (full.height * frac).min(full.height - vh);
        slide_to(&doc, &mut fonts, &mut windowed, scroll, vh);
        assert_eq!(
            windowed.height, full.height,
            "height with the window at {frac}"
        );
        assert_eq!(
            windowed.anchors, full.anchors,
            "anchors with the window at {frac}"
        );
    }
}

#[test]
fn select_all_and_copies_ignore_the_window() {
    let doc = tour_doc();
    let sel = oryx::ui::selection::all(&doc).expect("the tour selects");
    let plain = oryx::ui::selection::plain_text(&sel, &doc);
    let md = oryx::ui::selection::markdown(&sel, &doc);

    let mut fonts = fonts();
    let mut windowed = windowed_doc(&doc, 800.0, &mut fonts, 0.0, 300.0);
    let deep = windowed.height - 300.0;
    slide_to(&doc, &mut fonts, &mut windowed, deep, 300.0);
    assert_eq!(
        oryx::ui::selection::plain_text(&sel, &doc),
        plain,
        "plain copy is indifferent to the window"
    );
    assert_eq!(
        oryx::ui::selection::markdown(&sel, &doc),
        md,
        "markdown copy is indifferent to the window"
    );
}

#[test]
fn a_code_file_windows_by_line_range() {
    let mut source = String::from("```rust\n");
    for i in 0..400 {
        source.push_str(&format!("let value_{i} = compute({i}); // line {i}\n"));
    }
    source.push_str("```\n");
    let doc = markdown::parse(source.as_str());
    let mut fonts = fonts();
    let full = lay_doc(&doc, 700.0, &mut fonts);
    let vh = full.height / 30.0;
    let scroll = full.height * 0.5;
    let windowed = windowed_doc(&doc, 700.0, &mut fonts, scroll, vh);
    assert_eq!(
        windowed.height, full.height,
        "height of the windowed code file"
    );
    assert!(
        windowed.code_lines.len() * 2 < full.code_lines.len(),
        "the window keeps {} of {} line records",
        windowed.code_lines.len(),
        full.code_lines.len()
    );
    let lines: Vec<usize> = windowed.code_lines.iter().map(|c| c.line).collect();
    assert!(
        lines.windows(2).all(|w| w[1] == w[0] + 1),
        "the materialized lines are one contiguous range"
    );
    assert_band_covered(&windowed, &full, &doc, scroll, vh, "the code band");

    // A far jump inside the one block re-materializes only the landing,
    // never the lines between the old window and the new.
    let mut sliding = windowed_doc(&doc, 700.0, &mut fonts, 0.0, vh);
    for frac in [0.5_f32, 1.0, 0.25] {
        let stop = (full.height * frac).clamp(0.0, full.height - vh);
        slide_to(&doc, &mut fonts, &mut sliding, stop, vh);
        assert!(
            sliding.code_lines.len() * 2 < full.code_lines.len(),
            "after the jump to {frac} the window keeps {} of {} line records",
            sliding.code_lines.len(),
            full.code_lines.len()
        );
        assert_band_covered(
            &sliding,
            &full,
            &doc,
            stop,
            vh,
            &format!("the jump to {frac}"),
        );
    }
}

#[test]
fn a_cold_highlight_arrival_is_a_model_only_no_op() {
    let mut source = String::new();
    for i in 0..80 {
        source.push_str(&format!(
            "Filler paragraph number {i} with several words to give it body.\n\n"
        ));
    }
    source.push_str(
        "```rust\nfn main() {\n    let s = \"hi\";\n}\nlet tail = 9;\n```\n\nclosing paragraph\n",
    );
    let mut doc = markdown::parse(source.as_str());
    let code_block = doc
        .blocks
        .iter()
        .position(|b| matches!(b.kind, BlockKind::CodeBlock { .. }))
        .expect("the fixture holds code");
    let mut fonts = fonts();
    let vh = 300.0;
    let mut windowed = windowed_doc(&doc, 800.0, &mut fonts, 0.0, vh);
    assert!(
        windowed.code_lines.is_empty(),
        "the code block is cold under the top window"
    );
    let runs_before = windowed.runs.len();

    highlight_all(&mut doc);
    let spliced = recolor_code_lines(
        &mut windowed,
        &doc,
        &theme(),
        &mut fonts,
        &cfg(),
        code_block,
        0..4,
    );
    assert!(spliced.is_none(), "a cold arrival touches no records");
    assert_eq!(windowed.runs.len(), runs_before, "the layout is untouched");

    let full = lay_doc(&doc, 800.0, &mut fonts);
    let code_top = full
        .runs
        .iter()
        .filter(|r| r.block == code_block)
        .map(|r| r.y)
        .fold(f32::MAX, f32::min);
    let scroll = code_top.clamp(0.0, full.height - vh);
    slide_to(&doc, &mut fonts, &mut windowed, scroll, vh);
    assert_band_covered(
        &windowed,
        &full,
        &doc,
        scroll,
        vh,
        "a later entry shapes colored",
    );
}

#[test]
fn a_pooled_slide_matches_the_serial_window() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let full = lay_doc(&doc, 800.0, &mut fonts);
    let vh = full.height / 30.0;
    let pool = std::sync::Arc::new(ShapePool::new(3, &fonts.seed()));
    let mut media = MediaCache::new(PathBuf::from("."));
    let (mut windowed, mut pass) = layout_begin(&doc, &cfg(), 800.0);
    pass.attach_pool(std::sync::Arc::clone(&pool));
    pass.retain_around(0.0, vh);
    layout_more(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &cfg(),
        &mut windowed,
        &mut pass,
        None,
    );
    let before = pool.completed();
    let mid = full.height * 0.5;
    // The interactive hop fills only the viewport, through the pool.
    window_to(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &cfg(),
        &mut windowed,
        Some(&pool),
        mid,
        vh,
        false,
    );
    let viewport = run_keys_in(&windowed, &doc, mid, mid + vh);
    assert_eq!(
        viewport,
        run_keys_in(&full, &doc, mid, mid + vh),
        "the viewport fill"
    );
    assert!(!viewport.is_empty(), "the viewport holds runs");
    // The release fills the band around the same scroll.
    window_to(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &cfg(),
        &mut windowed,
        Some(&pool),
        mid,
        vh,
        true,
    );
    assert_band_covered(&windowed, &full, &doc, mid, vh, "the band fill on release");
    // Seeded jobs drain even past the calls; the pool provably worked.
    let waited = Instant::now();
    while pool.completed() == before && waited.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(pool.completed() > before, "the pool shaped window fills");
}

#[test]
fn zoom_rebuilds_the_table() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let mut media = MediaCache::new(PathBuf::from("."));
    let z2 = ViewConfig {
        zoom: 2.0,
        ..ViewConfig::default()
    };
    let full2 = layout(&doc, &theme(), &mut fonts, &mut media, &z2, 800.0);
    let vh = full2.height / 30.0;
    let (mut windowed2, mut pass) = layout_begin(&doc, &z2, 800.0);
    pass.retain_around(0.0, vh);
    layout_more(
        &doc,
        &theme(),
        &mut fonts,
        &mut media,
        &z2,
        &mut windowed2,
        &mut pass,
        None,
    );
    assert_eq!(
        windowed2.height, full2.height,
        "zoomed height from the zoomed table"
    );
    assert_band_covered(&windowed2, &full2, &doc, 0.0, vh, "the zoomed window");

    let windowed1 = windowed_doc(&doc, 800.0, &mut fonts, 0.0, vh);
    let deep = doc.blocks.len() / 2;
    let top1 = windowed1.approx_top(deep, 0).expect("the table answers");
    let top2 = windowed2
        .approx_top(deep, 0)
        .expect("the zoomed table answers");
    assert!(
        top2 > 1.5 * top1,
        "a deep block's recorded position scales with zoom: {top1} to {top2}"
    );
}

#[test]
fn approx_top_answers_cold_positions() {
    let doc = tour_doc();
    let mut fonts = fonts();
    let full = lay_doc(&doc, 800.0, &mut fonts);
    let vh = full.height / 30.0;
    let windowed = windowed_doc(&doc, 800.0, &mut fonts, 0.0, vh);
    let deep_run = full
        .runs
        .iter()
        .filter(|r| r.y > 0.7 * full.height && r.span != oryx::ui::selection::MARKER_SPAN)
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .expect("the tour has deep runs");
    // Raised scripts (footnote references, superscripts) sit above their
    // line's top, so they are excluded from the block-top estimate.
    let spans = match &doc.blocks[deep_run.block].kind {
        BlockKind::Paragraph { spans } | BlockKind::Heading { spans, .. } => spans.as_slice(),
        _ => &[],
    };
    let block_top = full
        .runs
        .iter()
        .filter(|r| r.block == deep_run.block)
        .filter(|r| {
            !spans
                .get(r.span)
                .is_some_and(|s| s.script != SpanScript::None)
        })
        .map(|r| r.y)
        .fold(f32::MAX, f32::min);
    let approx = windowed
        .approx_top(deep_run.block, 0)
        .expect("the table answers a cold block");
    assert!(
        approx <= block_top + 0.5 && block_top - approx < 60.0,
        "the recorded top {approx} sits at the block top {block_top}"
    );

    let mut source = String::from("```rust\n");
    for i in 0..400 {
        source.push_str(&format!("let value_{i} = compute({i}); // line {i}\n"));
    }
    source.push_str("```\n");
    let code = markdown::parse(source.as_str());
    let code_full = lay_doc(&code, 700.0, &mut fonts);
    let code_windowed = windowed_doc(&code, 700.0, &mut fonts, 0.0, code_full.height / 30.0);
    let line_top = code_full
        .runs
        .iter()
        .filter(|r| r.span == 300)
        .map(|r| r.y)
        .fold(f32::MAX, f32::min);
    assert_eq!(
        code_windowed.approx_top(0, 300),
        Some(line_top),
        "a cold code line's recorded top is exact"
    );
}

#[test]
fn model_selection_survives_recolor_and_relayout() {
    let mut doc = markdown::parse("intro\n\n```rust\nfn main() {}\n```\n\ntail");
    let mut fonts = FontStore::new();
    let sel = oryx::ui::selection::all(&doc).expect("selects");
    let before = oryx::ui::selection::plain_text(&sel, &doc);
    highlight_all(&mut doc);
    let mut lay = lay_doc(&doc, 900.0, &mut fonts);
    recolor_batch(
        &mut lay,
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &cfg(),
        &[(1, 0..1)],
    );
    assert_eq!(
        oryx::ui::selection::plain_text(&sel, &doc),
        before,
        "the anchor is the model; nothing to remap"
    );
}
