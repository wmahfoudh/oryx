//! The shared Linux packaging files under `packaging/linux/` are held to
//! the code: the desktop entry is the one `--register` writes, run from
//! the PATH, and the metainfo's newest release is the crate version, so
//! the release line is written at bump time and sits inside the tag the
//! recipes build from, and the screenshot links carry that tag. The stage
//! script is run on a small source folder and its tree checked file by
//! file.

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

/// The metainfo's release lines in order, newest first: the version
/// and whether the line is marked `type="development"`.
fn metainfo_releases() -> Vec<(String, bool)> {
    let xml = packaging("linux/com.steerania.Oryx.metainfo.xml");
    xml.split("<release ")
        .skip(1)
        .map(|rest| {
            let tag = rest.split('>').next().unwrap();
            let version = tag
                .split("version=\"")
                .nth(1)
                .and_then(|v| v.split('"').next())
                .expect("a version attribute");
            (version.to_string(), tag.contains("type=\"development\""))
        })
        .collect()
}

/// The newest release the recipes describe. A phase opens by bumping
/// the crate and adding a release line marked development; the recipes
/// keep the checksums of the last published tag until `make channels`
/// rewrites them after the release, when the mark comes off.
fn last_stable_release() -> String {
    metainfo_releases()
        .into_iter()
        .find(|(_, development)| !development)
        .map(|(version, _)| version)
        .expect("a stable release line")
}

#[test]
fn the_metainfo_newest_release_is_the_crate_version() {
    let releases = metainfo_releases();
    assert_eq!(releases[0].0, env!("CARGO_PKG_VERSION"));
}

