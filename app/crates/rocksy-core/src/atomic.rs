//! Atomic file replacement: hidden temporary file in the same directory,
//! fsync, then rename over the destination.
//!
//! `std::fs::rename` replaces an existing destination atomically on both Unix
//! (`rename(2)`) and Windows (`MoveFileExW(MOVEFILE_REPLACE_EXISTING)` /
//! POSIX-semantics rename), so no platform-specific API is needed. Only the
//! decision to retry a failed rename is specific to Windows.

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

/// How long a rename that fails for a transient reason keeps being retried.
///
/// On Windows the destination can be held open for a moment by a sync client,
/// an indexer or an antivirus. A note being saved is worth waiting for; the
/// application's own data files are not (they are rewritten later anyway, and
/// some are written while the process exits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryPolicy {
    /// Upper bound of the total time slept between attempts.
    budget: Duration,
}

impl RetryPolicy {
    /// Saving a user file: about two seconds.
    pub(crate) const USER_FILE: Self = Self { budget: Duration::from_millis(2000) };
    /// Files of the application data directory: a short window.
    pub(crate) const DATA_FILE: Self = Self { budget: Duration::from_millis(225) };

    const FIRST_DELAY: Duration = Duration::from_millis(25);
    const MAX_DELAY: Duration = Duration::from_millis(400);
}

/// `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`: another process
/// holds the file. Rust maps neither to `PermissionDenied`.
#[cfg(windows)]
const WINDOWS_TRANSIENT_CODES: [i32; 2] = [32, 33];

/// Whether a failed rename is worth another attempt.
#[cfg(windows)]
fn is_transient_rename_error(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied
        || e.raw_os_error().is_some_and(|code| WINDOWS_TRANSIENT_CODES.contains(&code))
}

/// Whether a failed rename is worth another attempt: never outside Windows,
/// where a refused rename is not a transient lock.
#[cfg(not(windows))]
fn is_transient_rename_error(_e: &io::Error) -> bool {
    false
}

/// The operations of the retry loop that tests replace.
struct RenameOps<'a> {
    rename: &'a mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    is_transient: &'a dyn Fn(&io::Error) -> bool,
    sleep: &'a mut dyn FnMut(Duration),
}

/// Renames `from` over `to`, retrying transient failures with a growing delay
/// until the budget of `policy` is spent.
///
/// `before_attempt` runs immediately before **every** attempt, the first one
/// included: an external write that completed during a pause must be seen
/// before the next rename would replace it.
fn rename_with_retry(
    from: &Path,
    to: &Path,
    policy: RetryPolicy,
    before_attempt: &mut dyn FnMut() -> Result<()>,
    ops: &mut RenameOps<'_>,
) -> Result<()> {
    let mut slept = Duration::ZERO;
    let mut delay = RetryPolicy::FIRST_DELAY;
    loop {
        before_attempt()?;
        match (ops.rename)(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if (ops.is_transient)(&e) && slept < policy.budget => {
                let pause = delay.min(policy.budget - slept);
                (ops.sleep)(pause);
                slept += pause;
                delay = (delay * 2).min(RetryPolicy::MAX_DELAY);
            }
            Err(e) => {
                return Err(ApiError::from_io(&e, format!("cannot replace {}", to.display())));
            }
        }
    }
}

/// Writes `bytes` to `dest` atomically.
///
/// `before_rename` runs after the temporary file is fully written and synced,
/// immediately before the rename, and again before every new attempt when the
/// rename is retried; returning an error aborts the write and leaves `dest`
/// untouched. The temporary file is removed on every failure path. When
/// `dest` already exists its Unix permissions are kept.
pub(crate) fn write_atomic(
    dest: &Path,
    bytes: &[u8],
    policy: RetryPolicy,
    mut before_rename: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut ops = RenameOps {
        rename: &mut |from, to| fs::rename(from, to),
        is_transient: &is_transient_rename_error,
        sleep: &mut |pause| std::thread::sleep(pause),
    };
    write_atomic_with(dest, bytes, policy, &mut before_rename, &mut ops)
}

