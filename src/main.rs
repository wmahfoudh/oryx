mod app;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(arg) if arg == "--version" => {
            println!("oryx {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        path => match app::run(path.map(PathBuf::from)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("oryx: {error}");
                ExitCode::FAILURE
            }
        },
    }
}
