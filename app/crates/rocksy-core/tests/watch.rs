//! Folder watcher against a real file system (inotify on Linux,
//! ReadDirectoryChangesW on Windows).
//!
//! Timing: nothing here relies on a tight sleep. Expectations are awaited
//! with a generous deadline; the only fixed waits are "nothing more happens"
//! checks, which can only fail if the watcher really misbehaves.

mod common;

use common::{fingerprint, make_vault, TempDir};
use rocksy_core::{save_file, watch_vault, ChangeBatch, ErrorKind, VaultWatcher, WatchOptions};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(20);
const FAST: WatchOptions = WatchOptions { debounce_ms: 150, max_wait_ms: 1000 };

/// Set when the watcher thread unwinds: the callback (and this probe with
/// it) is then dropped during a panic.
struct PanicProbe(Arc<AtomicBool>);

impl Drop for PanicProbe {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

#[derive(Default)]
struct Collector {
    batches: Mutex<Vec<ChangeBatch>>,
    changed: Condvar,
    running: AtomicBool,
    overlapped: AtomicBool,
    panicked: Arc<AtomicBool>,
}

impl Collector {
    fn len(&self) -> usize {
        self.batches.lock().unwrap().len()
    }

    fn all(&self) -> Vec<ChangeBatch> {
        self.batches.lock().unwrap().clone()
    }

    /// Waits until `pred` holds on the batches received from index `from`.
    fn wait(&self, what: &str, from: usize, pred: impl Fn(&[ChangeBatch]) -> bool) -> Vec<ChangeBatch> {
        let start = Instant::now();
        let mut guard = self.batches.lock().unwrap();
        loop {
            if pred(&guard[from.min(guard.len())..]) {
                return guard[from.min(guard.len())..].to_vec();
            }
            let left = DEADLINE.checked_sub(start.elapsed()).unwrap_or_else(|| {
                panic!("timed out waiting for {what}; batches so far: {:#?}", &guard[..])
            });
            guard = self.changed.wait_timeout(guard, left).unwrap().0;
        }
    }

