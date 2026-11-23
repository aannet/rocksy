#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell of Rocksy: thin wrappers around `rocksy-core` (see CONTRACT.md).
//!
//! What lives here is threading, events and window plumbing; every decision
//! that can be tested is in the core (`access`, `own_writes`, `refresh`,
//! `appdata`). This crate does not build on Linux, so it has no unit tests.
//!
//! Threads:
//! * commands run on Tauri's pool; each body is wrapped in [`guarded`], so a
//!   panic becomes an error for the page instead of a promise that never
//!   settles;
//! * the refresh worker lists the open vault when the watcher or the focus
//!   safety net asks for it (one listing per pass, shared with the index);
//! * the index worker loads and reconciles the index, one request at a time,
//!   so a long first indexing never blocks the refresh worker;
//! * a 500 ms tick commits the search index and persists what is due;
//! * a 2 s poll watches the open files.
//!
//! Lock order (when two are held): `switch` > `policy` > `index_job` >
//! `tree` > the others, which are never held together.
//!
//! `open_vault` and `build_tags` of the same vault may arrive in any order
//! (the page sends both at once): the index is reconciled with the listing
//! made for the tree whichever comes first, see [`index_open`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rocksy_core as core;
use rocksy_core::paths::{absolute_string, same_path};
use rocksy_core::{
    AccessPolicy, ApiError, BuildOptions, CancelToken, ChangedPaths, Fragment, IpcTree, OwnWrites, PathOrigin,
    RefreshOutcome, Session, TagCount, TagIndex, Tree, VaultRef,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

type ApiResult<T> = Result<T, ApiError>;

/// How long the page has to answer `close-requested` before the app exits.
const CLOSE_ACK_TIMEOUT: Duration = Duration::from_secs(5);
/// How long closing a vault or the app waits for the index worker to stop.
const INDEX_STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// How long `startup_info` waits for the asset scope to be in place.
const SCOPE_WAIT: Duration = Duration::from_millis(1500);
/// How long entries updated in the in-memory tree may wait for the snapshot.
const SNAPSHOT_DELAY: Duration = Duration::from_secs(60);
/// How long `build_tags` waits for the `open_vault` of the same vault to
/// show up before listing the vault on its own (the page sends both at once
/// and they run on different threads).
const OPEN_GRACE: Duration = Duration::from_millis(300);
/// Delays before the passes that read again the notes a reconcile could not
/// read (file held by another program, hiccup of a synced drive). After the
/// last one, the watcher and the focus safety net remain.
const PENDING_RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// An open file watched for external changes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Watched {
    mtime_ms: u64,
    /// Consecutive polls that did not find the file (capped at 3).
    misses: u8,
}

/// Tree of the open vault as last listed (or as read from the snapshot).
struct KnownTree {
    tree: Arc<Tree>,
    /// Entries were updated in memory by saves and are not in the snapshot yet.
    dirty_since: Option<Instant>,
}

#[derive(Default)]
struct TreeState {
    known: Option<KnownTree>,
    /// Root of a vault whose first listing was requested and has not ended.
    listing_pending: Option<String>,
}

/// The note read ahead of the page (`StartupInfo.firstFile`).
enum FirstFile {
    Pending,
    Ready(Option<PageFile>),
    Taken,
}

struct AppState {
    /// Roaming data: vault list, session, list of opened files, timings.
    data_dir: PathBuf,
    /// Local data: tree snapshots, tags and links, full-text index.
    derived_dir: PathBuf,
    arg_files: Vec<String>,
    process_start_ms: u64,
    /// Which paths the page may reach, which folders it may register.
    policy: Mutex<AccessPolicy>,
    /// Folders already added to the scope of the asset protocol.
    asset_roots: Mutex<HashSet<String>>,
    scope_ready: (Mutex<bool>, Condvar),
    first_file: Mutex<FirstFile>,
    index: Arc<TagIndex>,
    /// Vault whose index is being served, with the token cancelling its passes.
    index_job: Mutex<Option<(String, CancelToken)>>,
    index_tx: Mutex<Option<Sender<IndexRequest>>>,
    index_busy: AtomicBool,
    /// A reconcile was already asked for to rebuild a damaged search index.
    rebuild_requested: AtomicBool,
    /// Passes already scheduled in a row for notes that could not be read.
    pending_retries: AtomicU32,
    /// The `open_vault` in flight, if any: its number and the folder asked.
    opening: Mutex<Option<(u64, String)>>,
    /// Serialises opening and closing vaults.
    switch: Mutex<()>,
    /// Number of the latest `open_vault`/`close_vault`: an older `open_vault`
    /// that finishes late installs nothing.
    open_seq: AtomicU64,
    /// Vault shown in the tree (same string as `Tree.root`), its watcher, its tree.
    current_vault: Mutex<Option<String>>,
    watcher: Mutex<Option<core::VaultWatcher>>,
    tree: Mutex<TreeState>,
    own_writes: OwnWrites,
    /// Open files and their last known mtime.
    watched: Mutex<HashMap<String, Watched>>,
    refresh_tx: Mutex<Option<Sender<RefreshRequest>>>,
    last_refresh_ms: AtomicU64,
    /// Serialises appends to the timings log (commands run concurrently).
    timing_log: Mutex<()>,
    /// Number of the close request waiting for `close_ack`, if any.
    close_wait: Mutex<Option<u64>>,
    close_seq: AtomicU64,
    /// The app is shutting down: nothing new is started.
    closing: AtomicBool,
}

/// `FileContent` of the contract: what the core read, plus `readOnly`.
#[derive(Clone, Serialize)]
struct PageFile {
    #[serde(flatten)]
    content: core::FileContent,
    #[serde(rename = "readOnly")]
    read_only: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupInfo {
    arg_files: Vec<String>,
    vaults: Vec<VaultRef>,
    session: Option<Session>,
    first_file: Option<PageFile>,
    data_dir: String,
    version: String,
    process_start_ms: u64,
}

/// Asks the refresh worker to check a vault again.
struct RefreshRequest {
    root: String,
    /// What the watcher reported; unknown for the focus safety net and for
    /// the pass that follows `open_vault`.
    changes: ChangedPaths,
    /// First pass after `open_vault`: also removes orphaned temporary files.
    opening: bool,
}

enum IndexRequest {
    /// `build_tags`: load what is persisted, then reconcile.
    Open { root: String, cancel: CancelToken },
    /// A listing is available: bring the index in line with it.
    Reconcile { tree: Arc<Tree>, cancel: CancelToken },
    /// A save changed the links of the vault: tell the page (`index-ready`).
    LinksChanged { root: String },
}

#[derive(Clone, Serialize)]
struct OpenFiles {
    paths: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChanged {
    path: String,
    mtime_ms: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
}

/// Keeps the last warnings and errors the full-text engine (tantivy) wrote
/// to the `log` facade. When one of its threads fails, the error it returns
/// only says "a worker thread encountered an error": the cause is in these
/// lines. They go to `timings.jsonl` with the `index-pending` entry.
struct EngineLog;

static ENGINE_LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static ENGINE_LOGGER: EngineLog = EngineLog;

impl log::Log for EngineLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn && metadata.target().starts_with("tantivy")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let line: String = format!("{}: {}", record.level(), record.args()).chars().take(400).collect();
            let mut lines = lock(&ENGINE_LOG);
            if lines.len() >= 8 {
                lines.pop_front();
            }
            lines.push_back(line);
        }
    }

    fn flush(&self) {}
}

