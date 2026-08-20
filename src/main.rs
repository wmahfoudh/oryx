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

/// What the positional argument asks for: a folder opens the sidebar
/// there over the welcome page; anything else, existing or not, goes
/// to the loader as a file, whose error names a missing one.
fn launch(path: Option<PathBuf>) -> app::Launch {
    match path {
        None => app::Launch::Empty,
        Some(p) if p.is_dir() => app::Launch::Folder(p),
        Some(p) => app::Launch::File(p),
    }
}

/// The first line of the usage, which a refused command line repeats.
const USAGE_LINE: &str = "Usage: oryx [OPTIONS] [FILE | FOLDER]";

/// The text `--help` prints.
fn usage() -> String {
    format!(
        "oryx {}\n\n{USAGE_LINE}\n\n\
         Opens a markdown, code or text file, or a book (EPUB, FB2, MOBI,\n\
         AZW3, CBZ, CBR). A folder opens the sidebar on it. Without an\n\
         argument the window explains how to open a file.\n\n\
         Options:\n\
         \x20 --theme NAME   start with the named theme\n\
         \x20 --register     install the file association and icons\n\
         \x20 --version      print the version\n\
         \x20 --help, -h     print this text\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// What the command line asks for, read from the arguments after the
/// program name. Only arguments starting with `--` are options, so a
/// file whose name starts that way still opens through `./--name`.
#[derive(Debug, PartialEq)]
enum Cli {
    Run {
        path: Option<PathBuf>,
        theme: Option<String>,
    },
    Version,
    Register,
    Help,
    /// A refused command line and the message naming why.
    Refused(String),
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Cli {
    let mut path: Option<PathBuf> = None;
    let mut theme: Option<String> = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--version") => return Cli::Version,
            Some("--register") => return Cli::Register,
            Some("--help") | Some("-h") => return Cli::Help,
            Some("--theme") => match args.next().and_then(|name| name.into_string().ok()) {
                Some(name) => theme = Some(name),
                None => return Cli::Refused("--theme takes a theme name".to_string()),
            },
            Some(flag) if flag.starts_with("--") => {
                return Cli::Refused(format!("unknown option {flag}"));
            }
            _ => path = Some(PathBuf::from(&arg)),
        }
    }
    Cli::Run { path, theme }
}

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_parent_console();
    match parse_args(std::env::args_os().skip(1)) {
        Cli::Version => {
            println!("oryx {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Cli::Register => match oryx::platform::register::register() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("oryx: register failed: {error}");
                ExitCode::FAILURE
            }
        },
        Cli::Help => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        Cli::Refused(message) => {
            eprintln!("oryx: {message}\n{USAGE_LINE}\nTry 'oryx --help' for the options.");
            ExitCode::FAILURE
        }
        Cli::Run { path, theme } => match app::run(launch(path), theme) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("oryx: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_argument_decides_between_nothing_a_file_and_a_folder() {
        let dir = std::env::temp_dir().join(format!("oryx-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.md");
        std::fs::write(&file, "# notes\n").unwrap();
        let missing = dir.join("absent.md");
        assert_eq!(launch(None), app::Launch::Empty);
        assert_eq!(launch(Some(file.clone())), app::Launch::File(file));
        assert_eq!(launch(Some(dir.clone())), app::Launch::Folder(dir.clone()));
        assert_eq!(
            launch(Some(missing.clone())),
            app::Launch::File(missing),
            "a missing path goes to the loader, whose error names it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn args(list: &[&str]) -> impl Iterator<Item = OsString> {
        list.iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_usage_names_the_flags_and_both_argument_kinds() {
        let text = usage();
        for flag in ["--theme", "--register", "--version", "--help", "-h"] {
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
        assert_eq!(
            parse_args(args(&["--theme", "dracula", "notes.md"])),
            Cli::Run {
                path: Some(PathBuf::from("notes.md")),
                theme: Some("dracula".to_string()),
            }
        );
        assert_eq!(
            parse_args(args(&[])),
            Cli::Run {
                path: None,
                theme: None
            }
        );
        assert_eq!(
            parse_args(args(&["./--odd.md"])),
            Cli::Run {
                path: Some(PathBuf::from("./--odd.md")),
                theme: None,
            },
            "a path form opens a file whose name starts with dashes"
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
}
