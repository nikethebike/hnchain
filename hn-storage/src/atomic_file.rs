//! Generic, durable single-file write primitive: write to a temp file
//! next to the real path, fsync it, rename it over the real path, then
//! (on platforms that support it) fsync the containing directory so the
//! rename itself survives a crash, not just the file's own content.
//!
//! Nothing here is specific to any one caller's file format — it is
//! ADR-0048's own "Decided: Vote Journal (Write-Ahead Safety State)",
//! rule 1 ("written atomically: temp file, fsync, rename, fsync
//! directory"), factored out as a plain bytes-in/bytes-out primitive so
//! any future durable single-file record can reuse it without pulling in
//! `hn-consensus`'s own types.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` durably. A process that stops at any point
/// before the rename below leaves whatever was already at `path`
/// (missing or present) completely untouched — the write either lands
/// in full or not at all, never partially.
///
/// Steps: create a temp file next to `path`, write `bytes`, `fsync` the
/// temp file, rename it over `path` (atomic on both Windows and POSIX
/// filesystems), then `fsync` the containing directory. The directory
/// fsync is skipped on non-Unix targets: Windows has no equivalent of
/// opening a directory as a file to fsync its entry table, so there the
/// rename's own durability relies on the filesystem/OS cache alone —
/// documented here, not silently assumed equivalent to the Unix path.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let temp_path = temp_path_for(path);

    {
        let mut temp_file = File::create(&temp_path)?;
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
    }
    fs::rename(&temp_path, path)?;

    if let Some(dir) = dir {
        fsync_directory(dir)?;
    }

    Ok(())
}

#[cfg(unix)]
fn fsync_directory(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn fsync_directory(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Reads `path`'s full contents, or `None` if it does not exist. Any
/// other I/O error (permission denied, a directory where a file was
/// expected, ...) propagates — it is never safe to treat an error other
/// than "not found" as "missing".
pub fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut temp_name = path.file_name().map_or_else(Default::default, |name| {
        let mut owned = name.to_os_string();
        owned.push(".tmp");
        owned
    });
    if temp_name.is_empty() {
        temp_name.push(".tmp");
    }
    path.with_file_name(temp_name)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{read_optional, write_atomically};

    #[test]
    fn write_then_read_round_trips() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("journal");

        write_atomically(&path, b"hello")?;

        assert_eq!(read_optional(&path)?, Some(b"hello".to_vec()));
        Ok(())
    }

    #[test]
    fn read_optional_is_none_for_a_missing_file() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("does-not-exist");

        assert_eq!(read_optional(&path)?, None);
        Ok(())
    }

    #[test]
    fn a_second_write_fully_replaces_the_first() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("journal");

        write_atomically(&path, b"first")?;
        write_atomically(&path, b"second, shorter")?;

        assert_eq!(read_optional(&path)?, Some(b"second, shorter".to_vec()));
        Ok(())
    }

    #[test]
    fn no_temp_file_is_left_behind_after_a_successful_write() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("journal");

        write_atomically(&path, b"hello")?;

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir.path())? {
            entries.push(entry?.file_name());
        }
        assert_eq!(entries, vec![std::ffi::OsString::from("journal")]);
        Ok(())
    }

    /// Simulates a crash between the temp-file write and the rename
    /// (ADR-0048 rule 1: "a crash between write and send loses a vote,
    /// which is harmless"): the real path must still hold whatever was
    /// there before, completely untouched, and a stale leftover temp
    /// file from that crash must not corrupt the *next* write.
    #[test]
    fn a_crash_before_rename_leaves_the_previous_file_untouched() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("journal");
        let temp_path = super::temp_path_for(&path);

        write_atomically(&path, b"committed")?;

        // Simulate the crash: the temp file for a *next* write got
        // created and fsynced, but the process died before the rename.
        std::fs::write(&temp_path, b"half-written-next-value")?;

        assert_eq!(
            read_optional(&path)?,
            Some(b"committed".to_vec()),
            "a leftover temp file must never be mistaken for the real one"
        );

        // The next real write must still succeed despite the leftover.
        write_atomically(&path, b"recovered")?;
        assert_eq!(read_optional(&path)?, Some(b"recovered".to_vec()));
        Ok(())
    }
}
