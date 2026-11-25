//! The input method off the window: the replays of the logs of the
//! probe, and the fuzzer of the composition, of the gate of the presses
//! and of the route of a commit against a plain model.
//!
//! The logs under `tests/fixtures/input-logs/` are what winit handed
//! to the window in the sessions of November 2026, with fcitx5 and
//! ibus on Wayland and X11, and with Microsoft Pinyin, the Japanese
//! and Korean input methods and Weasel on Windows.

use oryx::input::keymap::KeyOwner;
use oryx::input::log::{self, KeyName, Record};
use oryx::input::method::{self, At, CommitTo, Composition, Event, Outcome, Shown};
use oryx::input::route::Field;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use winit::keyboard::{Key, NamedKey, NativeKey, SmolStr};

// The replays.

/// What has the keys, and whether the editor is open.
type Place = (KeyOwner, bool);

const EDITOR: CommitTo = CommitTo::Editor;
const SEARCH: CommitTo = CommitTo::Field(Field::Search);

/// A log, the presses of `Process` it holds, and the commits that are
/// not empty, in their order, each with the reader that types it.
struct Log {
    name: &'static str,
    process: usize,
    commits: &'static [(CommitTo, &'static str)],
}

const LOGS: &[Log] = &[
    Log {
        name: "win-W",
        process: 0,
        commits: &[(EDITOR, "😍❤❤🤣")],
    },
    Log {
        name: "win-fr-F",
        process: 0,
        commits: &[],
    },
    Log {
        name: "win-ms-J",
        process: 28,
        commits: &[(EDITOR, "今日はいい天気です"), (SEARCH, "日本語")],
    },
    Log {
        name: "win-ms-K",
        process: 25,
        commits: &[
            (EDITOR, "안"),
            (EDITOR, "녕"),
            (EDITOR, "하"),
            (EDITOR, "안"),
            (EDITOR, "녕"),
            (EDITOR, "하"),
            (SEARCH, "한"),
            (SEARCH, "글"),
        ],
    },
    Log {
        name: "win-pinyin-B",
        process: 37,
        commits: &[
            (EDITOR, "nihaonihao"),
            (EDITOR, "你好"),
            (EDITOR, "你号"),
            (EDITOR, "你"),
        ],
    },
    Log {
        name: "win-pinyin-B2",
        process: 39,
        commits: &[
            (EDITOR, "鸟"),
            (EDITOR, "nihao"),
            (EDITOR, "你好"),
            (EDITOR, "你啊ho"),
            (EDITOR, "你"),
        ],
    },
    Log {
        name: "win-pinyin-C1",
        process: 34,
        commits: &[
            (SEARCH, "你好"),
            (SEARCH, "nihao"),
            (SEARCH, "你好"),
            (SEARCH, "你号"),
        ],
    },
    Log {
        name: "win-pinyin-D1",
        process: 36,
        commits: &[(EDITOR, "ni'ni"), (EDITOR, "你好")],
    },
    Log {
        name: "win-ru-E",
        process: 0,
        commits: &[],
    },
    Log {
        name: "win-weasel-B",
        process: 34,
        commits: &[
            (SEARCH, "你好"),
            (SEARCH, "nihao"),
            (SEARCH, "你好"),
            (SEARCH, "妳好"),
        ],
    },
    Log {
        name: "win-weasel-D",
        process: 25,
        commits: &[(EDITOR, "你好")],
    },
    Log {
        name: "wl-B",
        process: 0,
        commits: &[
            (EDITOR, "鸟"),
            (EDITOR, "你好"),
            (EDITOR, "nihao"),
            (EDITOR, "ni ha"),
            (EDITOR, "d"),
            (EDITOR, "👋"),
            (EDITOR, "👋"),
            (EDITOR, "你哦"),
        ],
    },
    Log {
        name: "wl-B2",
        process: 0,
        commits: &[
            (EDITOR, "你好"),
            (EDITOR, "nihao"),
            (EDITOR, "你和"),
            (EDITOR, "你好吗"),
            (EDITOR, "你"),
        ],
    },
    Log {
        name: "wl-C2",
        process: 0,
        commits: &[
            (SEARCH, "你好"),
            (SEARCH, "nihao"),
            (SEARCH, "你好"),
            (SEARCH, "nihao"),
            (SEARCH, "你好"),
            (SEARCH, "👋"),
        ],
    },
    Log {
        name: "wl-D2",
        process: 0,
        commits: &[
            (EDITOR, "ni"),
            (EDITOR, "ni"),
            (EDITOR, "你好"),
            (EDITOR, "ni"),
        ],
    },
    Log {
        name: "wl-E",
        process: 0,
        commits: &[
            (EDITOR, "й"),
            (EDITOR, "ц"),
            (EDITOR, "у"),
            (SEARCH, "й"),
            (SEARCH, "ц"),
            (SEARCH, "у"),
            (EDITOR, "ф"),
        ],
    },
    Log {
        name: "wl-F",
        process: 0,
        commits: &[(EDITOR, "ê"), (EDITOR, "€"), (SEARCH, "ê"), (SEARCH, "€")],
    },
    Log {
        name: "wl-G",
        process: 0,
        commits: &[
            (EDITOR, "你好"),
            (EDITOR, "你好"),
            (EDITOR, "ni"),
            (EDITOR, "你好"),
        ],
    },
    Log {
        name: "x11-B",
        process: 0,
        commits: &[
            (EDITOR, "你好"),
            (EDITOR, "nihao"),
            (EDITOR, "你好"),
            (EDITOR, "👋"),
            (EDITOR, "你"),
            (EDITOR, "你"),
        ],
    },
    Log {
        name: "x11-C",
        process: 0,
        commits: &[
            (SEARCH, "你好"),
            (SEARCH, "nihao"),
            (SEARCH, "你好"),
            (SEARCH, "👋"),
        ],
    },
    Log {
        name: "x11-D",
        process: 0,
        commits: &[(EDITOR, "你好")],
    },
    Log {
        name: "x11-E",
        process: 0,
        commits: &[
            (EDITOR, "й"),
            (EDITOR, "ц"),
            (EDITOR, "у"),
            (SEARCH, "й"),
            (SEARCH, "ц"),
            (SEARCH, "у"),
            (EDITOR, "ф"),
        ],
    },
    Log {
        name: "x11-F",
        process: 0,
        commits: &[(EDITOR, "ê"), (EDITOR, "€"), (SEARCH, "ê"), (SEARCH, "€")],
    },
    Log {
        name: "x11-G",
        process: 0,
        commits: &[
            (EDITOR, "你好"),
            (EDITOR, "你好"),
            (EDITOR, "你好"),
            (EDITOR, "I"),
            (EDITOR, "阿门"),
            (EDITOR, "热捧饿啊听"),
            (EDITOR, "你好"),
            (EDITOR, "你好"),
            (EDITOR, "你好"),
        ],
    },
    Log {
        name: "xvfb-ibus-B",
        process: 0,
        commits: &[
            (EDITOR, "你好"),
            (EDITOR, "nihao"),
            (EDITOR, "你和"),
            (EDITOR, "你好吗"),
        ],
    },
    Log {
        name: "xvfb-ibus-D2",
        process: 0,
        commits: &[(EDITOR, "你")],
    },
    Log {
        name: "xvfb-ibus-D3",
        process: 0,
        commits: &[(EDITOR, "你")],
    },
    Log {
        name: "xvfb-ibus-D6",
        process: 0,
        commits: &[(EDITOR, "你好")],
    },
    Log {
        name: "xvfb-ibus-D7b",
        process: 0,
        commits: &[(EDITOR, "你好")],
    },
    Log {
        name: "xvfb-ibus-F6",
        process: 0,
        commits: &[(EDITOR, "号"), (EDITOR, "你好")],
    },
];

