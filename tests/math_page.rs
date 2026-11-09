//! The math reference page, `MATH.md`, read against noad's tables: every
//! command noad knows is on the page, and every formula of the page
//! parses without a literal, so the page shows nothing Oryx cannot
//! typeset. The section "A command Oryx does not know" shows literals on
//! purpose and stays out of that second check. The page's two parts are
//! the targets of its first line and of the F1 help, so their anchors
//! are checked too.

use oryx::doc::markdown::parse;
use oryx::doc::model::{BlockKind, Span};

const PAGE: &str = include_str!("../MATH.md");

/// The heading a section shows literals under, on purpose.
const LITERALS_ON_PURPOSE: &str = "A command Oryx does not know";

fn joined(spans: &[Span], source: &str) -> String {
    spans.iter().map(|s| s.text(source)).collect()
}

fn math_of(spans: &[Span], source: &str, heading: &str, out: &mut Vec<(String, String)>) {
    for span in spans.iter().filter(|s| s.math) {
        out.push((heading.to_string(), span.text(source).to_string()));
    }
}

/// Every formula of the page, with the heading it stands under.
fn formulas() -> Vec<(String, String)> {
    let doc = parse(PAGE.to_string());
    let source: &str = &doc.source;
    let mut out = Vec::new();
    let mut heading = String::new();
    for block in &doc.blocks {
        match &block.kind {
            BlockKind::Heading { spans, .. } => heading = joined(spans, source),
            BlockKind::Paragraph { spans, .. } | BlockKind::ListItem { spans, .. } => {
                math_of(spans, source, &heading, &mut out)
            }
            BlockKind::Table { header, rows, .. } => {
                for cell in header {
                    math_of(cell, source, &heading, &mut out);
                }
                for cell in rows.iter().flatten() {
                    math_of(cell, source, &heading, &mut out);
                }
            }
            BlockKind::MathBlock { tex } => out.push((heading.clone(), tex.clone())),
            _ => {}
        }
    }
    out
}

#[test]
fn every_formula_of_the_page_parses_without_a_literal() {
    let all = formulas();
    assert!(
        all.len() > 800,
        "{} formulas: the reference part is missing",
        all.len()
    );
    let mut bad = Vec::new();
    for (heading, tex) in &all {
        if heading == LITERALS_ON_PURPOSE {
            continue;
        }
        let literals = noad::parse::parse(tex).literals();
        if !literals.is_empty() {
            bad.push(format!("under {heading}: {tex} left {literals:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn the_section_of_unknown_commands_shows_one() {
    let shown = formulas()
        .iter()
        .filter(|(heading, _)| heading == LITERALS_ON_PURPOSE)
        .any(|(_, tex)| !noad::parse::parse(tex).literals().is_empty());
    assert!(
        shown,
        "the section {LITERALS_ON_PURPOSE:?} shows no literal"
    );
}

#[test]
fn every_command_noad_knows_is_on_the_page() {
    let mut wanted: Vec<String> = Vec::new();
    for &(name, _, _) in noad::parse::VOCABULARY {
        wanted.push(format!("`\\{name}`"));
    }
    for &(name, _) in noad::parse::DELIMITERS {
        wanted.push(format!("`\\{name}`"));
    }
    for &(name, _, _) in noad::parse::ACCENTS {
        wanted.push(format!("`\\{name}`"));
    }
    for &(name, _) in noad::parse::OP_NAMES {
        wanted.push(format!("`\\{name}`"));
    }
    for &(name, _) in noad::parse::SPACING_WORDS {
        wanted.push(format!("`\\{name}`"));
    }
    for &(name, _) in noad::parse::COMMANDS {
        wanted.push(format!("`\\{name}`"));
    }
    for &name in noad::parse::ENVIRONMENTS {
        wanted.push(format!("`\\begin{{{name}}}"));
    }
    for &(name, _) in noad::parse::COLORS {
        wanted.push(format!("`{name}`"));
    }
    let missing: Vec<&String> = wanted
        .iter()
        .filter(|w| !PAGE.contains(w.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{} of {} not on the page: {missing:?}",
        missing.len(),
        wanted.len()
    );
}

#[test]
fn the_two_parts_have_their_anchors() {
    let doc = parse(PAGE.to_string());
    for slug in ["lessons", "reference"] {
        assert!(
            doc.blocks.iter().any(|b| {
                matches!(&b.kind, BlockKind::Heading { level: 2, anchor, .. } if anchor == slug)
            }),
            "no level-two heading with the anchor {slug}"
        );
    }
    assert!(PAGE.contains("[Lessons](#lessons)") && PAGE.contains("[Reference](#reference)"));
}
