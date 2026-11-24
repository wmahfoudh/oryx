//! The system's input method, which composes Chinese, Japanese, Korean
//! and other scripts: the composing text the window holds, the press
//! that is the input method's, and where committed text goes. These
//! rules need no window, so they are under tests, a fuzzer and the
//! replays of recorded logs (`tests/input_method.rs`).
//!
//! With the setting off, nothing here acts.

use winit::keyboard::{Key, NamedKey};

use crate::input::keymap::KeyOwner;
use crate::input::route::{self, Field, First};

/// Whether what has the keys takes text from an input method: the
/// editor with a caret, the search bar, the path field, and a dialog's
/// field that takes a name. Not where a bare letter is a shortcut or
/// an answer, and not a field of digits or of a hex color.
pub fn takes_text(owner: KeyOwner, editing: bool, has_caret: bool, dialog_text: bool) -> bool {
    match owner {
        KeyOwner::Text | KeyOwner::Code => editing && has_caret,
        KeyOwner::Search | KeyOwner::Path => true,
        KeyOwner::Dialog { field: true } => dialog_text,
        KeyOwner::Dialog { field: false } | KeyOwner::GoTo | KeyOwner::Question => false,
    }
}

/// The state an event is read in: the setting, what has the keys, the
/// mode, and whether the keys take text there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    setting: bool,
    owner: KeyOwner,
    editing: bool,
    takes_text: bool,
}

impl At {
    /// `has_caret` is the editor's, and `dialog_text` says that the
    /// open dialog's field takes a name (`takes_text`).
    pub fn new(
        setting: bool,
        owner: KeyOwner,
        editing: bool,
        has_caret: bool,
        dialog_text: bool,
    ) -> Self {
        At {
            setting,
            owner,
            editing,
            takes_text: takes_text(owner, editing, has_caret, dialog_text),
        }
    }

    /// Whether the input method is wanted: the setting on, where the
    /// keys take text.
    pub fn wanted(self) -> bool {
        self.setting && self.takes_text
    }
}

/// Whether a press is the input method's and not Oryx's. On Windows
/// each key an input method uses arrives as `Process`, with the real
/// key's code and the modifiers: with the setting on it is dropped, a
/// shortcut included. With the setting off it passes, as it always
/// did.
pub fn drops_press(setting: bool, key: &Key) -> bool {
    setting && matches!(key, Key::Named(NamedKey::Process))
}

/// The reader that gets committed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitTo {
    Editor,
    Field(Field),
    /// The field of the open dialog.
    Dialog,
}

/// Where committed text goes: to the reader a key with no command
/// would reach (`route::first`), when that reader types. A commit is
/// to be given to it as a character key with no modifier: the reader's
/// own guards then apply, and a commit does there what the key does.
/// None means dropped: at a question, in a dialog with no field and in
/// reading mode, a commit answers nothing and is no shortcut.
pub fn commit_to(owner: KeyOwner, editing: bool) -> Option<CommitTo> {
    match route::first(owner, editing, false, None) {
        First::Editor => Some(CommitTo::Editor),
        First::Field(field) => Some(CommitTo::Field(field)),
        First::Dialog if owner == (KeyOwner::Dialog { field: true }) => Some(CommitTo::Dialog),
        First::Dialog | First::Question | First::Page | First::Nobody | First::Toggle(_) => None,
    }
}

/// An event of the system's input method, as winit names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// On Windows it opens each composition. Elsewhere it follows the
    /// window's own on and off.
    Enabled,
    Disabled,
    /// The composing text, with the input method's cursor in it: two
    /// ends in bytes, or nothing.
    Composing {
        text: &'a str,
        cursor: Option<(usize, usize)>,
    },
    Commit(&'a str),
}

/// What the window does after an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome<'a> {
    /// The box of the composing text changed: a frame is asked.
    pub redraw: bool,
    /// Text to type, and the reader that types it.
    pub commit: Option<(CommitTo, &'a str)>,
}

