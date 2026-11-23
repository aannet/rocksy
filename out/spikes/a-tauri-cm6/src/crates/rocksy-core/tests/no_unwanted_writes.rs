//! F7: the core never writes to a vault on its own. Opening the tree,
//! reconciling, indexing tags, reading files and managing the registry must
//! leave every byte and every modification time of the vault untouched, and
//! create nothing in it. The only write is `save_file` on the file asked for.
//! The only deletion is that of Rocksy's own orphaned temporary files
//! (`.<name>.rocksy-tmp`, older than an hour) when a vault is opened.

mod common;

use common::{fingerprint, make_vault, TempDir};
use rocksy_core::{
    add_vault, clean_vault_temp_files, file_tags, list_tree, list_vaults, load_session, open_tree,
    purge_vault_data, read_file, reconcile_tree, refresh_pass, remove_vault, save_file, save_session,
    vault_for_path, BuildOptions, CancelToken, ChangedPaths, ErrorKind, OwnWrites, Session, TagIndex,
};
use std::fs;
use std::time::{Duration, SystemTime};

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
    // The refresh pass of the worker, and the cleanup of orphaned temporary
    // files run at opening: a vault without such files loses nothing.
    refresh_pass(&data, &vault, Some(&first), &ChangedPaths::unknown(), &OwnWrites::new(), &|| false).unwrap();
    assert_eq!(clean_vault_temp_files(&first, SystemTime::now(), &|| false), 0);
    assert_eq!(clean_vault_temp_files(&first, SystemTime::now() + Duration::from_secs(10 * 365 * 86_400), &|| false), 0);

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
    // Removing a vault purges what was derived from it, in the data directory only.
    assert!(data.join("trees").read_dir().unwrap().next().is_some());
    purge_vault_data(&data, &vault).unwrap();
    assert!(data.join("trees").read_dir().unwrap().next().is_none());
    let _ = open_tree(&data, &vault).unwrap();

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

    // The persisted index and the full-text index (which hold text of the
    // notes) are in the data directory only. Both indexes are closed first:
    // on Windows the full-text index keeps its lock file open.
    drop(session);
    drop(index);
    let data_files = fingerprint(&data);
    let names: Vec<&str> = data_files.keys().map(String::as_str).collect();
    assert!(names.iter().all(|n| n.starts_with("tags/") || n.starts_with("search/")), "{names:?}");
    assert_eq!(names.iter().filter(|n| n.ends_with(".tags")).count(), 1);
    assert!(names.iter().any(|n| n.starts_with("search/") && n.ends_with("meta.json")));
}

#[test]
fn opening_a_vault_removes_only_rocksys_old_temporary_files() {
    let dir = TempDir::new("f7-tmp");
    let vault = make_vault(&dir, "vault");
    let age = |path: &std::path::Path, secs: u64| {
        fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(secs)).unwrap();
    };
    // Leftovers of a killed process, next to a note and in a sub-folder...
    let orphan_a = dir.write("vault/.Accueil.md.4242-3.rocksy-tmp", "half-written");
    let orphan_b = dir.write("vault/01 Travail/.Journal.md.4242-9.rocksy-tmp", "half-written");
    age(&orphan_a, 2 * 3600);
    age(&orphan_b, 30 * 86_400);
    // ...and everything that looks close but must stay.
    let recent = dir.write("vault/.Accueil.md.4242-4.rocksy-tmp", "being written right now");
    let in_obsidian = dir.write("vault/.obsidian/.app.json.1-1.rocksy-tmp", "hidden folder: never visited");
    let foreign = dir.write("vault/01 Travail/.~lock.Journal.md#", "another tool");
    let visible = dir.write("vault/01 Travail/notes.rocksy-tmp", "a user file with our suffix but no dot");
    for path in [&in_obsidian, &foreign, &visible] {
        age(path, 30 * 86_400);
    }
    let mut expected = fingerprint(&vault);
    assert!(expected.remove(".Accueil.md.4242-3.rocksy-tmp").is_some());
    assert!(expected.remove("01 Travail/.Journal.md.4242-9.rocksy-tmp").is_some());

    let tree = list_tree(&vault).unwrap();
    assert_eq!(clean_vault_temp_files(&tree, SystemTime::now(), &|| false), 2);

    // Exactly the two orphans are gone; every other byte, name and
    // modification time of a file is as before (the two folders that lost
    // an entry got a new modification time, which is not part of a
    // fingerprint).
    assert_eq!(fingerprint(&vault), expected);
    assert!(recent.exists());
}
