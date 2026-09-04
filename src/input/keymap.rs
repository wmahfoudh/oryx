//! Shortcut map: the single source for key dispatch and the help table.
//! Each table row carries the labels the help overlay renders and the
//! chords the app matches, so the two can never drift apart.

use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

/// Application command a shortcut resolves to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    OpenFile,
    Reload,
    Refetch,
    Sidebar,
    Export,
    ExportSettings,
    Help,
    Settings,
    ThemeBrowser,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Justify,
    Direction,
    SelectAll,
    CopyText,
    CopyMarkdown,
    Find,
    FindNext,
    FindPrev,
    Replace,
    LineUp,
    LineDown,
    PaneLeft,
    PaneRight,
    SidebarTab,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Back,
    Edit,
    Cut,
    Paste,
    Undo,
    Redo,
    Save,
    SaveAs,
    NewFile,
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    /// The heading level of the line, 1 to 6.
    Heading(u8),
    TaskToggle,
    Bold,
    Italic,
    Code,
    Link,
    MoveLineUp,
    MoveLineDown,
    Quit,
}

impl Command {
    /// Every variant; the coverage test checks each one against the table.
    pub const ALL: [Command; 39] = [
        Command::OpenFile,
        Command::Reload,
        Command::Refetch,
        Command::Sidebar,
        Command::Export,
        Command::ExportSettings,
        Command::Help,
        Command::Settings,
        Command::ThemeBrowser,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::Justify,
        Command::SelectAll,
        Command::CopyText,
        Command::CopyMarkdown,
        Command::Find,
        Command::FindNext,
        Command::FindPrev,
        Command::Replace,
        Command::LineUp,
        Command::LineDown,
        Command::PaneLeft,
        Command::PaneRight,
        Command::SidebarTab,
        Command::PageUp,
        Command::PageDown,
        Command::Top,
        Command::Bottom,
        Command::Back,
        Command::Edit,
        Command::Cut,
        Command::Paste,
        Command::Undo,
        Command::Redo,
        Command::Save,
        Command::SaveAs,
        Command::NewFile,
        Command::Quit,
    ];
}

