//! File tree of a vault: listing, persisted snapshot, background reconcile.
//!
//! Startup must not depend on the size of the vault or on the speed of the
//! disk (a cold Google Drive listing takes about a minute), so the tree shown
//! at launch comes from a snapshot stored in the application data directory.
//! The real listing runs afterwards and replaces the snapshot only when
//! something changed.
//!
//! Order of `Tree.entries`: depth-first; inside each folder, sub-folders
//! first, then files, each group sorted case-insensitively (ties broken by
//! the exact name). A folder is immediately followed by its content, so the
//! flat list can be rendered as a tree in one pass.

use crate::atomic::write_data_file;
use crate::error::{ApiError, Result};
use crate::paths::{absolute_string, fnv1a64, key_rel, mtime_ms, path_key};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// Relative to the vault root, `/`-separated.
    pub path: String,
    pub is_dir: bool,
    /// Bytes; 0 for folders.
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub root: String,
    pub from_snapshot: bool,
    pub entries: Vec<TreeEntry>,
}

/// A tree as the page receives it (`open_vault`, `tree-updated`): the JSON
/// of the contract, `{ root, fromSnapshot, entries: [{ path, isDir }] }`.
/// Sizes and modification times stay in the backend (the page never used
/// them, and they were a third of a payload of megabytes). Wraps a `Tree`,
/// a reference or an `Arc` without copying anything.
#[derive(Debug, Clone)]
pub struct IpcTree<T>(pub T);

impl<T: std::borrow::Borrow<Tree>> Serialize for IpcTree<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::{SerializeSeq, SerializeStruct};

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Entry<'a> {
            path: &'a str,
            is_dir: bool,
        }
        struct Entries<'a>(&'a [TreeEntry]);
        impl Serialize for Entries<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
                for e in self.0 {
                    seq.serialize_element(&Entry { path: &e.path, is_dir: e.is_dir })?;
                }
                seq.end()
            }
        }

        let tree: &Tree = self.0.borrow();
        let mut out = serializer.serialize_struct("Tree", 3)?;
        out.serialize_field("root", &tree.root)?;
        out.serialize_field("fromSnapshot", &tree.from_snapshot)?;
        out.serialize_field("entries", &Entries(&tree.entries))?;
        out.end()
    }
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

struct Child {
    name: String,
    /// Lowercased name, the primary sort key.
    key: String,
    size: u64,
    mtime_ms: u64,
}

#[derive(Default)]
struct DirListing {
    dirs: Vec<Child>,
    files: Vec<Child>,
}

fn rel_to_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    if !rel.is_empty() {
        // Push components one by one so the native separator is used.
        for comp in rel.split('/') {
            p.push(comp);
        }
    }
    p
}

fn join_child(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        let mut s = String::with_capacity(rel.len() + 1 + name.len());
        s.push_str(rel);
        s.push('/');
        s.push_str(name);
        s
    }
}

/// Reads one directory. Hidden entries (leading `.`), symbolic links and
/// names that are not valid Unicode are skipped.
fn read_one_dir(dir: &Path) -> std::io::Result<DirListing> {
    let mut listing = DirListing::default();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_symlink() {
            continue;
        }
        // Free on Windows (comes with the directory entry), one lstat on Unix.
        let Ok(meta) = entry.metadata() else { continue };
        let child = Child {
            name: name.to_string(),
            key: name.to_lowercase(),
            size: if file_type.is_dir() { 0 } else { meta.len() },
            mtime_ms: mtime_ms(&meta),
        };
        if file_type.is_dir() {
            listing.dirs.push(child);
        } else if file_type.is_file() {
            listing.files.push(child);
        }
    }
    let by_name = |a: &Child, b: &Child| a.key.cmp(&b.key).then_with(|| a.name.cmp(&b.name));
    listing.dirs.sort_by(by_name);
    listing.files.sort_by(by_name);
    Ok(listing)
}

/// Why a walk stops before every folder was read.
enum Stop {
    Cancelled,
    Failed(ApiError),
}

struct WalkQueue {
    pending: Vec<String>,
    active: usize,
    /// Set once: every worker then leaves, and the walk gives no tree.
    stop: Option<Stop>,
}

/// A panic elsewhere does not make the queue unusable: its content is plain
/// data, consistent after every statement.
fn lock_queue(queue: &Mutex<WalkQueue>) -> std::sync::MutexGuard<'_, WalkQueue> {
    queue.lock().unwrap_or_else(|e| e.into_inner())
}

