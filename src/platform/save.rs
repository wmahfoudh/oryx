//! File writes that land whole: the bytes go to a sibling temp file,
//! flushed, then renamed over the target; `std::fs::rename` replaces
//! an existing file on every platform Oryx ships to. The replacement
//! carries the target's permissions, so a saved script stays
//! executable. Three targets are written in place instead, truncated
//! then filled: one whose folder refuses a new file, one with another
//! hard link, which a rename would break, and one owned by someone
//! else, which a rename would hand to the writer (the last two on
//! Unix, where links and owners are).

use std::fs::Metadata;
use std::io;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    // The temp name is unique per process and per call, so two writes
    // into one folder at the same moment never rename each other's
    // bytes over their targets.
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let temp = dir
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".oryx-save-{}-{serial}", std::process::id()));
    let mut file = match std::fs::File::create(&temp) {
        Ok(file) => file,
        // The folder refuses a new file; the file itself may still
        // take the bytes.
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            return write_in_place(path, bytes);
        }
        Err(err) => return Err(err),
    };
    let target = std::fs::metadata(path).ok();
    if let Some(target) = &target {
        let ours = match file.metadata() {
            Ok(ours) => ours,
            Err(err) => {
                drop(file);
                std::fs::remove_file(&temp).ok();
                return Err(err);
            }
        };
        if keeps_its_inode(target, &ours) {
            drop(file);
            std::fs::remove_file(&temp).ok();
            return write_in_place(path, bytes);
        }
    }
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = written {
        std::fs::remove_file(&temp).ok();
        return Err(err);
    }
    // The target's permissions travel onto its replacement: the mode
    // bits on Unix, the read-only flag on Windows. A target that does
    // not exist yet leaves the temp file with a new file's defaults.
    if let Some(target) = target {
        if let Err(err) = std::fs::set_permissions(&temp, target.permissions()) {
            std::fs::remove_file(&temp).ok();
            return Err(err);
        }
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        std::fs::remove_file(&temp).ok();
    })
}

/// Whether the target must keep its inode, so that the bytes go into
/// it rather than into a replacement: it has another hard link, or its
/// owner is not the writer. `ours` is a file the writer just made,
/// which tells the writer's id without a call of its own.
#[cfg(unix)]
fn keeps_its_inode(target: &Metadata, ours: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    target.nlink() > 1 || target.uid() != ours.uid()
}

/// Windows has no hard link a rename breaks that Oryx meets, and no
/// owner a rename changes: the replacement is always right.
#[cfg(not(unix))]
fn keeps_its_inode(_target: &Metadata, _ours: &Metadata) -> bool {
    false
}

/// The bytes into the file itself, truncated first: a death between
/// the two leaves it short, the price of a target a rename cannot
/// replace.
fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_replaces_the_target_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("oryx-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("note.txt");
        std::fs::write(&target, b"old contents").unwrap();
        write_atomic(&target, b"new contents").expect("the write lands");
        assert_eq!(std::fs::read(&target).unwrap(), b"new contents");
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec!["note.txt"], "no temp file survives");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_write_keeps_the_targets_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oryx-save-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("run.sh");
        std::fs::write(&target, b"#!/bin/sh\necho old\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        write_atomic(&target, b"#!/bin/sh\necho new\n").expect("the write lands");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "the executable bits survive the save");
        assert_eq!(std::fs::read(&target).unwrap(), b"#!/bin/sh\necho new\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_write_creates_a_missing_target() {
        let dir = std::env::temp_dir().join(format!("oryx-save-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("fresh.txt");
        write_atomic(&target, b"born whole").expect("the write lands");
        assert_eq!(std::fs::read(&target).unwrap(), b"born whole");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file you can write in a folder you cannot write to is written
    /// in place, since no temp file can be made beside it (Task 340,
    /// claim 35 of the hunt of 1.3.0).
    #[cfg(unix)]
    #[test]
    fn a_file_in_a_folder_that_refuses_a_sibling_is_written_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oryx-save-sealed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("note.txt");
        std::fs::write(&target, b"old contents").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let written = write_atomic(&target, b"new contents");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        written.expect("written in place");
        assert_eq!(std::fs::read(&target).unwrap(), b"new contents");
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec!["note.txt"], "no temp file beside it");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file with another hard link keeps the link: the other name
    /// reads the new text too, where a rename left it the old text.
    #[cfg(unix)]
    #[test]
    fn a_file_with_another_hard_link_is_written_in_place() {
        use std::os::unix::fs::MetadataExt;
        let dir = std::env::temp_dir().join(format!("oryx-save-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("a.txt");
        let other = dir.join("b.txt");
        std::fs::write(&target, b"old contents").unwrap();
        std::fs::hard_link(&target, &other).unwrap();
        write_atomic(&target, b"new contents").expect("written in place");
        assert_eq!(
            std::fs::read(&other).unwrap(),
            b"new contents",
            "the link kept"
        );
        assert_eq!(std::fs::metadata(&target).unwrap().nlink(), 2);
        let ours = std::fs::metadata(&dir).unwrap();
        assert!(
            keeps_its_inode(&std::fs::metadata(&target).unwrap(), &ours),
            "two names"
        );
        std::fs::remove_file(&other).unwrap();
        assert!(
            !keeps_its_inode(&std::fs::metadata(&target).unwrap(), &ours),
            "one name, the same owner: the rename path"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
