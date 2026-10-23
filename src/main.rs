#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

/// Attaches stdout and stderr to the parent console so CLI output stays
/// visible when a windows-subsystem build is launched from a terminal.
/// Fails silently when no parent console exists or one is already attached.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Whether winit will open an X11 window: no Wayland display is named
/// and an X11 one is.
#[cfg(target_os = "linux")]
fn x11_session(
    wayland: Option<&std::ffi::OsStr>,
    socket: Option<&std::ffi::OsStr>,
    display: Option<&std::ffi::OsStr>,
) -> bool {
    // winit picks Wayland on WAYLAND_DISPLAY, else on WAYLAND_SOCKET (a
    // launcher passes one), and X11 on DISPLAY after both.
    let empty = |name: Option<&std::ffi::OsStr>| name.is_none_or(|name| name.is_empty());
    empty(wayland) && empty(socket) && display.is_some_and(|name| !name.is_empty())
}

/// On X11 winit loads libxkbcommon and libxkbcommon-x11 at run time and
/// panics when one is missing, as on the test machine of the AppImage
/// catalog. Loading them here first gives a plain line in its place.
#[cfg(target_os = "linux")]
fn missing_keyboard_library() -> Option<String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY");
    let socket = std::env::var_os("WAYLAND_SOCKET");
    let display = std::env::var_os("DISPLAY");
    if !x11_session(wayland.as_deref(), socket.as_deref(), display.as_deref()) {
        return None;
    }
    ["libxkbcommon.so.0", "libxkbcommon-x11.so.0"]
        .into_iter()
        // SAFETY: the system's own libraries, which winit loads with the
        // same call a moment later.
        .find(|name| unsafe { libloading::Library::new(name) }.is_err())
        .map(|name| {
            format!(
                "the keyboard library {name} is not installed, and X11 needs it. \
                 Install the package libxkbcommon-x11-0 (Debian, Ubuntu) \
                 or libxkbcommon-x11 (Fedora, Arch)."
            )
        })
}

/// What the positional argument asks for: a folder opens the sidebar
/// there over the welcome page; anything else, existing or not, goes
/// to the loader as a file, whose error names a missing one.
fn launch(path: Option<PathBuf>) -> app::Launch {
    match path {
        None => app::Launch::Empty,
        Some(p) if p.is_dir() => app::Launch::Folder(p),
        Some(p) => match oryx::ui::goto::split_arg(&p, |name| name.exists()) {
            (file, Some(_)) if file.is_dir() => app::Launch::Folder(file.to_path_buf()),
            (file, Some(target)) => app::Launch::FileAt(file.to_path_buf(), target),
            (_, None) => app::Launch::File(p),
        },
    }
}

/// Why `--recover` cannot start on `folder`, when it holds no note at
/// all: a wrong path gets a plain line in the terminal, as any other
/// wrong argument does, instead of a window that has nothing to show.
/// A folder another Oryx holds is the window's to report.
fn recover_refusal(folder: &std::path::Path) -> Option<String> {
    let note = folder.join(oryx::platform::notes::NOTE_NAME);
    (!note.is_file()).then(|| format!("no note to recover in {}", folder.display()))
}

/// The first line of the usage, which a refused command line repeats.
const USAGE_LINE: &str = "Usage: oryx [OPTIONS] [FILE | FOLDER | -]";

