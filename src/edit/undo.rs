//! Undo: the edit history as units over the splice ledger.
//!
//! A unit stores the splice as it was applied, the text it replaced,
//! and the caret on both sides. Consecutive typing coalesces into one
//! unit, broken by a second of rest, a caret jump, or a structural
//! operation, which is always a unit of its own; saving also closes the
//! open unit, so the save point never sits inside one. Undo hands back
//! the inverse splice and the caret to restore, redo the forward splice
//! and the caret after it; the caller applies them to the ledger.
//!
//! The save point is a stack position. The document is dirty whenever
//! the head stands elsewhere, which stays correct through undo past the
//! save point; recording truncates the redo tail, and a truncated-off
//! save point leaves the document dirty until the next save.

use std::ops::Range;
use std::time::{Duration, Instant};

use super::splice::Splice;

/// The rest that closes an open typing unit.
const REST: Duration = Duration::from_secs(1);

/// How an edit coalesces. Typing and single-character deletion chain
/// into the standing unit; a structural edit stands alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Insert,
    Delete,
    Structural,
}

/// One undoable edit. `forward` speaks the text as it stood before the
/// unit applied; `replaced` is what its range held, so the inverse is
/// derivable and both directions replay as ordinary ledger edits.
struct Unit {
    forward: Splice,
    replaced: String,
    caret: (usize, usize),
    kind: Kind,
}

pub struct Undo {
    units: Vec<Unit>,
    /// Units currently applied; the stack above it is the redo tail.
    head: usize,
    /// The head that matches the file on disk; None when the saved
    /// state was truncated off the stack.
    save: Option<usize>,
    /// When the head unit last grew; None closes it to coalescing.
    last: Option<Instant>,
}

impl Default for Undo {
    fn default() -> Undo {
        Undo::new()
    }
}

impl Undo {
    pub fn new() -> Undo {
        Undo {
            units: Vec::new(),
            head: 0,
            save: Some(0),
            last: None,
        }
    }

    /// Records one applied edit: `range` and `text` as handed to the
    /// ledger, `replaced` the current text the range held before it,
    /// `caret` the offsets on both sides of the edit.
    pub fn record(
        &mut self,
        range: Range<usize>,
        text: &str,
        replaced: &str,
        caret: (usize, usize),
        kind: Kind,
        now: Instant,
    ) {
        if self.head < self.units.len() {
            self.units.truncate(self.head);
            if self.save.is_some_and(|s| s > self.head) {
                self.save = None;
            }
        }
        let open = self
            .last
            .is_some_and(|t| now.saturating_duration_since(t) <= REST);
        self.last = Some(now);
        if open && kind != Kind::Structural {
            if let Some(unit) = self.units.last_mut().filter(|u| u.kind == kind) {
                let at = unit.forward.range.start;
                match kind {
                    Kind::Insert
                        if range.is_empty() && range.start == at + unit.forward.text.len() =>
                    {
                        unit.forward.text.push_str(text);
                        unit.caret.1 = caret.1;
                        return;
                    }
                    // A backspace chain walks left; a delete chain
                    // stands still. Either way the unit's range stays
                    // start..start + replaced.
                    Kind::Delete if text.is_empty() && range.end == at => {
                        unit.forward.range.start = range.start;
                        unit.replaced.insert_str(0, replaced);
                        unit.caret.1 = caret.1;
                        return;
                    }
                    Kind::Delete if text.is_empty() && range.start == at => {
                        unit.replaced.push_str(replaced);
                        unit.forward.range.end = at + unit.replaced.len();
                        unit.caret.1 = caret.1;
                        return;
                    }
                    _ => {}
                }
            }
        }
        self.units.push(Unit {
            forward: Splice {
                range,
                text: text.to_string(),
            },
            replaced: replaced.to_string(),
            caret,
            kind,
        });
        self.head = self.units.len();
    }

    /// Steps back one unit: the inverse splice to apply and the caret
    /// to restore. None when the stack is spent.
    pub fn undo(&mut self) -> Option<(Splice, usize)> {
        self.last = None;
        self.head = self.head.checked_sub(1)?;
        let unit = &self.units[self.head];
        let start = unit.forward.range.start;
        Some((
            Splice {
                range: start..start + unit.forward.text.len(),
                text: unit.replaced.clone(),
            },
            unit.caret.0,
        ))
    }

    /// Steps forward one unit: the forward splice to reapply and the
    /// caret after it. None at the head.
    pub fn redo(&mut self) -> Option<(Splice, usize)> {
        self.last = None;
        let unit = self.units.get(self.head)?;
        self.head += 1;
        Some((unit.forward.clone(), unit.caret.1))
    }

    /// Marks the head as the saved state and closes the open unit, so
    /// the save point never sits inside one.
    pub fn mark_saved(&mut self) {
        self.save = Some(self.head);
        self.last = None;
    }