fn read(name: &str) -> Vec<Record> {
    let path = format!(
        "{}/tests/fixtures/input-logs/{name}.log",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter_map(|line| log::parse(line).unwrap_or_else(|e| panic!("{name}: {e}")))
        .map(|(_, record)| record)
        .collect()
}

/// The place a key record names, and whether it is a press.
fn key_place(record: &Record) -> Option<(Place, bool)> {
    match record {
        Record::Key(key) => Some(((key.owner, key.editing), key.pressed)),
        _ => None,
    }
}

/// Whether the keys take text at a place. A log does not say whether
/// the editor had a caret or which field a dialog had: the editor of
/// the sessions had its caret, and no session opened the hex field.
fn takes(place: Place) -> bool {
    method::takes_text(place.0, place.1, true, true)
}

/// What had the keys when each record arrived. A key record says it.
/// An event of the input method has no place in a log, so it is read
/// from the key records around it:
///
/// - the same place before and after: that place;
/// - a press before, which may have changed the place: the place of
///   the next key record;
/// - a release before, so a click changed the place, and a click has
///   no line in a log: the place before, until the window's own
///   `allowed` line that the change explains, and the place after it
///   from there.
fn places(records: &[Record]) -> Vec<Place> {
    let keys: Vec<Option<(Place, bool)>> = records.iter().map(key_place).collect();
    let mut before = vec![None; records.len()];
    let mut last = None;
    for (at, key) in keys.iter().enumerate() {
        before[at] = last;
        if key.is_some() {
            last = Some(at);
        }
    }
    let mut after = vec![None; records.len()];
    let mut next = None;
    for (at, key) in keys.iter().enumerate().rev() {
        after[at] = next;
        if key.is_some() {
            next = Some(at);
        }
    }
    let mut focused = vec![true; records.len()];
    let mut focus = true;
    for (at, record) in records.iter().enumerate() {
        if let Record::Focus(on) = record {
            focus = *on;
        }
        focused[at] = focus;
    }
    (0..records.len())
        .map(|at| {
            if let Some((place, _)) = keys[at] {
                return place;
            }
            let prev = before[at].and_then(|k| keys[k].map(|key| (k, key)));
            let next = after[at].and_then(|k| keys[k].map(|key| (k, key)));
            match (prev, next) {
                (None, None) => (KeyOwner::Text, false),
                (None, Some((_, (place, _)))) | (Some((_, (place, _))), None) => place,
                (Some((_, (a, _))), Some((_, (b, _)))) if a == b => a,
                (Some((_, (_, true))), Some((_, (b, _)))) => b,
                (Some((from, (a, false))), Some((to, (b, _)))) => {
                    let flip = (from + 1..to).find(|&j| {
                        focused[j]
                            && takes(a) != takes(b)
                            && records[j] == Record::Allowed(takes(b))
                    });
                    if flip.is_some_and(|flip| flip <= at) {
                        b
                    } else {
                        a
                    }
                }
            }
        })
        .collect()
}

/// The named keys the logs hold.
const NAMED: [NamedKey; 19] = [
    NamedKey::Process,
    NamedKey::Control,
    NamedKey::Shift,
    NamedKey::Alt,
    NamedKey::AltGraph,
    NamedKey::Super,
    NamedKey::Enter,
    NamedKey::Escape,
    NamedKey::Space,
    NamedKey::Tab,
    NamedKey::Backspace,
    NamedKey::End,
    NamedKey::ArrowUp,
    NamedKey::ArrowDown,
    NamedKey::ArrowLeft,
    NamedKey::ArrowRight,
    NamedKey::F9,
    NamedKey::ZoomToggle,
    NamedKey::Hankaku,
];

/// The key of a record. A log names a key by the name of winit's
/// variant, which is not read back: a name that is not in `NAMED`
/// stops the test, and is added there.
fn key_of(name: &KeyName) -> Key {
    match name {
        KeyName::Named(named) => NAMED
            .iter()
            .find(|key| format!("{key:?}") == *named)
            .map(|key| Key::Named(*key))
            .unwrap_or_else(|| panic!("a named key that is not in NAMED: {named}")),
        KeyName::Character(typed) => Key::Character(SmolStr::new(typed)),
        KeyName::Dead(accent) => Key::Dead(*accent),
        KeyName::Unidentified => Key::Unidentified(NativeKey::Unidentified),
    }
}

/// What a replay gave.
#[derive(Debug, Default, PartialEq, Eq)]
struct Replayed {
    /// The commits to type, in their order, with their reader.
    commits: Vec<(CommitTo, String)>,
    /// The presses of `Process`, and those the gate dropped.
    process: usize,
    dropped: usize,
    /// The composing texts that were not empty, and those that showed
    /// at once with the cursor of the log.
    composing: usize,
    showed: usize,
    /// The frames asked.
    frames: usize,
    /// Whether a composing text was held after the last record.
    left: bool,
}

/// Gives the records of a log to the composition, the gate and the
/// route of a commit, in the wiring the design gives the window: an
/// event of the input method to the composition, a press to the gate,
/// the composition cleared at a loss of focus, and settled after each
/// record at the place the next one finds. A log has no line for a
/// change of file.
fn replay(name: &str, setting: bool) -> Replayed {
    let records = read(name);
    let places = places(&records);
    let at = |index: usize| {
        let place = places[index.min(places.len() - 1)];
        At::new(setting, place.0, place.1, true, true)
    };
    let mut composition = Composition::default();
    let mut out = Replayed::default();
    for (index, record) in records.iter().enumerate() {
        let here = at(index);
        let outcome = match record {
            Record::Key(key) => {
                let logical = key_of(&key.logical);
                if key.pressed && logical == Key::Named(NamedKey::Process) {
                    out.process += 1;
                    out.dropped += usize::from(method::drops_press(setting, &logical));
                } else if key.pressed {
                    assert!(!method::drops_press(setting, &logical), "{name}: {key:?}");
                }
                Outcome::default()
            }
            Record::Enabled => composition.event(here, Event::Enabled),
            Record::Disabled => composition.event(here, Event::Disabled),
            Record::Preedit { text, cursor } => {
                let outcome = composition.event(
                    here,
                    Event::Composing {
                        text,
                        cursor: *cursor,
                    },
                );
                if !text.is_empty() {
                    out.composing += 1;
                    let shown = Some(Shown {
                        text,
                        cursor: *cursor,
                    });
                    out.showed += usize::from(composition.shown(here) == shown);
                }
                outcome
            }
            Record::Commit(text) => composition.event(here, Event::Commit(text)),
            Record::Focus(false) => Outcome {
                redraw: composition.clear(),
                commit: None,
                composed: false,
            },
            Record::Focus(true) | Record::Mods(_) | Record::Allowed(_) => Outcome::default(),
        };
        if let Some((to, text)) = outcome.commit {
            out.commits.push((to, text.to_string()));
        }
        let settled = composition.settle(at(index + 1));
        out.frames += usize::from(outcome.redraw || settled);
    }
    out.left = !composition.is_empty();
    out
}

#[test]
fn each_log_of_the_folder_has_its_replay() {
    let folder = format!("{}/tests/fixtures/input-logs", env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    let mut listed: Vec<String> = LOGS.iter().map(|log| format!("{}.log", log.name)).collect();
    listed.sort();
    assert_eq!(found, listed);
    assert_eq!(LOGS.len(), 30);
}

/// With the setting on: the commits of each log in their order, each
/// given to the reader that has the keys, none dropped; each composing
/// text shows where it came, with the cursor of the log; no press of
/// `Process` acts; and no composing text is left at the end.
#[test]
fn the_logs_replay_with_the_setting_on() {
    for log in LOGS {
        let name = log.name;
        let replayed = replay(name, true);
        let commits: Vec<(CommitTo, &str)> = replayed
            .commits
            .iter()
            .map(|(to, text)| (*to, text.as_str()))
            .collect();
        assert_eq!(commits, log.commits, "{name}: the commits");
        assert_eq!(
            replayed.process, log.process,
            "{name}: the presses of Process"
        );
        assert_eq!(
            replayed.dropped, replayed.process,
            "{name}: a Process acted"
        );
        assert_eq!(
            replayed.showed, replayed.composing,
            "{name}: a composing text"
        );
        assert!(!replayed.left, "{name}: a composing text is left");
        let commits_in_the_log = read(name)
            .iter()
            .filter(|record| matches!(record, Record::Commit(text) if !text.is_empty()))
            .count();
        assert_eq!(
            replayed.commits.len(),
            commits_in_the_log,
            "{name}: a commit was dropped"
        );
    }
}

/// With the setting off only the presses act: no commit is typed, no
/// composing text is held, no frame is asked, and `Process` passes.
#[test]
fn the_logs_replay_with_the_setting_off() {
    for log in LOGS {
        let name = log.name;
        let replayed = replay(name, false);
        assert_eq!(
            replayed,
            Replayed {
                process: log.process,
                composing: replayed.composing,
                ..Replayed::default()
            },
            "{name}"
        );
    }
}

/// The logs hold each platform's part: the presses of `Process` are
/// on Windows alone, and the composing texts come on each system but
/// X11 with fcitx5, which draws its own.
#[test]
fn the_logs_hold_what_each_system_sends() {
    for log in LOGS {
        let name = log.name;
        let replayed = replay(name, true);
        let windows_method = ["win-pinyin", "win-weasel", "win-ms"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        assert_eq!(log.process > 0, windows_method, "{name}");
        if name.starts_with("x11-") {
            assert_eq!(replayed.composing, 0, "{name}: fcitx5 draws its own text");
        }
        if [
            "wl-B",
            "wl-C2",
            "xvfb-ibus-B",
            "win-pinyin-B",
            "win-ms-J",
            "win-ms-K",
        ]
        .contains(&name)
        {
            assert!(replayed.composing > 0, "{name}");
            assert!(replayed.frames > 0, "{name}");
        }
    }
}

/// Where the place of an event is read from the key records, on the
/// three cases the rule names.
#[test]
fn the_place_of_an_event_is_read_from_the_key_records_around_it() {
    let key = |pressed: bool, owner: KeyOwner, editing: bool| {
        Record::Key(log::KeyRecord {
            pressed,
            logical: KeyName::Character("f".to_string()),
            keycap: KeyName::Character("f".to_string()),
            code: "KeyF".to_string(),
            text: None,
            repeat: false,
            synthetic: false,
            mods: log::Mods::default(),
            owner,
            editing,
        })
    };
    let commit = || Record::Commit("x".to_string());
    let editor = (KeyOwner::Text, true);
    let bar = (KeyOwner::Search, true);
    let question = (KeyOwner::Question, true);
    // The same place around the event.
    let records = [
        key(false, editor.0, true),
        commit(),
        key(true, editor.0, true),
    ];
    assert_eq!(places(&records)[1], editor);
    // A press before: `Ctrl+F` opened the bar.
    let records = [key(true, editor.0, true), commit(), key(false, bar.0, true)];
    assert_eq!(places(&records)[1], bar);
    // A release before and a click after the commit, dated by the
    // window's own line: the commit was the editor's.
    let records = [
        key(false, editor.0, true),
        commit(),
        Record::Allowed(false),
        commit(),
        key(true, question.0, true),
    ];
    assert_eq!(places(&records)[1], editor);
    assert_eq!(places(&records)[3], question);
    // No key record after, and none at all.
    let records = [key(false, bar.0, true), commit()];
    assert_eq!(places(&records)[1], bar);
    assert_eq!(places(&[commit()]), [(KeyOwner::Text, false)]);
}

// The fuzzer.

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

/// What a composing text or a commit holds: nothing, letters, a
/// character of three bytes, a joined emoji, a space, a digit, and a
/// control character.
const TEXTS: [&str; 10] = [
    "",
    "n",
    "ni h",
    "\u{4f60}",
    "\u{4f60}\u{597d}",
    "\u{ea}",
    " ",
    "\u{1f468}\u{200d}\u{1f469}",
    "12",
    "a\u{1b}",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pressed {
    Letter(char),
    Process,
    Enter,
}

/// An event of the window.
#[derive(Debug, Clone)]
enum Step {
    Press(Pressed),
    Release,
    Composing(&'static str, Option<(usize, usize)>),
    Commit(&'static str),
    Enabled,
    Disabled,
    /// A key or a click changed what has the keys.
    Owner(KeyOwner),
    Mode(bool),
    Caret(bool),
    /// The dialog's field takes a name, or a hex color.
    DialogText(bool),
    Setting(bool),
    Focus(bool),
    /// Another file opened.
    File,
}

fn step() -> impl Strategy<Value = Step> {
    let text = || prop::sample::select(&TEXTS[..]);
    let cursor = prop::option::of((0usize..14, 0usize..14));
    prop_oneof![
        4 => (text(), cursor).prop_map(|(text, cursor)| Step::Composing(text, cursor)),
        3 => text().prop_map(Step::Commit),
        2 => prop_oneof![
            Just(Pressed::Letter('d')),
            Just(Pressed::Letter('7')),
            Just(Pressed::Process),
            Just(Pressed::Enter),
        ]
        .prop_map(Step::Press),
        1 => Just(Step::Release),
        1 => Just(Step::Enabled),
        1 => Just(Step::Disabled),
        2 => prop::sample::select(&OWNERS[..]).prop_map(Step::Owner),
        1 => any::<bool>().prop_map(Step::Mode),
        1 => any::<bool>().prop_map(Step::Caret),
        1 => any::<bool>().prop_map(Step::DialogText),
        1 => any::<bool>().prop_map(Step::Setting),
        1 => any::<bool>().prop_map(Step::Focus),
        1 => Just(Step::File),
    ]
}

/// The state of the window an event of the keyboard reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    setting: bool,
    owner: KeyOwner,
    editing: bool,
    caret: bool,
    dialog_text: bool,
}

impl Window {
    fn at(self) -> At {
        At::new(
            self.setting,
            self.owner,
            self.editing,
            self.caret,
            self.dialog_text,
        )
    }

    /// A step that changes the window's state. True when it changed
    /// what has the keys or the mode.
    fn change(&mut self, step: &Step) -> bool {
        let place = (self.owner, self.editing);
        match *step {
            Step::Owner(owner) => self.owner = owner,
            Step::Mode(editing) => self.editing = editing,
            Step::Caret(caret) => self.caret = caret,
            Step::DialogText(name) => self.dialog_text = name,
            Step::Setting(setting) => self.setting = setting,
            _ => {}
        }
        place != (self.owner, self.editing)
    }
}

/// What a session leaves: a plain document and a plain field, the
/// steps of the undo history, the shortcuts and the answers that ran,
/// and what acted, in its order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct World {
    document: String,
    field: String,
    undo: usize,
    commands: usize,
    acted: Vec<String>,
}

impl World {
    /// Text given to a reader as a character key with no modifier.
    /// The guards are the key's: the editor types nothing with no
    /// caret and drops a text that holds a control character whole, a
    /// field strips the control characters, and the go to line field
    /// takes digits and colons.
    fn type_text(&mut self, window: Window, to: CommitTo, text: &str) {
        match to {
            CommitTo::Editor => {
                if window.caret && !text.chars().any(char::is_control) {
                    self.document.push_str(text);
                    self.undo += 1;
                }
            }
            CommitTo::Field(Field::GoTo) => {
                if text.chars().all(|c| c.is_ascii_digit() || c == ':') {
                    self.field.push_str(text);
                }
            }
            CommitTo::Field(Field::Search | Field::Path) | CommitTo::Dialog => {
                self.field.extend(text.chars().filter(|c| !c.is_control()));
            }
        }
    }

    /// A press that reaches Oryx, as today: a letter answers a
    /// question, is a shortcut in reading mode and in a dialog with no
    /// field, and is typed where a caret is.
    fn press(&mut self, window: Window, pressed: Pressed) {
        self.acted.push(format!("{pressed:?}"));
        let Pressed::Letter(letter) = pressed else {
            return;
        };
        let typed = letter.to_string();
        match (window.owner, window.editing) {
            (KeyOwner::Question | KeyOwner::Dialog { field: false }, _) => self.commands += 1,
            (KeyOwner::Text | KeyOwner::Code, false) => self.commands += 1,
            (KeyOwner::Text | KeyOwner::Code, true) => {
                self.type_text(window, CommitTo::Editor, &typed);
            }
            (KeyOwner::Dialog { field: true }, _) => {
                self.type_text(window, CommitTo::Dialog, &typed);
            }
            (KeyOwner::Path, _) => self.type_text(window, CommitTo::Field(Field::Path), &typed),
            (KeyOwner::GoTo, _) => self.type_text(window, CommitTo::Field(Field::GoTo), &typed),
            (KeyOwner::Search, _) => {
                self.type_text(window, CommitTo::Field(Field::Search), &typed);
            }
        }
    }
}

/// The plain model: the table of the events of the design, written
/// with no place kept in the composition. Each change that ends a
/// composition clears it where it happens.
#[derive(Debug, Clone)]
struct Model {
    window: Window,
    composing: Option<(String, Option<(usize, usize)>)>,
    world: World,
}

impl Model {
    fn takes_text(&self) -> bool {
        let window = self.window;
        match window.owner {
            KeyOwner::Text | KeyOwner::Code => window.editing && window.caret,
            KeyOwner::Search | KeyOwner::Path => true,
            KeyOwner::Dialog { field } => field && window.dialog_text,
            KeyOwner::GoTo | KeyOwner::Question => false,
        }
    }

    /// The reader of a commit, the table "Where a commit goes".
    fn reader(&self) -> Option<CommitTo> {
        match (self.window.owner, self.window.editing) {
            (KeyOwner::Text | KeyOwner::Code, true) => Some(CommitTo::Editor),
            (KeyOwner::Search, _) => Some(CommitTo::Field(Field::Search)),
            (KeyOwner::Path, _) => Some(CommitTo::Field(Field::Path)),
            (KeyOwner::GoTo, _) => Some(CommitTo::Field(Field::GoTo)),
            (KeyOwner::Dialog { field: true }, _) => Some(CommitTo::Dialog),
            (KeyOwner::Question | KeyOwner::Dialog { field: false }, _) => None,
            (KeyOwner::Text | KeyOwner::Code, false) => None,
        }
    }

    fn step(&mut self, step: &Step) {
        let on = self.window.setting;
        match *step {
            Step::Press(pressed) => {
                if !(on && pressed == Pressed::Process) {
                    self.world.press(self.window, pressed);
                }
            }
            Step::Release | Step::Enabled | Step::Focus(true) => {}
            Step::Composing(text, cursor) if on && !text.is_empty() => {
                if self.takes_text() {
                    let ends: Vec<usize> = text
                        .char_indices()
                        .map(|(at, _)| at)
                        .chain([text.len()])
                        .collect();
                    let cursor = cursor
                        .filter(|(from, to)| ends.contains(from) && ends.contains(to))
                        .map(|(from, to)| if from <= to { (from, to) } else { (to, from) });
                    self.composing = Some((text.to_string(), cursor));
                }
            }
            Step::Composing(..) | Step::Disabled => {
                if on {
                    self.composing = None;
                }
            }
            Step::Commit(text) => {
                if on {
                    self.composing = None;
                    if let (Some(to), false) = (self.reader(), text.is_empty()) {
                        self.world.acted.push(format!("commit {text:?}"));
                        self.world.type_text(self.window, to, text);
                    }
                }
            }
            Step::Focus(false) | Step::File => self.composing = None,
            Step::Owner(_)
            | Step::Mode(_)
            | Step::Caret(_)
            | Step::DialogText(_)
            | Step::Setting(_) => {
                let moved = self.window.change(step);
                if moved || !(self.window.setting && self.takes_text()) {
                    self.composing = None;
                }
            }
        }
    }
}

/// The code under test, in the wiring the design gives the window: an
/// event of the input method to the composition, a press to the gate,
/// the composition cleared at a loss of focus and at a change of file,
/// and settled after each event. The unit tests of the module have
/// the events that find a text held at another place, which this
/// wiring never makes.
struct Bench {
    window: Window,
    composition: Composition,
    world: World,
}

impl Bench {
    fn step(&mut self, step: &Step) -> Result<(), TestCaseError> {
        let at = self.window.at();
        let before = self
            .composition
            .shown(at)
            .map(|shown| (shown.text.to_string(), shown.cursor));
        let world = self.world.clone();
        let event = match *step {
            Step::Composing(text, cursor) => Some(Event::Composing { text, cursor }),
            Step::Commit(text) => Some(Event::Commit(text)),
            Step::Enabled => Some(Event::Enabled),
            Step::Disabled => Some(Event::Disabled),
            _ => None,
        };
        if let Some(event) = event {
            let outcome = self.composition.event(at, event);
            let after = self
                .composition
                .shown(at)
                .map(|shown| (shown.text.to_string(), shown.cursor));
            prop_assert_eq!(
                outcome.redraw,
                before != after,
                "a frame is asked when the box changes"
            );
            if let Some((to, text)) = outcome.commit {
                prop_assert!(matches!(step, Step::Commit(_)), "only a commit types");
                prop_assert!(!text.is_empty(), "an empty commit types nothing");
                self.world.acted.push(format!("commit {text:?}"));
                self.world.type_text(self.window, to, text);
            }
            if !self.window.setting {
                prop_assert_eq!(outcome, Outcome::default(), "the setting off");
            }
            if !matches!(step, Step::Commit(_)) {
                prop_assert_eq!(&self.world, &world, "only a commit enters text");
            }
            prop_assert_eq!(
                self.world.commands,
                world.commands,
                "no shortcut and no answer"
            );
            prop_assert_eq!(self.window.at(), at, "the rule's answer is the window's");
        }
        match *step {
            Step::Press(pressed) => {
                let key = match pressed {
                    Pressed::Letter(letter) => Key::Character(SmolStr::new(letter.to_string())),
                    Pressed::Process => Key::Named(NamedKey::Process),
                    Pressed::Enter => Key::Named(NamedKey::Enter),
                };
                if method::drops_press(self.window.setting, &key) {
                    prop_assert_eq!(pressed, Pressed::Process);
                } else {
                    self.world.press(self.window, pressed);
                }
            }
            Step::Focus(false) | Step::File => {
                self.composition.clear();
            }
            _ => {}
        }
        let moved = self.window.change(step);
        let at = self.window.at();
        self.composition.settle(at);
        if moved || matches!(step, Step::Focus(false) | Step::File) {
            prop_assert!(self.composition.is_empty(), "left after a change");
        }
        prop_assert_eq!(
            self.composition.shown(at).is_some(),
            !self.composition.is_empty(),
            "a text is held that does not show"
        );
        if !at.wanted() {
            prop_assert!(
                self.composition.is_empty(),
                "held where the keys take no text"
            );
        }
        if let Some(shown) = self.composition.shown(at) {
            let ends = shown.cursor.unwrap_or((0, 0));
            prop_assert!(ends.0 <= ends.1, "the ends of the cursor in order");
            prop_assert!(
                shown.text.is_char_boundary(ends.0) && shown.text.is_char_boundary(ends.1)
            );
        }
        Ok(())
    }
}

/// Runs the steps on the code and on the plain model, and compares
/// them after each: the composing text that shows, the document, the
/// field, the undo history, the shortcuts and what acted, in its order.
fn check_session(mut window: Window, steps: &[Step]) -> Result<(), TestCaseError> {
    window.caret = true;
    let mut bench = Bench {
        window,
        composition: Composition::default(),
        world: World::default(),
    };
    let mut model = Model {
        window,
        composing: None,
        world: World::default(),
    };
    for (index, step) in steps.iter().enumerate() {
        bench.step(step)?;
        model.step(step);
        prop_assert_eq!(bench.window, model.window);
        let shown = bench
            .composition
            .shown(bench.window.at())
            .map(|shown| (shown.text.to_string(), shown.cursor));
        prop_assert_eq!(&shown, &model.composing, "step {}: {:?}", index, step);
        prop_assert_eq!(&bench.world, &model.world, "step {}: {:?}", index, step);
    }
    Ok(())
}

fn window(setting: bool, owner: KeyOwner, editing: bool) -> Window {
    Window {
        setting,
        owner,
        editing,
        caret: true,
        dialog_text: true,
    }
}

/// The composition, the gate of the presses and the route of a commit:
/// random runs of the window's events (presses, releases, composing
/// texts, commits, `Enabled`, `Disabled`, changes of what has the
/// keys, of the mode, of the caret, of the focus, of the file and of
/// the setting) against the plain model. The properties:
///
/// 1. with the setting off, a press acts as today and no other event
///    acts;
/// 2. committed text is given to the reader that has the keys, the
///    field of digits included, and dropped at a question, in a dialog
///    with no field and in reading mode; it is no shortcut and no
///    answer;
/// 3. composing text does not enter the document, the undo history or
///    a field's text;
/// 4. no composing text is left after a change of what has the keys,
///    of the file or of the window's focus;
/// 5. an empty commit changes no text, and an empty composing text
///    closes the composition;
/// 6. with the setting on, a press of `Process` changes nothing;
/// 7. `Enabled` and `Disabled` change no text;
/// 8. no composing text is held where the keys take no text.
///
/// Two properties of the design are the window's own and no line of
/// the module can break them: `Enabled` and `Disabled` do not change
/// the rule's answer, and a commit and a press act in the order they
/// came in. The bench has both by the way it is built.
#[test]
#[ignore]
fn fuzz_input_method() {
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(
        setting in prop::bool::weighted(0.8),
        owner in prop::sample::select(&OWNERS[..]),
        editing in any::<bool>(),
        steps in prop::collection::vec(step(), 1..60),
    )| {
        check_session(window(setting, owner, editing), &steps)?;
    });
}

/// One session of the fuzzer's bench, by hand, so that the bench runs
/// under `make check`: scenario B in short, a shortcut during a
/// composition, a question, and the setting turned off.
#[test]
fn a_session_runs_on_the_bench_of_the_fuzzer() {
    let steps = [
        Step::Composing("n", Some((1, 1))),
        Step::Composing("ni h", Some((0, 4))),
        Step::Press(Pressed::Process),
        Step::Composing("", None),
        Step::Commit("\u{4f60}\u{597d}"),
        Step::Disabled,
        Step::Enabled,
        Step::Composing("\u{4f60}", Some((1, 2))),
        Step::Owner(KeyOwner::Search),
        Step::Composing("\u{4f60}", None),
        Step::Commit("\u{4f60}"),
        Step::Owner(KeyOwner::Text),
        Step::Composing("ni h", None),
        Step::Focus(false),
        Step::Focus(true),
        Step::Composing("n", None),
        Step::File,
        Step::Owner(KeyOwner::Question),
        Step::Commit("d"),
        Step::Press(Pressed::Letter('d')),
        Step::Owner(KeyOwner::GoTo),
        Step::Commit("12"),
        Step::Commit("a\u{1b}"),
        Step::Owner(KeyOwner::Text),
        Step::Commit("a\u{1b}"),
        Step::Caret(false),
        Step::Commit("n"),
        Step::Caret(true),
        Step::Composing("n", None),
        Step::Setting(false),
        Step::Composing("n", None),
        Step::Commit("n"),
        Step::Press(Pressed::Process),
        Step::Press(Pressed::Letter('d')),
    ];
    check_session(window(true, KeyOwner::Text, true), &steps).unwrap();
    for owner in OWNERS {
        for editing in [true, false] {
            for setting in [true, false] {
                check_session(window(setting, owner, editing), &steps).unwrap();
            }
        }
    }
}
