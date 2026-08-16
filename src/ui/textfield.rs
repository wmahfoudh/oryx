//! A single-line text field: buffer, caret and selection. It owns no
//! drawing, no clipboard and no font system, so every site keeps its own
//! appearance and the whole thing stays testable without a display.

use std::ops::Range;
use std::time::Instant;

use winit::keyboard::{Key, NamedKey};

use crate::input::DOUBLE_CLICK;

/// What a key did to the field.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Edit {
    /// Not a field key. The owner should handle it.
    Ignored,
    /// Claimed, and the text is unchanged: a caret or selection move, or a
    /// key with nothing to do such as Backspace at the start. Claimed
    /// matters as much as the reason, since a key reported here must not
    /// reach the global keymap.
    Handled,
    /// Claimed, and the text changed.
    Changed,
}

/// Edits a field remembers for undo; enough for any single line.
const HISTORY: usize = 100;

/// A single line of editable text. The caret is a byte index and always
/// sits on a character boundary; `anchor` holds the fixed end of a
/// selection while the caret is its moving end. Every text change
/// records the prior text and caret, so Ctrl+Z and Ctrl+Shift+Z work in
/// every field.
#[derive(Debug, Default, Clone)]
pub struct TextField {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    last_click: Option<Instant>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

impl TextField {
    /// A field holding `text`, caret at the end, nothing selected.
    pub fn new(text: impl Into<String>) -> TextField {
        let text = text.into();
        TextField {
            caret: text.len(),
            text,
            anchor: None,
            last_click: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The selected byte range, low end first, or None when the selection
    /// is empty.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        match anchor.cmp(&self.caret) {
            std::cmp::Ordering::Less => Some(anchor..self.caret),
            std::cmp::Ordering::Greater => Some(self.caret..anchor),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn selected_text(&self) -> &str {
        match self.selection() {
            Some(range) => &self.text[range],
            None => "",
        }
    }

    /// Replaces the content, caret at the end, selection cleared. A
    /// programmatic reset, so the undo history clears with it.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.text.len();
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
    }

    pub fn clear(&mut self) {
        self.set_text(String::new());
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Replaces the selection with `text`, or inserts at the caret, and
    /// reports whether anything changed. Control characters are dropped: a
    /// pasted tab or newline would otherwise cross a boundary the field
    /// has no way to render.
    pub fn insert(&mut self, text: &str) -> Edit {
        self.insert_text(text)
    }

    /// Moves the caret to a byte index, snapped back to a character
    /// boundary, and clears the selection.
    pub fn set_caret(&mut self, index: usize) {
        self.caret = self.snap(index);
        self.anchor = None;
    }

    /// Handles one key. Keys the field does not claim, Enter and Escape
    /// above all, report `Ignored` so the owner can act on them.
    pub fn key(&mut self, key: &Key, ctrl: bool, shift: bool) -> Edit {
        match key {
            Key::Character(c) if ctrl => {
                if c.eq_ignore_ascii_case("a") {
                    self.select_all();
                    Edit::Handled
                } else if c.eq_ignore_ascii_case("z") {
                    if shift {
                        self.redo()
                    } else {
                        self.undo()
                    }
                } else {
                    Edit::Ignored
                }
            }
            Key::Character(c) => self.insert_text(c.as_str()),
            Key::Named(NamedKey::Space) => self.insert_text(" "),
            Key::Named(NamedKey::Backspace) => self.delete(Step::Back),
            Key::Named(NamedKey::Delete) => self.delete(Step::Forward),
            // Ctrl-modified navigation means nothing on one line; the
            // owner keeps its document jumps while a field is open.
            Key::Named(NamedKey::ArrowLeft) if !ctrl => self.move_caret(Motion::Left, shift),
            Key::Named(NamedKey::ArrowRight) if !ctrl => self.move_caret(Motion::Right, shift),
            Key::Named(NamedKey::Home) if !ctrl => self.move_caret(Motion::Start, shift),
            Key::Named(NamedKey::End) if !ctrl => self.move_caret(Motion::End, shift),
            _ => Edit::Ignored,
        }
    }

    /// Pixel offset of the caret from the start of the text, measured by
    /// the caller so the field stays free of the font system.
    pub fn caret_offset(&self, mut measure: impl FnMut(&str) -> f32) -> f32 {
        measure(&self.text[..self.caret])
    }

    /// Pixel offset of every character boundary, in order and ending past
    /// the last character. An owner computes this while drawing, where the
    /// font system is at hand, and keeps it for hit testing, since a click
    /// carries no painter.
    pub fn offsets(&self, mut measure: impl FnMut(&str) -> f32) -> Vec<f32> {
        self.boundaries()
            .map(|index| measure(&self.text[..index]))
            .collect()
    }

    /// The byte index whose boundary sits nearest `x`, against offsets from
    /// an earlier `offsets` call. A cache that does not match the current
    /// text is refused and the caret goes to the end.
    pub fn caret_at(&self, x: f32, offsets: &[f32]) -> usize {
        let boundaries: Vec<usize> = self.boundaries().collect();
        if offsets.len() != boundaries.len() {
            return self.text.len();
        }
        let mut best = 0;
        let mut best_distance = f32::INFINITY;
        for (index, offset) in boundaries.iter().zip(offsets) {
            let distance = (offset - x).abs();
            if distance < best_distance {
                best_distance = distance;
                best = *index;
            }
        }
        best
    }

    /// A click at `x`. A second click inside `DOUBLE_CLICK` selects
    /// everything; a single click places the caret.
    pub fn click(&mut self, x: f32, offsets: &[f32], now: Instant) {
        let double = self
            .last_click
            .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
        if double {
            self.select_all();
            self.last_click = None;
        } else {
            let index = self.caret_at(x, offsets);
            self.set_caret(index);
            self.last_click = Some(now);
        }
    }

    fn boundaries(&self) -> impl Iterator<Item = usize> + '_ {
        self.text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(self.text.len()))
    }