    /// Waits until no batch has arrived for `quiet`, and returns the count.
    fn settle(&self, quiet: Duration) -> usize {
        let start = Instant::now();
        let mut guard = self.batches.lock().unwrap();
        loop {
            let before = guard.len();
            guard = self.changed.wait_timeout(guard, quiet).unwrap().0;
            if guard.len() == before || start.elapsed() > DEADLINE {
                return guard.len();
            }
        }
    }
}

fn start(root: &Path, options: WatchOptions) -> (VaultWatcher, Arc<Collector>) {
    let collector = Arc::new(Collector::default());
    let sink = Arc::clone(&collector);
    let probe = PanicProbe(Arc::clone(&collector.panicked));
    let watcher = watch_vault(root, options, move |batch| {
        let _keep = &probe;
        if sink.running.swap(true, Ordering::SeqCst) {
            sink.overlapped.store(true, Ordering::SeqCst);
        }
        sink.batches.lock().unwrap().push(batch);
        sink.changed.notify_all();
        sink.running.store(false, Ordering::SeqCst);
    })
    .unwrap();
    (watcher, collector)
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn union(batches: &[ChangeBatch]) -> HashSet<String> {
    batches.iter().flat_map(|b| b.paths.iter().cloned()).collect()
}

fn has(batches: &[ChangeBatch], path: &Path) -> bool {
    let wanted = text(path);
    batches.iter().any(|b| b.paths.contains(&wanted))
}

fn is_hidden(root: &Path, path: &str) -> bool {
    Path::new(path)
        .strip_prefix(root)
        .map(|rel| rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')))
        .unwrap_or(true)
}

fn quiet() -> Duration {
    Duration::from_millis(FAST.debounce_ms * 4)
}

fn vault(dir: &TempDir) -> PathBuf {
    dir.write("v/a.md", "#a\n");
    dir.write("v/sub/b.md", "#b\n");
    dir.write("v/sub/deep/c.md", "#c\n");
    dir.write("v/.obsidian/app.json", "{}");
    dir.write("v/.trash/old.md", "x");
    dir.path().join("v")
}

#[test]
fn create_modify_remove_rename_of_visible_files() {
    let dir = TempDir::new("watch-basic");
    let root = vault(&dir);
    let (watcher, seen) = start(&root, FAST);
    assert_eq!(watcher.root(), text(&root));

    // Create.
    let created = root.join("sub").join("new.md");
    fs::write(&created, "#new\n").unwrap();
    let got = seen.wait("creation", 0, |b| has(b, &created));
    assert!(got.iter().all(|b| b.root == text(&root) && !b.truncated && b.events > 0));
    assert!(got.iter().all(|b| b.markdown == b.paths.iter().filter(|p| p.ends_with(".md")).count()));

    // Modify.
    let from = seen.settle(quiet());
    fs::write(root.join("a.md"), "#a changed\n").unwrap();
    let got = seen.wait("modification", from, |b| has(b, &root.join("a.md")));
    assert_eq!(union(&got), HashSet::from([text(&root.join("a.md"))]), "only the modified file");

    // Remove.
    let from = seen.settle(quiet());
    let doomed = root.join("sub").join("deep").join("c.md");
    fs::remove_file(&doomed).unwrap();
    let got = seen.wait("removal", from, |b| has(b, &doomed));
    assert_eq!(union(&got), HashSet::from([text(&doomed)]));

    // Rename: both the old and the new path.
    let from = seen.settle(quiet());
    let renamed = root.join("sub").join("renamed.md");
    fs::rename(&created, &renamed).unwrap();
    let got = seen.wait("rename", from, |b| has(b, &created) && has(b, &renamed));
    assert_eq!(union(&got), HashSet::from([text(&created), text(&renamed)]));

    // A non-Markdown file counts as a path but not as Markdown.
    let from = seen.settle(quiet());
    let image = root.join("img.png");
    fs::write(&image, [1u8, 2, 3]).unwrap();
    let got = seen.wait("attachment", from, |b| has(b, &image));
    assert!(got.iter().all(|b| b.markdown == 0));

    drop(watcher);
    assert!(!seen.overlapped.load(Ordering::SeqCst));
    assert!(!seen.panicked.load(Ordering::SeqCst));
}

#[test]
fn changes_in_dot_folders_produce_no_batch() {
    let dir = TempDir::new("watch-hidden");
    let root = vault(&dir);
    let (_watcher, seen) = start(&root, FAST);

    // Existing and new dot-folders at the root, and a hidden file.
    fs::write(root.join(".obsidian").join("workspace.json"), "{\"a\":1}").unwrap();
    fs::write(root.join(".obsidian").join("new.json"), "{}").unwrap();
    fs::create_dir_all(root.join(".opencode").join("node_modules").join("pkg")).unwrap();
    fs::write(root.join(".opencode").join("node_modules").join("pkg").join("index.js"), "x").unwrap();
    fs::rename(root.join(".trash").join("old.md"), root.join(".trash").join("older.md")).unwrap();
    fs::remove_file(root.join(".obsidian").join("new.json")).unwrap();
    fs::write(root.join(".hidden.md"), "#x").unwrap();
    fs::remove_file(root.join(".hidden.md")).unwrap();

    // A visible write afterwards proves the watcher was listening all along.
    let sentinel = root.join("sentinel.md");
    fs::write(&sentinel, "#s\n").unwrap();
    seen.wait("sentinel", 0, |b| has(b, &sentinel));
    seen.settle(quiet());
    let all = seen.all();
    assert_eq!(union(&all), HashSet::from([text(&sentinel)]), "{all:#?}");

    // Inside a visible folder, a dot-folder is ignored as well.
    let from = seen.len();
    fs::create_dir_all(root.join("sub").join(".cache")).unwrap();
    fs::write(root.join("sub").join(".cache").join("x.md"), "#x").unwrap();
    fs::write(&sentinel, "#s again\n").unwrap();
    seen.wait("second sentinel", from, |b| has(b, &sentinel));
    seen.settle(quiet());
    for path in union(&seen.all()) {
        assert!(!is_hidden(&root, &path), "{path}");
    }
}

#[test]
fn atomic_save_reports_only_the_saved_note() {
    let dir = TempDir::new("watch-save");
    let root = vault(&dir);
    let (_watcher, seen) = start(&root, FAST);

    // Existing note, in a sub-folder: temporary `.b.md.<pid>-<n>.rocksy-tmp`, then rename.
    let note = root.join("sub").join("b.md");
    save_file(&note, "#b saved\n", None).unwrap();
    seen.wait("save of an existing note", 0, |b| has(b, &note));
    let count = seen.settle(quiet());
    let all = seen.all();
    assert_eq!(union(&all), HashSet::from([text(&note)]), "{all:#?}");
    assert!(all.iter().all(|b| b.markdown == 1 && !b.truncated));

    // New note.
    let fresh = root.join("x.md");
    save_file(&fresh, "#x\n", None).unwrap();
    let got = seen.wait("save of a new note", count, |b| has(b, &fresh));
    seen.settle(quiet());
    assert_eq!(union(&got), HashSet::from([text(&fresh)]));
    assert_eq!(union(&seen.all()[count..]), HashSet::from([text(&fresh)]));
}

#[test]
fn burst_of_300_writes_gives_few_batches_and_every_path() {
    let dir = TempDir::new("watch-burst");
    let root = vault(&dir);
    let files: Vec<PathBuf> = (0..300).map(|i| root.join(format!("d{}", i % 6)).join(format!("n{i:03}.md"))).collect();
    for f in &files {
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, "#before\n").unwrap();
    }
    let (_watcher, seen) = start(&root, FAST);

    let started = Instant::now();
    for (i, f) in files.iter().enumerate() {
        fs::write(f, format!("#after/{i}\n")).unwrap();
    }
    let wrote_in = started.elapsed();
    let expected: HashSet<String> = files.iter().map(|f| text(f)).collect();
    seen.wait("all 300 paths", 0, |b| union(b).is_superset(&expected));
    let complete_in = started.elapsed();
    let count = seen.settle(quiet());
    let all = seen.all();
    eprintln!("burst: 300 writes in {wrote_in:?}, all paths after {complete_in:?}, {count} batch(es)");

    assert_eq!(union(&all), expected, "nothing but the 300 files");
    assert!(all.iter().all(|b| !b.truncated));
    assert_eq!(all.iter().map(|b| b.markdown).sum::<usize>(), all.iter().map(|b| b.paths.len()).sum::<usize>());
    // One batch when the burst is shorter than the debounce; never one per file.
    let allowed = 2 + (wrote_in.as_millis() as usize) / FAST.debounce_ms as usize;
    assert!(count <= allowed.min(20), "{count} batches for a burst of {wrote_in:?}");
}

#[test]
fn continuous_activity_still_emits_batches_every_max_wait() {
    let dir = TempDir::new("watch-continuous");
    let root = vault(&dir);
    // The debounce alone would never fire: a write every few milliseconds.
    let options = WatchOptions { debounce_ms: 500, max_wait_ms: 900 };
    let (_watcher, seen) = start(&root, options);

    let started = Instant::now();
    let mut writes = 0;
    while seen.len() < 2 {
        assert!(started.elapsed() < DEADLINE, "no batch while writing continuously for {DEADLINE:?}");
        fs::write(root.join(format!("burst{}.md", writes % 20)), format!("#w{writes}\n")).unwrap();
        writes += 1;
        thread::sleep(Duration::from_millis(5));
    }
    // Two batches arrived while the writes were still going on.
    let elapsed = started.elapsed();
    eprintln!("continuous: {writes} writes, 2 batches after {elapsed:?}");
    assert!(elapsed >= Duration::from_millis(options.max_wait_ms), "{elapsed:?}");
    let all = seen.all();
    assert!(all.iter().all(|b| !b.paths.is_empty() && b.paths.len() <= 20));
}

#[test]
fn folder_created_after_start_is_watched_too() {
    let dir = TempDir::new("watch-newdir");
    let root = vault(&dir);
    let (_watcher, seen) = start(&root, FAST);

    let folder = root.join("nouveau").join("dossier");
    fs::create_dir_all(&folder).unwrap();
    seen.wait("new folders", 0, |b| has(b, &root.join("nouveau")));
    let from = seen.settle(quiet());

    let inside = folder.join("dedans.md");
    fs::write(&inside, "#dedans\n").unwrap();
    let got = seen.wait("file in a folder created after start", from, |b| has(b, &inside));
    assert!(got.iter().any(|b| b.markdown == 1));

    // A folder moved in with its content, then removed.
    let from = seen.settle(quiet());
    fs::rename(root.join("sub"), root.join("moved")).unwrap();
    seen.wait("folder rename", from, |b| has(b, &root.join("sub")) && has(b, &root.join("moved")));
    let from = seen.settle(quiet());
    fs::write(root.join("moved").join("b.md"), "#b after move\n").unwrap();
    seen.wait("file in a renamed folder", from, |b| has(b, &root.join("moved").join("b.md")));
}

#[test]
fn drop_stops_immediately_even_during_a_burst() {
    let dir = TempDir::new("watch-drop");
    let root = vault(&dir);
    let (watcher, seen) = start(&root, FAST);

    let stop = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicUsize::new(0));
    let writer = {
        let (stop, writes, root) = (Arc::clone(&stop), Arc::clone(&writes), root.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let n = writes.fetch_add(1, Ordering::SeqCst);
                fs::write(root.join(format!("w{}.md", n % 50)), format!("#w{n}\n")).unwrap();
                thread::sleep(Duration::from_millis(2));
            }
        })
    };
    seen.wait("a first batch during the burst", 0, |b| !b.is_empty());

    let started = Instant::now();
    drop(watcher);
    let drop_took = started.elapsed();
    let at_drop = seen.len();
    let writes_at_drop = writes.load(Ordering::SeqCst);

    // The burst goes on, well past debounce + max_wait: not a single call.
    thread::sleep(Duration::from_millis(FAST.debounce_ms + FAST.max_wait_ms + 800));
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(writes.load(Ordering::SeqCst) > writes_at_drop + 20, "the burst did continue");
    assert_eq!(seen.len(), at_drop, "no batch after drop");
    assert!(drop_took < Duration::from_secs(5), "drop took {drop_took:?}");
    assert!(!seen.panicked.load(Ordering::SeqCst));
    eprintln!("drop during a burst took {drop_took:?}");
}

