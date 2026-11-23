//! Tag library of a vault: in memory, optionally persisted.
//!
//! The index is filled either by reading the vault's `.md` files with a
//! bounded pool of threads ([`TagIndex::build`], [`TagIndex::reconcile`]) or
//! from the file written by a previous session
//! ([`TagIndex::load_persisted`]). A reconcile only reads the files whose
//! size or modification time differ from what the index knows, so opening an
//! already indexed vault costs one directory walk and no note read.
//!
//! Nothing is ever written to the vault: the persisted form lives in the
//! application data directory (see `persist.rs` for the format).

use super::parse::parse;
use super::persist::{decode, encode, tags_index_path};
use super::store::{has_md_extension, FileParsed, Inner};
use super::{Fragment, ReconcileReport, TagCount, TagStats, TagsProgress, TagsReady};
use crate::atomic::write_data_file;
use crate::error::Result;
use crate::paths::{absolute_string, join_rel, mtime_ms, path_key};
use crate::tree::list_tree;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Instant;

/// Cooperative cancellation of a build or reconcile. Cloning shares the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// Reader threads. Default: twice the number of cores, between 4 and 16
    /// (reading is latency-bound on a synced drive, not CPU-bound).
    pub threads: Option<usize>,
}

fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (cores * 2).clamp(4, 16)
}

/// Source of `Inner::epoch` values.
static EPOCH: AtomicU64 = AtomicU64::new(1);

fn next_epoch() -> u64 {
    EPOCH.fetch_add(1, Ordering::Relaxed)
}

enum PendingChange {
    Set { path: String, text: String, meta: Option<(u64, u64)> },
    Remove(String),
}

/// A Markdown file found by the directory walk.
struct MdFile {
    rel: String,
    path: String,
    /// (size, mtime in ms) from the directory listing.
    meta: (u64, u64),
}

enum ReadOutcome {
    Parsed(FileParsed),
    NotUtf8,
    Failed,
}

struct Refresh {
    files: usize,
    reread: usize,
    removed: usize,
    unchanged: usize,
    cancelled: bool,
}

