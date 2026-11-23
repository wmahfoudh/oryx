//! The input log: a line per key event and per event of the system's
//! input method, as winit hands them to the window. `ORYX_INPUT_LOG`
//! names the file, in a build made with the `input-log` feature. The
//! log exists to see what each system and each input method sends, and
//! its lines are read back by the tests that replay them.
//!
//! A line is the milliseconds since the log opened, the kind of the
//! record, then `name=value` fields, separated by tabs. A value holds
//! no tab and no line break: those, the backslash and the other control
//! characters are escaped. A line that opens with `#` is a comment.
//!
//! The log holds what was typed. Nothing is written without the
//! variable.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use winit::keyboard::{Key, PhysicalKey};

use crate::input::keymap::KeyOwner;

/// A key as a line names it: winit's `Key`, with a named key by the
/// name of its variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyName {
    Character(String),
    Named(String),
    Dead(Option<char>),
    Unidentified,
}

/// The modifiers held at a record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
}

/// A key press or release, with what had the keys when it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub pressed: bool,
    pub logical: KeyName,
    /// The key with no modifier, `key_without_modifiers`.
    pub keycap: KeyName,
    /// The physical key, by the name of its `KeyCode`.
    pub code: String,
    /// The text winit gives with the press. None is not an empty text.
    pub text: Option<String>,
    pub repeat: bool,
    pub synthetic: bool,
    pub mods: Mods,
    pub owner: KeyOwner,
    pub editing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Key(KeyRecord),
    /// `Ime::Enabled` and `Ime::Disabled`, the system's own.
    Enabled,
    Disabled,
    /// `Ime::Preedit`: the composing text and the input method's caret
    /// in it, in bytes.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// `Ime::Commit`.
    Commit(String),
    Focus(bool),
    Mods(Mods),
    /// Oryx called `set_ime_allowed` with this value.
    Allowed(bool),
}

/// The name of a key of winit.
pub fn key_name(key: &Key) -> KeyName {
    match key {
        Key::Character(typed) => KeyName::Character(typed.to_string()),
        Key::Named(named) => KeyName::Named(format!("{named:?}")),
        Key::Dead(accent) => KeyName::Dead(*accent),
        Key::Unidentified(_) => KeyName::Unidentified,
    }
}

/// The name of a physical key.
pub fn code_name(code: PhysicalKey) -> String {
    match code {
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(_) => "Unidentified".to_string(),
    }
}

/// Writes a value so that it holds no tab and no line break. A space
/// at its end is escaped too: a mail or an editor may strip it.
fn escape(value: &str, out: &mut String) {
    let last = value.len().saturating_sub(1);
    for (at, c) in value.char_indices() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ' ' if at == last => out.push_str("\\u{20}"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
}

fn unescape(value: &str) -> Result<String, String> {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('u') => {
                if chars.next() != Some('{') {
                    return Err(format!("a code with no brace in {value:?}"));
                }
                let mut hex = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(digit) if digit.is_ascii_hexdigit() && hex.len() < 6 => {
                            hex.push(digit);
                        }
                        _ => return Err(format!("a code that does not end in {value:?}")),
                    }
                }
                let code = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| format!("no such character in {value:?}"))?;
                out.push(code);
            }
            _ => return Err(format!("a backslash that escapes nothing in {value:?}")),
        }
    }
    Ok(out)
}

fn name_value(name: &KeyName) -> String {
    match name {
        KeyName::Character(typed) => format!("char:{typed}"),
        KeyName::Named(named) => format!("named:{named}"),
        KeyName::Dead(Some(accent)) => format!("dead:{accent}"),
        KeyName::Dead(None) => "dead:".to_string(),
        KeyName::Unidentified => "unidentified".to_string(),
    }
}