/// The text `--help` prints.
fn usage() -> String {
    format!(
        "oryx {}\n\n{USAGE_LINE}\n\n\
         Opens a markdown, code or text file, or a book (EPUB, FB2, MOBI,\n\
         AZW3, CBZ, CBR). A folder opens the sidebar on it. Without an\n\
         argument the window explains how to open a file. FILE:412 opens\n\
         the file at line 412, and FILE:412:10 at column 10 of that line.\n\
         Text piped in is shown too, as in `git diff | oryx`; a lone - asks\n\
         for standard input outright. Save it under a name to keep it.\n\n\
         Options:\n\
         \x20 --as KIND      show piped text as this kind: md, diff, json, rs\n\
         \x20 --theme NAME   start with the named theme\n\
         \x20 --register     install the file association and icons\n\
         \x20 --clear-cache  remove the downloaded remote images\n\
         \x20 --version      print the version\n\
         \x20 --help, -h     print this text\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// The `--version` line: the crate version, then the short commit the
/// build script read from git. A build from a source archive (the AUR
/// source package, a distribution's build) has no checkout and prints the
/// version alone.
fn version_line(commit: Option<&str>) -> String {
    match commit {
        Some(commit) => format!("oryx {} ({commit})", env!("CARGO_PKG_VERSION")),
        None => format!("oryx {}", env!("CARGO_PKG_VERSION")),
    }
}

/// `--clear-cache`: the downloaded remote images go, and the line
/// printed names the folder and the count so the user sees what went.
fn clear_cache() -> ExitCode {
    let Some(dir) = oryx::doc::fetch::cache_dir() else {
        eprintln!("oryx: no cache folder on this system");
        return ExitCode::FAILURE;
    };
    match oryx::doc::fetch::clear_cache(&dir) {
        Ok(count) => {
            println!("removed {count} cached images from {}", dir.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("oryx: cannot clear {}: {error}", dir.display());
            ExitCode::FAILURE
        }
    }
}

/// What the command line asks for, read from the arguments after the
/// program name. Only arguments starting with `--` are options, so a
/// file whose name starts that way still opens through `./--name`.
#[derive(Debug, PartialEq)]
enum Cli {
    Run {
        path: Option<PathBuf>,
        theme: Option<String>,
        /// Started by a running copy as a second window; private,
        /// absent from the usage. `beside` is where to open when the
        /// first window knew its own place, which Wayland never tells.
        second: bool,
        beside: Option<(i32, i32)>,
        /// The folder of a leftover note to take over, from a running
        /// copy that recovers it in a second window; private too.
        recover: Option<PathBuf>,
        piped: Piped,
    },
    Version,
    Register,
    ClearCache,
    Help,
    /// A refused command line and the message naming why.
    Refused(String),
}

/// What the command line says about text piped in.
#[derive(Debug, Default, PartialEq)]
struct Piped {
    /// A lone `-`: standard input is asked for outright.
    explicit: bool,
    /// `--as KIND`: the kind the text is shown as, `md` or `diff`.
    kind: Option<String>,
}

/// The message for a refused `--as`.
const AS_REFUSAL: &str = "--as takes a kind, such as md, diff or json";

/// Whether standard input is read before the window opens. A lone `-`
/// asks for it. Without one it is read when no file is named and the
/// input is not a terminal: `git diff | oryx`. A second window of a
/// running Oryx never reads it, since it inherits the first one's
/// input. An empty input, a desktop launcher's, reads as no text.
fn reads_stdin(piped: &Piped, named: bool, terminal: bool, second_window: bool) -> bool {
    !second_window && (piped.explicit || (!named && !terminal))
}

/// Standard input read to its end, before the window opens: a slow
/// producer delays the window by as long as it runs. None for an empty
/// input and for one that cannot be read, which open the welcome page
/// as a launch with no file does.
fn piped_bytes() -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().lock().read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Cli {
    let mut path: Option<PathBuf> = None;
    let mut theme: Option<String> = None;
    let mut second = false;
    let mut beside: Option<(i32, i32)> = None;
    let mut recover: Option<PathBuf> = None;
    let mut piped = Piped::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--version") => return Cli::Version,
            Some("--register") => return Cli::Register,
            Some("--clear-cache") => return Cli::ClearCache,
            Some("--help") | Some("-h") => return Cli::Help,
            Some("--theme") => match args.next().and_then(|name| name.into_string().ok()) {
                Some(name) => theme = Some(name),
                None => return Cli::Refused("--theme takes a theme name".to_string()),
            },
            Some("--beside") => {
                let value = args.next().and_then(|value| value.into_string().ok());
                let position = value.as_deref().and_then(|value| {
                    let (x, y) = value.split_once(',')?;
                    Some((x.parse().ok()?, y.parse().ok()?))
                });
                match (value.as_deref(), position) {
                    (_, Some(at)) => beside = Some(at),
                    (Some("none"), None) => {}
                    _ => return Cli::Refused("--beside takes a position as X,Y".to_string()),
                }
                second = true;
            }
            Some("--as") => match args.next().and_then(|kind| kind.into_string().ok()) {
                Some(kind) if !kind.starts_with('-') && oryx::doc::load::kind_is_plain(&kind) => {
                    piped.kind = Some(kind);
                }
                _ => return Cli::Refused(AS_REFUSAL.to_string()),
            },
            Some("-") => piped.explicit = true,
            Some("--recover") => match args.next() {
                Some(folder) => recover = Some(PathBuf::from(folder)),
                None => return Cli::Refused("--recover takes a folder".to_string()),
            },
            Some(flag) if flag.starts_with("--") => {
                return Cli::Refused(format!("unknown option {flag}"));
            }
            _ => path = Some(PathBuf::from(&arg)),
        }
    }
    // A lone - and --as are about the piped text; beside a file name
    // one of the two would be dropped without a word.
    if path.is_some() && piped.explicit {
        return Cli::Refused(
            "a lone - reads standard input and takes no file beside it".to_string(),
        );
    }
    if path.is_some() && piped.kind.is_some() {
        return Cli::Refused("--as goes with piped text, not with a file".to_string());
    }
    Cli::Run {
        path,
        theme,
        second,
        beside,
        recover,
        piped,
    }
}

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_parent_console();
    match parse_args(std::env::args_os().skip(1)) {
        Cli::Version => {
            println!("{}", version_line(option_env!("ORYX_COMMIT")));
            ExitCode::SUCCESS
        }
        Cli::Register => match oryx::platform::register::register() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("oryx: register failed: {error}");
                ExitCode::FAILURE
            }
        },
        Cli::ClearCache => clear_cache(),
        Cli::Help => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        Cli::Refused(message) => {
            eprintln!("oryx: {message}\n{USAGE_LINE}\nTry 'oryx --help' for the options.");
            ExitCode::FAILURE
        }
        Cli::Run {
            path,
            theme,
            second,
            beside,
            recover,
            piped,
        } => {
            if let Some(message) = recover.as_deref().and_then(recover_refusal) {
                eprintln!("oryx: {message}");
                return ExitCode::FAILURE;
            }
            #[cfg(target_os = "linux")]
            if let Some(message) = missing_keyboard_library() {
                eprintln!("oryx: {message}");
                return ExitCode::FAILURE;
            }
            use std::io::IsTerminal;
            let second_window = second || recover.is_some();
            let terminal = std::io::stdin().is_terminal();
            let text = reads_stdin(&piped, path.is_some(), terminal, second_window)
                .then(piped_bytes)
                .flatten();
            let launch = match (recover, text) {
                (Some(folder), _) => app::Launch::Recover(folder),
                (None, Some(bytes)) => app::Launch::Piped(bytes, piped.kind),
                (None, None) => launch(path),
            };
            run(launch, theme, second, beside)
        }
    }
}

