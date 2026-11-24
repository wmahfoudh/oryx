//! The route of a key press: whether it is a keystroke, who is offered
//! it first, and what acts on a press its first reader left. The
//! window's key event asks these functions and runs their answers, so
//! the order is under tests that need no window.
//!
//! The order: the question, the four toggles, a dialog, a field, the
//! editor, the sidebar, then the command. A repeat of Escape is
//! dropped after the question and before the toggles.

use winit::keyboard::{Key, PhysicalKey};

use crate::edit::Mode;
use crate::input::keymap::{self, Command, KeyOwner};

/// What is open, of what takes the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    /// The unsaved changes or recovery question.
    pub question: bool,
    /// A dialog, with whether one of its text fields has the keys.
    pub dialog: Option<bool>,
    /// The path field.
    pub path: bool,
    /// The go to line field.
    pub goto: bool,
    /// The search bar.
    pub search: bool,
    /// The editor, not reading mode.
    pub editing: bool,
    /// The editor shows a markdown file's own bytes.
    pub markdown_source: bool,
}

impl Open {
    /// What has the keys: the first that is open in the order of the
    /// route, then the text.
    pub fn owner(self) -> KeyOwner {
        if self.question {
            KeyOwner::Question
        } else if let Some(field) = self.dialog {
            KeyOwner::Dialog { field }
        } else if self.path {
            KeyOwner::Path
        } else if self.goto {
            KeyOwner::GoTo
        } else if self.search {
            KeyOwner::Search
        } else if self.editing && !self.markdown_source {
            KeyOwner::Code
        } else {
            KeyOwner::Text
        }
    }
}

/// What had the keys when a key went down: the owner, the mode, and
/// whether the help page was open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldAt {
    pub owner: KeyOwner,
    pub mode: Mode,
    pub help: bool,
}

/// The keys that are down, each with what had the keys at its first
/// press.
#[derive(Debug, Default)]
pub struct Held(Vec<(PhysicalKey, HeldAt)>);

impl Held {
    /// Whether a press is a keystroke for what has the keys now, and
    /// the record of a first press.
    ///
    /// A synthetic press reports a key that was already held when
    /// focus arrived, not a keystroke. On Windows the Escape that
    /// cancels a native file dialog comes back exactly so when the
    /// window regains focus, and acting on it quit the app.
    ///
    /// The repeats of a held key go to what had the keys at its first
    /// press. Once that press closed a field or a dialog, or changed
    /// the mode, they are dropped until the key is pressed again: the
    /// Enter that closed the go to line field went on as line breaks
    /// in the text. Each key keeps its own record, so a Shift pressed
    /// while Enter is held does not renew Enter's.
    pub fn admits(&mut self, key: PhysicalKey, at: HeldAt, repeat: bool, synthetic: bool) -> bool {
        if synthetic {
            return false;
        }
        if !repeat {
            self.0.retain(|(held, _)| *held != key);
            self.0.push((key, at));
            return true;
        }
        !self
            .0
            .iter()
            .any(|(held, at_press)| *held == key && *at_press != at)
    }

    /// The key went up.
    pub fn release(&mut self, key: PhysicalKey) {
        self.0.retain(|(held, _)| *held != key);
    }

    /// The window lost the focus: the releases of the keys that are
    /// down go elsewhere.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The key and the Alt that the readers of a press get.
#[derive(Debug, Clone, Copy)]
pub struct Keys<'a> {
    /// Alt held, and off for a character typed with Option on a Mac.
    pub alt: bool,
    /// The key of a field and of the editor.
    pub key: &'a Key,
    /// The key of a dialog.
    pub panel_key: &'a Key,
}