    /// The nearest character boundary at or before `index`, clamped to the
    /// text.
    fn snap(&self, index: usize) -> usize {
        let mut index = index.min(self.text.len());
        while !self.text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }

    fn insert_text(&mut self, text: &str) -> Edit {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return Edit::Handled;
        }
        let range = self.selection().unwrap_or(self.caret..self.caret);
        self.replace(range, &clean);
        Edit::Changed
    }

    fn delete(&mut self, step: Step) -> Edit {
        if let Some(range) = self.selection() {
            self.replace(range, "");
            return Edit::Changed;
        }
        let range = match step {
            Step::Back => self.prev(self.caret).map(|from| from..self.caret),
            Step::Forward => self.next(self.caret).map(|to| self.caret..to),
        };
        match range {
            Some(range) => {
                self.replace(range, "");
                Edit::Changed
            }
            None => Edit::Handled,
        }
    }

    /// Drops the selection, text and caret untouched.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Deletes the selection if one stands.
    pub fn delete_selection(&mut self) -> Edit {
        match self.selection() {
            Some(range) => {
                self.replace(range, "");
                Edit::Changed
            }
            None => Edit::Handled,
        }
    }

    /// Restores the state before the last text change. An empty history
    /// reports `Ignored`, so the owner's own undo can answer instead of
    /// the key dying in the field.
    pub fn undo(&mut self) -> Edit {
        match self.undo.pop() {
            Some((text, caret)) => {
                self.redo.push((std::mem::take(&mut self.text), self.caret));
                self.text = text;
                self.caret = caret;
                self.anchor = None;
                Edit::Changed
            }
            None => Edit::Ignored,
        }
    }

    /// Restores the state an undo left, `Ignored` on an empty history
    /// like `undo`.
    pub fn redo(&mut self) -> Edit {
        match self.redo.pop() {
            Some((text, caret)) => {
                self.undo.push((std::mem::take(&mut self.text), self.caret));
                self.text = text;
                self.caret = caret;
                self.anchor = None;
                Edit::Changed
            }
            None => Edit::Ignored,
        }
    }

    fn replace(&mut self, range: Range<usize>, text: &str) {
        self.undo.push((self.text.clone(), self.caret));
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.text.replace_range(range.clone(), text);
        self.caret = range.start + text.len();
        self.anchor = None;
    }

    fn move_caret(&mut self, motion: Motion, shift: bool) -> Edit {
        // A plain arrow against a selection collapses to the matching edge
        // and goes no further, which is what every text field does.
        if !shift {
            if let Some(range) = self.selection() {
                self.caret = match motion {
                    Motion::Left => range.start,
                    Motion::Right => range.end,
                    Motion::Start => 0,
                    Motion::End => self.text.len(),
                };
                self.anchor = None;
                return Edit::Handled;
            }
        }
        let target = match motion {
            Motion::Left => self.prev(self.caret),
            Motion::Right => self.next(self.caret),
            Motion::Start => Some(0),
            Motion::End => Some(self.text.len()),
        };
        let Some(target) = target else {
            return Edit::Handled;
        };
        if shift {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = target;
        Edit::Handled
    }

    fn prev(&self, index: usize) -> Option<usize> {
        self.text[..index]
            .chars()
            .next_back()
            .map(|c| index - c.len_utf8())
    }

    fn next(&self, index: usize) -> Option<usize> {
        self.text[index..]
            .chars()
            .next()
            .map(|c| index + c.len_utf8())
    }
}

enum Step {
    Back,
    Forward,
}

enum Motion {
    Left,
    Right,
    Start,
    End,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use winit::keyboard::SmolStr;