/// Held by a worker while it reads a folder. If the worker unwinds (a panic
/// in the listing), the walk is marked as failed and the other workers are
/// woken up, instead of waiting for ever for a folder that will never be
/// reported.
struct Reading<'a> {
    queue: &'a Mutex<WalkQueue>,
    cv: &'a Condvar,
    reported: bool,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        if !self.reported {
            let mut q = lock_queue(self.queue);
            q.active = q.active.saturating_sub(1);
            if q.stop.is_none() {
                q.stop = Some(Stop::Failed(ApiError::io("a listing thread failed")));
            }
            drop(q);
            self.cv.notify_all();
        }
    }
}

/// Reads a sub-folder during a walk.
///
/// A folder that no longer exists (removed since its parent was read) or
/// that the user is not allowed to read is listed as empty: both are stable
/// facts, and one protected folder must not prevent the vault from being
/// listed. Any other failure (a drive that stops answering, an I/O error)
/// fails the walk: listing the folder as empty would make everything below
/// it look deleted, and the notes there would be dropped from the index and
/// from the snapshot.
fn read_sub_dir(root: &Path, rel: &str) -> Result<DirListing> {
    match read_one_dir(&rel_to_path(root, rel)) {
        Ok(listing) => Ok(listing),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied) => {
            Ok(DirListing::default())
        }
        Err(e) => Err(ApiError::from_io(&e, format!("cannot list {rel}"))),
    }
}

