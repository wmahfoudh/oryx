mod app;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
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
