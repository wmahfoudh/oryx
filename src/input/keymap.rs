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
    HiddenFiles,
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
    Regex,
    GoToLine,
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
    Forward,
    Edit,
    Cut,
    Paste,
    PasteFull,
    Undo,
    Redo,
    Save,
    SaveAs,
    NewFile,
    /// An untitled markdown note in the editor, named at the first save.
    NewNote,
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
    /// The case of the selection or the word: lower case, a capital on
    /// each word, upper case, in a cycle.
    Case,
    MoveLineUp,
    MoveLineDown,
    DuplicateLines,
    DeleteLines,
    Comment,
    Quit,
}

impl Command {
    /// Every variant. One test checks each entry has a row in the table,
    /// another that every command a row binds is listed here.
    pub const ALL: [Command; 67] = [
        Command::OpenFile,
        Command::Reload,
        Command::Refetch,
        Command::Sidebar,
        Command::HiddenFiles,
        Command::Export,
        Command::ExportSettings,
        Command::Help,
        Command::Settings,
        Command::ThemeBrowser,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::Justify,
        Command::Direction,
        Command::SelectAll,
        Command::CopyText,
        Command::CopyMarkdown,
        Command::Find,
        Command::FindNext,
        Command::FindPrev,
        Command::Replace,
        Command::Regex,
        Command::GoToLine,
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
        Command::Forward,
        Command::Edit,
        Command::Cut,
        Command::Paste,
        Command::PasteFull,
        Command::Undo,
        Command::Redo,
        Command::Save,
        Command::SaveAs,
        Command::NewFile,
        Command::NewNote,
        Command::BulletList,
        Command::NumberedList,
        Command::TaskList,
        Command::Quote,
        Command::Heading(1),
        Command::Heading(2),
        Command::Heading(3),
        Command::Heading(4),
        Command::Heading(5),
        Command::Heading(6),
        Command::TaskToggle,
        Command::Bold,
        Command::Italic,
        Command::Code,
        Command::Link,
        Command::Case,
        Command::MoveLineUp,
        Command::MoveLineDown,
        Command::DuplicateLines,
        Command::DeleteLines,
        Command::Comment,
        Command::Quit,
    ];
}

/// What has the keys, in the order the routing of the window's key
/// event asks: the question, a dialog, Go to line, the search bar,
/// then the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOwner {
    /// The unsaved changes or recovery question, answered by letters.
    Question,
    /// A dialog; `field` while one of its text fields is open.
    Dialog {
        field: bool,
    },
    GoTo,
    Search,
    /// The editor, or the page in reading mode.
    Text,
}

impl Command {
    /// Whether the command has a receiver while `owner` has the keys.
    /// The regex toggle acts on the search bar alone. While a text
    /// field or a dialog has the keyboard, the document's editing and
    /// selection keys fall silent, so nothing typed at a field ever
    /// edits the page; the app-wide chords stay live (files, find,
    /// navigation, view, export, help), and so does the editor toggle,
    /// a mode switch. A press whose command has no receiver is no
    /// chord (`command`), so that Option on a Mac types the key's
    /// character (`alt_typing`).
    pub fn live(self, owner: KeyOwner) -> bool {
        match (self, owner) {
            (Command::Regex, _) => owner == KeyOwner::Search,
            (_, KeyOwner::Text) => true,
            _ => match section_of(self) {
                Some("Edit") => self == Command::Edit,
                Some("Selection") => false,
                _ => true,
            },
        }
    }
}

