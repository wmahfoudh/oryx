#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

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

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_parent_console();
    let mut path: Option<PathBuf> = None;
    let mut theme: Option<String> = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--version" {
            println!("oryx {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        if arg == "--register" {
            return match oryx::platform::register::register() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("oryx: register failed: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        if arg == "--theme" {
            match args.next().and_then(|name| name.into_string().ok()) {
                Some(name) => theme = Some(name),
                None => {
                    eprintln!("oryx: --theme takes a theme name");
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        path = Some(PathBuf::from(arg));
    }
    match app::run(launch(path), theme) {
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
}