fn take_engine_log() -> Vec<String> {
    lock(&ENGINE_LOG).drain(..).collect()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A poisoned lock (a thread panicked while holding it) is used anyway: the
/// data it protects is always left in a usable state.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs the body of a command. A panic (the release profile unwinds) becomes
/// an `io` error: the page gets an answer and the app keeps running.
fn guarded<T>(name: &str, body: impl FnOnce() -> ApiResult<T>) -> ApiResult<T> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(_) => Err(ApiError::io(format!("internal error in {name}"))),
    }
}

/// Runs one step of a background loop; a panic ends the step, not the thread.
fn survive(step: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(step));
}

/// Command-line arguments that designate files, made absolute against `cwd`.
fn files_from_args(args: &[String], cwd: &Path) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with("--") && !a.is_empty())
        .filter_map(|a| {
            let p = Path::new(a);
            let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
            absolute_string(&abs).ok()
        })
        .collect()
}

fn mtime_of(path: &str) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| core::paths::mtime_ms(&m))
}

// ---------------------------------------------------------------------------
// Access: opened files, asset scope, guarded read
// ---------------------------------------------------------------------------

/// Adds the folders the policy allows to the scope of the asset protocol
/// (registered vaults, folders of the files opened explicitly), each with its
/// `.obsidian` forbidden. Never a network location: `asset_roots` has none,
/// and the test is repeated here because the protocol touches a path before
/// checking it. Idempotent; a folder is never removed before the next launch
/// (the scope has no such operation).
fn sync_asset_scope(app: &AppHandle) {
    let state = app.state::<AppState>();
    let roots = lock(&state.policy).asset_roots();
    let mut applied = lock(&state.asset_roots);
    let scope = app.asset_protocol_scope();
    for root in roots {
        if core::access::is_network_path(&root, core::PathStyle::native()) || !applied.insert(root.clone()) {
            continue;
        }
        let _ = scope.allow_directory(&root, true);
        let _ = scope.forbid_directory(Path::new(&root).join(".obsidian"), true);
    }
}

/// Records files the user opened explicitly. Only for paths that come from
/// the operating system (command line, second launch, dialog, drop on the
/// window), never for a path sent by the page.
fn grant_opened(app: &AppHandle, paths: &[String]) {
    let state = app.state::<AppState>();
    let files = {
        let mut policy = lock(&state.policy);
        let mut changed = false;
        for path in paths {
            changed |= policy.allow_opened(path).unwrap_or(false);
        }
        changed.then(|| policy.opened_files())
    };
    if let Some(files) = files {
        let _ = core::save_opened(&state.data_dir, &files);
        sync_asset_scope(app);
    }
}

/// `read_file` as the page gets it: access check, read, `readOnly`, and the
/// file joins the watched ones. Also used for the note read ahead at startup.
fn read_for_page(state: &AppState, path: &str) -> ApiResult<PageFile> {
    let read_only = {
        let policy = lock(&state.policy);
        policy.check_read(path, PathOrigin::Page)?;
        !core::is_savable_name(Path::new(path)) || policy.check_write(path, PathOrigin::Page).is_err()
    };
    let content = core::read_file(Path::new(path))?;
    lock(&state.watched).insert(content.path.clone(), Watched { mtime_ms: content.mtime_ms, misses: 0 });
    Ok(PageFile { content, read_only })
}

// ---------------------------------------------------------------------------
// Commands: startup, files
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn startup_info(state: State<'_, AppState>) -> ApiResult<StartupInfo> {
    guarded("startup_info", || {
        // Images of the first note are requested right after this answer:
        // the scope has to be in place (it normally is, long before).
        {
            let (ready, signal) = &state.scope_ready;
            let guard = lock(ready);
            let _ = signal.wait_timeout_while(guard, SCOPE_WAIT, |ready| !*ready);
        }
        // Given once; a read still in flight is abandoned to `read_file`.
        let first_file = match std::mem::replace(&mut *lock(&state.first_file), FirstFile::Taken) {
            FirstFile::Ready(file) => file,
            FirstFile::Pending | FirstFile::Taken => None,
        };
        let session = core::load_session(&state.data_dir).map(|s| lock(&state.policy).sanitize_session(&s));
        Ok(StartupInfo {
            arg_files: state.arg_files.clone(),
            vaults: core::list_vaults(&state.data_dir),
            session,
            first_file,
            data_dir: state.data_dir.to_string_lossy().into_owned(),
            version: env!("CARGO_PKG_VERSION").into(),
            process_start_ms: state.process_start_ms,
        })
    })
}

#[tauri::command(async)]
fn read_file(state: State<'_, AppState>, path: String) -> ApiResult<PageFile> {
    guarded("read_file", || read_for_page(&state, &path))
}

#[tauri::command(async)]
fn save_file(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_mtime_ms: Option<u64>,
) -> ApiResult<core::SaveResult> {
    guarded("save_file", || {
        lock(&state.policy).check_write(&path, PathOrigin::Page)?;
        let saved = core::save_file(Path::new(&path), &text, expected_mtime_ms)?;
        // Everything below only happens after a successful save.
        let abs = absolute_string(Path::new(&path)).unwrap_or_else(|_| path.clone());
        // The watcher will report this write: it must not cost a listing.
        state.own_writes.record(&abs, saved.size, saved.mtime_ms, saved.created);
        {
            let mut watched = lock(&state.watched);
            let stamp = Watched { mtime_ms: saved.mtime_ms, misses: 0 };
            if path != abs && watched.contains_key(&path) {
                watched.insert(path.clone(), stamp);
            }
            watched.insert(abs.clone(), stamp);
        }
        // Tags and links now, search at the next commit; the disk is not read.
        let job_root = lock(&state.index_job).as_ref().map(|(root, _)| root.clone());
        if let Some(root) = job_root {
            let links_before = state.index.links_generation();
            let indexed = state.index.update_file_with_meta(Path::new(&abs), &text, saved.size, saved.mtime_ms);
            // The links of the vault changed: the page is told, off this
            // thread (the payload needs the link graph, built on demand).
            if indexed && state.index.links_generation() != links_before {
                if let Some(tx) = lock(&state.index_tx).clone() {
                    let _ = tx.send(IndexRequest::LinksChanged { root });
                }
            }
        }
        // Keep the tree in memory in step, so that the next listing does not
        // take this save for an external change. A created file is not in
        // the tree: the listing that follows adds it. The snapshot is
        // written later by the housekeeping tick.
        if !saved.created {
            let mut tree = lock(&state.tree);
            if let Some(known) = tree.known.as_mut() {
                if core::paths::is_inside(&abs, &known.tree.root)
                    && Arc::make_mut(&mut known.tree).set_file_meta(Path::new(&abs), saved.size, saved.mtime_ms)
                {
                    known.dirty_since.get_or_insert_with(Instant::now);
                }
            }
        }
        Ok(saved)
    })
}

// ---------------------------------------------------------------------------
// Commands: vaults
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn list_vaults(state: State<'_, AppState>) -> ApiResult<Vec<VaultRef>> {
    guarded("list_vaults", || Ok(core::list_vaults(&state.data_dir)))
}