fn walk_worker(
    root: &Path,
    queue: &Mutex<WalkQueue>,
    cv: &Condvar,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Vec<(String, DirListing)> {
    let mut done = Vec::new();
    loop {
        let rel = {
            let mut q = lock_queue(queue);
            loop {
                if q.stop.is_some() {
                    break None;
                }
                if let Some(rel) = q.pending.pop() {
                    q.active += 1;
                    break Some(rel);
                }
                if q.active == 0 {
                    break None;
                }
                q = cv.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let Some(rel) = rel else {
            cv.notify_all();
            return done;
        };
        let mut reading = Reading { queue, cv, reported: false };
        #[cfg(test)]
        if rel.ends_with(tests::PANIC_FOLDER) {
            panic!("test: failure of a listing thread");
        }
        let listing = if cancelled() { Err(Stop::Cancelled) } else { read_sub_dir(root, &rel).map_err(Stop::Failed) };
        {
            let mut q = lock_queue(queue);
            match &listing {
                Ok(listing) => {
                    for d in &listing.dirs {
                        q.pending.push(join_child(&rel, &d.name));
                    }
                }
                Err(_) if q.stop.is_some() => {}
                Err(Stop::Cancelled) => q.stop = Some(Stop::Cancelled),
                Err(Stop::Failed(e)) => q.stop = Some(Stop::Failed(e.clone())),
            }
            q.active -= 1;
        }
        reading.reported = true;
        cv.notify_all();
        if let Ok(listing) = listing {
            done.push((rel, listing));
        }
    }
}

fn emit(rel: &str, listings: &mut HashMap<String, DirListing>, out: &mut Vec<TreeEntry>) {
    let Some(listing) = listings.remove(rel) else { return };
    for d in listing.dirs {
        let path = join_child(rel, &d.name);
        out.push(TreeEntry { path: path.clone(), is_dir: true, size: 0, mtime_ms: d.mtime_ms });
        emit(&path, listings, out);
    }
    for f in listing.files {
        out.push(TreeEntry {
            path: join_child(rel, &f.name),
            is_dir: false,
            size: f.size,
            mtime_ms: f.mtime_ms,
        });
    }
}

/// Lists the vault by walking the file system (`fromSnapshot: false`).
///
/// Files and folders whose name starts with `.` are ignored, symbolic links
/// are neither followed nor listed. Folders are read by a small pool of
/// threads, which matters on slow, latency-bound file systems.
///
/// The result is the whole visible tree or an error, never a part of it:
/// callers take what is missing from a listing for deleted.
///
/// Errors: `not_found` / `io` if the root cannot be read, `invalid` if it is
/// not a folder, `io` if a sub-folder could not be read for another reason
/// than "missing" or "access denied", or if a listing thread failed.
pub fn list_tree(root: &Path) -> Result<Tree> {
    match list_tree_cancellable(root, &|| false)? {
        Some(tree) => Ok(tree),
        None => Err(ApiError::io("listing interrupted")),
    }
}

/// Same as [`list_tree`], stopping early when `cancelled` answers `true`
/// (it is asked before each folder is read, from several threads). A
/// cancelled listing returns `Ok(None)`: there is no partial tree.
pub fn list_tree_cancellable(root: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Option<Tree>> {
    let root_string = absolute_string(root)?;
    let root_path = PathBuf::from(&root_string);
    if cancelled() {
        return Ok(None);
    }
    let meta = fs::metadata(&root_path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot open {root_string}")))?;
    if !meta.is_dir() {
        return Err(ApiError::invalid(format!("{root_string} is not a folder")));
    }
    let top = read_one_dir(&root_path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot list {root_string}")))?;

    let queue = Mutex::new(WalkQueue {
        pending: top.dirs.iter().map(|d| d.name.clone()).collect(),
        active: 0,
        stop: None,
    });
    let cv = Condvar::new();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
        .min(top.dirs.len().max(1));

    let mut listings: HashMap<String, DirListing> = HashMap::new();
    if !top.dirs.is_empty() {
        // `join` gives the panic of a worker back as an error: the scope
        // itself then does not panic.
        let parts: Vec<std::thread::Result<Vec<(String, DirListing)>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| scope.spawn(|| walk_worker(&root_path, &queue, &cv, cancelled)))
                .collect();
            handles.into_iter().map(|h| h.join()).collect()
        });
        let stop = lock_queue(&queue).stop.take();
        let mut panicked = false;
        for part in parts {
            match part {
                Ok(part) => listings.extend(part),
                Err(_) => panicked = true,
            }
        }
        match stop {
            Some(Stop::Failed(e)) => return Err(e),
            _ if panicked => return Err(ApiError::io(format!("a listing thread failed in {root_string}"))),
            Some(Stop::Cancelled) => return Ok(None),
            None => {}
        }
    }
    listings.insert(String::new(), top);

    let mut entries = Vec::with_capacity(listings.values().map(|l| l.dirs.len() + l.files.len()).sum());
    emit("", &mut listings, &mut entries);
    Ok(Some(Tree { root: root_string, from_snapshot: false, entries }))
}

impl Tree {
    /// Index of the entry for the absolute path `path`, if the tree lists it.
    fn position_of(&self, path: &Path) -> Option<usize> {
        let path = absolute_string(path).ok()?;
        let (path_key, root_key) = (path_key(&path), path_key(&self.root));
        let rel = key_rel(&path_key, &root_key, cfg!(windows))?;
        if rel.is_empty() {
            return None;
        }
        if cfg!(windows) {
            // Keys are lowercase with `\`; entries keep their case, with `/`.
            let rel = rel.replace('\\', "/");
            self.entries.iter().position(|e| e.path.len() == rel.len() && e.path.to_lowercase() == rel)
        } else {
            self.entries.iter().position(|e| e.path == rel)
        }
    }

    /// Records the new size and modification time of one file of the tree,
    /// right after the application saved it (`path` is absolute; the values
    /// are those returned by `save_file`). Nothing is listed. Returns
    /// whether the tree holds that file: `false` for a file it does not
    /// know (a note that was just created needs a real listing), a folder,
    /// or a path outside the root.
    ///
    /// Without this, the next real listing finds the entry different from
    /// what was known and reports a change that is only the application's
    /// own save.
    pub fn set_file_meta(&mut self, path: &Path, size: u64, mtime_ms: u64) -> bool {
        match self.position_of(path) {
            Some(i) if !self.entries[i].is_dir => {
                self.entries[i].size = size;
                self.entries[i].mtime_ms = mtime_ms;
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------
//
// Binary, little-endian:
//   magic "RKTREE01" | u64 FNV-1a of the payload | payload
//   payload: u32 root_len, root | u32 count | entries
//   entry:   u32 parent index (u32::MAX = root) | u8 flags (bit 0 = dir)
//            | u64 size | u64 mtime_ms | u16 name_len, name
// Storing the name and the index of the parent instead of the full path makes
// the file about three times smaller and still loads in a single pass.

const MAGIC: &[u8; 8] = b"RKTREE01";
const NO_PARENT: u32 = u32::MAX;
/// Longest relative path accepted when decoding (Windows allows 32 767
/// UTF-16 units for a whole path, Linux 4 096 bytes).
const MAX_PATH_BYTES: usize = 32 * 1024;
/// Largest ratio accepted between the paths rebuilt from a snapshot and the
/// size of its file. A real tree stays far below (about 1.3 on the 20x
/// synthetic vault; 12 for folders nested 100 deep with two-letter names).
const MAX_PATH_EXPANSION: usize = 64;

/// Location of the snapshot of `root` inside `data_dir`
/// (`trees/<hash of the root path>.tree`).
pub fn tree_snapshot_path(data_dir: &Path, root: &str) -> PathBuf {
    let hash = fnv1a64(path_key(root).as_bytes());
    data_dir.join("trees").join(format!("{hash:016x}.tree"))
}

fn encode_snapshot(tree: &Tree) -> Vec<u8> {
    let mut payload = Vec::with_capacity(tree.entries.len() * 40 + tree.root.len() + 16);
    payload.extend_from_slice(&(tree.root.len() as u32).to_le_bytes());
    payload.extend_from_slice(tree.root.as_bytes());
    payload.extend_from_slice(&(tree.entries.len() as u32).to_le_bytes());

    let mut dir_index: HashMap<&str, u32> = HashMap::new();
    for (i, e) in tree.entries.iter().enumerate() {
        let (parent, name) = match e.path.rfind('/') {
            Some(pos) => match dir_index.get(&e.path[..pos]) {
                Some(&idx) => (idx, &e.path[pos + 1..]),
                // Parent not seen before (unsorted input): store the full path.
                None => (NO_PARENT, e.path.as_str()),
            },
            None => (NO_PARENT, e.path.as_str()),
        };
        // Names longer than 64 KiB cannot exist on supported file systems.
        let name = &name.as_bytes()[..name.len().min(u16::MAX as usize)];
        payload.extend_from_slice(&parent.to_le_bytes());
        payload.push(u8::from(e.is_dir));
        payload.extend_from_slice(&e.size.to_le_bytes());
        payload.extend_from_slice(&e.mtime_ms.to_le_bytes());
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(name);
        if e.is_dir {
            dir_index.insert(e.path.as_str(), i as u32);
        }
    }

    let mut out = Vec::with_capacity(payload.len() + 16);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&fnv1a64(&payload).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes(b.try_into().unwrap()))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn str(&mut self, n: usize) -> Option<&'a str> {
        std::str::from_utf8(self.take(n)?).ok()
    }
}

fn decode_snapshot(bytes: &[u8]) -> Option<Tree> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return None;
    }
    let checksum = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let payload = &bytes[16..];
    if fnv1a64(payload) != checksum {
        return None;
    }
    let mut r = Reader { buf: payload, pos: 0 };
    let root_len = r.u32()? as usize;
    let root = r.str(root_len)?.to_string();
    let count = r.u32()? as usize;
    // Each entry takes at least 23 bytes: reject absurd counts before allocating.
    if count > payload.len() / 23 + 1 {
        return None;
    }
    let mut entries: Vec<TreeEntry> = Vec::with_capacity(count);
    // Paths are rebuilt from parent + name, so a crafted file (each entry
    // the child of the previous one) would expand to a size quadratic in
    // its own: refuse paths no file system accepts, and a total out of
    // proportion with the file.
    let path_budget = payload.len().saturating_mul(MAX_PATH_EXPANSION).saturating_add(4096);
    let mut path_bytes = 0usize;
    for i in 0..count {
        let parent = r.u32()?;
        let is_dir = r.u8()? & 1 == 1;
        let size = r.u64()?;
        let mtime_ms = r.u64()?;
        let name_len = r.u16()? as usize;
        let name = r.str(name_len)?;
        let path = if parent == NO_PARENT {
            name.to_string()
        } else {
            let p = parent as usize;
            if p >= i {
                return None;
            }
            if entries[p].path.len() + 1 + name.len() > MAX_PATH_BYTES {
                return None;
            }
            join_child(&entries[p].path, name)
        };
        path_bytes += path.len();
        if path.len() > MAX_PATH_BYTES || path_bytes > path_budget {
            return None;
        }
        entries.push(TreeEntry { path, is_dir, size, mtime_ms });
    }
    if r.pos != payload.len() {
        return None;
    }
    Some(Tree { root, from_snapshot: true, entries })
}

/// Persists `tree` as the snapshot of its root (atomic write in `data_dir`).
pub fn save_tree_snapshot(data_dir: &Path, tree: &Tree) -> Result<()> {
    write_data_file(&tree_snapshot_path(data_dir, &tree.root), &encode_snapshot(tree))
}

/// Loads the snapshot of `root` (`fromSnapshot: true`). `None` if there is
/// none, or if it is truncated, corrupted or belongs to another root.
pub fn load_tree_snapshot(data_dir: &Path, root: &Path) -> Option<Tree> {
    let root_string = absolute_string(root).ok()?;
    let bytes = fs::read(tree_snapshot_path(data_dir, &root_string)).ok()?;
    let mut tree = decode_snapshot(&bytes)?;
    if path_key(&tree.root) != path_key(&root_string) {
        return None;
    }
    tree.root = root_string;
    Some(tree)
}

// ---------------------------------------------------------------------------
// Open / reconcile
// ---------------------------------------------------------------------------

/// Tree to show right away: the snapshot if there is one (no access to the
/// vault at all), otherwise a fresh listing, which is then saved as the
/// snapshot. Call [`reconcile_tree`] in the background afterwards.
pub fn open_tree(data_dir: &Path, root: &Path) -> Result<Tree> {
    if let Some(tree) = load_tree_snapshot(data_dir, root) {
        return Ok(tree);
    }
    let tree = list_tree(root)?;
    // A read-only data directory must not prevent the vault from opening.
    let _ = save_tree_snapshot(data_dir, &tree);
    Ok(tree)
}

/// Folder modification times are ignored: they change when a hidden entry is
/// added to the folder, which does not alter the visible tree.
fn same_entries(a: &[TreeEntry], b: &[TreeEntry]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.path == y.path
                && x.is_dir == y.is_dir
                && (x.is_dir || (x.size == y.size && x.mtime_ms == y.mtime_ms))
        })
}

/// Lists the vault again and compares the result with the snapshot. When
/// they differ (or there is no snapshot), the snapshot is rewritten and
/// `on_update` is called with the fresh tree; otherwise nothing is written
/// and the callback is not called. Returns whether an update happened.
///
/// Blocking: run it on a background thread. If the vault cannot be listed
/// (drive not mounted), the error is returned and the snapshot is kept.
pub fn reconcile_tree(
    data_dir: &Path,
    root: &Path,
    on_update: impl FnOnce(&Tree),
) -> Result<bool> {
    let fresh = list_tree(root)?;
    if let Some(snapshot) = load_tree_snapshot(data_dir, root) {
        if same_entries(&snapshot.entries, &fresh.entries) {
            return Ok(false);
        }
    }
    let saved = save_tree_snapshot(data_dir, &fresh);
    on_update(&fresh);
    saved.map(|()| true)
}

/// Result of [`refresh_tree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRefresh {
    /// The vault as it is now (`fromSnapshot: false`).
    pub tree: Tree,
    /// Whether it differs from what was known (the tree given, or the
    /// snapshot): entries added or removed, a file whose size or
    /// modification time changed.
    pub changed: bool,
    /// False when the snapshot had to be rewritten and could not be.
    pub snapshot_saved: bool,
}

