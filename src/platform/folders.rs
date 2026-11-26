//! Where Oryx keeps its own files: the settings and the book positions,
//! the user's themes, the unsaved notes and the image cache. They are
//! in the folders the system names, except in a portable copy on
//! Windows, which keeps them in the `data` folder next to `oryx.exe`.
//!
//! The `data` folder is the mark of a portable copy: the Windows zip
//! has it, an installed copy does not. It is looked for at each launch
//! and stored nowhere, so the copy can move. A path saved in the
//! settings of a portable copy is relative to Oryx's folder, since a
//! USB stick can get another drive letter on each computer.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use directories::{BaseDirs, ProjectDirs};

use crate::platform::notes;

/// The folder next to `oryx.exe` that makes the copy portable.
pub const MARK: &str = "data";

/// Shown at launch when the `data` folder refuses a file.
pub const READ_ONLY_NOTICE: &str =
    "Oryx's folder is read-only, so the settings are saved on this computer instead.";

/// Printed by `--register` in a portable copy, before it registers.
pub const REGISTER_WARNING: &str =
    "This copy of Oryx is portable. The file types of this computer \
     will open with the Oryx of this folder. If the folder moves or the drive is removed, they \
     stop working.";

/// The folders of Oryx's own files. Each is None when the system names
/// no home folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Folders {
    /// `config.toml` and `positions.toml`, with their lock files.
    pub config: Option<PathBuf>,
    /// The themes the user copied or edited.
    pub themes: Option<PathBuf>,
    /// The unsaved notes, a folder per running Oryx.
    pub notes: Option<PathBuf>,
    /// The downloaded remote images.
    pub images: Option<PathBuf>,
}

/// What a launch found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    pub folders: Folders,
    /// Oryx's folder in a portable copy, the base of the relative paths
    /// in its settings. None elsewhere: paths are stored in full.
    pub base: Option<String>,
    /// The `data` folder is there and refused a file: the folders are
    /// the system's, and the launch says so.
    pub read_only: bool,
}

impl Resolved {
    /// Whether the `data` folder is next to the program, used or not.
    pub fn marked(&self) -> bool {
        self.base.is_some() || self.read_only
    }

    /// The notice a window shows at launch, if any.
    pub fn notice(&self) -> Option<&'static str> {
        self.read_only.then_some(READ_ONLY_NOTICE)
    }
}

/// The folders for a program in `program`, where the system names
/// `system`. `program` is None on a platform where the mark does not
/// count. `takes_a_file` says whether a folder accepts a new file.
pub fn resolve(
    program: Option<&Path>,
    system: Folders,
    takes_a_file: impl FnOnce(&Path) -> bool,
) -> Resolved {
    let installed = |read_only| Resolved {
        folders: system.clone(),
        base: None,
        read_only,
    };
    let Some(program) = program else {
        return installed(false);
    };
    let data = program.join(MARK);
    if !data.is_dir() {
        return installed(false);
    }
    if !takes_a_file(&data) {
        return installed(true);
    }
    let found = Resolved {
        folders: Folders {
            config: Some(data.join("config")),
            themes: Some(data.join("themes")),
            notes: Some(data.join("notes")),
            images: Some(data.join("cache").join("images")),
        },
        base: Some(program.display().to_string()),
        read_only: false,
    };
    debug_assert!(
        [
            &found.folders.config,
            &found.folders.themes,
            &found.folders.notes,
            &found.folders.images
        ]
        .iter()
        .all(|dir| dir.as_ref().is_some_and(|dir| dir.starts_with(&data))),
        "a portable copy keeps each folder under {}",
        data.display()
    );
    found
}

/// Whether `dir` accepts a new file, tried with a small file that is
/// removed at once. The permissions are not read: on Windows they do
/// not say whether a write will pass.
pub fn takes_a_file(dir: &Path) -> bool {
    use std::io::Write;
    let probe = dir.join(format!(".oryx-write-test-{}", std::process::id()));
    let written = std::fs::File::create(&probe).and_then(|mut file| file.write_all(b"oryx"));
    let _ = std::fs::remove_file(&probe);
    written.is_ok()
}

/// The folder of the running program where the mark counts, on Windows.
/// On Linux an AppImage runs from a temporary mount, and a Mac bundle
/// takes no data beside its binary.
fn program_folder() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    // The canonical form is the one a book's path is compared with: a
    // drive made with `subst` or a link would give another base.
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.parent().map(Path::to_path_buf)
}

/// The folders the system names, the ones of an installed copy.
pub fn system() -> Folders {
    let project = ProjectDirs::from("", "", "oryx");
    Folders {
        config: project.as_ref().map(|dirs| dirs.config_dir().to_path_buf()),
        themes: BaseDirs::new().map(|base| base.data_dir().join("oryx/themes")),
        notes: project
            .as_ref()
            .map(|dirs| notes::root(dirs.state_dir(), dirs.data_local_dir())),
        images: project.as_ref().map(|dirs| dirs.cache_dir().join("images")),
    }
}