#[test]
fn watcher_can_be_dropped_from_its_own_callback() {
    let dir = TempDir::new("watch-selfdrop");
    let root = vault(&dir);
    let slot: Arc<Mutex<Option<VaultWatcher>>> = Arc::new(Mutex::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let watcher = {
        let (slot, calls) = (Arc::clone(&slot), Arc::clone(&calls));
        watch_vault(&root, FAST, move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            // Stop watching from inside the callback: must not deadlock.
            drop(slot.lock().unwrap().take());
            let _ = tx.send(());
        })
        .unwrap()
    };
    *slot.lock().unwrap() = Some(watcher);

    fs::write(root.join("a.md"), "#first\n").unwrap();
    rx.recv_timeout(DEADLINE).expect("callback returned after dropping the watcher");
    fs::write(root.join("a.md"), "#second\n").unwrap();
    thread::sleep(Duration::from_millis(FAST.debounce_ms + FAST.max_wait_ms + 500));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn root_removed_while_watching_does_not_panic() {
    let dir = TempDir::new("watch-gone");
    let root = vault(&dir);
    let (watcher, seen) = start(&root, FAST);
    fs::write(root.join("a.md"), "#alive\n").unwrap();
    seen.wait("a batch before removal", 0, |b| has(b, &root.join("a.md")));

    // The folder disappears under the watcher (drive unmounted, folder deleted).
    // On Windows the open watch handle can make this fail or be deferred.
    let removed = fs::remove_dir_all(&root).is_ok() && !root.exists();
    seen.settle(Duration::from_millis(FAST.debounce_ms + FAST.max_wait_ms + 300));
    let started = Instant::now();
    drop(watcher);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!seen.panicked.load(Ordering::SeqCst), "watcher thread panicked");
    for batch in seen.all() {
        assert!(batch.paths.iter().all(|p| !is_hidden(&root, p)));
    }
    eprintln!("root removed: {removed}, batches: {}", seen.len());

    // Watching can start again once the folder is back.
    fs::create_dir_all(&root).unwrap();
    let (_again, seen) = start(&root, FAST);
    fs::write(root.join("back.md"), "#back\n").unwrap();
    seen.wait("a batch after the folder is back", 0, |b| has(b, &root.join("back.md")));
}

