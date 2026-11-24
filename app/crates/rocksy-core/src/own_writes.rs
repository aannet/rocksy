//! Registry of the files Rocksy has just saved itself.
//!
//! The folder watcher reports every change under the vault, the app's own
//! saves included; answering each of them with a full listing of the vault
//! is wasted work. `save_file` records what it wrote here, and the refresh
//! worker asks [`OwnWrites::only_own_writes`] whether a set of changed paths
//! consists of nothing but those saves, still intact on disk. Only then may
//! the listing be skipped. Every doubt answers "no": unknown path, folder,
//! deleted file, different size or mtime, expired record, incomplete batch,
//! file created by the save (the tree has to learn about it).
//!
//! Temporary files of the atomic save start with `.`: the watcher drops
//! hidden paths, so they never appear in a batch. If one ever did, it would
//! be an unknown path and the listing would run.

use crate::paths::{mtime_ms, path_key_for};
use crate::watch::ChangeBatch;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a record stays valid. A batch reaches the worker at most
/// `max_wait_ms` (3 s) after the save, plus the worker's own pause (1 s) and
/// the pass in progress.
pub const DEFAULT_TTL: Duration = Duration::from_secs(10);

/// Most records kept; the oldest are dropped first.
pub const DEFAULT_CAPACITY: usize = 256;

/// Most paths kept when merging change sets; beyond that the set is unknown.
pub const MAX_MERGED_PATHS: usize = 4096;

/// Size and modification time of a regular file, as `save_file` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub mtime_ms: u64,
}

/// `stat` of one path: `None` unless it is an existing regular file.
pub fn stat_file(path: &str) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    meta.is_file().then(|| FileStamp { size: meta.len(), mtime_ms: mtime_ms(&meta) })
}

/// Paths reported by one or several watcher batches, merged by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedPaths {
    /// The list is not known or not complete: everything must be checked.
    Unknown,
    /// Exactly these paths changed (never empty).
    Paths(Vec<String>),
}

impl ChangedPaths {
    /// Change set of a request that does not come from a watcher batch
    /// (focus safety net, vault switch).
    pub fn unknown() -> Self {
        Self::Unknown
    }

    /// Change set of one batch: unknown if the batch is truncated or lists
    /// no path.
    pub fn from_batch(batch: &ChangeBatch) -> Self {
        if batch.truncated {
            Self::Unknown
        } else {
            Self::from_paths(batch.paths.clone())
        }
    }