fn write_atomic_with(
    dest: &Path,
    bytes: &[u8],
    policy: RetryPolicy,
    before_rename: &mut dyn FnMut() -> Result<()>,
    ops: &mut RenameOps<'_>,
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

    rename_with_retry(&tmp, dest, policy, before_rename, ops)?;
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
    write_atomic(dest, bytes, RetryPolicy::DATA_FILE, || Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};
    use std::cell::{Cell, RefCell};

    fn locked() -> io::Error {
        io::Error::from(io::ErrorKind::PermissionDenied)
    }

    /// Runs `write_atomic_with` with a rename that fails `failures` times
    /// (calling `on_failure` each time) before doing the real rename.
    /// Returns the result and the pauses that were requested.
    fn write_with_failures(
        dest: &Path,
        policy: RetryPolicy,
        failures: usize,
        before_rename: &mut dyn FnMut() -> Result<()>,
        on_failure: &dyn Fn(),
    ) -> (Result<()>, Vec<Duration>) {
        let calls = Cell::new(0usize);
        let pauses = RefCell::new(Vec::new());
        let res = {
            let mut rename = |from: &Path, to: &Path| {
                calls.set(calls.get() + 1);
                if calls.get() <= failures {
                    on_failure();
                    Err(locked())
                } else {
                    fs::rename(from, to)
                }
            };
            let mut sleep = |pause: Duration| pauses.borrow_mut().push(pause);
            let mut ops = RenameOps {
                rename: &mut rename,
                is_transient: &|e: &io::Error| e.kind() == io::ErrorKind::PermissionDenied,
                sleep: &mut sleep,
            };
            write_atomic_with(dest, b"mine", policy, before_rename, &mut ops)
        };
        (res, pauses.into_inner())
    }

    #[test]
    fn external_write_during_a_retry_pause_is_a_conflict() {
        let dir = TempDir::new("retry-conflict");
        let dest = dir.write("note.md", "v1");
        // Stand-in for the mtime comparison of `save_file`.
        let mut unchanged = || -> Result<()> {
            if fs::read(&dest).unwrap() == b"v1" {
                Ok(())
            } else {
                Err(ApiError::conflict("changed on disk"))
            }
        };
        // The first rename fails because another process holds the file; that
        // process finishes its own write before the next attempt.
        let external = || fs::write(&dest, "external").unwrap();
        let (res, pauses) =
            write_with_failures(&dest, RetryPolicy::USER_FILE, 1, &mut unchanged, &external);

        assert_eq!(res.unwrap_err().kind, ErrorKind::Conflict);
        assert_eq!(fs::read(&dest).unwrap(), b"external", "the external write must survive");
        assert_eq!(pauses.len(), 1);
        assert_eq!(all_files(dir.path()), vec!["note.md"], "temporary file removed");
    }

    #[test]
    fn check_runs_before_every_attempt_and_retry_succeeds() {
        let dir = TempDir::new("retry-ok");
        let dest = dir.write("note.md", "v1");
        let checks = Cell::new(0);
        let mut check = || {
            checks.set(checks.get() + 1);
            Ok(())
        };
        let (res, pauses) = write_with_failures(&dest, RetryPolicy::USER_FILE, 3, &mut check, &|| ());
        res.unwrap();
        assert_eq!(checks.get(), 4, "one check per attempt");
        assert_eq!(fs::read(&dest).unwrap(), b"mine");
        assert_eq!(pauses, [25, 50, 100].map(Duration::from_millis));
        assert_eq!(all_files(dir.path()), vec!["note.md"]);
    }

    #[test]
    fn retry_window_is_about_two_seconds_for_notes_and_short_for_data_files() {
        let dir = TempDir::new("retry-budget");
        let dest = dir.write("note.md", "v1");
        for (policy, budget_ms) in [(RetryPolicy::USER_FILE, 2000), (RetryPolicy::DATA_FILE, 225)] {
            let (res, pauses) = write_with_failures(&dest, policy, usize::MAX, &mut || Ok(()), &|| ());
            assert_eq!(res.unwrap_err().kind, ErrorKind::Io);
            assert_eq!(pauses.iter().sum::<Duration>(), Duration::from_millis(budget_ms));
            // The last pause is cut to what is left of the budget.
            let full = &pauses[..pauses.len() - 1];
            assert!(full.windows(2).all(|w| w[1] >= w[0]) && full[1] > full[0], "growing delay: {pauses:?}");
            assert!(pauses.iter().all(|p| *p <= RetryPolicy::MAX_DELAY));
            assert_eq!(fs::read(&dest).unwrap(), b"v1");
            assert_eq!(all_files(dir.path()), vec!["note.md"]);
        }
    }

    #[test]
    fn permanent_error_is_not_retried() {
        let dir = TempDir::new("retry-permanent");
        let dest = dir.write("note.md", "v1");
        let mut rename = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::InvalidInput));
        let mut sleep = |_: Duration| panic!("no pause expected");
        let mut ops = RenameOps { rename: &mut rename, is_transient: &|_| false, sleep: &mut sleep };
        let err = write_atomic_with(&dest, b"mine", RetryPolicy::USER_FILE, &mut || Ok(()), &mut ops)
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Io);
        assert_eq!(all_files(dir.path()), vec!["note.md"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn nothing_is_transient_outside_windows() {
        assert!(!is_transient_rename_error(&locked()));
        assert!(!is_transient_rename_error(&io::Error::from_raw_os_error(32)));
    }

    #[cfg(windows)]
    #[test]
    fn sharing_and_lock_violations_are_transient_on_windows() {
        assert!(is_transient_rename_error(&locked()));
        assert!(is_transient_rename_error(&io::Error::from_raw_os_error(32)));
        assert!(is_transient_rename_error(&io::Error::from_raw_os_error(33)));
        assert!(!is_transient_rename_error(&io::Error::from_raw_os_error(2)));
    }
}
