//! Index of a vault: tags, links and full-text search, fed by one single
//! read of each note.
//!
//! Reading a note is the expensive operation (a synced drive answers in
//! ~150 ms per file), so every new or modified note is read once and that
//! one text feeds the three: tags and links go to the in-memory index
//! (persisted as one file, see `persist.rs`), the text goes to the full-text
//! engine (`crate::search`). [`TagIndex::reconcile`] is the single entry
//! point: it lists the vault and reads only the notes whose size or
//! modification time differ from what either store holds.
//!
//! The two stores are written independently and are never assumed to be in
//! step: each records the size and modification time of what it holds for
//! every note, and each is compared with the disk. After a crash between
//! the two writes, the notes that are stale on one side are simply read
//! again. Nothing is ever written to the vault.

use super::graph::{Graph, Target};
use super::parse::parse;
use super::persist::{decode, encode, tags_index_path};
use super::store::{has_md_extension, FileParsed, Inner};
use super::{
    Fragment, IndexReady, LinkKind, LinkRef, OutLink, ReconcileReport, TagCount, TagStats, TagsProgress,
    TagsReady, UnresolvedLink,
};
use crate::atomic::write_data_file;
use crate::error::{ApiError, Result};
use crate::paths::{absolute_string, join_rel, mtime_ms, path_key};
use crate::search::{parse_query, search_dir, SearchDoc, SearchEngine, SearchHit, SearchResult};
use crate::tree::{list_tree_cancellable, Tree};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

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

/// During a long reconcile, both stores are made durable this often, so
/// that an interrupted first indexing does not start over.
const CHECKPOINT: Duration = Duration::from_secs(5);

/// Size recorded in the search index for a text whose on-disk state is
/// unknown: never equal to a real size, so the note is read again.
const UNKNOWN_SIZE: u64 = u64::MAX;

/// How long a change made by `update_file*` / `remove_file` may wait before
/// the full-text index is committed (see
/// [`TagIndex::commit_search_if_due`]). With a caller that ticks every
/// 500 ms, a saved text is searchable 2 s after the save at the latest.
pub const SEARCH_COMMIT_DELAY: Duration = Duration::from_millis(1500);

/// How long tags and links changed by `update_file*` / `remove_file` may
/// stay unpersisted (see [`TagIndex::save_if_due`]). Losing them only costs
/// one read of each note concerned at the next launch.
pub const TAGS_SAVE_DELAY: Duration = Duration::from_secs(60);

/// Largest number of results [`TagIndex::search`] and
/// [`TagIndex::tag_fragments`] return, whatever `limit` asks for (the search
/// engine allocates in proportion to the limit).
pub const MAX_RESULTS: usize = 1000;

/// What [`TagIndex::try_reconcile_with_tree`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Done(ReconcileReport),
    /// The cancel token was set; the index is consistent and persistable.
    Cancelled,
    /// Another build or reconcile is running: nothing was done, and
    /// [`TagIndex::take_missed_reconcile`] will say so once.
    Busy,
}

/// What [`TagIndex::housekeeping`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Housekeeping {
    /// The full-text index was committed: searches now see the last saves.
    pub search_committed: bool,
    /// Tags and links were written to `data_dir`.
    pub tags_saved: bool,
}

enum PendingChange {
    Set { path: String, text: String, meta: Option<(u64, u64)> },
    Remove(String),
}

/// Changes made by the application while a build or reconcile is running.
#[derive(Default)]
struct Touched {
    /// Path keys: a result read from disk for one of them is outdated.
    keys: HashSet<String>,
    /// The changes themselves, kept only for `build`, which replaces the
    /// whole content at the end and must apply them again.
    replay: Option<Vec<PendingChange>>,
}

/// A Markdown file found by the directory walk.
struct MdFile {
    rel: String,
    path: String,
    /// (size, mtime in ms) from the directory listing.
    meta: (u64, u64),
}

enum ReadOutcome {
    Parsed { parsed: FileParsed, text: Option<String> },
    NotUtf8,
    Failed,
}

/// Index of a vault: tags, links and full-text search. (The name dates from
/// when it only held tags; [`VaultIndex`](super::VaultIndex) is an alias.)
///
/// Thread-safe: share it behind an `Arc`. Queries take a read lock and stay
/// available while a build or reconcile is running.
#[derive(Default)]
pub struct TagIndex {
    inner: RwLock<Inner>,
    /// Serialises builds, reconciles and loads.
    build_lock: Mutex<()>,
    /// Serialises writes of the persisted file.
    persist_lock: Mutex<()>,
    /// Serialises the application of changes to both stores (results of a
    /// reconcile on one side, saves by the application on the other).
    apply_lock: Mutex<()>,
    touched: Mutex<Option<Touched>>,
    /// Link graph of the current content, rebuilt on demand.
    graph: Mutex<Option<Arc<Graph>>>,
    /// Full-text engine of the current vault, once opened.
    search: RwLock<Option<Arc<SearchEngine>>>,
    /// Folder of a full-text index that failed while in use: it is deleted
    /// and rebuilt, not opened again, by the next reconcile of its vault.
    search_rebuild: Mutex<Option<PathBuf>>,
    /// A reconcile is running (it commits and persists on its own schedule).
    reconciling: AtomicBool,
    /// A reconcile was skipped because another pass was running.
    missed_reconcile: AtomicBool,
    /// When tags or links first changed without being persisted since.
    unsaved_since: Mutex<Option<Instant>>,
    /// Number of times the link graph was built (diagnostics and tests).
    graph_builds: AtomicU64,
}

/// Resets `TagIndex::reconciling` when the pass ends, however it ends.
struct Reconciling<'a>(&'a AtomicBool);

