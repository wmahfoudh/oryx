//! The fuzzers: random steps against a plain model. Ignored under
//! `make check`; `make fuzz` runs them in the debug profile, where the
//! assertions fire, and `PROPTEST_CASES=5000 make fuzz` runs them deep.
//! A failing case is shrunk by proptest and printed with its seed, and
//! is then kept as a plain test beside the code it caught.

use oryx::doc::load::FileKind;
use oryx::doc::markdown;
use oryx::doc::model::{BlockKind, Document, Span};
use oryx::edit::manners::case_edit;
use oryx::edit::splice::{combine, line_pieces, Ledger, Placed, Replay};
use oryx::edit::undo::{Kind, Undo};
use oryx::edit::{rendered_document, splice_document};
use oryx::paint::scroll::{self, Edge, EdgeScroll};
use oryx::ui::search;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The marker whose place the park's ledger must follow through the
/// edits: two bytes, so an offset inside it is a boundary fault too.
const MARK: char = '\u{a7}';

/// One step of an editing session. A place is a fraction of the text,
/// mapped onto a character boundary when the step applies.
#[derive(Debug, Clone)]
enum Step {
    Insert {
        at: u16,
        text: String,
        rest: bool,
    },
    Delete {
        at: u16,
        to: u16,
        rest: bool,
    },
    Undo,
    Redo,
    Park,
    Save,
    /// Replace All: every occurrence of a needle of `NEEDLES`, left to
    /// right, replaced by `with`.
    ReplaceAll {
        needle: usize,
        with: String,
    },
    /// A line command over the whole lines between two places: a mark
    /// set before each line, the line breaks untouched.
    Lines {
        at: u16,
        to: u16,
    },
}

/// What Replace All looks for: letters, a line break, and both.
const NEEDLES: &[&str] = &["a", "b", "\n", "a\n", "ab"];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (any::<u16>(), "[a-z \n]{1,5}", any::<bool>())
            .prop_map(|(at, text, rest)| Step::Insert { at, text, rest }),
        3 => (any::<u16>(), any::<u16>(), any::<bool>())
            .prop_map(|(at, to, rest)| Step::Delete { at, to, rest }),
        2 => Just(Step::Undo),
        1 => Just(Step::Redo),
        1 => Just(Step::Park),
        1 => Just(Step::Save),
        1 => (0..NEEDLES.len(), "[a-z\n]{0,3}")
            .prop_map(|(needle, with)| Step::ReplaceAll { needle, with }),
        1 => (any::<u16>(), any::<u16>()).prop_map(|(at, to)| Step::Lines { at, to }),
    ]
}

/// A character boundary of `text` at about `frac` of its length.
fn boundary(text: &str, frac: u16) -> usize {
    let mut at = (text.len() as u64 * u64::from(frac) / (u64::from(u16::MAX) + 1)) as usize;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What the app parks at a crossing into the editor: the page's text,
/// a ledger over it for the edits since, and where the marker stood.
struct Park {
    ledger: Ledger,
    text: String,
    mark: Option<usize>,
}

impl Park {
    fn at(text: &str) -> Park {
        Park {
            ledger: Ledger::new(Arc::from(text), Vec::new()),
            text: text.to_string(),
            mark: text.find(MARK),
        }
    }
}

/// The session: the ledger, the undo stack and the park driven the way
/// the app drives them, beside a plain string.
struct Session {
    led: Ledger,
    undo: Undo,
    park: Option<Park>,
    model: String,
    saved: String,
    /// The ending of each line break of the model that no edit wrote:
    /// `Some(true)` a return before it, `Some(false)` none, `None` for a
    /// line break an edit wrote, whose ending is the file's usual one.
    endings: Vec<Option<bool>>,
    now: Instant,
}

impl Session {
    fn apply(&mut self, step: &Replay) {
        self.led.replay(step);
        if let Some(park) = self.park.as_mut() {
            park.ledger.splice(step.range.clone(), &step.text);
        }
        if step.pieces.is_empty() {
            self.replace(step.range.clone(), &step.text);
        } else {
            for (range, text) in step.pieces.iter().rev() {
                self.replace(range.clone(), text);
            }
        }
    }

    /// One replacement in the model and its record of endings: the line
    /// breaks it removes leave the record, and the ones it writes join it
    /// as edited.
    fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let at = self.model[..range.start].matches('\n').count();
        let removed = self.model[range.clone()].matches('\n').count();
        let written = text.matches('\n').count();
        self.endings
            .splice(at..at + removed, std::iter::repeat_n(None, written));
        self.model.replace_range(range, text);
    }

    fn edit(&mut self, range: std::ops::Range<usize>, text: &str, kind: Kind, rest: bool) {
        if rest {
            self.now += Duration::from_secs(2);
        }
        let replaced = self.model[range.clone()].to_string();
        let caret = (range.end, range.start + text.len());
        self.apply(&Replay {
            range: range.clone(),
            text: text.to_string(),
            pieces: Vec::new(),
        });
        self.undo
            .record(range, text, &replaced, caret, kind, self.now);
    }

    fn step(&mut self, step: &Step) {
        match step {
            Step::Insert { at, text, rest } => {
                let at = boundary(&self.model, *at);
                self.edit(at..at, text, Kind::Insert, *rest);
            }
            Step::Delete { at, to, rest } => {
                let a = boundary(&self.model, *at);
                let b = boundary(&self.model, *to).max(a);
                if a == b {
                    return;
                }
                let kind = if self.model[a..b].chars().count() == 1 {
                    Kind::Delete
                } else {
                    Kind::Structural
                };
                self.edit(a..b, "", kind, *rest);
            }
            Step::Undo => {
                if let Some((step, _)) = self.undo.undo() {
                    self.apply(&step);
                }
            }
            Step::Redo => {
                if let Some((step, _)) = self.undo.redo() {
                    self.apply(&step);
                }
            }
            Step::ReplaceAll { needle, with } => {
                let needle = NEEDLES[*needle];
                let edits: Vec<(std::ops::Range<usize>, String)> = self
                    .model
                    .match_indices(needle)
                    .map(|(at, _)| (at..at + needle.len(), with.clone()))
                    .collect();
                let Some((range, text)) = combine(&self.model, &edits) else {
                    return;
                };
                let before = self.model.clone();
                self.apply(&Replay {
                    range,
                    text,
                    pieces: edits.clone(),
                });
                self.undo.record_each(&before, &edits, (0, 0), self.now);
            }
            Step::Lines { at, to } => {
                let a = boundary(&self.model, *at);
                let b = boundary(&self.model, *to).max(a);
                let start = self.model[..a].rfind('\n').map_or(0, |i| i + 1);
                let end = self.model[b..]
                    .find('\n')
                    .map_or(self.model.len(), |i| b + i);
                let old = self.model[start..end].to_string();
                let new = old
                    .split('\n')
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let Some(pieces) = line_pieces(&old, &new, start) else {
                    return;
                };
                let Some((range, text)) = combine(&self.model, &pieces) else {
                    return;
                };
                let before = self.model.clone();
                let step = Replay {
                    range,
                    text,
                    pieces: pieces.clone(),
                };
                self.led.replay(&step);
                if let Some(park) = self.park.as_mut() {
                    park.ledger.splice(step.range.clone(), &step.text);
                }
                // The model sets the marks on its own, at each line's
                // start, so a piece that took a line break shows.
                let starts: Vec<usize> = std::iter::once(start)
                    .chain(old.match_indices('\n').map(|(i, _)| start + i + 1))
                    .collect();
                for at in starts.into_iter().rev() {
                    self.replace(at..at, "> ");
                }
                self.undo.record_each(&before, &pieces, (0, 0), self.now);
            }
            Step::Park => self.park = Some(Park::at(&self.model)),
            Step::Save => {
                let written = self.led.commit();
                self.endings = endings_of(&written).into_iter().map(Some).collect();
                self.undo.mark_saved();
                self.saved = self.model.clone();
                // The app rebuilds the parked page at a save when the
                // bytes moved, and the park's ledger starts afresh.
                if self.park.as_ref().is_some_and(|p| p.text != self.model) {
                    self.park = Some(Park::at(&self.model));
                }
            }
        }
    }

    fn check(&self) -> Result<(), TestCaseError> {
        prop_assert_eq!(&self.led.current(), &self.model, "the ledger's text");
        let bytes = self.led.emit();
        let written = endings_of(&bytes);
        prop_assert_eq!(written.len(), self.endings.len(), "a line break each");
        for (index, (written, kept)) in written.iter().zip(&self.endings).enumerate() {
            if let Some(kept) = kept {
                prop_assert_eq!(
                    written,
                    kept,
                    "line break {} keeps its ending, no edit wrote it",
                    index
                );
            }
        }
        let emitted = String::from_utf8(bytes).expect("utf-8 out");
        let normalized = emitted.replace("\r\n", "\n");
        prop_assert_eq!(&normalized, &self.model, "the emitted bytes");
        prop_assert!(!normalized.contains('\r'), "no lone return");
        prop_assert!(
            self.led.is_dirty() || self.model == self.saved,
            "a clean ledger means the saved text"
        );
        if let Some(park) = &self.park {
            prop_assert!(
                park.ledger.is_dirty() || park.text == self.model,
                "a clean park ledger means the park's bytes"
            );
            if let (Some(at_park), Some(now_at)) = (park.mark, self.model.find(MARK)) {
                match park.ledger.place_of(at_park) {
                    Placed::Exact(at) => {
                        prop_assert_eq!(
                            at,
                            now_at,
                            "the marker's place through the edits since the park"
                        );
                    }
                    Placed::Replaced(start) => {
                        prop_assert!(start <= now_at, "the marker inside its replaced region");
                    }
                }
            }
        }
        Ok(())
    }
}

