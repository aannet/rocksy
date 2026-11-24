//! Own-writes registry against the real watcher and the real `save_file`:
//! what the refresh worker will see, and when it may skip the listing.

mod common;

use common::TempDir;
use rocksy_core::{save_file, watch_vault, ChangeBatch, ChangedPaths, OwnWrites, VaultWatcher, WatchOptions};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(20);
const FAST: WatchOptions = WatchOptions { debounce_ms: 150, max_wait_ms: 1000 };
const QUIET: Duration = Duration::from_millis(600);

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn vault(dir: &TempDir) -> PathBuf {
    dir.write("v/a.md", "#a\n");
    dir.write("v/sub/b.md", "#b\n");
    dir.write("v/folder/inside.md", "#inside\n");
    dir.path().join("v")
}

fn start(root: &Path) -> (VaultWatcher, mpsc::Receiver<ChangeBatch>) {
    let (tx, rx) = mpsc::channel();
    let watcher = watch_vault(root, FAST, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();
    (watcher, rx)
}

/// Waits for a batch naming `wanted`, then until nothing more arrives, and
/// merges everything received the way the worker merges piled-up requests.
fn changes_including(rx: &mpsc::Receiver<ChangeBatch>, wanted: &Path) -> (ChangedPaths, Vec<ChangeBatch>) {
    let wanted = text(wanted);
    let start = Instant::now();
    let mut batches: Vec<ChangeBatch> = Vec::new();
    loop {
        let seen = batches.iter().any(|b| b.paths.contains(&wanted));
        match rx.recv_timeout(if seen { QUIET } else { DEADLINE.saturating_sub(start.elapsed()) }) {
            Ok(batch) => batches.push(batch),
            Err(_) if seen => break,
            Err(_) => panic!("no batch for {wanted} within {DEADLINE:?}: {batches:#?}"),
        }
    }
    let mut merged = ChangedPaths::from_batch(&batches[0]);
    for batch in &batches[1..] {
        merged.merge(ChangedPaths::from_batch(batch));
    }
    (merged, batches)
}

/// `save_file` followed by the bookkeeping the shell command will do.
fn app_save(own: &OwnWrites, path: &Path, content: &str) {
    let created = !path.exists();
    let saved = save_file(path, content, None).unwrap();
    own.record(&text(path), saved.size, saved.mtime_ms, created);
}

#[test]
fn own_writes_let_the_worker_skip_the_listing_after_an_app_save() {
    let dir = TempDir::new("own-save");
    let root = vault(&dir);
    let (_watcher, rx) = start(&root);
    let own = OwnWrites::new();
    let note = root.join("sub").join("b.md");

    app_save(&own, &note, "#b saved by the app\n");
    let (changes, batches) = changes_including(&rx, &note);
    // The temporary file of the atomic save is hidden: the note is the only path.
    assert_eq!(changes, ChangedPaths::Paths(vec![text(&note); batches.len()]), "{batches:#?}");
    assert!(own.only_own_writes(&changes));

    // Two quick saves: whichever batch arrives, the latest record matches the disk.
    app_save(&own, &note, "#b second\n");
    app_save(&own, &note, "#b third, longer\n");
    let (changes, _) = changes_including(&rx, &note);
    assert!(own.only_own_writes(&changes));
}

#[test]
fn own_writes_never_hide_an_external_change() {
    let dir = TempDir::new("own-external");
    let root = vault(&dir);
    let (_watcher, rx) = start(&root);
    let own = OwnWrites::new();
    let note = root.join("a.md");
    let other = root.join("sub").join("b.md");

    // Another program rewrites the note right after the app saved it.
    app_save(&own, &note, "#a from the app\n");
    fs::write(&note, "#a rewritten by something else, other size\n").unwrap();
    let (changes, _) = changes_including(&rx, &note);
    assert!(!own.only_own_writes(&changes));

    // App save and external change of another file in the same pass: mixed.
    app_save(&own, &note, "#a again\n");
    fs::write(&other, "#b external\n").unwrap();
    let (mut changes, _) = changes_including(&rx, &other);
    changes.merge(ChangedPaths::from_paths(vec![text(&note)]));
    assert!(!own.only_own_writes(&changes));

    // The saved note is deleted afterwards.
    app_save(&own, &note, "#a once more\n");
    let _ = changes_including(&rx, &note);
    fs::remove_file(&note).unwrap();
    let (changes, _) = changes_including(&rx, &note);
    assert!(!own.only_own_writes(&changes));

    // A request with no path list (focus safety net) is never skipped.
    app_save(&own, &other, "#b by the app\n");
    let (mut changes, _) = changes_including(&rx, &other);
    assert!(own.only_own_writes(&changes));
    changes.merge(ChangedPaths::unknown());
    assert!(!own.only_own_writes(&changes));
}

#[test]
fn own_writes_do_not_skip_a_file_created_by_the_app() {
    let dir = TempDir::new("own-create");
    let root = vault(&dir);
    let (_watcher, rx) = start(&root);
    let own = OwnWrites::new();
    let fresh = root.join("new note.md");

    app_save(&own, &fresh, "#new\n");
    // Auto-save strikes again before the worker runs.
    app_save(&own, &fresh, "#new, edited\n");
    let (changes, _) = changes_including(&rx, &fresh);
    assert!(!own.only_own_writes(&changes), "the tree does not know the new file yet");

    // The worker lists the vault; from then on saves of that note are skipped.
    let listing_started = Instant::now();
    let tree = rocksy_core::list_tree(&root).unwrap();
    assert!(tree.entries.iter().any(|e| e.path == "new note.md"));
    own.listing_started_at(listing_started);
    app_save(&own, &fresh, "#new, edited twice\n");
    let (changes, _) = changes_including(&rx, &fresh);
    assert!(own.only_own_writes(&changes));
}

#[test]
fn own_writes_do_not_hide_a_folder_rename() {
    // BUG-8: the batch of a folder rename names folders only. It must never
    // look like an own write, even merged with a real one.
    let dir = TempDir::new("own-folder");
    let root = vault(&dir);
    let (_watcher, rx) = start(&root);
    let own = OwnWrites::new();
    let note = root.join("a.md");
    let renamed = root.join("renamed");

    app_save(&own, &note, "#a saved\n");
    fs::rename(root.join("folder"), &renamed).unwrap();
    let (changes, batches) = changes_including(&rx, &renamed);
    assert!(batches.iter().any(|b| b.paths.contains(&text(&renamed))));
    assert!(!own.only_own_writes(&changes));
    let mut merged = ChangedPaths::from_paths(vec![text(&note)]);
    assert!(own.only_own_writes(&merged));
    merged.merge(changes);
    assert!(!own.only_own_writes(&merged));

    // A folder that takes the place and the name of a saved note is not the note.
    let (_, _) = (fs::remove_file(&note).unwrap(), fs::create_dir(&note).unwrap());
    assert!(!own.only_own_writes(&ChangedPaths::from_paths(vec![text(&note)])));
}

#[test]
fn own_writes_expire_and_follow_the_real_stat() {
    let dir = TempDir::new("own-expire");
    let note = dir.write("v/a.md", "#a\n");
    let own = OwnWrites::with_limits(Duration::from_millis(300), 4);
    app_save(&own, &note, "#a saved\n");
    let changes = ChangedPaths::from_paths(vec![text(&note)]);
    assert!(own.only_own_writes(&changes));
    assert_eq!(rocksy_core::own_writes::stat_file(&text(&note)).map(|s| s.size), Some(9));
    std::thread::sleep(Duration::from_millis(400));
    assert!(!own.only_own_writes(&changes), "record expired");
}
