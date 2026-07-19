//! Persistent user configuration: `config.toml` in the user's config
//! directory. Any read problem yields defaults, never an error.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub body_family: String,
    pub code_family: String,
    pub body_size: f32,
    pub code_size: f32,
    /// Folder of the last opened file; the open dialog starts here when
    /// no file is open. Empty means never set.
    pub last_dir: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: "oryx-light".to_string(),
            body_family: BODY_FAMILY.to_string(),
            code_family: CODE_FAMILY.to_string(),
            body_size: 22.0,
            code_size: 20.0,
            last_dir: String::new(),
        }
    }
}

/// Location of the config file, None when the platform gives no home.
pub fn path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "oryx").map(|dirs| dirs.config_dir().join("config.toml"))
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

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oryx-config-{}-{name}", std::process::id()))
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
        };
        save_to(&path, &config);
        let loaded = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let loaded = load_from(Path::new("/nonexistent/oryx-config.toml"));
        assert_eq!(loaded, Config::default());
        assert_eq!(loaded.theme, "oryx-light");
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