/// For each line break of the written bytes, whether a return stands
/// before it.
fn endings_of(bytes: &[u8]) -> Vec<bool> {
    bytes
        .iter()
        .enumerate()
        .filter(|&(_, &byte)| byte == b'\n')
        .map(|(at, _)| at > 0 && bytes[at - 1] == b'\r')
        .collect()
}

fn run(a: &str, b: &str, endings: &[bool], steps: &[Step]) -> Result<(), TestCaseError> {
    let base = format!("{a}{MARK}{b}");
    let crlf: Vec<u32> = base
        .match_indices('\n')
        .zip(endings)
        .filter(|(_, &crlf)| crlf)
        .map(|((at, _), _)| at as u32)
        .collect();
    let mut session = Session {
        led: Ledger::new(Arc::from(base.as_str()), crlf),
        undo: Undo::new(),
        park: None,
        model: base.clone(),
        endings: base
            .match_indices('\n')
            .enumerate()
            .map(|(index, _)| Some(endings.get(index).copied().unwrap_or(false)))
            .collect(),
        saved: base,
        now: Instant::now(),
    };
    for step in steps {
        session.step(step);
        session.check()?;
    }
    Ok(())
}

/// The editor's ledger, undo stack and park: random typing, deletions,
/// Replace All, undos, redos, crossings and saves against a plain string,
/// in a file of mixed endings whose untouched lines keep theirs.
#[test]
#[ignore]
fn fuzz_ledger() {
    // A failing case is printed shrunk and kept as a plain test, not in
    // a regressions file beside the sources.
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        a in "[a-z\n]{0,40}",
        b in "[a-z\n]{0,40}",
        endings in prop::collection::vec(any::<bool>(), 0..40),
        steps in prop::collection::vec(step(), 1..200),
    )| {
        run(&a, &b, &endings, &steps)?;
    });
}

/// The page's shape that the case key must leave alone: for each block
/// its kind and its spans, or its formula.
#[derive(Debug, PartialEq)]
enum Shape {
    Spans(Vec<SpanShape>),
    Math(String),
}

#[derive(Debug, PartialEq, Clone)]
struct SpanShape {
    text: String,
    kept: bool,
    link: Option<String>,
    image: Option<String>,
    range: std::ops::Range<u32>,
    style: (bool, bool, bool, bool, bool),
}

fn span_shape(s: &Span, source: &str) -> SpanShape {
    SpanShape {
        text: s.text(source).to_string(),
        kept: s.code || s.math,
        link: s.link.clone(),
        image: s.image.as_ref().map(|i| i.src.clone()),
        range: s.range.clone(),
        style: (s.bold, s.italic, s.strike, s.underline, s.mark),
    }
}

/// None for a block kind the fuzzer does not shape, which ends the case.
fn shapes(doc: &Document) -> Option<Vec<Shape>> {
    doc.blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Paragraph { spans }
            | BlockKind::Heading { spans, .. }
            | BlockKind::ListItem { spans, .. } => Some(Shape::Spans(
                spans.iter().map(|s| span_shape(s, &doc.source)).collect(),
            )),
            BlockKind::MathBlock { tex } => Some(Shape::Math(tex.clone())),
            _ => None,
        })
        .collect()
}