/// The composing text that shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown<'a> {
    pub text: &'a str,
    /// The two ends of the input method's cursor, in order, each
    /// within the text and on a character boundary: `text[from..to]`
    /// is the range, and the two are equal for a point. None when the
    /// input method gave no cursor, or one that is off the text or
    /// inside a character.
    pub cursor: Option<(usize, usize)>,
}

/// The composing text the window holds, at the caret of what has the
/// keys. It is not in the document, the undo history, the layout or a
/// field's text. Empty means that no composition shows.
///
/// It belongs to the place it came at, what had the keys and the mode,
/// and shows nowhere else: `shown` answers nothing at another place,
/// and `settle` forgets a text that does not show. So the code that
/// changes what has the keys or the mode has no call to make. A place
/// left and found again before `settle` runs shows its text again.
#[derive(Debug, Default)]
pub struct Composition {
    text: String,
    cursor: Option<(usize, usize)>,
    place: Option<(KeyOwner, bool)>,
}

impl Composition {
    /// Whether no composing text is held.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The composing text to draw at the caret, if one shows here.
    pub fn shown(&self, at: At) -> Option<Shown<'_>> {
        let here = at.wanted() && self.place == Some((at.owner, at.editing));
        (here && !self.text.is_empty()).then_some(Shown {
            text: &self.text,
            cursor: self.cursor,
        })
    }

    /// Takes an event of the input method. With the setting off each
    /// event is ignored.
    ///
    /// A composing text is held only where the keys take text. An
    /// empty one, a commit and `Disabled` close the composition. On
    /// Windows `Disabled` closes each composition while the input
    /// method stays on, and `Enabled` opens the next: neither says
    /// anything of the window's own switch. A commit is typed where
    /// the keys go (`commit_to`), and an empty one types nothing.
    pub fn event<'a>(&mut self, at: At, event: Event<'a>) -> Outcome<'a> {
        if !at.setting {
            self.clear();
            return Outcome::default();
        }
        match event {
            Event::Enabled => Outcome::default(),
            Event::Composing { text, cursor } if !text.is_empty() => {
                if !at.takes_text {
                    self.clear();
                    return Outcome::default();
                }
                let cursor = cursor
                    .filter(|&ends| on_boundaries(text, ends))
                    .map(|(a, b)| (a.min(b), a.max(b)));
                let same = self.shown(at) == Some(Shown { text, cursor });
                if !same {
                    self.text.clear();
                    self.text.push_str(text);
                    self.cursor = cursor;
                    self.place = Some((at.owner, at.editing));
                }
                debug_assert!(self
                    .cursor
                    .is_none_or(|ends| ends.0 <= ends.1 && on_boundaries(&self.text, ends)));
                Outcome {
                    redraw: !same,
                    commit: None,
                }
            }
            Event::Composing { .. } | Event::Disabled => Outcome {
                redraw: self.close(at),
                commit: None,
            },
            Event::Commit(text) => {
                let redraw = self.close(at);
                let to = commit_to(at.owner, at.editing).filter(|_| !text.is_empty());
                Outcome {
                    redraw,
                    commit: to.map(|to| (to, text)),
                }
            }
        }
    }

    /// Forgets a composing text that no longer shows: what has the
    /// keys or the mode changed, the keys take no text there, or the
    /// setting went off. True when a text was forgotten, and a frame
    /// is to be asked. The window runs it before each of its events
    /// and before it reads the rule, so that a text never outlives the
    /// event that moved the keys away from it.
    pub fn settle(&mut self, at: At) -> bool {
        if self.is_empty() || self.shown(at).is_some() {
            return false;
        }
        self.clear()
    }

    /// Forgets the composing text, whatever has the keys. The place
    /// does not tell a loss of focus or another file opened in the
    /// same mode: those are for the caller to say. True when a text
    /// was held.
    pub fn clear(&mut self) -> bool {
        let held = !self.text.is_empty();
        self.text.clear();
        self.cursor = None;
        self.place = None;
        held
    }

    /// Clears the composition, and says whether its box showed here.
    fn close(&mut self, at: At) -> bool {
        let showed = self.shown(at).is_some();
        self.clear();
        showed
    }
}