/// The key each reader gets, from the key as typed and its Latin key
/// (`keymap::latin_key`).
///
/// A character typed with Alt held (Option on a Mac) is typing when no
/// chord of the keymap claims the key, and its readers get Alt off
/// (`keymap::alt_typing`). A field and the editor match a chord with
/// the Latin key, Ctrl or Alt alone held, and take the character as
/// typed otherwise (`keymap::chord_held`). The text fields of the
/// dialogs insert a character with Alt held (Option types one on a
/// Mac), so a dialog takes the Latin key with Ctrl and no Alt.
pub fn keys<'a>(
    typed: &'a Key,
    latin: &'a Key,
    ctrl: bool,
    alt: bool,
    claimed: bool,
    mac: bool,
) -> Keys<'a> {
    let alt = alt && !keymap::alt_typing(typed, ctrl, alt, claimed, mac);
    let key = if keymap::chord_held(ctrl, alt) {
        latin
    } else {
        typed
    };
    let panel_key = if ctrl && !alt { latin } else { typed };
    Keys {
        alt,
        key,
        panel_key,
    }
}

/// A field with a key function of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Path,
    GoTo,
    Search,
}

/// Who is offered a press first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum First {
    /// Nobody: the press is dropped.
    Nobody,
    /// The question, which keeps each key.
    Question,
    /// A toggle, run as a command: the themes, the settings, the
    /// sidebar or the open dialog.
    Toggle(Command),
    /// The open dialog, which keeps each key.
    Dialog,
    /// A field. What its key function leaves goes to `after`.
    Field(Field),
    /// The editor. What its key function leaves goes to `after`.
    Editor,
    /// The page in reading mode: nothing reads the key before `after`.
    Page,
}

/// Who is offered a press first, for what has the keys and the command
/// the keymap gave the press.
///
/// The question owns the keyboard outright until a decision lands. A
/// held key repeating into it answers nothing: the Enter that opened
/// it from the sidebar must not answer it too.
///
/// A held Escape repeats, and the ladder must not climb by itself:
/// each rung, the quit included, takes its own press. The same held
/// key kept repeating into the window after a dialog closed on it.
///
/// The toggles stay global so that their chord closes the dialog it
/// opened. Each other key feeds an open dialog.
pub fn first(owner: KeyOwner, editing: bool, repeat: bool, command: Option<Command>) -> First {
    let toggle = command.filter(|cmd| {
        matches!(
            cmd,
            Command::ThemeBrowser | Command::Settings | Command::Sidebar | Command::OpenFile
        )
    });
    match (owner, toggle) {
        (KeyOwner::Question, _) => {
            if repeat {
                First::Nobody
            } else {
                First::Question
            }
        }
        _ if repeat && command == Some(Command::Quit) => First::Nobody,
        (_, Some(cmd)) => First::Toggle(cmd),
        (KeyOwner::Dialog { .. }, None) => First::Dialog,
        (KeyOwner::Path, None) => First::Field(Field::Path),
        (KeyOwner::GoTo, None) => First::Field(Field::GoTo),
        (KeyOwner::Search, None) => First::Field(Field::Search),
        (KeyOwner::Text | KeyOwner::Code, None) => {
            if editing {
                First::Editor
            } else {
                First::Page
            }
        }
    }
}

/// What acts on a press its first reader left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// The command of the keymap.
    Command(Command),
    /// The selection of the sidebar moves by a row.
    SidebarMove(i32),
    /// The selected file of the sidebar opens in a new window.
    OpenBeside,
    /// The selected row of the sidebar is activated.
    SidebarEnter,
}

