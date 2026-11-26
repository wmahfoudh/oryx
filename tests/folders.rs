//! The folders Oryx keeps its own files in, as the rest of the code asks
//! for them. Without a `data` folder next to the program they are the
//! folders the system names, on each platform.

use directories::{BaseDirs, ProjectDirs};

use oryx::doc::fetch;
use oryx::platform::{config, folders};
use oryx::style::theme;

/// The pin of Task 442: each lookup answers what `directories` answers,
/// as before the lookups went through `platform::folders`.
#[test]
fn the_lookups_answer_the_folders_the_system_names() {
    let project = ProjectDirs::from("", "", "oryx").expect("a home folder");
    let base = BaseDirs::new().expect("a home folder");
    assert_eq!(
        config::path(),
        Some(project.config_dir().join("config.toml"))
    );
    assert_eq!(
        config::positions_path(),
        Some(project.config_dir().join("positions.toml"))
    );
    assert_eq!(theme::user_dir(), Some(base.data_dir().join("oryx/themes")));
    assert_eq!(fetch::cache_dir(), Some(project.cache_dir().join("images")));
}

/// The mark of a portable copy counts on Windows alone: elsewhere a
/// launch finds the system's folders, stores its paths in full and has
/// nothing to say.
#[cfg(not(windows))]
#[test]
fn off_windows_a_launch_finds_the_systems_folders() {
    let found = folders::get();
    assert_eq!(found.folders, folders::system());
    assert_eq!(folders::base(), None);
    assert!(!found.marked());
    assert_eq!(found.notice(), None);
}