    /// True while the head stands away from the save point.
    pub fn is_dirty(&self) -> bool {
        self.save != Some(self.head)
    }

    /// Units on the stack, applied or not; the coalescing assertions.
    pub fn unit_count(&self) -> usize {
        self.units.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::splice::Ledger;
    use std::sync::Arc;
    use std::time::Duration;

    fn ledger(base: &str) -> Ledger {
        Ledger::new(Arc::from(base), Vec::new())
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Types `text` at `at`, ledger and history together, the way the
    /// app drives the pair.
    fn type_at(led: &mut Ledger, undo: &mut Undo, at: usize, text: &str, when: Instant) {
        led.edit(at..at, text);
        undo.record(at..at, text, "", (at, at + text.len()), Kind::Insert, when);
    }

    /// Backspace at `at`: deletes the byte before the caret.
    fn backspace_at(led: &mut Ledger, undo: &mut Undo, at: usize, when: Instant) {
        let removed = led.current()[at - 1..at].to_string();
        led.edit(at - 1..at, "");
        undo.record(at - 1..at, "", &removed, (at, at - 1), Kind::Delete, when);
    }

    /// Delete at `at`: deletes the byte after the caret.
    fn delete_at(led: &mut Ledger, undo: &mut Undo, at: usize, when: Instant) {
        let removed = led.current()[at..at + 1].to_string();
        led.edit(at..at + 1, "");
        undo.record(at..at + 1, "", &removed, (at, at), Kind::Delete, when);
    }

    /// A structural edit: paste, cut, a selection replaced.
    fn replace(led: &mut Ledger, undo: &mut Undo, range: Range<usize>, text: &str, when: Instant) {
        let removed = led.current()[range.clone()].to_string();
        led.edit(range.clone(), text);
        undo.record(
            range.clone(),
            text,
            &removed,
            (range.end, range.start + text.len()),
            Kind::Structural,
            when,
        );
    }

    fn apply(led: &mut Ledger, splice: &Splice) {
        led.edit(splice.range.clone(), &splice.text);
    }

    #[test]
    fn consecutive_typing_coalesces_into_one_unit() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        type_at(&mut led, &mut undo, 2, "y", t + ms(200));
        type_at(&mut led, &mut undo, 3, "z", t + ms(400));
        assert_eq!(led.current(), "axyzb");
        assert_eq!(undo.unit_count(), 1, "a typed run is one unit");
        let (inverse, caret) = undo.undo().expect("one unit stands");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "ab", "the whole run undoes at once");
        assert_eq!(caret, 1, "the caret returns to where typing began");
        assert!(undo.undo().is_none(), "the stack is spent");
    }