/// What acts on a press that `reader` left.
///
/// A field has the keyboard: what it did not claim acts only when it
/// is app-wide, which the command already holds to (`Command::live`).
/// Nothing reaches the editor's own keys, the document's editing
/// chords or the sidebar while a field is open.
///
/// With no field open, the sidebar takes the two arrows and Enter
/// while it owns the keys of its pane (`sidebar`), and the command
/// acts otherwise.
pub fn after(
    reader: First,
    command: Option<Command>,
    sidebar: bool,
    ctrl: bool,
    enter: bool,
) -> After {
    match reader {
        First::Nobody | First::Question | First::Toggle(_) | First::Dialog => After::Nothing,
        First::Field(_) => command.map_or(After::Nothing, After::Command),
        First::Editor | First::Page => match command {
            Some(Command::LineUp) if sidebar => After::SidebarMove(-1),
            Some(Command::LineDown) if sidebar => After::SidebarMove(1),
            None if sidebar && ctrl && enter => After::OpenBeside,
            None if sidebar && enter => After::SidebarEnter,
            Some(cmd) => After::Command(cmd),
            None => After::Nothing,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode, NamedKey, NativeKeyCode};

    const CLOSED: Open = Open {
        question: false,
        dialog: None,
        path: false,
        goto: false,
        search: false,
        editing: false,
        markdown_source: false,
    };

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

    fn chr(s: &str) -> Key {
        Key::Character(s.into())
    }

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    /// The command of a press for `owner`, the keycap being the
    /// character typed, as on Linux and Windows.
    fn command(typed: &Key, ctrl: bool, shift: bool, owner: KeyOwner) -> Option<Command> {
        keymap::command(
            keymap::Press {
                typed,
                keycap: typed,
                code: PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
                ctrl,
                shift,
                alt: false,
            },
            owner,
        )
    }

    #[test]
    fn what_has_the_keys_is_the_first_open_in_the_order_of_the_route() {
        assert_eq!(CLOSED.owner(), KeyOwner::Text, "the page, reading");
        let editing = Open {
            editing: true,
            ..CLOSED
        };
        assert_eq!(editing.owner(), KeyOwner::Code, "a code or a text file");
        let markdown = Open {
            markdown_source: true,
            ..editing
        };
        assert_eq!(markdown.owner(), KeyOwner::Text, "markdown source");
        let search = Open {
            search: true,
            ..editing
        };
        assert_eq!(search.owner(), KeyOwner::Search);
        let goto = Open {
            goto: true,
            ..search
        };
        assert_eq!(goto.owner(), KeyOwner::GoTo, "before the search bar");
        let path = Open { path: true, ..goto };
        assert_eq!(path.owner(), KeyOwner::Path, "before Go to line");
        let dialog = Open {
            dialog: Some(false),
            ..path
        };
        assert_eq!(dialog.owner(), KeyOwner::Dialog { field: false });
        let field = Open {
            dialog: Some(true),
            ..path
        };
        assert_eq!(field.owner(), KeyOwner::Dialog { field: true });
        let question = Open {
            question: true,
            ..field
        };
        assert_eq!(question.owner(), KeyOwner::Question, "before a dialog");
    }

    const KEY_A: PhysicalKey = PhysicalKey::Code(KeyCode::KeyA);
    const ENTER: PhysicalKey = PhysicalKey::Code(KeyCode::Enter);
    const SHIFT: PhysicalKey = PhysicalKey::Code(KeyCode::ShiftLeft);

    fn at(owner: KeyOwner) -> HeldAt {
        HeldAt {
            owner,
            mode: Mode::Edit,
            help: false,
        }
    }

    #[test]
    fn the_repeats_of_a_key_go_to_what_had_the_keys_at_its_press() {
        let mut held = Held::default();
        let field = at(KeyOwner::GoTo);
        let text = at(KeyOwner::Code);
        assert!(held.admits(ENTER, field, false, false), "the first press");
        assert!(held.admits(ENTER, field, true, false), "its repeat");
        assert!(
            !held.admits(ENTER, text, true, false),
            "a repeat once the press closed the field"
        );
        assert!(
            !held.admits(ENTER, text, true, false),
            "and each repeat after it"
        );
        assert!(
            held.admits(ENTER, text, false, false),
            "the key pressed again"
        );
        assert!(held.admits(ENTER, text, true, false), "then its repeats");
    }

    #[test]
    fn the_mode_and_the_help_page_are_part_of_what_had_the_keys() {
        let mut held = Held::default();
        let reading = HeldAt {
            owner: KeyOwner::Text,
            mode: Mode::Read,
            help: false,
        };
        let editing = HeldAt {
            mode: Mode::Edit,
            ..reading
        };
        let help = HeldAt {
            help: true,
            ..reading
        };
        assert!(held.admits(KEY_A, reading, false, false));
        assert!(
            !held.admits(KEY_A, editing, true, false),
            "the same owner in another mode"
        );
        assert!(
            !held.admits(KEY_A, help, true, false),
            "the same owner on the help page"
        );
        assert!(held.admits(KEY_A, reading, true, false), "back as it was");
    }

    #[test]
    fn each_held_key_keeps_its_own_record() {
        let mut held = Held::default();
        let field = at(KeyOwner::GoTo);
        let text = at(KeyOwner::Code);
        assert!(held.admits(ENTER, field, false, false));
        assert!(
            held.admits(SHIFT, text, false, false),
            "Shift pressed after the field closed"
        );
        assert!(
            !held.admits(ENTER, text, true, false),
            "Shift did not renew the record of Enter"
        );
        assert!(held.admits(SHIFT, text, true, false));
    }

    #[test]
    fn a_synthetic_press_is_no_keystroke_and_leaves_no_record() {
        let mut held = Held::default();
        let field = at(KeyOwner::GoTo);
        let text = at(KeyOwner::Code);
        assert!(
            !held.admits(ENTER, field, false, true),
            "a key that was down when the focus came"
        );
        assert!(!held.admits(ENTER, field, true, true));
        assert!(
            held.admits(ENTER, text, true, false),
            "a repeat with no record goes on: the synthetic press left none"
        );
    }

    #[test]
    fn a_release_and_a_cleared_record_forget_the_key() {
        let mut held = Held::default();
        let field = at(KeyOwner::GoTo);
        let text = at(KeyOwner::Code);
        assert!(held.admits(ENTER, field, false, false));
        assert!(held.admits(KEY_A, field, false, false));
        held.release(ENTER);
        assert!(
            held.admits(ENTER, text, true, false),
            "no record after the release"
        );
        assert!(
            !held.admits(KEY_A, text, true, false),
            "the other key is still down"
        );
        held.clear();
        assert!(
            held.admits(KEY_A, text, true, false),
            "the window lost the focus"
        );
    }

    #[test]
    fn a_field_and_the_editor_get_the_latin_key_with_one_modifier_held() {
        let typed = chr("\u{441}");
        let latin = chr("c");
        let mac = false;
        let plain = keys(&typed, &latin, false, false, false, mac);
        assert_eq!((plain.key, plain.alt), (&typed, false), "a bare press");
        let ctrl = keys(&typed, &latin, true, false, true, mac);
        assert_eq!((ctrl.key, ctrl.alt), (&latin, false), "Ctrl alone");
        let alt = keys(&typed, &latin, false, true, false, mac);
        assert_eq!((alt.key, alt.alt), (&latin, true), "Alt alone");
        let altgr = keys(&typed, &latin, true, true, false, mac);
        assert_eq!(
            (altgr.key, altgr.alt),
            (&typed, true),
            "Ctrl and Alt, an AltGr character on Windows"
        );
    }

    #[test]
    fn a_dialog_gets_the_latin_key_with_ctrl_and_no_alt() {
        let typed = chr("\u{441}");
        let latin = chr("c");
        let mac = false;
        assert_eq!(
            keys(&typed, &latin, false, false, false, mac).panel_key,
            &typed
        );
        assert_eq!(
            keys(&typed, &latin, true, false, true, mac).panel_key,
            &latin
        );
        assert_eq!(
            keys(&typed, &latin, false, true, false, mac).panel_key,
            &typed,
            "its fields insert a character with Alt held"
        );
        assert_eq!(
            keys(&typed, &latin, true, true, false, mac).panel_key,
            &typed
        );
    }

    #[test]
    fn an_option_character_on_a_mac_reaches_the_readers_with_alt_off() {
        let typed = chr("\u{2202}");
        let latin = chr("d");
        let mac = true;
        let typing = keys(&typed, &latin, false, true, false, mac);
        assert_eq!(
            (typing.key, typing.panel_key, typing.alt),
            (&typed, &typed, false),
            "Option+D types its character"
        );
        let claimed = keys(&typed, &latin, false, true, true, mac);
        assert!(claimed.alt, "a chord of the keymap claims the key");
        let elsewhere = keys(&typed, &latin, false, true, false, !mac);
        assert!(elsewhere.alt, "on Linux and Windows Alt stays");
        let enter = named(NamedKey::Enter);
        assert!(
            keys(&enter, &enter, false, true, false, mac).alt,
            "a named key types nothing"
        );
    }

    #[test]
    fn the_question_keeps_each_key_and_a_repeat_answers_nothing() {
        let owner = KeyOwner::Question;
        for editing in [false, true] {
            for cmd in [
                None,
                Some(Command::Quit),
                Some(Command::Settings),
                Some(Command::Find),
            ] {
                assert_eq!(first(owner, editing, false, cmd), First::Question);
                assert_eq!(first(owner, editing, true, cmd), First::Nobody);
                assert_eq!(
                    after(First::Question, cmd, true, true, true),
                    After::Nothing
                );
            }
        }
    }

    #[test]
    fn a_held_escape_acts_once_under_each_owner() {
        let escape = named(NamedKey::Escape);
        for owner in OWNERS {
            let cmd = command(&escape, false, false, owner);
            assert_eq!(cmd, Some(Command::Quit), "{owner:?}");
            for editing in [false, true] {
                assert_eq!(first(owner, editing, true, cmd), First::Nobody, "{owner:?}");
            }
        }
        assert_eq!(
            first(KeyOwner::Search, false, false, Some(Command::Quit)),
            First::Field(Field::Search),
            "the first press is the bar's"
        );
        assert_eq!(
            first(KeyOwner::Search, false, true, Some(Command::Find)),
            First::Field(Field::Search),
            "another key repeats"
        );
    }

    #[test]
    fn the_four_toggles_come_before_a_dialog_a_field_and_the_editor() {
        let toggles = [
            (chr("t"), false, Command::ThemeBrowser),
            (chr(","), false, Command::Settings),
            (chr("B"), true, Command::Sidebar),
            (chr("o"), false, Command::OpenFile),
        ];
        for owner in OWNERS {
            for (key, shift, toggle) in &toggles {
                let cmd = command(key, true, *shift, owner);
                assert_eq!(cmd, Some(*toggle), "{owner:?}");
                let expected = if owner == KeyOwner::Question {
                    First::Question
                } else {
                    First::Toggle(*toggle)
                };
                for editing in [false, true] {
                    assert_eq!(first(owner, editing, false, cmd), expected, "{owner:?}");
                    assert_eq!(
                        first(owner, editing, true, cmd),
                        if owner == KeyOwner::Question {
                            First::Nobody
                        } else {
                            expected
                        },
                        "a held toggle repeats, {owner:?}"
                    );
                }
                assert_eq!(
                    after(First::Toggle(*toggle), cmd, true, true, true),
                    After::Nothing
                );
            }
        }
    }

    #[test]
    fn a_dialog_keeps_each_key_but_the_toggles() {
        for field in [false, true] {
            let owner = KeyOwner::Dialog { field };
            let find = command(&chr("f"), true, false, owner);
            assert_eq!(find, Some(Command::Find));
            let escape = command(&named(NamedKey::Escape), false, false, owner);
            for cmd in [None, find, escape, Some(Command::LineUp)] {
                for editing in [false, true] {
                    assert_eq!(first(owner, editing, false, cmd), First::Dialog);
                }
                assert_eq!(after(First::Dialog, cmd, true, true, true), After::Nothing);
            }
        }
    }

    #[test]
    fn a_field_reads_first_and_the_command_acts_on_what_it_left() {
        let fields = [
            (KeyOwner::Path, Field::Path),
            (KeyOwner::GoTo, Field::GoTo),
            (KeyOwner::Search, Field::Search),
        ];
        for (owner, field) in fields {
            let reader = First::Field(field);
            let find = command(&chr("f"), true, false, owner);
            assert_eq!(find, Some(Command::Find), "app-wide under a field");
            let bold = command(&chr("b"), true, false, owner);
            assert_eq!(bold, None, "no receiver under a field");
            let up = command(&named(NamedKey::ArrowUp), false, false, owner);
            assert_eq!(up, Some(Command::LineUp));
            for editing in [false, true] {
                for cmd in [None, find, up, Some(Command::Quit)] {
                    assert_eq!(first(owner, editing, false, cmd), reader, "{owner:?}");
                }
            }
            for sidebar in [false, true] {
                assert_eq!(
                    after(reader, find, sidebar, true, false),
                    After::Command(Command::Find)
                );
                assert_eq!(
                    after(reader, up, sidebar, false, false),
                    After::Command(Command::LineUp),
                    "the sidebar has no arrow under a field"
                );
                assert_eq!(
                    after(reader, None, sidebar, false, true),
                    After::Nothing,
                    "nor Enter"
                );
                assert_eq!(after(reader, None, sidebar, true, true), After::Nothing);
            }
        }
    }

    #[test]
    fn the_editor_reads_before_the_sidebar_and_the_command() {
        for owner in [KeyOwner::Text, KeyOwner::Code] {
            for cmd in [None, Some(Command::LineUp), Some(Command::Save)] {
                assert_eq!(first(owner, true, false, cmd), First::Editor, "{owner:?}");
            }
        }
        for cmd in [None, Some(Command::LineUp), Some(Command::Save)] {
            assert_eq!(
                first(KeyOwner::Text, false, false, cmd),
                First::Page,
                "reading: no reader before the sidebar and the command"
            );
        }
    }

    #[test]
    fn the_sidebar_takes_the_arrows_and_enter_that_the_editor_left() {
        for reader in [First::Editor, First::Page] {
            let sidebar = true;
            assert_eq!(
                after(reader, Some(Command::LineUp), sidebar, false, false),
                After::SidebarMove(-1)
            );
            assert_eq!(
                after(reader, Some(Command::LineDown), sidebar, false, false),
                After::SidebarMove(1)
            );
            assert_eq!(
                after(reader, None, sidebar, false, true),
                After::SidebarEnter
            );
            assert_eq!(after(reader, None, sidebar, true, true), After::OpenBeside);
            assert_eq!(
                after(reader, None, sidebar, true, false),
                After::Nothing,
                "a key with no command that is not Enter"
            );
            assert_eq!(
                after(reader, Some(Command::PageDown), sidebar, false, false),
                After::Command(Command::PageDown),
                "the other commands are the page's"
            );
            assert_eq!(
                after(reader, Some(Command::Find), sidebar, true, true),
                After::Command(Command::Find),
                "Enter with a command is the command's"
            );
            let page = false;
            assert_eq!(
                after(reader, Some(Command::LineUp), page, false, false),
                After::Command(Command::LineUp)
            );
            assert_eq!(
                after(reader, Some(Command::LineDown), page, false, false),
                After::Command(Command::LineDown)
            );
            assert_eq!(after(reader, None, page, false, true), After::Nothing);
            assert_eq!(after(reader, None, page, true, true), After::Nothing);
        }
    }

    /// What the window does with a press, in the order it does it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Act {
        ConfirmKey,
        Run(Command),
        DialogKey,
        PathKey,
        GotoKey,
        SearchKey,
        EditKey,
        SidebarMove(i32),
        OpenBeside,
        SidebarEnter,
        WakeFieldCaret,
    }

    /// One press past the record of the held keys: what is open, the
    /// sidebar having the keys of its pane, the press, and whether the
    /// key function that is asked takes it.
    #[derive(Debug, Clone, Copy)]
    struct Case {
        open: Open,
        sidebar: bool,
        repeat: bool,
        resolved: Option<Command>,
        ctrl: bool,
        enter: bool,
        takes: bool,
    }

    /// A key function of the window: it answers false with its field
    /// closed, and else is asked and answers `takes`.
    fn key_fn(open: bool, act: Act, takes: bool, acts: &mut Vec<Act>) -> bool {
        if !open {
            return false;
        }
        acts.push(act);
        takes
    }

    /// The key arm of `window_event` as it stood before the route, its
    /// lines in their order (`315ce35`, `src/app.rs` 10215 to 10348):
    /// the guards read the window's own fields, and a key function is
    /// asked in a guard.
    fn ladder(case: Case) -> Vec<Act> {
        let Case {
            open,
            sidebar,
            repeat,
            resolved,
            ctrl,
            enter,
            takes,
        } = case;
        let mut acts = Vec::new();
        if open.question {
            if !repeat {
                acts.push(Act::ConfirmKey);
            }
            return acts;
        }
        if repeat && resolved == Some(Command::Quit) {
            return acts;
        }
        match resolved {
            Some(
                cmd @ (Command::ThemeBrowser
                | Command::Settings
                | Command::Sidebar
                | Command::OpenFile),
            ) => acts.push(Act::Run(cmd)),
            _ if open.dialog.is_some() => acts.push(Act::DialogKey),
            _ if key_fn(open.path, Act::PathKey, takes, &mut acts) => {}
            _ if open.path => {
                if let Some(cmd) = resolved {
                    acts.push(Act::Run(cmd));
                }
            }
            _ if key_fn(open.goto, Act::GotoKey, takes, &mut acts) => {}
            _ if open.goto => {
                if let Some(cmd) = resolved {
                    acts.push(Act::Run(cmd));
                }
            }
            _ if key_fn(open.search, Act::SearchKey, takes, &mut acts) => {}
            _ if open.search => {
                if let Some(cmd) = resolved {
                    acts.push(Act::Run(cmd));
                }
            }
            _ if open.editing && key_fn(true, Act::EditKey, takes, &mut acts) => {}
            Some(Command::LineUp) if sidebar => acts.push(Act::SidebarMove(-1)),
            Some(Command::LineDown) if sidebar => acts.push(Act::SidebarMove(1)),
            None if sidebar && ctrl && enter => acts.push(Act::OpenBeside),
            None if sidebar && enter => acts.push(Act::SidebarEnter),
            Some(cmd) => acts.push(Act::Run(cmd)),
            None => {}
        }
        acts.push(Act::WakeFieldCaret);
        acts
    }

    /// The key arm of `window_event` as it asks the route.
    fn routed(case: Case) -> Vec<Act> {
        let Case {
            open,
            sidebar,
            repeat,
            resolved,
            ctrl,
            enter,
            takes,
        } = case;
        let mut acts = Vec::new();
        let reader = first(open.owner(), open.editing, repeat, resolved);
        let taken = match reader {
            First::Nobody => return acts,
            First::Question => {
                acts.push(Act::ConfirmKey);
                return acts;
            }
            First::Toggle(cmd) => {
                acts.push(Act::Run(cmd));
                true
            }
            First::Dialog => {
                acts.push(Act::DialogKey);
                true
            }
            First::Field(Field::Path) => key_fn(true, Act::PathKey, takes, &mut acts),
            First::Field(Field::GoTo) => key_fn(true, Act::GotoKey, takes, &mut acts),
            First::Field(Field::Search) => key_fn(true, Act::SearchKey, takes, &mut acts),
            First::Editor => key_fn(true, Act::EditKey, takes, &mut acts),
            First::Page => false,
        };
        if !taken {
            match after(reader, resolved, sidebar, ctrl, enter) {
                After::Nothing => {}
                After::Command(cmd) => acts.push(Act::Run(cmd)),
                After::SidebarMove(by) => acts.push(Act::SidebarMove(by)),
                After::OpenBeside => acts.push(Act::OpenBeside),
                After::SidebarEnter => acts.push(Act::SidebarEnter),
            }
        }
        acts.push(Act::WakeFieldCaret);
        acts
    }

    /// Each combination of what is open, of the press and of the key
    /// function's answer, with each command of the keymap and with
    /// none: 313,344 cases. The editor's flags follow the window, where
    /// the markdown source view is a state of the editor.
    #[test]
    fn the_route_does_what_the_key_arm_did_in_each_state() {
        let flags = [false, true];
        let commands: Vec<Option<Command>> = std::iter::once(None)
            .chain(Command::ALL.into_iter().map(Some))
            .collect();
        let mut cases = 0;
        for question in flags {
            for dialog in [None, Some(false), Some(true)] {
                for path in flags {
                    for goto in flags {
                        for search in flags {
                            for (editing, markdown_source) in
                                [(false, false), (true, false), (true, true)]
                            {
                                let open = Open {
                                    question,
                                    dialog,
                                    path,
                                    goto,
                                    search,
                                    editing,
                                    markdown_source,
                                };
                                for bits in 0..32u8 {
                                    for &resolved in &commands {
                                        let case = Case {
                                            open,
                                            sidebar: bits & 1 != 0,
                                            repeat: bits & 2 != 0,
                                            ctrl: bits & 4 != 0,
                                            enter: bits & 8 != 0,
                                            takes: bits & 16 != 0,
                                            resolved,
                                        };
                                        assert_eq!(routed(case), ladder(case), "{case:?}");
                                        cases += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2 * 3 * 2 * 2 * 2 * 3 * 32 * 68);
    }
}
