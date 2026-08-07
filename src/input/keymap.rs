//! Shortcut map: the single source for key dispatch and the help table.
//! Each table row carries the labels the help overlay renders and the
//! chords the app matches, so the two can never drift apart.

use winit::keyboard::{Key, NamedKey};

/// Application command a shortcut resolves to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    OpenFile,
    Reload,
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
    SelectAll,
    CopyText,
    CopyMarkdown,
    Find,
    FindNext,
    FindPrev,
    LineUp,
    LineDown,
    PaneLeft,
    PaneRight,
    SidebarTab,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Quit,
}

impl Command {
    /// Every variant; the coverage test checks each one against the table.
    pub const ALL: [Command; 28] = [
        Command::OpenFile,
        Command::Reload,
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
        Command::LineUp,
        Command::LineDown,
        Command::PaneLeft,
        Command::PaneRight,
        Command::SidebarTab,
        Command::PageUp,
        Command::PageDown,
        Command::Top,
        Command::Bottom,
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
}

/// One help-table row: display labels plus the chords the row covers.
pub struct Shortcut {
    pub keys: &'static str,
    pub action: &'static str,
    bindings: &'static [(Binding, Command)],
}

pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        keys: "Ctrl+O",
        action: "Open file",
        bindings: &[(Binding::Ctrl("o"), Command::OpenFile)],
    },
    Shortcut {
        keys: "Ctrl+,",
        action: "Settings",
        bindings: &[(Binding::Ctrl(","), Command::Settings)],
    },
    Shortcut {
        keys: "Ctrl+T",
        action: "Theme browser",
        bindings: &[(Binding::Ctrl("t"), Command::ThemeBrowser)],
    },
    Shortcut {
        keys: "Ctrl+B",
        action: "Folder sidebar",
        bindings: &[(Binding::Ctrl("b"), Command::Sidebar)],
    },
    Shortcut {
        keys: "Ctrl+P",
        action: "Export to PDF",
        bindings: &[(Binding::Ctrl("p"), Command::Export)],
    },
    Shortcut {
        keys: "Ctrl+Shift+P",
        action: "Export settings",
        bindings: &[(Binding::CtrlShift("p"), Command::ExportSettings)],
    },
    Shortcut {
        keys: "F1",
        action: "Shortcuts help",
        bindings: &[(Binding::Named(NamedKey::F1), Command::Help)],
    },
    Shortcut {
        keys: "F5 / Ctrl+R",
        action: "Reload from disk",
        bindings: &[
            (Binding::Named(NamedKey::F5), Command::Reload),
            (Binding::Ctrl("r"), Command::Reload),
        ],
    },
    Shortcut {
        keys: "Ctrl+Plus / Ctrl+Minus",
        action: "Zoom in / out",
        bindings: &[
            (Binding::Ctrl("+"), Command::ZoomIn),
            (Binding::Ctrl("="), Command::ZoomIn),
            (Binding::Ctrl("-"), Command::ZoomOut),
        ],
    },
    Shortcut {
        keys: "Ctrl+0",
        action: "Reset zoom",
        bindings: &[(Binding::Ctrl("0"), Command::ZoomReset)],
    },
    Shortcut {
        keys: "Ctrl+J",
        action: "Justify text (EPUB books only)",
        bindings: &[(Binding::Ctrl("j"), Command::Justify)],
    },
    Shortcut {
        keys: "Ctrl+A",
        action: "Select all",
        bindings: &[(Binding::Ctrl("a"), Command::SelectAll)],
    },
    Shortcut {
        keys: "Ctrl+C",
        action: "Copy selection as text",
        bindings: &[(Binding::Ctrl("c"), Command::CopyText)],
    },
    Shortcut {
        keys: "Ctrl+Shift+C",
        action: "Copy selection as markdown",
        bindings: &[(Binding::CtrlShift("c"), Command::CopyMarkdown)],
    },
    Shortcut {
        keys: "Ctrl+F",
        action: "Find in document",
        bindings: &[(Binding::Ctrl("f"), Command::Find)],
    },
    Shortcut {
        keys: "F3 / Shift+F3",
        action: "Next / previous match",
        bindings: &[
            (Binding::ShiftNamed(NamedKey::F3), Command::FindPrev),
            (Binding::Named(NamedKey::F3), Command::FindNext),
        ],
    },
    Shortcut {
        keys: "Up / Down",
        action: "Scroll by line, or move the sidebar selection",
        bindings: &[
            (Binding::Named(NamedKey::ArrowUp), Command::LineUp),
            (Binding::Named(NamedKey::ArrowDown), Command::LineDown),
        ],
    },
    Shortcut {
        keys: "Left / Right",
        action: "Toggle between sidebar and document",
        bindings: &[
            (Binding::Named(NamedKey::ArrowLeft), Command::PaneLeft),
            (Binding::Named(NamedKey::ArrowRight), Command::PaneRight),
        ],
    },
    Shortcut {
        keys: "Ctrl+Left / Ctrl+Right",
        action: "Toggle the sidebar tab",
        bindings: &[
            (Binding::CtrlNamed(NamedKey::ArrowLeft), Command::SidebarTab),
            (
                Binding::CtrlNamed(NamedKey::ArrowRight),
                Command::SidebarTab,
            ),
        ],
    },
    Shortcut {
        keys: "Page Up / Page Down, Space / Shift+Space",
        action: "Scroll by page",
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
        bindings: &[
            (Binding::Named(NamedKey::Home), Command::Top),
            (Binding::Named(NamedKey::End), Command::Bottom),
        ],
    },
    Shortcut {
        keys: "Escape",
        action: "Close overlay or sidebar / quit",
        bindings: &[(Binding::Named(NamedKey::Escape), Command::Quit)],
    },
];