    /// A list of paths; an empty list is unknown.
    pub fn from_paths(paths: Vec<String>) -> Self {
        if paths.is_empty() || paths.len() > MAX_MERGED_PATHS {
            Self::Unknown
        } else {
            Self::Paths(paths)
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Adds the changes of another request. Unknown absorbs everything.
    pub fn merge(&mut self, other: ChangedPaths) {
        let merged = match (std::mem::replace(self, Self::Unknown), other) {
            (Self::Paths(mut a), Self::Paths(b)) => {
                a.extend(b);
                Self::from_paths(a)
            }
            _ => Self::Unknown,
        };
        *self = merged;
    }
}

struct Record {
    stamp: FileStamp,
    at: Instant,
}

#[derive(Default)]
struct Inner {
    records: HashMap<String, Record>,
    /// A save created a file at this instant and no listing started since:
    /// the tree is missing an entry, nothing may be skipped.
    created_at: Option<Instant>,
}

/// Thread-safe registry (interior lock, never held during a `stat`).
pub struct OwnWrites {
    inner: Mutex<Inner>,
    ttl: Duration,
    capacity: usize,
    windows: bool,
}

impl Default for OwnWrites {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnWrites {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_TTL, DEFAULT_CAPACITY)
    }

    /// Registry with its own expiry delay and size (at least one record).
    pub fn with_limits(ttl: Duration, capacity: usize) -> Self {
        Self::with_style(ttl, capacity, cfg!(windows))
    }

    fn with_style(ttl: Duration, capacity: usize, windows: bool) -> Self {
        Self { inner: Mutex::new(Inner::default()), ttl, capacity: capacity.max(1), windows }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn key(&self, path: &str) -> String {
        path_key_for(path, self.windows)
    }

    /// Records a successful `save_file`: call it after the save returned
    /// `Ok`, never on an error. `created` says the file did not exist before
    /// the save; the tree then has to be listed again, and nothing is
    /// skipped until [`OwnWrites::listing_started_at`] reports a listing
    /// begun after this call.
    pub fn record(&self, path: &str, size: u64, mtime_ms: u64, created: bool) {
        self.record_at(path, FileStamp { size, mtime_ms }, created, Instant::now());
    }

    /// [`OwnWrites::record`] with an explicit clock.
    pub fn record_at(&self, path: &str, stamp: FileStamp, created: bool, now: Instant) {
        let key = self.key(path);
        let mut inner = self.lock();
        let ttl = self.ttl;
        inner.records.retain(|_, r| now.saturating_duration_since(r.at) < ttl);
        if created {
            inner.created_at = Some(now);
        }
        inner.records.insert(key, Record { stamp, at: now });
        while inner.records.len() > self.capacity {
            let Some(oldest) = inner.records.iter().min_by_key(|(_, r)| r.at).map(|(k, _)| k.clone()) else {
                break;
            };
            inner.records.remove(&oldest);
        }
    }

    /// Tells the registry that a full listing of the vault began at
    /// `started` (take the instant before calling `reconcile_tree`) and
    /// completed. Files created by saves recorded before that instant are now
    /// in the tree.
    pub fn listing_started_at(&self, started: Instant) {
        let mut inner = self.lock();
        if inner.created_at.is_some_and(|created| created <= started) {
            inner.created_at = None;
        }
    }

    /// Drops the record of `path` (for instance when a later save failed).
    pub fn forget(&self, path: &str) {
        let key = self.key(path);
        self.lock().records.remove(&key);
    }

    /// Number of records, expired ones included until the next `record`.
    pub fn len(&self) -> usize {
        self.lock().records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True if `changes` contains nothing but saves made by the app that are
    /// still intact on disk: the listing of the vault can be skipped.
    /// Each path that matches a record costs one `stat`; nothing else is
    /// touched, and no `stat` happens at all when any path is unknown.
    pub fn only_own_writes(&self, changes: &ChangedPaths) -> bool {
        self.only_own_writes_at(changes, Instant::now(), stat_file)
    }

    /// [`OwnWrites::only_own_writes`] with an explicit clock and `stat`.
    pub fn only_own_writes_at(
        &self,
        changes: &ChangedPaths,
        now: Instant,
        stat: impl Fn(&str) -> Option<FileStamp>,
    ) -> bool {
        let ChangedPaths::Paths(paths) = changes else { return false };
        if paths.is_empty() {
            return false;
        }
        // Expected stamps are copied out so that the lock is not held while
        // the file system is queried (a `stat` can be slow on a synced drive).
        let mut expected: HashMap<String, (&str, FileStamp)> = HashMap::new();
        {
            let inner = self.lock();
            if inner.created_at.is_some() {
                return false;
            }
            for path in paths {
                let key = self.key(path);
                match inner.records.get(&key) {
                    Some(r) if now.saturating_duration_since(r.at) < self.ttl => {
                        expected.entry(key).or_insert((path.as_str(), r.stamp));
                    }
                    _ => return false,
                }
            }
        }
        expected.values().all(|(path, stamp)| stat(path) == Some(*stamp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(10);

    fn stamp(size: u64, mtime_ms: u64) -> FileStamp {
        FileStamp { size, mtime_ms }
    }

    fn paths(list: &[&str]) -> ChangedPaths {
        ChangedPaths::from_paths(list.iter().map(|s| s.to_string()).collect())
    }

    /// `stat` answering from a fixed table.
    fn disk<'a>(files: &'a [(&'a str, FileStamp)]) -> impl Fn(&str) -> Option<FileStamp> + 'a {
        move |p| files.iter().find(|(f, _)| *f == p).map(|(_, s)| *s)
    }

    fn unix() -> OwnWrites {
        OwnWrites::with_style(TTL, 8, false)
    }

    #[test]
    fn own_save_is_ignorable_while_the_file_is_intact() {
        let w = unix();
        let t0 = Instant::now();
        w.record_at("/v/a.md", stamp(10, 1000), false, t0);
        let later = t0 + Duration::from_millis(500);
        assert!(w.only_own_writes_at(&paths(&["/v/a.md"]), later, disk(&[("/v/a.md", stamp(10, 1000))])));
        // The record is not consumed: several batches may report one save.
        assert!(w.only_own_writes_at(&paths(&["/v/a.md", "/v/a.md"]), later, disk(&[("/v/a.md", stamp(10, 1000))])));
        // Changed since: other mtime, other size, deleted, replaced by a folder (stat gives None).
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), later, disk(&[("/v/a.md", stamp(10, 1001))])));
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), later, disk(&[("/v/a.md", stamp(11, 1000))])));
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), later, disk(&[])));
    }

    #[test]
    fn any_other_path_makes_the_set_not_ignorable_without_any_stat() {
        let w = unix();
        let t0 = Instant::now();
        w.record_at("/v/a.md", stamp(10, 1000), false, t0);
        let no_stat = |_: &str| -> Option<FileStamp> { panic!("no stat expected") };
        // Mixed batch, folder (BUG-8: a moved folder is never an own write), temporary file.
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md", "/v/b.md"]), t0, no_stat));
        assert!(!w.only_own_writes_at(&paths(&["/v/dir", "/v/a.md"]), t0, no_stat));
        assert!(!w.only_own_writes_at(&paths(&["/v/old", "/v/new"]), t0, no_stat));
        assert!(!w.only_own_writes_at(&paths(&["/v/.a.md.12-0.rocksy-tmp", "/v/a.md"]), t0, no_stat));
        assert!(!w.only_own_writes_at(&ChangedPaths::Unknown, t0, no_stat));
        assert!(!w.only_own_writes_at(&ChangedPaths::Paths(Vec::new()), t0, no_stat));
        // Unix paths are case-sensitive.
        assert!(!w.only_own_writes_at(&paths(&["/v/A.md"]), t0, no_stat));
    }

    #[test]
    fn records_expire() {
        let w = unix();
        let t0 = Instant::now();
        w.record_at("/v/a.md", stamp(10, 1000), false, t0);
        let files = [("/v/a.md", stamp(10, 1000))];
        assert!(w.only_own_writes_at(&paths(&["/v/a.md"]), t0 + TTL - Duration::from_millis(1), disk(&files)));
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), t0 + TTL, disk(&files)));
        // Expired records are purged by the next record.
        assert_eq!(w.len(), 1);
        w.record_at("/v/b.md", stamp(1, 1), false, t0 + TTL);
        assert_eq!(w.len(), 1);
        // A new save of the same file renews the record with the new stamp.
        w.record_at("/v/a.md", stamp(12, 2000), false, t0 + TTL);
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), t0 + TTL, disk(&files)));
        assert!(w.only_own_writes_at(&paths(&["/v/a.md"]), t0 + TTL, disk(&[("/v/a.md", stamp(12, 2000))])));
    }

    #[test]
    fn registry_is_bounded_and_drops_the_oldest() {
        let w = OwnWrites::with_style(TTL, 3, false);
        let t0 = Instant::now();
        for i in 0..5u64 {
            w.record_at(&format!("/v/{i}.md"), stamp(i, i), false, t0 + Duration::from_millis(i));
        }
        assert_eq!(w.len(), 3);
        let now = t0 + Duration::from_millis(10);
        let stat = |p: &str| -> Option<FileStamp> {
            let i: u64 = p.trim_start_matches("/v/").trim_end_matches(".md").parse().ok()?;
            Some(stamp(i, i))
        };
        assert!(!w.only_own_writes_at(&paths(&["/v/0.md"]), now, stat));
        assert!(!w.only_own_writes_at(&paths(&["/v/1.md"]), now, stat));
        assert!(w.only_own_writes_at(&paths(&["/v/2.md", "/v/3.md", "/v/4.md"]), now, stat));
        w.forget("/v/4.md");
        assert!(!w.only_own_writes_at(&paths(&["/v/4.md"]), now, stat));
        assert!(!w.is_empty());
        assert_eq!(OwnWrites::with_limits(TTL, 0).capacity, 1);
    }

    #[test]
    fn created_file_blocks_skipping_until_a_later_listing() {
        let w = unix();
        let t0 = Instant::now();
        w.record_at("/v/a.md", stamp(10, 1000), false, t0);
        w.record_at("/v/new.md", stamp(3, 2000), true, t0 + Duration::from_millis(100));
        let files = [("/v/a.md", stamp(10, 1000)), ("/v/new.md", stamp(4, 3000))];
        let now = t0 + Duration::from_secs(1);
        assert!(!w.only_own_writes_at(&paths(&["/v/new.md"]), now, disk(&files)));
        // Other saves are not skipped either: the tree is known to be stale.
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), now, disk(&files)));
        // A second, ordinary save of the new note does not hide the creation.
        w.record_at("/v/new.md", stamp(4, 3000), false, t0 + Duration::from_millis(300));
        assert!(!w.only_own_writes_at(&paths(&["/v/new.md"]), now, disk(&files)));
        // A listing begun before the creation proves nothing.
        w.listing_started_at(t0 + Duration::from_millis(50));
        assert!(!w.only_own_writes_at(&paths(&["/v/new.md"]), now, disk(&files)));
        // One begun after it has seen the file.
        w.listing_started_at(t0 + Duration::from_millis(500));
        assert!(w.only_own_writes_at(&paths(&["/v/new.md"]), now, disk(&files)));
        assert!(w.only_own_writes_at(&paths(&["/v/a.md", "/v/new.md"]), now, disk(&files)));
        // The flag does not expire by itself.
        w.record_at("/v/other.md", stamp(1, 1), true, now);
        w.record_at("/v/a.md", stamp(10, 1000), false, now + TTL * 3);
        assert!(!w.only_own_writes_at(&paths(&["/v/a.md"]), now + TTL * 3, disk(&files)));
    }

    #[test]
    fn windows_paths_match_whatever_the_case_separator_or_prefix() {
        let w = OwnWrites::with_style(TTL, 8, true);
        let t0 = Instant::now();
        w.record_at(r"G:\Mon Drive\PKM\Note.md", stamp(10, 1000), false, t0);
        let stat = |_: &str| Some(stamp(10, 1000));
        for same in [r"g:\mon drive\pkm\note.md", "G:/Mon Drive/PKM/NOTE.md", r"\\?\G:\Mon Drive\PKM\Note.md"] {
            assert!(w.only_own_writes_at(&paths(&[same]), t0, stat), "{same}");
        }
        assert!(!w.only_own_writes_at(&paths(&[r"G:\Mon Drive\PKM\Note2.md"]), t0, stat));
        // One stat per distinct file, whatever the spellings in the batch.
        let calls = std::cell::Cell::new(0);
        let counting = |_: &str| {
            calls.set(calls.get() + 1);
            Some(stamp(10, 1000))
        };
        assert!(w.only_own_writes_at(&paths(&[r"G:\Mon Drive\PKM\Note.md", "g:/mon drive/pkm/note.md"]), t0, counting));
        assert_eq!(calls.get(), 1);
        w.forget("g:/MON DRIVE/pkm/note.md");
        assert!(w.is_empty());
    }

    #[test]
    fn change_sets_merge_and_unknown_absorbs() {
        let batch = |paths: &[&str], truncated: bool| ChangeBatch {
            root: "/v".into(),
            paths: paths.iter().map(|s| s.to_string()).collect(),
            truncated,
            markdown: 0,
            events: 1,
        };
        assert_eq!(ChangedPaths::from_batch(&batch(&["/v/a.md"], false)), paths(&["/v/a.md"]));
        assert!(ChangedPaths::from_batch(&batch(&["/v/a.md"], true)).is_unknown());
        assert!(ChangedPaths::from_batch(&batch(&[], false)).is_unknown());
        assert!(ChangedPaths::unknown().is_unknown());

        let mut set = paths(&["/v/a.md"]);
        set.merge(paths(&["/v/b.md", "/v/a.md"]));
        assert_eq!(set, paths(&["/v/a.md", "/v/b.md", "/v/a.md"]));
        set.merge(ChangedPaths::Unknown);
        assert!(set.is_unknown());
        set.merge(paths(&["/v/a.md"]));
        assert!(set.is_unknown());

        let mut unknown_first = ChangedPaths::Unknown;
        unknown_first.merge(paths(&["/v/a.md"]));
        assert!(unknown_first.is_unknown());

        // A merged set that grows too large is no longer trusted.
        let many: Vec<String> = (0..MAX_MERGED_PATHS).map(|i| format!("/v/{i}.md")).collect();
        let mut big = ChangedPaths::from_paths(many);
        assert!(!big.is_unknown());
        big.merge(paths(&["/v/one-more.md"]));
        assert!(big.is_unknown());
    }

    #[test]
    fn merged_requests_are_ignorable_only_if_every_path_is_an_own_write() {
        let w = unix();
        let t0 = Instant::now();
        w.record_at("/v/a.md", stamp(1, 1), false, t0);
        w.record_at("/v/b.md", stamp(2, 2), false, t0);
        let files = [("/v/a.md", stamp(1, 1)), ("/v/b.md", stamp(2, 2))];
        let mut merged = paths(&["/v/a.md"]);
        merged.merge(paths(&["/v/b.md"]));
        assert!(w.only_own_writes_at(&merged, t0, disk(&files)));
        merged.merge(paths(&["/v/external.md"]));
        assert!(!w.only_own_writes_at(&merged, t0, disk(&files)));
        let mut with_focus = paths(&["/v/a.md"]);
        with_focus.merge(ChangedPaths::unknown());
        assert!(!w.only_own_writes_at(&with_focus, t0, disk(&files)));
    }
}