/// The page after a press is the page before with its text in another
/// form: the same blocks, the same spans, the formulas and the code
/// identical, the addresses and the styles identical, the text equal up
/// to case. Over the cycle of forms, one press upper-cases every text.
fn check_case(text: &str) -> Result<(), TestCaseError> {
    if text.lines().any(|l| l.starts_with('<')) {
        return Ok(());
    }
    let before = markdown::parse(text);
    let Some(shape_before) = shapes(&before) else {
        return Ok(());
    };
    let mut current = text.to_string();
    let mut upper_seen = false;
    for _press in 0..3 {
        let Some(edit) = case_edit(&current, Some(0..current.len()), current.len(), true) else {
            break;
        };
        prop_assert_eq!(edit.replace, 0..current.len());
        current = edit.text;
        let after = markdown::parse(current.as_str());
        let Some(shape_after) = shapes(&after) else {
            prop_assert!(false, "the page's blocks changed kind: {:?}", current);
            unreachable!();
        };
        prop_assert_eq!(
            shape_after.len(),
            shape_before.len(),
            "the blocks: {:?}",
            current
        );
        let mut all_upper = true;
        for (a, b) in shape_before.iter().zip(&shape_after) {
            match (a, b) {
                (Shape::Math(x), Shape::Math(y)) => {
                    prop_assert_eq!(x, y, "a formula block changed")
                }
                (Shape::Spans(x), Shape::Spans(y)) => {
                    prop_assert_eq!(x.len(), y.len(), "the spans of a block: {:?}", current);
                    for (s, u) in x.iter().zip(y) {
                        prop_assert_eq!(&s.range, &u.range, "a span's source range");
                        prop_assert_eq!(&s.link, &u.link, "an address changed");
                        prop_assert_eq!(&s.image, &u.image, "a picture's source changed");
                        prop_assert_eq!(s.style, u.style, "a style changed");
                        prop_assert_eq!(s.kept, u.kept, "a kept span's kind");
                        if s.kept {
                            prop_assert_eq!(
                                &s.text,
                                &u.text,
                                "a formula or code changed: {:?}",
                                current
                            );
                        } else {
                            prop_assert!(
                                s.text.eq_ignore_ascii_case(&u.text),
                                "text changed beyond its case: {:?} to {:?} in {:?}",
                                s.text,
                                u.text,
                                current
                            );
                            if u.text != s.text.to_ascii_uppercase() {
                                all_upper = false;
                            }
                        }
                    }
                }
                _ => prop_assert!(false, "a block changed kind: {:?}", current),
            }
        }
        upper_seen |= all_upper;
    }
    let has_text = shape_before.iter().any(|s| match s {
        Shape::Spans(v) => v
            .iter()
            .any(|s| !s.kept && s.text.bytes().any(|b| b.is_ascii_lowercase())),
        Shape::Math(_) => false,
    });
    if has_text {
        prop_assert!(
            upper_seen,
            "no press upper-cased the page's text: {:?}",
            text
        );
    }
    Ok(())
}

/// The case key against the page: random paragraphs of letters, dollar
/// signs, braces, backticks, brackets, angle brackets and the rest.
#[test]
#[ignore]
fn fuzz_case() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(text in "[a-z $\\n{}`\\[\\]()<>!/=\"0-9.*-]{0,60}")| {
        check_case(&text)?;
    });
}

/// The patterns of the search fuzzer: greedy runs that would cross the
/// blocks, anchors, word boundaries, and patterns that match empty.
const PATTERNS: &[&str] = &[
    r"[\s\S]+",
    r"(?s).+",
    r"\D+",
    r"[^q]+",
    r"\w+",
    r"[a-z]+ [a-z]+",
    r"a.",
    r"^\w",
    r"\w$",
    r"b*",
    r"e\b",
    r"\bc",
    r"\s+",
    r".",
];

/// One page of paragraphs against the model: the pattern run on each
/// paragraph's own text. The search finds the same non-empty matches,
/// block by block, and the replace walk the same selections.
fn check_search(paragraphs: &[String], pattern: &str) -> Result<(), TestCaseError> {
    let page = paragraphs.join("\n\n") + "\n";
    let doc = markdown::parse(page.as_str());
    let model = fancy_regex::Regex::new(&format!("(?m){pattern}")).expect("a valid pattern");
    let mut expected = Vec::new();
    for (block, text) in paragraphs.iter().enumerate() {
        for found in model.find_iter(text) {
            let found = found.expect("no backtracking limit on these patterns");
            if !found.as_str().is_empty() {
                expected.push((block, found.start(), found.end()));
            }
        }
    }
    let matches = search::regex_matches(&doc, pattern).expect("a valid pattern");
    let mut got = Vec::new();
    for selection in &matches {
        prop_assert_eq!(
            selection.start.block,
            selection.end.block,
            "a match crossed a block"
        );
        prop_assert_eq!(selection.start.span, 0);
        prop_assert_eq!(selection.end.span, 0);
        got.push((
            selection.start.block,
            selection.start.byte,
            selection.end.byte,
        ));
    }
    prop_assert_eq!(&got, &expected, "pattern {:?} on {:?}", pattern, page);
    let paired: Vec<_> = search::regex_replacements(&doc, pattern, "x")
        .expect("a valid pattern")
        .into_iter()
        .map(|(selection, _)| selection)
        .collect();
    prop_assert_eq!(paired, matches, "the replace walk left the search's");
    Ok(())
}

/// The regex search against a plain model: pages of paragraphs of
/// lowercase words, and a pattern from `PATTERNS`.
#[test]
#[ignore]
fn fuzz_search() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        paragraphs in prop::collection::vec("[a-e]{1,4}( [a-e]{1,4}){0,5}", 1..12),
        pattern in prop::sample::select(PATTERNS),
    )| {
        check_search(&paragraphs, pattern)?;
    });
}

/// One edit of a code file's line table against a fresh build: the
/// table spliced in place must hold the rows the loader builds from the
/// edited text.
fn check_lines(text: &str, at: u16, to: u16, insert: &str) -> Result<(), TestCaseError> {
    let kind = FileKind::Code("rust");
    let a = boundary(text, at);
    let b = boundary(text, to).max(a);
    let mut current = text.to_string();
    current.replace_range(a..b, insert);
    let start_line = text[..a].matches('\n').count();
    let touched = start_line..start_line + insert.matches('\n').count() + 1;
    let mut spliced = rendered_document(kind, text);
    prop_assert!(
        splice_document(&mut spliced, &current, a..b, touched).is_some(),
        "a code file takes the splice"
    );
    let fresh = rendered_document(kind, &current);
    let rows = |doc: &Document| -> Vec<String> {
        match &doc.blocks[0].kind {
            BlockKind::CodeBlock { lines, .. } => {
                lines.iter(&doc.source).map(str::to_string).collect()
            }
            _ => Vec::new(),
        }
    };
    prop_assert_eq!(
        rows(&spliced),
        rows(&fresh),
        "{:?} after {:?} at {}..{}",
        text,
        insert,
        a,
        b
    );
    Ok(())
}

/// A code file's line table: random texts of letters, returns and line
/// breaks, one edit spliced in place against a fresh build.
#[test]
#[ignore]
fn fuzz_lines() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        text in "[ab\r\n]{0,30}",
        at in any::<u16>(),
        to in any::<u16>(),
        insert in "[ab\r\n]{0,4}",
    )| {
        check_lines(&text, at, to, &insert)?;
    });
}

/// One step on the coloring of a code block, as the app drives it.
#[derive(Debug, Clone)]
enum Color {
    /// A key: `del` bytes replaced by `text` at a place, the end of the
    /// text when none is given, its walk cut after `walk` lines, as a
    /// budget running out cuts it, or run to the end of the touched
    /// lines.
    Key {
        at: Option<u16>,
        del: u8,
        text: String,
        walk: Option<u8>,
    },
    /// The worker's resume from the frontier, `chunk` lines a chunk,
    /// killed after a few chunks or run to the end.
    Sweep { chunk: u8, deliver: Option<u8> },
    /// A caret seam asked for the line at a place, landed.
    Caret { at: u16 },
    /// A guess over `len` lines from a place, landed.
    Guess { at: u16, len: u8 },
    /// A line marked slow by hand, as a parse over the budget marks it.
    Slow { at: u16 },
    /// A key on the fallback arm: the document reparsed whole, the table
    /// shifted, no walk.
    Fallback {
        at: Option<u16>,
        del: u8,
        text: String,
    },
}

