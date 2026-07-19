use std::path::PathBuf;

use oryx::doc::images::MediaCache;
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
    let mut media = MediaCache::new(PathBuf::from("."));
    layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts(),
        &mut media,
        &cfg(),
        width,
    )
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
fn code_lines_wrap_inside_the_panel() {
    let src = "```rust\n// this comment is long enough that it must wrap into more than one visual line at a narrow width\nlet x = 1;\n```";
    let l = lay(src, 420.0);
    let t = Theme::default_dark();
    let panel = l
        .rects
        .iter()
        .find(|r| r.color == t.blocks.code_bg)
        .unwrap();
    let right = panel.x + panel.width;
    for r in l.runs.iter().filter(|r| r.family == CODE_FAMILY) {
        assert!(
            r.x + r.width <= right + 0.5,
            "run overflows the panel: {:?}",
            r.text
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
    let l = lay_with_images("![the alt text](gone.png)", 800.0, dir);
    let t = Theme::default_dark();
    assert!(l.images.is_empty());
    assert!(
        l.rects
            .iter()
            .any(|r| r.color == t.blocks.code_border && r.stroke > 0.0),
        "placeholder outline"
    );
    assert!(
        l.runs.iter().any(|r| r.text.contains("the alt text")),
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
    let l = lay("intro [click here](https://a.tld) outro", 800.0);
    let link = l.runs.iter().find(|r| r.link.is_some()).unwrap();
    let hit = l.link_at(link.x + link.width / 2.0, link.y + link.size / 2.0);
    assert_eq!(hit, Some("https://a.tld"));
}

#[test]
fn link_at_returns_none_outside_links() {
    let l = lay("intro [click here](https://a.tld) outro", 800.0);
    let plain = l.runs.iter().find(|r| r.link.is_none()).unwrap();
    assert_eq!(l.link_at(plain.x + 1.0, plain.y + 1.0), None);
    assert_eq!(l.link_at(-10.0, -10.0), None);
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
    let l = lay(
        "coverage: <img src=\"c.png\" width=\"40\" height=\"20\">",
        800.0,
    );
    let text = l.runs.iter().find(|r| r.text.contains("coverage")).unwrap();
    let img = &l.images[0];
    assert!(img.x >= text.x + text.width - 1.0, "badge after the text");
    assert!(
        img.y >= text.y - 1.0 && img.y + img.height <= text.y + 34.0,
        "badge inside the text line box"
    );
}

#[test]
fn linked_inline_image_is_clickable() {
    let l = lay(
        "<p align=\"center\"><a href=\"https://z.tld\"><img src=\"d.png\" width=\"40\" height=\"20\"></a></p>",
        800.0,
    );
    let img = &l.images[0];
    assert_eq!(
        l.link_at(img.x + 5.0, img.y + 5.0),
        Some("https://z.tld"),
        "image hit box carries the link"
    );
}

#[test]
fn footnote_reference_superscript_and_definitions_last() {
    let t = Theme::default_dark();
    // The definition sits mid-document; layout must still render it last.
    let l = lay("body text[^n]\n\n[^n]: the note itself\n\nmore", 800.0);
    let reference = l
        .runs
        .iter()
        .find(|r| r.link.as_deref() == Some("footnote:n"))
        .expect("reference run");
    assert!(
        (reference.size - 22.0 * 0.7).abs() < 0.5,
        "superscript size"
    );
    assert_eq!(reference.color, t.text.link);
    let body = l.runs.iter().find(|r| r.text.contains("body")).unwrap();
    assert!(
        reference.baseline < body.baseline - 2.0,
        "reference baseline raised"
    );
    let note = l
        .runs
        .iter()
        .find(|r| r.text.contains("the note itself"))
        .expect("definition text");
    let more = l.runs.iter().find(|r| r.text == "more").unwrap();
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
    let l = lay("energy $E=mc^2$ inline", 800.0);
    let m = l
        .runs
        .iter()
        .find(|r| r.text.contains("E=mc"))
        .expect("math run");
    assert_eq!(m.color, t.text.math);
    assert_eq!(m.family, CODE_FAMILY);
    assert!(m.italic);
    let sup = l
        .runs
        .iter()
        .find(|r| r.text == "2" && r.size < m.size)
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
        let pixels = oryx::paint::band(&l, &t, &mut fonts(), &mut media, &[], 0.0, 800, 900);
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
        let l = lay(&format!("> [!{tag}]\n> Body here."), 800.0);
        let title = l
            .runs
            .iter()
            .find(|r| r.text == title_text)
            .unwrap_or_else(|| panic!("{tag}: no title run"));
        assert_eq!(title.weight, 700, "{tag}");
        assert_eq!(title.color, color, "{tag}");
        let body = l
            .runs
            .iter()
            .find(|r| r.text.contains("Body here"))
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
    let l = lay("---\ntitle: Oryx\ntags: docs\n---\n\n# Head\n\nBody", 800.0);
    let panel = l
        .rects
        .iter()
        .find(|r| r.color == theme.blocks.frontmatter_bg)
        .expect("no frontmatter panel");
    let meta = l
        .runs
        .iter()
        .find(|r| r.text.contains("title: Oryx"))
        .expect("no metadata line");
    assert_eq!(meta.color, theme.blocks.frontmatter_fg);
    let heading = l.runs.iter().find(|r| r.text == "Head").unwrap();
    assert!(panel.y < heading.y);
    assert!(panel.y + panel.height <= heading.y);
    assert!(meta.y < heading.y);
}
