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
    match app::run(path, theme) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oryx: {error}");
            ExitCode::FAILURE
        }
    }
}