fn name_from(value: &str) -> Result<KeyName, String> {
    if value == "unidentified" {
        return Ok(KeyName::Unidentified);
    }
    match value.split_once(':') {
        Some(("char", typed)) => Ok(KeyName::Character(typed.to_string())),
        Some(("named", named))
            if !named.is_empty() && named.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            Ok(KeyName::Named(named.to_string()))
        }
        Some(("dead", accent)) => {
            let mut chars = accent.chars();
            match (chars.next(), chars.next()) {
                (None, _) => Ok(KeyName::Dead(None)),
                (Some(c), None) => Ok(KeyName::Dead(Some(c))),
                _ => Err(format!("a dead key of several characters: {value:?}")),
            }
        }
        _ => Err(format!("no such key: {value:?}")),
    }
}

fn mods_value(mods: Mods) -> String {
    [
        (mods.ctrl, 'c'),
        (mods.shift, 's'),
        (mods.alt, 'a'),
        (mods.logo, 'l'),
    ]
    .iter()
    .map(|&(held, letter)| if held { letter } else { '-' })
    .collect()
}

fn mods_from(value: &str) -> Result<Mods, String> {
    let held: Vec<char> = value.chars().collect();
    let [ctrl, shift, alt, logo] = held[..] else {
        return Err(format!("the modifiers are four characters: {value:?}"));
    };
    let one = |c: char, letter: char| match c {
        c if c == letter => Ok(true),
        '-' => Ok(false),
        _ => Err(format!("no such modifiers: {value:?}")),
    };
    Ok(Mods {
        ctrl: one(ctrl, 'c')?,
        shift: one(shift, 's')?,
        alt: one(alt, 'a')?,
        logo: one(logo, 'l')?,
    })
}

const OWNERS: [(KeyOwner, &str); 8] = [
    (KeyOwner::Question, "question"),
    (KeyOwner::Dialog { field: false }, "dialog"),
    (KeyOwner::Dialog { field: true }, "dialog-field"),
    (KeyOwner::Path, "path"),
    (KeyOwner::GoTo, "goto"),
    (KeyOwner::Search, "search"),
    (KeyOwner::Text, "text"),
    (KeyOwner::Code, "code"),
];

fn owner_value(owner: KeyOwner) -> &'static str {
    OWNERS
        .iter()
        .find(|(o, _)| *o == owner)
        .map_or("text", |(_, name)| name)
}