/// One listing for both the tree and the index: lists the vault, compares
/// the result with `known` (the tree of the last listing if the caller kept
/// it in memory; otherwise the snapshot is read from `data_dir`), rewrites
/// the snapshot when they differ, and returns the fresh tree so that the
/// caller can show it and then hand it to
/// [`TagIndex::reconcile_with_tree`](crate::TagIndex::reconcile_with_tree)
/// instead of listing the vault a second time.
///
/// `Ok(None)` when `cancelled` answered `true` during the listing; nothing
/// was written. On error nothing was written either, and the snapshot stays.
pub fn refresh_tree(
    data_dir: &Path,
    root: &Path,
    known: Option<&Tree>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<TreeRefresh>> {
    let Some(tree) = list_tree_cancellable(root, cancelled)? else { return Ok(None) };
    let changed = match known {
        Some(known) => !same_entries(&known.entries, &tree.entries),
        None => !load_tree_snapshot(data_dir, root).is_some_and(|s| same_entries(&s.entries, &tree.entries)),
    };
    let snapshot_saved = !changed || save_tree_snapshot(data_dir, &tree).is_ok();
    Ok(Some(TreeRefresh { tree, changed, snapshot_saved }))
}

/// Records in the snapshot of `root` the size and modification time of one
/// file the application just saved (see [`Tree::set_file_meta`]), without
/// listing the vault. Returns whether the snapshot was rewritten: `false`
/// when there is no snapshot or when it does not list the file.
///
/// Reads and rewrites the whole snapshot (about 1 MB for 20 000 entries):
/// call it off the path that answers `save_file`. A caller that keeps the
/// tree in memory can use [`Tree::set_file_meta`] and save the snapshot
/// when it sees fit.
pub fn update_snapshot_entry(data_dir: &Path, root: &Path, path: &Path, size: u64, mtime_ms: u64) -> Result<bool> {
    let Some(mut tree) = load_tree_snapshot(data_dir, root) else { return Ok(false) };
    let unchanged = tree.position_of(path).is_some_and(|i| {
        let e = &tree.entries[i];
        !e.is_dir && e.size == size && e.mtime_ms == mtime_ms
    });
    if unchanged {
        return Ok(true);
    }
    if !tree.set_file_meta(path, size, mtime_ms) {
        return Ok(false);
    }
    save_tree_snapshot(data_dir, &tree)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};
    use std::cell::RefCell;

    /// A folder with this name makes the listing thread that reads it panic.
    pub(super) const PANIC_FOLDER: &str = "__panic-in-listing__";

    fn paths(tree: &Tree) -> Vec<&str> {
        tree.entries.iter().map(|e| e.path.as_str()).collect()
    }

    fn sample_vault(dir: &TempDir) -> PathBuf {
        for (rel, content) in [
            ("vault/b.md", "bb"),
            ("vault/Abc.md", "a"),
            ("vault/a.md", "a2"),
            ("vault/Zeta/note.md", "z"),
            ("vault/alpha/inner/deep.md", "deep"),
            ("vault/alpha/Beta.md", "b"),
            ("vault/alpha/alpha.png", "png"),
            ("vault/.obsidian/app.json", "{}"),
            ("vault/.opencode/node_modules/x/index.js", "x"),
            ("vault/alpha/.hidden.md", "h"),
            ("vault/alpha/.trash/old.md", "o"),
            ("vault/board.canvas", "{}"),
        ] {
            dir.write(rel, content);
        }
        fs::create_dir_all(dir.path().join("vault/empty")).unwrap();
        dir.path().join("vault")
    }

    #[test]
    fn listing_is_sorted_and_skips_hidden_entries() {
        let dir = TempDir::new("tree");
        let root = sample_vault(&dir);
        let tree = list_tree(&root).unwrap();
        assert!(!tree.from_snapshot);
        assert_eq!(tree.root, root.to_str().unwrap());
        assert_eq!(
            paths(&tree),
            [
                "alpha",
                "alpha/inner",
                "alpha/inner/deep.md",
                "alpha/alpha.png",
                "alpha/Beta.md",
                "empty",
                "Zeta",
                "Zeta/note.md",
                "a.md",
                "Abc.md",
                "b.md",
                "board.canvas",
            ]
        );
        let alpha = &tree.entries[0];
        assert!(alpha.is_dir && alpha.size == 0);
        let deep = &tree.entries[2];
        assert!(!deep.is_dir);
        assert_eq!(deep.size, 4);
        assert!(deep.mtime_ms > 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_nor_listed() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("tree-links");
        dir.write("vault/a.md", "a");
        dir.write("outside/secret.md", "s");
        let root = dir.path().join("vault");
        symlink(dir.path().join("outside"), root.join("linked-dir")).unwrap();
        symlink(dir.path().join("outside/secret.md"), root.join("linked.md")).unwrap();
        symlink(&root, root.join("loop")).unwrap();
        assert_eq!(paths(&list_tree(&root).unwrap()), ["a.md"]);
    }

    #[test]
    fn listing_errors() {
        let dir = TempDir::new("tree-err");
        let err = list_tree(&dir.path().join("missing")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        let f = dir.write("file.md", "x");
        assert_eq!(list_tree(&f).unwrap_err().kind, ErrorKind::Invalid);
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(list_tree(&empty).unwrap().entries.is_empty());
    }

    #[test]
    fn snapshot_round_trip() {
        let dir = TempDir::new("snap");
        let root = sample_vault(&dir);
        let data = dir.path().join("data");
        assert!(load_tree_snapshot(&data, &root).is_none());

        let tree = list_tree(&root).unwrap();
        save_tree_snapshot(&data, &tree).unwrap();
        let loaded = load_tree_snapshot(&data, &root).unwrap();
        assert!(loaded.from_snapshot);
        assert_eq!(loaded.root, tree.root);
        assert_eq!(loaded.entries, tree.entries);

        // One file per root, named after the hash of the root path.
        let files = all_files(&data);
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with("trees/") && files[0].ends_with(".tree"));
        assert!(load_tree_snapshot(&data, &dir.path().join("other")).is_none());
    }

    #[test]
    fn snapshot_encoding_handles_any_order_and_unicode() {
        let tree = Tree {
            root: "/v".into(),
            from_snapshot: false,
            entries: vec![
                TreeEntry { path: "z/orphan.md".into(), is_dir: false, size: 1, mtime_ms: 2 },
                TreeEntry { path: "Dossier é".into(), is_dir: true, size: 0, mtime_ms: 3 },
                TreeEntry { path: "Dossier é/日本語 ★.md".into(), is_dir: false, size: u64::MAX, mtime_ms: 4 },
            ],
        };
        let decoded = decode_snapshot(&encode_snapshot(&tree)).unwrap();
        assert_eq!(decoded.entries, tree.entries);
        let empty = Tree { root: "/v".into(), from_snapshot: false, entries: vec![] };
        assert!(decode_snapshot(&encode_snapshot(&empty)).unwrap().entries.is_empty());
    }

    #[test]
    fn damaged_snapshot_is_ignored() {
        let dir = TempDir::new("snap-bad");
        let root = sample_vault(&dir);
        let data = dir.path().join("data");
        let tree = list_tree(&root).unwrap();
        save_tree_snapshot(&data, &tree).unwrap();
        let file = tree_snapshot_path(&data, &tree.root);
        let good = fs::read(&file).unwrap();

        // Truncated at every possible length, then with a flipped byte.
        for len in 0..good.len() {
            assert!(decode_snapshot(&good[..len]).is_none(), "len {len}");
        }
        let mut flipped = good.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0x40;
        assert!(decode_snapshot(&flipped).is_none());

        fs::write(&file, &good[..good.len() / 2]).unwrap();
        assert!(load_tree_snapshot(&data, &root).is_none());
        // open_tree falls back to a listing and repairs the snapshot.
        let opened = open_tree(&data, &root).unwrap();
        assert!(!opened.from_snapshot);
        assert!(open_tree(&data, &root).unwrap().from_snapshot);
    }

    #[test]
    fn open_then_reconcile_detects_additions_and_removals() {
        let dir = TempDir::new("reconcile");
        let root = sample_vault(&dir);
        let data = dir.path().join("data");

        let first = open_tree(&data, &root).unwrap();
        assert!(!first.from_snapshot);
        let second = open_tree(&data, &root).unwrap();
        assert!(second.from_snapshot);
        assert_eq!(second.entries, first.entries);

        // Nothing changed: no callback, snapshot file not rewritten.
        let snap_file = tree_snapshot_path(&data, &first.root);
        let snap_before = fs::read(&snap_file).unwrap();
        let calls = RefCell::new(0);
        assert!(!reconcile_tree(&data, &root, |_| *calls.borrow_mut() += 1).unwrap());
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(fs::read(&snap_file).unwrap(), snap_before);

        // A hidden file appears: the visible tree is the same.
        fs::write(root.join("alpha/.DS_Store"), "x").unwrap();
        assert!(!reconcile_tree(&data, &root, |_| *calls.borrow_mut() += 1).unwrap());

        // Add one file and one folder, remove one file.
        fs::write(root.join("alpha/new.md"), "new").unwrap();
        fs::create_dir(root.join("fresh")).unwrap();
        fs::remove_file(root.join("b.md")).unwrap();
        let seen = RefCell::new(Vec::new());
        let updated = reconcile_tree(&data, &root, |t| {
            assert!(!t.from_snapshot);
            *seen.borrow_mut() = t.entries.iter().map(|e| e.path.clone()).collect();
        })
        .unwrap();
        assert!(updated);
        let seen = seen.into_inner();
        assert!(seen.contains(&"alpha/new.md".to_string()));
        assert!(seen.contains(&"fresh".to_string()));
        assert!(!seen.contains(&"b.md".to_string()));

        // The snapshot now matches the disk.
        let reopened = open_tree(&data, &root).unwrap();
        assert!(reopened.from_snapshot);
        assert_eq!(paths(&reopened), seen.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(!reconcile_tree(&data, &root, |_| panic!("no change expected")).unwrap());
    }

    #[test]
    fn reconcile_detects_content_change_by_size_or_mtime() {
        let dir = TempDir::new("reconcile-mod");
        let root = sample_vault(&dir);
        let data = dir.path().join("data");
        open_tree(&data, &root).unwrap();
        fs::write(root.join("b.md"), "a much longer content").unwrap();
        let mut size = 0;
        assert!(reconcile_tree(&data, &root, |t| {
            size = t.entries.iter().find(|e| e.path == "b.md").unwrap().size;
        })
        .unwrap());
        assert_eq!(size, 21);
    }

    #[test]
    fn reconcile_without_snapshot_and_with_missing_root() {
        let dir = TempDir::new("reconcile-edge");
        let root = sample_vault(&dir);
        let data = dir.path().join("data");

        // No snapshot yet: the listing is reported and saved.
        let mut called = false;
        assert!(reconcile_tree(&data, &root, |_| called = true).unwrap());
        assert!(called);
        let snapshot = load_tree_snapshot(&data, &root).unwrap();

        // The vault disappears (drive unmounted): error, snapshot kept.
        let moved = dir.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        let err = reconcile_tree(&data, &root, |_| panic!("must not be called")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(load_tree_snapshot(&data, &root).unwrap().entries, snapshot.entries);
        // And the tree can still be opened from the snapshot.
        assert!(open_tree(&data, &root).unwrap().from_snapshot);
    }

    /// BUG-3, consequence of `panic = "unwind"`: a listing thread that dies
    /// must fail the listing. Its folders used to be silently missing from
    /// the result, and everything below them was then taken for deleted.
    #[test]
    fn a_panic_in_a_listing_thread_fails_the_listing_and_removes_nothing() {
        let dir = TempDir::new("tree-panic");
        let root = sample_vault(&dir);
        for i in 0..12 {
            dir.write(&format!("vault/many/d{i}/n{i}.md"), format!("note {i} #t{i}\n"));
        }
        let data = dir.path().join("data");
        let complete = open_tree(&data, &root).unwrap();
        let index = crate::TagIndex::new();
        let options = crate::BuildOptions::default();
        let cancel = crate::CancelToken::new();
        let before = index.reconcile(&data, &root, options, &cancel, |_| {}).unwrap().unwrap();

        // From now on, one of the listing threads panics.
        fs::create_dir_all(root.join("many").join(PANIC_FOLDER)).unwrap();
        for _ in 0..3 {
            let err = list_tree(&root).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Io, "{err:?}");
        }
        assert!(list_tree_cancellable(&root, &|| false).is_err());
        assert!(refresh_tree(&data, &root, None, &|| false).is_err());

        // The snapshot is kept as it was, and no callback sees a partial tree.
        assert!(reconcile_tree(&data, &root, |_| panic!("a partial listing was reported")).is_err());
        assert_eq!(load_tree_snapshot(&data, &root).unwrap().entries, complete.entries);
        // The index keeps every note.
        assert!(index.reconcile(&data, &root, options, &cancel, |_| {}).is_err());
        let state = index.index_ready(false, 0, None);
        assert_eq!((state.files, state.tags), (before.files, before.tags));
        assert_eq!(index.tag_fragments("t7", None).len(), 1);

        // The cause goes away: everything works again (no lock left poisoned).
        fs::remove_dir(root.join("many").join(PANIC_FOLDER)).unwrap();
        assert_eq!(paths(&list_tree(&root).unwrap()), paths(&complete));
        assert_eq!(index.reconcile(&data, &root, options, &cancel, |_| {}).unwrap().unwrap().removed, 0);
    }

    #[test]
    fn a_cancelled_listing_gives_no_tree() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = TempDir::new("tree-cancel");
        let root = sample_vault(&dir);
        for i in 0..40 {
            dir.write(&format!("vault/deep/a{i}/b/c/n.md"), "x");
        }
        let data = dir.path().join("data");
        let complete = open_tree(&data, &root).unwrap();
        assert!(list_tree_cancellable(&root, &|| true).unwrap().is_none());
        // Cancelled at every possible moment of the walk: never a partial
        // tree, and the snapshot is never touched.
        let mut cancelled_runs = 0;
        for after in 0..200 {
            let calls = AtomicUsize::new(0);
            let stop = || calls.fetch_add(1, Ordering::SeqCst) >= after;
            match refresh_tree(&data, &root, None, &stop).unwrap() {
                None => cancelled_runs += 1,
                Some(refresh) => {
                    assert_eq!(refresh.tree.entries, complete.entries, "after {after}");
                    assert!(!refresh.changed);
                }
            }
            assert_eq!(load_tree_snapshot(&data, &root).unwrap().entries, complete.entries);
        }
        assert!(cancelled_runs > 100, "{cancelled_runs}");
        assert_eq!(list_tree_cancellable(&root, &|| false).unwrap().unwrap().entries, complete.entries);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_without_read_access_is_listed_as_empty() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("tree-denied");
        let root = sample_vault(&dir);
        let locked = root.join("alpha/inner");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let denied = fs::read_dir(&locked).is_err();
        let listed = list_tree(&root);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if !denied {
            return; // running as root: nothing is ever denied
        }
        let listed = listed.unwrap();
        assert!(paths(&listed).contains(&"alpha/inner"));
        assert!(!paths(&listed).contains(&"alpha/inner/deep.md"));
        assert!(paths(&listed).contains(&"alpha/Beta.md"));
    }

    /// SEC-15. A snapshot where each entry is the child of the previous one
    /// would be rebuilt into paths of a total size quadratic in the file.
    #[test]
    fn a_snapshot_that_expands_out_of_proportion_is_rejected() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(b"/v");
        let count = 40_000u32;
        payload.extend_from_slice(&count.to_le_bytes());
        for i in 0..count {
            payload.extend_from_slice(&(if i == 0 { NO_PARENT } else { i - 1 }).to_le_bytes());
            payload.push(1);
            payload.extend_from_slice(&0u64.to_le_bytes());
            payload.extend_from_slice(&0u64.to_le_bytes());
            payload.extend_from_slice(&20u16.to_le_bytes());
            payload.extend_from_slice(b"dossier-de-vingt-car");
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&fnv1a64(&payload).to_le_bytes());
        bytes.extend_from_slice(&payload);
        let started = std::time::Instant::now();
        assert!(decode_snapshot(&bytes).is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));

        // A real tree, even a deep one, is far from the limits.
        let mut entries = Vec::new();
        let mut path = String::new();
        for depth in 0..100 {
            if depth > 0 {
                path.push('/');
            }
            path.push_str("ab");
            entries.push(TreeEntry { path: path.clone(), is_dir: true, size: 0, mtime_ms: 1 });
        }
        entries.push(TreeEntry { path: format!("{path}/n.md"), is_dir: false, size: 3, mtime_ms: 2 });
        let deep = Tree { root: "/v".into(), from_snapshot: false, entries };
        assert_eq!(decode_snapshot(&encode_snapshot(&deep)).unwrap().entries, deep.entries);
    }

    #[test]
    fn tree_json_is_camel_case() {
        let tree = Tree {
            root: "/v".into(),
            from_snapshot: true,
            entries: vec![TreeEntry { path: "a.md".into(), is_dir: false, size: 3, mtime_ms: 9 }],
        };
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(
            json,
            r#"{"root":"/v","fromSnapshot":true,"entries":[{"path":"a.md","isDir":false,"size":3,"mtimeMs":9}]}"#
        );
    }
}
