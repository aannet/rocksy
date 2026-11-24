//! Folder watcher: turns file-system notifications into debounced batches.
//!
//! The watcher is a trigger, never the source of truth: a batch only says
//! "something visible changed under these paths, check again". The caller
//! answers with `reconcile_tree` and `TagIndex::reconcile`, and should also
//! run them periodically, because notifications are not guaranteed on every
//! file system (synced or virtual drives in particular).
//!
//! Nothing is ever written, anywhere.

use crate::error::{ApiError, Result};
use crate::paths::{absolute_string, strip_verbatim};
use notify::event::{EventKind, ModifyKind};
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

/// Most paths reported in one batch; beyond that the batch is `truncated`.
pub const MAX_BATCH_PATHS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOptions {
    /// Quiet time required before a batch is emitted.
    pub debounce_ms: u64,
    /// Longest time a batch is held back while events keep arriving.
    pub max_wait_ms: u64,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self { debounce_ms: 400, max_wait_ms: 3000 }
    }
}

/// A group of changes observed under a vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeBatch {
    pub root: String,
    /// Absolute paths of the visible files and folders that were touched,
    /// without duplicates, in order of first appearance, at most
    /// [`MAX_BATCH_PATHS`]. A rename contributes both its old and new path.
    pub paths: Vec<String>,
    /// The list is incomplete (more paths than the cap, events lost by the
    /// operating system, watcher error, or the root itself changed): check
    /// everything.
    pub truncated: bool,
    /// Number of `.md` files among `paths`.
    pub markdown: usize,
    /// Raw events that contributed to the batch (after filtering).
    pub events: usize,
}

enum Message {
    Event(notify::Result<notify::Event>),
    Stop,
}

/// Collects filtered events until the batch is emitted.
struct Accumulator {
    root: String,
    root_path: PathBuf,
    paths: Vec<String>,
    seen: HashSet<String>,
    /// Paths already found to be folders in this batch (see `folder_filter`),
    /// so that a folder reported again is not looked up again.
    folders: HashSet<String>,
    /// Drop "modified" events about folders. This costs one `stat` per new
    /// path, so it is bounded: see `add`.
    folder_filter: bool,
    is_dir: fn(&Path) -> bool,
    truncated: bool,
    events: usize,
}

