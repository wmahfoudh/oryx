//! OS integration installed by `oryx --register`: a desktop entry and
//! hicolor icons under the app id on Linux; ProgId keys per extension and
//! the Applications entry naming the app in Open with on Windows; bundle
//! guidance on macOS. Each action prints what it wrote, and the Linux one
//! the desktop caches it rebuilt. A packaged Linux binary registers
//! nothing, since its package installed the same files system-wide.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The application id on Linux: the desktop entry and icon file names,
/// the `Icon` and `StartupWMClass` the entry declares, and the Wayland
/// app_id (X11 class) the window sets, so every desktop matches window,
/// entry and icon. Flathub exports only files named after it.
pub const APP_ID: &str = "com.steerania.Oryx";

/// The file names `--register` wrote before the app id, removed on the
/// next registration so launchers do not list Oryx twice.
const OLD_ENTRY: &str = "oryx.desktop";
const OLD_ICON: &str = "oryx.png";

/// Icon rasters produced by the build script, largest last.
const ICONS: [(u32, &[u8]); 6] = [
    (
        16,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_16.rgba")),
    ),
    (
        32,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_32.rgba")),
    ),
    (
        48,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_48.rgba")),
    ),
    (
        64,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_64.rgba")),
    ),
    (
        128,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_128.rgba")),
    ),
    (
        256,
        include_bytes!(concat!(env!("OUT_DIR"), "/icon_256.rgba")),
    ),
];

/// The types the entry claims, under the names shared-mime-info gives the
/// formats `load::detect` accepts: markdown and plain text, then each book
/// and comic container. A file manager offers Open with Oryx only for a
/// type listed here.
const MIME_TYPES: [&str; 11] = [
    "text/markdown",
    "text/x-markdown",
    "text/plain",
    "application/epub+zip",
    "application/x-mobipocket-ebook",
    "application/vnd.amazon.ebook",
    "application/vnd.amazon.mobi8-ebook",
    "application/x-fictionbook+xml",
    "application/x-zip-compressed-fb2",
    "application/vnd.comicbook+zip",
    "application/vnd.comicbook-rar",
];

/// The desktop entry content. `Icon` and `StartupWMClass` carry the app
/// id the window sets, so Wayland compositors resolve the icon.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Oryx\n\
         GenericName=Markdown editor and book reader\n\
         Comment=Fast editor for markdown and code, reader for ebooks and comics\n\
         Exec={} %f\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Office;Viewer;\n\
         MimeType={};\n\
         StartupWMClass={APP_ID}\n",
        exe.display(),
        MIME_TYPES.join(";")
    )
}