#[tauri::command(async)]
fn add_vault(app: AppHandle, state: State<'_, AppState>, path: String) -> ApiResult<Vec<VaultRef>> {
    guarded("add_vault", || {
        // A registered vault is fully readable and writable: the page may
        // only register a folder the user chose in the folder dialog.
        lock(&state.policy).check_add_vault(&path)?;
        // The core checks the roaming directory; the local one must not be
        // inside a vault either.
        let abs = absolute_string(Path::new(&path))?;
        if absolute_string(&state.derived_dir).is_ok_and(|derived| core::paths::is_inside(&derived, &abs)) {
            return Err(ApiError::invalid(format!(
                "the application data directory is inside {abs}: it cannot be a vault"
            )));
        }
        let vaults = core::add_vault(&state.data_dir, Path::new(&path))?;
        lock(&state.policy).set_vaults(&vaults);
        sync_asset_scope(&app);
        Ok(vaults)
    })
}

#[tauri::command(async)]
fn remove_vault(state: State<'_, AppState>, path: String) -> ApiResult<Vec<VaultRef>> {
    guarded("remove_vault", || {
        let abs = absolute_string(Path::new(&path))?;
        {
            let _switch = lock(&state.switch);
            supersede_open_of(&state, &abs);
            close_current_vault(&state, Some(&abs));
        }
        let vaults = core::remove_vault(&state.data_dir, Path::new(&path))?;
        lock(&state.policy).set_vaults(&vaults);
        // The index may hold this vault without it being the open one.
        {
            let mut job = lock(&state.index_job);
            if job.as_ref().is_some_and(|(root, _)| same_path(root, &abs)) {
                if let Some((_, cancel)) = job.take() {
                    cancel.cancel();
                }
            }
            if state.index.root().is_some_and(|root| same_path(&root, &abs)) {
                wait_index_idle(&state);
                state.index.clear();
            }
        }
        // Derived data of the vault (file names, text of the notes): gone
        // with it. Best effort: a file still held is left for a later purge.
        let _ = core::purge_vault_data(&state.derived_dir, Path::new(&abs));
        Ok(vaults)
    })
}

/// Waits until the index worker is idle, [`INDEX_STOP_TIMEOUT`] at most. The
/// caller cancelled the job first, so the worker stops at the next note.
fn wait_index_idle(state: &AppState) {
    let deadline = Instant::now() + INDEX_STOP_TIMEOUT;
    while state.index_busy.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Writes the snapshot of the open vault if saves updated entries in memory
/// (`force`: now; otherwise once they are [`SNAPSHOT_DELAY`] old).
fn flush_tree_snapshot(state: &AppState, force: bool) {
    let tree = {
        let mut tree = lock(&state.tree);
        match tree.known.as_mut() {
            Some(known) if known.dirty_since.is_some_and(|since| force || since.elapsed() >= SNAPSHOT_DELAY) => {
                known.dirty_since = None;
                Some(known.tree.clone())
            }
            _ => None,
        }
    };
    if let Some(tree) = tree {
        let _ = core::save_tree_snapshot(&state.derived_dir, &tree);
    }
}

/// Closes the open vault (`only`: if it is that one): stops the watcher,
/// cancels the indexing, persists the index and frees it. The caller holds
/// `switch`. The save happens on this thread even if the index worker did
/// not stop in time: what is in memory is written, the rest is read again
/// next time.
fn close_current_vault(state: &AppState, only: Option<&str>) {
    let Some(root) = lock(&state.current_vault).clone() else { return };
    if only.is_some_and(|only| !same_path(&root, only)) {
        return;
    }
    // Dropping the watcher joins its thread: after that no batch arrives.
    let watcher = lock(&state.watcher).take();
    drop(watcher);
    *lock(&state.current_vault) = None;
    {
        // Held to the end: a `build_tags` that arrives meanwhile starts
        // after the index was saved and freed.
        let mut job = lock(&state.index_job);
        let other = job.as_ref().is_some_and(|(job_root, _)| !same_path(job_root, &root));
        if !other {
            if let Some((_, cancel)) = job.take() {
                cancel.cancel();
            }
            wait_index_idle(state);
            let _ = state.index.save(&state.derived_dir);
            state.index.clear();
        }
        // Otherwise the index was already asked for another vault (the page
        // switches vaults with `close_vault`, `build_tags` and `open_vault`
        // sent together): that job is not ours to cancel, and the index
        // worker saves and frees what the index still holds of this vault
        // before it loads the other one.
    }
    flush_tree_snapshot(state, true);
    *lock(&state.tree) = TreeState::default();
}

/// Installs the folder watcher on `root`. The watcher is only a trigger:
/// each batch asks the refresh worker to look at the vault again. Without a
/// watcher (creation refused), the focus safety net remains.
fn watch_current_vault(state: &AppState, root: &str) {
    let previous = lock(&state.watcher).take();
    drop(previous);
    *lock(&state.current_vault) = Some(root.to_string());
    let Some(tx) = lock(&state.refresh_tx).clone() else { return };
    let batch_root = root.to_string();
    let watcher = core::watch_vault(Path::new(root), core::WatchOptions::default(), move |batch| {
        // Never work on the watcher thread: hand the batch over and return.
        let _ = tx.send(RefreshRequest {
            root: batch_root.clone(),
            changes: ChangedPaths::from_batch(&batch),
            opening: false,
        });
    });
    *lock(&state.watcher) = watcher.ok();
}

fn post_refresh(state: &AppState, root: &str, opening: bool) {
    if let Some(tx) = lock(&state.refresh_tx).clone() {
        let _ = tx.send(RefreshRequest { root: root.to_string(), changes: ChangedPaths::unknown(), opening });
    }
}

/// Makes an `open_vault` of `root` that is still listing install nothing when
/// it ends (the vault is being closed or removed). The opening of another
/// vault is left alone: when the page switches vaults, the `close_vault` of
/// the old one may well arrive after the `open_vault` of the new one. The
/// caller holds `switch`.
fn supersede_open_of(state: &AppState, root: &str) {
    if lock(&state.opening).as_ref().is_some_and(|(_, path)| same_path(path, root)) {
        state.open_seq.fetch_add(1, Ordering::SeqCst);
    }
}

/// Clears the mark of an `open_vault` in flight when it ends, however it ends.
struct OpeningGuard<'a> {
    state: &'a AppState,
    seq: u64,
}

impl Drop for OpeningGuard<'_> {
    fn drop(&mut self) {
        let mut opening = lock(&self.state.opening);
        if opening.as_ref().is_some_and(|(seq, _)| *seq == self.seq) {
            *opening = None;
        }
    }
}

/// Hands a listing of the open vault to the index worker, if the index of
/// that vault was asked for (`build_tags`).
fn send_reconcile(state: &AppState, tree: &Arc<Tree>) {
    if let Some(cancel) = job_is(state, &tree.root) {
        if let Some(tx) = lock(&state.index_tx).clone() {
            let _ = tx.send(IndexRequest::Reconcile { tree: tree.clone(), cancel });
        }
    }
}

