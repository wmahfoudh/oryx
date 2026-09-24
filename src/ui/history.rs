//! The places a reader jumped away from, walked back and forward the
//! way a browser's history is: a jump files the place it leaves and
//! drops what lay ahead, a step back files the place it leaves ahead.
//! A place is a file and a source offset, one currency for reading and
//! for editing, so the walk crosses files and modes alike; the height
//! its line stood at on the screen comes along, so a step brings back
//! the view that was left.

use std::path::PathBuf;

/// How many places each direction keeps; the oldest goes first.
const DEPTH: usize = 100;

#[derive(Debug, Clone)]
pub struct Entry {
    /// None for a page with no file behind it, the welcome page or the
    /// quick reference: a place to return to only while that page shows.
    pub file: Option<PathBuf>,
    pub offset: usize,
    /// How far under the top of the view the place's line stood, so a
    /// step brings the view back as it was left. Negative for a line cut
    /// by the top edge.
    pub below: f32,
}

/// Two visits to one line are one place, whatever height it stood at.
impl PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        self.file == other.file && self.offset == other.offset
    }
}

impl Eq for Entry {}

#[derive(Debug, Default)]
pub struct History {
    back: Vec<Entry>,
    forward: Vec<Entry>,
}

impl History {
    /// A jump is leaving `from`. Jumping again from the same place
    /// files one return, not two.
    pub fn jump(&mut self, from: Entry) {
        self.forward.clear();
        file(&mut self.back, from);
    }

    /// The place a step would land on, without taking it: a step into
    /// another file may still be refused by the unsaved question.
    pub fn peek(&self, forward: bool, here: Option<&Entry>) -> Option<&Entry> {
        let from = if forward { &self.forward } else { &self.back };
        from.iter().rev().find(|place| Some(*place) != here)
    }

    /// Takes the step: the landing place, with `here` filed in the
    /// other direction. Places equal to `here` are passed over, so a key
    /// press never lands where the reader already stands.
    pub fn step(&mut self, forward: bool, here: Option<Entry>) -> Option<Entry> {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        let keep = from
            .iter()
            .rposition(|place| Some(place) != here.as_ref())?;
        from.truncate(keep + 1);
        let landing = from.pop()?;
        if let Some(here) = here {
            file(to, here);
        }
        Some(landing)
    }

    /// Drops the places the test refuses, a file that is gone for one.
    pub fn retain(&mut self, keep: impl Fn(&Entry) -> bool) {
        self.back.retain(&keep);
        self.forward.retain(&keep);
    }

    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

/// Files a place on a stack, once, inside the depth. Filed again, the
/// place keeps its later height.
fn file(stack: &mut Vec<Entry>, place: Entry) {
    if let Some(last) = stack.last_mut().filter(|last| **last == place) {
        *last = place;
        return;
    }
    stack.push(place);
    if stack.len() > DEPTH {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: usize) -> Entry {
        at_height(offset, 0.0)
    }

    fn at_height(offset: usize, below: f32) -> Entry {
        Entry {
            file: Some(PathBuf::from("/notes/a.md")),
            offset,
            below,
        }
    }

    fn other(offset: usize) -> Entry {
        Entry {
            file: Some(PathBuf::from("/notes/b.md")),
            offset,
            below: 0.0,
        }
    }

    #[test]
    fn a_place_comes_back_at_the_height_it_stood() {
        let mut h = History::default();
        h.jump(at_height(10, 120.0));
        let back = h.step(false, Some(at_height(900, 290.0))).unwrap();
        assert_eq!(back.below, 120.0);
        let ahead = h.step(true, Some(back)).unwrap();
        assert_eq!(ahead.below, 290.0, "forward returns the jump's own height");
    }

    #[test]
    fn the_height_is_not_part_of_the_place() {
        assert_eq!(at_height(10, 0.0), at_height(10, 50.0));
        let mut h = History::default();
        h.jump(at_height(10, 50.0));
        h.jump(at_height(10, 80.0));
        let back = h.step(false, Some(at(99))).unwrap();
        assert_eq!(back.below, 80.0, "the later height");
        assert_eq!(h.step(false, Some(back)), None, "one return");
    }

    #[test]
    fn back_returns_through_the_jumps_and_forward_goes_again() {
        let mut h = History::default();
        h.jump(at(10));
        h.jump(at(200));
        assert_eq!(h.step(false, Some(at(900))), Some(at(200)));
        assert_eq!(h.step(false, Some(at(200))), Some(at(10)));
        assert_eq!(h.step(false, Some(at(10))), None, "nothing further back");
        assert_eq!(h.step(true, Some(at(10))), Some(at(200)));
        assert_eq!(h.step(true, Some(at(200))), Some(at(900)));
        assert_eq!(h.step(true, Some(at(900))), None, "nothing further ahead");
    }

    #[test]
    fn a_new_jump_drops_what_lay_ahead() {
        let mut h = History::default();
        h.jump(at(10));
        assert_eq!(h.step(false, Some(at(500))), Some(at(10)));
        h.jump(at(10));
        assert_eq!(h.step(true, Some(at(700))), None, "the browser's rule");
        assert_eq!(h.step(false, Some(at(700))), Some(at(10)));
    }

    #[test]
    fn jumping_twice_from_one_place_files_one_return() {
        let mut h = History::default();
        h.jump(at(10));
        h.jump(at(10));
        assert_eq!(h.step(false, Some(at(50))), Some(at(10)));
        assert_eq!(h.step(false, Some(at(10))), None);
    }

    #[test]
    fn a_step_never_lands_where_the_reader_stands() {
        let mut h = History::default();
        h.jump(at(10));
        h.jump(at(0));
        assert_eq!(
            h.peek(false, Some(&at(0))),
            Some(&at(10)),
            "the place equal to here is passed over"
        );
        assert_eq!(h.step(false, Some(at(0))), Some(at(10)));
        assert_eq!(h.step(true, Some(at(10))), Some(at(0)));
    }

    #[test]
    fn peek_names_the_landing_and_changes_nothing() {
        let mut h = History::default();
        h.jump(at(10));
        h.jump(other(40));
        assert_eq!(h.peek(false, Some(&at(7))), Some(&other(40)));
        assert_eq!(h.peek(false, Some(&at(7))), Some(&other(40)));
        assert_eq!(h.peek(true, Some(&at(7))), None);
        assert_eq!(h.step(false, Some(at(7))), Some(other(40)));
        assert_eq!(h.peek(true, Some(&other(40))), Some(&at(7)));
    }

    #[test]
    fn a_page_with_no_file_files_nothing_ahead() {
        let mut h = History::default();
        h.jump(at(10));
        assert_eq!(h.step(false, None), Some(at(10)));
        assert_eq!(h.step(true, Some(at(10))), None);
    }

    #[test]
    fn the_depth_is_bounded_and_the_oldest_goes_first() {
        let mut h = History::default();
        for i in 0..150 {
            h.jump(at(i));
        }
        let mut walked = 0;
        let mut here = at(9999);
        while let Some(place) = h.step(false, Some(here.clone())) {
            here = place;
            walked += 1;
        }
        assert_eq!(walked, 100);
        assert_eq!(here, at(50), "the fifty oldest went");
    }

    #[test]
    fn places_in_a_file_that_is_gone_are_dropped() {
        let mut h = History::default();
        h.jump(at(10));
        h.jump(other(40));
        h.jump(at(20));
        h.retain(|e| e.file != other(0).file);
        assert_eq!(h.step(false, Some(at(99))), Some(at(20)));
        assert_eq!(h.step(false, Some(at(20))), Some(at(10)));
    }
}