    #[test]
    fn a_second_of_rest_breaks_the_typing_unit() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        type_at(&mut led, &mut undo, 2, "y", t + ms(1100));
        assert_eq!(undo.unit_count(), 2, "the pause closed the first unit");
        let (inverse, caret) = undo.undo().expect("the rested unit");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "axb", "only the second unit undoes");
        assert_eq!(caret, 2);
    }

    #[test]
    fn a_caret_jump_breaks_the_typing_unit() {
        let mut led = ledger("abcdef");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        type_at(&mut led, &mut undo, 4, "y", t + ms(100));
        assert_eq!(undo.unit_count(), 2, "the jump closed the first unit");
        let (inverse, caret) = undo.undo().expect("the jumped unit");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "axbcdef");
        assert_eq!(caret, 4);
    }

    #[test]
    fn a_structural_operation_is_always_its_own_unit() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        replace(&mut led, &mut undo, 2..2, "YY", t + ms(100));
        type_at(&mut led, &mut undo, 4, "z", t + ms(200));
        assert_eq!(led.current(), "axYYzb");
        assert_eq!(
            undo.unit_count(),
            3,
            "adjacency and speed never merge a structural edit"
        );
        let (inverse, _) = undo.undo().expect("the trailing typing");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "axYYb");
        let (inverse, _) = undo.undo().expect("the structural unit");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "axb");
        let (inverse, _) = undo.undo().expect("the leading typing");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "ab");
    }

    #[test]
    fn backspaces_coalesce_walking_left() {
        let mut led = ledger("abcdef");
        let mut undo = Undo::new();
        let t = Instant::now();
        backspace_at(&mut led, &mut undo, 4, t);
        backspace_at(&mut led, &mut undo, 3, t + ms(100));
        backspace_at(&mut led, &mut undo, 2, t + ms(200));
        assert_eq!(led.current(), "aef");
        assert_eq!(undo.unit_count(), 1, "a backspace chain is one unit");
        let (inverse, caret) = undo.undo().expect("the chain");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "abcdef", "the chain restores in one step");
        assert_eq!(caret, 4, "the caret returns to where deleting began");
    }

    #[test]
    fn forward_deletes_coalesce_at_the_point() {
        let mut led = ledger("abcdef");
        let mut undo = Undo::new();
        let t = Instant::now();
        delete_at(&mut led, &mut undo, 2, t);
        delete_at(&mut led, &mut undo, 2, t + ms(100));
        delete_at(&mut led, &mut undo, 2, t + ms(200));
        assert_eq!(led.current(), "abf");
        assert_eq!(undo.unit_count(), 1, "a delete chain is one unit");
        let (inverse, caret) = undo.undo().expect("the chain");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "abcdef");
        assert_eq!(caret, 2);
    }

    #[test]
    fn undo_hands_back_inverses_newest_first() {
        let mut led = ledger("hello world");
        let mut undo = Undo::new();
        let t = Instant::now();
        replace(&mut led, &mut undo, 0..5, "bye", t);
        assert_eq!(led.current(), "bye world");
        type_at(&mut led, &mut undo, 3, "!", t + ms(100));
        assert_eq!(led.current(), "bye! world");
        let (inverse, caret) = undo.undo().expect("the typing");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "bye world");
        assert_eq!(caret, 3);
        let (inverse, caret) = undo.undo().expect("the replacement");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "hello world");
        assert_eq!(caret, 5, "the caret stands where it stood before");
        assert!(undo.undo().is_none());
        assert!(!led.is_dirty(), "everything undone leaves a clean ledger");
    }

    #[test]
    fn redo_reapplies_the_forward_splices() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        type_at(&mut led, &mut undo, 2, "y", t + ms(1500));
        let (inverse, _) = undo.undo().expect("the second unit");
        apply(&mut led, &inverse);
        let (inverse, _) = undo.undo().expect("the first unit");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "ab");
        let (forward, caret) = undo.redo().expect("the first unit returns");
        apply(&mut led, &forward);
        assert_eq!(led.current(), "axb");
        assert_eq!(caret, 2, "redo seats the caret after the edit");
        let (forward, caret) = undo.redo().expect("the second unit returns");
        apply(&mut led, &forward);
        assert_eq!(led.current(), "axyb");
        assert_eq!(caret, 3);
        assert!(undo.redo().is_none(), "the head is the newest unit");
    }

    #[test]
    fn recording_truncates_the_redo_tail() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        let (inverse, _) = undo.undo().expect("the typed unit");
        apply(&mut led, &inverse);
        type_at(&mut led, &mut undo, 1, "z", t + ms(100));
        assert_eq!(led.current(), "azb");
        assert!(undo.redo().is_none(), "the undone unit is gone");
        assert_eq!(undo.unit_count(), 1);
        let (inverse, _) = undo.undo().expect("the new unit");
        apply(&mut led, &inverse);
        assert_eq!(led.current(), "ab");
    }

    #[test]
    fn the_asterisk_follows_the_head_against_the_save_point() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        assert!(!undo.is_dirty(), "a fresh file is clean");
        type_at(&mut led, &mut undo, 1, "x", t);
        assert!(undo.is_dirty());
        undo.mark_saved();
        assert!(!undo.is_dirty(), "the save point is the head");
        type_at(&mut led, &mut undo, 2, "y", t + ms(100));
        assert_eq!(
            undo.unit_count(),
            2,
            "saving closes the open unit, so the point never sits inside one"
        );
        assert!(undo.is_dirty());
        let (inverse, _) = undo.undo().expect("the post-save typing");
        apply(&mut led, &inverse);
        assert!(!undo.is_dirty(), "undo back to the save point is clean");
        let (inverse, _) = undo.undo().expect("the pre-save typing");
        apply(&mut led, &inverse);
        assert!(undo.is_dirty(), "undo past the save point is dirty again");
        let (forward, _) = undo.redo().expect("forward to the save point");
        apply(&mut led, &forward);
        assert!(!undo.is_dirty(), "redo lands back on the saved state");
        let (forward, _) = undo.redo().expect("forward past it");
        apply(&mut led, &forward);
        assert!(undo.is_dirty());
    }

    #[test]
    fn a_truncated_save_point_stays_dirty_until_the_next_save() {
        let mut led = ledger("ab");
        let mut undo = Undo::new();
        let t = Instant::now();
        type_at(&mut led, &mut undo, 1, "x", t);
        undo.mark_saved();
        let (inverse, _) = undo.undo().expect("the saved typing");
        apply(&mut led, &inverse);
        type_at(&mut led, &mut undo, 1, "z", t + ms(2000));
        assert!(undo.is_dirty(), "the saved state is unreachable");
        let (inverse, _) = undo.undo().expect("the new typing");
        apply(&mut led, &inverse);
        assert!(
            undo.is_dirty(),
            "no head position matches the lost save point"
        );
        let (forward, _) = undo.redo().expect("the new typing returns");
        apply(&mut led, &forward);
        assert!(undo.is_dirty());
        undo.mark_saved();
        assert!(!undo.is_dirty(), "only the next save cleans the title");
    }
}
