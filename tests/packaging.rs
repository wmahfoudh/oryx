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

/// The screenshot links carry the tag of the release the metainfo ships
/// in, never a branch: AppStream readers fetch them long after the branch
/// has moved on, and Flathub refuses branch links outright. The bump step
/// rewrites the tag; the files must exist in the tree.
#[test]
fn the_metainfo_screenshots_link_to_the_release_tag_and_exist() {
    let xml = packaging("linux/com.steerania.Oryx.metainfo.xml");
    let prefix = format!(
        "https://raw.githubusercontent.com/wmahfoudh/oryx/v{}/screenshots/",
        env!("CARGO_PKG_VERSION")
    );
    let names: Vec<&str> = xml
        .split("<image>")
        .skip(1)
        .map(|rest| rest.split("</image>").next().unwrap())
        .map(|url| {
            url.strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("{url} is not under {prefix}"))
        })
        .collect();
    assert_eq!(
        names,
        [
            "flathub-hero.png",
            "flathub-math.png",
            "flathub-code.png",
            "flathub-regex.png",
            "flathub-rtl-ar.png",
            "flathub-themes-editor.png",
        ]
    );
    for name in names {
        assert!(
            repo().join("screenshots").join(name).is_file(),
            "screenshots/{name} is missing"
        );
    }
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
        "Depends: libc6 (>= 2.35), libssl3 (>= 3.0.0)\n",
        "Homepage: https://github.com/wmahfoudh/oryx\n",
        "Description: Fast editor for markdown and code, reader for ebooks and comics\n",
    ] {
        assert!(control.contains(line), "{line:?} missing:\n{control}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_rpm_requires_libssl_3_and_carries_the_metadata() {
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
    assert!(requires.contains("libssl.so.3()(64bit)\n"), "{requires}");
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
    let out = command.output().unwrap();
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

#[test]
fn appimage_sh_packs_the_staged_tree_with_the_entry_and_the_icon_on_top() {
    for tool in ["appimagetool", "rsvg-convert"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("{tool} is not installed, skipped");
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
    let image = dir.join("Oryx-1.2.3-x86_64.AppImage");
    let out = Command::new("sh")
        .arg(repo().join("packaging/appimage.sh"))
        .arg(&usr)
        .arg(&image)
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
fn the_source_pkgbuild_builds_the_tag_at_the_crate_version_and_conflicts_with_oryx() {
    let text = pkgbuild("oryx-editor");
    assert_eq!(pkgbuild_field(&text, "pkgname"), "oryx-editor");
    assert_eq!(pkgbuild_field(&text, "pkgver"), env!("CARGO_PKG_VERSION"));
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
        "('openssl' 'gcc-libs' 'glibc' 'hicolor-icon-theme')"
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
    assert_eq!(pkgbuild_field(&text, "pkgver"), env!("CARGO_PKG_VERSION"));
    assert_eq!(
        pkgbuild_field(&text, "pkgdesc"),
        format!("'{}'", env!("CARGO_PKG_DESCRIPTION"))
    );
    assert_eq!(pkgbuild_field(&text, "license"), "('GPL-3.0-only')");
    assert_eq!(pkgbuild_field(&text, "provides"), "('oryx-editor')");
    assert_eq!(pkgbuild_field(&text, "conflicts"), "('oryx' 'oryx-editor')");
    assert_eq!(
        pkgbuild_field(&text, "depends"),
        "('openssl' 'gcc-libs' 'glibc' 'hicolor-icon-theme')"
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
    let version = env!("CARGO_PKG_VERSION");
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

#[test]
fn the_flathub_manifest_builds_the_tag_offline_into_app() {
    let text = packaging("flathub/com.steerania.Oryx.yml");
    for line in [
        &format!("app-id: {APP_ID}\n"),
        "runtime: org.freedesktop.Platform\n",
        "runtime-version: '25.08'\n",
        "sdk: org.freedesktop.Sdk\n",
        "  - org.freedesktop.Sdk.Extension.rust-stable\n",
        "command: oryx\n",
        "  - --socket=wayland\n",
        "  - --socket=fallback-x11\n",
        "  - --share=ipc\n",
        "  - --share=network\n",
        "  - --filesystem=home\n",
        "      - cargo --offline build --release --verbose\n",
        "      - sh packaging/stage-linux.sh stage /app\n",
        &format!(
            "        url: https://github.com/wmahfoudh/oryx/archive/refs/tags/v{}.tar.gz\n",
            env!("CARGO_PKG_VERSION")
        ),
        "      - cargo-sources.json\n",
    ] {
        assert!(text.contains(line), "{line:?} missing");
    }
}

/// `cargo-sources.json` is regenerated from `Cargo.lock` by Flathub's
/// generator, cloned beside the repository; a stale file would make the
/// offline build miss a crate. Skipped when the generator or its
/// Python modules are absent.
#[test]
fn the_flathub_cargo_sources_match_the_lock_file() {
    let generator = repo()
        .parent()
        .unwrap()
        .join("flatpak-builder-tools/cargo/flatpak-cargo-generator.py");
    if !generator.is_file() {
        eprintln!("flatpak-builder-tools is not cloned beside the repository, skipped");
        return;
    }
    if Command::new("python3")
        .args(["-c", "import aiohttp, toml"])
        .output()
        .map(|out| !out.status.success())
        .unwrap_or(true)
    {
        eprintln!("python3 with aiohttp and toml is not installed, skipped");
        return;
    }
    let out_path =
        std::env::temp_dir().join(format!("oryx-cargo-sources-{}.json", std::process::id()));
    let out = Command::new("python3")
        .arg(&generator)
        .arg(repo().join("Cargo.lock"))
        .arg("-o")
        .arg(&out_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fresh = std::fs::read_to_string(&out_path).unwrap();
    let committed = packaging("flathub/cargo-sources.json");
    std::fs::remove_file(&out_path).unwrap();
    assert_eq!(
        committed, fresh,
        "packaging/flathub/cargo-sources.json is stale: regenerate it with make channels"
    );
}