#[test]
fn only_the_newest_release_may_be_marked_development() {
    let releases = metainfo_releases();
    assert!(
        releases.iter().skip(1).all(|(_, development)| !development),
        "a development mark below the newest line is a release that never shipped"
    );
    let stable = last_stable_release();
    if releases[0].1 {
        assert_eq!(
            stable, releases[1].0,
            "the recipes lag the crate by one release"
        );
    } else {
        assert_eq!(stable, env!("CARGO_PKG_VERSION"));
    }
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

/// Extensions Windows reserves for the operating system: a package that
/// declares one fails to register (0x80080204) and the Store rejects it
/// in certification. The reservation binds MSIX packages only; the MSI
/// and `oryx --register` keep the full list. The reserved set is listed
/// at learn.microsoft.com/windows/apps/develop/launch/reserved-uri-scheme-names.
const MSIX_RESERVED: [&str; 6] = ["bat", "cmd", "js", "pl", "py", "rb"];

#[test]
fn the_msix_manifest_claims_every_extension_but_the_windows_reserved() {
    let xml = packaging("msix/AppxManifest.xml");
    let mut listed = manifest_file_types(&xml);
    listed.sort_unstable();
    let mut expected: Vec<String> = oryx::doc::load::recognized_extensions()
        .into_iter()
        .filter(|ext| !MSIX_RESERVED.contains(ext))
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
    let names: Vec<String> = archive
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    for footprint in [
        "AppxManifest.xml",
        "AppxBlockMap.xml",
        "[Content_Types].xml",
    ] {
        assert!(
            names.iter().any(|name| name == footprint),
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

/// `build-linux.sh --check <binary> <max glibc>`: the portable-build
/// script's floor check, run on the test executable itself, which
/// imports some glibc version whatever the machine.
fn glibc_check(max: &str) -> std::process::Output {
    Command::new("sh")
        .arg(repo().join("packaging/build-linux.sh"))
        .arg("--check")
        .arg(std::env::current_exe().unwrap())
        .arg(max)
        .output()
        .unwrap()
}

#[test]
fn build_linux_refuses_a_binary_above_the_glibc_floor() {
    let out = glibc_check("2.0");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("GLIBC_"),
        "the message names the version: {stderr}"
    );
    assert!(stderr.contains("2.0"), "and the floor: {stderr}");
}

#[test]
fn build_linux_accepts_a_binary_within_the_glibc_floor() {
    let out = glibc_check("99.0");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One tar member of a `.deb` (an `ar` archive) read through bsdtar,
/// whichever compression nfpm chose for it.
fn deb_member(deb: &Path, member: &str) -> Vec<u8> {
    for name in [
        format!("{member}.tar.gz"),
        format!("{member}.tar.zst"),
        format!("{member}.tar.xz"),
    ] {
        let out = Command::new("bsdtar")
            .args(["-xOf"])
            .arg(deb)
            .arg(&name)
            .output()
            .unwrap();
        if out.status.success() && !out.stdout.is_empty() {
            return out.stdout;
        }
    }
    panic!("{member}.tar.* missing from {}", deb.display());
}

fn tar_listing(bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("bsdtar")
        .args(["-tf", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned()
}

fn tar_file(bytes: &[u8], path: &str) -> String {
    use std::io::Write;
    let mut child = Command::new("bsdtar")
        .args(["-xOf", "-", path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned()
}

/// Runs nfpm over a staged stub tree, the way `make release` does from
/// `release/linux`: the working directory holds `usr/`.
fn nfpm_packages(name: &str) -> Option<PathBuf> {
    for tool in ["nfpm", "rsvg-convert", "bsdtar", "rpm"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("{tool} is not installed, skipped");
            return None;
        }
    }
    let dir = std::env::temp_dir().join(format!("oryx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    source_folder(&src);
    let out = stage(&src, &dir.join("usr"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for packager in ["deb", "rpm"] {
        let out = Command::new("nfpm")
            .args(["package", "-f"])
            .arg(repo().join("packaging/nfpm.yaml"))
            .args(["-p", packager, "-t", "."])
            .current_dir(&dir)
            .env("VERSION", "1.2.3")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Some(dir)
}

#[test]
fn the_deb_installs_the_staged_tree_and_names_its_dependencies() {
    let Some(dir) = nfpm_packages("deb") else {
        return;
    };
    let deb = dir.join("oryx-editor_1.2.3_amd64.deb");
    assert!(deb.is_file(), "the package name carries the version");
    let data = tar_listing(&deb_member(&deb, "data"));
    for file in [
        "./usr/bin/oryx",
        "./usr/share/applications/com.steerania.Oryx.desktop",
        "./usr/share/metainfo/com.steerania.Oryx.metainfo.xml",
        "./usr/share/icons/hicolor/512x512/apps/com.steerania.Oryx.png",
        "./usr/share/icons/hicolor/scalable/apps/com.steerania.Oryx.svg",
        "./usr/share/oryx/themes/dracula.toml",
        "./usr/share/oryx/examples/sample.md",
        "./usr/share/licenses/oryx-editor/LICENSE",
    ] {
        assert!(
            data.contains(&format!("{file}\n")),
            "{file} missing:\n{data}"
        );
    }
    let control = tar_file(&deb_member(&deb, "control"), "./control");
    for line in [
        "Package: oryx-editor\n",
        "Version: 1.2.3\n",
        "Section: editors\n",
        "Architecture: amd64\n",
        "Maintainer: Walid Mahfoudh <walid.mahfoudh@gmail.com>\n",
        "Depends: libc6 (>= 2.35), libssl3 (>= 3.0.0), libxkbcommon0, libxkbcommon-x11-0\n",
        "Homepage: https://github.com/wmahfoudh/oryx\n",
        "Description: Fast editor for markdown and code, reader for ebooks and comics\n",
    ] {
        assert!(control.contains(line), "{line:?} missing:\n{control}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_rpm_requires_its_libraries_and_carries_the_metadata() {
    let Some(dir) = nfpm_packages("rpm") else {
        return;
    };
    let rpm = dir.join("oryx-editor-1.2.3-1.x86_64.rpm");
    assert!(
        rpm.is_file(),
        "the package name carries the version and release"
    );
    let requires = Command::new("rpm")
        .args(["-qp", "--requires"])
        .arg(&rpm)
        .output()
        .unwrap();
    let requires = String::from_utf8_lossy(&requires.stdout);
    for library in [
        "libssl.so.3()(64bit)\n",
        "libxkbcommon.so.0()(64bit)\n",
        "libxkbcommon-x11.so.0()(64bit)\n",
    ] {
        assert!(
            requires.contains(library),
            "{library:?} missing:\n{requires}"
        );
    }
    let info = Command::new("rpm")
        .args([
            "-qp",
            "--qf",
            "%{NAME}|%{VERSION}|%{LICENSE}|%{URL}|%{VENDOR}|%{PACKAGER}",
        ])
        .arg(&rpm)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&info.stdout),
        "oryx-editor|1.2.3|GPL-3.0-only|https://github.com/wmahfoudh/oryx|Steerania|Walid Mahfoudh <walid.mahfoudh@gmail.com>"
    );
    let files = Command::new("rpm")
        .args(["-qpl"])
        .arg(&rpm)
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&files.stdout);
    for file in ["/usr/bin/oryx\n", "/usr/share/oryx/themes/dracula.toml\n"] {
        assert!(files.contains(file), "{file} missing:\n{files}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A stand-in AppDir: `AppRun` from `packaging/linux/` over a `usr/bin/oryx`
/// that prints the data dirs it was given and then its arguments.
fn app_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oryx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
    std::fs::write(
        dir.join("usr/bin/oryx"),
        "#!/bin/sh\nprintf '%s\\n' \"$XDG_DATA_DIRS\" \"$@\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.join("usr/bin/oryx"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    std::fs::copy(repo().join("packaging/linux/AppRun"), dir.join("AppRun")).unwrap();
    dir
}

fn app_run(dir: &Path, data_dirs: Option<&str>, args: &[&str]) -> String {
    let mut command = Command::new("sh");
    command
        .arg(dir.join("AppRun"))
        .args(args)
        .env_remove("APPDIR");
    match data_dirs {
        Some(value) => command.env("XDG_DATA_DIRS", value),
        None => command.env_remove("XDG_DATA_DIRS"),
    };
    // The stub was written moments ago, and a test on another thread
    // may fork its own child while the write handle is still open; the
    // child keeps that handle until it execs, and running the stub in
    // that window fails with "Text file busy". The window is
    // microseconds long, so a short retry rides it out.
    let mut out = command.output().unwrap();
    for _ in 0..20 {
        if out.status.success() || !String::from_utf8_lossy(&out.stderr).contains("Text file busy")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        out = command.output().unwrap();
    }
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn app_run_puts_the_bundle_share_before_the_data_dirs_and_passes_the_arguments() {
    let dir = app_dir("apprun");
    let out = app_run(
        &dir,
        Some("/x/share:/y/share"),
        &["--theme", "dracula", "notes.md"],
    );
    assert_eq!(
        out,
        format!(
            "{}/usr/share:/x/share:/y/share\n--theme\ndracula\nnotes.md\n",
            dir.display()
        )
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn app_run_keeps_the_default_data_dirs_when_the_variable_is_unset() {
    let dir = app_dir("apprun-unset");
    let out = app_run(&dir, None, &[]);
    assert_eq!(
        out,
        format!("{}/usr/share:/usr/local/share:/usr/share\n", dir.display())
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A stand-in for a tool, on a folder the test puts first on the PATH: a
/// shell script with the given body.
#[cfg(unix)]
fn stand_in(bin: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin).unwrap();
    let tool = bin.join(name);
    std::fs::write(&tool, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The PATH with `bin` in front of the session's.
#[cfg(unix)]
fn path_with(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// The three libraries of the X11 keyboard path that the AppImage
/// bundles, by soname.
#[cfg(unix)]
const KEYBOARD_LIBRARIES: [&str; 3] = [
    "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0",
    "libxcb-xkb.so.1",
];

/// The keyboard libraries as a distribution installs them, as small
/// files: the soname is a link to the versioned file. The folder is what
/// `APPIMAGE_LIBS` names in the tests of appimage.sh.
#[cfg(unix)]
fn keyboard_library_stubs(dir: &Path) -> PathBuf {
    let libs = dir.join("libs");
    std::fs::create_dir_all(&libs).unwrap();
    for soname in KEYBOARD_LIBRARIES {
        let file = format!("{soname}.0.0");
        std::fs::write(libs.join(&file), format!("{soname} bytes\n")).unwrap();
        std::os::unix::fs::symlink(&file, libs.join(soname)).unwrap();
    }
    libs
}

#[cfg(unix)]
#[test]
fn appimage_sh_packs_the_staged_tree_with_the_entry_and_the_icon_on_top() {
    // appimagetool is itself an AppImage and needs FUSE to start, so
    // it can be installed and still not run.
    for tool in ["appimagetool", "rsvg-convert"] {
        let runs = Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success());
        if !runs {
            eprintln!("{tool} is not installed or cannot run here, skipped");
            return;
        }
    }
    let dir = std::env::temp_dir().join(format!("oryx-appimage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let usr = dir.join("usr");
    source_folder(&src);
    let out = stage(&src, &usr);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::write(
        usr.join("bin/oryx"),
        "#!/bin/sh\nprintf '%s\\n' \"$XDG_DATA_DIRS\" \"$@\"\n",
    )
    .unwrap();
    // The stub binary is a shell script, which patchelf would refuse.
    let bin = dir.join("bin");
    stand_in(&bin, "patchelf", "");
    let libs = keyboard_library_stubs(&dir);
    let image = dir.join("Oryx-1.2.3-x86_64.AppImage");
    let out = Command::new("sh")
        .arg(repo().join("packaging/appimage.sh"))
        .arg(&usr)
        .arg(&image)
        .env("PATH", path_with(&bin))
        .env("APPIMAGE_LIBS", &libs)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&image).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the image is executable");
    }
    // The runtime unpacks the image without FUSE, into `squashfs-root`
    // under the working directory.
    let out = Command::new(&image)
        .arg("--appimage-extract")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let root = dir.join("squashfs-root");
    for file in [
        "AppRun",
        "com.steerania.Oryx.desktop",
        "com.steerania.Oryx.png",
        "usr/bin/oryx",
        "usr/share/applications/com.steerania.Oryx.desktop",
        "usr/share/metainfo/com.steerania.Oryx.metainfo.xml",
        "usr/share/oryx/themes/dracula.toml",
        "usr/share/oryx/examples/sample.md",
        "usr/share/icons/hicolor/256x256/apps/com.steerania.Oryx.png",
        "usr/lib/libxkbcommon.so.0",
        "usr/lib/libxkbcommon-x11.so.0",
        "usr/lib/libxcb-xkb.so.1",
    ] {
        assert!(root.join(file).is_file(), "{file} missing");
    }
    let png = std::fs::read(root.join("com.steerania.Oryx.png")).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert_eq!(
        std::fs::read_link(root.join(".DirIcon")).unwrap(),
        Path::new("com.steerania.Oryx.png"),
        ".DirIcon points at the top icon"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("com.steerania.Oryx.desktop")).unwrap(),
        packaging("linux/com.steerania.Oryx.desktop"),
        "the top entry is the packaging one, unchanged"
    );
    let out = Command::new(root.join("AppRun"))
        .arg("notes.md")
        .env_remove("APPDIR")
        .env("XDG_DATA_DIRS", "/x/share")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}/usr/share:/x/share\nnotes.md\n", root.display()),
        "the unpacked AppRun runs the bundled binary"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The mode of the AppDir becomes the mode of the top folder of the
/// image. firejail mounts the image as a plain user, who must be able to
/// enter that folder. A stand-in `appimagetool` writes the mode of the
/// AppDir it is given into the output file, so the test runs without
/// the real tool.
#[cfg(unix)]
#[test]
fn appimage_sh_lets_every_user_enter_the_top_folder_of_the_image() {
    if Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("rsvg-convert is not installed, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-appimage-mode-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let usr = dir.join("usr");
    source_folder(&src);
    let out = stage(&src, &usr);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = dir.join("bin");
    stand_in(
        &bin,
        "appimagetool",
        "for arg; do appdir=$out; out=$arg; done\nstat -c %a \"$appdir\" > \"$out\"\n",
    );
    stand_in(&bin, "patchelf", "");
    let libs = keyboard_library_stubs(&dir);
    let image = dir.join("Oryx-1.2.3-x86_64.AppImage");
    let out = Command::new("sh")
        .arg(repo().join("packaging/appimage.sh"))
        .arg(&usr)
        .arg(&image)
        .env("PATH", path_with(&bin))
        .env("APPIMAGE_RUNTIME", dir.join("no-runtime"))
        .env("APPIMAGE_LIBS", &libs)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&image).unwrap(),
        "755\n",
        "the mode of the AppDir handed to appimagetool"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The AppImage bundles the three libraries of the X11 keyboard path,
/// which winit loads at run time and which a minimal system lacks, and
/// gives the binary a search path to them: a RUNPATH, so that nothing
/// leaks into the programs Oryx starts. Stand-ins for `appimagetool`
/// (which copies the AppDir out) and `patchelf` (which records its
/// arguments) let the test run without the tools and without an ELF.
#[cfg(unix)]
#[test]
fn appimage_sh_bundles_the_keyboard_libraries_and_sets_their_search_path() {
    if Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("rsvg-convert is not installed, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-appimage-libs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let usr = dir.join("usr");
    source_folder(&src);
    let out = stage(&src, &usr);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let libs = keyboard_library_stubs(&dir);
    let bin = dir.join("bin");
    stand_in(
        &bin,
        "appimagetool",
        "for arg; do appdir=$out; out=$arg; done\ncp -r \"$appdir\" \"$out\"\n",
    );
    // The stand-in records each call as a line.
    let record = dir.join("patchelf.args");
    stand_in(
        &bin,
        "patchelf",
        &format!("echo \"$*\" >> {}\n", record.display()),
    );
    let path = path_with(&bin);
    let image = dir.join("Oryx-1.2.3-x86_64.AppImage");
    let run = |libs: &Path| {
        Command::new("sh")
            .arg(repo().join("packaging/appimage.sh"))
            .arg(&usr)
            .arg(&image)
            .env("PATH", &path)
            .env("APPIMAGE_RUNTIME", dir.join("no-runtime"))
            .env("APPIMAGE_LIBS", libs)
            .output()
            .unwrap()
    };
    let out = run(&libs);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for soname in KEYBOARD_LIBRARIES {
        let copy = image.join("usr/lib").join(soname);
        assert!(
            copy.is_file() && !copy.is_symlink(),
            "{soname} is a plain file under usr/lib"
        );
        assert_eq!(
            std::fs::read_to_string(&copy).unwrap(),
            format!("{soname} bytes\n")
        );
    }
    // Each library gets a RUNPATH to its own folder, for its own needs,
    // and the binary one to the library folder.
    let calls = std::fs::read_to_string(&record).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(calls.len(), 4, "{calls:?}");
    for (call, soname) in calls.iter().zip(KEYBOARD_LIBRARIES) {
        assert!(
            call.starts_with("--set-rpath $ORIGIN ")
                && call.ends_with(&format!("/usr/lib/{soname}")),
            "{calls:?}"
        );
    }
    assert!(
        calls[3].starts_with("--set-rpath $ORIGIN/../lib ") && calls[3].ends_with("/usr/bin/oryx"),
        "{calls:?}"
    );
    // A library missing from the source folder stops the script before
    // anything is written, and the message names it.
    std::fs::remove_file(libs.join("libxcb-xkb.so.1")).unwrap();
    std::fs::remove_dir_all(&image).unwrap();
    let out = run(&libs);
    assert!(!out.status.success(), "the script stops");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("libxcb-xkb.so.1"), "{stderr}");
    assert!(
        !image.exists(),
        "nothing is written when a library is missing"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `packaging/bump.sh` opens a phase at a new version: the crate line
/// and the lock, a metainfo release line marked development with the
/// day's date above the newest, the screenshot links on the new tag,
/// the changelog heading. Run on a small workspace shaped like the
/// repository's files, with the real metainfo.
#[test]
fn bump_sh_moves_the_version_in_the_four_files_and_refuses_a_step_back() {
    let dir = std::env::temp_dir().join(format!("oryx-bump-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("packaging/linux")).unwrap();
    // The metainfo as a release leaves it: the newest line unmarked.
    let metainfo =
        packaging("linux/com.steerania.Oryx.metainfo.xml").replace(" type=\"development\"", "");
    let (current, _) = metainfo_releases().into_iter().next().unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"oryx\"\nversion = \"{current}\"\nedition = \"2021\"\n\n[dependencies]\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    let xml_path = dir.join("packaging/linux/com.steerania.Oryx.metainfo.xml");
    std::fs::write(&xml_path, &metainfo).unwrap();
    std::fs::write(
        dir.join("CHANGELOG.md"),
        format!("# Changelog\n\n## v{current}\n\n- A bullet.\n"),
    )
    .unwrap();
    let bump = |version: &str| {
        Command::new("sh")
            .arg(repo().join("packaging/bump.sh"))
            .arg(version)
            .arg(&dir)
            .output()
            .unwrap()
    };
    for refused in [current.as_str(), "0.1.0", "1.2", "1.2.3a"] {
        let out = bump(refused);
        assert!(!out.status.success(), "{refused} accepted");
    }
    let next = {
        let mut parts: Vec<u64> = current.split('.').map(|p| p.parse().unwrap()).collect();
        parts[1] += 1;
        parts[2] = 0;
        format!("{}.{}.{}", parts[0], parts[1], parts[2])
    };
    let out = bump(&next);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(toml.contains(&format!("version = \"{next}\"")), "{toml}");
    let lock = std::fs::read_to_string(dir.join("Cargo.lock")).unwrap();
    assert!(
        lock.contains(&format!("name = \"oryx\"\nversion = \"{next}\"")),
        "the lock follows: {lock}"
    );
    let today =
        String::from_utf8(Command::new("date").arg("+%F").output().unwrap().stdout).unwrap();
    let xml = std::fs::read_to_string(&xml_path).unwrap();
    let lines: Vec<&str> = xml.lines().filter(|l| l.contains("<release ")).collect();
    assert_eq!(
        lines[0].trim(),
        format!(
            "<release version=\"{next}\" date=\"{}\" type=\"development\"/>",
            today.trim()
        )
    );
    assert!(
        lines[1].contains(&format!("version=\"{current}\"")),
        "the old newest stays below"
    );
    assert_eq!(
        lines[0].len() - lines[0].trim_start().len(),
        lines[1].len() - lines[1].trim_start().len(),
        "same indent"
    );
    let changelog = std::fs::read_to_string(dir.join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.starts_with(&format!("# Changelog\n\n## v{next}\n\n## v{current}\n")),
        "{changelog}"
    );
    // A phase is open now: a second bump is refused until the release
    // removes the development mark.
    let out = bump("9.9.9");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("development"));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn pkgbuild(package: &str) -> String {
    packaging(&format!("aur/{package}/PKGBUILD"))
}

/// The right-hand side of a `key=value` line of a PKGBUILD.
fn pkgbuild_field<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("{key} missing"))
}

#[test]
fn the_source_pkgbuild_builds_the_tag_of_the_last_release_and_conflicts_with_oryx() {
    let text = pkgbuild("oryx-editor");
    assert_eq!(pkgbuild_field(&text, "pkgname"), "oryx-editor");
    assert_eq!(pkgbuild_field(&text, "pkgver"), last_stable_release());
    assert_eq!(
        pkgbuild_field(&text, "pkgdesc"),
        format!("'{}'", env!("CARGO_PKG_DESCRIPTION"))
    );
    assert_eq!(
        pkgbuild_field(&text, "url"),
        "'https://github.com/wmahfoudh/oryx'"
    );
    assert_eq!(pkgbuild_field(&text, "license"), "('GPL-3.0-only')");
    assert_eq!(pkgbuild_field(&text, "arch"), "('x86_64')");
    assert_eq!(pkgbuild_field(&text, "conflicts"), "('oryx')");
    assert_eq!(
        pkgbuild_field(&text, "depends"),
        "('openssl' 'gcc-libs' 'glibc' 'hicolor-icon-theme' 'libxkbcommon-x11')"
    );
    assert_eq!(pkgbuild_field(&text, "makedepends"), "('cargo' 'librsvg')");
    assert!(
        text.contains("$url/archive/refs/tags/v$pkgver.tar.gz"),
        "the source is the tag tarball"
    );
    assert!(text.contains("--frozen --release"), "the build is frozen");
    assert!(
        text.contains("packaging/stage-linux.sh"),
        "the package is the staged tree"
    );
}

#[test]
fn the_bin_pkgbuild_fetches_the_release_files_and_provides_oryx_editor() {
    let text = pkgbuild("oryx-editor-bin");
    assert_eq!(pkgbuild_field(&text, "pkgname"), "oryx-editor-bin");
    assert_eq!(pkgbuild_field(&text, "pkgver"), last_stable_release());
    assert_eq!(
        pkgbuild_field(&text, "pkgdesc"),
        format!("'{}'", env!("CARGO_PKG_DESCRIPTION"))
    );
    assert_eq!(pkgbuild_field(&text, "license"), "('GPL-3.0-only')");
    assert_eq!(pkgbuild_field(&text, "provides"), "('oryx-editor')");
    assert_eq!(pkgbuild_field(&text, "conflicts"), "('oryx' 'oryx-editor')");
    assert_eq!(
        pkgbuild_field(&text, "depends"),
        "('openssl' 'gcc-libs' 'glibc' 'hicolor-icon-theme' 'libxkbcommon-x11')"
    );
    assert_eq!(pkgbuild_field(&text, "makedepends"), "('librsvg')");
    for source in [
        "$url/releases/download/v$pkgver/oryx-$pkgver-linux-$CARCH.tar.gz",
        "/v$pkgver/packaging/linux/com.steerania.Oryx.desktop",
        "/v$pkgver/packaging/linux/com.steerania.Oryx.metainfo.xml",
        "/v$pkgver/assets/icon/oryx.svg",
        "/v$pkgver/packaging/stage-linux.sh",
    ] {
        assert!(text.contains(source), "{source} missing from the sources");
    }
}

#[test]
fn the_srcinfo_files_are_what_makepkg_prints() {
    if Command::new("makepkg").arg("--version").output().is_err() {
        eprintln!("makepkg is not installed, skipped");
        return;
    }
    for package in ["oryx-editor", "oryx-editor-bin"] {
        let dir = repo().join("packaging/aur").join(package);
        let out = Command::new("makepkg")
            .arg("--printsrcinfo")
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let printed = String::from_utf8_lossy(&out.stdout);
        let file = std::fs::read_to_string(dir.join(".SRCINFO")).unwrap();
        assert_eq!(file, printed, "{package}/.SRCINFO is stale");
    }
}

#[test]
fn the_bin_package_installs_the_staged_tree() {
    for tool in ["makepkg", "fakeroot", "rsvg-convert", "bsdtar"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("{tool} is not installed, skipped");
            return;
        }
    }
    // The sources are named with the PKGBUILD's own version, the last
    // release, which the crate may be ahead of during a phase.
    let text = pkgbuild("oryx-editor-bin");
    let version = pkgbuild_field(&text, "pkgver");
    let dir = std::env::temp_dir().join(format!("oryx-aur-bin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The sources under the names the PKGBUILD downloads them as, which
    // makepkg picks up from the build folder before fetching anything.
    let start = dir.join("start");
    std::fs::create_dir_all(&start).unwrap();
    std::fs::copy(
        repo().join("packaging/aur/oryx-editor-bin/PKGBUILD"),
        start.join("PKGBUILD"),
    )
    .unwrap();
    let release = dir.join("release");
    source_folder(&release.join("oryx"));
    std::fs::write(release.join("oryx/install.sh"), "# per-user\n").unwrap();
    let tarball = start.join(format!("oryx-{version}-linux-x86_64.tar.gz"));
    let out = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&release)
        .arg("oryx")
        .output()
        .unwrap();
    assert!(out.status.success());
    for (name, file) in [
        ("desktop", "packaging/linux/com.steerania.Oryx.desktop"),
        (
            "metainfo.xml",
            "packaging/linux/com.steerania.Oryx.metainfo.xml",
        ),
        ("svg", "assets/icon/oryx.svg"),
        ("stage-linux.sh", "packaging/stage-linux.sh"),
    ] {
        std::fs::copy(
            repo().join(file),
            start.join(format!("oryx-editor-bin-{version}.{name}")),
        )
        .unwrap();
    }
    let out = Command::new("makepkg")
        .args(["-f", "--nodeps", "--skipinteg", "--noconfirm"])
        .current_dir(&start)
        .env("SRCDEST", &start)
        .env("PKGDEST", &dir)
        .env("BUILDDIR", dir.join("build"))
        .env("LOGDEST", dir.join("build"))
        .env("PACKAGER", "tests <tests@example.invalid>")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let package = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("oryx-editor-bin-{version}-1-x86_64.pkg.tar"))
                        && !name.ends_with(".sig")
                })
        })
        .expect("the built package");
    let listing = Command::new("bsdtar")
        .arg("-tf")
        .arg(&package)
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout);
    for file in [
        "usr/bin/oryx",
        "usr/share/applications/com.steerania.Oryx.desktop",
        "usr/share/metainfo/com.steerania.Oryx.metainfo.xml",
        "usr/share/icons/hicolor/512x512/apps/com.steerania.Oryx.png",
        "usr/share/icons/hicolor/scalable/apps/com.steerania.Oryx.svg",
        "usr/share/oryx/themes/dracula.toml",
        "usr/share/oryx/examples/sample.md",
        "usr/share/licenses/oryx-editor/LICENSE",
    ] {
        assert!(
            listing.contains(&format!("{file}\n")),
            "{file} missing:\n{listing}"
        );
    }
    assert!(
        !listing.contains("install.sh"),
        "the per-user script has no place in the package"
    );
    let info = Command::new("bsdtar")
        .args(["-xOf"])
        .arg(&package)
        .arg(".PKGINFO")
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&info.stdout);
    for line in [
        "pkgname = oryx-editor-bin\n",
        &format!("pkgver = {version}-1\n"),
        "provides = oryx-editor\n",
        "conflict = oryx\n",
        "conflict = oryx-editor\n",
        "depend = openssl\n",
        "depend = hicolor-icon-theme\n",
        "license = GPL-3.0-only\n",
    ] {
        assert!(info.contains(line), "{line:?} missing:\n{info}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The three winget manifest templates under `packaging/winget/`, as
/// `channels.sh` fills them: the identifier, the schema version and the
/// texts are fixed, the version, the checksum, the product code and the
/// date are placeholders.
fn winget(name: &str) -> String {
    packaging(&format!("winget/Steerania.Oryx.{name}.yaml"))
}

#[test]
fn the_winget_installer_template_describes_the_msi_at_machine_scope() {
    let text = winget("installer");
    for line in [
        "PackageIdentifier: Steerania.Oryx\n",
        "PackageVersion: \"@VERSION@\"\n",
        "InstallerType: wix\n",
        "Scope: machine\n",
        "ReleaseDate: @RELEASE_DATE@\n",
        "- Architecture: x64\n",
        "  InstallerUrl: https://github.com/wmahfoudh/oryx/releases/download/v@VERSION@/oryx-@VERSION@-windows-x86_64.msi\n",
        "  InstallerSha256: @SHA256@\n",
        "  ProductCode: '@PRODUCT_CODE@'\n",
        "  - UpgradeCode: '{8EA2EE23-91F8-46EC-9310-6DFBF39A04C9}'\n",
        "ManifestType: installer\n",
        "ManifestVersion: 1.12.0\n",
    ] {
        assert!(text.contains(line), "{line:?} missing");
    }
    let extensions: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "FileExtensions:")
        .skip(1)
        .take_while(|line| line.starts_with("- "))
        .map(|line| &line[2..])
        .collect();
    for ext in [
        "md", "markdown", "txt", "epub", "fb2", "fbz", "mobi", "azw3", "azw", "cbz", "cbr",
    ] {
        assert!(
            extensions.contains(&ext),
            "{ext} missing from FileExtensions"
        );
    }
}

#[test]
fn the_winget_locale_template_carries_the_two_texts_and_the_links() {
    let text = winget("locale.en-US");
    for line in [
        "PackageIdentifier: Steerania.Oryx\n",
        "PackageLocale: en-US\n",
        "Publisher: Steerania\n",
        "Author: Walid Mahfoudh\n",
        "PackageName: Oryx\n",
        "PackageUrl: https://github.com/wmahfoudh/oryx\n",
        "License: GPL-3.0-only\n",
        "LicenseUrl: https://github.com/wmahfoudh/oryx/blob/main/LICENSE\n",
        "PrivacyUrl: https://github.com/wmahfoudh/oryx/blob/main/PRIVACY.md\n",
        "ReleaseNotesUrl: https://github.com/wmahfoudh/oryx/releases/tag/v@VERSION@\n",
        "Moniker: oryx\n",
        "ManifestType: defaultLocale\n",
        "ManifestVersion: 1.12.0\n",
    ] {
        assert!(text.contains(line), "{line:?} missing");
    }
    assert!(
        text.contains(&format!(
            "ShortDescription: {}\n",
            env!("CARGO_PKG_DESCRIPTION")
        )),
        "the short line is the crate description"
    );
    let tags = text
        .lines()
        .skip_while(|line| *line != "Tags:")
        .skip(1)
        .take_while(|line| line.starts_with("- "))
        .count();
    assert!(
        (1..=16).contains(&tags),
        "{tags} tags, the schema allows 16"
    );
}

#[test]
fn the_winget_version_template_names_the_default_locale() {
    let text = winget("version");
    assert_eq!(
        text,
        "# yaml-language-server: $schema=https://aka.ms/winget-manifest.version.1.12.0.schema.json\nPackageIdentifier: Steerania.Oryx\nPackageVersion: \"@VERSION@\"\nDefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.12.0\n"
    );
}

/// The Mac's Info.plist: the values under a key, as `<string>` items of
/// the array that follows it, from every occurrence of the key.
fn plist_strings_under(xml: &str, key: &str) -> Vec<String> {
    // The parser refuses a document type declaration; the plist keeps
    // its own for macOS, and the line goes for the read here.
    let xml: String = xml
        .lines()
        .filter(|line| !line.starts_with("<!DOCTYPE"))
        .collect::<Vec<_>>()
        .join("\n");
    let doc = roxmltree::Document::parse(&xml).expect("the plist parses as XML");
    let mut out = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("key")) {
        if node.text() != Some(key) {
            continue;
        }
        let value = node.next_sibling_element().expect("a value after the key");
        if value.has_tag_name("array") {
            out.extend(
                value
                    .children()
                    .filter(|n| n.has_tag_name("string"))
                    .filter_map(|n| n.text())
                    .map(str::to_string),
            );
        } else if let Some(text) = value.text() {
            out.push(text.to_string());
        }
    }
    out
}

#[test]
fn the_mac_plist_claims_every_extension_the_code_opens() {
    let xml = packaging("macos/Info.plist");
    let mut listed = plist_strings_under(&xml, "CFBundleTypeExtensions");
    listed.sort_unstable();
    listed.dedup();
    let mut expected: Vec<String> = oryx::doc::load::recognized_extensions()
        .into_iter()
        .map(str::to_string)
        .collect();
    expected.sort_unstable();
    assert_eq!(listed, expected);
}

#[test]
fn the_mac_plist_names_the_app_and_takes_its_version_from_the_build() {
    let xml = packaging("macos/Info.plist");
    assert_eq!(
        plist_strings_under(&xml, "CFBundleIdentifier"),
        [APP_ID.to_string()]
    );
    assert_eq!(plist_strings_under(&xml, "CFBundleExecutable"), ["oryx"]);
    assert_eq!(plist_strings_under(&xml, "CFBundleIconFile"), ["oryx"]);
    // The script fills the version in from Cargo.toml, so a bump never
    // edits the plist and the two cannot drift apart.
    assert_eq!(
        plist_strings_under(&xml, "CFBundleShortVersionString"),
        ["@VERSION@"]
    );
    assert_eq!(plist_strings_under(&xml, "CFBundleVersion"), ["@VERSION@"]);
    assert!(!xml.contains("@VERSION@ "), "the placeholder stands alone");
}

#[test]
fn the_mac_workflow_runs_the_script_and_uploads_what_it_writes() {
    let workflow = std::fs::read_to_string(repo().join(".github/workflows/macos.yml")).unwrap();
    assert!(workflow.contains("workflow_dispatch"), "started by hand");
    assert!(workflow.contains("sh packaging/macos/build.sh release"));
    assert!(workflow.contains("release/*.dmg"));
    assert!(workflow.contains("release/*.sha256"));
    assert!(
        !workflow.contains("softprops") && !workflow.contains("gh release"),
        "the workflow publishes nothing"
    );
    let script = packaging("macos/build.sh");
    assert!(script.contains("aarch64-apple-darwin") && script.contains("x86_64-apple-darwin"));
    assert!(script.contains("lipo -create"));
    assert!(script.contains("codesign --force --sign -"));
    assert!(script.contains("-macos-universal.dmg"));
    assert!(
        script.contains("trap 'rm -rf \"$stage\"' EXIT"),
        "the staging folder goes on a failure too"
    );
    if let Some(result) = validate("sh", &["-n"], &repo().join("packaging/macos/build.sh")) {
        result.expect("the script parses as POSIX shell");
    }
}

/// Runs the Arch package script on a release folder and returns what it
/// said; the script refuses before any build when something is off.
/// Whether every tool `packaging/arch.sh` gates on answers; the tests
/// that drive the script skip without them.
fn arch_tools_present() -> bool {
    for tool in ["makepkg", "fakeroot", "rsvg-convert", "git"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("{tool} is not installed, skipped");
            return false;
        }
    }
    true
}

fn arch_script(version: &str, release: &Path) -> (bool, String) {
    let out = Command::new("sh")
        .arg(repo().join("packaging/arch.sh"))
        .arg(version)
        .arg(release)
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn the_arch_script_refuses_until_the_release_files_and_the_checksums_exist() {
    if let Some(result) = validate("sh", &["-n"], &repo().join("packaging/arch.sh")) {
        result.expect("the script parses as POSIX shell");
    }
    // The script names a missing tool before anything else, so its
    // refusals are reachable only where the tools are; the AUR source
    // package's own check runs this test in a build chroot without git.
    if !arch_tools_present() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-arch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let text = pkgbuild("oryx-editor-bin");
    let pkgver = pkgbuild_field(&text, "pkgver");
    // A version the PKGBUILD does not carry: the checksums are not
    // there yet, and the script names the step that writes them.
    let (ok, said) = arch_script("0.0.0", &dir);
    assert!(!ok);
    assert!(said.contains("make channels"), "{said}");
    // The PKGBUILD's own version, but no tarball beside it.
    let (ok, said) = arch_script(pkgver, &dir);
    assert!(!ok);
    assert!(said.contains("make release"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A tarball of the PKGBUILD's version that is not the release's fails
/// makepkg's checksum, past the point where the script made its work
/// folder; the folder must be gone all the same. TMPDIR aims mktemp at
/// a folder of the test's own, so what is left there is the script's.
#[test]
fn the_arch_script_leaves_no_work_folder_behind_a_failure() {
    if !arch_tools_present() {
        return;
    }
    let text = pkgbuild("oryx-editor-bin");
    let pkgver = pkgbuild_field(&text, "pkgver");
    let tag = format!("v{pkgver}");
    let tagged = Command::new("git")
        .args([
            "-C",
            &repo().display().to_string(),
            "rev-parse",
            "--verify",
            "--quiet",
        ])
        .arg(format!("{tag}^{{commit}}"))
        .output()
        .is_ok_and(|out| out.status.success());
    if !tagged {
        eprintln!("the tag {tag} is not in this checkout, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oryx-arch-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let release = dir.join("release");
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        release.join(format!("oryx-{pkgver}-linux-x86_64.tar.gz")),
        b"not the release tarball",
    )
    .unwrap();
    let out = Command::new("sh")
        .arg(repo().join("packaging/arch.sh"))
        .arg(pkgver)
        .arg(&release)
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    assert!(!out.status.success(), "the checksum refuses the tarball");
    let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().flatten().collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The toolchain file names the compiler alone. The two cross targets
/// of `make check` and of the Windows build are asked for by the
/// Makefile, with the line that adds them, the way `make audit` asks for
/// its tool, so a checkout that never builds them downloads nothing.
#[test]
fn the_toolchain_file_names_no_target_and_the_makefile_asks_for_them() {
    let toolchain = std::fs::read_to_string(repo().join("rust-toolchain.toml")).unwrap();
    assert!(toolchain.contains("channel = "), "the compiler is named");
    assert!(!toolchain.contains("targets ="), "the file names no target");
    let makefile = std::fs::read_to_string(repo().join("Makefile")).unwrap();
    let guard = |target: &str| {
        format!(
            "@rustup target list --installed | grep -qx {target} || {{ echo \"the target {target} is not installed: rustup target add {target}\"; exit 1; }}"
        )
    };
    let check = makefile
        .split("check:")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .next()
        .unwrap();
    for target in ["x86_64-pc-windows-gnu", "aarch64-apple-darwin"] {
        assert!(
            check.contains(&guard(target)),
            "the check asks for {target}"
        );
    }
    let release = makefile
        .split("release:")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .next()
        .unwrap();
    assert!(
        release.contains(&guard("x86_64-pc-windows-gnu")),
        "the Windows build asks for its target"
    );
}

#[test]
fn the_makefile_builds_the_arch_package_after_the_channels() {
    let makefile = std::fs::read_to_string(repo().join("Makefile")).unwrap();
    assert!(makefile.contains("arch:\n\tsh packaging/arch.sh $(VERSION) release"));
    assert!(makefile.contains(".PHONY: check audit release channels arch"));
}