fn color_step() -> impl Strategy<Value = Color> {
    prop_oneof![
        4 => (prop::option::of(any::<u16>()), 0..8u8, "[a*/\" \n]{0,3}", prop::option::of(0..6u8))
            .prop_map(|(at, del, text, walk)| Color::Key { at, del, text, walk }),
        3 => (1..6u8, prop::option::of(0..4u8))
            .prop_map(|(chunk, deliver)| Color::Sweep { chunk, deliver }),
        1 => any::<u16>().prop_map(|at| Color::Caret { at }),
        1 => (any::<u16>(), 1..8u8).prop_map(|(at, len)| Color::Guess { at, len }),
        1 => any::<u16>().prop_map(|at| Color::Slow { at }),
        1 => (prop::option::of(any::<u16>()), 0..8u8, "[a*/\" \n]{0,3}")
            .prop_map(|(at, del, text)| Color::Fallback { at, del, text }),
    ]
}

/// The bytes a key replaces: from a place, the end of the text when
/// none is given, over `del` bytes, cut to the text and to a character
/// boundary.
fn edit_span(text: &str, at: Option<u16>, del: u8) -> std::ops::Range<usize> {
    let a = at.map_or(text.len(), |at| boundary(text, at));
    let mut b = (a + del as usize).min(text.len());
    while !text.is_char_boundary(b) {
        b += 1;
    }
    a..b
}

const COLOR_KIND: FileKind = FileKind::Code("rust");

/// The coloring of one code file driven step by step: the document,
/// its seam table and the ledger, the way the app holds them.
struct Coloring {
    doc: Document,
    table: oryx::style::seams::Table,
    led: Ledger,
    key_lines: std::ops::Range<usize>,
}

fn arrival_of(c: oryx::style::highlight::Chunk) -> oryx::style::highlight::Arrival {
    oryx::style::highlight::Arrival {
        block: 0,
        start_line: c.start_line,
        spans: c.spans,
        seam: Some(c.seam),
        converged: c.converged,
        reach: c.reach,
        speculative: false,
        slow: c.slow,
    }
}

fn code_parts(
    doc: &Document,
) -> (
    &oryx::doc::model::CodeBody,
    &Vec<oryx::style::highlight::LineSpans>,
    usize,
) {
    match &doc.blocks[0].kind {
        BlockKind::CodeBlock {
            lines,
            highlights,
            exact,
            ..
        } => (lines, highlights, *exact),
        _ => panic!("a code file is one code block"),
    }
}

/// The full parse of the block: the spans of every line, and the state
/// before each line and after the last.
fn oracle(
    doc: &Document,
) -> (
    Vec<oryx::style::highlight::LineSpans>,
    Vec<oryx::style::highlight::Seam>,
) {
    use oryx::style::highlight::{spans_chunked, walk};
    let (lines, _, _) = code_parts(doc);
    let mut rows = Vec::new();
    let initial = walk(
        &doc.source,
        lines,
        Some("rust"),
        0,
        None,
        0,
        0,
        Duration::MAX,
        &[],
        || true,
    )
    .seam;
    let mut states = vec![initial];
    spans_chunked(
        &doc.source,
        lines,
        Some("rust"),
        1,
        Duration::MAX,
        None,
        || true,
        |c| {
            rows.extend(c.spans);
            states.push(c.seam);
            true
        },
    );
    (rows, states)
}

/// The worker's sweep in miniature: the resume from the frontier,
/// chunks folded in order, killed after `deliver` chunks when given.
fn sweep_into(
    doc: &mut Document,
    table: &mut oryx::style::seams::Table,
    chunk: usize,
    cuts: &[usize],
    deliver: Option<usize>,
) {
    let (lines, _, exact) = code_parts(doc);
    let lines = lines.clone();
    let resume = table.resume(exact, cuts);
    let source = Arc::clone(&doc.source);
    let mut chunks = Vec::new();
    oryx::style::highlight::spans_chunked(
        &source,
        &lines,
        Some("rust"),
        chunk,
        Duration::MAX,
        Some(&resume),
        || true,
        |c| {
            chunks.push(c);
            deliver.is_none_or(|n| chunks.len() <= n)
        },
    );
    for c in chunks {
        table.fold(doc, &arrival_of(c));
    }
}

impl Coloring {
    fn new(text: &str) -> Coloring {
        let doc = rendered_document(COLOR_KIND, text);
        let led = Ledger::new(Arc::clone(&doc.source), Vec::new());
        Coloring {
            doc,
            table: oryx::style::seams::Table::default(),
            led,
            key_lines: 0..0,
        }
    }

    /// The line of a place, the last line past the end.
    fn line_at(&self, at: u16) -> usize {
        let (lines, _, _) = code_parts(&self.doc);
        let offset = boundary(&self.doc.source, at);
        lines
            .line_at(offset)
            .unwrap_or(0)
            .min(lines.len().saturating_sub(1))
    }

