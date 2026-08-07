//! Persistent user configuration: `config.toml` in the user's config
//! directory. Any read problem yields defaults, never an error.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::export::ExportSettings;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    /// Where the reader was last looking: the folder of the last opened
    /// file, or the folder the sidebar was last rooted at, whichever came
    /// last. Seeds both the sidebar and the open dialog on the next run.
    /// Empty means never set.
    pub last_dir: String,
    /// Whether the folder sidebar was open at the last toggle, and how
    /// wide it was left. Both are written when a gesture ends, not per
    /// frame, so a drag does not hammer the disk.
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    /// The panel tab last active, so the sidebar reopens where it was.
    pub sidebar_tab: crate::ui::sidebar::Tab,
    /// Whether book text justifies; Ctrl+J flips it, EPUB display only.
    pub justify: bool,
    /// How the reader last exported; None until the first export, which
    /// seeds it from the fields above. A table, so it follows the plain
    /// values and precedes the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportSettings>,
    /// Window geometry from the last clean exit; None until the first
    /// one. Must stay the last field so its table serializes after the
    /// plain values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// Saved window geometry in physical pixels. The position is absent on
/// Wayland, where the compositor owns placement. While the window is
/// maximized the fields keep the floating geometry underneath, so
/// unmaximizing after a restart lands where the user left it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    pub maximized: bool,
}

impl WindowState {
    /// The saved position, kept only while the window rectangle still
    /// overlaps one of the monitor rectangles (x, y, width, height), so a
    /// window last seen on an unplugged monitor falls back to the
    /// platform's own placement.
    pub fn position_on(&self, monitors: &[(i32, i32, u32, u32)]) -> Option<(i32, i32)> {
        let (x, y) = (self.x?, self.y?);
        let visible = monitors.iter().any(|&(mx, my, mw, mh)| {
            x < mx + mw as i32
                && x + self.width as i32 > mx
                && y < my + mh as i32
                && y + self.height as i32 > my
        });
        visible.then_some((x, y))
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: "dracula".to_string(),
            body_family: BODY_FAMILY.to_string(),
            code_family: CODE_FAMILY.to_string(),
            body_size: 22.0,
            code_size: 20.0,
            last_dir: String::new(),
            sidebar_open: false,
            sidebar_width: crate::ui::sidebar::DEFAULT_WIDTH,
            sidebar_tab: crate::ui::sidebar::Tab::Files,
            justify: true,
            export: None,
            window: None,
        }
    }
}

/// Where to start browsing: the first candidate that is still a directory,
/// falling back to the working directory. A folder that has been deleted
/// or unmounted since the last run drops through to the next candidate
/// rather than opening an empty tree.
pub fn browse_dir(candidates: impl IntoIterator<Item = Option<PathBuf>>) -> PathBuf {
    candidates
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Location of the config file, None when the platform gives no home.
pub fn path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "oryx").map(|dirs| dirs.config_dir().join("config.toml"))
}

/// Location of the reading-position file, beside the config.
pub fn positions_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "oryx").map(|dirs| dirs.config_dir().join("positions.toml"))
}

/// How many books keep a remembered position.
const POSITIONS_KEPT: usize = 100;

/// Where reading stopped, per book, most recent last. Keyed by the
/// book's `dc:identifier` with the canonical path as fallback, holding
/// the source offset of the line at the top of the viewport.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Positions {
    pub book: Vec<BookPosition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BookPosition {
    pub key: String,
    pub offset: usize,
}

