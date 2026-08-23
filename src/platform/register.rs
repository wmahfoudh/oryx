//! OS integration installed by `oryx --register`: a desktop entry and
//! hicolor icons on Linux; ProgId keys per extension and the
//! Applications entry naming the app in Open with on Windows; bundle
//! guidance on macOS. Each action prints what it wrote, and the Linux one
//! the desktop caches it rebuilt.

use std::path::{Path, PathBuf};

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

/// The desktop entry content; the app_id `oryx` set on the window matches
/// StartupWMClass and Icon so Wayland compositors resolve the icon.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Oryx\n\
         GenericName=Markdown editor and book reader\n\
         Comment=Fast editor for markdown and code, reader for ebooks and comics\n\
         Exec={} %f\n\
         Icon=oryx\n\
         Terminal=false\n\
         Categories=Office;Viewer;\n\
         MimeType={};\n\
         StartupWMClass=oryx\n",
        exe.display(),
        MIME_TYPES.join(";")
    )
}

/// Writes the desktop entry and hicolor icons under `data_dir`
/// (`~/.local/share` in a real install). Returns the files written.
pub fn install_linux(data_dir: &Path, exe: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    let applications = data_dir.join("applications");
    std::fs::create_dir_all(&applications)?;
    let entry = applications.join("oryx.desktop");
    std::fs::write(&entry, desktop_entry(exe))?;
    written.push(entry);
    for (size, rgba) in ICONS {
        let dir = data_dir.join(format!("icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("oryx.png");
        let image =
            image::RgbaImage::from_raw(size, size, rgba.to_vec()).expect("icon raster dimensions");
        image
            .save_with_format(&path, image::ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        written.push(path);
    }
    Ok(written)
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

/// Installs the platform integration and prints every path it wrote.
pub fn register() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").ok_or(std::io::ErrorKind::NotFound)?;
        let data_dir = PathBuf::from(home).join(".local/share");
        for path in install_linux(&data_dir, &exe)? {
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
        register_windows(&exe)?;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = exe;
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
        assert!(entry.contains("Icon=oryx\n"));
        assert!(entry.contains("StartupWMClass=oryx\n"));
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

    #[test]
    fn install_linux_writes_entry_and_icons() {
        let dir = std::env::temp_dir().join(format!("oryx-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let written = install_linux(&dir, Path::new("/opt/oryx")).unwrap();
        assert_eq!(written.len(), 7, "desktop entry plus six icons");
        let entry = std::fs::read_to_string(&written[0]).unwrap();
        assert!(entry.contains("Exec=/opt/oryx %f"));
        let png = std::fs::read(&written[1]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
