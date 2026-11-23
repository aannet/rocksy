//! F7: the core never writes to a vault on its own. Opening the tree,
//! reconciling, indexing tags, reading files and managing the registry must
//! leave every byte and every modification time of the vault untouched, and
//! create nothing in it. The only write is `save_file` on the file asked for.

mod common;

use common::{fingerprint, make_vault, TempDir};
use rocksy_core::{
    add_vault, file_tags, list_vaults, load_session, open_tree, read_file, reconcile_tree,
    remove_vault, save_file, save_session, vault_for_path, BuildOptions, CancelToken, ErrorKind,
    Session, TagIndex,
};
use std::fs;

#[test]
fn opening_reconciling_and_indexing_do_not_touch_the_vault() {
    let dir = TempDir::new("f7");
    let vault = make_vault(&dir, "vault");
    let data = dir.path().join("appdata");
    let before = fingerprint(&vault);
    assert!(before.len() > 20);

    // Registry.
    add_vault(&data, &vault).unwrap();
    assert_eq!(list_vaults(&data).len(), 1);
    assert!(vault_for_path(&data, &vault.join("Accueil.md")).is_some());

    // Tree: first open (listing + snapshot), second open (snapshot), reconcile.
    let first = open_tree(&data, &vault).unwrap();
    assert!(!first.from_snapshot);
    assert!(first.entries.iter().all(|e| !e.path.split('/').any(|c| c.starts_with('.'))));
    let second = open_tree(&data, &vault).unwrap();
    assert!(second.from_snapshot);
    assert!(!reconcile_tree(&data, &vault, |_| panic!("nothing changed")).unwrap());

    // Tag index: build, query, update in memory.
    let index = TagIndex::new();
    let ready = index.build(&vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    assert_eq!(ready.files, 6);
    assert_eq!(index.stats().skipped, 1, "the Latin-1 file is skipped, not converted");
    let tags = index.tags_list("");
    assert!(tags.iter().any(|t| t.tag == "projet" && t.count == 1));
    assert!(tags.iter().any(|t| t.tag == "été"));
    assert!(!tags.iter().any(|t| ["canvas", "corbeille", "not-a-tag"].contains(&t.tag.as_str())));
    for t in &tags {
        assert_eq!(index.tag_fragments(&t.tag, None).len() as u32, t.count);
    }
    index.update_file(&vault.join("Accueil.md"), "#modifie en memoire\n");

    // Reading files, including the ones that must fail.
    for entry in &first.entries {
        if !entry.is_dir {
            let _ = read_file(&vault.join(&entry.path));
        }
    }
    assert_eq!(read_file(&vault.join("02 Perso/latin1.md")).unwrap_err().kind, ErrorKind::NotUtf8);
    let journal = read_file(&vault.join("01 Travail/Journal.md")).unwrap();
    assert!(journal.text.starts_with('\u{feff}') && journal.text.contains("\r\n"));
    file_tags(&journal.path, &journal.text);

    // Session, then removal from the registry.
    save_session(&data, &Session { active_vault: Some(first.root.clone()), tabs: vec![journal.path.clone()], active_tab: Some(journal.path) }).unwrap();
    assert!(load_session(&data).is_some());

    // Refused writes.
    for rel in [".obsidian/app.json", ".obsidian/nouveau.json", "Tableau.canvas", "Nouveau.canvas", ".obsidian/plugins/demo/data.json"] {
        assert_eq!(save_file(&vault.join(rel), "x", None).unwrap_err().kind, ErrorKind::Invalid, "{rel}");
    }
    remove_vault(&data, &vault).unwrap();

    // Nothing created, removed, modified or even re-timestamped.
    let after = fingerprint(&vault);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    assert_eq!(after, before);

    // Rocksy's own data is in the data directory only.
    let data_files = fingerprint(&data);
    let names: Vec<&str> = data_files.keys().map(String::as_str).collect();
    assert_eq!(names.len(), 4, "{names:?}");
    assert!(names.contains(&"vaults.json") && names.contains(&"session.json") && names.contains(&"trees/"));
    assert!(names.iter().any(|n| n.starts_with("trees/") && n.ends_with(".tree")));
}

#[test]
fn save_changes_only_the_saved_file() {
    let dir = TempDir::new("f7-save");
    let vault = make_vault(&dir, "vault");
    let before = fingerprint(&vault);

    let target = vault.join("01 Travail/Journal.md");
    let original = read_file(&target).unwrap();
    let edited = format!("{}- mardi #journal/2026\r\n", original.text);
    let saved = save_file(&target, &edited, Some(original.mtime_ms)).unwrap();
    assert_eq!(saved.size, edited.len() as u64);

    let after = fingerprint(&vault);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>(), "no file created (no leftover temporary)");
    for (path, state) in &after {
        if path == "01 Travail/Journal.md" {
            assert_ne!(state, &before[path]);
        } else if !path.ends_with('/') {
            assert_eq!(state, &before[path], "{path}");
        }
    }
    // Byte-order mark and CRLF survive the round trip untouched.
    let bytes = fs::read(&target).unwrap();
    assert_eq!(bytes, edited.as_bytes());
    assert!(bytes.starts_with(b"\xEF\xBB\xBF# Journal\r\n"));

    // Saving the same text back restores the exact original bytes.
    save_file(&target, &original.text, Some(saved.mtime_ms)).unwrap();
    assert_eq!(fingerprint(&vault)["01 Travail/Journal.md"].as_ref().unwrap().hash, before["01 Travail/Journal.md"].as_ref().unwrap().hash);
}