/// One line of `timings.jsonl` per listing of a vault (`by`: `open`, the
/// listing `open_vault` made for lack of a snapshot; `refresh`, a pass of
/// the refresh worker; `index`, a `build_tags` on a vault that is not
/// open). Lets a test count the listings.
fn log_listing(state: &AppState, by: &str, started: Instant) {
    append_timing(
        state,
        serde_json::json!({
            "name": "vault-listing",
            "by": by,
            "ms": started.elapsed().as_millis() as u64,
            "sinceProcessStartMs": now_ms().saturating_sub(state.process_start_ms),
        }),
    );
}

#[tauri::command(async)]
fn open_vault(state: State<'_, AppState>, path: String) -> ApiResult<IpcTree<Arc<Tree>>> {
    guarded("open_vault", || {
        lock(&state.policy).check_vault(&path)?;
        let seq = state.open_seq.fetch_add(1, Ordering::SeqCst) + 1;
        // Announced before the listing: a `build_tags` of this vault that
        // runs meanwhile waits for this listing instead of making its own.
        *lock(&state.opening) = Some((seq, path.clone()));
        let _opening = OpeningGuard { state: &state, seq };
        {
            let _switch = lock(&state.switch);
            let current = lock(&state.current_vault).clone();
            if current.is_some_and(|current| !same_path(&current, &path)) {
                close_current_vault(&state, None);
            }
        }
        // Snapshot if there is one (no access to the vault), else a listing
        // that can be slow: no lock is held meanwhile.
        let listing = Instant::now();
        let tree = Arc::new(core::open_tree(&state.derived_dir, Path::new(&path))?);
        if !tree.from_snapshot {
            log_listing(&state, "open", listing);
        }
        {
            let _switch = lock(&state.switch);
            // Another vault was opened (or this one closed) while we were
            // listing: the watcher and the current vault are not ours to set.
            if state.open_seq.load(Ordering::SeqCst) != seq {
                return Ok(IpcTree(tree));
            }
            watch_current_vault(&state, &tree.root);
            *lock(&state.tree) = TreeState {
                known: Some(KnownTree { tree: tree.clone(), dirty_since: None }),
                listing_pending: tree.from_snapshot.then(|| tree.root.clone()),
            };
            state.rebuild_requested.store(false, Ordering::Release);
            state.pending_retries.store(0, Ordering::Release);
            // `build_tags` came first and waits for a listing: this one, if
            // it is one (a snapshot is verified by the pass posted below,
            // which hands its listing to the index).
            if !tree.from_snapshot {
                send_reconcile(&state, &tree);
            }
        }
        // Background pass: verifies a snapshot against the disk (one listing,
        // shared with the index), removes orphaned temporary files.
        post_refresh(&state, &tree.root, true);
        Ok(IpcTree(tree))
    })
}

