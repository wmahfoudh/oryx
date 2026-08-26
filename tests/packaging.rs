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

/// A stage shaped like `release/windows/oryx` with a stand-in `oryx.exe`
/// of incompressible bytes, so the MSI's size tells what it embeds.
fn windows_stage(dir: &Path, exe_len: usize) {
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let exe: Vec<u8> = (0..exe_len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    std::fs::write(dir.join("oryx.exe"), exe).unwrap();
    std::fs::write(dir.join("LICENSE"), "GPL\n").unwrap();
    std::fs::write(dir.join("install.ps1"), "# per-user\n").unwrap();
    std::fs::write(dir.join("themes/dracula.toml"), "name = \"dracula\"\n").unwrap();
    std::fs::write(dir.join("examples/sample.md"), "# Sample\n").unwrap();
}

/// `msiinfo <command> <msi> <rest...>`, as the tool orders its arguments;
/// table exports come in IDT form with CRLF line ends, normalized here.
fn msiinfo(command: &str, msi: &Path, rest: &[&str]) -> String {
    let out = Command::new("msiinfo")
        .arg(command)
        .arg(msi)
        .args(rest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

#[test]
fn the_msi_embeds_an_icon_file_not_a_second_copy_of_the_exe() {
    for tool in ["wixl", "msiinfo"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("{tool} is not installed, skipped");
            return;
        }
    }
    let dir = std::env::temp_dir().join(format!("oryx-msi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stage = dir.join("oryx");
    let exe_len = 2 << 20;
    windows_stage(&stage, exe_len);
    let ico = Path::new(env!("OUT_DIR")).join("oryx.ico");
    let msi = dir.join("oryx.msi");
    let out = Command::new("sh")
        .arg(repo().join("packaging/msi.sh"))
        .arg("1.2.3")
        .arg(&stage)
        .arg(&msi)
        .arg(&ico)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let streams = msiinfo("streams", &msi, &[]);
    assert!(streams.contains("oryx.cab\n"), "{streams}");
    assert!(streams.contains("Icon.oryx.ico\n"), "{streams}");
    assert!(!streams.contains("Icon.oryx.exe"), "{streams}");
    let properties = msiinfo("export", &msi, &["Property"]);
    assert!(
        properties.contains("Manufacturer\tSteerania\n"),
        "{properties}"
    );
    assert!(
        properties.contains("ProductVersion\t1.2.3\n"),
        "{properties}"
    );
    assert!(
        properties.contains("UpgradeCode\t{8ea2ee23-91f8-46ec-9310-6dfbf39a04c9}\n"),
        "{properties}"
    );
    let msi_len = std::fs::metadata(&msi).unwrap().len() as usize;
    assert!(
        msi_len < exe_len + exe_len / 2,
        "the MSI is {msi_len} bytes for a {exe_len} byte exe: something is embedded twice"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The `<uap:FileType>` entries of a manifest, without their dots.
fn manifest_file_types(xml: &str) -> Vec<String> {
    xml.split("<uap:FileType>")
        .skip(1)
        .filter_map(|rest| rest.split("</uap:FileType>").next())
        .map(|ext| ext.trim_start_matches('.').to_string())
        .collect()
}

#[test]
fn the_msix_manifest_claims_every_extension_the_code_registers() {
    let xml = packaging("msix/AppxManifest.xml");
    let mut listed = manifest_file_types(&xml);
    listed.sort_unstable();
    let mut expected: Vec<String> = oryx::doc::load::recognized_extensions()
        .into_iter()
        .map(str::to_string)
        .collect();
    expected.sort_unstable();
    assert_eq!(listed, expected);
}

#[test]
fn msix_sh_builds_a_package_that_unpacks_to_the_staged_files() {
    if Command::new("makemsix").arg("-?").output().is_err() {
        eprintln!("makemsix is not installed, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-msix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stage = dir.join("oryx");
    windows_stage(&stage, 64 << 10);
    let msix = dir.join("Oryx.msix");
    let out = Command::new("sh")
        .arg(repo().join("packaging/msix.sh"))
        .arg("1.2.3")
        .arg(&stage)
        .arg(&msix)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let unpacked = dir.join("unpacked");
    // The Store signs the package; until then it is unsigned, and the
    // SDK's reader needs `-ss` to open one.
    let out = Command::new("makemsix")
        .args(["unpack", "-ss", "-p"])
        .arg(&msix)
        .arg("-d")
        .arg(&unpacked)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The reader writes the payload and the manifest; the two other
    // footprint files stay inside the archive and are listed from it.
    let archive = zip::ZipArchive::new(std::fs::File::open(&msix).unwrap()).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    for footprint in [
        "AppxManifest.xml",
        "AppxBlockMap.xml",
        "[Content_Types].xml",
    ] {
        assert!(
            names.contains(&footprint),
            "{footprint} missing from {names:?}"
        );
    }
    for file in [
        "oryx.exe",
        "LICENSE",
        "themes/dracula.toml",
        "examples/sample.md",
        "AppxManifest.xml",
        "Assets/StoreLogo.png",
        "Assets/Square44x44Logo.png",
        "Assets/Square71x71Logo.png",
        "Assets/Square150x150Logo.png",
        "Assets/Square310x310Logo.png",
        "Assets/Wide310x150Logo.png",
    ] {
        assert!(unpacked.join(file).is_file(), "{file} missing");
    }
    assert!(
        !unpacked.join("install.ps1").exists(),
        "the per-user script has no place in the package"
    );
    assert_eq!(
        std::fs::read(unpacked.join("oryx.exe")).unwrap(),
        std::fs::read(stage.join("oryx.exe")).unwrap(),
        "the executable comes back byte for byte"
    );
    let manifest = std::fs::read_to_string(unpacked.join("AppxManifest.xml")).unwrap();
    for needle in [
        "Name=\"Steerania.OryxEditor\"",
        "Publisher=\"CN=D7BBF0E0-38CE-442D-8B8D-130515345FA0\"",
        "Version=\"1.2.3.0\"",
        "<DisplayName>Oryx Editor</DisplayName>",
        "<PublisherDisplayName>Steerania</PublisherDisplayName>",
        "Executable=\"oryx.exe\"",
        "EntryPoint=\"Windows.FullTrustApplication\"",
        "<rescap:Capability Name=\"runFullTrust\"/>",
        "<Capability Name=\"internetClient\"/>",
        "<uap:FileType>.md</uap:FileType>",
        "<uap:FileType>.epub</uap:FileType>",
    ] {
        assert!(
            manifest.contains(needle),
            "{needle} missing from the manifest"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