/// The caret's bar, in the window's physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// The position and the size the system gets for the caret, so that
/// it opens its candidates beside it. X11 takes a point and no size
/// (`point`): the foot of the bar, under which the candidates open.
pub fn caret_spot(caret: Area, point: bool) -> ((i32, i32), (u32, u32)) {
    let y = if point {
        caret.y.saturating_add_unsigned(caret.h)
    } else {
        caret.y
    };
    ((caret.x, y), (caret.w, caret.h))
}

/// The caret's area the system was last given.
#[derive(Debug, Default)]
pub struct Sent(Option<Area>);

impl Sent {
    /// The area to give the system now, if any: the caret's, when the
    /// input method is on and the area is not the one last given. An
    /// area given while the input method is off is dropped by the
    /// system, so none is.
    pub fn due(&mut self, on: bool, caret: Option<Area>) -> Option<Area> {
        let caret = caret.filter(|_| on)?;
        if self.0 == Some(caret) {
            return None;
        }
        self.0 = Some(caret);
        Some(caret)
    }

    /// Forgets the area, so the next one is given even when it is the
    /// same. The system has none in a new window and after a turn on,
    /// Wayland resets it at each `Enabled`, and under X11 a point is
    /// given again when a composition opens.
    pub fn forget(&mut self) {
        self.0 = None;
    }

    /// Whether the system has no area: none was given since the last
    /// `forget`.
    pub fn is_forgotten(&self) -> bool {
        self.0.is_none()
    }
}