#[test]
fn a_file_outside_any_vault_gets_nothing_created_next_to_it() {
    let dir = TempDir::new("f7-loose");
    // A folder that looks like an Obsidian vault but is not registered.
    let folder = make_vault(&dir, "not-registered");
    let data = dir.path().join("appdata");
    let note = folder.join("Accueil.md");
    let before = fingerprint(&folder);

    assert!(vault_for_path(&data, &note).is_none(), "unregistered folder: isolated file");
    let content = read_file(&note).unwrap();
    let fragments = file_tags(&content.path, &content.text);
    assert_eq!(fragments.iter().map(|f| f.tag.as_str()).collect::<Vec<_>>(), ["accueil", "projet/rocksy", "demo"]);
    // An index that was never built accepts in-memory updates without touching the disk.
    let index = TagIndex::new();
    index.update_file(&note, &content.text);
    assert_eq!(index.tags_list("demo").len(), 1);

    assert_eq!(fingerprint(&folder), before);
    assert!(!data.exists(), "nothing persisted just for reading a loose file");

    // A file inside an unregistered `.obsidian` folder opens like any other.
    let inner = read_file(&folder.join(".obsidian/app.json")).unwrap();
    assert_eq!(inner.text, "{\"promptDelete\":false}");
    assert_eq!(fingerprint(&folder), before);
}

#[test]
fn persisting_and_reconciling_tags_do_not_touch_the_vault() {
    let dir = TempDir::new("f7-persist");
    let vault = make_vault(&dir, "vault");
    let data = dir.path().join("appdata");
    let before = fingerprint(&vault);

    // First session: full build through reconcile, in-memory edits, save.
    let index = TagIndex::new();
    let report = index.reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    assert_eq!((report.files, report.reread, report.persisted), (6, 6, true));
    index.update_file(&vault.join("Accueil.md"), "#modifie en memoire\n");
    index.update_file_with_meta(&vault.join("02 Perso/vide.md"), "#autre\n", 7, 1);
    index.remove_file(&vault.join("02 Perso/Recettes.md"));
    assert!(index.save(&data).unwrap());

    // Second session: load, reconcile (re-reads what the edits made stale), save.
    let session = TagIndex::new();
    assert!(session.load_persisted(&data, &vault).is_some());
    let report = session.reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    assert_eq!((report.files, report.reread, report.removed), (6, 3, 0));
    assert!(!session.save(&data).unwrap());
    // Cancelled reconcile and clear.
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(session.reconcile(&data, &vault, BuildOptions::default(), &cancel, |_| {}).unwrap().is_none());
    session.clear();

    assert_eq!(fingerprint(&vault), before, "vault untouched, nothing created in it");

    // The persisted index (which holds excerpts of notes) is in the data directory only.
    let data_files = fingerprint(&data);
    let names: Vec<&str> = data_files.keys().map(String::as_str).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"tags/") && names.iter().any(|n| n.starts_with("tags/") && n.ends_with(".tags")));
}