/// One matchable key chord.
#[derive(PartialEq, Debug)]
enum Binding {
    /// Ctrl plus a character, case-insensitive, any shift state.
    Ctrl(&'static str),
    /// Ctrl and Shift plus a character, case-insensitive.
    CtrlShift(&'static str),
    /// A named key, any modifier state.
    Named(NamedKey),
    /// A named key with Shift held.
    ShiftNamed(NamedKey),
    /// A named key with Ctrl held; tried before the plain named form.
    CtrlNamed(NamedKey),
    /// A named key with Alt held; tried before the plain named form.
    AltNamed(NamedKey),
    /// Alt plus a character, case-insensitive, any shift state.
    Alt(&'static str),
    /// Alt plus a key by its place on the keyboard, tried last: the
    /// digit chords, on layouts where a digit needs Shift.
    AltCode(KeyCode),
    /// Ctrl plus a key by its place on the keyboard, tried last, once
    /// no character chord claims the press. The zoom keys live here so
    /// that a layout printing something else on the `0`, `-` or `=`
    /// keys still zooms with them, the way browsers do.
    CtrlCode(KeyCode),
}

/// One help-table row: display labels plus the chords the row covers.
/// Rows sharing a section sit together; the help overlay draws a caption
/// where the section changes.
pub struct Shortcut {
    pub keys: &'static str,
    pub action: &'static str,
    pub section: &'static str,
    bindings: &'static [(Binding, Command)],
}

pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        keys: "Ctrl+O",
        action: "Open a file",
        section: "Files",
        bindings: &[(Binding::Ctrl("o"), Command::OpenFile)],
    },
    Shortcut {
        keys: "Ctrl+N",
        action: "New file",
        section: "Files",
        bindings: &[(Binding::Ctrl("n"), Command::NewFile)],
    },
    Shortcut {
        keys: "Ctrl+S",
        action: "Save (editing)",
        section: "Files",
        bindings: &[(Binding::Ctrl("s"), Command::Save)],
    },
    Shortcut {
        keys: "Ctrl+Shift+S",
        action: "Save as (editing)",
        section: "Files",
        bindings: &[(Binding::CtrlShift("s"), Command::SaveAs)],
    },
    Shortcut {
        keys: "F5 / Ctrl+R",
        action: "Reload from disk",
        section: "Files",
        bindings: &[
            (Binding::Named(NamedKey::F5), Command::Reload),
            (Binding::Ctrl("r"), Command::Reload),
        ],
    },
    Shortcut {
        keys: "Ctrl+Shift+R",
        action: "Reload and refetch remote images",
        section: "Files",
        bindings: &[(Binding::CtrlShift("r"), Command::Refetch)],
    },
    Shortcut {
        keys: "Up / Down",
        action: "Scroll by line, or move the sidebar selection",
        section: "Navigation",
        bindings: &[
            (Binding::Named(NamedKey::ArrowUp), Command::LineUp),
            (Binding::Named(NamedKey::ArrowDown), Command::LineDown),
        ],
    },
    Shortcut {
        keys: "Page Up / Page Down, Space / Shift+Space",
        action: "Scroll by page",
        section: "Navigation",
        bindings: &[
            (Binding::Named(NamedKey::PageUp), Command::PageUp),
            (Binding::Named(NamedKey::PageDown), Command::PageDown),
            (Binding::ShiftNamed(NamedKey::Space), Command::PageUp),
            (Binding::Named(NamedKey::Space), Command::PageDown),
        ],
    },
    Shortcut {
        keys: "Home / End",
        action: "Jump to top / bottom",
        section: "Navigation",
        bindings: &[
            (Binding::Named(NamedKey::Home), Command::Top),
            (Binding::Named(NamedKey::End), Command::Bottom),
        ],
    },
    Shortcut {
        keys: "Alt+Left",
        action: "Go back after a link or outline jump",
        section: "Navigation",
        bindings: &[(Binding::AltNamed(NamedKey::ArrowLeft), Command::Back)],
    },
    Shortcut {
        keys: "Ctrl+Shift+B",
        action: "Toggle sidebar (files and outline)",
        section: "Navigation",
        bindings: &[(Binding::CtrlShift("b"), Command::Sidebar)],
    },
    Shortcut {
        keys: "Left / Right",
        action: "Move to the sidebar / to the document",
        section: "Navigation",
        bindings: &[
            (Binding::Named(NamedKey::ArrowLeft), Command::PaneLeft),
            (Binding::Named(NamedKey::ArrowRight), Command::PaneRight),
        ],
    },
    Shortcut {
        keys: "Ctrl+Tab",
        action: "Toggle the sidebar tab",
        section: "Navigation",
        bindings: &[(Binding::CtrlNamed(NamedKey::Tab), Command::SidebarTab)],
    },
    Shortcut {
        keys: "Ctrl+F",
        action: "Find in document",
        section: "Find",
        bindings: &[(Binding::Ctrl("f"), Command::Find)],
    },
    Shortcut {
        keys: "F3 / Shift+F3",
        action: "Next / previous match",
        section: "Find",
        bindings: &[
            (Binding::ShiftNamed(NamedKey::F3), Command::FindPrev),
            (Binding::Named(NamedKey::F3), Command::FindNext),
        ],
    },
    // Handled by the open search bar itself, so no command binding.
    Shortcut {
        keys: "Alt+R",
        action: "Regex matching on/off",
        section: "Find",
        bindings: &[],
    },
    Shortcut {
        keys: "Ctrl+H",
        action: "Find and replace (editing only)",
        section: "Find",
        bindings: &[(Binding::Ctrl("h"), Command::Replace)],
    },
    // Handled by the open search bar itself, so no command binding.
    Shortcut {
        keys: "Ctrl+Enter",
        action: "Replace all (replace open)",
        section: "Find",
        bindings: &[],
    },
    Shortcut {
        keys: "Ctrl+A",
        action: "Select all",
        section: "Selection",
        bindings: &[(Binding::Ctrl("a"), Command::SelectAll)],
    },
    Shortcut {
        keys: "Ctrl+C",
        action: "Copy selection as text",
        section: "Selection",
        bindings: &[(Binding::Ctrl("c"), Command::CopyText)],
    },
    Shortcut {
        keys: "Ctrl+Shift+C",
        action: "Copy selection as markdown",
        section: "Selection",
        bindings: &[(Binding::CtrlShift("c"), Command::CopyMarkdown)],
    },
    Shortcut {
        keys: "Ctrl+E",
        action: "Edit the document",
        section: "Edit",
        bindings: &[(Binding::Ctrl("e"), Command::Edit)],
    },
    Shortcut {
        keys: "Ctrl+X",
        action: "Cut the selection (editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("x"), Command::Cut)],
    },
    Shortcut {
        keys: "Ctrl+V",
        action: "Paste at the caret (editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("v"), Command::Paste)],
    },
    Shortcut {
        keys: "Ctrl+Z",
        action: "Undo the last edit",
        section: "Edit",
        bindings: &[(Binding::Ctrl("z"), Command::Undo)],
    },
    Shortcut {
        keys: "Ctrl+Shift+Z / Ctrl+Y",
        action: "Redo an undone edit",
        section: "Edit",
        bindings: &[
            (Binding::CtrlShift("z"), Command::Redo),
            (Binding::Ctrl("y"), Command::Redo),
        ],
    },
    Shortcut {
        keys: "Alt+-",
        action: "Bullet list on the selected lines, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Alt("-"), Command::BulletList),
            (Binding::AltCode(KeyCode::Minus), Command::BulletList),
        ],
    },
    Shortcut {
        keys: "Alt+1",
        action: "Numbered list on the selected lines, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Alt("1"), Command::NumberedList),
            (Binding::AltCode(KeyCode::Digit1), Command::NumberedList),
        ],
    },
    Shortcut {
        keys: "Alt+X",
        action: "Task list on the selected lines, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[(Binding::Alt("x"), Command::TaskList)],
    },
    // Alt with the period key, not Alt+>: Alt+Shift switches the
    // keyboard layout on Linux and Windows desktops.
    Shortcut {
        keys: "Alt+.",
        action: "Quote the selected lines, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Alt("."), Command::Quote),
            (Binding::Alt(">"), Command::Quote),
            (Binding::AltCode(KeyCode::Period), Command::Quote),
        ],
    },
    Shortcut {
        keys: "Ctrl+1 to Ctrl+6",
        action: "Heading level of the line, the same level again to clear it (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Ctrl("1"), Command::Heading(1)),
            (Binding::Ctrl("2"), Command::Heading(2)),
            (Binding::Ctrl("3"), Command::Heading(3)),
            (Binding::Ctrl("4"), Command::Heading(4)),
            (Binding::Ctrl("5"), Command::Heading(5)),
            (Binding::Ctrl("6"), Command::Heading(6)),
            (Binding::CtrlCode(KeyCode::Digit1), Command::Heading(1)),
            (Binding::CtrlCode(KeyCode::Digit2), Command::Heading(2)),
            (Binding::CtrlCode(KeyCode::Digit3), Command::Heading(3)),
            (Binding::CtrlCode(KeyCode::Digit4), Command::Heading(4)),
            (Binding::CtrlCode(KeyCode::Digit5), Command::Heading(5)),
            (Binding::CtrlCode(KeyCode::Digit6), Command::Heading(6)),
        ],
    },
    Shortcut {
        keys: "Ctrl+L",
        action: "Tick or untick the task box of the line (markdown editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("l"), Command::TaskToggle)],
    },
    Shortcut {
        keys: "Ctrl+B / Ctrl+I",
        action:
            "Bold / italic around the selection or the word, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Ctrl("b"), Command::Bold),
            (Binding::Ctrl("i"), Command::Italic),
        ],
    },
    Shortcut {
        keys: "Ctrl+`",
        action: "Inline code around the selection or the word, again to remove (markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Ctrl("`"), Command::Code),
            (Binding::CtrlCode(KeyCode::Backquote), Command::Code),
        ],
    },
    Shortcut {
        keys: "Ctrl+K",
        action: "Link around the selection, or an empty link; pasting an address over a selection links it too (markdown editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("k"), Command::Link)],
    },
    Shortcut {
        keys: "Alt+Up / Alt+Down",
        action: "Move the line or the selected lines up / down (editing)",
        section: "Edit",
        bindings: &[
            (Binding::AltNamed(NamedKey::ArrowUp), Command::MoveLineUp),
            (Binding::AltNamed(NamedKey::ArrowDown), Command::MoveLineDown),
        ],
    },
    Shortcut {
        keys: "Ctrl+T",
        action: "Choose a theme",
        section: "View",
        bindings: &[(Binding::Ctrl("t"), Command::ThemeBrowser)],
    },
    Shortcut {
        keys: "Ctrl+,",
        action: "Settings: fonts, sizes and interface scale",
        section: "View",
        bindings: &[(Binding::Ctrl(","), Command::Settings)],
    },
    Shortcut {
        keys: "Ctrl+Plus / Ctrl+Minus",
        action: "Zoom in / out (in a comic: page width, whole page, two pages)",
        section: "View",
        bindings: &[
            (Binding::Ctrl("+"), Command::ZoomIn),
            (Binding::Ctrl("="), Command::ZoomIn),
            (Binding::Ctrl("-"), Command::ZoomOut),
            (Binding::CtrlCode(KeyCode::Equal), Command::ZoomIn),
            (Binding::CtrlCode(KeyCode::NumpadAdd), Command::ZoomIn),
            (Binding::CtrlCode(KeyCode::Minus), Command::ZoomOut),
            (Binding::CtrlCode(KeyCode::NumpadSubtract), Command::ZoomOut),
        ],
    },
    Shortcut {
        keys: "Ctrl+0",
        action: "Reset zoom (in a comic: whole page)",
        section: "View",
        bindings: &[
            (Binding::Ctrl("0"), Command::ZoomReset),
            (Binding::CtrlCode(KeyCode::Digit0), Command::ZoomReset),
            (Binding::CtrlCode(KeyCode::Numpad0), Command::ZoomReset),
        ],
    },
    Shortcut {
        keys: "Ctrl+J",
        action: "Justify prose (markdown and books)",
        section: "View",
        bindings: &[(Binding::Ctrl("j"), Command::Justify)],
    },
    Shortcut {
        keys: "Ctrl+D",
        action: "Reading direction: automatic, right to left, left to right",
        section: "View",
        bindings: &[(Binding::Ctrl("d"), Command::Direction)],
    },
    Shortcut {
        keys: "Ctrl+P",
        action: "Export to PDF",
        section: "Export",
        bindings: &[(Binding::Ctrl("p"), Command::Export)],
    },
    Shortcut {
        keys: "Ctrl+Shift+P",
        action: "Choose export settings, then export",
        section: "Export",
        bindings: &[(Binding::CtrlShift("p"), Command::ExportSettings)],
    },
    Shortcut {
        keys: "F1",
        action: "Open this help page, and close it",
        section: "Help",
        bindings: &[(Binding::Named(NamedKey::F1), Command::Help)],
    },
    Shortcut {
        keys: "Escape",
        action: "Close overlay, clear the selection, leave editing, quit",
        section: "Help",
        bindings: &[(Binding::Named(NamedKey::Escape), Command::Quit)],
    },
];

/// Resolves a key event against the table. Chords that require a
/// modifier are tried first, so Ctrl+Shift+C never falls through to
/// Ctrl+C and Ctrl+Left never falls through to plain Left; the key's
/// place on the keyboard is tried last, once no character chord has
/// claimed the press.
pub fn command(
    key: &Key,
    code: PhysicalKey,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<Command> {
    let bindings = || SHORTCUTS.iter().flat_map(|row| row.bindings.iter());
    let shifted = bindings().find(|(binding, _)| match binding {
        Binding::CtrlShift(c) => ctrl && shift && is_char(key, c),
        Binding::ShiftNamed(n) => shift && is_named(key, n),
        Binding::CtrlNamed(n) => ctrl && is_named(key, n),
        Binding::AltNamed(n) => alt && is_named(key, n),
        Binding::Alt(c) => alt && is_char(key, c),
        _ => false,
    });
    let plain = || {
        bindings().find(|(binding, _)| match binding {
            Binding::Ctrl(c) => ctrl && is_char(key, c),
            Binding::Named(n) => is_named(key, n),
            _ => false,
        })
    };
    let physical = || {
        bindings().find(|(binding, _)| match binding {
            Binding::CtrlCode(c) => ctrl && code == PhysicalKey::Code(*c),
            Binding::AltCode(c) => alt && code == PhysicalKey::Code(*c),
            _ => false,
        })
    };
    shifted
        .or_else(plain)
        .or_else(physical)
        .map(|(_, cmd)| *cmd)
}

fn is_char(key: &Key, c: &str) -> bool {
    matches!(key, Key::Character(s) if s.eq_ignore_ascii_case(c))
}

fn is_named(key: &Key, n: &NamedKey) -> bool {
    matches!(key, Key::Named(k) if k == n)
}

/// Chord label for the running platform: Ctrl renders as Cmd on macOS.
pub fn display(keys: &str) -> String {
    platform_label(keys, cfg!(target_os = "macos"))
}

fn platform_label(keys: &str, macos: bool) -> String {
    if macos {
        keys.replace("Ctrl", "Cmd")
    } else {
        keys.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::NativeKeyCode;

    fn chr(s: &str) -> Key {
        Key::Character(s.into())
    }

    /// The table predates the Alt modifier and the physical key; the
    /// shadow keeps the older assertions readable.
    fn command(key: &Key, ctrl: bool, shift: bool) -> Option<Command> {
        super::command(
            key,
            PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            ctrl,
            shift,
            false,
        )
    }

    /// The zoom chords hold by the key's place on the keyboard once no
    /// character chord claims the press: on AZERTY, Ctrl with the key
    /// that prints `à` resets zoom the way browsers do.
    #[test]
    fn the_zoom_chords_match_by_physical_key_on_other_layouts() {
        let on = |c: &str, code: KeyCode, ctrl: bool| {
            super::command(&chr(c), PhysicalKey::Code(code), ctrl, false, false)
        };
        assert_eq!(on("à", KeyCode::Digit0, true), Some(Command::ZoomReset));
        assert_eq!(on("0", KeyCode::Digit0, true), Some(Command::ZoomReset));
        assert_eq!(on("à", KeyCode::Numpad0, true), Some(Command::ZoomReset));
        assert_eq!(on(")", KeyCode::Minus, true), Some(Command::ZoomOut));
        assert_eq!(
            on("-", KeyCode::NumpadSubtract, true),
            Some(Command::ZoomOut)
        );
        assert_eq!(on("=", KeyCode::Equal, true), Some(Command::ZoomIn));
        assert_eq!(on("+", KeyCode::NumpadAdd, true), Some(Command::ZoomIn));
        assert_eq!(
            on("à", KeyCode::Digit0, false),
            None,
            "no chord without Ctrl"
        );
        assert_eq!(on(")", KeyCode::Minus, false), None);
    }

    /// Every physical row names the same command as the character
    /// chord for its key's US character, so the two matchers never
    /// disagree on one press.
    #[test]
    fn the_physical_rows_agree_with_their_character_chords() {
        let us = |code: &KeyCode| match code {
            KeyCode::Digit0 | KeyCode::Numpad0 => "0",
            KeyCode::Digit1 => "1",
            KeyCode::Digit2 => "2",
            KeyCode::Digit3 => "3",
            KeyCode::Digit4 => "4",
            KeyCode::Digit5 => "5",
            KeyCode::Digit6 => "6",
            KeyCode::Minus | KeyCode::NumpadSubtract => "-",
            KeyCode::Equal => "=",
            KeyCode::NumpadAdd => "+",
            KeyCode::Period => ".",
            KeyCode::Backquote => "`",
            other => panic!("{other:?} has no US character in this table"),
        };
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let mut seen = 0;
        for (binding, cmd) in SHORTCUTS.iter().flat_map(|row| row.bindings.iter()) {
            let (code, ctrl, alt) = match binding {
                Binding::CtrlCode(code) => (code, true, false),
                Binding::AltCode(code) => (code, false, true),
                _ => continue,
            };
            seen += 1;
            let by_char = super::command(&chr(us(code)), none, ctrl, false, alt);
            assert_eq!(
                by_char,
                Some(*cmd),
                "the character chord for {code:?} agrees"
            );
        }
        assert!(seen >= 6, "the zoom keys at least");
    }

    #[test]
    fn the_list_chords_take_alt_with_the_character_or_the_digit_key() {
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let alt = |key: &Key, code: PhysicalKey| super::command(key, code, false, false, true);
        assert_eq!(alt(&chr("-"), none), Some(Command::BulletList));
        assert_eq!(
            alt(&chr(")"), PhysicalKey::Code(KeyCode::Minus)),
            Some(Command::BulletList),
            "the key at the US minus position"
        );
        assert_eq!(alt(&chr("x"), none), Some(Command::TaskList));
        assert_eq!(
            super::command(&chr("X"), none, false, true, true),
            Some(Command::TaskList),
            "shift makes no difference"
        );
        assert_eq!(alt(&chr("1"), none), Some(Command::NumberedList));
        assert_eq!(
            alt(&chr("&"), PhysicalKey::Code(KeyCode::Digit1)),
            Some(Command::NumberedList),
            "AZERTY: the 1 key prints & unshifted"
        );
        assert_eq!(
            command(&chr("-"), false, false),
            None,
            "a plain dash is typing"
        );
        assert_eq!(alt(&chr("."), none), Some(Command::Quote));
        assert_eq!(
            super::command(&chr(">"), none, false, true, true),
            Some(Command::Quote),
            "a layout with > on a plain key"
        );
        assert_eq!(
            alt(&chr(";"), PhysicalKey::Code(KeyCode::Period)),
            Some(Command::Quote),
            "AZERTY: the period key prints ; unshifted"
        );
    }

    #[test]
    fn the_heading_chords_take_ctrl_with_a_digit_or_its_key() {
        assert_eq!(command(&chr("1"), true, false), Some(Command::Heading(1)));
        assert_eq!(command(&chr("6"), true, false), Some(Command::Heading(6)));
        assert_eq!(command(&chr("7"), true, false), None);
        assert_eq!(
            super::command(
                &chr("é"),
                PhysicalKey::Code(KeyCode::Digit2),
                true,
                false,
                false
            ),
            Some(Command::Heading(2)),
            "AZERTY: the 2 key prints é unshifted"
        );
        assert_eq!(
            command(&chr("1"), false, false),
            None,
            "a plain digit is typing"
        );
    }

    #[test]
    fn alt_up_and_down_move_lines_and_plain_arrows_still_scroll() {
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let up = Key::Named(NamedKey::ArrowUp);
        let down = Key::Named(NamedKey::ArrowDown);
        assert_eq!(
            super::command(&up, none, false, false, true),
            Some(Command::MoveLineUp)
        );
        assert_eq!(
            super::command(&down, none, false, false, true),
            Some(Command::MoveLineDown)
        );
        assert_eq!(
            super::command(&up, none, false, false, false),
            Some(Command::LineUp)
        );
    }

    #[test]
    fn ctrl_k_makes_a_link() {
        assert_eq!(command(&chr("k"), true, false), Some(Command::Link));
    }

    #[test]
    fn ctrl_l_toggles_the_task_box() {
        assert_eq!(command(&chr("l"), true, false), Some(Command::TaskToggle));
        assert_eq!(command(&chr("l"), false, false), None);
    }

    #[test]
    fn alt_left_goes_back_and_plain_left_still_switches_panes() {
        let left = Key::Named(NamedKey::ArrowLeft);
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        assert_eq!(
            super::command(&left, none, false, false, true),
            Some(Command::Back),
            "Alt+Left returns from a jump"
        );
        assert_eq!(
            super::command(&left, none, false, false, false),
            Some(Command::PaneLeft),
            "plain Left keeps the pane switch"
        );
    }

    #[test]
    fn sections_group_contiguously() {
        let mut seen: Vec<&str> = Vec::new();
        for row in SHORTCUTS {
            if seen.last() != Some(&row.section) {
                assert!(
                    !seen.contains(&row.section),
                    "section {} appears in two places",
                    row.section
                );
                seen.push(row.section);
            }
        }
        assert!(seen.len() > 1, "the table carries named sections");
    }

    #[test]
    fn every_command_is_bound() {
        for cmd in Command::ALL {
            assert!(
                SHORTCUTS
                    .iter()
                    .any(|row| row.bindings.iter().any(|(_, c)| *c == cmd)),
                "{cmd:?} has no row in SHORTCUTS"
            );
        }
    }

    #[test]
    fn key_labels_are_unique() {
        for (i, row) in SHORTCUTS.iter().enumerate() {
            assert!(
                SHORTCUTS[i + 1..]
                    .iter()
                    .all(|other| other.keys != row.keys),
                "duplicate key label {}",
                row.keys
            );
        }
    }

    #[test]
    fn bindings_are_unique() {
        let all: Vec<&(Binding, Command)> = SHORTCUTS
            .iter()
            .flat_map(|row| row.bindings.iter())
            .collect();
        for (i, (binding, _)) in all.iter().enumerate() {
            assert!(
                all[i + 1..].iter().all(|(other, _)| other != binding),
                "duplicate binding {binding:?}"
            );
        }
    }

    #[test]
    fn ctrl_chords_resolve() {
        assert_eq!(command(&chr("o"), true, false), Some(Command::OpenFile));
        assert_eq!(command(&chr("t"), true, false), Some(Command::ThemeBrowser));
        assert_eq!(command(&chr("T"), true, true), Some(Command::ThemeBrowser));
        assert_eq!(command(&chr(","), true, false), Some(Command::Settings));
        assert_eq!(command(&chr("a"), true, false), Some(Command::SelectAll));
        assert_eq!(command(&chr("0"), true, false), Some(Command::ZoomReset));
    }

    #[test]
    fn the_print_pair_exports() {
        assert_eq!(command(&chr("p"), true, false), Some(Command::Export));
        assert_eq!(
            command(&chr("P"), true, true),
            Some(Command::ExportSettings)
        );
    }

    #[test]
    fn ctrl_e_toggles_the_editor() {
        assert_eq!(command(&chr("e"), true, false), Some(Command::Edit));
        // Ctrl+Shift+E falls through to the same command, like every
        // shifted Ctrl chord without a CtrlShift binding of its own.
        assert_eq!(command(&chr("E"), true, true), Some(Command::Edit));
    }

    #[test]
    fn ctrl_d_cycles_the_reading_direction() {
        assert_eq!(command(&chr("d"), true, false), Some(Command::Direction));
    }

    #[test]
    fn cut_and_paste_resolve() {
        assert_eq!(command(&chr("x"), true, false), Some(Command::Cut));
        assert_eq!(command(&chr("v"), true, false), Some(Command::Paste));
    }

    #[test]
    fn the_save_family_resolves() {
        assert_eq!(command(&chr("s"), true, false), Some(Command::Save));
        assert_eq!(command(&chr("S"), true, true), Some(Command::SaveAs));
        assert_eq!(command(&chr("n"), true, false), Some(Command::NewFile));
    }

    #[test]
    fn undo_resolves_and_redo_answers_both_chords() {
        assert_eq!(command(&chr("z"), true, false), Some(Command::Undo));
        assert_eq!(command(&chr("Z"), true, true), Some(Command::Redo));
        assert_eq!(
            command(&chr("y"), true, false),
            Some(Command::Redo),
            "Ctrl+Y is the everyday redo alias"
        );
    }

    #[test]
    fn the_sidebar_re_homes_and_ctrl_b_is_bold() {
        assert_eq!(command(&chr("B"), true, true), Some(Command::Sidebar));
        assert_eq!(command(&chr("b"), true, false), Some(Command::Bold));
        assert_eq!(command(&chr("i"), true, false), Some(Command::Italic));
        assert_eq!(command(&chr("`"), true, false), Some(Command::Code));
        assert_eq!(
            super::command(
                &chr("²"),
                PhysicalKey::Code(KeyCode::Backquote),
                true,
                false,
                false
            ),
            Some(Command::Code),
            "AZERTY: the backtick key prints ² and the backtick needs AltGr"
        );
    }

    #[test]
    fn ctrl_j_toggles_justify() {
        assert_eq!(command(&chr("j"), true, false), Some(Command::Justify));
    }

    #[test]
    fn zoom_matches_plus_equals_minus() {
        assert_eq!(command(&chr("+"), true, true), Some(Command::ZoomIn));
        assert_eq!(command(&chr("="), true, false), Some(Command::ZoomIn));
        assert_eq!(command(&chr("-"), true, false), Some(Command::ZoomOut));
    }

    #[test]
    fn shift_distinguishes_the_copies() {
        assert_eq!(command(&chr("c"), true, false), Some(Command::CopyText));
        assert_eq!(command(&chr("C"), true, true), Some(Command::CopyMarkdown));
    }

    #[test]
    fn space_pages_both_ways() {
        let space = Key::Named(NamedKey::Space);
        assert_eq!(command(&space, false, false), Some(Command::PageDown));
        assert_eq!(command(&space, false, true), Some(Command::PageUp));
    }

    #[test]
    fn find_chords_resolve() {
        assert_eq!(command(&chr("f"), true, false), Some(Command::Find));
        assert_eq!(
            command(&Key::Named(NamedKey::F3), false, false),
            Some(Command::FindNext)
        );
        assert_eq!(
            command(&Key::Named(NamedKey::F3), false, true),
            Some(Command::FindPrev)
        );
    }

    #[test]
    fn plain_arrows_hand_the_panes() {
        let named = |n| Key::Named(n);
        assert_eq!(
            command(&named(NamedKey::ArrowLeft), false, false),
            Some(Command::PaneLeft)
        );
        assert_eq!(
            command(&named(NamedKey::ArrowRight), false, false),
            Some(Command::PaneRight)
        );
    }

    #[test]
    fn ctrl_tab_toggles_the_sidebar_tab() {
        let named = |n| Key::Named(n);
        assert_eq!(
            command(&named(NamedKey::Tab), true, false),
            Some(Command::SidebarTab)
        );
        // Ctrl on the bare arrows falls through to the pane transfer,
        // like every shifted chord; the caret's word jumps intercept
        // upstream while editing.
        assert_eq!(
            command(&named(NamedKey::ArrowLeft), true, false),
            Some(Command::PaneLeft)
        );
        assert_eq!(
            command(&named(NamedKey::ArrowRight), true, false),
            Some(Command::PaneRight)
        );
    }

    #[test]
    fn reload_matches_f5_and_ctrl_r() {
        assert_eq!(
            command(&Key::Named(NamedKey::F5), false, false),
            Some(Command::Reload)
        );
        assert_eq!(command(&chr("r"), true, false), Some(Command::Reload));
        assert_eq!(command(&chr("r"), false, false), None);
    }

    #[test]
    fn the_refetch_takes_ctrl_shift_r_and_leaves_the_reload_alone() {
        assert_eq!(command(&chr("r"), true, true), Some(Command::Refetch));
        assert_eq!(command(&chr("R"), true, true), Some(Command::Refetch));
        assert_eq!(command(&chr("r"), true, false), Some(Command::Reload));
        let row = SHORTCUTS
            .iter()
            .find(|row| row.keys == "Ctrl+Shift+R")
            .expect("a help row");
        assert_eq!(row.section, "Files");
    }

    #[test]
    fn named_keys_scroll_and_quit() {
        let named = |n| Key::Named(n);
        assert_eq!(
            command(&named(NamedKey::ArrowDown), false, false),
            Some(Command::LineDown)
        );
        assert_eq!(
            command(&named(NamedKey::ArrowUp), false, false),
            Some(Command::LineUp)
        );
        assert_eq!(
            command(&named(NamedKey::PageDown), false, false),
            Some(Command::PageDown)
        );
        assert_eq!(
            command(&named(NamedKey::PageUp), false, false),
            Some(Command::PageUp)
        );
        assert_eq!(
            command(&named(NamedKey::Home), true, false),
            Some(Command::Top)
        );
        assert_eq!(
            command(&named(NamedKey::End), false, false),
            Some(Command::Bottom)
        );
        assert_eq!(
            command(&named(NamedKey::Escape), false, false),
            Some(Command::Quit)
        );
        assert_eq!(
            command(&named(NamedKey::F1), false, false),
            Some(Command::Help)
        );
    }

    #[test]
    fn unmodified_characters_resolve_to_nothing() {
        assert_eq!(command(&chr("t"), false, false), None);
        assert_eq!(command(&chr("="), false, false), None);
    }

    #[test]
    fn labels_swap_ctrl_for_cmd_on_macos() {
        assert_eq!(platform_label("Ctrl+Shift+C", true), "Cmd+Shift+C");
        assert_eq!(platform_label("Ctrl+T", false), "Ctrl+T");
        assert_eq!(platform_label("F1", true), "F1");
    }
}