    fn step(&mut self, step: &Color) -> Result<(), TestCaseError> {
        match step {
            Color::Key {
                at,
                del,
                text,
                walk,
            } => {
                let span = edit_span(&self.doc.source, *at, *del);
                if span.is_empty() && text.is_empty() {
                    return Ok(());
                }
                let exact0 = code_parts(&self.doc).2;
                let touched = self.led.edit(span.clone(), text);
                let current = self.led.current();
                let spliced = splice_document(&mut self.doc, &current, span, touched);
                let Some((old, new)) = spliced else {
                    prop_assert!(false, "a code file takes the splice");
                    unreachable!()
                };
                self.key_lines = new.clone();
                let source = Arc::clone(&self.doc.source);
                let mut left = walk.map(|n| n as usize);
                let BlockKind::CodeBlock {
                    language,
                    lines,
                    highlights,
                    exact,
                } = &mut self.doc.blocks[0].kind
                else {
                    unreachable!()
                };
                self.table.on_key(
                    &source,
                    lines,
                    language.as_deref(),
                    highlights,
                    exact,
                    exact0,
                    &old,
                    &new,
                    Duration::MAX,
                    || match left.as_mut() {
                        Some(0) => false,
                        Some(n) => {
                            *n -= 1;
                            true
                        }
                        None => true,
                    },
                );
            }
            Color::Sweep { chunk, deliver } => {
                let cuts = [self.key_lines.start, self.key_lines.end];
                sweep_into(
                    &mut self.doc,
                    &mut self.table,
                    *chunk as usize,
                    &cuts,
                    deliver.map(|n| n as usize),
                );
            }
            Color::Caret { at } => {
                let line = self.line_at(*at);
                let (lines, _, exact) = code_parts(&self.doc);
                let Some((start, seam)) = self.table.caret_ask(exact, line, 2) else {
                    return Ok(());
                };
                let walked = oryx::style::highlight::walk(
                    &self.doc.source,
                    lines,
                    Some("rust"),
                    start,
                    seam,
                    line,
                    line,
                    Duration::MAX,
                    &[],
                    || true,
                );
                let arrival = oryx::style::highlight::Arrival {
                    block: 0,
                    start_line: line,
                    spans: Vec::new(),
                    seam: Some(walked.seam),
                    converged: false,
                    reach: 0,
                    speculative: false,
                    slow: Vec::new(),
                };
                self.table.fold(&mut self.doc, &arrival);
            }
            Color::Guess { at, len } => {
                let (lines, _, exact) = code_parts(&self.doc);
                // As the app: no guess while a seam stands past the
                // frontier, where a converged stop would vouch for it.
                if self.table.past_frontier(exact) {
                    return Ok(());
                }
                let start = self.line_at(*at);
                let end = (start + *len as usize).min(lines.len());
                if start >= end {
                    return Ok(());
                }
                let spans = oryx::style::highlight::spans_band(
                    &self.doc.source,
                    lines,
                    Some("rust"),
                    start..end,
                );
                let arrival = oryx::style::highlight::Arrival {
                    block: 0,
                    start_line: start,
                    spans,
                    seam: None,
                    converged: false,
                    reach: 0,
                    speculative: true,
                    slow: Vec::new(),
                };
                self.table.fold(&mut self.doc, &arrival);
            }
            Color::Slow { at } => {
                let line = self.line_at(*at);
                if line < code_parts(&self.doc).0.len() {
                    self.table.mark_slow(line);
                }
            }
            Color::Fallback { at, del, text } => {
                let span = edit_span(&self.doc.source, *at, *del);
                if span.is_empty() && text.is_empty() {
                    return Ok(());
                }
                let removed = self.doc.source[span.clone()].matches('\n').count();
                let old_len = code_parts(&self.doc).0.len();
                let touched = self.led.edit(span, text);
                let current = self.led.current();
                let old_touched = touched.start..touched.start + removed + 1;
                self.doc = oryx::edit::reparse(
                    COLOR_KIND,
                    &current,
                    &self.doc,
                    old_touched.clone(),
                    touched.clone(),
                );
                let new_len = code_parts(&self.doc).0.len();
                let (old_eff, new_eff) =
                    oryx::edit::effective_lines(old_len, new_len, &old_touched, &touched);
                self.table.shift(&old_eff, &new_eff);
                self.key_lines = new_eff;
            }
        }
        Ok(())
    }

    /// The invariants against the full parse: the rows and the seams at
    /// or before the frontier are the full parse's; a chained seam
    /// whose state is the full parse's has its rows and its next seam
    /// equal to the full parse's; the seams stand within the rows.
    fn check(&self, what: &str) -> Result<(), TestCaseError> {
        let (lines, highlights, exact) = code_parts(&self.doc);
        let (rows, states) = oracle(&self.doc);
        prop_assert!(
            exact <= highlights.len() && highlights.len() <= lines.len(),
            "{}: the frontier {} within the rows {} within the lines {}",
            what,
            exact,
            highlights.len(),
            lines.len()
        );
        for i in 0..exact {
            prop_assert!(
                highlights[i] == rows[i],
                "{}: row {} before the frontier {}: {:?} against {:?}",
                what,
                i,
                exact,
                highlights[i],
                rows[i]
            );
        }
        let seams = self.table.seams();
        for (i, f) in seams.iter().enumerate() {
            prop_assert!(
                f.line <= highlights.len(),
                "{}: the seam at {} within the rows",
                what,
                f.line
            );
            if f.line <= exact {
                prop_assert!(
                    f.seam == states[f.line],
                    "{}: the seam at {} before the frontier {} holds the full parse's state",
                    what,
                    f.line,
                    exact
                );
            }
            if !f.chained || f.seam != states[f.line] {
                continue;
            }
            let next = seams.get(i + 1);
            let end = next.map_or(lines.len(), |n| n.line);
            for l in f.line..end {
                prop_assert!(
                    highlights[l] == rows[l],
                    "{}: the chained seam at {} whose state holds vouches for row {}",
                    what,
                    f.line,
                    l
                );
            }
            if let Some(n) = next {
                prop_assert!(
                    n.seam == states[n.line],
                    "{}: the chained seam at {} whose state holds vouches for the seam at {}",
                    what,
                    f.line,
                    n.line
                );
            }
        }
        Ok(())
    }
}

/// A resume run to its end from the state as it stands leaves every
/// row and every seam equal to the full parse's.
fn settle(
    mut doc: Document,
    mut table: oryx::style::seams::Table,
    what: &str,
) -> Result<(), TestCaseError> {
    sweep_into(&mut doc, &mut table, 7, &[], None);
    let (rows, states) = oracle(&doc);
    let (lines, highlights, exact) = code_parts(&doc);
    prop_assert!(
        exact == lines.len(),
        "{}: the frontier reaches the end after the resume ({} of {})",
        what,
        exact,
        lines.len()
    );
    let wrong: Vec<usize> = (0..lines.len())
        .filter(|i| highlights[*i] != rows[*i])
        .collect();
    prop_assert!(
        wrong.is_empty(),
        "{}: rows wrong after the resume: {:?}",
        what,
        wrong
    );
    for f in table.seams() {
        prop_assert!(
            f.seam == states[f.line],
            "{}: the seam at {} after the resume holds the full parse's state",
            what,
            f.line
        );
    }
    Ok(())
}

fn run_coloring(text: &str, steps: &[Color]) -> Result<(), TestCaseError> {
    let mut c = Coloring::new(text);
    c.check("at the open")?;
    for (i, step) in steps.iter().enumerate() {
        c.step(step)?;
        let what = format!("after step {i}, {step:?}");
        c.check(&what)?;
        settle(c.doc.clone(), c.table.clone(), &what)?;
    }
    Ok(())
}

/// The coloring of a code block against a full parse: random texts of
/// Rust lines, comments and strings among them, and random keys, among
/// them deletions and edits at the very end of the text, walks cut
/// short, sweeps killed early, caret seams, guesses, slow marks and
/// fallback reparses, the invariants of the seam table checked after
/// each step and a resume run to the end from each state.
#[test]
#[ignore]
fn fuzz_seams() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        text in "((let a = 1;|/\\*|\\*/|\" x|fn f\\(\\) \\{|\\}|)\n){3,16}",
        steps in prop::collection::vec(color_step(), 1..8),
    )| {
        run_coloring(&text, &steps)?;
    });
}

/// One step of a selection drag held near the view's edge.
#[derive(Debug, Clone, Copy)]
enum EdgeStep {
    /// The pointer moves to a y: inside the view or past an edge.
    Move(f32),
    /// A tick, that many milliseconds after the last step.
    Tick(u16),
    /// The drag ends.
    Stop,
}