fn stat_is_dir(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

impl Accumulator {
    fn new(root: &str) -> Self {
        // On Windows every change inside a folder also reports the folder
        // itself as modified; that says nothing the child's own event does
        // not, so it is dropped.
        Self::with_folder_filter(root, cfg!(windows), stat_is_dir)
    }

    fn with_folder_filter(root: &str, folder_filter: bool, is_dir: fn(&Path) -> bool) -> Self {
        Self {
            root: root.to_string(),
            root_path: PathBuf::from(root),
            paths: Vec::new(),
            seen: HashSet::new(),
            folders: HashSet::new(),
            folder_filter,
            is_dir,
            truncated: false,
            events: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.events == 0
    }

    /// Records one notification. Returns whether it counted.
    fn add(&mut self, message: notify::Result<notify::Event>) -> bool {
        let event = match message {
            Ok(event) => event,
            Err(_) => {
                // Whatever went wrong, the caller must not trust the list.
                self.truncated = true;
                self.events += 1;
                return true;
            }
        };
        let mut counted = false;
        if event.need_rescan() {
            self.truncated = true;
            counted = true;
        }
        if !matches!(event.kind, EventKind::Access(_)) {
            let skip_folders = self.folder_filter
                && matches!(event.kind, EventKind::Modify(kind) if !matches!(kind, ModifyKind::Name(_)));
            for path in &event.paths {
                match self.classify(path) {
                    PathClass::Hidden | PathClass::Outside => {}
                    PathClass::Root => {
                        // The root itself was removed, renamed or re-attributed.
                        self.truncated = true;
                        counted = true;
                    }
                    PathClass::Visible(text) => {
                        if self.seen.contains(&text) {
                            counted = true;
                        } else if self.paths.len() >= MAX_BATCH_PATHS {
                            // The list is full and the caller will check
                            // everything: no file-system access any more. A
                            // burst must not cost one `stat` per event (slow
                            // on a synced drive) while the queue grows.
                            self.truncated = true;
                            counted = true;
                        } else if skip_folders && self.folders.contains(&text) {
                            // a folder already identified: not counted
                        } else if skip_folders && self.folders.len() >= MAX_BATCH_PATHS {
                            // Too many folders to remember: stop looking.
                            self.truncated = true;
                            counted = true;
                        } else if skip_folders && (self.is_dir)(path) {
                            self.folders.insert(text); // not counted
                        } else {
                            counted = true;
                            self.seen.insert(text.clone());
                            self.paths.push(text);
                        }
                    }
                }
            }
        }
        if counted {
            self.events += 1;
        }
        counted
    }

    fn classify(&self, path: &Path) -> PathClass {
        let Ok(rel) = path.strip_prefix(&self.root_path) else { return PathClass::Outside };
        let mut components = rel.components().peekable();
        if components.peek().is_none() {
            return PathClass::Root;
        }
        if components.any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
            return PathClass::Hidden;
        }
        match path.to_str() {
            Some(text) => PathClass::Visible(strip_verbatim(text)),
            None => PathClass::Hidden, // not valid Unicode: not in the tree either
        }
    }

    fn take(&mut self) -> ChangeBatch {
        let paths = std::mem::take(&mut self.paths);
        self.seen.clear();
        self.folders.clear();
        let markdown = paths
            .iter()
            .filter(|p| p.len() > 3 && p.as_bytes()[p.len() - 3..].eq_ignore_ascii_case(b".md"))
            .count();
        let batch = ChangeBatch {
            root: self.root.clone(),
            paths,
            truncated: self.truncated,
            markdown,
            events: self.events,
        };
        self.truncated = false;
        self.events = 0;
        batch
    }
}

enum PathClass {
    Visible(String),
    Hidden,
    Root,
    Outside,
}

fn run(
    receiver: mpsc::Receiver<Message>,
    root: String,
    options: WatchOptions,
    on_batch: impl Fn(ChangeBatch),
) {
    let debounce = Duration::from_millis(options.debounce_ms);
    let max_wait = Duration::from_millis(options.max_wait_ms.max(options.debounce_ms));
    let mut acc = Accumulator::new(&root);
    // Start of the current batch and time of its latest event.
    let mut window: Option<(Instant, Instant)> = None;
    loop {
        let message = match window {
            None => match receiver.recv() {
                Ok(m) => m,
                Err(_) => return,
            },
            Some((first, last)) => {
                let deadline = (last + debounce).min(first + max_wait);
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(m) => m,
                    Err(RecvTimeoutError::Timeout) => {
                        window = None;
                        if !acc.is_empty() {
                            on_batch(acc.take());
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        };
        match message {
            Message::Stop => return,
            Message::Event(event) => {
                if acc.add(event) {
                    let now = Instant::now();
                    let first = window.map_or(now, |(first, _)| first);
                    window = Some((first, now));
                    if now.duration_since(first) >= max_wait {
                        window = None;
                        on_batch(acc.take());
                    }
                }
            }
        }
    }
}

/// A running watcher. Dropping it stops the notifications and joins the
/// thread: once `drop` returns, `on_batch` is not running and will never be
/// called again. Events not yet emitted are discarded.
pub struct VaultWatcher {
    root: String,
    watcher: Option<notify::RecommendedWatcher>,
    sender: mpsc::Sender<Message>,
    thread: Option<JoinHandle<()>>,
    thread_id: ThreadId,
}

impl VaultWatcher {
    /// Root being watched (absolute).
    pub fn root(&self) -> &str {
        &self.root
    }
}

impl Drop for VaultWatcher {
    fn drop(&mut self) {
        // Stop the source first, then the batching thread.
        drop(self.watcher.take());
        let _ = self.sender.send(Message::Stop);
        if let Some(handle) = self.thread.take() {
            // Dropped from inside `on_batch`: joining ourselves would deadlock.
            if thread::current().id() != self.thread_id {
                let _ = handle.join();
            }
        }
    }
}

/// Watches `root` recursively and calls `on_batch` with debounced groups of
/// changes.
///
/// * Paths with a component starting with `.` (relative to the root) are
///   ignored, like in the tree: `.obsidian/`, `.trash/`, dot-folders and the
///   temporary files of `save_file` never produce a batch.
/// * Only creations, content or metadata changes, removals and renames
///   count; plain reads do not.
/// * A batch is emitted after `debounce_ms` without a new event, or at the
///   latest `max_wait_ms` after its first event while events keep coming.
/// * `on_batch` runs on the watcher's own thread, never concurrently with
///   itself; while it runs, new events queue up for the next batch.
///
/// Errors: `not_found` if the root does not exist, `invalid` if it is not a
/// folder, `io` if the operating system refuses the watch (for instance the
/// inotify watch limit on Linux). After a successful start nothing fails
/// loudly: if the root disappears the watcher simply goes quiet (possibly
/// after a last `truncated` batch).
pub fn watch_vault(
    root: &Path,
    options: WatchOptions,
    on_batch: impl Fn(ChangeBatch) + Send + 'static,
) -> Result<VaultWatcher> {
    let root_string = absolute_string(root)?;
    let meta = fs::metadata(&root_string)
        .map_err(|e| ApiError::from_io(&e, format!("cannot watch {root_string}")))?;
    if !meta.is_dir() {
        return Err(ApiError::invalid(format!("{root_string} is not a folder")));
    }

    let (sender, receiver) = mpsc::channel::<Message>();
    let event_sender = sender.clone();
    let to_error = |e: notify::Error| match e.kind {
        notify::ErrorKind::PathNotFound => ApiError::not_found(format!("cannot watch {root_string}: not found")),
        _ => ApiError::io(format!("cannot watch {root_string}: {e}")),
    };
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = event_sender.send(Message::Event(event));
    })
    .map_err(to_error)?;
    watcher.watch(Path::new(&root_string), RecursiveMode::Recursive).map_err(to_error)?;

    let thread_root = root_string.clone();
    let handle = thread::Builder::new()
        .name("rocksy-watch".into())
        .spawn(move || run(receiver, thread_root, options, on_batch))
        .map_err(|e| ApiError::io(format!("cannot start the watcher thread: {e}")))?;
    let thread_id = handle.thread().id();
    Ok(VaultWatcher { root: root_string, watcher: Some(watcher), sender, thread: Some(handle), thread_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, Flag, RemoveKind, RenameMode};
    use notify::Event;

    #[cfg(windows)]
    const ROOT: &str = r"C:\vault";
    #[cfg(not(windows))]
    const ROOT: &str = "/vault";

    fn p(rel: &str) -> PathBuf {
        let mut path = PathBuf::from(ROOT);
        rel.split('/').filter(|c| !c.is_empty()).for_each(|c| path.push(c));
        path
    }

    fn s(rel: &str) -> String {
        p(rel).to_str().unwrap().to_string()
    }

    fn event(kind: EventKind, rels: &[&str]) -> notify::Result<Event> {
        let mut e = Event::new(kind);
        for rel in rels {
            e = e.add_path(p(rel));
        }
        Ok(e)
    }

    const CREATE: EventKind = EventKind::Create(CreateKind::File);
    const DATA: EventKind = EventKind::Modify(ModifyKind::Data(DataChange::Any));
    const REMOVE: EventKind = EventKind::Remove(RemoveKind::File);
    const RENAME: EventKind = EventKind::Modify(ModifyKind::Name(RenameMode::Both));

    #[test]
    fn visible_paths_are_collected_once_in_order() {
        let mut acc = Accumulator::new(ROOT);
        assert!(acc.is_empty());
        assert!(acc.add(event(CREATE, &["b/note.md"])));
        assert!(acc.add(event(DATA, &["b/note.md"])));
        assert!(acc.add(event(DATA, &["a.MD"])));
        assert!(acc.add(event(REMOVE, &["img.png"])));
        assert!(acc.add(event(RENAME, &["old.md", "sub/new.md"])));
        let batch = acc.take();
        assert_eq!(batch.root, ROOT);
        assert_eq!(batch.paths, [s("b/note.md"), s("a.MD"), s("img.png"), s("old.md"), s("sub/new.md")]);
        assert_eq!((batch.markdown, batch.events, batch.truncated), (4, 5, false));
        // The accumulator starts over.
        assert!(acc.is_empty());
        assert!(acc.add(event(DATA, &["b/note.md"])));
        assert_eq!(acc.take().paths, [s("b/note.md")]);
    }

    #[test]
    fn hidden_outside_and_access_events_are_ignored() {
        let mut acc = Accumulator::new(ROOT);
        for rel in [".obsidian/workspace.json", ".trash/x.md", ".hidden.md", "sub/.cache/x.md", ".opencode/node_modules/p/i.js", "a/.x.md.12-3.rocksy-tmp"] {
            assert!(!acc.add(event(CREATE, &[rel])), "{rel}");
            assert!(!acc.add(event(DATA, &[rel])), "{rel}");
            assert!(!acc.add(event(REMOVE, &[rel])), "{rel}");
        }
        assert!(!acc.add(event(EventKind::Access(AccessKind::Open(AccessMode::Read)), &["a.md"])));
        assert!(!acc.add(event(EventKind::Access(AccessKind::Close(AccessMode::Write)), &["a.md"])));
        let mut outside = Event::new(DATA);
        outside = outside.add_path(PathBuf::from(format!("{ROOT}-other")).join("a.md"));
        assert!(!acc.add(Ok(outside)));
        assert!(acc.is_empty());

        // Atomic save: temporary renamed over the note. Only the note counts.
        assert!(acc.add(event(RENAME, &["a/.x.md.12-3.rocksy-tmp", "a/x.md"])));
        // Moved to the trash: the visible side is still reported.
        assert!(acc.add(event(RENAME, &["b.md", ".trash/b.md"])));
        let batch = acc.take();
        assert_eq!(batch.paths, [s("a/x.md"), s("b.md")]);
        assert_eq!(batch.events, 2);
    }

    #[test]
    fn errors_rescans_and_root_events_truncate() {
        let mut acc = Accumulator::new(ROOT);
        assert!(acc.add(Err(notify::Error::generic("lost"))));
        let batch = acc.take();
        assert_eq!((batch.truncated, batch.events, batch.paths.len()), (true, 1, 0));

        assert!(acc.add(Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan))));
        assert!(acc.take().truncated);

        assert!(acc.add(event(REMOVE, &[""])), "the root itself");
        let batch = acc.take();
        assert!(batch.truncated && batch.paths.is_empty());

        // The flag does not leak into the next batch.
        assert!(acc.add(event(DATA, &["a.md"])));
        assert!(!acc.take().truncated);
    }

    #[test]
    fn path_count_is_capped() {
        let mut acc = Accumulator::new(ROOT);
        for i in 0..MAX_BATCH_PATHS + 50 {
            assert!(acc.add(event(DATA, &[&format!("d/n{i}.md")])));
        }
        let batch = acc.take();
        assert_eq!(batch.paths.len(), MAX_BATCH_PATHS);
        assert_eq!(batch.markdown, MAX_BATCH_PATHS);
        assert_eq!(batch.events, MAX_BATCH_PATHS + 50);
        assert!(batch.truncated);
    }

    thread_local! {
        static STATS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Stands for `stat`: counts the calls; a path without extension is a folder.
    fn counting_is_dir(path: &Path) -> bool {
        STATS.with(|c| c.set(c.get() + 1));
        path.extension().is_none()
    }

    fn stats() -> usize {
        STATS.with(|c| c.get())
    }

    #[test]
    fn folder_filter_drops_folder_modifications_only() {
        let mut acc = Accumulator::with_folder_filter(ROOT, true, counting_is_dir);
        assert!(!acc.add(event(DATA, &["dir"])), "a modified folder says nothing");
        assert!(acc.is_empty());
        assert!(acc.add(event(DATA, &["dir/a.md"])));
        assert!(acc.add(event(CREATE, &["newdir"])), "creations are never filtered");
        assert!(acc.add(event(RENAME, &["old", "new"])), "nor renames");
        assert!(acc.add(event(REMOVE, &["gone"])));
        let batch = acc.take();
        assert_eq!(batch.paths, [s("dir/a.md"), s("newdir"), s("old"), s("new"), s("gone")]);
        assert_eq!((batch.events, batch.truncated), (4, false));
        assert_eq!(stats(), 2, "only the two content events were looked up");
    }

    #[test]
    fn each_path_is_looked_up_at_most_once_per_batch() {
        let mut acc = Accumulator::with_folder_filter(ROOT, true, counting_is_dir);
        for _ in 0..500 {
            assert!(!acc.add(event(DATA, &["dir"])));
            assert!(acc.add(event(DATA, &["dir/a.md"])));
        }
        assert_eq!(stats(), 2);
        assert_eq!(acc.take().paths, [s("dir/a.md")]);
        // The memory of folders does not outlive the batch.
        assert!(!acc.add(event(DATA, &["dir"])));
        assert_eq!(stats(), 3);
    }

    #[test]
    fn no_lookup_once_the_batch_is_full() {
        let mut acc = Accumulator::with_folder_filter(ROOT, true, counting_is_dir);
        for i in 0..MAX_BATCH_PATHS {
            assert!(acc.add(event(DATA, &[&format!("d/n{i}.md")])));
        }
        assert_eq!(stats(), MAX_BATCH_PATHS);
        // The rest of the burst: new files, the same ones again, folders.
        for i in 0..5000 {
            assert!(acc.add(event(DATA, &[&format!("e/n{}.md", i % 100)])));
            assert!(acc.add(event(DATA, &["e"])));
        }
        assert_eq!(stats(), MAX_BATCH_PATHS, "no stat after the cap");
        let batch = acc.take();
        assert_eq!(batch.paths.len(), MAX_BATCH_PATHS);
        assert!(batch.truncated);
        assert_eq!(batch.events, MAX_BATCH_PATHS + 10_000);
    }

    #[test]
    fn folder_lookups_are_bounded_too() {
        let mut acc = Accumulator::with_folder_filter(ROOT, true, counting_is_dir);
        for i in 0..MAX_BATCH_PATHS {
            assert!(!acc.add(event(DATA, &[&format!("dir{i}")])));
        }
        assert!(acc.is_empty());
        assert_eq!(stats(), MAX_BATCH_PATHS);
        for i in 0..1000 {
            assert!(acc.add(event(DATA, &[&format!("more{i}")])));
        }
        assert_eq!(stats(), MAX_BATCH_PATHS);
        let batch = acc.take();
        assert!(batch.truncated && batch.paths.is_empty());
    }

    #[test]
    fn batch_json_is_camel_case() {
        let batch = ChangeBatch { root: "/v".into(), paths: vec!["/v/a.md".into()], truncated: false, markdown: 1, events: 2 };
        assert_eq!(
            serde_json::to_value(&batch).unwrap(),
            serde_json::json!({"root": "/v", "paths": ["/v/a.md"], "truncated": false, "markdown": 1, "events": 2})
        );
        assert_eq!(WatchOptions::default(), WatchOptions { debounce_ms: 400, max_wait_ms: 3000 });
    }
}
