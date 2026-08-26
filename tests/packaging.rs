//! The shared Linux packaging files under `packaging/linux/` are held to
//! the code: the desktop entry is the one `--register` writes, run from
//! the PATH, and the metainfo's newest release is the crate version, so
//! the release line is written at bump time and sits inside the tag
//! Flathub builds from. The stage script is run on a small source folder
//! and its tree checked file by file.

use std::path::{Path, PathBuf};
use std::process::Command;

use oryx::platform::register::{desktop_entry, APP_ID, MIME_TYPES};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn packaging(name: &str) -> String {
    let path = repo().join("packaging").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// Runs a validator over a file; `None` when the tool is not installed,
/// which skips the test rather than failing it.
fn validate(tool: &str, args: &[&str], path: &Path) -> Option<Result<(), String>> {
    match Command::new(tool).args(args).arg(path).output() {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{tool} is not installed, skipped");
            None
        }
        Err(err) => Some(Err(err.to_string())),
        Ok(out) if out.status.success() => Some(Ok(())),
        Ok(out) => Some(Err(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))),
    }
}

#[test]
fn the_desktop_entry_claims_the_types_the_code_registers() {
    let entry = packaging("linux/com.steerania.Oryx.desktop");
    let line = entry
        .lines()
        .find(|line| line.starts_with("MimeType="))
        .expect("a MimeType line");
    let mut listed: Vec<&str> = line["MimeType=".len()..]
        .split(';')
        .filter(|kind| !kind.is_empty())
        .collect();
    listed.sort_unstable();
    let mut expected = MIME_TYPES.to_vec();
    expected.sort_unstable();
    assert_eq!(listed, expected);
}

#[test]
fn the_desktop_entry_is_the_code_entry_run_from_the_path() {
    let entry = packaging("linux/com.steerania.Oryx.desktop");
    assert!(entry.contains(&format!("Icon={APP_ID}\n")));
    assert!(entry.contains(&format!("StartupWMClass={APP_ID}\n")));
    assert!(entry.contains("Exec=oryx %f\n"));
    assert_eq!(entry, desktop_entry(Path::new("oryx")));
}

#[test]
fn the_desktop_entry_validates_when_the_tool_is_installed() {
    let path = repo().join("packaging/linux/com.steerania.Oryx.desktop");
    if let Some(result) = validate("desktop-file-validate", &[], &path) {
        result.unwrap();
    }
}

#[test]
fn the_metainfo_newest_release_is_the_crate_version() {
    let xml = packaging("linux/com.steerania.Oryx.metainfo.xml");
    let first = xml.find("<release ").expect("a release list");
    let version = xml[first..]
        .split("version=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a version attribute");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_metainfo_names_the_app_id_its_entry_and_every_type() {
    let xml = packaging("linux/com.steerania.Oryx.metainfo.xml");
    assert!(xml.contains(&format!("<id>{APP_ID}</id>")));
    assert!(xml.contains(&format!(
        "<launchable type=\"desktop-id\">{APP_ID}.desktop</launchable>"
    )));
    for kind in MIME_TYPES {
        assert!(
            xml.contains(&format!("<mediatype>{kind}</mediatype>")),
            "{kind} missing from provides"
        );
    }
    assert!(xml.contains("<content_rating type=\"oars-1.1\""));
}

#[test]
fn the_metainfo_validates_when_the_tool_is_installed() {
    let path = repo().join("packaging/linux/com.steerania.Oryx.metainfo.xml");
    if let Some(result) = validate("appstreamcli", &["validate", "--no-net"], &path) {
        result.unwrap();
    }
}

/// A source folder shaped like `release/linux/oryx`: a stand-in binary,
/// one theme, one example and the license.
fn source_folder(dir: &Path) {
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    std::fs::write(dir.join("oryx"), "#!/bin/sh\n").unwrap();
    std::fs::write(dir.join("themes/dracula.toml"), "name = \"dracula\"\n").unwrap();
    std::fs::write(dir.join("examples/sample.md"), "# Sample\n").unwrap();
    std::fs::write(dir.join("LICENSE"), "GPL\n").unwrap();
}

fn stage(src: &Path, dest: &Path) -> std::process::Output {
    Command::new("sh")
        .arg(repo().join("packaging/stage-linux.sh"))
        .arg(src)
        .arg(dest)
        .output()
        .unwrap()
}

#[test]
fn stage_linux_lays_the_release_folder_out_as_the_package_tree() {
    if Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("rsvg-convert is not installed, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let dest = dir.join("usr");
    source_folder(&src);
    let out = stage(&src, &dest);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for file in [
        "bin/oryx",
        "share/oryx/themes/dracula.toml",
        "share/oryx/examples/sample.md",
        "share/applications/com.steerania.Oryx.desktop",
        "share/metainfo/com.steerania.Oryx.metainfo.xml",
        "share/icons/hicolor/scalable/apps/com.steerania.Oryx.svg",
        "share/licenses/oryx-editor/LICENSE",
    ] {
        assert!(dest.join(file).is_file(), "{file} missing");
    }
    for size in [16, 32, 48, 64, 128, 256, 512] {
        let icon = dest.join(format!(
            "share/icons/hicolor/{size}x{size}/apps/com.steerania.Oryx.png"
        ));
        let png = std::fs::read(&icon).unwrap_or_else(|err| panic!("{}: {err}", icon.display()));
        assert_eq!(&png[1..4], b"PNG", "{size}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("bin/oryx"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the binary is executable");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stage_linux_refuses_a_source_folder_missing_a_file() {
    let dir = std::env::temp_dir().join(format!("oryx-stage-short-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    source_folder(&src);
    std::fs::remove_file(src.join("LICENSE")).unwrap();
    let out = stage(&src, &dir.join("usr"));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("LICENSE"),
        "the message names the file: {stderr}"
    );
    assert!(
        !dir.join("usr").exists(),
        "nothing is written before the check"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