impl Drop for Reconciling<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Lets go of a full-text engine. Its uncommitted changes are committed
/// first: they would otherwise be lost with its writer.
fn release_engine(slot: &mut Option<Arc<SearchEngine>>) {
    if let Some(engine) = slot.take() {
        if !engine.is_broken() {
            engine.commit(false);
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Splits a listing into Markdown files and the other visible files.
fn split_listing(tree: &Tree) -> (String, Vec<MdFile>, Vec<String>) {
    let root = tree.root.clone();
    let mut files = Vec::new();
    let mut others = Vec::new();
    for entry in &tree.entries {
        if entry.is_dir {
            continue;
        }
        if has_md_extension(&entry.path) {
            files.push(MdFile {
                path: join_rel(&root, &entry.path),
                rel: entry.path.clone(),
                meta: (entry.size, entry.mtime_ms),
            });
        } else {
            others.push(entry.path.clone());
        }
    }
    (root, files, others)
}

/// Rebuilds `live` with its notes in the order of `order` (the vault tree),
/// so that tag spelling and result order do not depend on the history of
/// updates. Notes that are not in `order` are kept after the others.
fn reorder(live: &mut Inner, order: &[MdFile]) {
    let Some((root, _)) = live.root.clone() else { return };
    let mut fresh = Inner::with_root(&root);
    let mut tag_map = vec![u32::MAX; live.tags.len()];
    let mut moved = vec![false; live.files.len()];
    for file in order {
        if let Some(&id) = live.by_path.get(&path_key(&file.path)) {
            if live.files[id as usize].present && !moved[id as usize] {
                moved[id as usize] = true;
                fresh.adopt_file(live, id, &file.path, &file.rel, &mut tag_map);
            }
        }
    }
    for id in 0..live.files.len() {
        if live.files[id].present && !moved[id] {
            let (path, rel) = (live.files[id].path.clone(), live.files[id].rel.clone());
            fresh.adopt_file(live, id as u32, &path, &rel, &mut tag_map);
        }
    }
    fresh.others = std::mem::take(&mut live.others);
    fresh.generation = live.generation;
    fresh.saved_generation = live.saved_generation;
    fresh.renew();
    *live = fresh;
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

    fn engine(&self) -> Option<Arc<SearchEngine>> {
        self.search.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Full-text engine of `root`, opened (and created if asked) on first
    /// use and kept until the vault changes.
    ///
    /// An engine that failed while in use (damaged files) is not handed out
    /// again: with `create` its folder is deleted and an empty index is
    /// started, which the caller fills by reading every note (the manifest
    /// that says what is indexed goes away with the engine); without,
    /// `None`.
    fn open_engine(&self, data_dir: &Path, root: &str, create: bool) -> Option<Arc<SearchEngine>> {
        let dir = search_dir(data_dir, root);
        let mut slot = self.search.write().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = slot.as_ref() {
            if engine.dir() == dir {
                if !engine.is_broken() {
                    return Some(Arc::clone(engine));
                }
                *lock(&self.search_rebuild) = Some(dir.clone());
                if !create {
                    return None;
                }
            }
        }
        // Release the previous vault's index before opening another one.
        release_engine(&mut slot);
        let mut rebuild = lock(&self.search_rebuild);
        let engine = if rebuild.as_deref() == Some(dir.as_path()) {
            if !create {
                return None;
            }
            SearchEngine::recreate(&dir)?
        } else {
            SearchEngine::open(&dir, create)?
        };
        *rebuild = None;
        let engine = Arc::new(engine);
        *slot = Some(Arc::clone(&engine));
        Some(engine)
    }

    // -----------------------------------------------------------------------
    // Filling the index
    // -----------------------------------------------------------------------

    /// Reads and analyses `todo` (indices into `files`) with a bounded pool
    /// of threads and hands each result to `sink` on the calling thread, in
    /// completion order. Stops early when `cancel` is set.
    fn read_stream(
        files: &[MdFile],
        todo: &[usize],
        options: BuildOptions,
        cancel: &CancelToken,
        keep_text: bool,
        mut sink: impl FnMut(usize, ReadOutcome),
    ) {
        let total = todo.len();
        if total == 0 {
            return;
        }
        let threads = options.threads.unwrap_or_else(default_threads).clamp(1, 64).min(total);
        let next = AtomicUsize::new(0);
        // Bounded: texts waiting to be applied never pile up in memory.
        let (sender, receiver) = mpsc::sync_channel::<(usize, ReadOutcome)>(threads * 4);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let sender = sender.clone();
                let next = &next;
                scope.spawn(move || loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    if n >= total {
                        break;
                    }
                    let index = todo[n];
                    // A panic while analysing one note must not take the
                    // whole pass down (the scope would propagate it): the
                    // note counts as unread and is tried again next time.
                    let read = std::panic::catch_unwind(|| match fs::read(&files[index].path) {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(text) => {
                                let parsed = FileParsed::from_parsed(parse(&text));
                                ReadOutcome::Parsed { parsed, text: keep_text.then_some(text) }
                            }
                            Err(_) => ReadOutcome::NotUtf8,
                        },
                        Err(_) => ReadOutcome::Failed,
                    });
                    let outcome = read.unwrap_or(ReadOutcome::Failed);
                    if sender.send((index, outcome)).is_err() {
                        break;
                    }
                });
            }
            drop(sender);
            for (index, outcome) in receiver {
                sink(index, outcome);
            }
        });
    }

    /// Indexes every `.md` file of the vault (hidden files and folders
    /// excluded) and replaces the tags and links of the index. Reads every
    /// file, persists nothing and does not touch the full-text index;
    /// prefer [`TagIndex::reconcile`] in the application.
    ///
    /// Blocking: run it on a background thread. `on_progress` is called at
    /// most every few dozen files, with non-decreasing `done`, and once at
    /// the end with `done == total`. Returns `Ok(None)` if cancelled, in
    /// which case the previous content is kept. Unreadable and non-UTF-8
    /// files are skipped (see [`TagIndex::stats`]).
    pub fn build(
        &self,
        vault: &Path,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<Option<TagsReady>> {
        let _building = lock(&self.build_lock);
        let started = Instant::now();
        let Some(tree) = list_tree_cancellable(vault, &|| cancel.is_cancelled())? else { return Ok(None) };
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let (root, files, others) = split_listing(&tree);
        drop(tree);
        let todo: Vec<usize> = (0..files.len()).collect();
        let total = todo.len();
        *lock(&self.touched) = Some(Touched { keys: HashSet::new(), replay: Some(Vec::new()) });
        on_progress(TagsProgress { done: 0, total });

        let mut outcomes: Vec<Option<ReadOutcome>> = (0..total).map(|_| None).collect();
        let step = (total / 100).clamp(16, 256);
        let mut done = 0;
        Self::read_stream(&files, &todo, options, cancel, false, |index, outcome| {
            outcomes[index] = Some(outcome);
            done += 1;
            if done % step == 0 && done < total {
                on_progress(TagsProgress { done, total });
            }
        });
        if cancel.is_cancelled() {
            *lock(&self.touched) = None;
            return Ok(None);
        }

        // In tree order, so that tag spelling and result order are
        // deterministic whatever the thread scheduling.
        let mut fresh = Inner::with_root(&root);
        for (file, outcome) in files.iter().zip(outcomes) {
            match outcome {
                Some(ReadOutcome::Parsed { parsed, .. }) => {
                    fresh.put_file(&file.path, &file.rel, Some(file.meta), false, parsed)
                }
                Some(ReadOutcome::NotUtf8) => {
                    fresh.put_file(&file.path, &file.rel, Some(file.meta), true, FileParsed::default())
                }
                Some(ReadOutcome::Failed) | None => {
                    fresh.put_file(&file.path, &file.rel, None, true, FileParsed::default())
                }
            }
        }
        fresh.set_others(others);
        fresh.generation = 1;
        fresh.saved_generation = None;
        fresh.renew();

        let ready = {
            let _apply = lock(&self.apply_lock);
            let changes = lock(&self.touched).take().and_then(|t| t.replay).unwrap_or_default();
            let mut live = self.write();
            *live = fresh;
            // What the application saved while the files were being read.
            for change in changes {
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
            TagsReady {
                files: total,
                tags: live.live_tags(),
                occurrences: live.occurrences,
                ms: started.elapsed().as_millis() as u64,
            }
        };
        // A full-text index opened for another vault no longer applies.
        let stale = self.engine().is_some_and(|e| e.dir().file_name() != search_dir(Path::new(""), &root).file_name());
        if stale {
            release_engine(&mut self.search.write().unwrap_or_else(|e| e.into_inner()));
        }
        on_progress(TagsProgress { done: total, total });
        Ok(Some(ready))
    }

    /// Loads what was persisted for `vault` in `data_dir` and replaces the
    /// content of the index, without reading any note: tags and links are
    /// usable at once. Returns `None`, and leaves the index untouched, if
    /// there is no usable file (missing, truncated, corrupted, written by
    /// another version or for another root). Follow it with
    /// [`TagIndex::open_search`], then [`TagIndex::reconcile`] in the
    /// background.
    pub fn load_persisted(&self, data_dir: &Path, vault: &Path) -> Option<TagsReady> {
        let _building = lock(&self.build_lock);
        let started = Instant::now();
        let root = absolute_string(vault).ok()?;
        let bytes = fs::read(tags_index_path(data_dir, &root)).ok()?;
        let mut inner = decode(&bytes, &root)?;
        inner.renew();
        let ready = TagsReady {
            files: inner.md_files(),
            tags: inner.live_tags(),
            occurrences: inner.occurrences,
            ms: 0,
        };
        {
            let _apply = lock(&self.apply_lock);
            *self.write() = inner;
        }
        // A full-text index opened for another vault no longer applies.
        let other = self.engine().is_some_and(|e| e.dir() != search_dir(data_dir, &root));
        if other {
            release_engine(&mut self.search.write().unwrap_or_else(|e| e.into_inner()));
        }
        *lock(&self.unsaved_since) = None;
        Some(TagsReady { ms: started.elapsed().as_millis() as u64, ..ready })
    }

    /// Opens the full-text index persisted for `vault`, if there is a usable
    /// one, so that [`TagIndex::search`] answers before any reconcile. The
    /// index is memory-mapped: nothing is loaded up front. Returns whether
    /// search is ready (`false` when the index is missing, damaged or was
    /// never completed; `reconcile` then builds or finishes it).
    pub fn open_search(&self, data_dir: &Path, vault: &Path) -> bool {
        let Ok(root) = absolute_string(vault) else { return false };
        self.open_engine(data_dir, &root, false).is_some_and(|e| e.is_complete())
    }

    /// Brings tags, links and full-text search in line with the vault,
    /// reading as little as possible, then persists.
    ///
    /// The vault is listed (same rules as the tree) and, for each `.md`
    /// file, its size and modification time are compared with what the
    /// in-memory index and the full-text index each hold. A note is read,
    /// once, if either is missing or out of date, and that one read updates
    /// both; notes that disappeared are removed; the list of other files is
    /// refreshed for link resolution. With an empty index, or the index of
    /// another vault, this is a full build. A full-text index that is
    /// missing, damaged or behind (the application was killed between the
    /// two writes) is repaired the same way, by reading only what it lacks.
    ///
    /// Both stores are made durable every few seconds during a long pass and
    /// at the end, also when the pass is cancelled: an interrupted first
    /// indexing resumes where it stopped. A failure to write the persisted
    /// file is not an error (`persisted` is then `false`; call
    /// [`TagIndex::save`] to get the reason).
    ///
    /// Blocking: run it on a background thread. Results are applied as they
    /// arrive, so queries see the index fill up. Progress counts the notes
    /// to read (`total` is 0 when nothing changed). A note whose content
    /// changed without its size or modification time changing is not seen.
    /// Cancellation returns `Ok(None)`; the index stays consistent.
    ///
    /// The listing itself is cancellable and is all or nothing: if it is
    /// cancelled or fails (a folder that cannot be read, a listing thread
    /// that died), nothing is touched. Notes are only ever removed on the
    /// strength of a complete listing. After a cancelled pass, what was
    /// already read is in memory and in the committed full-text index;
    /// [`TagIndex::save`] persists it and the next launch resumes from
    /// there.
    ///
    /// Persistence at the end: tags and links are written if the pass read
    /// or removed notes (or if their file is missing). Changes that came
    /// only from `update_file*` / `remove_file` are left to
    /// [`TagIndex::save_if_due`] and [`TagIndex::save`].
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
        let Some(tree) = list_tree_cancellable(vault, &|| cancel.is_cancelled())? else { return Ok(None) };
        self.reconcile_listed(data_dir, &tree, started, options, cancel, on_progress)
    }

    /// Same as [`TagIndex::reconcile`] with a listing the caller already
    /// has, so that one pass lists the vault once for both the tree and the
    /// index (see [`refresh_tree`](crate::tree::refresh_tree)): show the
    /// tree first, then call this.
    ///
    /// `tree` must be a complete listing made for this pass
    /// ([`list_tree`](crate::list_tree), `refresh_tree`): notes it does not
    /// contain are removed from the index. A tree read from a snapshot is
    /// refused (`invalid`).
    pub fn reconcile_with_tree(
        &self,
        data_dir: &Path,
        tree: &Tree,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<Option<ReconcileReport>> {
        Self::check_listing(tree)?;
        let _building = lock(&self.build_lock);
        self.reconcile_listed(data_dir, tree, Instant::now(), options, cancel, on_progress)
    }

    /// Same as [`TagIndex::reconcile_with_tree`], but never waits for a
    /// build or reconcile that is already running (the first indexing of a
    /// vault can take minutes on a synced drive): it answers
    /// [`ReconcileOutcome::Busy`] at once and remembers that a pass was
    /// skipped. The caller that runs the long pass asks
    /// [`TagIndex::take_missed_reconcile`] when it ends and, if `true`,
    /// schedules a new pass.
    pub fn try_reconcile_with_tree(
        &self,
        data_dir: &Path,
        tree: &Tree,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<ReconcileOutcome> {
        Self::check_listing(tree)?;
        let _building = match self.build_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                self.missed_reconcile.store(true, Ordering::Release);
                return Ok(ReconcileOutcome::Busy);
            }
        };
        Ok(match self.reconcile_listed(data_dir, tree, Instant::now(), options, cancel, on_progress)? {
            Some(report) => ReconcileOutcome::Done(report),
            None => ReconcileOutcome::Cancelled,
        })
    }

    /// True while a reconcile is running.
    pub fn is_reconciling(&self) -> bool {
        self.reconciling.load(Ordering::Acquire)
    }

    /// Whether [`TagIndex::try_reconcile_with_tree`] answered `Busy` since
    /// the last pass started; resets the flag.
    pub fn take_missed_reconcile(&self) -> bool {
        self.missed_reconcile.swap(false, Ordering::AcqRel)
    }

    fn check_listing(tree: &Tree) -> Result<()> {
        if tree.from_snapshot {
            return Err(ApiError::invalid("a tree read from a snapshot is not a listing of the vault"));
        }
        Ok(())
    }

    /// The pass itself. The caller holds `build_lock`.
    fn reconcile_listed(
        &self,
        data_dir: &Path,
        tree: &Tree,
        started: Instant,
        options: BuildOptions,
        cancel: &CancelToken,
        on_progress: impl Fn(TagsProgress) + Sync,
    ) -> Result<Option<ReconcileReport>> {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        self.reconciling.store(true, Ordering::Release);
        let _pass = Reconciling(&self.reconciling);
        // This pass works from a listing at least as recent as the skipped one.
        self.missed_reconcile.store(false, Ordering::Release);
        let (root, files, others) = split_listing(tree);
        let root_key = path_key(&root);
        // Without a writable full-text index (folder held by another
        // process), tags and links are still reconciled. An index that was
        // opened from disk is verified once: if a file is damaged, the
        // second call deletes it and starts an empty one, filled below.
        let mut engine = self.open_engine(data_dir, &root, true);
        if engine.as_ref().is_some_and(|e| !e.verify()) {
            // Let go of it first: its files cannot be deleted while they
            // are mapped (Windows).
            drop(engine.take());
            engine = self.open_engine(data_dir, &root, true);
        }
        let engine = engine.filter(|e| e.writable());

        // What do the two stores already hold?
        let mut todo: Vec<usize> = Vec::new();
        // Notes whose tags and links are already up to date: if they are
        // read, it is only for the full-text index.
        let mut index_current = vec![false; files.len()];
        let removed;
        let known_before;
        let others_changed;
        // Whether the notes already known are in tree order (they are not
        // after a pass that was interrupted before it could reorder them).
        let mut ordered = true;
        {
            let _apply = lock(&self.apply_lock);
            let mut live = self.write();
            if live.root_key() != Some(root_key.as_str()) {
                let mut fresh = Inner::with_root(&root);
                fresh.renew();
                *live = fresh;
                *lock(&self.unsaved_since) = None;
            }
            known_before = live.files.len();
            let mut seen = vec![false; live.files.len()];
            let mut last_id = None;
            for (i, file) in files.iter().enumerate() {
                let (in_index, unreadable) = match live.by_path.get(&path_key(&file.path)) {
                    Some(&id) if live.files[id as usize].present => {
                        seen[id as usize] = true;
                        ordered &= last_id.is_none_or(|last| last < id);
                        last_id = Some(id);
                        let data = &live.files[id as usize];
                        (data.meta == Some(file.meta), data.unreadable)
                    }
                    _ => (false, false),
                };
                // A note that is not text has nothing to give to the search.
                let in_search = unreadable || engine.as_ref().is_none_or(|e| e.meta(&file.rel) == Some(file.meta));
                index_current[i] = in_index;
                if !(in_index && in_search) {
                    todo.push(i);
                }
            }
            let gone: Vec<String> = live
                .files
                .iter()
                .zip(&seen)
                .filter(|(f, seen)| f.present && !**seen)
                .map(|(f, _)| f.path.clone())
                .collect();
            for path in &gone {
                live.drop_file(path);
            }
            removed = gone.len();
            others_changed = live.set_others(others);
            if let Some(engine) = &engine {
                let on_disk: HashSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
                for rel in engine.paths() {
                    if !on_disk.contains(rel.as_str()) {
                        engine.delete(&rel);
                    }
                }
            }
            *lock(&self.touched) = Some(Touched::default());
        }
        let total = todo.len();
        let unchanged = files.len() - total;
        on_progress(TagsProgress { done: 0, total });

        // Read what is stale, apply each result to both stores as it comes.
        let step = (total / 100).clamp(16, 256);
        let mut reread = 0;
        let mut unread = 0;
        let mut checkpoint = Instant::now();
        Self::read_stream(&files, &todo, options, cancel, engine.is_some(), |index, outcome| {
            let file = &files[index];
            {
                let _apply = lock(&self.apply_lock);
                // Saved by the application meanwhile: its version is newer.
                let superseded = lock(&self.touched).as_ref().is_some_and(|t| t.keys.contains(&path_key(&file.path)));
                if !superseded {
                    match outcome {
                        ReadOutcome::Parsed { parsed, text } => {
                            let tags = parsed.tags_with_ancestors();
                            if !index_current[index] {
                                self.write().put_file(&file.path, &file.rel, Some(file.meta), false, parsed);
                            }
                            if let (Some(engine), Some(text)) = (&engine, text) {
                                let doc = SearchDoc {
                                    rel: &file.rel,
                                    text: &text,
                                    size: file.meta.0,
                                    mtime_ms: file.meta.1,
                                    tags: &tags,
                                };
                                // A refusal means the writer had failed and
                                // was dropped with what it had not committed
                                // (see `SearchEngine::with_writer`): a second
                                // try gets a new one, so this note at least
                                // is not lost. Those lost before it are
                                // counted in `pending` at the end.
                                if !engine.upsert(&doc) {
                                    engine.upsert(&doc);
                                }
                            }
                        }
                        ReadOutcome::NotUtf8 => {
                            self.write().put_file(&file.path, &file.rel, Some(file.meta), true, FileParsed::default());
                            if let Some(engine) = &engine {
                                engine.delete(&file.rel);
                            }
                        }
                        // Unknown size/mtime: tried again by the next reconcile.
                        ReadOutcome::Failed => {
                            unread += 1;
                            self.write().put_file(&file.path, &file.rel, None, true, FileParsed::default());
                        }
                    }
                }
            }
            reread += 1;
            if reread % step == 0 && reread < total {
                on_progress(TagsProgress { done: reread, total });
            }
            if checkpoint.elapsed() >= CHECKPOINT {
                if let Some(engine) = &engine {
                    engine.commit(false);
                }
                let _ = self.persist(data_dir, false);
                checkpoint = Instant::now();
            }
        });
        let cancelled = cancel.is_cancelled();

        let saved_meanwhile;
        {
            let _apply = lock(&self.apply_lock);
            saved_meanwhile = lock(&self.touched).take().map(|t| t.keys).unwrap_or_default();
            let mut live = self.write();
            // New notes were appended: put everything back in tree order.
            if live.files.len() > known_before || !ordered {
                reorder(&mut live, &files);
            }
        }
        if let Some(engine) = &engine {
            engine.commit(!cancelled);
        }
        // Tags and links are rewritten whole (megabytes on a large vault):
        // only when this pass changed something, or to create the file.
        let changed = reread > 0 || removed > 0 || others_changed;
        let persisted = self.persist(data_dir, !changed).unwrap_or(false);
        if cancelled {
            return Ok(None);
        }
        on_progress(TagsProgress { done: total, total });
        let live = self.read();
        // What is still behind the listing after the last commit: reads
        // that failed, full-text writes lost with a writer that failed.
        // Notes saved by the application during the pass are newer than the
        // listing, not behind it.
        let pending = files
            .iter()
            .filter(|file| {
                let key = path_key(&file.path);
                if saved_meanwhile.contains(&key) {
                    return false;
                }
                let data = match live.by_path.get(&key) {
                    Some(&id) if live.files[id as usize].present => &live.files[id as usize],
                    _ => return true,
                };
                let in_index = data.meta == Some(file.meta);
                let in_search =
                    data.unreadable || engine.as_ref().is_none_or(|e| e.meta(&file.rel) == Some(file.meta));
                !(in_index && in_search)
            })
            .count();
        Ok(Some(ReconcileReport {
            files: files.len(),
            tags: live.live_tags(),
            occurrences: live.occurrences,
            ms: started.elapsed().as_millis() as u64,
            reread,
            removed,
            unchanged,
            persisted,
            pending,
            unread: unread.min(pending),
        }))
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Writes tags and links if they have unpersisted changes or if their
    /// file is missing; with `only_if_missing`, only in the second case.
    fn persist(&self, data_dir: &Path, only_if_missing: bool) -> Result<bool> {
        let _persisting = lock(&self.persist_lock);
        let (bytes, generation, epoch, path) = {
            let live = self.read();
            let Some((root, _)) = &live.root else { return Ok(false) };
            let path = tags_index_path(data_dir, root);
            if path.exists() && (only_if_missing || !live.is_dirty()) {
                return Ok(false);
            }
            let Some(bytes) = encode(&live) else { return Ok(false) };
            (bytes, live.generation, live.epoch, path)
        };
        write_data_file(&path, &bytes)?;
        let still_dirty = {
            let mut live = self.write();
            if live.epoch == epoch {
                live.saved_generation = Some(generation);
            }
            live.root.is_some() && live.is_dirty()
        };
        // Changed again while the file was being written: a new delay starts.
        *lock(&self.unsaved_since) = still_dirty.then(Instant::now);
        Ok(true)
    }

    /// Makes everything durable now: commits the full-text index if it has
    /// pending changes, then persists tags and links if they have changes
    /// that are not on disk yet (or if their file is missing). Returns
    /// whether the tags file was written; `false` when there was nothing to
    /// save or no vault is loaded. Atomic write in `data_dir/tags/`.
    ///
    /// Call it when the application closes, when a vault is closed, and
    /// after a cancelled reconcile. It never waits for a running build or
    /// reconcile: what is in memory at that moment is written, and the next
    /// launch reads again whatever is missing. During editing, use
    /// [`TagIndex::housekeeping`] instead, which spaces the writes out.
    pub fn save(&self, data_dir: &Path) -> Result<bool> {
        self.flush_search();
        self.persist(data_dir, false)
    }

    /// Persists tags and links if they have held unpersisted changes for
    /// [`TAGS_SAVE_DELAY`] (60 s) at `now`. Meant to be called from a timer
    /// with `Instant::now()`: while notes are being edited, the file (several
    /// megabytes on a large vault) is then rewritten once a minute at most
    /// instead of after every save. What is lost if the process is killed in
    /// between is read again at the next launch, one read per note saved
    /// since the last write. Returns whether the file was written.
    pub fn save_if_due(&self, data_dir: &Path, now: Instant) -> Result<bool> {
        if !self.is_dirty() {
            *lock(&self.unsaved_since) = None;
            return Ok(false);
        }
        let since = *lock(&self.unsaved_since).get_or_insert(now);
        if now.saturating_duration_since(since) < TAGS_SAVE_DELAY {
            return Ok(false);
        }
        self.persist(data_dir, false)
    }

    /// True while saves made through `update_file*` / `remove_file` are not
    /// visible to [`TagIndex::search`] yet (they wait for a commit).
    pub fn search_commit_pending(&self) -> bool {
        self.engine().is_some_and(|e| e.has_pending())
    }

    /// Commits the full-text index if a change has been waiting for
    /// [`SEARCH_COMMIT_DELAY`] at `now`. Returns whether a commit was made.
    /// Does nothing while a reconcile is running: it commits on its own
    /// schedule (every few seconds and at its end), and extra commits would
    /// only fragment the index.
    pub fn commit_search_if_due(&self, now: Instant) -> bool {
        let Some(engine) = self.engine() else { return false };
        if engine.is_broken() || self.is_reconciling() {
            return false;
        }
        match engine.pending_since() {
            Some(since) if now.saturating_duration_since(since) >= SEARCH_COMMIT_DELAY => engine.commit(false),
            _ => false,
        }
    }

    /// Commits the full-text index now if it has pending changes. Returns
    /// whether a commit was made.
    pub fn flush_search(&self) -> bool {
        self.engine().is_some_and(|e| !e.is_broken() && e.has_pending() && e.commit(false))
    }

    /// The one call a timer needs (every 500 ms or so, with
    /// `Instant::now()`): [`TagIndex::commit_search_if_due`], then
    /// [`TagIndex::save_if_due`]. Costs two lock acquisitions when there is
    /// nothing to do. A failure to write the tags file is not reported here
    /// (it is tried again 60 s later, and at [`TagIndex::save`]).
    pub fn housekeeping(&self, data_dir: &Path, now: Instant) -> Housekeeping {
        Housekeeping {
            search_committed: self.commit_search_if_due(now),
            tags_saved: self.save_if_due(data_dir, now).unwrap_or(false),
        }
    }

    /// True if tags or links hold changes that have not been persisted.
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
        // Parse outside the locks.
        let parsed = FileParsed::from_parsed(parse(text));
        let tags = parsed.tags_with_ancestors();
        let _apply = lock(&self.apply_lock);
        if let Some(touched) = lock(&self.touched).as_mut() {
            touched.keys.insert(path_key(&path));
            if let Some(replay) = touched.replay.as_mut() {
                replay.push(PendingChange::Set { path: path.clone(), text: text.to_string(), meta });
            }
        }
        let rel = {
            let mut live = self.write();
            if !live.accepts(&path) {
                return false;
            }
            let rel = live.rel_of(&path);
            live.put_file(&path, &rel, meta, false, parsed);
            rel
        };
        lock(&self.unsaved_since).get_or_insert_with(Instant::now);
        // Same text into the full-text index, without waiting for a commit
        // (tens of milliseconds): searches see it after the next one.
        if !rel.is_empty() {
            if let Some(engine) = self.engine() {
                let (size, mtime_ms) = meta.unwrap_or((UNKNOWN_SIZE, 0));
                let doc = SearchDoc { rel: &rel, text, size, mtime_ms, tags: &tags };
                // A refusal means the writer had failed and was dropped: a
                // second try gets a new one. If that fails too, the note
                // stays out of the manifest and the next reconcile reads it.
                if !engine.upsert(&doc) {
                    engine.upsert(&doc);
                }
            }
        }
        true
    }

    /// Reflects the new content of one note right after it was saved, in
    /// tags, links and full-text search, from the text in memory: the disk
    /// is not read. `size` and `mtime_ms` are the values returned by
    /// `save_file`, so that the next reconcile does not read the file again.
    /// When the call returns, tags and links reflect the new text. Full-text
    /// search follows at the next commit: [`TagIndex::commit_search_if_due`]
    /// (within [`SEARCH_COMMIT_DELAY`] plus the caller's timer period),
    /// [`TagIndex::flush_search`], [`TagIndex::save`], or the end of a
    /// reconcile. If the process dies before that commit, the full-text
    /// index does not know the new content and the note is read again at
    /// the next launch.
    /// Returns `false` if the file does not belong in the index (not `.md`,
    /// outside the indexed vault, or below a hidden folder).
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

    /// Drops a deleted (or renamed-away) note from tags, links and search
    /// (search: at the next commit, as for [`TagIndex::update_file_with_meta`]).
    pub fn remove_file(&self, path: &Path) {
        let Ok(path) = absolute_string(path) else { return };
        let _apply = lock(&self.apply_lock);
        if let Some(touched) = lock(&self.touched).as_mut() {
            touched.keys.insert(path_key(&path));
            if let Some(replay) = touched.replay.as_mut() {
                replay.push(PendingChange::Remove(path.clone()));
            }
        }
        let rel = {
            let mut live = self.write();
            let rel = live.rel_of(&path);
            live.drop_file(&path);
            rel
        };
        lock(&self.unsaved_since).get_or_insert_with(Instant::now);
        if !rel.is_empty() {
            if let Some(engine) = self.engine() {
                engine.delete(&rel);
            }
        }
    }

    /// Empties the index and forgets its vault (vault closed); the full-text
    /// index is committed and closed. Persisted files are left in place.
    /// Tags and links that were not persisted are dropped: call
    /// [`TagIndex::save`] first.
    pub fn clear(&self) {
        let _apply = lock(&self.apply_lock);
        let mut fresh = Inner::default();
        fresh.renew();
        *self.write() = fresh;
        release_engine(&mut self.search.write().unwrap_or_else(|e| e.into_inner()));
        *lock(&self.graph) = None;
        *lock(&self.unsaved_since) = None;
    }

    // -----------------------------------------------------------------------
    // Links
    // -----------------------------------------------------------------------

    /// Link graph of the content behind `live`, rebuilt only if what it is
    /// made from changed since the last call: the targets of the links of a
    /// note, the set of notes, the other files. A save that leaves the links
    /// of the note as they were reuses the graph. No note is read.
    fn graph(&self, live: &Inner) -> Arc<Graph> {
        let mut slot = lock(&self.graph);
        if let Some(graph) = slot.as_ref() {
            if graph.stamp == (live.epoch, live.graph_generation) {
                return Arc::clone(graph);
            }
        }
        self.graph_builds.fetch_add(1, Ordering::Relaxed);
        let graph = Arc::new(Graph::build(live));
        *slot = Some(Arc::clone(&graph));
        graph
    }

    /// Number of times the link graph has been built since this index was
    /// created (diagnostics: it should not grow with saves that leave the
    /// links alone).
    pub fn graph_builds(&self) -> u64 {
        self.graph_builds.load(Ordering::Relaxed)
    }

    /// A value that changes whenever the answer of [`TagIndex::backlinks`],
    /// [`TagIndex::outgoing_links`] or [`TagIndex::unresolved_links`] may
    /// have changed for some note: links added, removed, moved or reworded,
    /// notes or other files added or removed, another vault loaded. Equal
    /// values mean identical answers, so a caller that shows links can skip
    /// its refresh. Values are never reused within a process.
    pub fn links_generation(&self) -> u64 {
        self.read().links_generation
    }

    fn target_path(live: &Inner, target: Target) -> String {
        match target {
            Target::Note(id) => live.files[id as usize].path.clone(),
            Target::Other(id) => match &live.root {
                Some((root, _)) => join_rel(root, &live.others[id as usize]),
                None => live.others[id as usize].clone(),
            },
        }
    }

    /// Links and embeds of the other notes of the vault that point to
    /// `path` (a note or an attachment), sorted by source path then
    /// position. Paths are absolute and usable as such by `read_file`.
    pub fn backlinks(&self, path: &Path) -> Vec<LinkRef> {
        let Ok(path) = absolute_string(path) else { return Vec::new() };
        let live = self.read();
        let graph = self.graph(&live);
        let target = match live.by_path.get(&path_key(&path)) {
            Some(&id) if live.files[id as usize].present => Target::Note(id),
            _ => match graph.by_rel.get(&live.rel_of(&path).to_lowercase()) {
                Some(&target) => target,
                None => return Vec::new(),
            },
        };
        let Some(sources) = graph.backlinks.get(&target) else { return Vec::new() };
        let mut refs: Vec<LinkRef> = sources
            .iter()
            .map(|&(file, n)| {
                let data = &live.files[file as usize];
                let link = &data.links[n as usize];
                LinkRef {
                    path: data.path.clone(),
                    kind: if link.embed { LinkKind::Embed } else { LinkKind::Link },
                    line: link.line,
                    col: link.col,
                    excerpt: link.excerpt.clone(),
                    ranges: vec![link.range],
                    heading: link.heading.clone(),
                    alias: link.alias.clone(),
                }
            })
            .collect();
        refs.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)).then(a.col.cmp(&b.col)));
        refs
    }

    /// Links and embeds contained in the note `path`, in text order, each
    /// with the absolute path of the file it resolves to (`None` if there
    /// is no such file in the vault).
    pub fn outgoing_links(&self, path: &Path) -> Vec<OutLink> {
        let Ok(path) = absolute_string(path) else { return Vec::new() };
        let live = self.read();
        let Some(&id) = live.by_path.get(&path_key(&path)) else { return Vec::new() };
        let data = &live.files[id as usize];
        if !data.present {
            return Vec::new();
        }
        let graph = self.graph(&live);
        data.links
            .iter()
            .zip(&graph.resolved[id as usize])
            .map(|(link, resolved)| OutLink {
                target: link.target.clone(),
                resolved: resolved.map(|t| Self::target_path(&live, t)),
                kind: if link.embed { LinkKind::Embed } else { LinkKind::Link },
                line: link.line,
                col: link.col,
                heading: link.heading.clone(),
                alias: link.alias.clone(),
            })
            .collect()
    }

    /// Link targets that match no file of the vault, most frequent first,
    /// at most `limit` (500 by default, [`MAX_RESULTS`] at most). `target` is exactly the text of
    /// the corresponding [`OutLink::target`]; `sources` holds up to five of
    /// the notes that contain it.
    pub fn unresolved_links(&self, limit: Option<usize>) -> Vec<UnresolvedLink> {
        let live = self.read();
        let graph = self.graph(&live);
        graph
            .unresolved
            .iter()
            .take(limit.unwrap_or(500).min(MAX_RESULTS))
            .map(|u| UnresolvedLink {
                target: u.target.clone(),
                count: u.count,
                sources: u.sources.iter().map(|&id| live.files[id as usize].path.clone()).collect(),
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Why a write to the full-text index last failed, if one did since the
    /// last call (diagnostics: a file of the index held by another program,
    /// a full disk). The notes concerned are reported by
    /// [`ReconcileReport::pending`] and read again.
    pub fn take_search_error(&self) -> Option<String> {
        self.engine().and_then(|e| e.take_last_error())
    }

    /// True once the full-text index of the current vault has been filled
    /// completely at least once. Before that, [`TagIndex::search`] returns
    /// nothing or partial results.
    pub fn search_ready(&self) -> bool {
        self.engine().is_some_and(|e| e.is_complete() && !e.is_broken())
    }

    /// True when the full-text index failed while in use (damaged files):
    /// [`TagIndex::search`] answers nothing until the next reconcile, which
    /// deletes it and builds it again by reading every note. The caller
    /// should schedule that reconcile. Stays true if the folder could not
    /// be emptied yet (files still held): the reconcile after tries again.
    pub fn search_needs_rebuild(&self) -> bool {
        self.engine().is_some_and(|e| e.is_broken()) || lock(&self.search_rebuild).is_some()
    }

    /// Full-text search (see CONTRACT.md, "Recherche"): at most `limit`
    /// files (50 by default, [`MAX_RESULTS`] at most), best first, each with
    /// up to three excerpts taken from the index. Never fails and never
    /// panics: without a full-text index, for an empty query, or when the
    /// index turns out to be damaged (`searchReady` is then `false`, see
    /// [`TagIndex::search_needs_rebuild`]), the result is empty.
    ///
    /// Saves are visible after the next commit (see
    /// [`TagIndex::update_file_with_meta`]). As a safety net for a caller
    /// whose timer did not run, a change that has waited longer than
    /// [`SEARCH_COMMIT_DELAY`] is committed here first.
    pub fn search(&self, query: &str, limit: Option<usize>) -> SearchResult {
        let started = Instant::now();
        let mut result = SearchResult { query: query.to_string(), total: 0, hits: Vec::new(), ms: 0.0, search_ready: false };
        let limit = limit.unwrap_or(50).min(MAX_RESULTS);
        if let Some(engine) = self.engine().filter(|e| !e.is_broken()) {
            let parsed = parse_query(query);
            if !parsed.is_empty() {
                self.commit_search_if_due(Instant::now());
                if let Some((total, hits)) = engine.search(&parsed, limit) {
                    let root = self.root().unwrap_or_default();
                    result.total = total;
                    result.hits = hits
                        .into_iter()
                        .map(|hit| SearchHit {
                            path: join_rel(&root, &hit.rel),
                            score: hit.score,
                            name_match: hit.name_match,
                            match_count: hit.match_count,
                            matches: hit.matches,
                        })
                        .collect();
                }
            }
            result.search_ready = engine.is_complete() && !engine.is_broken();
        }
        result.ms = (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0;
        result
    }

    /// State of the whole index, as the payload of the `index-ready` event.
    /// `report` is the result of the reconcile that just finished, if any.
    pub fn index_ready(&self, from_cache: bool, ms: u64, report: Option<&ReconcileReport>) -> IndexReady {
        let live = self.read();
        let graph = self.graph(&live);
        let engine = self.engine();
        IndexReady {
            vault: live.root.as_ref().map(|(root, _)| root.clone()).unwrap_or_default(),
            from_cache,
            files: live.md_files(),
            tags: live.live_tags(),
            occurrences: live.occurrences,
            links: graph.links,
            unresolved_links: graph.unresolved.len(),
            links_generation: live.links_generation,
            search_ready: engine.as_ref().is_some_and(|e| e.is_complete() && !e.is_broken()),
            search_docs: engine.as_ref().map_or(0, |e| e.num_docs()),
            reread: report.map(|r| r.reread),
            removed: report.map(|r| r.removed),
            pending: report.map(|r| r.pending),
            ms,
        }
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
    /// tree order) then position, truncated to `limit` if given (a limit
    /// above [`MAX_RESULTS`] counts as [`MAX_RESULTS`]). The tag is matched
    /// case-insensitively, with or without `#`.
    pub fn tag_fragments(&self, tag: &str, limit: Option<usize>) -> Vec<Fragment> {
        let limit = limit.map(|l| l.min(MAX_RESULTS));
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
