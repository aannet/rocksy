//! Atomic file replacement: hidden temporary file in the same directory,
//! fsync, then rename over the destination.
//!
//! `std::fs::rename` replaces an existing destination atomically on both Unix
//! (`rename(2)`) and Windows (`MoveFileExW(MOVEFILE_REPLACE_EXISTING)` /
//! POSIX-semantics rename), so no platform-specific API is needed.

use crate::error::{ApiError, Result};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Suffix of every temporary file created by this module.
pub(crate) const TMP_SUFFIX: &str = ".rocksy-tmp";

/// Removes the temporary file unless the rename succeeded.
struct TmpGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TmpGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn tmp_path(dest: &Path) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApiError::invalid(format!("no file name in {}", dest.display())))?;
    // Keep the temporary name well under the usual 255-byte limit.
    let mut cut = name.len().min(100);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = format!(".{}.{}-{}{}", &name[..cut], std::process::id(), n, TMP_SUFFIX);
    Ok(dest.with_file_name(tmp))
}

fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    // On Windows the destination can be held open for a moment by a sync
    // client, an indexer or an antivirus: retry briefly before giving up.
    let attempts = if cfg!(windows) { 6 } else { 1 };
    let mut last = None;
    for attempt in 0..attempts {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt + 1 < attempts => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(15 * (attempt as u64 + 1)));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
}

/// Writes `bytes` to `dest` atomically.
///
/// `before_rename` runs after the temporary file is fully written and synced,
/// immediately before the rename; returning an error aborts the write and
/// leaves `dest` untouched. The temporary file is removed on every failure
/// path. When `dest` already exists its Unix permissions are kept.
pub(crate) fn write_atomic(
    dest: &Path,
    bytes: &[u8],
    before_rename: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let tmp = tmp_path(dest)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| ApiError::from_io(&e, format!("cannot create temporary file for {}", dest.display())))?;
    let mut guard = TmpGuard { path: tmp.clone(), armed: true };

    let write = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(dest) {
            // Best effort: a rename would otherwise reset the mode to the umask default.
            let _ = file.set_permissions(meta.permissions());
        }
        file.sync_all()
    })();
    drop(file);
    write.map_err(|e| ApiError::from_io(&e, format!("cannot write {}", dest.display())))?;

    before_rename()?;

    rename_with_retry(&tmp, dest)
        .map_err(|e| ApiError::from_io(&e, format!("cannot replace {}", dest.display())))?;
    guard.armed = false;

    // Make the rename itself durable (not possible, and not needed, on Windows).
    #[cfg(unix)]
    if let Some(parent) = dest.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Atomic write into the application data directory, creating it if needed.
pub(crate) fn write_data_file(dest: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| ApiError::from_io(&e, format!("cannot create {}", parent.display())))?;
    }
    write_atomic(dest, bytes, || Ok(()))
}