/// The section of the table row that binds `cmd`.
fn section_of(cmd: Command) -> Option<&'static str> {
    SHORTCUTS
        .iter()
        .find(|row| row.bindings.iter().any(|(_, c)| *c == cmd))
        .map(|row| row.section)
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
    /// Alt alone plus a key by its place on the keyboard, tried last:
    /// the digit chords, on layouts where a digit needs Shift. The
    /// bullet list's row holds on Linux and Windows only: a Mac layout
    /// has `-` on a plain key, matched by the keycap, and Option on the
    /// key at the minus's place types a character of its own there.
    AltCode(KeyCode),
    /// Ctrl alone plus a key by its place on the keyboard, tried last,
    /// once no character chord claims the press. The zoom keys live
    /// here so that a layout printing something else on the `0`, `-`
    /// or `=` keys still zooms with them, the way browsers do.
    CtrlCode(KeyCode),
    /// A chord that holds on macOS only, where the Mac's own habits
    /// take a key: `Option+Left` moves by a word in every Mac app, so
    /// back and forward take the browser's `Cmd+[` and `Cmd+]` there.
    Mac(&'static Binding),
    /// A chord that holds everywhere but on macOS.
    NotMac(&'static Binding),
}

impl Binding {
    /// The chord as it stands on a platform: itself, or the one it
    /// wraps for that platform, or nothing there.
    fn on(&self, macos: bool) -> Option<&Binding> {
        match self {
            Binding::Mac(inner) => macos.then_some(*inner),
            Binding::NotMac(inner) => (!macos).then_some(*inner),
            other => Some(other),
        }
    }
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
        keys: "Ctrl+M",
        action: "New markdown note; the name and the type are chosen when saving",
        section: "Files",
        bindings: &[(Binding::Ctrl("m"), Command::NewNote)],
    },
    Shortcut {
        keys: "Ctrl+S",
        action: "Save (editing)",
        section: "Files",
        bindings: &[(Binding::Ctrl("s"), Command::Save)],
    },
    Shortcut {
        keys: "Ctrl+Shift+S",
        action: "Save as",
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
        keys: "Alt+Left / Alt+Right",
        action: "Go back to where a jump left, and forward again",
        section: "Navigation",
        bindings: &[
            (
                Binding::NotMac(&Binding::AltNamed(NamedKey::ArrowLeft)),
                Command::Back,
            ),
            (
                Binding::NotMac(&Binding::AltNamed(NamedKey::ArrowRight)),
                Command::Forward,
            ),
            (Binding::Mac(&Binding::Ctrl("[")), Command::Back),
            (Binding::Mac(&Binding::Ctrl("]")), Command::Forward),
            (Binding::Mac(&Binding::CtrlCode(KeyCode::BracketLeft)), Command::Back),
            (Binding::Mac(&Binding::CtrlCode(KeyCode::BracketRight)), Command::Forward),
        ],
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
        keys: "Ctrl+Shift+H",
        action: "Show or hide hidden files in the sidebar",
        section: "Navigation",
        bindings: &[(Binding::CtrlShift("h"), Command::HiddenFiles)],
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
    Shortcut {
        keys: "Alt+R",
        action: "Regex matching on/off",
        section: "Find",
        bindings: &[(Binding::Alt("r"), Command::Regex)],
    },
    Shortcut {
        keys: "Ctrl+H",
        action: "Find and replace (editing only)",
        section: "Find",
        bindings: &[(Binding::Ctrl("h"), Command::Replace)],
    },
    // Handled by the open search bar and by the sidebar themselves, so
    // no command binding.
    Shortcut {
        keys: "Ctrl+Enter",
        action: "Replace all (replace open); in the sidebar, open the highlighted file in a new window, as a middle click on its row does",
        section: "Find",
        bindings: &[],
    },
    Shortcut {
        keys: "Ctrl+G",
        action: "Go to a line: 412, or 412:10 for a column too",
        section: "Find",
        bindings: &[(Binding::Ctrl("g"), Command::GoToLine)],
    },
    Shortcut {
        keys: "Ctrl+A",
        action: "Select all",
        section: "Selection",
        bindings: &[(Binding::Ctrl("a"), Command::SelectAll)],
    },
    Shortcut {
        keys: "Ctrl+C",
        action: "Copy the selection with its formatting, or the line (editing)",
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
        action: "Cut the selection, or the line (editing)",
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
        keys: "Ctrl+Shift+V",
        action: "Paste a big picture at its full size (editing markdown)",
        section: "Edit",
        bindings: &[(Binding::CtrlShift("v"), Command::PasteFull)],
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
            (Binding::NotMac(&Binding::AltCode(KeyCode::Minus)), Command::BulletList),
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
        action: "Link around the selection or the word, or an empty link; pasting an address over a selection links it too (markdown editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("k"), Command::Link)],
    },
    Shortcut {
        keys: "Ctrl+U",
        action: "Change the case of the selection or the word, in turn: lower case, a capital on each word, upper case (editing)",
        section: "Edit",
        bindings: &[(Binding::Ctrl("u"), Command::Case)],
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
        keys: "Ctrl+Shift+D",
        action: "Duplicate the line or the selected lines (editing)",
        section: "Edit",
        bindings: &[(Binding::CtrlShift("d"), Command::DuplicateLines)],
    },
    Shortcut {
        keys: "Ctrl+Shift+K",
        action: "Delete the line or the selected lines (editing)",
        section: "Edit",
        bindings: &[(Binding::CtrlShift("k"), Command::DeleteLines)],
    },
    Shortcut {
        keys: "Ctrl+/",
        action: "Comment or uncomment the line or the selected lines (code and markdown editing)",
        section: "Edit",
        bindings: &[
            (Binding::Ctrl("/"), Command::Comment),
            (Binding::CtrlCode(KeyCode::Slash), Command::Comment),
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
        action: "Settings: fonts, sizes, interface scale, line numbers, word count and autosave",
        section: "View",
        bindings: &[
            (Binding::Ctrl(","), Command::Settings),
            (Binding::CtrlCode(KeyCode::Comma), Command::Settings),
        ],
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

/// A key press as the window reports it.
#[derive(Clone, Copy)]
pub struct Press<'a> {
    /// What the press types, with the modifiers applied: a character,
    /// or a named key.
    pub typed: &'a Key,
    /// The key's own character, with no modifier: the keycap.
    pub keycap: &'a Key,
    /// The key's place on the keyboard.
    pub code: PhysicalKey,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Resolves a key press against the table, for `owner` holding the
/// keys. Chords that require a modifier are tried first, so
/// Ctrl+Shift+C never falls through to Ctrl+C and Ctrl+Left never
/// falls through to plain Left; the key's place on the keyboard is
/// tried last, once no character chord has claimed the press, with the
/// row's modifier alone, and never for a plain letter, which is a
/// chord by its letter wherever it sits. A letter chord is matched
/// with `latin_key`, so a layout that types another script keeps its
/// letter chords. A command with no receiver under `owner` is no chord
/// (`Command::live`): the regex toggle without a search bar, a list
/// key under a field. On a Mac such a press types its Option character
/// (`alt_typing`).
pub fn command(press: Press, owner: KeyOwner) -> Option<Command> {
    command_on(press, owner, cfg!(target_os = "macos"))
}

/// `command` for a named platform, so a test can ask for the Mac's
/// chords from anywhere.
fn command_on(press: Press, owner: KeyOwner, macos: bool) -> Option<Command> {
    let Press {
        typed,
        keycap,
        code,
        ctrl,
        shift,
        alt,
    } = press;
    let character = matches!(typed, Key::Character(_));
    // Ctrl and Alt both held with a character key is the left Ctrl+Alt
    // of Windows, the AltGr of a layout: the character stands and no
    // row runs, by the character or by the key's place.
    if ctrl && alt && character {
        return None;
    }
    // Option on a Mac gives a key a character of its own. With Shift
    // the press types it. Otherwise a chord is matched by the keycap,
    // as the Mac's menus do, so `Alt+-` holds wherever a layout puts
    // the minus key, and a row by place does not take a plain ASCII
    // character the layout keeps under Option (`}` under `)` on a
    // French Mac): the press types it.
    let option = macos && alt && !ctrl && character;
    if option && shift {
        return None;
    }
    let latin = latin_key(if option { keycap } else { typed }, code);
    let key = &latin;
    let plain_option = option && matches!(typed, Key::Character(c) if c.is_ascii());
    let letter =
        matches!(key, Key::Character(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_alphabetic());
    let by_place = !plain_option && !letter;
    let bindings = || {
        SHORTCUTS
            .iter()
            .flat_map(|row| row.bindings.iter())
            .filter_map(move |(binding, cmd)| Some((binding.on(macos)?, *cmd)))
    };
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
            Binding::CtrlCode(c) => {
                by_place && ctrl && !shift && !alt && code == PhysicalKey::Code(*c)
            }
            Binding::AltCode(c) => {
                by_place && alt && !shift && !ctrl && code == PhysicalKey::Code(*c)
            }
            _ => false,
        })
    };
    shifted
        .or_else(plain)
        .or_else(physical)
        .map(|(_, cmd)| cmd)
        .filter(|cmd| cmd.live(owner))
}

/// The key a chord is matched with. The character the press types
/// stands when it is ASCII: a Latin letter wherever a layout puts it,
/// or the punctuation that AZERTY, Dvorak and BÉPO put on letter keys.
/// Otherwise, when the key sits on a letter of the US layout, that
/// letter serves, so that `Ctrl+C` holds on a Cyrillic, Greek or
/// Hebrew layout. The rule is the one Zed applies on Linux.
pub fn latin_key(key: &Key, code: PhysicalKey) -> Key {
    let Key::Character(typed) = key else {
        return key.clone();
    };
    if typed.is_ascii() {
        return key.clone();
    }
    match code {
        PhysicalKey::Code(code) => match us_letter(code) {
            Some(letter) => Key::Character(letter.into()),
            None => key.clone(),
        },
        PhysicalKey::Unidentified(_) => key.clone(),
    }
}

/// A press with Option held that types rather than chords, on a Mac
/// only: Option gives a character of its own there (a symbol, or an
/// accented letter), and a key that no chord claims types it. Elsewhere
/// Alt leaves the character as it is, and a press with Alt is a chord or
/// nothing. Ctrl held beside Alt stands as typed (`chord_held`). The
/// editor and the fields then take the character with Alt off.
pub fn alt_typing(logical: &Key, ctrl: bool, alt: bool, claimed: bool, mac: bool) -> bool {
    mac && alt && !ctrl && !claimed && matches!(logical, Key::Character(_))
}

/// Whether a press with these modifiers is a chord for a field or the
/// editor, which then match it with `latin_key`: Ctrl or Alt alone.
/// Both held is an AltGr character on Windows, which stands as typed.
pub fn chord_held(ctrl: bool, alt: bool) -> bool {
    ctrl != alt
}

/// The letter printed on a key of the US layout.
fn us_letter(code: KeyCode) -> Option<&'static str> {
    Some(match code {
        KeyCode::KeyA => "a",
        KeyCode::KeyB => "b",
        KeyCode::KeyC => "c",
        KeyCode::KeyD => "d",
        KeyCode::KeyE => "e",
        KeyCode::KeyF => "f",
        KeyCode::KeyG => "g",
        KeyCode::KeyH => "h",
        KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j",
        KeyCode::KeyK => "k",
        KeyCode::KeyL => "l",
        KeyCode::KeyM => "m",
        KeyCode::KeyN => "n",
        KeyCode::KeyO => "o",
        KeyCode::KeyP => "p",
        KeyCode::KeyQ => "q",
        KeyCode::KeyR => "r",
        KeyCode::KeyS => "s",
        KeyCode::KeyT => "t",
        KeyCode::KeyU => "u",
        KeyCode::KeyV => "v",
        KeyCode::KeyW => "w",
        KeyCode::KeyX => "x",
        KeyCode::KeyY => "y",
        KeyCode::KeyZ => "z",
        _ => return None,
    })
}

fn is_char(key: &Key, c: &str) -> bool {
    matches!(key, Key::Character(s) if s.eq_ignore_ascii_case(c))
}

fn is_named(key: &Key, n: &NamedKey) -> bool {
    matches!(key, Key::Named(k) if k == n)
}

/// Chord label for the running platform: Ctrl renders as Cmd on macOS,
/// and back and forward as the Mac's own keys.
pub fn display(keys: &str) -> String {
    platform_label(keys, cfg!(target_os = "macos"))
}

fn platform_label(keys: &str, macos: bool) -> String {
    if macos {
        keys.replace("Alt+Left", "Cmd+[")
            .replace("Alt+Right", "Cmd+]")
            .replace("Ctrl", "Cmd")
    } else {
        keys.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::NativeKeyCode;

    #[test]
    fn an_option_character_is_typing_unless_a_chord_claims_it() {
        let option_d = Key::Character("\u{2202}".into());
        let at = Key::Character("@".into());
        let mac = true;
        assert!(
            alt_typing(&option_d, false, true, false, mac),
            "Option+D on a Mac types its character"
        );
        assert!(
            alt_typing(&at, false, true, false, mac),
            "Option+L on a German Mac types @, ASCII"
        );
        assert!(
            !alt_typing(&option_d, false, false, false, mac),
            "without Alt it is a plain character already"
        );
        assert!(
            !alt_typing(&option_d, false, true, true, mac),
            "a chord that claims the key wins, as Alt+X does"
        );
        assert!(
            !alt_typing(&option_d, true, true, false, mac),
            "Cmd beside Option is a chord, not typing"
        );
        let cyrillic_d = Key::Character("\u{432}".into());
        assert!(
            !alt_typing(&cyrillic_d, false, true, false, !mac),
            "elsewhere Alt leaves the character: a chord or nothing"
        );
        assert!(
            !alt_typing(&cyrillic_d, true, true, false, !mac),
            "Ctrl+Alt with a letter stands as typed, and types nothing"
        );
    }

    fn chr(s: &str) -> Key {
        Key::Character(s.into())
    }

    /// A press on Linux or Windows, where Alt leaves the character: the
    /// keycap is the character typed.
    fn resolve(
        key: &Key,
        code: PhysicalKey,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Command> {
        resolve_on(key, code, ctrl, shift, alt, false)
    }

    /// `resolve` for a named platform, the keycap still the character
    /// typed; a Mac press with Option builds its own `Press`.
    fn resolve_on(
        key: &Key,
        code: PhysicalKey,
        ctrl: bool,
        shift: bool,
        alt: bool,
        macos: bool,
    ) -> Option<Command> {
        super::command_on(
            Press {
                typed: key,
                keycap: key,
                code,
                ctrl,
                shift,
                alt,
            },
            KeyOwner::Text,
            macos,
        )
    }

    /// A Mac press with Option held: the character it types, the
    /// keycap, the key's place, and who has the keys.
    fn option_on(
        typed: &str,
        keycap: &str,
        code: KeyCode,
        shift: bool,
        owner: KeyOwner,
    ) -> Option<Command> {
        super::command_on(
            Press {
                typed: &chr(typed),
                keycap: &chr(keycap),
                code: PhysicalKey::Code(code),
                ctrl: false,
                shift,
                alt: true,
            },
            owner,
            true,
        )
    }

    /// The table predates the Alt modifier and the physical key; the
    /// shadow keeps the older assertions readable.
    fn command(key: &Key, ctrl: bool, shift: bool) -> Option<Command> {
        resolve(
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
            resolve(&chr(c), PhysicalKey::Code(code), ctrl, false, false)
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

    /// On a layout that types another script, a letter chord holds by
    /// the key's place: Ctrl with the key of C types "с" on a Cyrillic
    /// layout and still copies. A Latin layout that moves its letters
    /// keeps the character, as before.
    #[test]
    fn the_letter_chords_hold_on_a_cyrillic_layout_by_the_key() {
        let on = |c: &str, code: KeyCode, ctrl: bool, shift: bool| {
            resolve(&chr(c), PhysicalKey::Code(code), ctrl, shift, false)
        };
        assert_eq!(on("с", KeyCode::KeyC, true, false), Some(Command::CopyText));
        assert_eq!(on("м", KeyCode::KeyV, true, false), Some(Command::Paste));
        assert_eq!(
            on("ф", KeyCode::KeyA, true, false),
            Some(Command::SelectAll)
        );
        assert_eq!(on("Ы", KeyCode::KeyS, true, true), Some(Command::SaveAs));
        assert_eq!(
            on("a", KeyCode::KeyQ, true, false),
            Some(Command::SelectAll),
            "AZERTY keeps the character"
        );
        assert_eq!(
            on("с", KeyCode::KeyC, false, false),
            None,
            "no chord without Ctrl"
        );
    }

    /// The key a chord is matched with: the character typed when it is
    /// ASCII, else the letter of the key's place on the US layout when
    /// it sits on one.
    #[test]
    fn the_latin_key_of_a_press() {
        let latin = |c: &str, code: KeyCode| latin_key(&chr(c), PhysicalKey::Code(code));
        assert_eq!(latin("с", KeyCode::KeyC), chr("c"));
        assert_eq!(latin("Ж", KeyCode::Semicolon), chr("Ж"), "not a letter key");
        assert_eq!(latin("a", KeyCode::KeyQ), chr("a"), "a Latin letter stands");
        assert_eq!(latin("é", KeyCode::Digit2), chr("é"));
        assert_eq!(
            latin_key(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(KeyCode::KeyA)
            ),
            Key::Named(NamedKey::Enter)
        );
        assert_eq!(
            latin_key(
                &chr("с"),
                PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
            ),
            chr("с"),
            "no place known"
        );
    }

    /// An ASCII character on a letter key stands: the punctuation that
    /// AZERTY, Dvorak and BÉPO put on letter keys keeps its chord. A
    /// non-ASCII letter there goes by the key's place, as Zed does on
    /// Linux.
    #[test]
    fn ascii_characters_on_letter_keys_keep_their_chords() {
        let on = |c: &str, code: KeyCode, ctrl: bool, alt: bool| {
            resolve(&chr(c), PhysicalKey::Code(code), ctrl, false, alt)
        };
        assert_eq!(
            on(",", KeyCode::KeyM, true, false),
            Some(Command::Settings),
            "AZERTY Ctrl+,"
        );
        assert_eq!(
            on(".", KeyCode::KeyE, false, true),
            Some(Command::Quote),
            "Dvorak Alt+."
        );
        assert_eq!(
            on("/", KeyCode::KeyQ, true, false),
            Some(Command::Comment),
            "Hebrew Ctrl+/"
        );
        assert_eq!(
            on("à", KeyCode::KeyZ, true, false),
            Some(Command::Undo),
            "BÉPO Ctrl+à, by the key"
        );
    }

    /// Ctrl and Alt both held is how Windows reports an AltGr character
    /// typed with the left keys: the character stands and no row runs,
    /// by the character or by the key's place. A named key keeps its
    /// rows.
    #[test]
    fn an_altgr_character_under_ctrl_and_alt_is_no_chord() {
        let on = |c: &str, code: KeyCode, shift: bool| {
            resolve(&chr(c), PhysicalKey::Code(code), true, shift, true)
        };
        assert_eq!(on("€", KeyCode::KeyE, false), None, "German Ctrl+Alt+E");
        assert_eq!(on("ś", KeyCode::KeyS, false), None, "Polish Ctrl+Alt+S");
        assert_eq!(on("x", KeyCode::KeyX, false), None, "US: no task list");
        assert_eq!(
            on("1", KeyCode::Digit1, false),
            None,
            "US: no list, no heading"
        );
        assert_eq!(on("S", KeyCode::KeyS, true), None, "US: no save as");
        assert_eq!(
            resolve(
                &Key::Named(NamedKey::ArrowUp),
                PhysicalKey::Code(KeyCode::ArrowUp),
                true,
                false,
                true
            ),
            Some(Command::MoveLineUp),
            "a named key keeps its row"
        );
    }

    /// The settings chord holds by the key's place, as the zoom and the
    /// heading chords do: `Ctrl` with the key at the US comma opens the
    /// settings on a layout where that key prints a letter. On a Mac,
    /// back and forward hold by the bracket keys the same way.
    #[test]
    fn the_settings_and_the_mac_brackets_hold_by_the_key_on_other_layouts() {
        let on = |c: &str, code: KeyCode, ctrl: bool| {
            resolve(&chr(c), PhysicalKey::Code(code), ctrl, false, false)
        };
        assert_eq!(
            on("б", KeyCode::Comma, true),
            Some(Command::Settings),
            "Russian"
        );
        assert_eq!(
            on("ת", KeyCode::Comma, true),
            Some(Command::Settings),
            "Hebrew"
        );
        assert_eq!(on(",", KeyCode::Comma, true), Some(Command::Settings));
        assert_eq!(
            on("б", KeyCode::Comma, false),
            None,
            "no chord without Ctrl"
        );
        let mac = |c: &str, code: KeyCode| {
            resolve_on(&chr(c), PhysicalKey::Code(code), true, false, false, true)
        };
        assert_eq!(
            mac("х", KeyCode::BracketLeft),
            Some(Command::Back),
            "Russian Mac"
        );
        assert_eq!(mac("ъ", KeyCode::BracketRight), Some(Command::Forward));
    }

    /// A physical row holds with its own modifier alone. With Shift, or
    /// with Ctrl and Alt both held, the press types its character: the
    /// em dash of Option+Shift+minus on a Mac, and the characters the
    /// left Ctrl+Alt types on a German Windows layout.
    #[test]
    fn a_physical_row_holds_with_its_own_modifier_alone() {
        let on = |c: &str, code: KeyCode, ctrl: bool, shift: bool, alt: bool, mac: bool| {
            resolve_on(&chr(c), PhysicalKey::Code(code), ctrl, shift, alt, mac)
        };
        assert_eq!(
            on("\u{b2}", KeyCode::Digit2, true, false, true, false),
            None,
            "German Windows: Ctrl+Alt+2 types ²"
        );
        assert_eq!(
            on("}", KeyCode::Digit0, true, false, true, false),
            None,
            "German Windows: Ctrl+Alt+0 types a closing brace"
        );
        assert_eq!(
            on("\\", KeyCode::Minus, true, false, true, false),
            None,
            "German Windows: Ctrl+Alt+ß types a backslash"
        );
        assert_eq!(
            on("\u{2014}", KeyCode::Minus, false, true, true, true),
            None,
            "Mac: Option+Shift+minus types an em dash"
        );
        assert_eq!(
            on("_", KeyCode::Minus, false, true, true, false),
            None,
            "elsewhere Alt+Shift+minus is no chord"
        );
    }

    /// Option on a Mac: a chord by the keycap, as the Mac's menus match
    /// one; the typed character with Shift, or when it is a plain one
    /// the layout keeps under Option.
    #[test]
    fn on_the_mac_option_chords_by_the_keycap_and_types_the_rest() {
        let on = |typed: &str, keycap: &str, code: KeyCode, shift: bool| {
            option_on(typed, keycap, code, shift, KeyOwner::Text)
        };
        assert_eq!(
            on("\u{2013}", "-", KeyCode::Minus, false),
            Some(Command::BulletList),
            "US: Option+minus types an en dash, and is the bullet list"
        );
        assert_eq!(
            on("\u{2014}", "-", KeyCode::Equal, false),
            Some(Command::BulletList),
            "French: the minus key sits at the US equal place"
        );
        assert_eq!(
            on("\u{2013}", "-", KeyCode::Slash, false),
            Some(Command::BulletList),
            "German: the minus key sits at the US slash place"
        );
        assert_eq!(
            on("\u{a1}", "1", KeyCode::Digit1, false),
            Some(Command::NumberedList)
        );
        assert_eq!(
            on("\u{2248}", "x", KeyCode::KeyX, false),
            Some(Command::TaskList)
        );
        assert_eq!(
            on("\u{2265}", ".", KeyCode::Period, false),
            Some(Command::Quote)
        );
        assert_eq!(
            on("\u{2026}", "&", KeyCode::Digit1, false),
            Some(Command::NumberedList),
            "AZERTY Mac: the 1 key prints &, and the row by place holds"
        );
        assert_eq!(
            on("}", ")", KeyCode::Minus, false),
            None,
            "French: Option with the ) key types a closing brace"
        );
        assert_eq!(
            on("\\", "\u{df}", KeyCode::Minus, false),
            None,
            "German: Option with the ß key types a backslash"
        );
        assert_eq!(
            on("@", "l", KeyCode::KeyL, false),
            None,
            "German: Option+L types @"
        );
        assert_eq!(
            on("\u{2014}", "-", KeyCode::Minus, true),
            None,
            "US: Option+Shift+minus types an em dash"
        );
        assert_eq!(
            on("\u{2db}", "x", KeyCode::KeyX, true),
            None,
            "Option+Shift+X types, no task list"
        );
    }

    /// After the read of Task 316: the bullet list's row by place holds
    /// on Linux and Windows only, a plain Option character wins over a
    /// row by place and not over a chord by the keycap, and a row by
    /// place takes no plain letter.
    #[test]
    fn the_rows_by_place_leave_the_mac_dash_a_plain_option_character_and_a_letter() {
        let mac = |typed: &str, keycap: &str, code: KeyCode| {
            option_on(typed, keycap, code, false, KeyOwner::Text)
        };
        assert_eq!(
            mac("\u{bf}", "\u{df}", KeyCode::Minus),
            None,
            "German Mac: Option with the ß key types an inverted question mark"
        );
        assert_eq!(
            mac("\u{201c}", "[", KeyCode::Minus),
            None,
            "Dvorak Mac: Option+[ types a quotation mark"
        );
        assert_eq!(
            resolve(
                &chr(")"),
                PhysicalKey::Code(KeyCode::Minus),
                false,
                false,
                true
            ),
            Some(Command::BulletList),
            "elsewhere the key at the US minus still makes a bullet list"
        );
        assert_eq!(
            mac("-", "-", KeyCode::NumpadSubtract),
            Some(Command::BulletList),
            "the keypad's minus with Option"
        );
        assert_eq!(
            mac("1", "1", KeyCode::Numpad1),
            Some(Command::NumberedList),
            "the keypad's 1 with Option"
        );
        assert_eq!(
            mac("!", "1", KeyCode::Digit1),
            Some(Command::NumberedList),
            "Ukrainian Mac: Option+1 types ! and is the numbered list"
        );
        assert_eq!(
            mac(">", ".", KeyCode::Period),
            Some(Command::Quote),
            "Czech Mac: Option+. types > and is the quote"
        );
        assert_eq!(
            mac("|", ":", KeyCode::Period),
            None,
            "Italian Mac: Option with the : key types |"
        );
        assert_eq!(
            mac("}", ")", KeyCode::Minus),
            None,
            "French Mac: Option with the ) key still types a closing brace"
        );
        assert_eq!(
            mac("\u{f7}", ":", KeyCode::Period),
            Some(Command::Quote),
            "French Mac: the quote by place on the : key"
        );
        let ctrl =
            |c: &str, code: KeyCode| resolve(&chr(c), PhysicalKey::Code(code), true, false, false);
        assert_eq!(
            ctrl("w", KeyCode::Comma),
            None,
            "Dvorak: Ctrl+W is a letter chord, bound to nothing"
        );
        assert_eq!(ctrl("\u{431}", KeyCode::Comma), Some(Command::Settings));
        assert_eq!(
            ctrl(";", KeyCode::Comma),
            Some(Command::Settings),
            "AZERTY: Ctrl with the ; key, by place"
        );
        assert_eq!(
            ctrl("z", KeyCode::Slash),
            Some(Command::Undo),
            "Dvorak: Ctrl+Z by its letter"
        );
    }

    /// Alt+R is a row of the keymap, so the rules of Option apply to it.
    /// Its receiver is the search bar, so it is a chord there alone: in
    /// the editor, Option+R is no chord and types ® on a Mac (claim 14
    /// of the hunt of 1.3.0).
    #[test]
    fn alt_r_toggles_the_regex_under_the_search_bar_alone() {
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let linux = |typed: &str, shift: bool, owner: KeyOwner| {
            super::command_on(
                Press {
                    typed: &chr(typed),
                    keycap: &chr(typed),
                    code: none,
                    ctrl: false,
                    shift,
                    alt: true,
                },
                owner,
                false,
            )
        };
        assert_eq!(linux("r", false, KeyOwner::Search), Some(Command::Regex));
        assert_eq!(
            linux("R", true, KeyOwner::Search),
            Some(Command::Regex),
            "Shift makes no difference elsewhere"
        );
        assert_eq!(
            linux("r", false, KeyOwner::Text),
            None,
            "no search bar, nothing to toggle"
        );
        assert_eq!(
            linux("r", false, KeyOwner::GoTo),
            None,
            "Go to line closes the search bar"
        );
        assert_eq!(
            option_on("\u{ae}", "r", KeyCode::KeyR, false, KeyOwner::Search),
            Some(Command::Regex),
            "US Mac: Option+R under the search bar"
        );
        assert_eq!(
            option_on("\u{ae}", "r", KeyCode::KeyR, false, KeyOwner::Text),
            None,
            "US Mac: Option+R in the editor is no chord"
        );
        let registered = chr("\u{ae}");
        assert!(
            alt_typing(&registered, false, true, false, true),
            "and types ® into the editor"
        );
        assert!(
            !alt_typing(&registered, false, true, true, true),
            "claimed under the search bar, it toggles"
        );
        assert_eq!(
            option_on("\u{2030}", "r", KeyCode::KeyR, true, KeyOwner::Search),
            None,
            "US Mac: Option+Shift+R types a per mille sign"
        );
        assert_eq!(
            option_on("\u{ae}", "r", KeyCode::KeyO, false, KeyOwner::Search),
            Some(Command::Regex),
            "Dvorak Mac: the key printed R"
        );
        assert_eq!(
            option_on("\u{3c0}", "p", KeyCode::KeyR, false, KeyOwner::Search),
            None,
            "Dvorak Mac: the key printed P at the US R place types pi"
        );
    }

    /// The list and quote keys act on the editor's lines. Under the
    /// search bar, Go to line or a dialog's field they are no chord, so
    /// on a Mac Option types its character into the field: an en dash
    /// for Option+-, where the chord typed nothing before.
    #[test]
    fn under_a_field_the_option_list_keys_type_on_a_mac() {
        let keys = [
            ("\u{2013}", "-", KeyCode::Minus, Command::BulletList),
            ("\u{a1}", "1", KeyCode::Digit1, Command::NumberedList),
            ("\u{2248}", "x", KeyCode::KeyX, Command::TaskList),
            ("\u{2265}", ".", KeyCode::Period, Command::Quote),
        ];
        for (typed, keycap, code, cmd) in keys {
            assert_eq!(
                option_on(typed, keycap, code, false, KeyOwner::Text),
                Some(cmd),
                "{cmd:?} with the text"
            );
            for owner in [
                KeyOwner::Search,
                KeyOwner::GoTo,
                KeyOwner::Dialog { field: true },
            ] {
                assert_eq!(
                    option_on(typed, keycap, code, false, owner),
                    None,
                    "{cmd:?} under {owner:?}"
                );
            }
            assert!(
                alt_typing(&chr(typed), false, true, false, true),
                "{typed} is typed into the field"
            );
        }
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        assert_eq!(
            super::command_on(
                Press {
                    typed: &chr("-"),
                    keycap: &chr("-"),
                    code: none,
                    ctrl: false,
                    shift: false,
                    alt: true,
                },
                KeyOwner::Search,
                false,
            ),
            None,
            "Linux: Alt+minus under the search bar is nothing, as before"
        );
    }

    #[test]
    fn ascii_strings_and_the_empty_string_stand() {
        assert_eq!(
            latin_key(&chr("^q"), PhysicalKey::Code(KeyCode::KeyQ)),
            chr("^q")
        );
        assert_eq!(
            latin_key(&chr(""), PhysicalKey::Code(KeyCode::KeyA)),
            chr("")
        );
        assert_eq!(
            latin_key(&chr(","), PhysicalKey::Code(KeyCode::KeyM)),
            chr(",")
        );
    }

    #[test]
    fn a_field_takes_the_latin_key_with_one_modifier_held() {
        assert!(chord_held(true, false));
        assert!(chord_held(false, true));
        assert!(!chord_held(true, true), "an AltGr character on Windows");
        assert!(!chord_held(false, false), "a bare press types");
    }

    /// The Mac's table takes the same fallback with Cmd.
    #[test]
    fn on_the_mac_cmd_with_a_cyrillic_letter_copies() {
        assert_eq!(
            resolve_on(
                &chr("с"),
                PhysicalKey::Code(KeyCode::KeyC),
                true,
                false,
                false,
                true
            ),
            Some(Command::CopyText)
        );
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
            KeyCode::Slash => "/",
            KeyCode::Comma => ",",
            KeyCode::BracketLeft => "[",
            KeyCode::BracketRight => "]",
            other => panic!("{other:?} has no US character in this table"),
        };
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let mut seen = 0;
        for (binding, cmd) in SHORTCUTS.iter().flat_map(|row| row.bindings.iter()) {
            let (code, ctrl, alt, macos) = match (binding.on(false), binding.on(true)) {
                (Some(Binding::CtrlCode(code)), _) => (code, true, false, false),
                (Some(Binding::AltCode(code)), _) => (code, false, true, false),
                (None, Some(Binding::CtrlCode(code))) => (code, true, false, true),
                _ => continue,
            };
            seen += 1;
            let by_char = resolve_on(&chr(us(code)), none, ctrl, false, alt, macos);
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
        let alt = |key: &Key, code: PhysicalKey| resolve(key, code, false, false, true);
        assert_eq!(alt(&chr("-"), none), Some(Command::BulletList));
        assert_eq!(
            alt(&chr(")"), PhysicalKey::Code(KeyCode::Minus)),
            Some(Command::BulletList),
            "the key at the US minus position"
        );
        assert_eq!(alt(&chr("x"), none), Some(Command::TaskList));
        assert_eq!(
            resolve(&chr("X"), none, false, true, true),
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
            resolve(&chr(">"), none, false, true, true),
            Some(Command::Quote),
            "a layout with > on a plain key"
        );
        assert_eq!(
            alt(&chr(":"), PhysicalKey::Code(KeyCode::Period)),
            Some(Command::Quote),
            "AZERTY: the key at the US period place prints : unshifted"
        );
    }

    #[test]
    fn the_heading_chords_take_ctrl_with_a_digit_or_its_key() {
        assert_eq!(command(&chr("1"), true, false), Some(Command::Heading(1)));
        assert_eq!(command(&chr("6"), true, false), Some(Command::Heading(6)));
        assert_eq!(command(&chr("7"), true, false), None);
        assert_eq!(
            resolve(
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
            resolve(&up, none, false, false, true),
            Some(Command::MoveLineUp)
        );
        assert_eq!(
            resolve(&down, none, false, false, true),
            Some(Command::MoveLineDown)
        );
        assert_eq!(
            resolve(&up, none, false, false, false),
            Some(Command::LineUp)
        );
    }

    #[test]
    fn ctrl_shift_d_and_k_duplicate_and_delete_lines() {
        assert_eq!(
            command(&chr("D"), true, true),
            Some(Command::DuplicateLines)
        );
        assert_eq!(command(&chr("K"), true, true), Some(Command::DeleteLines));
        assert_eq!(
            command(&chr("d"), true, false),
            Some(Command::Direction),
            "plain Ctrl+D stays"
        );
        assert_eq!(command(&chr("k"), true, false), Some(Command::Link));
    }

    /// The file managers' chord for hidden files is Ctrl+H, which is
    /// find and replace here; the toggle takes Shift with it.
    #[test]
    fn ctrl_shift_h_toggles_hidden_files_and_plain_ctrl_h_replaces() {
        assert_eq!(command(&chr("H"), true, true), Some(Command::HiddenFiles));
        assert_eq!(command(&chr("h"), true, false), Some(Command::Replace));
    }

    #[test]
    fn ctrl_slash_toggles_a_comment_by_character_or_key() {
        assert_eq!(command(&chr("/"), true, false), Some(Command::Comment));
        assert_eq!(
            resolve(
                &chr("!"),
                PhysicalKey::Code(KeyCode::Slash),
                true,
                false,
                false
            ),
            Some(Command::Comment),
            "AZERTY: the key at the US slash place prints ! unshifted"
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
    fn on_the_mac_back_and_forward_are_cmd_brackets_and_option_arrows_are_free() {
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        let mac = |key: &Key, ctrl: bool, alt: bool| resolve_on(key, none, ctrl, false, alt, true);
        let elsewhere =
            |key: &Key, ctrl: bool, alt: bool| resolve_on(key, none, ctrl, false, alt, false);
        assert_eq!(mac(&chr("["), true, false), Some(Command::Back));
        assert_eq!(mac(&chr("]"), true, false), Some(Command::Forward));
        let left = Key::Named(NamedKey::ArrowLeft);
        let right = Key::Named(NamedKey::ArrowRight);
        assert_eq!(
            mac(&left, false, true),
            Some(Command::PaneLeft),
            "Option+Left is not back on the Mac, where every app moves by a word with it"
        );
        assert_eq!(mac(&right, false, true), Some(Command::PaneRight));
        assert_eq!(elsewhere(&left, false, true), Some(Command::Back));
        assert_eq!(elsewhere(&right, false, true), Some(Command::Forward));
        assert_eq!(elsewhere(&chr("["), true, false), None);
        assert_eq!(elsewhere(&chr("]"), true, false), None);
        assert_eq!(
            platform_label("Alt+Left / Alt+Right", true),
            "Cmd+[ / Cmd+]"
        );
        assert_eq!(
            platform_label("Alt+Left / Alt+Right", false),
            "Alt+Left / Alt+Right"
        );
        assert_eq!(
            platform_label("Alt+Up / Alt+Down", true),
            "Alt+Up / Alt+Down"
        );
    }

    #[test]
    fn alt_left_goes_back_and_plain_left_still_switches_panes() {
        // Off the Mac, whatever machine runs the test.
        let left = Key::Named(NamedKey::ArrowLeft);
        let none = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
        assert_eq!(
            resolve_on(&left, none, false, false, true, false),
            Some(Command::Back),
            "Alt+Left returns from a jump"
        );
        let right = Key::Named(NamedKey::ArrowRight);
        assert_eq!(
            resolve_on(&right, none, false, false, true, false),
            Some(Command::Forward),
            "Alt+Right goes forward again"
        );
        assert_eq!(
            resolve_on(&right, none, false, false, false, false),
            Some(Command::PaneRight),
            "plain Right keeps the pane switch"
        );
        assert_eq!(
            resolve(&left, none, false, false, false),
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

    /// The other way round, which keeps `ALL` whole: a command the table
    /// binds stands in the list. A new command is useless until a row
    /// binds it, and the row fails here until the list names it, so the
    /// tests that walk `ALL` never miss one.
    #[test]
    fn every_bound_command_stands_in_the_list() {
        let missing: Vec<String> = SHORTCUTS
            .iter()
            .flat_map(|row| row.bindings.iter().map(move |(_, cmd)| (row.keys, cmd)))
            .filter(|(_, cmd)| !Command::ALL.contains(cmd))
            .map(|(keys, cmd)| format!("{cmd:?} ({keys})"))
            .collect();
        assert!(
            missing.is_empty(),
            "bound by a row and missing from Command::ALL: {missing:?}"
        );
    }

    /// While a text field has the keyboard, the document's editing and
    /// selection keys fall silent; the app-wide chords stay live. The
    /// editor toggle is a mode switch, not an edit. The regex toggle
    /// acts on the search bar alone.
    #[test]
    fn under_a_field_the_document_keys_fall_silent() {
        for cmd in [
            Command::Cut,
            Command::Paste,
            Command::Undo,
            Command::Redo,
            Command::SelectAll,
            Command::CopyText,
            Command::CopyMarkdown,
            Command::Bold,
            Command::Heading(3),
            Command::MoveLineUp,
            Command::Comment,
            Command::DeleteLines,
        ] {
            assert!(cmd.live(KeyOwner::Text), "{cmd:?} acts on the text");
            assert!(
                !cmd.live(KeyOwner::Search),
                "{cmd:?} edits or selects in the document"
            );
        }
        for cmd in [
            Command::OpenFile,
            Command::Save,
            Command::NewNote,
            Command::Edit,
            Command::Sidebar,
            Command::ZoomIn,
            Command::Find,
            Command::Replace,
            Command::FindNext,
            Command::LineUp,
            Command::PageDown,
            Command::Back,
            Command::Settings,
            Command::Help,
            Command::Quit,
            Command::Export,
        ] {
            assert!(cmd.live(KeyOwner::Text), "{cmd:?} is app-wide");
            assert!(cmd.live(KeyOwner::Search), "{cmd:?} is app-wide");
        }
        let owners = [
            KeyOwner::Question,
            KeyOwner::Dialog { field: false },
            KeyOwner::Dialog { field: true },
            KeyOwner::GoTo,
            KeyOwner::Search,
            KeyOwner::Text,
        ];
        for row in SHORTCUTS {
            for (_, cmd) in row.bindings {
                for owner in owners {
                    let expected = match (cmd, owner) {
                        (Command::Regex, _) => owner == KeyOwner::Search,
                        (_, KeyOwner::Text) => true,
                        _ => match row.section {
                            "Edit" => *cmd == Command::Edit,
                            "Selection" => false,
                            _ => true,
                        },
                    };
                    assert_eq!(
                        cmd.live(owner),
                        expected,
                        "{cmd:?} in the {} section under {owner:?}",
                        row.section
                    );
                }
            }
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
        assert_eq!(command(&chr("V"), true, true), Some(Command::PasteFull));
    }

    #[test]
    fn the_save_family_resolves() {
        assert_eq!(command(&chr("s"), true, false), Some(Command::Save));
        assert_eq!(command(&chr("S"), true, true), Some(Command::SaveAs));
        assert_eq!(command(&chr("n"), true, false), Some(Command::NewFile));
        assert_eq!(command(&chr("m"), true, false), Some(Command::NewNote));
        assert_eq!(
            command(&chr("m"), false, false),
            None,
            "a plain m is typing"
        );
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
            resolve(
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
        assert_eq!(command(&chr("g"), true, false), Some(Command::GoToLine));
        assert_eq!(command(&chr("G"), true, true), Some(Command::GoToLine));
        assert!(
            Command::GoToLine.live(KeyOwner::Search),
            "Ctrl+G under the search field swaps the two bars"
        );
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

    #[test]
    fn ctrl_u_changes_the_case_and_is_bound_once() {
        assert_eq!(
            command(&Key::Character("u".into()), true, false),
            Some(Command::Case)
        );
        let rows = SHORTCUTS
            .iter()
            .filter(|row| {
                row.bindings
                    .iter()
                    .any(|(b, _)| matches!(b, Binding::Ctrl("u")))
            })
            .count();
        assert_eq!(rows, 1, "one row binds Ctrl+U");
    }
}
