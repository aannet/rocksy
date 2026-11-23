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
use crate::paths::{absolute_string, fnv1a64, mtime_ms, path_key};
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

struct WalkQueue {
    pending: Vec<String>,
    active: usize,
}

fn walk_worker(
    root: &Path,
    queue: &Mutex<WalkQueue>,
    cv: &Condvar,
) -> Vec<(String, DirListing)> {
    let mut done = Vec::new();
    loop {
        let rel = {
            let mut q = queue.lock().unwrap();
            loop {
                if let Some(rel) = q.pending.pop() {
                    q.active += 1;
                    break Some(rel);
                }
                if q.active == 0 {
                    break None;
                }
                q = cv.wait(q).unwrap();
            }
        };
        let Some(rel) = rel else {
            cv.notify_all();
            return done;
        };
        // An unreadable sub-folder is listed as empty rather than failing the walk.
        let listing = read_one_dir(&rel_to_path(root, &rel)).unwrap_or_default();
        {
            let mut q = queue.lock().unwrap();
            for d in &listing.dirs {
                q.pending.push(join_child(&rel, &d.name));
            }
            q.active -= 1;
        }
        cv.notify_all();
        done.push((rel, listing));
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
/// Errors: `not_found` / `io` if the root cannot be read, `invalid` if it is
/// not a folder.
pub fn list_tree(root: &Path) -> Result<Tree> {
    let root_string = absolute_string(root)?;
    let root_path = PathBuf::from(&root_string);
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
    });
    let cv = Condvar::new();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
        .min(top.dirs.len().max(1));

    let mut listings: HashMap<String, DirListing> = HashMap::new();
    if !top.dirs.is_empty() {
        let parts: Vec<Vec<(String, DirListing)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| scope.spawn(|| walk_worker(&root_path, &queue, &cv)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        });
        for part in parts {
            listings.extend(part);
        }
    }
    listings.insert(String::new(), top);

    let mut entries = Vec::with_capacity(listings.values().map(|l| l.dirs.len() + l.files.len()).sum());
    emit("", &mut listings, &mut entries);
    Ok(Tree { root: root_string, from_snapshot: false, entries })
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
            join_child(&entries[p].path, name)
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};
    use std::cell::RefCell;

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