fn edge_step() -> impl Strategy<Value = EdgeStep> {
    prop_oneof![
        (-200f32..800f32).prop_map(EdgeStep::Move),
        (0u16..200).prop_map(EdgeStep::Tick),
        Just(EdgeStep::Stop),
    ]
}

/// The edge scroll against a plain model: a view of `view_h` pixels on
/// a document of `doc_h`, the pointer moving, passes of the loop and
/// the drag's end. The view moves only while the drag is live and the
/// pointer is at or past an edge, at a pass 16 ms or more after the
/// last tick or the arrival at the edge, toward that edge, by the speed
/// of the pixels past it times that time, and it stays within the
/// document.
fn check_edge_scroll(view_h: f32, doc_h: f32, steps: &[EdgeStep]) -> Result<(), TestCaseError> {
    let row_h = 24.0;
    let mut now = Instant::now();
    let mut edge = EdgeScroll::default();
    let mut scroll_y = scroll::clamp(doc_h / 2.0, doc_h, view_h);
    let mut at: Option<(Edge, f32)> = None;
    let mut since: Option<Instant> = None;
    for step in steps {
        match *step {
            EdgeStep::Move(y) => {
                edge.moved(y, view_h, now);
                at = scroll::edge_of(y, view_h);
                since = at.map(|_| since.unwrap_or(now));
                prop_assert_eq!(edge.running(), at.is_some(), "moved to {}", y);
            }
            EdgeStep::Tick(ms) => {
                now += Duration::from_millis(u64::from(ms));
                let got = edge.tick(now, row_h, view_h);
                let Some((side, past)) = at else {
                    prop_assert_eq!(got, None, "idle, no step");
                    continue;
                };
                let clock = since.expect("the model's clock runs with its edge");
                if now < clock + scroll::EDGE_SCROLL_TICK {
                    prop_assert_eq!(got, None, "not due, no step");
                    continue;
                }
                let elapsed = (now - clock).as_secs_f32();
                since = Some(now);
                let rows = scroll::edge_speed(past / row_h, view_h / row_h) * elapsed;
                let expected = match side {
                    Edge::Top => -rows * row_h,
                    Edge::Bottom => rows * row_h,
                };
                let Some(step) = got else {
                    prop_assert!(false, "no step while {:?} by {} px", side, past);
                    unreachable!();
                };
                prop_assert!(
                    (step - expected).abs() <= 0.001 * expected.abs().max(1.0),
                    "{:?} by {} px after {} ms: {} px, not {}",
                    side,
                    past,
                    ms,
                    step,
                    expected
                );
                scroll_y = scroll::clamp(scroll_y + step, doc_h, view_h);
                prop_assert!(
                    scroll_y >= 0.0 && scroll_y <= (doc_h - view_h).max(0.0),
                    "the view left the document: {}",
                    scroll_y
                );
            }
            EdgeStep::Stop => {
                edge.stop();
                at = None;
                since = None;
                prop_assert!(!edge.running(), "stopped");
            }
        }
    }
    Ok(())
}

/// The edge scroll of a selection drag: views of 200 to 1200 pixels on
/// documents up to 12,000, random moves of the pointer inside or past
/// the edges, ticks of up to 200 ms and the drag's end, against the
/// plain model.
#[test]
#[ignore]
fn fuzz_edge_scroll() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        view_h in 200f32..1200f32,
        doc_h in 100f32..12000f32,
        steps in prop::collection::vec(edge_step(), 1..40),
    )| {
        check_edge_scroll(view_h, doc_h, &steps)?;
    });
}

/// The math font noad lays out with, the one the app ships.
const MATH_FONT: &[u8] = include_bytes!("../assets/fonts/STIXTwoMath-Regular.otf");

/// What a mutation inserts into a formula: TeX's syntax characters and a
/// character of two bytes.
const MATH_SPECIALS: &[char] = &[
    '\\', '{', '}', '^', '_', '&', '#', '[', ']', '%', '\'', '$', '~', 'é',
];
const MATH_SYMBOLS: &[&str] = &[
    "+", "-", "=", "<", ">", ",", ";", ":", "!", "(", ")", "[", "]", "|", "/", "*", ".",
];
const MATH_DELIMS: &[&str] = &["(", ")", "[", "]", "|", "."];
const MATH_FRACTIONS: &[&str] = &[
    "frac", "tfrac", "dfrac", "cfrac", "binom", "tbinom", "dbinom",
];
const MATH_ENVIRONMENTS: &[&str] = &[
    "matrix",
    "pmatrix",
    "bmatrix",
    "Bmatrix",
    "vmatrix",
    "Vmatrix",
    "smallmatrix",
    "cases",
];
const MATH_ALPHABETS: &[&str] = &[
    "mathbb",
    "mathbf",
    "mathcal",
    "mathfrak",
    "mathit",
    "mathsf",
    "mathtt",
    "boldsymbol",
    "bm",
    "Bbb",
    "bold",
    "frak",
    "mathscr",
    "scr",
    "mathnormal",
];
const MATH_TEXT: &[&str] = &[
    "text",
    "mathrm",
    "textrm",
    "textnormal",
    "textup",
    "hbox",
    "mbox",
];
const MATH_CLASSES: &[&str] = &[
    "mathop",
    "mathrel",
    "mathbin",
    "mathord",
    "mathopen",
    "mathclose",
    "mathpunct",
    "mathinner",
];
const MATH_MODULO: &[&str] = &["pmod", "mod", "bmod"];
/// The boxes of category D (Task 353): the lines and the phantoms over one
/// operand, the braces with a limit, the stacks, the long arrows, the
/// symbols `\not` negates (one with no negated form among them), and the
/// colors.
const MATH_LINES: &[&str] = &["overline", "underline", "phantom", "vphantom", "hphantom"];
const MATH_BRACES: &[&str] = &["overbrace", "underbrace"];
const MATH_STACKS: &[&str] = &["stackrel", "overset", "underset"];
const MATH_ARROWS: &[&str] = &[
    "xrightarrow",
    "xleftarrow",
    "xRightarrow",
    "xLeftarrow",
    "xleftrightarrow",
    "xLeftrightarrow",
    "xmapsto",
];
const MATH_NEGATED: &[&str] = &["\\in", "=", "<", "\\equiv", "\\subset", "\\leq", "\\alpha"];
/// The colors by name alone: a `#rgb` form inside a macro's body reads as
/// a parameter mark, in TeX as in noad, and the generator nests freely.
const MATH_COLORS: &[&str] = &["red", "blue", "teal", "gray", "violet", "black"];
/// TeX's infix fraction words and its switches, which act on the group
/// they stand in (Task 352).
const MATH_INFIX: &[&str] = &["over", "atop", "choose", "brace", "brack"];
const MATH_SWITCHES: &[&str] = &[
    "bf",
    "rm",
    "it",
    "sf",
    "tt",
    "cal",
    "displaystyle",
    "textstyle",
    "scriptstyle",
    "scriptscriptstyle",
];
/// The sixteen words of TeX's sized delimiters (Task 351).
const MATH_SIZES: &[&str] = &[
    "big", "Big", "bigg", "Bigg", "bigl", "Bigl", "biggl", "Biggl", "bigr", "Bigr", "biggr",
    "Biggr", "bigm", "Bigm", "biggm", "Biggm",
];
/// The commands that read one operand, the place a macro expanding to
/// nothing used to crash.
const MATH_OPERAND_TAKERS: &[&str] = &[
    "sqrt",
    "mathbf",
    "hat",
    "mathop",
    "mathrel",
    "pmod",
    "boldsymbol",
    "tfrac1",
    "frac1",
];
const MATH_SPACES: &[&str] = &[
    ",",
    ":",
    ";",
    "!",
    " ",
    "quad",
    "qquad",
    "enspace",
    "thinspace",
    "medspace",
    "thickspace",
    "negthinspace",
    "negmedspace",
    "negthickspace",
    "nobreakspace",
    "space",
];