/// The folders of this Oryx, found at the first ask and kept: the write
/// test runs once, and each caller gets the same answer.
pub fn get() -> &'static Resolved {
    static RESOLVED: OnceLock<Resolved> = OnceLock::new();
    RESOLVED.get_or_init(|| resolve(program_folder().as_deref(), system(), takes_a_file))
}

/// The base of the relative paths, in a portable copy.
pub fn base() -> Option<&'static str> {
    get().base.as_deref()
}

/// The form `path` takes in the settings. With a `base`, a full path on
/// the base's drive is written relative to it; any other value stays as
/// it is.
pub fn stored(path: &str, base: Option<&str>) -> String {
    let relative = || {
        let folder = parse(base?)?;
        let full = parse(path)?;
        if full.drive != folder.drive {
            return None;
        }
        let shared = folder
            .names
            .iter()
            .zip(&full.names)
            .take_while(|(a, b)| a == b)
            .count();
        let mut parts = vec![".."; folder.names.len() - shared];
        parts.extend(full.names[shared..].iter().map(String::as_str));
        let value = if parts.is_empty() {
            ".".to_string()
        } else {
            parts.join("\\")
        };
        debug_assert!(
            parse(&read(&value, base)).is_some_and(|back| back.names == full.names),
            "{value} does not read back as {path}"
        );
        Some(value)
    };
    relative().unwrap_or_else(|| path.to_string())
}

/// The path a stored `value` names. With a `base`, a relative value is
/// read from it; a full path stays as it is, and so does a value with a
/// colon, which no name of a relative path holds.
pub fn read(value: &str, base: Option<&str>) -> String {
    let full = || {
        let text = base?;
        let base = parse(text)?;
        if value.is_empty() || value.starts_with(SEPARATORS) || value.contains(':') {
            return None;
        }
        let mut names = base.names;
        for part in value.split(SEPARATORS).filter(|part| !part.is_empty()) {
            match part {
                "." => {}
                ".." => {
                    names.pop()?;
                }
                name => names.push(name.to_string()),
            }
        }
        let verbatim = if text.starts_with(VERBATIM) {
            VERBATIM
        } else {
            ""
        };
        Some(format!("{verbatim}{}:\\{}", base.drive, names.join("\\")))
    };
    full().unwrap_or_else(|| value.to_string())
}

/// The prefix of a canonical path on Windows, as `canonicalize` gives
/// it and as the position keys hold it.
const VERBATIM: &str = r"\\?\";

const SEPARATORS: [char; 2] = ['\\', '/'];

/// A full Windows path in pieces: the drive's letter in upper case, and
/// the names under the drive's root.
struct Parsed {
    drive: char,
    names: Vec<String>,
}

