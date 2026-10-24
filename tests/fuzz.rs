//! The fuzzers: random steps against a plain model. Ignored under
//! `make check`; `make fuzz` runs them in the debug profile, where the
//! assertions fire, and `PROPTEST_CASES=5000 make fuzz` runs them deep.
//! A failing case is shrunk by proptest and printed with its seed, and
//! is then kept as a plain test beside the code it caught.

use oryx::edit::splice::{Ledger, Placed, Splice};
use oryx::edit::undo::{Kind, Undo};
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
    Insert { at: u16, text: String, rest: bool },
    Delete { at: u16, to: u16, rest: bool },
    Undo,
    Redo,
    Park,
    Save,
}

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
    now: Instant,
}

impl Session {
    fn apply(&mut self, splice: &Splice) {
        self.led.edit(splice.range.clone(), &splice.text);
        if let Some(park) = self.park.as_mut() {
            park.ledger.splice(splice.range.clone(), &splice.text);
        }
        self.model.replace_range(splice.range.clone(), &splice.text);
    }

    fn edit(&mut self, range: std::ops::Range<usize>, text: &str, kind: Kind, rest: bool) {
        if rest {
            self.now += Duration::from_secs(2);
        }
        let replaced = self.model[range.clone()].to_string();
        let caret = (range.end, range.start + text.len());
        self.apply(&Splice {
            range: range.clone(),
            text: text.to_string(),
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
                if let Some((splice, _)) = self.undo.undo() {
                    self.apply(&splice);
                }
            }
            Step::Redo => {
                if let Some((splice, _)) = self.undo.redo() {
                    self.apply(&splice);
                }
            }
            Step::Park => self.park = Some(Park::at(&self.model)),
            Step::Save => {
                self.led.commit();
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
        let emitted = String::from_utf8(self.led.emit()).expect("utf-8 out");
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
/// undos, redos, crossings and saves against a plain string.
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