/// One leaf of a formula: a letter, a digit, a symbol command, an
/// operator name or a plain symbol.
fn math_leaf() -> impl Strategy<Value = String> {
    use noad::parse::{OP_NAMES, VOCABULARY};
    prop_oneof![
        4 => "[a-zA-Z]".prop_map(String::from),
        2 => "[0-9]".prop_map(String::from),
        3 => (0..VOCABULARY.len()).prop_map(|i| format!("\\{} ", VOCABULARY[i].0)),
        1 => (0..OP_NAMES.len()).prop_map(|i| format!("\\{} ", OP_NAMES[i].0)),
        1 => prop::sample::select(MATH_SYMBOLS).prop_map(String::from),
    ]
}

/// A delimiter after `\left` or `\right`: a character or a command.
fn math_delimiter() -> impl Strategy<Value = String> {
    use noad::parse::DELIMITERS;
    prop_oneof![
        3 => prop::sample::select(MATH_DELIMS).prop_map(String::from),
        2 => (0..DELIMITERS.len()).prop_map(|i| format!("\\{}", DELIMITERS[i].0)),
    ]
}

/// A well-formed formula over the vocabulary noad knows, nested a few
/// levels deep. Each math category adds its productions here.
fn math_formula() -> impl Strategy<Value = String> {
    use noad::parse::ACCENTS;
    math_leaf().prop_recursive(3, 48, 4, |inner| {
        let i = move || inner.clone();
        prop_oneof![
            3 => (i(), i()).prop_map(|(a, b)| format!("{a} {b}")),
            2 => i().prop_map(|a| format!("{{{a}}}")),
            2 => (prop::sample::select(MATH_FRACTIONS), i(), i())
                .prop_map(|(f, a, b)| format!("\\{f}{{{a}}}{{{b}}}")),
            1 => i().prop_map(|a| format!("\\sqrt{{{a}}}")),
            // The degree is braced: a `]` inside it would end the optional
            // argument, in LaTeX as here.
            1 => (i(), i()).prop_map(|(a, b)| format!("\\sqrt[{{{a}}}]{{{b}}}")),
            2 => (i(), i()).prop_map(|(a, b)| format!("{{{a}}}^{{{b}}}")),
            2 => (i(), i()).prop_map(|(a, b)| format!("{{{a}}}_{{{b}}}")),
            1 => (i(), i(), i()).prop_map(|(a, b, c)| format!("{{{a}}}^{{{b}}}_{{{c}}}")),
            1 => (i(), i(), i()).prop_map(|(a, b, c)| format!("\\sum_{{{a}}}^{{{b}}} {c}")),
            1 => (i(), i()).prop_map(|(a, b)| format!("\\lim_{{{a}}} {b}")),
            1 => i().prop_map(|a| format!("{a}'")),
            2 => (math_delimiter(), i(), math_delimiter())
                .prop_map(|(l, a, r)| format!("\\left{l} {a} \\right{r}")),
            1 => (prop::sample::select(MATH_ENVIRONMENTS), i(), i(), i(), i()).prop_map(
                |(e, a, b, c, d)| format!("\\begin{{{e}}} {a} & {b} \\\\ {c} & {d} \\end{{{e}}}")
            ),
            1 => (i(), i()).prop_map(|(a, b)| {
                format!("\\begin{{aligned}} {a} &= {b} \\\\ {b} &= {a} \\end{{aligned}}")
            }),
            1 => (0..ACCENTS.len(), i()).prop_map(|(k, a)| format!("\\{}{{{a}}}", ACCENTS[k].0)),
            2 => (prop::sample::select(MATH_ALPHABETS), i())
                .prop_map(|(al, a)| format!("\\{al}{{{a}}}")),
            1 => (prop::sample::select(MATH_TEXT), "[a-z ]{1,6}")
                .prop_map(|(t, s)| format!("\\{t}{{{s}}}")),
            1 => "[a-z]".prop_map(|t| format!("\\mathrm {t}")),
            1 => "[a-z]{1,5}".prop_map(|t| format!("\\operatorname{{{t}}}")),
            1 => (prop::sample::select(MATH_CLASSES), i())
                .prop_map(|(s, a)| format!("\\{s}{{{a}}}")),
            1 => (prop::sample::select(MATH_MODULO), i(), i())
                .prop_map(|(m, a, b)| format!("{a} \\{m}{{{b}}}")),
            1 => (prop::sample::select(MATH_SPACES), i(), i())
                .prop_map(|(s, a, b)| format!("{a}\\{s} {b}")),
            1 => (i(), i()).prop_map(|(a, b)| format!("{a}~{b}")),
            // A macro defined and used: as an atom, and as the operand of
            // a construct, its body a formula or nothing (Task 350).
            2 => ("[A-Z]{2,3}", prop::option::of(i()), i(), prop::sample::select(MATH_OPERAND_TAKERS))
                .prop_map(|(name, body, a, taker)| {
                    let body = body.unwrap_or_default();
                    format!("\\newcommand{{\\{name}}}{{{body}}} \\{name} {a} \\{taker}\\{name}")
                }),
            // A macro with a parameter, its argument mixed into a text or
            // a formula of the body (Task 350).
            1 => ("[A-Z]{2,3}", i(), i(), any::<bool>()).prop_map(|(name, body, arg, in_text)| {
                let body = if in_text { format!("\\text{{#1 {body}}}") } else { format!("#1 {body}") };
                format!("\\newcommand{{\\{name}}}[1]{{{body}}} \\{name}{{{arg}}}")
            }),
            // The boxes of category D (Task 353).
            1 => (prop::sample::select(MATH_LINES), i()).prop_map(|(w, a)| format!("\\{w}{{{a}}}")),
            1 => (prop::sample::select(MATH_BRACES), i(), i()).prop_map(|(w, a, b)| {
                let marker = if w == "overbrace" { '^' } else { '_' };
                format!("\\{w}{{{a}}}{marker}{{{b}}}")
            }),
            1 => (prop::sample::select(MATH_STACKS), i(), i())
                .prop_map(|(w, a, b)| format!("\\{w}{{{a}}}{{{b}}}")),
            1 => (prop::sample::select(MATH_ARROWS), i(), prop::option::of(i())).prop_map(
                |(w, a, b)| match b {
                    Some(b) => format!("\\{w}[{{{b}}}]{{{a}}}"),
                    None => format!("\\{w}{{{a}}}"),
                },
            ),
            1 => prop::sample::select(MATH_NEGATED).prop_map(|r| format!("a \\not{r} b")),
            1 => (prop::sample::select(MATH_COLORS), i(), i())
                .prop_map(|(c, a, b)| format!("{{\\color{{{c}}} {a}}} \\textcolor{{{c}}}{{{b}}}")),
            // An infix word between two halves of a group, and a switch over
            // the rest of one (Task 352).
            2 => (prop::sample::select(MATH_INFIX), i(), i())
                .prop_map(|(w, a, b)| format!("{{{a} \\{w} {b}}}")),
            1 => (prop::sample::select(MATH_SWITCHES), i(), i())
                .prop_map(|(w, a, b)| format!("{{{a} \\{w} {b}}}")),
            // A sized delimiter on each side of a formula (Task 351).
            2 => (
                prop::sample::select(MATH_SIZES),
                math_delimiter(),
                i(),
                prop::sample::select(MATH_SIZES),
                math_delimiter(),
            )
                .prop_map(|(s1, l, a, s2, r)| format!("\\{s1}{l} {a} \\{s2}{r}")),
        ]
    })
}