/// Whether both ends of a cursor are within the text and on character
/// boundaries.
fn on_boundaries(text: &str, (from, to): (usize, usize)) -> bool {
    text.is_char_boundary(from) && text.is_char_boundary(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    const OWNERS: [KeyOwner; 8] = [
        KeyOwner::Question,
        KeyOwner::Dialog { field: false },
        KeyOwner::Dialog { field: true },
        KeyOwner::Path,
        KeyOwner::GoTo,
        KeyOwner::Search,
        KeyOwner::Text,
        KeyOwner::Code,
    ];

    /// The setting on, with a caret in the editor and a name in a
    /// dialog's field.
    fn at(owner: KeyOwner, editing: bool) -> At {
        At::new(true, owner, editing, true, true)
    }

    /// The same place with the setting off.
    fn off(owner: KeyOwner, editing: bool) -> At {
        At::new(false, owner, editing, true, true)
    }

    /// The editor before it has a caret.
    fn no_caret() -> At {
        At::new(true, KeyOwner::Text, true, false, true)
    }

    fn editor() -> At {
        at(KeyOwner::Text, true)
    }

    fn composing(text: &str, cursor: Option<(usize, usize)>) -> Event<'_> {
        Event::Composing { text, cursor }
    }

    fn shown(text: &str, cursor: Option<(usize, usize)>) -> Option<Shown<'_>> {
        Some(Shown { text, cursor })
    }

    /// A composition that shows `text` in the editor.
    fn open(text: &str) -> Composition {
        let mut composition = Composition::default();
        composition.event(editor(), composing(text, None));
        assert_eq!(composition.shown(editor()), shown(text, None));
        composition
    }

    // The keys take text.

    #[test]
    fn the_keys_take_text_where_a_caret_types() {
        for owner in [KeyOwner::Text, KeyOwner::Code] {
            assert!(takes_text(owner, true, true, false), "the editor");
            assert!(!takes_text(owner, true, false, false), "no caret");
            assert!(!takes_text(owner, false, true, false), "reading mode");
        }
        for editing in [true, false] {
            for has_caret in [true, false] {
                let takes = |owner, name| takes_text(owner, editing, has_caret, name);
                assert!(takes(KeyOwner::Search, false), "the search bar");
                assert!(takes(KeyOwner::Path, false), "the path field");
                assert!(takes(KeyOwner::Dialog { field: true }, true), "a name");
                assert!(
                    !takes(KeyOwner::Dialog { field: true }, false),
                    "a hex color"
                );
                assert!(!takes(KeyOwner::GoTo, true), "digits");
                assert!(!takes(KeyOwner::Question, true), "a question");
                assert!(!takes(KeyOwner::Dialog { field: false }, true), "a list");
            }
        }
    }

    #[test]
    fn the_input_method_is_wanted_with_the_setting_on_where_the_keys_take_text() {
        for owner in OWNERS {
            for editing in [true, false] {
                let on = at(owner, editing);
                assert_eq!(on.wanted(), on.takes_text, "{owner:?}");
                assert!(!off(owner, editing).wanted(), "the setting off: {owner:?}");
            }
        }
    }

    // The composition, a test per row of the table of the events.

    #[test]
    fn a_composing_text_shows_with_its_cursor_and_asks_a_frame() {
        let mut composition = Composition::default();
        assert!(composition.is_empty());
        let outcome = composition.event(editor(), composing("ni", Some((2, 2))));
        assert_eq!(
            outcome,
            Outcome {
                redraw: true,
                commit: None
            }
        );
        assert_eq!(composition.shown(editor()), shown("ni", Some((2, 2))));
        let outcome = composition.event(editor(), composing("ni h", Some((0, 4))));
        assert!(outcome.redraw, "the next text takes its place");
        assert_eq!(composition.shown(editor()), shown("ni h", Some((0, 4))));
    }

    #[test]
    fn the_same_composing_text_again_asks_no_frame() {
        let mut composition = open("ni");
        let outcome = composition.event(editor(), composing("ni", None));
        assert_eq!(outcome, Outcome::default());
        let outcome = composition.event(editor(), composing("ni", Some((0, 2))));
        assert!(outcome.redraw, "the cursor moved");
    }

    #[test]
    fn an_empty_composing_text_closes_the_composition() {
        let mut composition = open("ni");
        let outcome = composition.event(editor(), composing("", None));
        assert_eq!(
            outcome,
            Outcome {
                redraw: true,
                commit: None
            }
        );
        assert!(composition.is_empty());
        let outcome = composition.event(editor(), composing("", Some((0, 0))));
        assert_eq!(outcome, Outcome::default(), "nothing showed");
    }

    #[test]
    fn a_commit_closes_the_composition_and_gives_its_text() {
        let mut composition = open("ni");
        let outcome = composition.event(editor(), Event::Commit("\u{4f60}"));
        assert_eq!(
            outcome,
            Outcome {
                redraw: true,
                commit: Some((CommitTo::Editor, "\u{4f60}"))
            }
        );
        assert!(composition.is_empty());
        // A keyboard chosen in fcitx5 types through commits, with no
        // composing text before them.
        let outcome = composition.event(editor(), Event::Commit("\u{43f}"));
        assert_eq!(
            outcome,
            Outcome {
                redraw: false,
                commit: Some((CommitTo::Editor, "\u{43f}"))
            }
        );
    }

    #[test]
    fn an_empty_commit_closes_the_composition_and_gives_nothing() {
        let mut composition = open("ni");
        let outcome = composition.event(editor(), Event::Commit(""));
        assert_eq!(
            outcome,
            Outcome {
                redraw: true,
                commit: None
            }
        );
        assert!(composition.is_empty());
    }

    #[test]
    fn enabled_changes_nothing_and_disabled_closes_the_composition() {
        let mut composition = open("ni");
        let outcome = composition.event(editor(), Event::Enabled);
        assert_eq!(outcome, Outcome::default());
        assert_eq!(composition.shown(editor()), shown("ni", None));
        let outcome = composition.event(editor(), Event::Disabled);
        assert_eq!(
            outcome,
            Outcome {
                redraw: true,
                commit: None
            }
        );
        assert!(composition.is_empty());
        let outcome = composition.event(editor(), Event::Disabled);
        assert_eq!(outcome, Outcome::default(), "nothing showed");
    }

    #[test]
    fn with_the_setting_off_each_event_is_ignored() {
        for owner in OWNERS {
            for editing in [true, false] {
                let off = off(owner, editing);
                let mut composition = Composition::default();
                for event in [
                    composing("ni", Some((2, 2))),
                    composing("", None),
                    Event::Commit("\u{4f60}"),
                    Event::Commit(""),
                    Event::Enabled,
                    Event::Disabled,
                ] {
                    let outcome = composition.event(off, event);
                    assert_eq!(outcome, Outcome::default(), "{owner:?} {event:?}");
                    assert!(composition.is_empty(), "{owner:?} {event:?}");
                }
            }
        }
    }

    #[test]
    fn a_composing_text_is_ignored_where_the_keys_take_no_text() {
        for (owner, editing) in [
            (KeyOwner::Text, false),
            (KeyOwner::Code, false),
            (KeyOwner::GoTo, true),
            (KeyOwner::Question, true),
            (KeyOwner::Dialog { field: false }, true),
        ] {
            let here = at(owner, editing);
            assert!(!here.takes_text, "{owner:?}");
            let mut composition = Composition::default();
            let outcome = composition.event(here, composing("ni", None));
            assert_eq!(outcome, Outcome::default(), "{owner:?}");
            assert!(composition.is_empty(), "{owner:?}");
        }
        let mut composition = Composition::default();
        composition.event(no_caret(), composing("ni", None));
        assert!(composition.is_empty(), "the editor with no caret");
    }

    #[test]
    fn a_composition_shows_in_each_place_that_takes_text() {
        for (owner, editing) in [
            (KeyOwner::Text, true),
            (KeyOwner::Code, true),
            (KeyOwner::Search, true),
            (KeyOwner::Search, false),
            (KeyOwner::Path, false),
            (KeyOwner::Dialog { field: true }, false),
        ] {
            let here = at(owner, editing);
            let mut composition = Composition::default();
            let outcome = composition.event(here, composing("ni", None));
            assert!(outcome.redraw, "{owner:?}");
            assert_eq!(composition.shown(here), shown("ni", None), "{owner:?}");
        }
    }

    // The cursor.

    #[test]
    fn a_cursor_is_nothing_a_point_or_a_range() {
        let mut composition = Composition::default();
        // Kana, then the clause being converted (`win-ms-J.log`).
        let text = "\u{306b}\u{307b}\u{3093}\u{3054}";
        for cursor in [
            None,
            Some((12, 12)),
            Some((0, 0)),
            Some((0, 6)),
            Some((6, 12)),
        ] {
            composition.event(editor(), composing(text, cursor));
            assert_eq!(composition.shown(editor()), shown(text, cursor));
        }
    }

    #[test]
    fn a_cursor_off_the_text_or_inside_a_character_is_no_cursor() {
        let mut composition = Composition::default();
        let text = "\u{4f60}\u{597d}";
        for cursor in [(1, 1), (0, 4), (2, 6), (0, 7), (7, 7), (usize::MAX, 0)] {
            composition.event(editor(), composing(text, Some(cursor)));
            assert_eq!(
                composition.shown(editor()),
                shown(text, None),
                "{cursor:?}: drawn at the end of the text"
            );
        }
        // The ends come in order, whichever way the system sent them.
        composition.event(editor(), composing(text, Some((6, 0))));
        assert_eq!(composition.shown(editor()), shown(text, Some((0, 6))));
        // A joined emoji is several characters (`win-W.log`).
        let family = "\u{1f468}\u{200d}\u{1f469}";
        composition.event(editor(), composing(family, Some((4, 7))));
        assert_eq!(composition.shown(editor()), shown(family, Some((4, 7))));
        composition.event(editor(), composing(family, Some((4, 5))));
        assert_eq!(composition.shown(editor()), shown(family, None));
    }

    // Each path that clears the composition.

    #[test]
    fn a_composition_shows_only_where_it_came() {
        let composition = open("ni");
        for owner in OWNERS {
            for editing in [true, false] {
                if (owner, editing) == (KeyOwner::Text, true) {
                    continue;
                }
                assert_eq!(composition.shown(at(owner, editing)), None, "{owner:?}");
            }
        }
        let bar = at(KeyOwner::Search, false);
        let mut composition = Composition::default();
        composition.event(bar, composing("ni", None));
        assert_eq!(
            composition.shown(at(KeyOwner::Search, true)),
            None,
            "the mode changed under the search bar"
        );
    }

    #[test]
    fn a_change_of_what_has_the_keys_drops_the_composition() {
        let mut composition = open("ni");
        assert!(!composition.settle(editor()), "nothing changed");
        assert_eq!(composition.shown(editor()), shown("ni", None));
        let bar = at(KeyOwner::Search, true);
        assert!(composition.settle(bar), "the search bar opened");
        assert!(composition.is_empty());
        assert!(!composition.settle(bar));
        assert_eq!(
            composition.shown(editor()),
            None,
            "back in the editor nothing shows again"
        );
    }

    #[test]
    fn a_composition_goes_on_where_the_keys_now_go() {
        // `Ctrl+F` during a composition under fcitx5: Oryx drops its
        // box, and the system's next composing text shows in the bar.
        let mut composition = open("ni");
        let bar = at(KeyOwner::Search, true);
        let outcome = composition.event(bar, composing("nih", None));
        assert!(outcome.redraw);
        assert_eq!(composition.shown(bar), shown("nih", None));
        assert_eq!(composition.shown(editor()), None);
        let outcome = composition.event(bar, Event::Commit("\u{4f60}"));
        assert_eq!(
            outcome.commit,
            Some((CommitTo::Field(Field::Search), "\u{4f60}"))
        );
    }

    #[test]
    fn the_rule_going_off_drops_the_composition() {
        let off = off(KeyOwner::Text, true);
        let mut composition = open("ni");
        assert_eq!(composition.shown(off), None, "the setting turned off");
        assert!(composition.settle(off));
        assert!(composition.is_empty());

        let mut composition = open("ni");
        assert_eq!(composition.shown(no_caret()), None);
        assert!(composition.settle(no_caret()));
        assert!(composition.is_empty());
    }

    // An event that finds a text held at another place, before
    // `settle` ran: the text does not come back with the place.

    #[test]
    fn a_composing_text_where_the_keys_take_no_text_forgets_the_one_held_elsewhere() {
        let mut composition = open("ni");
        let reading = at(KeyOwner::Text, false);
        let outcome = composition.event(reading, composing("nih", None));
        assert_eq!(outcome, Outcome::default());
        assert!(composition.is_empty());
        assert_eq!(composition.shown(editor()), None, "back in the editor");
    }

    #[test]
    fn an_event_with_the_setting_off_forgets_the_text_held() {
        for event in [Event::Enabled, Event::Disabled, composing("", None)] {
            let mut composition = open("ni");
            let outcome = composition.event(off(KeyOwner::Text, true), event);
            assert_eq!(outcome, Outcome::default(), "{event:?}");
            assert!(composition.is_empty(), "{event:?}");
            assert_eq!(composition.shown(editor()), None, "the setting on again");
        }
    }

    #[test]
    fn the_same_composing_text_at_another_place_shows_there() {
        let mut composition = open("ni");
        let bar = at(KeyOwner::Search, true);
        let outcome = composition.event(bar, composing("ni", None));
        assert!(outcome.redraw, "the same text, in the bar now");
        assert_eq!(composition.shown(bar), shown("ni", None));
        assert_eq!(composition.shown(editor()), None);
    }

    #[test]
    fn an_event_that_closes_at_another_place_forgets_the_text_and_asks_no_frame() {
        let bar = at(KeyOwner::Search, true);
        for event in [Event::Disabled, composing("", None), Event::Commit("")] {
            let mut composition = open("ni");
            let outcome = composition.event(bar, event);
            assert_eq!(
                outcome,
                Outcome::default(),
                "{event:?}: no box showed in the bar"
            );
            assert!(composition.is_empty(), "{event:?}");
            assert_eq!(composition.shown(editor()), None, "{event:?}");
        }
    }

    #[test]
    fn clear_forgets_the_composition() {
        // The window lost the focus, or another file opened.
        let mut composition = open("ni");
        assert!(composition.clear());
        assert!(composition.is_empty());
        assert_eq!(composition.shown(editor()), None);
        assert!(!composition.clear(), "nothing was held");
    }

    #[test]
    fn a_commit_where_a_composition_does_not_show_still_clears_it() {
        let mut composition = open("ni");
        let question = at(KeyOwner::Question, true);
        let outcome = composition.event(question, Event::Commit("\u{4f60}"));
        assert_eq!(outcome, Outcome::default(), "dropped, and no box showed");
        assert!(composition.is_empty());
    }

    // The gate of the presses.

    #[test]
    fn a_press_of_process_is_the_input_methods_with_the_setting_on() {
        let process = Key::Named(NamedKey::Process);
        assert!(drops_press(true, &process));
        assert!(!drops_press(false, &process), "the setting off: today's");
    }

    #[test]
    fn each_other_key_passes_the_gate() {
        let keys = [
            Key::Character(SmolStr::new("n")),
            Key::Character(SmolStr::new(",")),
            Key::Character(SmolStr::new("\u{4f60}")),
            Key::Dead(Some('^')),
            Key::Dead(None),
        ];
        let named = [
            NamedKey::Enter,
            NamedKey::Escape,
            NamedKey::Space,
            NamedKey::Tab,
            NamedKey::Backspace,
            NamedKey::Delete,
            NamedKey::Insert,
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
            NamedKey::ArrowUp,
            NamedKey::ArrowDown,
            NamedKey::Home,
            NamedKey::End,
            NamedKey::PageUp,
            NamedKey::PageDown,
            NamedKey::F1,
            NamedKey::F9,
            NamedKey::F12,
            NamedKey::Control,
            NamedKey::Shift,
            NamedKey::Alt,
            NamedKey::AltGraph,
            NamedKey::Super,
            NamedKey::CapsLock,
            NamedKey::ContextMenu,
            // The keys of the input methods themselves.
            NamedKey::Compose,
            NamedKey::Convert,
            NamedKey::NonConvert,
            NamedKey::Accept,
            NamedKey::ModeChange,
            NamedKey::AllCandidates,
            NamedKey::PreviousCandidate,
            NamedKey::Alphanumeric,
            NamedKey::KanaMode,
            NamedKey::KanjiMode,
            NamedKey::ZenkakuHankaku,
            NamedKey::HangulMode,
            NamedKey::HanjaMode,
            NamedKey::JunjaMode,
        ];
        let keys: Vec<Key> = keys
            .into_iter()
            .chain(named.into_iter().map(Key::Named))
            .collect();
        for key in &keys {
            for setting in [true, false] {
                assert!(!drops_press(setting, key), "{key:?}");
            }
        }
    }

    // The caret's rectangle given to the system.

    const BAR: Area = Area {
        x: 120,
        y: 48,
        w: 2,
        h: 22,
    };

    #[test]
    fn the_carets_area_is_sent_once_and_again_when_it_moves() {
        let mut sent = Sent::default();
        assert_eq!(sent.due(true, Some(BAR)), Some(BAR));
        assert_eq!(sent.due(true, Some(BAR)), None, "the same place");
        let moved = Area { x: 131, ..BAR };
        assert_eq!(sent.due(true, Some(moved)), Some(moved));
        assert_eq!(sent.due(true, Some(moved)), None);
        assert_eq!(sent.due(true, Some(BAR)), Some(BAR), "back where it was");
    }

    #[test]
    fn nothing_is_sent_while_the_input_method_is_off_or_the_caret_has_no_place() {
        let mut sent = Sent::default();
        assert_eq!(
            sent.due(false, Some(BAR)),
            None,
            "off: the system would drop it"
        );
        assert_eq!(
            sent.due(true, None),
            None,
            "a caret the layout does not hold"
        );
        assert_eq!(
            sent.due(true, Some(BAR)),
            Some(BAR),
            "due at the first chance"
        );
        assert_eq!(sent.due(true, None), None);
        assert_eq!(sent.due(true, Some(BAR)), None, "the system still has it");
    }

    #[test]
    fn a_forgotten_area_is_sent_again() {
        // The system forgets the place at each turn on, and on Wayland
        // at each `Enabled`.
        let mut sent = Sent::default();
        assert!(sent.is_forgotten(), "nothing was given yet");
        assert_eq!(sent.due(true, Some(BAR)), Some(BAR));
        assert!(!sent.is_forgotten());
        sent.forget();
        assert!(sent.is_forgotten());
        assert_eq!(sent.due(false, Some(BAR)), None);
        assert!(sent.is_forgotten(), "nothing is given while off");
        assert_eq!(sent.due(true, Some(BAR)), Some(BAR));
        assert_eq!(sent.due(true, Some(BAR)), None);
    }

    #[test]
    fn the_system_gets_the_bar_and_under_x11_the_point_at_its_foot() {
        assert_eq!(caret_spot(BAR, false), ((120, 48), (2, 22)));
        assert_eq!(caret_spot(BAR, true), ((120, 70), (2, 22)));
    }

    // Where a commit goes, a test per row of its table.

    #[test]
    fn a_commit_in_the_editor_goes_to_the_editors_key() {
        for owner in [KeyOwner::Text, KeyOwner::Code] {
            assert_eq!(commit_to(owner, true), Some(CommitTo::Editor));
        }
    }

    #[test]
    fn a_commit_in_a_field_goes_to_the_fields_key() {
        for editing in [true, false] {
            assert_eq!(
                commit_to(KeyOwner::Search, editing),
                Some(CommitTo::Field(Field::Search)),
                "the search bar, its replace field included"
            );
            assert_eq!(
                commit_to(KeyOwner::Path, editing),
                Some(CommitTo::Field(Field::Path))
            );
            // The input method is off there: a commit comes only from
            // a key pressed before the switch, and the field's key
            // refuses what is not a digit or a colon.
            assert_eq!(
                commit_to(KeyOwner::GoTo, editing),
                Some(CommitTo::Field(Field::GoTo))
            );
        }
    }

    #[test]
    fn a_commit_in_a_dialogs_field_goes_to_the_dialogs_key() {
        for editing in [true, false] {
            assert_eq!(
                commit_to(KeyOwner::Dialog { field: true }, editing),
                Some(CommitTo::Dialog),
                "the rename field, the hex field"
            );
        }
    }

    #[test]
    fn a_commit_is_dropped_at_a_question_a_list_and_in_reading_mode() {
        for editing in [true, false] {
            assert_eq!(commit_to(KeyOwner::Question, editing), None);
            assert_eq!(commit_to(KeyOwner::Dialog { field: false }, editing), None);
        }
        assert_eq!(commit_to(KeyOwner::Text, false), None);
        assert_eq!(commit_to(KeyOwner::Code, false), None);
    }

    #[test]
    fn a_dropped_commit_gives_no_text() {
        let mut composition = Composition::default();
        for (owner, editing) in [
            (KeyOwner::Question, true),
            (KeyOwner::Dialog { field: false }, false),
            (KeyOwner::Text, false),
        ] {
            let outcome = composition.event(at(owner, editing), Event::Commit("d"));
            assert_eq!(outcome, Outcome::default(), "{owner:?}");
        }
        let outcome = composition.event(at(KeyOwner::GoTo, true), Event::Commit("12"));
        assert_eq!(outcome.commit, Some((CommitTo::Field(Field::GoTo), "12")));
    }
}
