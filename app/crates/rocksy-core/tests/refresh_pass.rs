//! The refresh pass as the shell's worker runs it: own saves skipped, one
//! listing shared by the tree and the index, folder renames reindexed
//! (BUG-8), the page-facing JSON of the tree.

mod common;

use common::TempDir;
use rocksy_core::{
    list_tree, load_tree_snapshot, open_tree, refresh_pass, save_file, BuildOptions, CancelToken, ChangedPaths,
    IpcTree, OwnWrites, RefreshOutcome, TagIndex, Tree,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn vault(dir: &TempDir) -> PathBuf {
    dir.write("v/home.md", "# Home\n\nsee [[inside]] #home\n");
    dir.write("v/folder/inside.md", "unique-word-zeta #inside\n");
    dir.write("v/folder/pic.png", "png");
    dir.path().join("v")
}

fn listed(outcome: RefreshOutcome) -> rocksy_core::TreeRefresh {
    match outcome {
        RefreshOutcome::Listed(refresh) => refresh,
        other => panic!("expected a listing, got {other:?}"),
    }
}

fn changes(paths: &[&Path]) -> ChangedPaths {
    ChangedPaths::from_paths(paths.iter().map(|p| text(p)).collect())
}

#[test]
fn own_save_skips_the_listing_and_anything_else_lists_once() {
    let dir = TempDir::new("pass-own");
    let root = vault(&dir);
    let data = dir.path().join("data");
    let own = OwnWrites::new();
    let mut known = open_tree(&data, &root).unwrap();
    let never = || false;

    // A save by the application: recorded, tree entry updated in memory.
    let note = root.join("home.md");
    let saved = save_file(&note, "# Home\n\nedited [[inside]] #home\n", None).unwrap();
    assert!(!saved.created);
    own.record(&text(&note), saved.size, saved.mtime_ms, saved.created);
    assert!(known.set_file_meta(&note, saved.size, saved.mtime_ms));
    let outcome = refresh_pass(&data, &root, Some(&known), &changes(&[&note]), &own, &never).unwrap();
    assert_eq!(outcome, RefreshOutcome::OwnWrites);

    // The safety net (unknown changes) lists, and finds nothing new because
    // the known tree was kept up to date: no `tree-updated`.
    let refresh = listed(refresh_pass(&data, &root, Some(&known), &ChangedPaths::unknown(), &own, &never).unwrap());
    assert!(!refresh.changed);
    assert!(!refresh.tree.from_snapshot);

    // The same save together with an external change: the pass lists.
    let other = root.join("folder/inside.md");
    fs::write(&other, "changed outside #inside\n").unwrap();
    let saved = save_file(&note, "# Home\n\nagain [[inside]] #home\n", None).unwrap();
    own.record(&text(&note), saved.size, saved.mtime_ms, saved.created);
    known.set_file_meta(&note, saved.size, saved.mtime_ms);
    let refresh = listed(refresh_pass(&data, &root, Some(&known), &changes(&[&note, &other]), &own, &never).unwrap());
    assert!(refresh.changed);
    known = refresh.tree;

    // A save that creates a note: the tree has to learn about it.
    let created = root.join("new.md");
    let saved = save_file(&created, "#new\n", None).unwrap();
    assert!(saved.created);
    own.record(&text(&created), saved.size, saved.mtime_ms, saved.created);
    assert!(!known.set_file_meta(&created, saved.size, saved.mtime_ms));
    let refresh = listed(refresh_pass(&data, &root, Some(&known), &changes(&[&created]), &own, &never).unwrap());
    assert!(refresh.changed);
    assert!(refresh.tree.entries.iter().any(|e| e.path == "new.md"));
    // The snapshot follows what was shown.
    assert_eq!(load_tree_snapshot(&data, &root).unwrap().entries, refresh.tree.entries);
    // Once listed, the next save of that note is an ordinary own write.
    let saved = save_file(&created, "#new again\n", Some(saved.mtime_ms)).unwrap();
    own.record(&text(&created), saved.size, saved.mtime_ms, saved.created);
    let outcome = refresh_pass(&data, &root, Some(&refresh.tree), &changes(&[&created]), &own, &never).unwrap();
    assert_eq!(outcome, RefreshOutcome::OwnWrites);

    // A cancelled pass writes nothing.
    let outcome = refresh_pass(&data, &root, None, &ChangedPaths::unknown(), &own, &|| true).unwrap();
    assert_eq!(outcome, RefreshOutcome::Cancelled);
}

#[test]
fn folder_rename_is_reindexed_from_the_shared_listing() {
    let dir = TempDir::new("pass-rename");
    let root = vault(&dir);
    let data = dir.path().join("data");
    let own = OwnWrites::new();
    let index = TagIndex::new();
    let cancel = CancelToken::new();
    let never = || false;

    // Opening: one listing for the tree and the index.
    let first = listed(refresh_pass(&data, &root, None, &ChangedPaths::unknown(), &own, &never).unwrap());
    let report = index.reconcile_with_tree(&data, &first.tree, BuildOptions::default(), &cancel, |_| {}).unwrap().unwrap();
    assert_eq!((report.files, report.reread), (2, 2));
    index.flush_search();
    let old_path = text(&root.join("folder/inside.md"));
    assert_eq!(index.search("unique-word-zeta", None).hits[0].path, old_path);
    assert_eq!(index.outgoing_links(&root.join("home.md"))[0].resolved.as_deref(), Some(old_path.as_str()));
    let generation = index.links_generation();

    // An external tool renames the folder. The watcher reports the two
    // folder paths only: no `.md` among them.
    fs::rename(root.join("folder"), root.join("renamed")).unwrap();
    let batch = changes(&[&root.join("folder"), &root.join("renamed")]);
    let refresh = listed(refresh_pass(&data, &root, Some(&first.tree), &batch, &own, &never).unwrap());
    assert!(refresh.changed);
    let report = index.reconcile_with_tree(&data, &refresh.tree, BuildOptions::default(), &cancel, |_| {}).unwrap().unwrap();
    assert_eq!((report.files, report.reread, report.removed), (2, 1, 1));
    index.flush_search();

    let new_path = text(&root.join("renamed/inside.md"));
    let hits = index.search("unique-word-zeta", None).hits;
    assert_eq!(hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), [new_path.as_str()]);
    assert_eq!(index.outgoing_links(&root.join("home.md"))[0].resolved.as_deref(), Some(new_path.as_str()));
    assert_eq!(index.backlinks(Path::new(&new_path)).len(), 1);
    assert!(index.backlinks(Path::new(&old_path)).is_empty());
    assert!(index.tag_fragments("inside", None).iter().all(|f| f.path == new_path));
    assert_ne!(index.links_generation(), generation, "the page is told to refresh its links");
    let ready = index.index_ready(false, 0, Some(&report));
    assert_eq!(ready.vault, refresh.tree.root);
    assert_eq!(ready.links_generation, index.links_generation());

    // A pass with nothing new reads no note and keeps the links generation.
    let generation = index.links_generation();
    let again = listed(refresh_pass(&data, &root, Some(&refresh.tree), &ChangedPaths::unknown(), &own, &never).unwrap());
    assert!(!again.changed);
    let report = index.reconcile_with_tree(&data, &again.tree, BuildOptions::default(), &cancel, |_| {}).unwrap().unwrap();
    assert_eq!((report.reread, report.removed), (0, 0));
    assert_eq!(index.links_generation(), generation);
}

#[test]
fn tree_sent_to_the_page_has_no_size_nor_mtime() {
    let dir = TempDir::new("pass-ipc");
    let root = vault(&dir);
    let tree = list_tree(&root).unwrap();
    let expected = json!({
        "root": tree.root,
        "fromSnapshot": false,
        "entries": [
            { "path": "folder", "isDir": true },
            { "path": "folder/inside.md", "isDir": false },
            { "path": "folder/pic.png", "isDir": false },
            { "path": "home.md", "isDir": false },
        ],
    });
    assert_eq!(serde_json::to_value(IpcTree(&tree)).unwrap(), expected);
    // Same JSON from a shared tree, as events send it.
    let shared: Arc<Tree> = Arc::new(tree.clone());
    assert_eq!(serde_json::to_value(IpcTree(shared)).unwrap(), expected);
    // The backend keeps sizes and times (snapshot, change detection).
    let full = serde_json::to_value(&tree).unwrap();
    assert!(full["entries"][1]["size"].as_u64().unwrap() > 0);
    assert!(full["entries"][1]["mtimeMs"].as_u64().unwrap() > 0);
}