    fn ch(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    fn named(k: NamedKey) -> Key {
        Key::Named(k)
    }

    /// Ten pixels per character, so offsets are readable in assertions.
    fn measure(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    fn field_at(text: &str, caret: usize) -> TextField {
        let mut f = TextField::new(text);
        f.set_caret(caret);
        f
    }

    #[test]
    fn ctrl_navigation_is_left_to_the_owner() {
        let mut f = field_at("hello", 2);
        for key in [
            NamedKey::Home,
            NamedKey::End,
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
        ] {
            assert_eq!(f.key(&named(key), true, false), Edit::Ignored);
        }
        assert_eq!(f.caret(), 2, "the field caret never moved");
    }

    #[test]
    fn clear_selection_keeps_text_and_caret() {
        let mut f = TextField::new("hello");
        f.select_all();
        f.clear_selection();
        assert_eq!(f.selection(), None);
        assert_eq!(f.text(), "hello");
        assert_eq!(f.caret(), 5);
    }

    #[test]
    fn undo_restores_text_and_caret_and_redo_returns() {
        let mut f = field_at("ab", 2);
        assert_eq!(f.key(&ch("c"), false, false), Edit::Changed);
        assert_eq!(f.text(), "abc");
        assert_eq!(f.key(&ch("z"), true, false), Edit::Changed);
        assert_eq!(f.text(), "ab");
        assert_eq!(f.caret(), 2);
        assert_eq!(f.key(&ch("z"), true, true), Edit::Changed);
        assert_eq!(f.text(), "abc");
        assert_eq!(f.caret(), 3);
    }

    #[test]
    fn undo_with_no_history_is_not_claimed() {
        let mut f = field_at("ab", 2);
        assert_eq!(f.key(&ch("z"), true, false), Edit::Ignored);
        assert_eq!(f.text(), "ab");
    }

    #[test]
    fn a_paste_undoes_as_one_unit() {
        let mut f = TextField::new("");
        assert_eq!(f.insert("hello"), Edit::Changed);
        assert_eq!(f.key(&ch("z"), true, false), Edit::Changed);
        assert_eq!(f.text(), "");
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut f = field_at("ab", 2);
        f.key(&ch("c"), false, false);
        f.key(&ch("z"), true, false);
        f.key(&ch("d"), false, false);
        assert_eq!(f.text(), "abd");
        assert_eq!(f.key(&ch("z"), true, true), Edit::Ignored);
        assert_eq!(f.text(), "abd");
    }

    #[test]
    fn set_text_resets_the_history() {
        let mut f = field_at("ab", 2);
        f.key(&ch("c"), false, false);
        f.set_text("fresh");
        assert_eq!(f.key(&ch("z"), true, false), Edit::Ignored);
        assert_eq!(f.text(), "fresh");
    }

    #[test]
    fn delete_selection_removes_it_and_undo_brings_it_back() {
        let mut f = TextField::new("hello");
        f.select_all();
        assert_eq!(f.delete_selection(), Edit::Changed);
        assert_eq!(f.text(), "");
        assert_eq!(f.delete_selection(), Edit::Handled);
        assert_eq!(f.key(&ch("z"), true, false), Edit::Changed);
        assert_eq!(f.text(), "hello");
    }

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut f = field_at("hello", 2);
        assert_eq!(f.key(&ch("X"), false, false), Edit::Changed);
        assert_eq!(f.text(), "heXllo");
        assert_eq!(f.caret(), 3);
    }

    #[test]
    fn space_arrives_as_a_named_key_and_still_types() {
        let mut f = field_at("ab", 1);
        assert_eq!(f.key(&named(NamedKey::Space), false, false), Edit::Changed);
        assert_eq!(f.text(), "a b");
        assert_eq!(f.caret(), 2);
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut f = TextField::new("hello");
        f.select_all();
        assert_eq!(f.key(&ch("z"), false, false), Edit::Changed);
        assert_eq!(f.text(), "z");
        assert_eq!(f.caret(), 1);
        assert_eq!(f.selection(), None);
    }

    #[test]
    fn backspace_removes_the_character_before_the_caret() {
        let mut f = field_at("hello", 3);
        assert_eq!(
            f.key(&named(NamedKey::Backspace), false, false),
            Edit::Changed
        );
        assert_eq!(f.text(), "helo");
        assert_eq!(f.caret(), 2);
    }

    #[test]
    fn backspace_at_the_start_is_claimed_but_changes_nothing() {
        let mut f = field_at("hello", 0);
        assert_eq!(
            f.key(&named(NamedKey::Backspace), false, false),
            Edit::Handled,
            "claimed, so it never reaches the global keymap"
        );
        assert_eq!(f.text(), "hello");
    }

    #[test]
    fn backspace_and_delete_remove_a_selection_and_nothing_else() {
        let mut f = TextField::new("hello");
        f.set_caret(1);
        f.key(&named(NamedKey::ArrowRight), false, true);
        f.key(&named(NamedKey::ArrowRight), false, true);
        assert_eq!(f.selected_text(), "el");
        assert_eq!(
            f.key(&named(NamedKey::Backspace), false, false),
            Edit::Changed
        );
        assert_eq!(f.text(), "hlo");
        assert_eq!(f.caret(), 1);
        assert_eq!(f.selection(), None);

        let mut g = TextField::new("hello");
        g.select_all();
        assert_eq!(g.key(&named(NamedKey::Delete), false, false), Edit::Changed);
        assert_eq!(g.text(), "");
    }

    #[test]
    fn delete_removes_the_character_after_the_caret_and_stops_at_the_end() {
        let mut f = field_at("hello", 4);
        assert_eq!(f.key(&named(NamedKey::Delete), false, false), Edit::Changed);
        assert_eq!(f.text(), "hell");
        assert_eq!(f.key(&named(NamedKey::Delete), false, false), Edit::Handled);
        assert_eq!(f.text(), "hell");
    }

    #[test]
    fn arrows_step_whole_characters_over_multibyte_text() {
        // 'é' is two bytes, '🦌' is four.
        let mut f = field_at("é🦌e", 0);
        f.key(&named(NamedKey::ArrowRight), false, false);
        assert_eq!(f.caret(), 2);
        f.key(&named(NamedKey::ArrowRight), false, false);
        assert_eq!(f.caret(), 6);
        f.key(&named(NamedKey::ArrowLeft), false, false);
        assert_eq!(f.caret(), 2);
        assert!(f.text().is_char_boundary(f.caret()));
    }

    #[test]
    fn arrows_clamp_at_both_ends() {
        let mut f = field_at("ab", 0);
        assert_eq!(
            f.key(&named(NamedKey::ArrowLeft), false, false),
            Edit::Handled
        );
        assert_eq!(f.caret(), 0);
        f.set_caret(2);
        assert_eq!(
            f.key(&named(NamedKey::ArrowRight), false, false),
            Edit::Handled
        );
        assert_eq!(f.caret(), 2);
    }

    #[test]
    fn a_plain_arrow_collapses_a_selection_to_the_matching_edge() {
        let mut f = TextField::new("hello");
        f.select_all();
        assert_eq!(
            f.key(&named(NamedKey::ArrowLeft), false, false),
            Edit::Handled
        );
        assert_eq!(f.caret(), 0, "left collapses to the start");
        assert_eq!(f.selection(), None);

        let mut g = TextField::new("hello");
        g.select_all();
        g.key(&named(NamedKey::ArrowRight), false, false);
        assert_eq!(g.caret(), 5, "right collapses to the end");
        assert_eq!(g.selection(), None);
    }

    #[test]
    fn shift_arrows_extend_and_shrink_the_selection() {
        let mut f = field_at("hello", 2);
        assert_eq!(
            f.key(&named(NamedKey::ArrowRight), false, true),
            Edit::Handled
        );
        assert_eq!(f.selection(), Some(2..3));
        f.key(&named(NamedKey::ArrowRight), false, true);
        assert_eq!(f.selected_text(), "ll");
        f.key(&named(NamedKey::ArrowLeft), false, true);
        assert_eq!(f.selected_text(), "l");
        f.key(&named(NamedKey::ArrowLeft), false, true);
        assert_eq!(
            f.selection(),
            None,
            "back at the anchor nothing is selected"
        );
    }

    #[test]
    fn home_and_end_move_the_caret_and_shift_selects_to_them() {
        let mut f = field_at("hello", 2);
        assert_eq!(f.key(&named(NamedKey::Home), false, false), Edit::Handled);
        assert_eq!(f.caret(), 0);
        assert_eq!(f.key(&named(NamedKey::End), false, false), Edit::Handled);
        assert_eq!(f.caret(), 5);

        let mut g = field_at("hello", 2);
        g.key(&named(NamedKey::Home), false, true);
        assert_eq!(g.selected_text(), "he");
        g.key(&named(NamedKey::End), false, true);
        assert_eq!(g.selected_text(), "llo");
    }

    #[test]
    fn ctrl_a_selects_everything() {
        let mut f = field_at("hello", 2);
        assert_eq!(f.key(&ch("a"), true, false), Edit::Handled);
        assert_eq!(f.selected_text(), "hello");
        assert_eq!(f.key(&ch("A"), true, false), Edit::Handled, "case ignored");
    }

    #[test]
    fn other_ctrl_chords_are_left_to_the_owner() {
        let mut f = field_at("hello", 2);
        for c in ["c", "v", "x"] {
            assert_eq!(f.key(&ch(c), true, false), Edit::Ignored, "ctrl+{c}");
        }
        assert_eq!(f.text(), "hello", "no ctrl chord types a character");
    }

    #[test]
    fn enter_and_escape_are_left_to_the_owner() {
        let mut f = field_at("hello", 2);
        assert_eq!(f.key(&named(NamedKey::Enter), false, false), Edit::Ignored);
        assert_eq!(f.key(&named(NamedKey::Escape), false, false), Edit::Ignored);
    }

    #[test]
    fn insert_strips_control_characters() {
        let mut f = TextField::new("");
        f.insert("a\tb\nc");
        assert_eq!(f.text(), "abc");
        assert_eq!(f.caret(), 3);
    }

    #[test]
    fn insert_replaces_a_selection() {
        let mut f = TextField::new("hello");
        f.select_all();
        f.insert("#1a1b26");
        assert_eq!(f.text(), "#1a1b26");
        assert_eq!(f.selection(), None);
    }

    #[test]
    fn set_caret_snaps_to_a_character_boundary() {
        let mut f = TextField::new("é🦌");
        f.set_caret(1);
        assert_eq!(f.caret(), 0, "inside 'é' snaps back");
        f.set_caret(4);
        assert_eq!(f.caret(), 2, "inside the emoji snaps back");
        f.set_caret(99);
        assert_eq!(f.caret(), 6, "past the end clamps");
    }

    #[test]
    fn caret_offset_measures_the_text_before_the_caret() {
        let f = field_at("hello", 2);
        assert_eq!(f.caret_offset(measure), 20.0);
        assert_eq!(TextField::new("hello").caret_offset(measure), 50.0);
    }

    #[test]
    fn caret_at_finds_the_nearest_boundary_and_clamps() {
        let f = TextField::new("hello");
        let at = f.offsets(measure);
        assert_eq!(at, vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
        assert_eq!(f.caret_at(0.0, &at), 0);
        assert_eq!(f.caret_at(-30.0, &at), 0, "before the start");
        assert_eq!(f.caret_at(21.0, &at), 2, "just past the second gap");
        assert_eq!(f.caret_at(26.0, &at), 3, "nearer the third gap");
        assert_eq!(f.caret_at(999.0, &at), 5, "past the end");
    }

    #[test]
    fn caret_at_round_trips_through_caret_offset() {
        let text = "h\u{e9}llo\u{1F98C}";
        let f = TextField::new(text);
        let at = f.offsets(measure);
        for (index, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let mut probe = TextField::new(text);
            probe.set_caret(index);
            let x = probe.caret_offset(measure);
            assert_eq!(f.caret_at(x, &at), index, "at byte {index}");
        }
    }

    #[test]
    fn a_stale_offset_cache_is_refused() {
        let f = TextField::new("hello");
        assert_eq!(
            f.caret_at(10.0, &[]),
            5,
            "no cache puts the caret at the end"
        );
        assert_eq!(
            f.caret_at(10.0, &[0.0, 10.0]),
            5,
            "a wrong length is refused"
        );
    }

    #[test]
    fn a_single_click_places_the_caret_and_a_double_click_selects_all() {
        let mut f = TextField::new("hello");
        let at = f.offsets(measure);
        let start = Instant::now();
        f.click(21.0, &at, start);
        assert_eq!(f.caret(), 2);
        assert_eq!(f.selection(), None);
        f.click(21.0, &at, start + Duration::from_millis(100));
        assert_eq!(f.selected_text(), "hello", "inside the window");

        let mut g = TextField::new("hello");
        let at = g.offsets(measure);
        g.click(21.0, &at, start);
        g.click(41.0, &at, start + Duration::from_millis(900));
        assert_eq!(g.selection(), None, "too slow to be a double click");
        assert_eq!(g.caret(), 4);
    }
}