#[test]
fn start_errors_are_clean() {
    let dir = TempDir::new("watch-errors");
    let missing = watch_vault(&dir.path().join("missing"), WatchOptions::default(), |_| {});
    assert_eq!(missing.err().unwrap().kind, ErrorKind::NotFound);
    let file = dir.write("file.md", "x");
    let not_a_folder = watch_vault(&file, WatchOptions::default(), |_| {});
    assert_eq!(not_a_folder.err().unwrap().kind, ErrorKind::Invalid);

    // Several watchers on the same folder work independently, and stopping
    // one does not stop the other.
    let root = vault(&dir);
    let (first, seen_first) = start(&root, FAST);
    let (_second, seen_second) = start(&root, FAST);
    fs::write(root.join("a.md"), "#both\n").unwrap();
    seen_first.wait("first watcher", 0, |b| has(b, &root.join("a.md")));
    seen_second.wait("second watcher", 0, |b| has(b, &root.join("a.md")));
    drop(first);
    let from = seen_second.settle(quiet());
    fs::write(root.join("a.md"), "#second only\n").unwrap();
    seen_second.wait("second watcher after the first stopped", from, |b| has(b, &root.join("a.md")));
}

#[test]
fn watching_a_vault_does_not_modify_it() {
    // F7: listening, reading and stopping leave every byte and timestamp alone.
    let dir = TempDir::new("watch-f7");
    let root = make_vault(&dir, "vault");
    let before = fingerprint(&root);
    let (watcher, seen) = start(&root, FAST);
    for entry in rocksy_core::list_tree(&root).unwrap().entries {
        if !entry.is_dir {
            let _ = rocksy_core::read_file(&root.join(&entry.path));
        }
    }
    seen.settle(Duration::from_millis(FAST.debounce_ms + FAST.max_wait_ms));
    drop(watcher);
    assert_eq!(fingerprint(&root), before);
    assert_eq!(seen.len(), 0, "reading files is not a change: {:#?}", seen.all());
}
