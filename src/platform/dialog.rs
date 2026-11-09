//! The file dialogs: the system's, through the `rfd` library, with the
//! answer read so that a dialog that could not appear is told apart
//! from one the reader cancelled. On Linux the library asks the desktop
//! portal and falls back to the `zenity` program; a system with neither,
//! a window manager without a desktop (GitHub issue 12), gets an empty
//! answer, the same as a cancel. The library reports its failures
//! through the `log` crate as error records and a cancel as none, and
//! it tries zenity after every portal failure, so no dialog appeared
//! exactly when zenity could not run: a small logger keeps the last
//! error record of the library, and an empty answer whose last error is
//! zenity's means the dialog could not appear. A portal error alone
//! means zenity ran and was dismissed, a cancel.

use std::path::PathBuf;
use std::sync::Mutex;

/// The last error record of the dialog library, kept until read.
static LAST_FAILURE: Mutex<Option<String>> = Mutex::new(None);

/// The logger: the error records of the dialog library are kept, one at
/// a time; every other record is dropped.
struct DialogLog;

impl log::Log for DialogLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Error && metadata.target().starts_with("rfd")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            if let Ok(mut last) = LAST_FAILURE.lock() {
                *last = Some(record.args().to_string());
            }
        }
    }

    fn flush(&self) {}
}

/// Installs the logger, once per process; a second call does nothing.
/// Errors are the only records let through, so the libraries' warnings
/// cost nothing.
pub fn install() {
    if log::set_logger(&DialogLog).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }
}

/// The failure the library reported since the last call, if any.
fn take_failure() -> Option<String> {
    LAST_FAILURE.lock().ok().and_then(|mut last| last.take())
}

/// What a dialog answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The path chosen.
    Path(PathBuf),
    /// The dialog appeared and was dismissed.
    Cancel,
    /// The dialog could not appear; the library's own words say why.
    NoDialog(String),
}

/// Reads an empty answer against the last error the library logged
/// during the call: zenity failing to run means no dialog appeared, any
/// other error came before a dialog that did appear (the portal's, with
/// zenity run after it; a bus warning before a real dialog), and none
/// means a cancel. A path is a path whatever was logged.
pub fn classify(answer: Option<PathBuf>, failure: Option<String>) -> Answer {
    match (answer, failure) {
        (Some(path), _) => Answer::Path(path),
        (None, Some(why)) if why.contains("zenity") => Answer::NoDialog(why),
        (None, _) => Answer::Cancel,
    }
}

/// The open dialog, its answer read.
pub fn pick_file(dialog: rfd::FileDialog) -> Answer {
    let _ = take_failure();
    let answer = dialog.pick_file();
    classify(answer, take_failure())
}

/// The save dialog, its answer read.
pub fn save_file(dialog: rfd::FileDialog) -> Answer {
    let _ = take_failure();
    let answer = dialog.save_file();
    classify(answer, take_failure())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_logger_keeps_the_last_error_of_the_dialog_library_and_nothing_else() {
        install();
        install();
        assert_eq!(take_failure(), None);
        log::error!(target: "rfd::backend::xdg_desktop_portal", "Failed to connect to session bus: no address");
        assert_eq!(
            take_failure().as_deref(),
            Some("Failed to connect to session bus: no address")
        );
        assert_eq!(take_failure(), None, "taken once");
        log::warn!(target: "rfd::backend::xdg_desktop_portal", "Using zenity fallback");
        log::error!(target: "winit::platform", "another library's error");
        assert_eq!(
            take_failure(),
            None,
            "a warning or another library leaves nothing"
        );
        log::error!(target: "rfd::backend::xdg_desktop_portal", "OpenFile failed: a");
        log::error!(target: "rfd::backend::xdg_desktop_portal", "Failed to pick file with zenity: b");
        assert_eq!(
            take_failure().as_deref(),
            Some("Failed to pick file with zenity: b"),
            "the last record"
        );
    }

    #[test]
    fn an_empty_answer_is_a_cancel_unless_zenity_failed_to_run() {
        let path = PathBuf::from("/tmp/a.md");
        assert!(matches!(classify(Some(path.clone()), None), Answer::Path(p) if p == path));
        assert!(
            matches!(
                classify(
                    Some(path.clone()),
                    Some("Failed to pick file with zenity: x".into())
                ),
                Answer::Path(_)
            ),
            "a path answers whatever was logged"
        );
        assert!(matches!(classify(None, None), Answer::Cancel));
        let zenity = "Failed to save file with zenity: No such file or directory (os error 2)";
        assert!(
            matches!(classify(None, Some(zenity.into())), Answer::NoDialog(why) if why == zenity)
        );
        // The portal failed and the library went on to zenity, which ran
        // and was dismissed: the last error is the portal's, and the
        // answer is a cancel.
        assert!(
            matches!(
                classify(
                    None,
                    Some("Failed to connect to session bus: no address".into())
                ),
                Answer::Cancel
            ),
            "a portal failure alone means zenity ran"
        );
        assert!(
            matches!(
                classify(None, Some("Failed to add match rule: x".into())),
                Answer::Cancel
            ),
            "a dialog that appeared and was cancelled after a bus warning"
        );
    }
}
