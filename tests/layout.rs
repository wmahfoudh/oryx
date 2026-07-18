use oryx::doc::markdown;
use oryx::layout::{layout, LayoutDoc, TextRun, ViewConfig};
use oryx::style::fonts::{FontStore, CODE_FAMILY};
use oryx::style::theme::Theme;

fn fonts() -> FontStore {
    FontStore::new()
}

fn cfg() -> ViewConfig {
    ViewConfig::default()
}

fn lay(source: &str, width: f32) -> LayoutDoc {
    let doc = markdown::parse(source);
    layout(&doc, &Theme::default_dark(), &mut fonts(), &cfg(), width)
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
    let zoomed = layout(&doc, &Theme::default_dark(), &mut fonts(), &config, 800.0);
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
    let l = lay("body `mono` body", 800.0);
    let code = l.runs.iter().find(|r| r.family == CODE_FAMILY).unwrap();
    assert_eq!(code.text, "mono");
    assert_eq!(code.size, 20.0);
}

#[test]
fn link_color_and_target() {
    let l = lay("[click](https://a.tld)", 800.0);
    let t = Theme::default_dark();
    let link = l.runs.iter().find(|r| r.link.is_some()).unwrap();
    assert_eq!(link.color, t.text.link);
    assert_eq!(link.link.as_deref(), Some("https://a.tld"));
}

#[test]
fn code_block_panel_and_highlighting() {
    let l = lay("```rust\nfn main() {\n    let s = \"hi\";\n}\n```", 800.0);
    let t = Theme::default_dark();
    assert!(l.rects.iter().any(|r| r.color == t.blocks.code_bg));
    assert!(l.rects.iter().any(|r| r.color == t.blocks.code_border));
    let kw = l
        .runs
        .iter()
        .find(|r| r.color == t.syntax.keyword)
        .expect("keyword-colored run");
    assert_eq!(kw.family, CODE_FAMILY);
    assert!(l.runs.iter().any(|r| r.color == t.syntax.string));
    let rows: std::collections::BTreeSet<i64> = l.runs.iter().map(|r| r.y as i64).collect();
    assert!(rows.len() >= 3, "one row per source line, got {rows:?}");
}

#[test]
fn inline_code_gets_pill() {
    let l = lay("with `mono` inside", 800.0);
    let t = Theme::default_dark();
    let run = l.runs.iter().find(|r| r.family == CODE_FAMILY).unwrap();
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
    let l = lay("1. one\n2. two\n3. three", 800.0);
    assert!(l.runs.iter().any(|r| r.text == "3."));
    let one = l.runs.iter().find(|r| r.text == "one").unwrap();
    let marker = l.runs.iter().find(|r| r.text == "1.").unwrap();
    assert!(marker.x < one.x);
}

#[test]
fn nested_items_indent_deeper() {
    let l = lay("- outer\n  - inner", 800.0);
    let outer = l.runs.iter().find(|r| r.text == "outer").unwrap();
    let inner = l.runs.iter().find(|r| r.text == "inner").unwrap();
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
fn table_columns_header_and_stripes() {
    let l = lay("|alpha|beta|gamma|\n|-|-|-|\n|a|b|c|\n|d|e|f|", 800.0);
    let t = Theme::default_dark();
    let alpha = l.runs.iter().find(|r| r.text == "alpha").unwrap();
    let beta = l.runs.iter().find(|r| r.text == "beta").unwrap();
    let gamma = l.runs.iter().find(|r| r.text == "gamma").unwrap();
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
    let a = l.runs.iter().find(|r| r.text == "a").unwrap();
    assert!(a.y > alpha.y, "body row below header");
    assert!((a.x - alpha.x).abs() < 1.0, "same column aligns");
}

#[test]
fn table_cells_wrap_within_capped_columns() {
    let l = lay(
        "|short|this long cell has quite a lot of text and must wrap|\n|-|-|\n|x|y|",
        500.0,
    );
    for r in &l.runs {
        assert!(
            r.x + r.width <= 460.5,
            "run exceeds table bounds: {} {}",
            r.text,
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
