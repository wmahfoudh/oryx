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