/// The pieces of `E:\Books\a.cbz` or of its canonical form,
/// `\\?\E:\Books\a.cbz`. None for anything else: a network share,
/// a path with no drive or no root, a path that still holds a `.` or a
/// `..`, which a canonical path never does, a name with a colon, which
/// `read` takes for a value that is no relative path. The rule is written on the
/// text and not on `std::path`, which knows a drive letter on Windows
/// alone: the tests run on each platform.
fn parse(text: &str) -> Option<Parsed> {
    let rest = text.strip_prefix(VERBATIM).unwrap_or(text);
    let mut chars = rest.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str().strip_prefix(':')?;
    if !rest.starts_with(SEPARATORS) {
        return None;
    }
    let names: Vec<String> = rest
        .split(SEPARATORS)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    if names
        .iter()
        .any(|name| name == "." || name == ".." || name.contains(':'))
    {
        return None;
    }
    Some(Parsed {
        drive: drive.to_ascii_uppercase(),
        names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_folders() -> Folders {
        Folders {
            config: Some(PathBuf::from("/sys/roaming/oryx/config")),
            themes: Some(PathBuf::from("/sys/roaming/oryx/themes")),
            notes: Some(PathBuf::from("/sys/local/oryx/data/notes")),
            images: Some(PathBuf::from("/sys/local/oryx/cache/images")),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-folders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_a_program_folder_the_folders_are_the_systems() {
        let found = resolve(None, system_folders(), |_| panic!("no write test"));
        assert_eq!(found.folders, system_folders());
        assert_eq!(found.base, None);
        assert!(!found.read_only && !found.marked());
        assert_eq!(found.notice(), None);
    }

    #[test]
    fn without_the_data_folder_the_folders_are_the_systems() {
        let program = scratch("plain");
        let found = resolve(Some(&program), system_folders(), |_| {
            panic!("no write test")
        });
        assert_eq!(found.folders, system_folders());
        assert_eq!(found.base, None);
        assert!(!found.marked());
        std::fs::remove_dir_all(&program).unwrap();
    }

    #[test]
    fn a_file_named_data_is_not_the_mark() {
        let program = scratch("file");
        std::fs::write(program.join(MARK), "").unwrap();
        let found = resolve(Some(&program), system_folders(), |_| {
            panic!("no write test")
        });
        assert_eq!(found.folders, system_folders());
        assert!(!found.marked());
        assert_eq!(found.notice(), None);
        std::fs::remove_dir_all(&program).unwrap();
    }

    #[test]
    fn with_the_data_folder_each_of_the_five_places_is_under_it() {
        let program = scratch("marked");
        let data = program.join(MARK);
        std::fs::create_dir(&data).unwrap();
        let found = resolve(Some(&program), system_folders(), takes_a_file);
        assert_eq!(
            found.folders,
            Folders {
                config: Some(data.join("config")),
                themes: Some(data.join("themes")),
                notes: Some(data.join("notes")),
                images: Some(data.join("cache").join("images")),
            }
        );
        assert_eq!(found.base.as_deref(), program.to_str());
        assert!(found.marked() && !found.read_only);
        assert_eq!(found.notice(), None);
        assert_eq!(
            std::fs::read_dir(&data).unwrap().count(),
            0,
            "the write test leaves nothing behind"
        );
        std::fs::remove_dir_all(&program).unwrap();
    }

    #[test]
    fn a_data_folder_that_refuses_a_file_gives_the_systems_folders_and_the_notice() {
        let program = scratch("refused");
        std::fs::create_dir(program.join(MARK)).unwrap();
        let found = resolve(Some(&program), system_folders(), |_| false);
        assert_eq!(found.folders, system_folders());
        assert_eq!(
            found.base, None,
            "paths are stored in full after the fallback"
        );
        assert!(found.read_only && found.marked());
        assert_eq!(found.notice(), Some(READ_ONLY_NOTICE));
        std::fs::remove_dir_all(&program).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_write_test_fails_in_a_folder_that_refuses_a_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("sealed");
        assert!(takes_a_file(&dir));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // An administrator writes anywhere: the test then says nothing.
        let refused = std::fs::File::create(dir.join("probe")).is_err();
        if refused {
            assert!(!takes_a_file(&dir));
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_write_test_fails_where_there_is_no_folder() {
        let dir = scratch("gone");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!takes_a_file(&dir));
    }

    const BASE: &str = r"\\?\E:\Tools\Oryx";

    #[test]
    fn without_a_base_a_path_is_stored_and_read_as_it_is() {
        for path in [
            r"\\?\E:\Books\a.cbz",
            r"C:\Users\u\notes",
            "/home/u/notes",
            "",
            "urn:uuid:1",
        ] {
            assert_eq!(stored(path, None), path);
            assert_eq!(read(path, None), path);
        }
    }

    #[test]
    fn a_path_on_the_base_drive_is_stored_relative_to_the_base() {
        let cases = [
            (r"\\?\E:\Tools\Oryx\books\a.cbz", r"books\a.cbz"),
            (r"\\?\E:\Tools\Oryx", "."),
            (r"\\?\E:\Tools\Oryx\", "."),
            (r"\\?\E:\Tools\Reader\b.epub", r"..\Reader\b.epub"),
            (r"\\?\E:\Books\a.cbz", r"..\..\Books\a.cbz"),
            (r"\\?\E:\", r"..\.."),
            (r"E:\Books\a.cbz", r"..\..\Books\a.cbz"),
            (r"e:\Books\a.cbz", r"..\..\Books\a.cbz"),
            (r"E:/Books/a.cbz", r"..\..\Books\a.cbz"),
        ];
        for (path, expected) in cases {
            assert_eq!(stored(path, Some(BASE)), expected, "{path}");
        }
    }

    #[test]
    fn a_path_on_another_drive_or_on_a_share_is_stored_in_full() {
        for path in [
            r"\\?\C:\Users\u\Books\a.cbz",
            r"C:\Users\u\Books",
            r"\\server\share\Books\a.cbz",
            r"\\?\UNC\server\share\Books\a.cbz",
        ] {
            assert_eq!(stored(path, Some(BASE)), path);
            assert_eq!(read(path, Some(BASE)), path);
        }
    }

    #[test]
    fn a_value_that_is_no_full_path_is_stored_as_it_is() {
        for value in [
            "",
            "urn:uuid:6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "urn:isbn:9780140449136|mobi",
            "a book",
            r"E:Books",
            r"\Books\a.cbz",
            r"\\?\E:\Books\..\a.cbz",
        ] {
            assert_eq!(stored(value, Some(BASE)), value);
        }
    }

    #[test]
    fn a_path_with_a_colon_in_a_name_is_stored_in_full() {
        for path in [r"\\?\E:\Tools\a:b", r"E:\Books\notes.md:stream"] {
            assert_eq!(stored(path, Some(BASE)), path);
            assert_eq!(read(path, Some(BASE)), path);
        }
        assert_eq!(
            stored(r"\\?\E:\Books\a.cbz", Some(r"\\?\E:\To:ols\Oryx")),
            r"\\?\E:\Books\a.cbz"
        );
    }

    #[test]
    fn the_prefix_counts_on_neither_side() {
        let plain = r"E:\Tools\Oryx";
        let book = r"\\?\E:\Books\a.cbz";
        assert_eq!(stored(book, Some(plain)), r"..\..\Books\a.cbz");
        assert_eq!(stored(book, Some(BASE)), stored(book, Some(plain)));
        assert_eq!(
            stored(r"E:\Books\a.cbz", Some(plain)),
            stored(book, Some(plain))
        );
        assert_eq!(read(r"..\..\Books\a.cbz", Some(plain)), r"E:\Books\a.cbz");
        assert_eq!(read(r"..\..\Books\a.cbz", Some(BASE)), book);
    }

    #[test]
    fn a_relative_value_is_read_from_the_base() {
        let cases = [
            (r"books\a.cbz", r"\\?\E:\Tools\Oryx\books\a.cbz"),
            (".", r"\\?\E:\Tools\Oryx"),
            (r"..\Reader\b.epub", r"\\?\E:\Tools\Reader\b.epub"),
            (r"..\..\Books\a.cbz", r"\\?\E:\Books\a.cbz"),
            (r"..\..", r"\\?\E:\"),
            ("../../Books/a.cbz", r"\\?\E:\Books\a.cbz"),
        ];
        for (value, expected) in cases {
            assert_eq!(read(value, Some(BASE)), expected, "{value}");
        }
        assert_eq!(
            read(r"books\a.cbz", Some(r"F:\Oryx")),
            r"F:\Oryx\books\a.cbz"
        );
        assert_eq!(read(r"..\Books", Some(r"F:\Oryx")), r"F:\Books");
        assert_eq!(read("..", Some(r"F:\Oryx")), r"F:\");
    }

    #[test]
    fn a_value_that_climbs_above_the_drive_is_read_as_it_is() {
        assert_eq!(read(r"..\..\..\x", Some(BASE)), r"..\..\..\x");
        for value in [
            "",
            "urn:uuid:1",
            "E:Books",
            r"\Books",
            r"\\server\share\Books",
        ] {
            assert_eq!(read(value, Some(BASE)), value);
        }
    }

    #[test]
    fn a_stored_path_is_found_after_the_folder_moves_with_its_neighbors() {
        let book = r"\\?\E:\Stick\Books\a.cbz";
        let value = stored(book, Some(r"\\?\E:\Stick\Oryx"));
        assert_eq!(value, r"..\Books\a.cbz");
        assert_eq!(
            read(&value, Some(r"\\?\G:\Stick\Oryx")),
            r"\\?\G:\Stick\Books\a.cbz",
            "another drive letter"
        );
        assert_eq!(
            read(&value, Some(r"\\?\C:\Users\u\Carried\Oryx")),
            r"\\?\C:\Users\u\Carried\Books\a.cbz",
            "the folder and its neighbors copied to a computer"
        );
    }

    /// The round trip, for any base and any path of plain names on its
    /// drive: what is stored reads back as the path. A colon in a name
    /// of either one leaves the path in full.
    #[test]
    #[ignore = "fuzzer, run by make fuzz"]
    fn fuzz_stored_paths() {
        use proptest::prelude::*;
        let name = "[A-Za-z0-9 _.:-]{1,8}"
            .prop_filter("a plain name", |n| n != "." && n != ".." && n.trim() == n);
        let names = || prop::collection::vec(name.clone(), 0..5);
        let config = ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        };
        proptest!(config, |(base_names in names(), path_names in names(), verbatim in any::<bool>())| {
            let prefix = if verbatim { r"\\?\E:\" } else { r"E:\" };
            let base = format!("{prefix}{}", base_names.join(r"\"));
            let full = format!("{prefix}{}", path_names.join(r"\"));
            let value = stored(&full, Some(&base));
            let colon = |names: &[String]| names.iter().any(|n| n.contains(':'));
            if colon(&base_names) || colon(&path_names) {
                prop_assert_eq!(&value, &full);
            } else {
                prop_assert!(parse(&value).is_none(), "{value} is still a full path");
            }
            prop_assert_eq!(read(&value, Some(&base)), full);
        });
    }
}