#[tauri::command(async)]
fn close_vault(state: State<'_, AppState>, path: String) -> ApiResult<()> {
    guarded("close_vault", || {
        let abs = absolute_string(Path::new(&path))?;
        let _switch = lock(&state.switch);
        supersede_open_of(&state, &abs);
        close_current_vault(&state, Some(&abs));
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

/// Payload of `tags-ready`: the contract fields plus where the content comes from.
fn tags_ready_payload(
    vault: &str,
    files: usize,
    tags: usize,
    occurrences: usize,
    ms: u64,
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut payload =
        serde_json::json!({ "vault": vault, "files": files, "tags": tags, "occurrences": occurrences, "ms": ms });
    if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
        dst.extend(src.clone());
    }
    payload
}

/// Makes the index of a vault available: first from what is persisted (no
/// note is read), then a reconciliation rereads only the changed notes. The
/// work is done by the index worker.
#[tauri::command(async)]
fn build_tags(state: State<'_, AppState>, vault_path: String) -> ApiResult<()> {
    guarded("build_tags", || {
        lock(&state.policy).check_vault(&vault_path)?;
        let root = absolute_string(Path::new(&vault_path))?;
        // Events carry the root exactly as `Tree.root` spells it.
        let root = lock(&state.current_vault).clone().filter(|current| same_path(current, &root)).unwrap_or(root);
        let cancel = CancelToken::new();
        // Set before the request is sent: the refresh worker looks at the
        // job after it stored a listing, the index worker looks for a
        // listing after the job is set, so one of them sees the other.
        // The request is sent with the job locked: a listing handed over
        // for this job (`send_reconcile`) is queued after it, never before.
        let mut job = lock(&state.index_job);
        if let Some((_, previous)) = job.replace((root.clone(), cancel.clone())) {
            previous.cancel();
        }
        state.pending_retries.store(0, Ordering::Release);
        if let Some(tx) = lock(&state.index_tx).clone() {
            let _ = tx.send(IndexRequest::Open { root, cancel });
        }
        Ok(())
    })
}

/// Whether `cancel` still belongs to the job of `root`.
fn job_is(state: &AppState, root: &str) -> Option<CancelToken> {
    lock(&state.index_job)
        .as_ref()
        .filter(|(job_root, _)| same_path(job_root, root))
        .map(|(_, cancel)| cancel.clone())
}

/// What remains to be done after [`index_open`].
enum AfterOpen {
    /// Reconciled already, or cancelled.
    Nothing,
    /// A listing of the open vault is at hand: reconcile with it.
    Reconcile(IndexRequest),
    /// The refresh worker is listing the open vault and will send the listing.
    AwaitListing,
}

fn index_open(app: &AppHandle, state: &AppState, root: &str, cancel: &CancelToken) -> AfterOpen {
    let index = &state.index;
    let derived = &state.derived_dir;
    let vault = Path::new(root);
    if index.root().is_some_and(|held| same_path(&held, root)) {
        // Same vault again: what is in memory is at least as recent as the
        // file; write it rather than load over it.
        let _ = index.save(derived);
        let stats = index.index_ready(true, 0, None);
        let _ = app.emit(
            "tags-ready",
            tags_ready_payload(root, stats.files, stats.tags, stats.occurrences, 0, serde_json::json!({ "fromCache": true })),
        );
        let _ = app.emit("index-ready", stats);
    } else {
        // Another vault's content must not be served (nor saved) under this one.
        let _ = index.save(derived);
        index.clear();
        if let Some(ready) = index.load_persisted(derived, vault) {
            let _ = app.emit(
                "tags-ready",
                tags_ready_payload(
                    root,
                    ready.files,
                    ready.tags,
                    ready.occurrences,
                    ready.ms,
                    serde_json::json!({ "fromCache": true }),
                ),
            );
            // Tags and links first; full-text search is opened next and announced separately.
            let _ = app.emit("index-ready", index.index_ready(true, ready.ms, None));
            let opened = now_ms();
            if index.open_search(derived, vault) {
                let _ = app.emit("index-ready", index.index_ready(true, ready.ms + now_ms().saturating_sub(opened), None));
            }
        }
    }
    if cancel.is_cancelled() {
        return AfterOpen::Nothing;
    }
    // Where does the listing come from? The open vault is listed for its
    // tree, by `open_vault` or by the refresh worker: that listing serves
    // (one listing for the tree and the index). The page sends `build_tags`
    // and `open_vault` together, so the vault may not be open yet.
    let deadline = Instant::now() + OPEN_GRACE;
    let current = loop {
        // Read in this order: `open_vault` sets the current vault before
        // its mark goes away.
        let opening = lock(&state.opening).as_ref().is_some_and(|(_, path)| same_path(path, root));
        if let Some(current) = lock(&state.current_vault).clone().filter(|current| same_path(current, root)) {
            break current;
        }
        if cancel.is_cancelled() {
            return AfterOpen::Nothing;
        }
        if opening {
            // `open_vault` is listing (or reading its snapshot): when it is
            // done it finds this job and sends its listing, or posts the
            // pass that will.
            return AfterOpen::AwaitListing;
        }
        if Instant::now() >= deadline {
            // Not the vault shown in the tree: nobody else lists it.
            index_reconcile(app, state, root, None, cancel, true);
            return AfterOpen::Nothing;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut tree = lock(&state.tree);
    match tree.known.as_ref() {
        Some(known) if !known.tree.from_snapshot && known.tree.root == current => {
            AfterOpen::Reconcile(IndexRequest::Reconcile { tree: known.tree.clone(), cancel: cancel.clone() })
        }
        _ => {
            if tree.listing_pending.as_deref() != Some(current.as_str()) {
                tree.listing_pending = Some(current.clone());
                drop(tree);
                post_refresh(state, &current, false);
            }
            AfterOpen::AwaitListing
        }
    }
}

/// One reconciliation. `tree`: the listing to use (`None`: list the vault
/// here). `announce`: emit progress and the final events even when nothing
/// changed (the pass that follows `build_tags`).
fn index_reconcile(
    app: &AppHandle,
    state: &AppState,
    root: &str,
    tree: Option<&Tree>,
    cancel: &CancelToken,
    announce: bool,
) {
    let index = &state.index;
    let derived = &state.derived_dir;
    let links_before = index.links_generation();
    let last_emit = AtomicU64::new(0);
    let on_progress = |p: core::TagsProgress| {
        if !announce {
            return;
        }
        // Throttled by time (readers report concurrently): the page only needs a coarse progress bar.
        let now = now_ms();
        let last = last_emit.load(Ordering::Relaxed);
        if p.done == p.total || now.saturating_sub(last) >= 100 {
            last_emit.store(now, Ordering::Relaxed);
            let _ = app.emit("tags-progress", serde_json::json!({ "vault": root, "done": p.done, "total": p.total }));
        }
    };
    let result = match tree {
        Some(tree) => index.reconcile_with_tree(derived, tree, BuildOptions::default(), cancel, on_progress),
        None => {
            let listing = Instant::now();
            let result = index.reconcile(derived, Path::new(root), BuildOptions::default(), cancel, on_progress);
            log_listing(state, "index", listing);
            result
        }
    };
    match result {
        Ok(Some(r)) => {
            state.rebuild_requested.store(false, Ordering::Release);
            // Notes that could not be read are not left for the next
            // external change: a few passes are scheduled, further and
            // further apart. The page is told meanwhile (`pending`).
            if r.pending == 0 {
                state.pending_retries.store(0, Ordering::Release);
            } else {
                // Kept in the timings log: which of the two stores fell behind, and why.
                append_timing(
                    state,
                    serde_json::json!({
                        "name": "index-pending",
                        "ms": r.ms,
                        "sinceProcessStartMs": now_ms().saturating_sub(state.process_start_ms),
                        "pending": r.pending,
                        "unread": r.unread,
                        "searchError": index.take_search_error(),
                        "engineLog": take_engine_log(),
                    }),
                );
                retry_pending(app, state, root, cancel);
            }
            if announce || r.reread > 0 || r.removed > 0 || r.pending > 0 || index.links_generation() != links_before {
                let extra = serde_json::json!({ "fromCache": false, "reread": r.reread, "removed": r.removed, "persisted": r.persisted });
                let _ = app.emit("tags-ready", tags_ready_payload(root, r.files, r.tags, r.occurrences, r.ms, extra));
                let _ = app.emit("index-ready", index.index_ready(false, r.ms, Some(&r)));
            }
        }
        // Cancelled (vault switch or exit): keep what was already reread for next time.
        Ok(None) => {
            let _ = index.save(derived);
        }
        Err(_) => {}
    }
}

/// Schedules a pass for the notes the last reconcile could not read, unless
/// the tries in a row are used up.
fn retry_pending(app: &AppHandle, state: &AppState, root: &str, cancel: &CancelToken) {
    let attempt = state.pending_retries.fetch_add(1, Ordering::AcqRel) as usize;
    let Some(delay) = PENDING_RETRY_DELAYS.get(attempt).copied() else { return };
    let (app, root, cancel) = (app.clone(), root.to_string(), cancel.clone());
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let state = app.state::<AppState>();
        // Still the same job on the same open vault?
        if cancel.is_cancelled() || state.closing.load(Ordering::Acquire) {
            return;
        }
        post_refresh(&state, &root, false);
    });
}

/// Worker that serves the index requests one at a time. Several listings
/// that piled up during a long pass cost one reconciliation (the last).
fn spawn_index_worker(app: AppHandle) {
    let (tx, rx) = channel::<IndexRequest>();
    *lock(&app.state::<AppState>().index_tx) = Some(tx);
    std::thread::spawn(move || {
        let mut queue: VecDeque<IndexRequest> = VecDeque::new();
        // The next reconciliation answers a `build_tags`: it announces itself.
        let mut announce = false;
        loop {
            let state = app.state::<AppState>();
            if queue.is_empty() {
                state.index_busy.store(false, Ordering::Release);
                match rx.recv() {
                    Ok(request) => queue.push_back(request),
                    Err(_) => return,
                }
            }
            state.index_busy.store(true, Ordering::Release);
            while let Ok(request) = rx.try_recv() {
                queue.push_back(request);
            }
            let Some(request) = queue.pop_front() else { continue };
            if state.closing.load(Ordering::Acquire) {
                continue;
            }
            match request {
                IndexRequest::Open { root, cancel } => {
                    if cancel.is_cancelled() {
                        continue;
                    }
                    let mut after = AfterOpen::Nothing;
                    survive(|| after = index_open(&app, &state, &root, &cancel));
                    announce = match after {
                        AfterOpen::Nothing => false,
                        AfterOpen::Reconcile(request) => {
                            queue.push_front(request);
                            true
                        }
                        AfterOpen::AwaitListing => true,
                    };
                }
                IndexRequest::Reconcile { tree, cancel } => {
                    // A newer listing is already waiting: this one is outdated.
                    let superseded = queue.iter().any(|r| matches!(r, IndexRequest::Reconcile { .. }));
                    if superseded || cancel.is_cancelled() {
                        continue;
                    }
                    let announced = std::mem::take(&mut announce);
                    survive(|| index_reconcile(&app, &state, &tree.root, Some(&tree), &cancel, announced));
                }
                IndexRequest::LinksChanged { root } => {
                    // Several saves in a row: one event, with the last state.
                    let again = queue.iter().any(|r| matches!(r, IndexRequest::LinksChanged { .. }));
                    let held = state.index.root().is_some_and(|held| same_path(&held, &root));
                    if !again && held {
                        survive(|| {
                            let _ = app.emit("index-ready", state.index.index_ready(false, 0, None));
                        });
                    }
                }
            }
        }
    });
}

#[tauri::command(async)]
fn tags_list(state: State<'_, AppState>, query: String) -> ApiResult<Vec<TagCount>> {
    guarded("tags_list", || Ok(state.index.tags_list(&query)))
}

#[tauri::command(async)]
fn tag_fragments(state: State<'_, AppState>, tag: String, limit: Option<usize>) -> ApiResult<Vec<Fragment>> {
    // Unbounded results can reach megabytes of JSON on a frequent tag.
    guarded("tag_fragments", || Ok(state.index.tag_fragments(&tag, Some(limit.unwrap_or(500).min(core::MAX_RESULTS)))))
}

#[tauri::command(async)]
fn search(state: State<'_, AppState>, query: String, limit: Option<usize>) -> ApiResult<core::SearchResult> {
    guarded("search", || {
        let result = state.index.search(&query, limit);
        // A search index found damaged is rebuilt by the next reconciliation: ask for it, once.
        if state.index.search_needs_rebuild() && !state.rebuild_requested.swap(true, Ordering::AcqRel) {
            match lock(&state.current_vault).clone() {
                Some(root) => post_refresh(&state, &root, false),
                None => state.rebuild_requested.store(false, Ordering::Release),
            }
        }
        Ok(result)
    })
}

#[tauri::command(async)]
fn backlinks(state: State<'_, AppState>, path: String) -> ApiResult<Vec<core::LinkRef>> {
    guarded("backlinks", || Ok(state.index.backlinks(Path::new(&path))))
}

#[tauri::command(async)]
fn outgoing_links(state: State<'_, AppState>, path: String) -> ApiResult<Vec<core::OutLink>> {
    guarded("outgoing_links", || Ok(state.index.outgoing_links(Path::new(&path))))
}

#[tauri::command(async)]
fn unresolved_links(state: State<'_, AppState>, limit: Option<usize>) -> ApiResult<Vec<core::UnresolvedLink>> {
    guarded("unresolved_links", || Ok(state.index.unresolved_links(limit)))
}

#[tauri::command(async)]
fn file_tags(path: String, text: String) -> ApiResult<Vec<Fragment>> {
    guarded("file_tags", || Ok(core::file_tags(&path, &text)))
}

// ---------------------------------------------------------------------------
// Commands: session, dialogs, misc
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn set_session(state: State<'_, AppState>, session: Session) -> ApiResult<()> {
    guarded("set_session", || {
        // What the page may not read is dropped silently: neither persisted nor watched.
        let session = lock(&state.policy).sanitize_session(&session);
        // Only the open tabs are watched for external changes.
        let missing: Vec<String> = {
            let mut watched = lock(&state.watched);
            watched.retain(|p, _| session.tabs.iter().any(|t| t == p));
            session.tabs.iter().filter(|t| !watched.contains_key(*t)).cloned().collect()
        };
        for tab in missing {
            if let Some(mtime_ms) = mtime_of(&tab) {
                lock(&state.watched).entry(tab).or_insert(Watched { mtime_ms, misses: 0 });
            }
        }
        core::save_session(&state.data_dir, &session)
    })
}

#[tauri::command(async)]
fn pick_folder(app: AppHandle) -> ApiResult<Option<String>> {
    guarded("pick_folder", || {
        let picked =
            app.dialog().file().blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned());
        // Chosen by the user in a system dialog: `add_vault` will accept it.
        if let Some(path) = &picked {
            let _ = lock(&app.state::<AppState>().policy).allow_picked_folder(path);
        }
        Ok(picked)
    })
}

#[tauri::command(async)]
fn pick_file(app: AppHandle) -> ApiResult<Option<String>> {
    guarded("pick_file", || {
        let picked = app
            .dialog()
            .file()
            .add_filter("Markdown", &["md", "markdown", "txt"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .and_then(|p| absolute_string(&p).ok());
        // Chosen by the user in a system dialog: opened explicitly.
        if let Some(path) = &picked {
            grant_opened(&app, std::slice::from_ref(path));
        }
        Ok(picked)
    })
}

fn append_timing(state: &AppState, line: serde_json::Value) {
    let _guard = lock(&state.timing_log);
    let file = state.data_dir.join("timings.jsonl");
    if std::fs::metadata(&file).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&file);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// Startup milestones measured by the UI, appended to `<data_dir>/timings.jsonl`.
#[tauri::command(async)]
fn log_timing(state: State<'_, AppState>, name: String, ms: f64) -> ApiResult<()> {
    guarded("log_timing", || {
        let name: String = name.chars().take(200).collect();
        append_timing(
            &state,
            serde_json::json!({
                "name": name,
                "ms": ms,
                "sinceProcessStartMs": now_ms().saturating_sub(state.process_start_ms),
                "processStartMs": state.process_start_ms,
            }),
        );
        Ok(())
    })
}

/// Opens a link in the system browser or mail client. Only web and mail links:
/// a note must not be able to launch an arbitrary program or file.
#[tauri::command(async)]
fn open_external(url: String) -> ApiResult<()> {
    guarded("open_external", || {
        let url = core::check_external_url(&url)?;
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| ApiError::io(format!("cannot open link: {e}")))
    })
}

fn emit_open_files(app: &AppHandle, paths: Vec<String>) {
    if !paths.is_empty() {
        // Allowed before the page hears about them.
        grant_opened(app, &paths);
        let _ = app.emit("open-files", OpenFiles { paths });
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// ---------------------------------------------------------------------------
// Background: refresh worker, focus, housekeeping, open files
// ---------------------------------------------------------------------------

/// One pass of the refresh worker (see `rocksy_core::refresh`). Returns
/// whether the vault was listed.
fn refresh_once(app: &AppHandle, state: &AppState, request: &RefreshRequest) -> bool {
    let root = request.root.as_str();
    let is_current = || lock(&state.current_vault).as_deref() == Some(root);
    let clear_pending = || {
        let mut tree = lock(&state.tree);
        if tree.listing_pending.as_deref() == Some(root) {
            tree.listing_pending = None;
        }
    };
    // The vault may have been switched or closed since the request was posted.
    if state.closing.load(Ordering::Acquire) || !is_current() {
        return false;
    }
    let known = lock(&state.tree).known.as_ref().filter(|k| k.tree.root == root).map(|k| k.tree.clone());
    // `open_vault` listed the vault itself (no snapshot yet): that listing serves.
    let fresh = known.as_ref().filter(|tree| request.opening && !tree.from_snapshot).cloned();
    let (tree, listed) = match fresh {
        Some(tree) => (tree, false),
        None => {
            let cancelled = || state.closing.load(Ordering::Acquire) || !is_current();
            let listing = Instant::now();
            let outcome = core::refresh_pass(
                &state.derived_dir,
                Path::new(root),
                known.as_deref(),
                &request.changes,
                &state.own_writes,
                &cancelled,
            );
            match outcome {
                // Only saves made by the app: nothing to list, nothing to tell.
                Ok(RefreshOutcome::OwnWrites) => return false,
                Ok(RefreshOutcome::Cancelled) | Err(_) => {
                    clear_pending();
                    return false;
                }
                Ok(RefreshOutcome::Listed(refresh)) => {
                    log_listing(state, "refresh", listing);
                    let tree = Arc::new(refresh.tree);
                    {
                        let mut state_tree = lock(&state.tree);
                        if is_current() {
                            // Entries updated in memory are still missing from
                            // the snapshot when it was not rewritten.
                            let dirty_since = match (&state_tree.known, refresh.changed) {
                                (Some(previous), false) => previous.dirty_since,
                                _ => None,
                            };
                            state_tree.known = Some(KnownTree { tree: tree.clone(), dirty_since });
                        }
                        if state_tree.listing_pending.as_deref() == Some(root) {
                            state_tree.listing_pending = None;
                        }
                    }
                    // Tree first: the page sees the change before the index is brought in line.
                    if refresh.changed && is_current() {
                        let _ = app.emit("tree-updated", IpcTree(tree.clone()));
                    }
                    (tree, true)
                }
            }
        }
    };
    if !listed {
        clear_pending();
    }
    // Index next, with the same listing, on its own worker (a first indexing
    // can take minutes; the tree keeps being refreshed meanwhile).
    if listed {
        send_reconcile(state, &tree);
    }
    // Orphaned temporary files of our own atomic writes (see CONTRACT.md,
    // the one exception to "the only write in a vault is save_file").
    if request.opening {
        let cancelled = || state.closing.load(Ordering::Acquire) || !is_current();
        core::clean_vault_temp_files(&tree, SystemTime::now(), &cancelled);
    }
    listed
}

/// Worker that serves refresh requests one at a time, merging those that piled
/// up meanwhile (a burst of external changes costs a few listings, not hundreds).
fn spawn_refresh_worker(app: AppHandle) {
    let (tx, rx) = channel::<RefreshRequest>();
    *lock(&app.state::<AppState>().refresh_tx) = Some(tx);
    std::thread::spawn(move || {
        while let Ok(mut request) = rx.recv() {
            while let Ok(next) = rx.try_recv() {
                if next.root == request.root {
                    request.changes.merge(next.changes);
                    request.opening |= next.opening;
                } else {
                    // Another vault: whatever was reported for the first one is void.
                    request = RefreshRequest { root: next.root, changes: ChangedPaths::unknown(), opening: next.opening };
                }
            }
            let state = app.state::<AppState>();
            let mut listed = false;
            survive(|| listed = refresh_once(&app, &state, &request));
            if listed {
                state.last_refresh_ms.store(now_ms(), Ordering::Relaxed);
                // Rate limit: under a long burst, at most one listing per second.
                std::thread::sleep(Duration::from_millis(1000));
            }
        }
    });
}

/// Safety net for missed notifications (a synced drive may not report every change):
/// when the window comes back to the front, check the vault if the last pass is old.
fn refresh_on_focus(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.closing.load(Ordering::Acquire) {
        return;
    }
    let last = state.last_refresh_ms.load(Ordering::Relaxed);
    if now_ms().saturating_sub(last) < 30_000 {
        return;
    }
    let root = lock(&state.current_vault).clone();
    if let Some(root) = root {
        state.last_refresh_ms.store(now_ms(), Ordering::Relaxed);
        post_refresh(&state, &root, false);
    }
}

/// Every 500 ms: commit of the search index when a save has waited long
/// enough (search is up to date within 2 s), tags and links persisted at
/// most once a minute, tree snapshot likewise.
fn spawn_housekeeping(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let state = app.state::<AppState>();
        if state.closing.load(Ordering::Acquire) {
            return;
        }
        survive(|| {
            state.index.housekeeping(&state.derived_dir, Instant::now());
            flush_tree_snapshot(&state, false);
        });
    });
}

/// Polls the mtime of the open files (a handful of `stat` calls every 2 s).
/// A path the page may not read is never touched.
fn poll_open_files(app: &AppHandle, state: &AppState) {
    let snapshot: Vec<(String, Watched)> = lock(&state.watched).iter().map(|(p, w)| (p.clone(), *w)).collect();
    for (path, known) in snapshot {
        if lock(&state.policy).check_read(&path, PathOrigin::Page).is_err() {
            lock(&state.watched).remove(&path);
            continue;
        }
        let current = mtime_of(&path);
        let mut watched = lock(&state.watched);
        // A save or a close may have changed the entry while we were looking.
        if watched.get(&path) != Some(&known) {
            continue;
        }
        let event = match current {
            Some(mtime_ms) => {
                watched.insert(path.clone(), Watched { mtime_ms, misses: 0 });
                // Changed, or back after having been reported as deleted.
                (mtime_ms != known.mtime_ms || known.misses >= 2).then_some(FileChanged { path, mtime_ms, deleted: false })
            }
            None => {
                let misses = known.misses.saturating_add(1).min(3);
                watched.insert(path.clone(), Watched { misses, ..known });
                // Two polls in a row: not a passing hiccup of a synced drive. Told once.
                (misses == 2).then_some(FileChanged { path, mtime_ms: 0, deleted: true })
            }
        };
        drop(watched);
        if let Some(event) = event {
            let _ = app.emit("file-changed", event);
        }
    }
}

fn spawn_file_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(2000));
        let state = app.state::<AppState>();
        if state.closing.load(Ordering::Acquire) {
            return;
        }
        survive(|| poll_open_files(&app, &state));
    });
}