/// Thread-safe tag library. Share it behind an `Arc`; queries take a read
/// lock and are served by the previous content while a build or reconcile is
/// running (the new content is swapped in at the end).
#[derive(Default)]
pub struct TagIndex {
    inner: RwLock<Inner>,
    /// Serialises builds, reconciles and loads.
    build_lock: Mutex<()>,
    /// Serialises writes of the persisted file.
    persist_lock: Mutex<()>,
    /// Changes received while a build is running, replayed on its result.
    pending: Mutex<Option<Vec<PendingChange>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl TagIndex {
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    // -----------------------------------------------------------------------
    // Filling the index
    // -----------------------------------------------------------------------

    /// Indexes every `.md` file of the vault (hidden files and folders
    /// excluded) and replaces the content of the index. Reads every file and
    /// persists nothing; prefer [`TagIndex::reconcile`] in the application.
    ///
    /// Blocking: run it on a background thread. `on_progress` is called from
    /// worker threads, at most every few dozen files, with non-decreasing
    /// `done`, and once at the end with `done == total`. Returns `Ok(None)`
    /// if cancelled, in which case the previous content is kept. Unreadable
    /// and non-UTF-8 files are skipped (see [`TagIndex::stats`]).
    pub fn build(
        &self,
        vault: &Path,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<Option<TagsReady>> {
        let _building = lock(&self.build_lock);
        let started = Instant::now();
        *lock(&self.pending) = Some(Vec::new());
        let refreshed = self.refresh(vault, options, cancel, &on_progress, false);
        *lock(&self.pending) = None;
        let Some(refresh) = refreshed? else { return Ok(None) };
        let live = self.read();
        Ok(Some(TagsReady {
            files: refresh.files,
            tags: live.live_tags(),
            occurrences: live.occurrences,
            ms: started.elapsed().as_millis() as u64,
        }))
    }

    /// Loads the index persisted for `vault` in `data_dir` and replaces the
    /// content of the index, without reading any note. Returns `None`, and
    /// leaves the index untouched, if there is no usable file (missing,
    /// truncated, corrupted, written by another version or for another
    /// root). Follow it with [`TagIndex::reconcile`] in the background.
    pub fn load_persisted(&self, data_dir: &Path, vault: &Path) -> Option<TagsReady> {
        let _building = lock(&self.build_lock);
        let started = Instant::now();
        let root = absolute_string(vault).ok()?;
        let bytes = fs::read(tags_index_path(data_dir, &root)).ok()?;
        let mut inner = decode(&bytes, &root)?;
        inner.epoch = next_epoch();
        let ready = TagsReady {
            files: inner.md_files(),
            tags: inner.live_tags(),
            occurrences: inner.occurrences,
            ms: 0,
        };
        *self.write() = inner;
        Some(TagsReady { ms: started.elapsed().as_millis() as u64, ..ready })
    }

    /// Brings the index in line with the vault, reading as little as
    /// possible, then persists it.
    ///
    /// The vault's `.md` files are listed (same rules as `build`) and their
    /// size and modification time compared with what the index knows; only
    /// new and changed files are read and analysed again, and files that
    /// disappeared are removed. With an empty index, or an index of another
    /// vault, this is a full build. The persisted file is rewritten if
    /// anything changed, if the index has unsaved changes, or if the file
    /// does not exist; a failure to write it is not an error (`persisted`
    /// is then `false`; call [`TagIndex::save`] to get the reason).
    ///
    /// Blocking: run it on a background thread. Progress counts the files to
    /// read again (`total` is 0 when nothing changed). A file whose content
    /// changed without its size or modification time changing is not seen.
    ///
    /// Cancellation returns `Ok(None)`. The files already read are kept in
    /// the index (which stays consistent, marked as unsaved), so the next
    /// reconcile only reads the rest; nothing is written to disk.
    pub fn reconcile(
        &self,
        data_dir: &Path,
        vault: &Path,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<Option<ReconcileReport>> {
        let _building = lock(&self.build_lock);
        let started = Instant::now();
        *lock(&self.pending) = Some(Vec::new());
        let refreshed = self.refresh(vault, options, cancel, &on_progress, true);
        *lock(&self.pending) = None;
        let Some(refresh) = refreshed? else { return Ok(None) };
        if refresh.cancelled {
            return Ok(None);
        }
        let persisted = self.persist(data_dir).unwrap_or(false);
        let live = self.read();
        Ok(Some(ReconcileReport {
            files: refresh.files,
            tags: live.live_tags(),
            occurrences: live.occurrences,
            ms: started.elapsed().as_millis() as u64,
            reread: refresh.reread,
            removed: refresh.removed,
            unchanged: refresh.unchanged,
            persisted,
        }))
    }

    /// Reads and analyses `todo` (indices into `files`) with a bounded pool.
    fn read_files(
        files: &[MdFile],
        todo: &[usize],
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: &(impl Fn(TagsProgress) + Sync),
    ) -> Vec<(usize, ReadOutcome)> {
        let total = todo.len();
        if total == 0 {
            return Vec::new();
        }
        let threads = options.threads.unwrap_or_else(default_threads).clamp(1, 64).min(total);
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        // Highest `done` reported so far; the lock keeps reports ordered.
        let reported = Mutex::new(0usize);
        let step = (total / 100).clamp(16, 256);

        let parts: Vec<Vec<(usize, ReadOutcome)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut local = Vec::new();
                        loop {
                            if cancel.is_cancelled() {
                                break;
                            }
                            let n = next.fetch_add(1, Ordering::Relaxed);
                            if n >= total {
                                break;
                            }
                            let index = todo[n];
                            let outcome = match fs::read(&files[index].path) {
                                Ok(bytes) => match String::from_utf8(bytes) {
                                    Ok(text) => ReadOutcome::Parsed(FileParsed::from_parsed(parse(&text))),
                                    Err(_) => ReadOutcome::NotUtf8,
                                },
                                Err(_) => ReadOutcome::Failed,
                            };
                            local.push((index, outcome));
                            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                            if d.is_multiple_of(step) && d < total {
                                let mut last = lock(&reported);
                                if d > *last {
                                    *last = d;
                                    on_progress(TagsProgress { done: d, total });
                                }
                            }
                        }
                        local
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        });
        parts.into_iter().flatten().collect()
    }

    /// Common part of `build` (`incremental == false`: read everything,
    /// replace the content, keep the previous content if cancelled) and
    /// `reconcile` (`incremental == true`: reuse what is unchanged, keep the
    /// partial work if cancelled). `None`: cancelled without any effect.
    fn refresh(
        &self,
        vault: &Path,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: &(impl Fn(TagsProgress) + Sync),
        incremental: bool,
    ) -> Result<Option<Refresh>> {
        let tree = list_tree(vault)?;
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let root = tree.root;
        let root_key = path_key(&root);
        let files: Vec<MdFile> = tree
            .entries
            .into_iter()
            .filter(|e| !e.is_dir && has_md_extension(&e.path))
            .map(|e| MdFile { path: join_rel(&root, &e.path), rel: e.path, meta: (e.size, e.mtime_ms) })
            .collect();

        // What does the current content already know?
        let mut known: Vec<Option<u32>> = vec![None; files.len()];
        let mut todo: Vec<usize> = Vec::new();
        let mut removed = 0;
        let epoch;
        let same_root;
        {
            let live = self.read();
            epoch = live.epoch;
            same_root = incremental && live.root_key() == Some(root_key.as_str());
            if same_root {
                let mut seen = vec![false; live.files.len()];
                for (i, file) in files.iter().enumerate() {
                    match live.by_path.get(&path_key(&file.path)) {
                        Some(&id) if live.files[id as usize].present => {
                            seen[id as usize] = true;
                            known[i] = Some(id);
                            if live.files[id as usize].meta != Some(file.meta) {
                                todo.push(i);
                            }
                        }
                        _ => todo.push(i),
                    }
                }
                removed = live.files.iter().zip(&seen).filter(|(f, seen)| f.present && !**seen).count();
            } else {
                todo = (0..files.len()).collect();
            }
        }
        let total = todo.len();
        let unchanged = files.len() - total;
        on_progress(TagsProgress { done: 0, total });

        let results = Self::read_files(&files, &todo, options, cancel, on_progress);
        let cancelled = cancel.is_cancelled();
        if cancelled && !incremental {
            return Ok(None);
        }
        let reread = results.len();

        if !(same_root && reread == 0 && removed == 0) {
            let mut outcomes: Vec<Option<ReadOutcome>> = (0..files.len()).map(|_| None).collect();
            for (index, outcome) in results {
                outcomes[index] = Some(outcome);
            }

            let mut pending = lock(&self.pending);
            let mut live = self.write();
            if same_root && live.epoch != epoch {
                // The content was replaced meanwhile (cleared): the file ids
                // gathered above mean nothing any more.
                return Ok(None);
            }
            // Rebuilt in tree order, so that tag spelling and result order
            // do not depend on thread scheduling nor on the update history.
            let mut fresh = Inner::with_root(&root);
            let mut tag_map = vec![u32::MAX; live.tags.len()];
            for (i, file) in files.iter().enumerate() {
                match (outcomes[i].take(), known[i]) {
                    (Some(ReadOutcome::Parsed(parsed)), _) => {
                        fresh.put_file(&file.path, &file.rel, Some(file.meta), false, parsed)
                    }
                    (Some(ReadOutcome::NotUtf8), _) => {
                        fresh.put_file(&file.path, &file.rel, Some(file.meta), true, FileParsed::default())
                    }
                    // Unknown size/mtime: tried again by the next reconcile.
                    (Some(ReadOutcome::Failed), _) => {
                        fresh.put_file(&file.path, &file.rel, None, true, FileParsed::default())
                    }
                    (None, Some(old)) => fresh.adopt_file(&mut live, old, &file.path, &file.rel, &mut tag_map),
                    // New file not read yet (cancelled): next reconcile.
                    (None, None) => {}
                }
            }
            fresh.generation = if same_root { live.generation + 1 } else { 1 };
            fresh.saved_generation = if same_root { live.saved_generation } else { None };
            fresh.epoch = next_epoch();
            *live = fresh;

            // Replay what was saved while the files were being read.
            for change in pending.take().unwrap_or_default() {
                match change {
                    PendingChange::Set { path, text, meta } => {
                        if live.accepts(&path) {
                            let rel = live.rel_of(&path);
                            live.put_file(&path, &rel, meta, false, FileParsed::from_parsed(parse(&text)));
                        }
                    }
                    PendingChange::Remove(path) => {
                        live.drop_file(&path);
                    }
                }
            }
        }
        if !cancelled {
            on_progress(TagsProgress { done: total, total });
        }
        Ok(Some(Refresh { files: files.len(), reread, removed, unchanged, cancelled }))
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    fn persist(&self, data_dir: &Path) -> Result<bool> {
        let _persisting = lock(&self.persist_lock);
        let (bytes, generation, epoch, path) = {
            let live = self.read();
            let Some((root, _)) = &live.root else { return Ok(false) };
            let path = tags_index_path(data_dir, root);
            if !live.is_dirty() && path.exists() {
                return Ok(false);
            }
            let Some(bytes) = encode(&live) else { return Ok(false) };
            (bytes, live.generation, live.epoch, path)
        };
        write_data_file(&path, &bytes)?;
        let mut live = self.write();
        if live.epoch == epoch {
            live.saved_generation = Some(generation);
        }
        Ok(true)
    }

    /// Persists the index if it has changes that are not on disk yet (after
    /// `update_file`, `remove_file`, a cancelled reconcile or a `build`), or
    /// if its file is missing. Returns whether a file was written; `false`
    /// when there was nothing to save or no vault is loaded. Atomic write in
    /// `data_dir/tags/`; cheap enough to call after each save (debounced)
    /// and when the application closes.
    pub fn save(&self, data_dir: &Path) -> Result<bool> {
        self.persist(data_dir)
    }

    /// True if the index holds changes that have not been persisted.
    pub fn is_dirty(&self) -> bool {
        let live = self.read();
        live.root.is_some() && live.is_dirty()
    }

    /// Root of the vault held by the index, if any.
    pub fn root(&self) -> Option<String> {
        self.read().root.as_ref().map(|(root, _)| root.clone())
    }

    // -----------------------------------------------------------------------
    // Incremental changes
    // -----------------------------------------------------------------------

    fn apply_update(&self, path: String, text: &str, meta: Option<(u64, u64)>) -> bool {
        if let Some(queue) = lock(&self.pending).as_mut() {
            queue.push(PendingChange::Set { path: path.clone(), text: text.to_string(), meta });
        }
        // Parse outside the write lock.
        let parsed = FileParsed::from_parsed(parse(text));
        let mut live = self.write();
        if !live.accepts(&path) {
            return false;
        }
        let rel = live.rel_of(&path);
        live.put_file(&path, &rel, meta, false, parsed);
        true
    }

    /// Reflects the new content of one file right after it was saved:
    /// `size` and `mtime_ms` are the values returned by `save_file`, so that
    /// the next reconcile does not read the file again. Returns `false` if
    /// the file does not belong in the index (not `.md`, outside the indexed
    /// vault, or below a hidden folder).
    pub fn update_file_with_meta(&self, path: &Path, text: &str, size: u64, mtime_ms: u64) -> bool {
        let Ok(path) = absolute_string(path) else { return false };
        self.apply_update(path, text, Some((size, mtime_ms)))
    }

    /// Same as [`TagIndex::update_file_with_meta`] when the caller does not
    /// have the file's metadata: the file is `stat`ed, and its size and
    /// modification time are recorded only if the size on disk is the length
    /// of `text` (otherwise the text is assumed not to be what is on disk,
    /// and the next reconcile reads the file).
    pub fn update_file(&self, path: &Path, text: &str) -> bool {
        let Ok(path) = absolute_string(path) else { return false };
        let meta = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() == text.len() as u64)
            .map(|m| (m.len(), mtime_ms(&m)));
        self.apply_update(path, text, meta)
    }

    /// Drops the occurrences of a deleted (or renamed-away) file.
    pub fn remove_file(&self, path: &Path) {
        let Ok(path) = absolute_string(path) else { return };
        if let Some(queue) = lock(&self.pending).as_mut() {
            queue.push(PendingChange::Remove(path.clone()));
        }
        self.write().drop_file(&path);
    }

    /// Empties the index and forgets its vault (vault closed). The persisted
    /// file is left in place.
    pub fn clear(&self) {
        *self.write() = Inner { epoch: next_epoch(), ..Inner::default() };
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn stats(&self) -> TagStats {
        let live = self.read();
        TagStats {
            files: live.files.iter().filter(|f| f.present && !f.occs.is_empty()).count(),
            tags: live.live_tags(),
            occurrences: live.occurrences,
            skipped: live.files.iter().filter(|f| f.present && f.unreadable).count(),
        }
    }

    /// Tags whose name contains `query` (case-insensitive substring; a
    /// leading `#` is ignored; an empty query lists everything).
    ///
    /// Order: tags starting with the query, then tags with a level starting
    /// with it (`b` matches `a/b`), then the other matches; inside each
    /// group by decreasing count, then alphabetically. Counts aggregate the
    /// descendants, and a parent that only exists through its children
    /// (`a` when only `#a/b` is used) is listed too.
    pub fn tags_list(&self, query: &str) -> Vec<TagCount> {
        let q = query.trim().trim_start_matches('#').to_lowercase();
        let live = self.read();
        let mut hits = Vec::new();
        for node in &live.tags {
            if node.total == 0 {
                continue;
            }
            let rank = if q.is_empty() || node.key.starts_with(&q) {
                0
            } else if let Some(pos) = node.key.find(&q) {
                // Level prefix: the first match is right after a `/`, or a later one is.
                if node.key.as_bytes()[pos - 1] == b'/'
                    || node.key.match_indices(&q).any(|(p, _)| node.key.as_bytes()[p - 1] == b'/')
                {
                    1
                } else {
                    2
                }
            } else {
                continue;
            };
            hits.push((rank, node));
        }
        hits.sort_unstable_by(|(ra, a), (rb, b)| {
            ra.cmp(rb).then_with(|| b.total.cmp(&a.total)).then_with(|| a.key.cmp(&b.key))
        });
        hits.into_iter()
            .map(|(_, n)| TagCount { tag: n.display.clone(), count: n.total })
            .collect()
    }

    /// Fragments of `tag` and of its descendants, ordered by file (vault
    /// tree order) then position, truncated to `limit` if given. The tag is
    /// matched case-insensitively, with or without `#`.
    pub fn tag_fragments(&self, tag: &str, limit: Option<usize>) -> Vec<Fragment> {
        let key = tag.trim().trim_start_matches('#').trim_matches('/').to_lowercase();
        let live = self.read();
        let Some(&root) = live.by_key.get(&key) else { return Vec::new() };

        let mut postings: Vec<(u32, u32)> = Vec::with_capacity(live.tags[root as usize].total as usize);
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let node = &live.tags[id as usize];
            postings.extend_from_slice(&node.postings);
            stack.extend_from_slice(&node.children);
        }
        if let Some(limit) = limit {
            if postings.len() > limit {
                if limit == 0 {
                    return Vec::new();
                }
                postings.select_nth_unstable(limit - 1);
                postings.truncate(limit);
            }
        }
        postings.sort_unstable();

        postings
            .into_iter()
            .map(|(file, idx)| {
                let data = &live.files[file as usize];
                let o = &data.occs[idx as usize];
                Fragment {
                    path: data.path.clone(),
                    tag: live.tags[o.tag as usize].display.clone(),
                    source: o.source,
                    start_line: o.start_line,
                    end_line: o.end_line,
                    tag_line: o.tag_line,
                    tag_col: o.tag_col,
                    excerpt: data.excerpts[o.excerpt as usize].to_string(),
                }
            })
            .collect()
    }
}