impl Positions {
    pub fn load() -> Positions {
        positions_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Positions {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = positions_path() {
            self.save_to(&p);
        }
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = toml::to_string(self).expect("positions serialize");
        if let Err(err) = std::fs::write(path, text) {
            eprintln!("oryx: cannot save positions {}: {err}", path.display());
        }
    }

    /// Files a book's position, most recent last, the oldest pruned
    /// past the cap.
    pub fn remember(&mut self, key: &str, offset: usize) {
        self.book.retain(|b| b.key != key);
        self.book.push(BookPosition {
            key: key.to_string(),
            offset,
        });
        if self.book.len() > POSITIONS_KEPT {
            let excess = self.book.len() - POSITIONS_KEPT;
            self.book.drain(..excess);
        }
    }

    pub fn lookup(&self, key: &str) -> Option<usize> {
        self.book
            .iter()
            .rev()
            .find(|b| b.key == key)
            .map(|b| b.offset)
    }
}

pub fn load() -> Config {
    path().map(|p| load_from(&p)).unwrap_or_default()
}

pub fn load_from(path: &Path) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(config: &Config) {
    if let Some(p) = path() {
        save_to(&p, config);
    }
}

pub fn save_to(path: &Path, config: &Config) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = toml::to_string(config).expect("config serializes");
    if let Err(err) = std::fs::write(path, text) {
        eprintln!("oryx: cannot save config {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Orientation, PageSize};

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oryx-config-{}-{name}", std::process::id()))
    }

    #[test]
    fn justify_defaults_on() {
        assert!(
            Config::default().justify,
            "book text justifies out of the box"
        );
    }

    #[test]
    fn browse_dir_takes_the_first_folder_that_still_exists() {
        let real = std::env::temp_dir();
        let gone = real.join("oryx-no-such-folder-ever");
        assert_eq!(
            browse_dir([Some(gone.clone()), Some(real.clone())]),
            real,
            "a folder that vanished drops through"
        );
        assert_eq!(browse_dir([Some(real.clone()), Some(gone.clone())]), real);
        assert_eq!(
            browse_dir([None, Some(real.clone())]),
            real,
            "an absent candidate is skipped"
        );
    }

    #[test]
    fn browse_dir_falls_back_to_the_working_directory() {
        let gone = std::env::temp_dir().join("oryx-no-such-folder-ever");
        assert_eq!(browse_dir([None, None]), PathBuf::from("."));
        assert_eq!(browse_dir([Some(gone)]), PathBuf::from("."));
    }

    #[test]
    fn browse_dir_refuses_a_file_as_a_starting_folder() {
        let file = std::env::temp_dir().join(format!("oryx-browse-{}", std::process::id()));
        std::fs::write(&file, "x").unwrap();
        let real = std::env::temp_dir();
        assert_eq!(browse_dir([Some(file.clone()), Some(real.clone())]), real);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn config_round_trips() {
        let path = temp_path("round.toml");
        let config = Config {
            theme: "dracula".to_string(),
            body_family: "Test Sans".to_string(),
            code_family: "Test Mono".to_string(),
            body_size: 18.0,
            code_size: 16.0,
            last_dir: "/home/user/notes".to_string(),
            sidebar_open: true,
            sidebar_width: 320.0,
            sidebar_tab: crate::ui::sidebar::Tab::Outline,
            justify: false,
            export: None,
            window: None,
        };
        save_to(&path, &config);
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn a_config_written_before_the_sidebar_fields_defaults_them() {
        let path = temp_path("older.toml");
        std::fs::write(
            &path,
            "theme = \"nord\"\nbody_size = 20.0\nlast_dir = \"/tmp\"\n",
        )
        .unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.theme, "nord");
        assert!(!loaded.sidebar_open, "closed until the reader opens it");
        assert_eq!(loaded.sidebar_width, crate::ui::sidebar::DEFAULT_WIDTH);
        assert_eq!(
            loaded.sidebar_tab,
            crate::ui::sidebar::Tab::Files,
            "a config predating the tab field defaults to Files"
        );
    }

    #[test]
    fn export_settings_seed_from_the_appearance_settings() {
        let config = Config {
            theme: "nord".to_string(),
            body_size: 22.0,
            ..Config::default()
        };
        let export = ExportSettings::seeded_from(&config);
        assert_eq!(export.theme, "nord");
        assert_eq!(export.body_size, 22.0);
        assert_eq!(export.code_family, config.code_family);
        assert_eq!(export.page, PageSize::A4);
        assert_eq!(export.orientation, Orientation::Portrait);
        assert!(export.page_numbers, "on until the reader turns them off");
    }

    #[test]
    fn an_export_table_without_orientation_reads_portrait() {
        let path = temp_path("no-orientation.toml");
        std::fs::write(&path, "[export]\ntheme = \"nord\"\n").unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        let export = loaded.export.expect("the table is read");
        assert_eq!(export.orientation, Orientation::Portrait);
    }

    #[test]
    fn a_config_written_before_the_export_table_defaults_it() {
        let path = temp_path("no-export.toml");
        std::fs::write(&path, "theme = \"nord\"\n").unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.export, None);
    }

    #[test]
    fn export_table_round_trips_and_stays_before_the_window_table() {
        let path = temp_path("export.toml");
        let config = Config {
            export: Some(ExportSettings {
                theme: "oryx-light".to_string(),
                body_family: "DejaVu Sans".to_string(),
                code_family: "Courier Prime".to_string(),
                body_size: 11.0,
                code_size: 9.0,
                page: PageSize::Letter,
                orientation: Orientation::Landscape,
                page_numbers: false,
            }),
            window: Some(WindowState {
                width: 1280,
                height: 800,
                x: None,
                y: None,
                maximized: false,
            }),
            ..Config::default()
        };
        save_to(&path, &config);
        let text = std::fs::read_to_string(&path).unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
        let export = text.find("[export]").expect("the export table is written");
        let window = text.find("[window]").expect("the window table is written");
        assert!(export < window, "window stays the last table");
    }

    #[test]
    fn window_state_round_trips() {
        let path = temp_path("window.toml");
        let config = Config {
            window: Some(WindowState {
                width: 1280,
                height: 800,
                x: Some(64),
                y: Some(-32),
                maximized: true,
            }),
            ..Config::default()
        };
        save_to(&path, &config);
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn window_state_without_position_round_trips() {
        let path = temp_path("window-nopos.toml");
        let config = Config {
            window: Some(WindowState {
                width: 1024,
                height: 768,
                x: None,
                y: None,
                maximized: false,
            }),
            ..Config::default()
        };
        save_to(&path, &config);
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn absent_window_table_gives_none() {
        let path = temp_path("no-window.toml");
        std::fs::write(&path, "theme = \"nord\"\n").unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.window, None);
    }

    fn saved(x: i32, y: i32) -> WindowState {
        WindowState {
            width: 800,
            height: 600,
            x: Some(x),
            y: Some(y),
            maximized: false,
        }
    }

    #[test]
    fn position_kept_when_window_touches_a_monitor() {
        let monitors = [(0, 0, 1920, 1080), (1920, 0, 1920, 1080)];
        assert_eq!(saved(100, 100).position_on(&monitors), Some((100, 100)));
        assert_eq!(saved(2000, 200).position_on(&monitors), Some((2000, 200)));
        // Partly off the left edge still counts as visible.
        assert_eq!(saved(-400, 100).position_on(&monitors), Some((-400, 100)));
    }

    #[test]
    fn position_dropped_when_window_is_off_screen() {
        let monitors = [(0, 0, 1920, 1080)];
        // Fully right of, below, and left of the only monitor.
        assert_eq!(saved(2000, 100).position_on(&monitors), None);
        assert_eq!(saved(100, 1200).position_on(&monitors), None);
        assert_eq!(saved(-900, 100).position_on(&monitors), None);
    }

    #[test]
    fn position_absent_without_saved_coordinates() {
        let state = WindowState {
            width: 800,
            height: 600,
            x: None,
            y: None,
            maximized: false,
        };
        assert_eq!(state.position_on(&[(0, 0, 1920, 1080)]), None);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let loaded = load_from(Path::new("/nonexistent/oryx-config.toml"));
        assert_eq!(loaded, Config::default());
        assert_eq!(loaded.theme, "dracula");
    }

    #[test]
    fn garbage_file_gives_defaults() {
        let path = temp_path("garbage.toml");
        std::fs::write(&path, "not [valid\ntoml = ").unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let path = temp_path("partial.toml");
        std::fs::write(&path, "theme = \"nord\"\n").unwrap();
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.theme, "nord");
        assert_eq!(loaded.body_size, 22.0);
        assert_eq!(loaded.last_dir, "");
    }
}