// ---------------------------------------------------------------------------
// Closing
// ---------------------------------------------------------------------------

/// The user asked to close the window: the page is told and has
/// [`CLOSE_ACK_TIMEOUT`] to say it is alive (`close_ack`); it then saves and
/// calls `quit`, or does nothing if the user cancels. Without an answer (page
/// frozen or not loaded) the app exits. Repeated requests while waiting for
/// the answer change nothing.
fn request_close(app: &AppHandle) {
    let state = app.state::<AppState>();
    let seq = {
        let mut waiting = lock(&state.close_wait);
        if waiting.is_some() {
            return;
        }
        let seq = state.close_seq.fetch_add(1, Ordering::SeqCst) + 1;
        *waiting = Some(seq);
        seq
    };
    let _ = app.emit("close-requested", ());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(CLOSE_ACK_TIMEOUT);
        let unanswered = *lock(&app.state::<AppState>().close_wait) == Some(seq);
        if unanswered {
            shutdown(&app);
        }
    });
}

#[tauri::command(async)]
fn close_ack(state: State<'_, AppState>) -> ApiResult<()> {
    guarded("close_ack", || {
        *lock(&state.close_wait) = None;
        Ok(())
    })
}

#[tauri::command(async)]
fn quit(app: AppHandle) -> ApiResult<()> {
    guarded("quit", || {
        shutdown(&app);
        Ok(())
    })
}

