//! Atomic file writes: the bytes land whole or not at all. The write
//! goes to a sibling temp file, flushed, then renamed over the target;
//! `std::fs::rename` replaces an existing file on every platform Oryx
//! ships to. The replacement carries the target's permissions, so a
//! saved script stays executable.

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
    let mut file = std::fs::File::create(&temp)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = written {
        std::fs::remove_file(&temp).ok();
        return Err(err);
    }
    // The target's permissions travel onto its replacement: the mode
    // bits on Unix, the read-only flag on Windows. A target that does
    // not exist yet leaves the temp file with a new file's defaults.
    if let Ok(meta) = std::fs::metadata(path) {
        if let Err(err) = std::fs::set_permissions(&temp, meta.permissions()) {
            std::fs::remove_file(&temp).ok();
            return Err(err);
        }
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        std::fs::remove_file(&temp).ok();
    })
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
}