/// Resolves a key event against the table. Chords that require a
/// modifier are tried first, so Ctrl+Shift+C never falls through to
/// Ctrl+C and Ctrl+Left never falls through to plain Left.
pub fn command(key: &Key, ctrl: bool, shift: bool) -> Option<Command> {
    let bindings = || SHORTCUTS.iter().flat_map(|row| row.bindings.iter());
    let shifted = bindings().find(|(binding, _)| match binding {
        Binding::CtrlShift(c) => ctrl && shift && is_char(key, c),
        Binding::ShiftNamed(n) => shift && is_named(key, n),
        Binding::CtrlNamed(n) => ctrl && is_named(key, n),
        _ => false,
    });
    let plain = || {
        bindings().find(|(binding, _)| match binding {
            Binding::Ctrl(c) => ctrl && is_char(key, c),
            Binding::Named(n) => is_named(key, n),
            _ => false,
        })
    };
    shifted.or_else(plain).map(|(_, cmd)| *cmd)
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

    fn chr(s: &str) -> Key {
        Key::Character(s.into())
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
        assert_eq!(command(&chr("b"), true, false), Some(Command::Sidebar));
        assert_eq!(command(&chr("t"), true, false), Some(Command::ThemeBrowser));
        assert_eq!(command(&chr("T"), true, true), Some(Command::ThemeBrowser));
        assert_eq!(command(&chr(","), true, false), Some(Command::Settings));
        assert_eq!(command(&chr("a"), true, false), Some(Command::SelectAll));
        assert_eq!(command(&chr("0"), true, false), Some(Command::ZoomReset));
    }

    #[test]
    fn the_print_pair_exports_and_the_e_chords_are_free() {
        assert_eq!(command(&chr("p"), true, false), Some(Command::Export));
        assert_eq!(
            command(&chr("P"), true, true),
            Some(Command::ExportSettings)
        );
        assert_eq!(
            command(&chr("e"), true, false),
            None,
            "reserved for the editing phase's view toggle"
        );
        assert_eq!(command(&chr("E"), true, true), None);
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
    fn ctrl_arrows_toggle_the_sidebar_tab() {
        let named = |n| Key::Named(n);
        assert_eq!(
            command(&named(NamedKey::ArrowLeft), true, false),
            Some(Command::SidebarTab)
        );
        assert_eq!(
            command(&named(NamedKey::ArrowRight), true, false),
            Some(Command::SidebarTab)
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
