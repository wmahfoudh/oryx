// Windows resource script content: the exe icon plus the version block
// whose FileDescription names the app in Explorer's Open with menu.
// Shared with `build.rs` through `include!`, so it must stay free of
// crate imports and of inner doc comments.

/// The application name Explorer's Open with menu shows; `--register`
/// writes it as FriendlyAppName. Unused in the build script, which
/// includes this file for the version block alone.
#[allow(dead_code)]
pub const FRIENDLY_APP_NAME: &str = "Oryx";

/// The one-line description carried in the executable's version block,
/// shown in the file's Properties and in Task Manager.
pub const FILE_DESCRIPTION: &str =
    "Fast editor for markdown and code, reader for ebooks and comics";

/// The resource script compiled by windres and linked into the Windows
/// executable. `version_digits` is the comma form (`0,6,0,0`), `version`
/// the display form (`0.6.0`).
pub fn resource_script(ico_path: &str, version_digits: &str, version: &str) -> String {
    format!(
        "1 ICON \"{ico_path}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION {version_digits}\n\
         PRODUCTVERSION {version_digits}\n\
         BEGIN\n\
         BLOCK \"StringFileInfo\"\n\
         BEGIN\n\
         BLOCK \"040904B0\"\n\
         BEGIN\n\
         VALUE \"FileDescription\", \"{FILE_DESCRIPTION}\"\n\
         VALUE \"ProductName\", \"Oryx\"\n\
         VALUE \"FileVersion\", \"{version}\"\n\
         VALUE \"ProductVersion\", \"{version}\"\n\
         VALUE \"InternalName\", \"oryx\"\n\
         VALUE \"OriginalFilename\", \"oryx.exe\"\n\
         END\n\
         END\n\
         BLOCK \"VarFileInfo\"\n\
         BEGIN\n\
         VALUE \"Translation\", 0x409, 0x4B0\n\
         END\n\
         END\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_script_names_the_app() {
        let rc = resource_script("C:/out/oryx.ico", "0,6,0,0", "0.6.0");
        assert!(rc.contains("1 ICON \"C:/out/oryx.ico\""));
        assert!(rc.contains("1 VERSIONINFO"));
        assert!(rc.contains("FILEVERSION 0,6,0,0"));
        assert!(rc.contains("PRODUCTVERSION 0,6,0,0"));
        assert!(rc.contains(&format!(
            "VALUE \"FileDescription\", \"{FILE_DESCRIPTION}\""
        )));
        assert!(rc.contains("VALUE \"ProductName\", \"Oryx\""));
        assert!(rc.contains("VALUE \"FileVersion\", \"0.6.0\""));
        assert!(rc.contains("VALUE \"OriginalFilename\", \"oryx.exe\""));
        assert!(rc.contains("VALUE \"Translation\", 0x409, 0x4B0"));
    }
}