/// One mutation of a formula: a truncation, a deletion, an insertion of
/// a syntax character, a duplicated run or a swap, on character
/// boundaries.
fn mutate_math(src: &str, kind: u8, at: u16, ch: char) -> String {
    let mut out: Vec<char> = src.chars().collect();
    if out.is_empty() {
        return String::new();
    }
    let i = at as usize % out.len();
    match kind % 5 {
        0 => out.truncate(i),
        1 => {
            out.remove(i);
        }
        2 => out.insert(i, ch),
        3 => {
            let j = (i * 7 + 3) % out.len();
            let (a, b) = (i.min(j), i.max(j));
            let run: Vec<char> = out[a..b].to_vec();
            out.splice(a..a, run);
        }
        _ => {
            let j = (i * 5 + 1) % out.len();
            out.swap(i, j);
        }
    }
    out.into_iter().collect()
}

/// Every span of a math list lies inside the source on character
/// boundaries, and a nucleus inside its atom.
fn check_math_spans(list: &noad::mlist::MathList, tex: &str) -> Result<(), TestCaseError> {
    use noad::mlist::Field;
    for atom in list.atoms() {
        for span in [&atom.span, &atom.nucleus_span] {
            prop_assert!(
                span.start <= span.end
                    && span.end <= tex.len()
                    && tex.is_char_boundary(span.start)
                    && tex.is_char_boundary(span.end),
                "span {span:?} in {tex:?}"
            );
        }
        prop_assert!(
            atom.span.start <= atom.nucleus_span.start && atom.nucleus_span.end <= atom.span.end,
            "nucleus {:?} outside its atom {:?} in {tex:?}",
            atom.nucleus_span,
            atom.span
        );
        match &atom.nucleus {
            Field::List(l) => check_math_spans(l, tex)?,
            Field::Fraction {
                numerator,
                denominator,
                ..
            } => {
                check_math_spans(numerator, tex)?;
                check_math_spans(denominator, tex)?;
            }
            Field::Radical { radicand, degree } => {
                check_math_spans(radicand, tex)?;
                if let Some(d) = degree {
                    check_math_spans(d, tex)?;
                }
            }
            Field::LeftRight { body, .. }
            | Field::Phantom { body, .. }
            | Field::Colored { body, .. } => check_math_spans(body, tex)?,
            Field::Line { base, .. } | Field::Brace { base, .. } => check_math_spans(base, tex)?,
            Field::Arrow { above, below, .. } => {
                check_math_spans(above, tex)?;
                check_math_spans(below, tex)?;
            }
            Field::Accent { base, .. } => check_math_spans(base, tex)?,
            Field::Table { rows, .. } => {
                for row in rows {
                    for cell in row {
                        check_math_spans(cell, tex)?;
                    }
                }
            }
            Field::Symbol(_)
            | Field::Sized { .. }
            | Field::Text(_)
            | Field::Kern(_)
            | Field::Literal(_)
            | Field::Empty => {}
        }
        if let Some(s) = &atom.sup {
            check_math_spans(s, tex)?;
        }
        if let Some(s) = &atom.sub {
            check_math_spans(s, tex)?;
        }
    }
    Ok(())
}

/// The properties of a formula: parsed whole when `whole` (no literal),
/// its spans inside the source, its geometry finite and bounded in both
/// styles, every glyph's source inside the formula.
fn check_math(tex: &str, font: &noad::font::TtfMathFont, whole: bool) -> Result<(), TestCaseError> {
    use noad::layout::{layout, MathStyle};
    let list = noad::parse::parse(tex);
    if whole {
        let literals = list.literals();
        prop_assert!(literals.is_empty(), "literals {literals:?} in {tex:?}");
    }
    check_math_spans(&list, tex)?;
    // A glyph is at most a few ems; a formula cannot be wider or taller
    // than a few ems per byte of its source. A formula that defines a
    // macro can multiply its content up to noad's expansion cap (a
    // mutation can make a macro call itself), so it is held to finite
    // geometry alone.
    let bound = if tex.contains("\\newcommand") {
        f32::INFINITY
    } else {
        60.0 * (tex.len() as f32 + 4.0)
    };
    for style in [MathStyle::Display, MathStyle::Text] {
        let l = layout(tex, style, 20.0, font);
        for v in [l.width, l.ascent, l.descent] {
            prop_assert!(
                v.is_finite() && v >= 0.0 && v <= bound,
                "geometry {v} of {tex:?}"
            );
        }
        let sources = l
            .glyphs
            .iter()
            .map(|g| g.source.clone())
            .chain(l.literals.iter().map(|t| t.source.clone()));
        for source in sources {
            prop_assert!(
                source.start <= source.end
                    && source.end <= tex.len()
                    && tex.is_char_boundary(source.start)
                    && tex.is_char_boundary(source.end),
                "source {source:?} in {tex:?}"
            );
        }
    }
    Ok(())
}

/// The math fuzzer (Task 349): well-formed formulas from the vocabulary
/// noad knows, nested, against four properties: no panic, no literal,
/// finite and bounded geometry in both styles, every span inside the
/// source on a character boundary. The formula mutated keeps the first
/// and the last.
#[test]
#[ignore]
fn fuzz_math() {
    let font = noad::font::TtfMathFont::from_bytes(MATH_FONT).expect("the math font");
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        tex in math_formula(),
        kind in 0..5u8,
        at in any::<u16>(),
        ch in prop::sample::select(MATH_SPECIALS),
    )| {
        check_math(&tex, &font, true)?;
        check_math(&mutate_math(&tex, kind, at, ch), &font, false)?;
    });
}
