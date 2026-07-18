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
    l.runs.iter().find(|r| r.size == 16.0).expect("no body run")
}

#[test]
fn h1_larger_and_spaced() {
    let l = lay("# Title\n\nBody text", 800.0);
    let title = &l.runs[0];
    assert_eq!(title.size, 32.0);
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
    assert_eq!(zoomed.runs[0].size, 64.0);
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
    let line_height = 16.0 * 1.5;
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
    let h2 = l.runs.iter().find(|r| r.size == 16.0 * 1.6).unwrap();
    assert!((l.anchors[1].1 - h2.y).abs() < 0.5);
}

#[test]
fn inline_code_uses_code_font() {
    let l = lay("body `mono` body", 800.0);
    let code = l.runs.iter().find(|r| r.family == CODE_FAMILY).unwrap();
    assert_eq!(code.text, "mono");
    assert_eq!(code.size, 14.0);
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
fn strike_emits_line_rect() {
    let l = lay("~~gone~~", 800.0);
    let run = &l.runs[0];
    let rect = l
        .rects
        .iter()
        .find(|r| r.y > run.y && r.y < run.y + 24.0)
        .expect("no strike rect");
    assert!((rect.width - run.width).abs() < 1.0);
}