/// Stops the background work and persists the index, once. Runs on the
/// calling thread and is bounded: the watcher is dropped, the indexing is
/// cancelled and given [`INDEX_STOP_TIMEOUT`] to stop, then what is in
/// memory is saved whether it stopped or not.
fn finalize(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.closing.swap(true, Ordering::AcqRel) {
        return;
    }
    survive(|| {
        let watcher = lock(&state.watcher).take();
        drop(watcher);
        if let Some((_, cancel)) = lock(&state.index_job).as_ref() {
            cancel.cancel();
        }
        wait_index_idle(&state);
        flush_tree_snapshot(&state, true);
        let _ = state.index.save(&state.derived_dir);
    });
}

fn shutdown(app: &AppHandle) {
    finalize(app);
    // Goes through `RunEvent::Exit`; never `std::process::exit` from here.
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// Roaming and local data directories. `ROCKSY_DATA_DIR` (tests,
/// measurements) puts both in one place.
fn data_dirs(app: &tauri::App) -> Result<(PathBuf, PathBuf, bool), Box<dyn std::error::Error>> {
    if let Some(dir) = std::env::var_os("ROCKSY_DATA_DIR").map(PathBuf::from) {
        return Ok((dir.clone(), dir, true));
    }
    let roaming = app.path().app_data_dir()?;
    let local = app.path().app_local_data_dir().unwrap_or_else(|_| roaming.clone());
    Ok((roaming, local, false))
}

fn setup(app: &mut tauri::App, arg_files: Vec<String>, process_start_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
    let (data_dir, local_dir, isolated) = data_dirs(app)?;
    std::fs::create_dir_all(&data_dir)?;
    // Before anything reads `trees/`, `tags/` or `search/`: move what an
    // earlier version left in the roaming directory (renames only). If that
    // fails, this run keeps using the roaming directory.
    let migration = core::migrate_derived_data(&data_dir, &local_dir);
    let derived_dir = migration.dir.clone();
    let _ = std::fs::create_dir_all(&derived_dir);

    // Who may reach what: registered vaults, files opened explicitly (the
    // backend's own list, restricted to the tabs of the last session), and
    // the files named on the command line.
    let vaults = core::list_vaults(&data_dir);
    let session = core::load_session(&data_dir);
    let persisted = core::load_opened(&data_dir);
    let mut policy = AccessPolicy::new(&vaults);
    // An isolated instance (tests, measurements) registers vaults without
    // the folder dialog. Whoever launches the process decides, not the page.
    policy.set_free_vault_registration(isolated);
    policy.restore_opened(&persisted, session.as_ref());
    for file in &arg_files {
        let _ = policy.allow_opened(file);
    }
    let opened = policy.opened_files();
    let opened_changed = opened != persisted;
    // The note the page will show first, read while the WebView starts.
    let first_path = arg_files.first().cloned().or_else(|| session.as_ref().and_then(|s| s.active_tab.clone()));

    app.manage(AppState {
        data_dir: data_dir.clone(),
        derived_dir,
        arg_files,
        process_start_ms,
        policy: Mutex::new(policy),
        asset_roots: Mutex::new(HashSet::new()),
        scope_ready: (Mutex::new(false), Condvar::new()),
        first_file: Mutex::new(if first_path.is_some() { FirstFile::Pending } else { FirstFile::Ready(None) }),
        index: Arc::new(TagIndex::new()),
        index_job: Mutex::new(None),
        index_tx: Mutex::new(None),
        index_busy: AtomicBool::new(false),
        rebuild_requested: AtomicBool::new(false),
        pending_retries: AtomicU32::new(0),
        opening: Mutex::new(None),
        switch: Mutex::new(()),
        open_seq: AtomicU64::new(0),
        current_vault: Mutex::new(None),
        watcher: Mutex::new(None),
        tree: Mutex::new(TreeState::default()),
        own_writes: OwnWrites::new(),
        watched: Mutex::new(HashMap::new()),
        refresh_tx: Mutex::new(None),
        last_refresh_ms: AtomicU64::new(0),
        timing_log: Mutex::new(()),
        close_wait: Mutex::new(None),
        close_seq: AtomicU64::new(0),
        closing: AtomicBool::new(false),
    });

    if let Some(path) = first_path {
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            let state = handle.state::<AppState>();
            // Same checks and same effects as `read_file`. A failure is not
            // reported here: the page falls back on `read_file` and gets the error.
            let mut file = None;
            survive(|| file = read_for_page(&state, &path).ok());
            let mut slot = lock(&state.first_file);
            if matches!(*slot, FirstFile::Pending) {
                *slot = FirstFile::Ready(file);
            }
        });
    }
    {
        let handle = app.handle().clone();
        let note = migration.error.clone();
        std::thread::spawn(move || {
            let state = handle.state::<AppState>();
            survive(|| sync_asset_scope(&handle));
            {
                let (ready, signal) = &state.scope_ready;
                *lock(ready) = true;
                signal.notify_all();
            }
            survive(|| {
                if opened_changed {
                    let _ = core::save_opened(&state.data_dir, &opened);
                }
                if let Some(message) = note {
                    append_timing(&state, serde_json::json!({ "name": "derived-data-migration-failed", "ms": 0, "message": message }));
                }
                // Leftovers of interrupted writes of our own data files.
                let now = SystemTime::now();
                for dir in [state.data_dir.clone(), state.derived_dir.join("trees"), state.derived_dir.join("tags")] {
                    core::remove_stale_temp_files(&dir, now);
                }
            });
        });
    }

    // The window is created here rather than in tauri.conf.json so that an isolated
    // profile also isolates the WebView's own storage (UI preferences).
    let mut window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Rocksy")
        .inner_size(1280.0, 820.0)
        .min_inner_size(480.0, 320.0)
        .background_color(tauri::window::Color(0x14, 0x18, 0x1f, 0xff));
    if isolated {
        window = window.data_directory(data_dir.join("webview"));
    }
    window.build()?;

    spawn_file_watcher(app.handle().clone());
    spawn_housekeeping(app.handle().clone());
    spawn_refresh_worker(app.handle().clone());
    spawn_index_worker(app.handle().clone());
    Ok(())
}