fn on_off(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

/// The line of a record, with no line break at its end.
pub fn line(ms: u64, record: &Record) -> String {
    let mut out = ms.to_string();
    let kind = match record {
        Record::Key(key) if key.pressed => "press",
        Record::Key(_) => "release",
        Record::Enabled => "ime-enabled",
        Record::Disabled => "ime-disabled",
        Record::Preedit { .. } => "preedit",
        Record::Commit(_) => "commit",
        Record::Focus(_) => "focus",
        Record::Mods(_) => "mods",
        Record::Allowed(_) => "allowed",
    };
    out.push('\t');
    out.push_str(kind);
    let mut field = |name: &str, value: &str| {
        out.push('\t');
        out.push_str(name);
        out.push('=');
        escape(value, &mut out);
    };
    match record {
        Record::Key(key) => {
            field("key", &name_value(&key.logical));
            field("cap", &name_value(&key.keycap));
            field("code", &key.code);
            if let Some(text) = &key.text {
                field("text", text);
            }
            field("repeat", on_off(key.repeat));
            field("synthetic", on_off(key.synthetic));
            field("mods", &mods_value(key.mods));
            field("owner", owner_value(key.owner));
            field("mode", if key.editing { "edit" } else { "read" });
        }
        Record::Enabled | Record::Disabled => {}
        Record::Preedit { text, cursor } => {
            field("text", text);
            if let Some((from, to)) = cursor {
                field("cursor", &format!("{from},{to}"));
            }
        }
        Record::Commit(text) => field("text", text),
        Record::Focus(on) | Record::Allowed(on) => field("on", on_off(*on)),
        Record::Mods(mods) => field("mods", &mods_value(*mods)),
    }
    out
}

/// Reads a line back. A comment or a blank line is no record.
pub fn parse(line: &str) -> Result<Option<(u64, Record)>, String> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut parts = line.split('\t');
    let ms = parts
        .next()
        .and_then(|ms| ms.parse::<u64>().ok())
        .ok_or_else(|| format!("no time in {line:?}"))?;
    let kind = parts.next().ok_or_else(|| format!("no kind in {line:?}"))?;
    let mut fields: Vec<(&str, String)> = Vec::new();
    for part in parts {
        let (name, value) = part
            .split_once('=')
            .ok_or_else(|| format!("a field with no value: {part:?}"))?;
        fields.push((name, unescape(value)?));
    }
    let get = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, value)| value.as_str())
    };
    let need = |name: &str| get(name).ok_or_else(|| format!("no {name} in {line:?}"));
    let flag = |name: &str| match need(name)? {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(format!("{name} is 0 or 1, not {other:?}")),
    };
    let record = match kind {
        "press" | "release" => {
            let owner = need("owner")?;
            Record::Key(KeyRecord {
                pressed: kind == "press",
                logical: name_from(need("key")?)?,
                keycap: name_from(need("cap")?)?,
                code: need("code")?.to_string(),
                text: get("text").map(str::to_string),
                repeat: flag("repeat")?,
                synthetic: flag("synthetic")?,
                mods: mods_from(need("mods")?)?,
                owner: OWNERS
                    .iter()
                    .find(|(_, name)| *name == owner)
                    .map(|(owner, _)| *owner)
                    .ok_or_else(|| format!("no such owner: {owner:?}"))?,
                editing: match need("mode")? {
                    "edit" => true,
                    "read" => false,
                    other => return Err(format!("no such mode: {other:?}")),
                },
            })
        }
        "ime-enabled" => Record::Enabled,
        "ime-disabled" => Record::Disabled,
        "preedit" => Record::Preedit {
            text: need("text")?.to_string(),
            cursor: match get("cursor") {
                None => None,
                Some(cursor) => Some(
                    cursor
                        .split_once(',')
                        .and_then(|(from, to)| Some((from.parse().ok()?, to.parse().ok()?)))
                        .ok_or_else(|| format!("no such cursor: {cursor:?}"))?,
                ),
            },
        },
        "commit" => Record::Commit(need("text")?.to_string()),
        "focus" => Record::Focus(flag("on")?),
        "allowed" => Record::Allowed(flag("on")?),
        "mods" => Record::Mods(mods_from(need("mods")?)?),
        other => return Err(format!("no such record: {other:?}")),
    };
    Ok(Some((ms, record)))
}

/// The open log file.
pub struct InputLog {
    file: File,
    start: Instant,
}