/// The window's whole life; an error that ends it is named in the
/// terminal.
fn run(
    launch: app::Launch,
    theme: Option<String>,
    second: bool,
    beside: Option<(i32, i32)>,
) -> ExitCode {
    match app::run(launch, theme, second, beside) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oryx: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn the_keyboard_check_runs_when_winit_would_pick_x11() {
        let wayland = Some(std::ffi::OsStr::new("wayland-0"));
        let display = Some(std::ffi::OsStr::new(":0"));
        let empty = Some(std::ffi::OsStr::new(""));
        let socket = Some(std::ffi::OsStr::new("3"));
        assert!(x11_session(None, None, display));
        assert!(x11_session(empty, None, display));
        assert!(x11_session(None, empty, display));
        assert!(!x11_session(wayland, None, display));
        assert!(
            !x11_session(None, socket, display),
            "a socket passed by a launcher is Wayland too"
        );
        assert!(!x11_session(None, None, None));
        assert!(!x11_session(None, None, empty));
    }

    #[test]
    fn the_argument_decides_between_nothing_a_file_and_a_folder() {
        let dir = std::env::temp_dir().join(format!("oryx-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, "# notes\n").unwrap();
        let missing = dir.join("absent.md");
        assert_eq!(launch(None), app::Launch::Empty);
        assert_eq!(launch(Some(file.clone())), app::Launch::File(file.clone()));
        assert_eq!(launch(Some(dir.clone())), app::Launch::Folder(dir.clone()));
        assert_eq!(
            launch(Some(missing.clone())),
            app::Launch::File(missing),
            "a missing path goes to the loader, whose error names it"
        );
        let with = |tail: &str| {
            let mut name = file.clone().into_os_string();
            name.push(tail);
            PathBuf::from(name)
        };
        let at = |line, column| oryx::ui::goto::Target { line, column };
        assert_eq!(
            launch(Some(with(":412"))),
            app::Launch::FileAt(file.clone(), at(412, None))
        );
        assert_eq!(
            launch(Some(with(":412:10"))),
            app::Launch::FileAt(file.clone(), at(412, Some(10))),
            "the form a compiler prints"
        );
        let odd = dir.join("notes:7");
        std::fs::write(&odd, "text\n").unwrap();
        assert_eq!(
            launch(Some(odd.clone())),
            app::Launch::File(odd),
            "a file really named so opens as typed"
        );
        let mut folder = dir.clone().into_os_string();
        folder.push(":3");
        assert_eq!(
            launch(Some(PathBuf::from(folder))),
            app::Launch::Folder(dir.clone()),
            "a line means nothing on a folder"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dash_or_a_kind_beside_a_file_is_refused() {
        for line in [
            &["notes.md", "-"][..],
            &["-", "notes.md"],
            &["--as", "md", "notes.md"],
        ] {
            assert!(
                matches!(parse_args(args(line)), Cli::Refused(_)),
                "{line:?}: the file or the piped text would be dropped without a word"
            );
        }
    }

    #[test]
    fn as_refuses_an_option_as_its_kind() {
        assert_eq!(
            parse_args(args(&["--as", "--theme", "dracula"])),
            Cli::Refused(AS_REFUSAL.to_string())
        );
        assert_eq!(
            parse_args(args(&["--as", "-"])),
            Cli::Refused(AS_REFUSAL.to_string())
        );
    }

    #[test]
    fn beside_none_marks_a_second_window_with_no_place() {
        assert_eq!(
            parse_args(args(&["--beside", "none", "notes.md"])),
            Cli::Run {
                path: Some(PathBuf::from("notes.md")),
                theme: None,
                second: true,
                beside: None,
                recover: None,
                piped: Piped::default(),
            },
            "Wayland tells no window where it stands, and the copy is a second window still"
        );
    }

    fn args(list: &[&str]) -> impl Iterator<Item = OsString> {
        list.iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_version_line_names_the_commit_when_the_build_knows_it() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            version_line(Some("abc1234")),
            format!("oryx {version} (abc1234)")
        );
        assert_eq!(version_line(None), format!("oryx {version}"));
    }

    #[test]
    fn the_usage_names_the_flags_and_both_argument_kinds() {
        let text = usage();
        for flag in [
            "--theme",
            "--register",
            "--clear-cache",
            "--version",
            "--help",
            "-h",
        ] {
            assert!(text.contains(flag), "the usage names {flag}");
        }
        assert!(text.starts_with(&format!("oryx {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains(USAGE_LINE));
        assert!(text.contains("FILE") && text.contains("FOLDER"));
    }

    #[test]
    fn the_arguments_parse_to_one_request() {
        assert_eq!(parse_args(args(&["--help"])), Cli::Help);
        assert_eq!(parse_args(args(&["-h"])), Cli::Help);
        assert_eq!(parse_args(args(&["--version"])), Cli::Version);
        assert_eq!(parse_args(args(&["--register"])), Cli::Register);
        assert_eq!(parse_args(args(&["--clear-cache"])), Cli::ClearCache);
        assert_eq!(
            parse_args(args(&["--theme", "dracula", "notes.md"])),
            Cli::Run {
                path: Some(PathBuf::from("notes.md")),
                theme: Some("dracula".to_string()),
                second: false,
                beside: None,
                recover: None,
                piped: Piped::default(),
            }
        );
        assert_eq!(
            parse_args(args(&[])),
            Cli::Run {
                path: None,
                theme: None,
                second: false,
                beside: None,
                recover: None,
                piped: Piped::default(),
            }
        );
        assert_eq!(
            parse_args(args(&["./--odd.md"])),
            Cli::Run {
                path: Some(PathBuf::from("./--odd.md")),
                theme: None,
                second: false,
                beside: None,
                recover: None,
                piped: Piped::default(),
            },
            "a path form opens a file whose name starts with dashes"
        );
    }

    /// The private flag a running copy passes to the second window it
    /// opens: where to place it, a step down and right of itself.
    #[test]
    fn recovering_from_a_folder_without_a_note_is_refused_in_words() {
        let dir = std::env::temp_dir().join(format!("oryx-recover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            recover_refusal(&dir),
            Some(format!("no note to recover in {}", dir.display()))
        );
        assert_eq!(
            recover_refusal(std::path::Path::new("this")),
            Some("no note to recover in this".to_string())
        );
        std::fs::write(dir.join("untitled.md"), "kept").unwrap();
        assert_eq!(recover_refusal(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_recover_flag_carries_a_folder() {
        assert_eq!(
            parse_args(args(&[
                "--beside",
                "40,60",
                "--recover",
                "/state/notes/7-1"
            ])),
            Cli::Run {
                path: None,
                theme: None,
                second: true,
                beside: Some((40, 60)),
                recover: Some(PathBuf::from("/state/notes/7-1")),
                piped: Piped::default(),
            }
        );
        assert_eq!(
            parse_args(args(&["--recover"])),
            Cli::Refused("--recover takes a folder".to_string())
        );
    }

    #[test]
    fn the_beside_flag_carries_a_position() {
        assert_eq!(
            parse_args(args(&["--beside", "40,60", "notes.md"])),
            Cli::Run {
                path: Some(PathBuf::from("notes.md")),
                theme: None,
                second: true,
                beside: Some((40, 60)),
                recover: None,
                piped: Piped::default(),
            }
        );
        assert_eq!(
            parse_args(args(&["--beside", "-10,7"])),
            Cli::Run {
                path: None,
                theme: None,
                second: true,
                beside: Some((-10, 7)),
                recover: None,
                piped: Piped::default(),
            },
            "a monitor left of the main one has negative x"
        );
        assert_eq!(
            parse_args(args(&["--beside", "x"])),
            Cli::Refused("--beside takes a position as X,Y".to_string())
        );
        assert_eq!(
            parse_args(args(&["--beside"])),
            Cli::Refused("--beside takes a position as X,Y".to_string())
        );
    }

    #[test]
    fn an_unknown_option_and_a_bare_theme_are_refused_by_name() {
        assert_eq!(
            parse_args(args(&["--nope"])),
            Cli::Refused("unknown option --nope".to_string())
        );
        assert_eq!(
            parse_args(args(&["--theme"])),
            Cli::Refused("--theme takes a theme name".to_string())
        );
    }

    #[test]
    fn a_dash_asks_for_standard_input_and_as_names_the_kind() {
        let run = |piped: Piped| Cli::Run {
            path: None,
            theme: None,
            second: false,
            beside: None,
            recover: None,
            piped,
        };
        assert_eq!(
            parse_args(args(&["-"])),
            run(Piped {
                explicit: true,
                kind: None
            })
        );
        assert_eq!(
            parse_args(args(&["--as", "md"])),
            run(Piped {
                explicit: false,
                kind: Some("md".to_string())
            })
        );
        assert_eq!(
            parse_args(args(&["-", "--as", "diff"])),
            run(Piped {
                explicit: true,
                kind: Some("diff".to_string())
            })
        );
        assert_eq!(
            parse_args(args(&["--as"])),
            Cli::Refused(AS_REFUSAL.to_string())
        );
        assert_eq!(
            parse_args(args(&["--as", "../notes"])),
            Cli::Refused(AS_REFUSAL.to_string())
        );
    }

    #[test]
    fn standard_input_is_read_only_when_text_can_come_from_it() {
        let plain = Piped::default();
        let dash = Piped {
            explicit: true,
            kind: None,
        };
        assert!(reads_stdin(&plain, false, false, false), "git diff | oryx");
        assert!(
            !reads_stdin(&plain, false, true, false),
            "oryx, typed in a terminal"
        );
        assert!(
            !reads_stdin(&plain, true, false, false),
            "git diff | oryx notes.md"
        );
        assert!(!reads_stdin(&plain, false, false, true), "a second window");
        assert!(
            reads_stdin(&dash, false, true, false),
            "oryx -, then typed text"
        );
        assert!(!reads_stdin(&dash, false, false, true));
    }
}