fn main() {
    let process_start_ms = now_ms();
    if log::set_logger(&ENGINE_LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    // Panics are caught and survived (see `guarded`, `survive` and the core);
    // where they happened is kept with the same lines.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let line: String = format!("PANIC: {info}").chars().take(400).collect();
        let mut lines = lock(&ENGINE_LOG);
        if lines.len() >= 8 {
            lines.pop_front();
        }
        lines.push_back(line);
        drop(lines);
        default_hook(info);
    }));
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let arg_files = files_from_args(&args, &cwd);

    let mut builder = tauri::Builder::default();
    // An isolated profile (ROCKSY_DATA_DIR, used by tests and measurements) runs as its own
    // instance, so it never lands in the window of the user's running app.
    if std::env::var_os("ROCKSY_DATA_DIR").is_none() {
        // Must be the first plugin: a second launch forwards its arguments here and exits.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            emit_open_files(app, files_from_args(&argv, Path::new(&cwd)));
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| setup(app, arg_files, process_start_ms))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(true) => refresh_on_focus(window.app_handle()),
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                // Once the shutdown has started the window is let go.
                if !app.state::<AppState>().closing.load(Ordering::Acquire) {
                    api.prevent_close();
                    request_close(app);
                }
            }
            // Dropped on the window by the user: the paths come from the
            // operating system, not from the page.
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                let files: Vec<String> =
                    paths.iter().filter(|p| p.is_file()).filter_map(|p| absolute_string(p).ok()).collect();
                emit_open_files(window.app_handle(), files);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            startup_info,
            read_file,
            save_file,
            list_vaults,
            add_vault,
            remove_vault,
            open_vault,
            close_vault,
            build_tags,
            tags_list,
            tag_fragments,
            file_tags,
            search,
            backlinks,
            outgoing_links,
            unresolved_links,
            set_session,
            pick_folder,
            pick_file,
            log_timing,
            open_external,
            close_ack,
            quit
        ])
        .build(tauri::generate_context!())
        .expect("error while running Rocksy")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                finalize(app);
            }
        });
}