impl InputLog {
    /// Opens the file to append, so a second launch on the same path
    /// keeps the first one's lines, and writes the header as a comment.
    pub fn open(path: &Path, header: &str) -> std::io::Result<InputLog> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        // The log has the keys typed: a new file is its owner's alone.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(format!("# {header}\n").as_bytes())?;
        Ok(InputLog {
            file,
            start: Instant::now(),
        })
    }

    /// Writes a record. A failed write is dropped: the log does not
    /// stop the window.
    pub fn write(&mut self, record: &Record) {
        let ms = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX);
        // The line and its break go in one write: a second window
        // of Oryx appends to the same file.
        let mut text = line(ms, record);
        text.push('\n');
        let _ = self.file.write_all(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode, NamedKey};

    fn key(logical: KeyName) -> KeyRecord {
        KeyRecord {
            pressed: true,
            keycap: logical.clone(),
            logical,
            code: "KeyA".to_string(),
            text: None,
            repeat: false,
            synthetic: false,
            mods: Mods::default(),
            owner: KeyOwner::Text,
            editing: true,
        }
    }

    fn back(record: &Record) -> Record {
        let written = line(7, record);
        assert!(
            !written.contains('\n') && !written.contains('\r'),
            "a record is one line: {written:?}"
        );
        let (ms, read) = parse(&written)
            .unwrap_or_else(|err| panic!("{written:?} is refused: {err}"))
            .unwrap_or_else(|| panic!("{written:?} is read as no record"));
        assert_eq!(ms, 7);
        read
    }

    #[test]
    fn each_kind_of_record_is_read_back_as_written() {
        let owners = [
            KeyOwner::Question,
            KeyOwner::Dialog { field: false },
            KeyOwner::Dialog { field: true },
            KeyOwner::Path,
            KeyOwner::GoTo,
            KeyOwner::Search,
            KeyOwner::Text,
            KeyOwner::Code,
        ];
        let mut records = vec![
            Record::Enabled,
            Record::Disabled,
            Record::Preedit {
                text: "ni".to_string(),
                cursor: Some((2, 2)),
            },
            Record::Preedit {
                text: String::new(),
                cursor: None,
            },
            Record::Commit("你".to_string()),
            Record::Focus(true),
            Record::Focus(false),
            Record::Mods(Mods {
                ctrl: true,
                shift: false,
                alt: true,
                logo: false,
            }),
            Record::Mods(Mods {
                ctrl: false,
                shift: true,
                alt: false,
                logo: true,
            }),
            Record::Allowed(true),
            Record::Allowed(false),
            Record::Key(KeyRecord {
                pressed: false,
                repeat: true,
                synthetic: true,
                editing: false,
                text: Some("a".to_string()),
                ..key(KeyName::Character("a".to_string()))
            }),
            Record::Key(key(KeyName::Named("Enter".to_string()))),
            Record::Key(key(KeyName::Dead(Some('´')))),
            Record::Key(key(KeyName::Dead(None))),
            Record::Key(key(KeyName::Unidentified)),
        ];
        for owner in owners {
            records.push(Record::Key(KeyRecord {
                owner,
                ..key(KeyName::Named("Process".to_string()))
            }));
        }
        for record in &records {
            assert_eq!(&back(record), record);
        }
    }

    #[test]
    fn a_text_with_tabs_line_breaks_and_backslashes_is_read_back() {
        let texts = [
            "",
            " ",
            "a\tb",
            "a\nb\r\nc\rd",
            "a\\tb",
            "\\",
            "name=value",
            "char:x",
            "你好",
            "한글",
            "🦌",
            "e\u{301}",
            "a\u{2028}b\u{2029}c\u{85}d",
            "\u{0}\u{1b}\u{7f}",
            "# no comment",
        ];
        for text in texts {
            let commit = Record::Commit(text.to_string());
            assert_eq!(back(&commit), commit, "{text:?}");
            let preedit = Record::Preedit {
                text: text.to_string(),
                cursor: Some((0, text.len())),
            };
            assert_eq!(back(&preedit), preedit, "{text:?}");
            let typed = Record::Key(KeyRecord {
                text: Some(text.to_string()),
                ..key(KeyName::Character(text.to_string()))
            });
            assert_eq!(back(&typed), typed, "{text:?}");
        }
    }

    #[test]
    fn a_press_with_no_text_is_not_a_press_with_an_empty_text() {
        let none = Record::Key(key(KeyName::Named("Shift".to_string())));
        let empty = Record::Key(KeyRecord {
            text: Some(String::new()),
            ..key(KeyName::Named("Shift".to_string()))
        });
        assert_ne!(line(0, &none), line(0, &empty));
        assert_eq!(back(&none), none);
        assert_eq!(back(&empty), empty);
    }

    #[test]
    fn a_comment_and_a_blank_line_are_no_record() {
        assert_eq!(parse("# oryx 1.4.0 linux"), Ok(None));
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("   "), Ok(None));
    }

    #[test]
    fn a_line_that_is_no_record_is_refused() {
        let good = line(3, &Record::Commit("a".to_string()));
        assert!(parse(&good).is_ok(), "{good:?}");
        for bad in [
            "x\tcommit\ttext=a",
            "3",
            "3\tnothing",
            "3\tcommit",
            "3\tcommit\ttext",
            "3\tcommit\ttext=a\\",
            "3\tcommit\ttext=a\\q",
            "3\tcommit\ttext=a\\u{110000}",
            "3\tpreedit\ttext=a\tcursor=1",
            "3\tfocus\ton=2",
            "3\tmods\tmods=cs",
            "3\tpress\tkey=char:a",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} is read");
        }
    }

    #[test]
    fn the_keys_of_winit_are_named() {
        assert_eq!(
            key_name(&Key::Character("é".into())),
            KeyName::Character("é".to_string())
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Enter)),
            KeyName::Named("Enter".to_string())
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Process)),
            KeyName::Named("Process".to_string())
        );
        assert_eq!(key_name(&Key::Dead(Some('´'))), KeyName::Dead(Some('´')));
        assert_eq!(key_name(&Key::Dead(None)), KeyName::Dead(None));
        assert_eq!(code_name(PhysicalKey::Code(KeyCode::KeyC)), "KeyC");
        assert_eq!(code_name(PhysicalKey::Code(KeyCode::Enter)), "Enter");
    }

    #[test]
    fn the_file_takes_a_header_and_a_line_per_record() {
        let path = std::env::temp_dir().join(format!("oryx-input-log-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let records = [Record::Allowed(true), Record::Commit("好".to_string())];
        for record in &records {
            let mut log = InputLog::open(&path, "oryx test").unwrap();
            log.write(record);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text:?}");
        assert_eq!(lines[0], "# oryx test");
        assert_eq!(lines[2], "# oryx test");
        let read: Vec<Record> = lines
            .iter()
            .filter_map(|l| parse(l).unwrap())
            .map(|(_, record)| record)
            .collect();
        assert_eq!(read, records);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_made_for_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("oryx-input-log-mode-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        drop(InputLog::open(&path, "oryx test").unwrap());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        std::fs::remove_file(&path).unwrap();
        assert_eq!(mode, 0o600, "{mode:o}");
    }

    /// The log's lines against the records they came from: any record,
    /// with any text, is one line and reads back as written.
    #[test]
    #[ignore]
    fn fuzz_input_log() {
        use proptest::prelude::*;
        fn name() -> impl Strategy<Value = KeyName> {
            prop_oneof![
                any::<String>().prop_map(KeyName::Character),
                "[A-Za-z0-9]{1,12}".prop_map(KeyName::Named),
                any::<Option<char>>().prop_map(KeyName::Dead),
                Just(KeyName::Unidentified),
            ]
        }
        fn mods() -> impl Strategy<Value = Mods> {
            any::<(bool, bool, bool, bool)>().prop_map(|(ctrl, shift, alt, logo)| Mods {
                ctrl,
                shift,
                alt,
                logo,
            })
        }
        fn owner() -> impl Strategy<Value = KeyOwner> {
            prop_oneof![
                Just(KeyOwner::Question),
                any::<bool>().prop_map(|field| KeyOwner::Dialog { field }),
                Just(KeyOwner::Path),
                Just(KeyOwner::GoTo),
                Just(KeyOwner::Search),
                Just(KeyOwner::Text),
                Just(KeyOwner::Code),
            ]
        }
        fn record() -> impl Strategy<Value = Record> {
            let key = (
                any::<(bool, bool, bool, bool)>(),
                name(),
                name(),
                "[A-Za-z0-9]{1,12}",
                any::<Option<String>>(),
                mods(),
                owner(),
            )
                .prop_map(|(flags, logical, keycap, code, text, mods, owner)| {
                    Record::Key(KeyRecord {
                        pressed: flags.0,
                        repeat: flags.1,
                        synthetic: flags.2,
                        editing: flags.3,
                        logical,
                        keycap,
                        code,
                        text,
                        mods,
                        owner,
                    })
                });
            prop_oneof![
                key,
                Just(Record::Enabled),
                Just(Record::Disabled),
                (any::<String>(), any::<Option<(usize, usize)>>())
                    .prop_map(|(text, cursor)| Record::Preedit { text, cursor }),
                any::<String>().prop_map(Record::Commit),
                any::<bool>().prop_map(Record::Focus),
                mods().prop_map(Record::Mods),
                any::<bool>().prop_map(Record::Allowed),
            ]
        }
        proptest!(|(ms in any::<u64>(), record in record())| {
            let written = line(ms, &record);
            prop_assert!(!written.contains('\n') && !written.contains('\r'), "{:?}", written);
            prop_assert_eq!(parse(&written), Ok(Some((ms, record))));
        });
    }
}