/// What a Linux registration did under the data dir: the entry and
/// icons written, and the files of an earlier version removed.
#[derive(Debug, Default)]
pub struct LinuxFiles {
    pub written: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

/// Writes the desktop entry and hicolor icons under `data_dir`
/// (`~/.local/share` in a real install), removing the old names beside
/// each new one.
pub fn install_linux(data_dir: &Path, exe: &Path) -> std::io::Result<LinuxFiles> {
    let mut files = LinuxFiles::default();
    let applications = data_dir.join("applications");
    std::fs::create_dir_all(&applications)?;
    remove_old(&applications.join(OLD_ENTRY), &mut files.removed)?;
    let entry = applications.join(format!("{APP_ID}.desktop"));
    std::fs::write(&entry, desktop_entry(exe))?;
    files.written.push(entry);
    for (size, rgba) in ICONS {
        let dir = data_dir.join(format!("icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)?;
        remove_old(&dir.join(OLD_ICON), &mut files.removed)?;
        let path = dir.join(format!("{APP_ID}.png"));
        let image =
            image::RgbaImage::from_raw(size, size, rgba.to_vec()).expect("icon raster dimensions");
        image
            .save_with_format(&path, image::ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        files.written.push(path);
    }
    Ok(files)
}

/// Removes a file an earlier version wrote and records it; a missing
/// file is the normal case on a fresh machine.
fn remove_old(path: &Path, removed: &mut Vec<PathBuf>) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            removed.push(path.to_path_buf());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Whether a package installed this binary and, with it, the desktop
/// entry and icons: under a system prefix (`/usr/bin` and `/usr/lib` for
/// a distribution package, `/app` for a Flatpak, `/opt` for a vendor
/// tree), or inside a Flatpak sandbox, which mounts `/.flatpak-info`.
/// A per-user entry written then would shadow the packaged one and
/// outlive the package. An AppImage mounts under `/tmp`, so it is not
/// packaged in this sense and registers per user.
pub fn packaged(exe: &Path, flatpak_info: bool) -> bool {
    const PREFIXES: [&str; 4] = ["/usr/bin", "/usr/lib", "/app", "/opt"];
    flatpak_info || PREFIXES.iter().any(|prefix| exe.starts_with(prefix))
}

/// The path the desktop entry runs. Inside an AppImage the executable
/// lives in a temporary mount that goes away with the process, so the
/// entry names the AppImage file `$APPIMAGE` points at.
pub fn exec_path() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exec_path_from(
        std::env::var_os("APPIMAGE").as_deref(),
        &exe,
    ))
}

fn exec_path_from(appimage: Option<&OsStr>, exe: &Path) -> PathBuf {
    match appimage {
        Some(image) if !image.is_empty() => PathBuf::from(image),
        _ => exe.to_path_buf(),
    }
}

/// The caches that file managers and launchers read instead of the files
/// under `data_dir`, each as the tool that rebuilds it and the tool's
/// arguments. GNOME's file chooser reads the applications cache, so a
/// fresh entry shows before the next login only once it is rebuilt. The
/// icon cache takes `-t` because a user's hicolor folder has no
/// index.theme.
pub fn cache_refreshers(data_dir: &Path) -> [(&'static str, Vec<PathBuf>); 2] {
    [
        (
            "update-desktop-database",
            vec![PathBuf::from("-q"), data_dir.join("applications")],
        ),
        (
            "gtk-update-icon-cache",
            vec![
                PathBuf::from("-q"),
                PathBuf::from("-t"),
                PathBuf::from("-f"),
                data_dir.join("icons/hicolor"),
            ],
        ),
    ]
}

/// Runs one cache tool. `None` when the tool is not installed, which is
/// no failure: the desktop reads the files themselves at the next login.
/// `Some` carries whether the run succeeded.
pub fn refresh_cache(tool: &str, args: &[PathBuf]) -> Option<bool> {
    match std::process::Command::new(tool).args(args).status() {
        Ok(status) => Some(status.success()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(false),
    }
}

/// Installs the platform integration and prints every path it touched.
pub fn register() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let exe = exec_path()?;
        if packaged(&exe, Path::new("/.flatpak-info").exists()) {
            println!("installed by the package, nothing to register");
            return Ok(());
        }
        let home = std::env::var_os("HOME").ok_or(std::io::ErrorKind::NotFound)?;
        let data_dir = PathBuf::from(home).join(".local/share");
        let files = install_linux(&data_dir, &exe)?;
        for path in &files.removed {
            println!("removed {}", path.display());
        }
        for path in &files.written {
            println!("wrote {}", path.display());
        }
        for (tool, args) in cache_refreshers(&data_dir) {
            match refresh_cache(tool, &args) {
                Some(true) => println!("ran {tool}"),
                Some(false) => println!("{tool} failed; the entry shows after the next login"),
                None => {}
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        register_windows(&std::env::current_exe()?)?;
    }
    #[cfg(target_os = "macos")]
    {
        println!("association on macOS comes with the app bundle; no user setup needed");
    }
    Ok(())
}

/// HKCU ProgId, the Applications entry whose FriendlyAppName labels the
/// Open with menu, and an OpenWithProgids entry per supported extension.
#[cfg(target_os = "windows")]
fn register_windows(exe: &Path) -> std::io::Result<()> {
    use crate::doc::load;
    use crate::platform::resource::FRIENDLY_APP_NAME;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes =
        hkcu.open_subkey_with_flags("Software\\Classes", winreg::enums::KEY_ALL_ACCESS)?;
    let (progid, _) = classes.create_subkey("Oryx.Document")?;
    progid.set_value("", &"Oryx Document")?;
    let (icon, _) = progid.create_subkey("DefaultIcon")?;
    icon.set_value("", &format!("{},0", exe.display()))?;
    let (command, _) = progid.create_subkey("shell\\open\\command")?;
    command.set_value("", &format!("\"{}\" \"%1\"", exe.display()))?;
    println!("wrote HKCU\\Software\\Classes\\Oryx.Document");
    let exe_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("oryx.exe");
    let (app, _) = classes.create_subkey(format!("Applications\\{exe_name}"))?;
    app.set_value("FriendlyAppName", &FRIENDLY_APP_NAME)?;
    let (app_command, _) = app.create_subkey("shell\\open\\command")?;
    app_command.set_value("", &format!("\"{}\" \"%1\"", exe.display()))?;
    println!("wrote HKCU\\Software\\Classes\\Applications\\{exe_name}");
    for ext in load::recognized_extensions() {
        let (key, _) = classes.create_subkey(format!(".{ext}\\OpenWithProgids"))?;
        key.set_value("Oryx.Document", &"")?;
        println!("wrote HKCU\\Software\\Classes\\.{ext}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;

    #[test]
    fn desktop_entry_content_exact() {
        let entry = desktop_entry(Path::new("/usr/bin/oryx"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Exec=/usr/bin/oryx %f\n"));
        assert_eq!(APP_ID, "com.steerania.Oryx");
        assert!(entry.contains("Icon=com.steerania.Oryx\n"));
        assert!(entry.contains("StartupWMClass=com.steerania.Oryx\n"));
        assert!(entry.contains("MimeType=text/markdown;"));
        assert!(
            entry.contains("application/epub+zip;"),
            "books open from the file manager"
        );
        assert!(entry.contains("GenericName=Markdown editor and book reader\n"));
        assert!(entry
            .contains("Comment=Fast editor for markdown and code, reader for ebooks and comics\n"));
    }

    #[test]
    fn the_entry_claims_the_book_and_comic_types() {
        let entry = desktop_entry(Path::new("/usr/bin/oryx"));
        let line = entry
            .lines()
            .find(|line| line.starts_with("MimeType="))
            .expect("a MimeType line");
        assert!(line.ends_with(';'), "the list closes on a semicolon");
        for kind in [
            "application/x-mobipocket-ebook",
            "application/vnd.amazon.ebook",
            "application/vnd.amazon.mobi8-ebook",
            "application/x-fictionbook+xml",
            "application/x-zip-compressed-fb2",
            "application/vnd.comicbook+zip",
            "application/vnd.comicbook-rar",
        ] {
            assert!(line.contains(&format!(";{kind};")), "{kind} missing");
        }
    }

    #[test]
    fn the_cache_refresh_names_both_tools_and_their_folders() {
        let data_dir = Path::new("/home/someone/.local/share");
        let refreshers = cache_refreshers(data_dir);
        let tools: Vec<&str> = refreshers.iter().map(|(tool, _)| *tool).collect();
        assert_eq!(tools, ["update-desktop-database", "gtk-update-icon-cache"]);
        let folders: Vec<&PathBuf> = refreshers
            .iter()
            .map(|(_, args)| args.last().unwrap())
            .collect();
        assert_eq!(
            folders,
            [
                &data_dir.join("applications"),
                &data_dir.join("icons/hicolor")
            ]
        );
        assert!(
            refreshers[1].1.iter().any(|arg| arg == "-t"),
            "a user's hicolor folder has no index.theme"
        );
    }

    #[test]
    fn a_missing_tool_is_skipped_and_a_present_one_reports_its_exit() {
        assert_eq!(refresh_cache("oryx-no-such-tool", &[]), None);
        #[cfg(unix)]
        {
            assert_eq!(refresh_cache("true", &[]), Some(true));
            assert_eq!(refresh_cache("false", &[]), Some(false));
        }
    }

    #[test]
    fn registered_extensions_match_load_detect() {
        let exts = load::recognized_extensions();
        assert!(exts.contains(&"md"));
        assert!(exts.contains(&"rs"));
        assert!(exts.contains(&"txt"));
        for ext in exts {
            let known = load::detect(Path::new(&format!("f.{ext}"))) != load::FileKind::Unknown;
            assert!(known, "{ext} not recognized by load::detect");
        }
    }

    /// A fresh data dir for one test, removed by the caller.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn install_linux_writes_entry_and_icons_under_the_app_id() {
        let dir = scratch("reg");
        let files = install_linux(&dir, Path::new("/opt/oryx")).unwrap();
        assert_eq!(files.written.len(), 7, "desktop entry plus six icons");
        assert_eq!(
            files.written[0],
            dir.join("applications/com.steerania.Oryx.desktop")
        );
        let entry = std::fs::read_to_string(&files.written[0]).unwrap();
        assert!(entry.contains("Exec=/opt/oryx %f"));
        assert!(files.written[1..]
            .iter()
            .all(|path| path.ends_with("com.steerania.Oryx.png")));
        let png = std::fs::read(&files.written[1]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert!(files.removed.is_empty(), "nothing old on a bare machine");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn install_linux_removes_the_old_names_when_present() {
        let dir = scratch("reg-old");
        let applications = dir.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::write(applications.join("oryx.desktop"), "old").unwrap();
        let old_icons = dir.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&old_icons).unwrap();
        std::fs::write(old_icons.join("oryx.png"), "old").unwrap();
        let files = install_linux(&dir, Path::new("/opt/oryx")).unwrap();
        assert_eq!(
            files.removed,
            [
                applications.join("oryx.desktop"),
                old_icons.join("oryx.png")
            ]
        );
        assert!(!applications.join("oryx.desktop").exists());
        assert!(!old_icons.join("oryx.png").exists());
        assert!(applications.join("com.steerania.Oryx.desktop").exists());
        assert!(old_icons.join("com.steerania.Oryx.png").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn packaged_under_a_system_prefix_or_in_a_flatpak() {
        for exe in [
            "/usr/bin/oryx",
            "/usr/lib/oryx/oryx",
            "/app/bin/oryx",
            "/opt/oryx/oryx",
        ] {
            assert!(packaged(Path::new(exe), false), "{exe}");
        }
        for exe in [
            "/home/someone/.local/bin/oryx",
            "/usr/local/bin/oryx",
            "/tmp/.mount_oryx/usr/bin/oryx",
        ] {
            assert!(!packaged(Path::new(exe), false), "{exe}");
        }
        assert!(
            packaged(Path::new("/home/someone/.local/bin/oryx"), true),
            "a Flatpak sandbox counts wherever the binary sits"
        );
    }

    #[test]
    fn exec_path_prefers_the_appimage_variable() {
        use std::ffi::OsStr;
        let exe = Path::new("/tmp/.mount_oryx/usr/bin/oryx");
        assert_eq!(
            exec_path_from(Some(OsStr::new("/home/someone/Oryx.AppImage")), exe),
            PathBuf::from("/home/someone/Oryx.AppImage")
        );
        assert_eq!(exec_path_from(None, exe), exe);
        assert_eq!(
            exec_path_from(Some(OsStr::new("")), exe),
            exe,
            "an empty variable counts as unset"
        );
    }
}
