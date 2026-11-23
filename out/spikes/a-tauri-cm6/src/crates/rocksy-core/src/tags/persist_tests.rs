//! Tests of the persisted tag index: round trip, reconcile, damaged files,
//! cancellation.

use super::*;
use crate::testutil::{all_files, TempDir};
use crate::{save_file, ErrorKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Everything observable through the query API.
#[derive(Debug, PartialEq)]
struct Dump {
    tags: Vec<TagCount>,
    fragments: Vec<Vec<Fragment>>,
    stats: TagStats,
}

fn dump(index: &TagIndex) -> Dump {
    let tags = index.tags_list("");
    let fragments = tags.iter().map(|t| index.tag_fragments(&t.tag, None)).collect();
    Dump { tags, fragments, stats: index.stats() }
}

fn assert_consistent(index: &TagIndex) {
    for t in index.tags_list("") {
        assert_eq!(index.tag_fragments(&t.tag, None).len() as u32, t.count, "{}", t.tag);
    }
}

/// What a from-scratch build of the vault gives.
fn fresh_dump(vault: &Path) -> Dump {
    let index = TagIndex::new();
    index.build(vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    dump(&index)
}

fn reconcile(index: &TagIndex, data: &Path, vault: &Path) -> ReconcileReport {
    index.reconcile(data, vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap()
}

/// 9 Markdown files: 7 with tags, 1 without, 1 not UTF-8.
fn sample_vault(dir: &TempDir) -> PathBuf {
    dir.write("v/a.md", "---\ntags: [projet, Projet/Rocksy]\n---\n# A\n\n- todo #projet/rocksy/core\n  - sous #idee\n");
    dir.write("v/b/b1.md", "para #idee et #idee/brillante\n\nautre #misc\n");
    dir.write("v/b/b2.md", "rien ici\n");
    dir.write("v/b/crlf.md", "\u{feff}# Titre #bom\r\n\r\n- un #crlf/élément\r\n  - deux 🎉 #emoji\r\n");
    dir.write("v/c.md", "#projet/autre\n\n> [!tip] #misc\n> #recette/dessert\n");
    dir.write("v/d/long.md", format!("{} #loin fin\n", "mot ".repeat(200)));
    dir.write("v/d/table.md", "| a | b |\n|---|---|\n| x #cellule | y |\n");
    dir.write("v/e.md", "---\ntags:\n  - été\n  - \"#quoté\"\n---\ncorps\n");
    dir.write("v/latin1.md", [b'#', b'x', b' ', 0xE9, b'\n']);
    dir.write("v/.obsidian/workspace.md", "#cache\n");
    dir.write("v/notes.txt", "#texte\n");
    dir.path().join("v")
}

fn persisted_file(data: &Path, vault: &Path) -> PathBuf {
    tags_index_path(data, vault.to_str().unwrap())
}

fn mtime_ns(path: &Path) -> u128 {
    fs::metadata(path).unwrap().modified().unwrap().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
}

fn set_mtime(path: &Path, time: SystemTime) {
    fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(time).unwrap();
}

// ---------------------------------------------------------------------------

#[test]
fn round_trip_restores_exactly_the_same_index() {
    let dir = TempDir::new("persist-rt");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");

    let built = TagIndex::new();
    built.build(&vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    assert!(built.is_dirty(), "a build is not on disk yet");
    assert!(built.save(&data).unwrap());
    assert!(!built.is_dirty());
    assert!(!built.save(&data).unwrap(), "nothing to save twice");
    assert_eq!(all_files(&data).len(), 1);
    assert!(all_files(&data)[0].starts_with("tags/") && all_files(&data)[0].ends_with(".tags"));

    let loaded = TagIndex::new();
    let ready = loaded.load_persisted(&data, &vault).unwrap();
    assert_eq!(ready.files, 9, "files without tags and unreadable files are remembered");
    assert_eq!(ready.occurrences, built.stats().occurrences);
    assert_eq!(ready.tags, built.stats().tags);
    assert_eq!(dump(&loaded), dump(&built));
    assert_eq!(loaded.stats().skipped, 1);
    assert_eq!(loaded.root(), built.root());
    assert_eq!(loaded.root().as_deref(), vault.to_str());
    assert!(!loaded.is_dirty());
    assert_consistent(&loaded);

    // Filtering and limits behave the same on a loaded index.
    assert_eq!(loaded.tags_list("ro"), built.tags_list("ro"));
    assert_eq!(loaded.tag_fragments("projet", Some(2)), built.tag_fragments("projet", Some(2)));
    // Saving a loaded index writes the very same bytes.
    let before = fs::read(persisted_file(&data, &vault)).unwrap();
    fs::remove_file(persisted_file(&data, &vault)).unwrap();
    assert!(loaded.save(&data).unwrap(), "missing file is written again");
    assert_eq!(fs::read(persisted_file(&data, &vault)).unwrap(), before);
}

#[test]
fn positions_survive_the_round_trip_with_crlf_and_bom() {
    let dir = TempDir::new("persist-pos");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let loaded = TagIndex::new();
    loaded.load_persisted(&data, &vault).unwrap();

    let path = vault.join("b").join("crlf.md");
    let text = fs::read_to_string(&path).unwrap();
    let expected = file_tags(path.to_str().unwrap(), &text);
    assert_eq!(expected.len(), 3);
    // BOM counts for one unit on line 0; the emoji for two.
    assert_eq!((expected[0].tag.as_str(), expected[0].tag_line, expected[0].tag_col), ("bom", 0, 9));
    assert_eq!((expected[2].tag.as_str(), expected[2].tag_line, expected[2].tag_col), ("emoji", 3, 12));
    assert_eq!((expected[1].start_line, expected[1].end_line), (2, 3));
    for f in &expected {
        let got = loaded.tag_fragments(&f.tag, None);
        assert_eq!(got, vec![f.clone()], "{}", f.tag);
    }
    // Frontmatter source and the long excerpt come back identical too.
    let e = loaded.tag_fragments("quoté", None);
    assert_eq!((e[0].source, e[0].start_line, e[0].end_line, e[0].tag_line, e[0].tag_col), (TagSource::Frontmatter, 0, 5, 3, 5));
    let far = &loaded.tag_fragments("loin", None)[0];
    assert!(far.excerpt.starts_with('…') && far.excerpt.contains("#loin fin"));
}

#[test]
fn reconcile_builds_then_reads_nothing_when_nothing_changed() {
    let dir = TempDir::new("persist-nochange");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let vault_before = all_files(&vault);

    // Empty index: full build, persisted.
    let index = TagIndex::new();
    let events = Mutex::new(Vec::new());
    let first = index
        .reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |p| events.lock().unwrap().push(p))
        .unwrap()
        .unwrap();
    assert_eq!((first.files, first.reread, first.removed, first.unchanged, first.persisted), (9, 9, 0, 0, true));
    assert_eq!(first.occurrences, index.stats().occurrences);
    assert_eq!(first.tags, index.stats().tags);
    let events = events.into_inner().unwrap();
    assert_eq!(events.first(), Some(&TagsProgress { done: 0, total: 9 }));
    assert_eq!(events.last(), Some(&TagsProgress { done: 9, total: 9 }));
    assert_eq!(dump(&index), fresh_dump(&vault));

    let file = persisted_file(&data, &vault);
    let (bytes, stamp) = (fs::read(&file).unwrap(), mtime_ns(&file));

    // Same index again: nothing read, nothing written.
    let events = Mutex::new(Vec::new());
    let second = index
        .reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |p| events.lock().unwrap().push(p))
        .unwrap()
        .unwrap();
    assert_eq!((second.files, second.reread, second.removed, second.unchanged, second.persisted), (9, 0, 0, 9, false));
    assert!(events.into_inner().unwrap().iter().all(|p| p.total == 0 && p.done == 0));

    // New session: load, reconcile, still nothing read nor written.
    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();
    let third = reconcile(&session, &data, &vault);
    assert_eq!((third.reread, third.removed, third.unchanged, third.persisted), (0, 0, 9, false));
    assert_eq!(dump(&session), dump(&index));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(mtime_ns(&file), stamp, "persisted file not rewritten");

    // The unreadable file is not read again either, and nothing landed in the vault.
    assert_eq!(session.stats().skipped, 1);
    assert_eq!(all_files(&vault), vault_before);
}

#[test]
fn reconcile_reads_only_modified_added_and_renamed_files() {
    let dir = TempDir::new("persist-changes");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);

    // Work from a freshly loaded index, as after a restart.
    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();

    fs::write(vault.join("c.md"), "#projet/autre et #nouveau/tag\n").unwrap(); // modified
    fs::write(vault.join("b/added.md"), "- ajout #misc\n").unwrap(); // added
    fs::remove_file(vault.join("d/table.md")).unwrap(); // removed
    fs::rename(vault.join("e.md"), vault.join("b/e-renamed.md")).unwrap(); // renamed

    let events = Mutex::new(Vec::new());
    let report = session
        .reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |p| events.lock().unwrap().push(p))
        .unwrap()
        .unwrap();
    // Read: c.md, added.md, e-renamed.md. Gone: table.md, e.md.
    assert_eq!((report.files, report.reread, report.removed, report.unchanged, report.persisted), (9, 3, 2, 6, true));
    assert!(events.into_inner().unwrap().iter().all(|p| p.total == 3));
    assert_eq!(dump(&session), fresh_dump(&vault));
    assert!(session.tag_fragments("cellule", None).is_empty());
    assert!(session.tag_fragments("recette", None).is_empty());
    assert!(session.tag_fragments("été", None)[0].path.ends_with("e-renamed.md"));
    assert_consistent(&session);

    // What was persisted is that new state.
    let reloaded = TagIndex::new();
    reloaded.load_persisted(&data, &vault).unwrap();
    assert_eq!(dump(&reloaded), dump(&session));
    assert_eq!(reconcile(&reloaded, &data, &vault).reread, 0);

    // A file emptied of its tags, and a file that becomes unreadable.
    fs::write(vault.join("b/b1.md"), "plus rien\n").unwrap();
    fs::write(vault.join("a.md"), [0xFF, 0xFE, b'#', b'x']).unwrap();
    let report = reconcile(&reloaded, &data, &vault);
    assert_eq!((report.reread, report.removed, report.unchanged), (2, 0, 7));
    assert_eq!(reloaded.stats().skipped, 2);
    assert_eq!(dump(&reloaded), fresh_dump(&vault));
    assert_eq!(reconcile(&reloaded, &data, &vault).reread, 0, "unreadable files are not retried each time");
}

#[test]
fn change_detection_uses_size_and_mtime_only() {
    let dir = TempDir::new("persist-detect");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let target = vault.join("b/b1.md");
    let original_mtime = fs::metadata(&target).unwrap().modified().unwrap();

    // Same size, other mtime: read again.
    set_mtime(&target, original_mtime + Duration::from_secs(10));
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.persisted), (1, true));

    // Same mtime, other size: read again.
    fs::write(&target, "para #idee\n").unwrap();
    set_mtime(&target, original_mtime + Duration::from_secs(10));
    assert_eq!(reconcile(&index, &data, &vault).reread, 1);
    assert_eq!(index.tag_fragments("misc", None).len(), 1);

    // Documented limit: other content with the same size and mtime is not seen.
    fs::write(&target, "para #autr\n").unwrap();
    set_mtime(&target, original_mtime + Duration::from_secs(10));
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.persisted), (0, false));
    assert!(index.tag_fragments("autr", None).is_empty());
    // `build` reads everything and sees it.
    index.build(&vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap();
    assert_eq!(index.tag_fragments("autr", None).len(), 1);
}

#[test]
fn damaged_persisted_file_is_ignored_then_rebuilt() {
    let dir = TempDir::new("persist-bad");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let expected = dump(&index);
    let file = persisted_file(&data, &vault);
    let good = fs::read(&file).unwrap();
    assert!(good.len() > 200);

    let mut other_version = good.clone();
    other_version[8] = other_version[8].wrapping_add(1);
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0x01;
    let mut longer = good.clone();
    longer.extend_from_slice(b"junk");
    let mut cases: Vec<(String, Vec<u8>)> = vec![
        ("empty".into(), Vec::new()),
        ("garbage".into(), b"this is not a tag index at all, not even close".to_vec()),
        ("other version".into(), other_version),
        ("flipped bit".into(), flipped),
        ("trailing bytes".into(), longer),
    ];
    for len in [1, 7, 8, 12, 27, 28, 29, good.len() / 3, good.len() - 1] {
        cases.push((format!("truncated at {len}"), good[..len].to_vec()));
    }

    for (name, bytes) in cases {
        fs::write(&file, &bytes).unwrap();
        // Ignored without error, and an index that already has content keeps it.
        let fresh = TagIndex::new();
        assert_eq!(fresh.load_persisted(&data, &vault), None, "{name}");
        assert!(fresh.tags_list("").is_empty());
        assert_eq!(index.load_persisted(&data, &vault), None, "{name}");
        assert_eq!(dump(&index), expected, "{name}");

        // Reconcile then does a complete, correct rebuild and repairs the file.
        let report = reconcile(&fresh, &data, &vault);
        assert_eq!((report.reread, report.persisted), (9, true), "{name}");
        assert_eq!(dump(&fresh), expected, "{name}");
        assert_eq!(fs::read(&file).unwrap(), good, "{name}");
    }

    // A valid file stored under the name of another root is rejected.
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    fs::copy(&file, persisted_file(&data, &other)).unwrap();
    assert_eq!(TagIndex::new().load_persisted(&data, &other), None);
    // No file at all.
    assert_eq!(TagIndex::new().load_persisted(&dir.path().join("no-data"), &vault), None);
}

#[test]
fn cancelled_reconcile_keeps_a_consistent_index_and_a_loadable_checkpoint() {
    let dir = TempDir::new("persist-cancel");
    for i in 0..300 {
        dir.write(&format!("v/d{}/n{i:03}.md", i % 7), format!("note {i} #t/{} #all\n", i % 5));
    }
    let vault = dir.path().join("v");
    let data = dir.path().join("data");
    let index = TagIndex::new();

    // Cancelled before starting: no effect at all.
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(index.reconcile(&data, &vault, BuildOptions::default(), &cancel, |_| {}).unwrap(), None);
    assert!(index.tags_list("").is_empty());
    assert!(!data.exists());

    // Cancelled midway, from the progress callback.
    let cancel = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let res = index
        .reconcile(&data, &vault, BuildOptions { threads: Some(2) }, &cancel, |_| {
            if calls.fetch_add(1, Ordering::Relaxed) == 2 {
                cancel.cancel();
            }
        })
        .unwrap();
    assert_eq!(res, None);
    // The files already read are kept, and the index is coherent.
    let partial = index.tag_fragments("all", None).len();
    assert!(partial > 0 && partial < 300, "{partial}");
    assert_consistent(&index);
    // What was done is on disk as a complete, loadable checkpoint.
    assert!(!index.is_dirty());
    let checkpoint = TagIndex::new();
    assert_eq!(checkpoint.load_persisted(&data, &vault).unwrap().files, partial);
    assert_eq!(dump(&checkpoint), dump(&index));
    drop(checkpoint);

    // The next reconcile reads only the rest and reaches the full state.
    let report = reconcile(&index, &data, &vault);
    assert_eq!(report.reread, 300 - partial);
    assert_eq!((report.files, report.unchanged, report.persisted), (300, partial, true));
    assert_eq!(dump(&index), fresh_dump(&vault));

    // Partial progress is resumed by another session.
    let other = TagIndex::new();
    let cancel = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let data2 = dir.path().join("data2");
    other
        .reconcile(&data2, &vault, BuildOptions { threads: Some(2) }, &cancel, |_| {
            if calls.fetch_add(1, Ordering::Relaxed) == 3 {
                cancel.cancel();
            }
        })
        .unwrap();
    let partial = other.tag_fragments("all", None).len();
    assert!(!other.save(&data2).unwrap(), "already saved by the cancelled reconcile");
    drop(other);
    let resumed = TagIndex::new();
    assert_eq!(resumed.load_persisted(&data2, &vault).unwrap().files, partial);
    let report = reconcile(&resumed, &data2, &vault);
    assert_eq!(report.reread, 300 - partial);
    assert_eq!(dump(&resumed), fresh_dump(&vault));
}

#[test]
fn saved_file_is_not_read_again_after_update_with_meta() {
    let dir = TempDir::new("persist-update");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    assert!(!index.is_dirty());

    // The application saves a note and tells the index, with the metadata
    // returned by save_file.
    let note = vault.join("c.md");
    let text = "#projet/autre puis #sauvé\n";
    let saved = save_file(&note, text, None).unwrap();
    assert!(index.update_file_with_meta(&note, text, saved.size, saved.mtime_ms));
    assert!(index.is_dirty());
    assert!(index.save(&data).unwrap());
    assert!(!index.save(&data).unwrap());

    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();
    assert_eq!(session.tag_fragments("sauvé", None).len(), 1);
    assert!(session.tag_fragments("recette", None).is_empty());
    let report = reconcile(&session, &data, &vault);
    assert_eq!((report.reread, report.removed, report.unchanged, report.persisted), (0, 0, 9, false));
    assert_eq!(dump(&session), fresh_dump(&vault));

    // Old signature, after a save: the file is stat'ed, same result.
    let text2 = "#projet/autre puis #encore\n";
    save_file(&note, text2, None).unwrap();
    assert!(session.update_file(&note, text2));
    assert_eq!(reconcile(&session, &data, &vault).reread, 0);
    assert_eq!(session.tag_fragments("encore", None).len(), 1);

    // Old signature with text that is not what is on disk (unsaved edit):
    // the next reconcile reads the file and the disk wins.
    assert!(session.update_file(&note, "#brouillon pas encore enregistré\n"));
    assert_eq!(session.tag_fragments("brouillon", None).len(), 1);
    let report = reconcile(&session, &data, &vault);
    assert_eq!(report.reread, 1);
    assert!(session.tag_fragments("brouillon", None).is_empty());
    assert_eq!(session.tag_fragments("encore", None).len(), 1);

    // A new file created by the application, and a deletion.
    let created = vault.join("b/created.md");
    let saved = save_file(&created, "- #créé\n", None).unwrap();
    assert!(session.update_file_with_meta(&created, "- #créé\n", saved.size, saved.mtime_ms));
    fs::remove_file(vault.join("d/table.md")).unwrap();
    session.remove_file(&vault.join("d/table.md"));
    assert!(session.save(&data).unwrap());
    let after = TagIndex::new();
    assert_eq!(after.load_persisted(&data, &vault).unwrap().files, 9);
    let report = reconcile(&after, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (0, 0, false));
    assert_eq!(dump(&after), fresh_dump(&vault));
}

#[test]
fn missing_vault_is_a_clean_error_and_keeps_everything() {
    let dir = TempDir::new("persist-gone");
    let vault = sample_vault(&dir);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let expected = dump(&index);
    let file = persisted_file(&data, &vault);
    let bytes = fs::read(&file).unwrap();

    // The drive is not mounted.
    fs::rename(&vault, dir.path().join("moved")).unwrap();
    let err = index.reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    assert_eq!(dump(&index), expected, "index kept");
    assert_eq!(fs::read(&file).unwrap(), bytes, "persisted file kept");

    // Tags are still available at the next start, from the file alone.
    let session = TagIndex::new();
    assert!(session.load_persisted(&data, &vault).is_some());
    assert_eq!(dump(&session), expected);
    assert!(session.reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {}).is_err());
    assert_eq!(dump(&session), expected);

    // Back again: nothing to read.
    fs::rename(dir.path().join("moved"), &vault).unwrap();
    assert_eq!(reconcile(&session, &data, &vault).reread, 0);
}

#[test]
fn updates_during_a_reconcile_are_replayed() {
    let dir = TempDir::new("persist-race");
    for i in 0..200 {
        dir.write(&format!("v/n{i:03}.md"), format!("#base/{}\n", i % 3));
    }
    let vault = dir.path().join("v");
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);

    // Touch every file so the reconcile has work, then save one note and
    // delete another while it runs.
    for i in 0..200 {
        fs::write(vault.join(format!("n{i:03}.md")), format!("#base/{} modifié\n", i % 3)).unwrap();
    }
    let target = vault.join("n005.md");
    let fired = AtomicUsize::new(0);
    let report = index
        .reconcile(&data, &vault, BuildOptions { threads: Some(2) }, &CancelToken::new(), |p| {
            if p.done > 0 && fired.fetch_add(1, Ordering::Relaxed) == 0 {
                // Queries are answered by the previous content meanwhile.
                assert_eq!(index.tags_list("base")[0].count, 200);
                index.update_file_with_meta(&target, "#saved/during\n", 14, 1);
                index.remove_file(&vault.join("n006.md"));
            }
        })
        .unwrap()
        .unwrap();
    assert_eq!(report.reread, 200);
    assert_eq!(index.tags_list("saved").len(), 2);
    assert_eq!(index.tags_list("base")[0].count, 198);
    assert_eq!(report.occurrences, 199);
    assert!(report.persisted);
    // The persisted file holds the replayed state.
    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();
    assert_eq!(dump(&session), dump(&index));
}

#[test]
fn clear_forgets_the_vault_and_another_vault_gets_its_own_file() {
    let dir = TempDir::new("persist-roots");
    let vault = sample_vault(&dir);
    let other = dir.path().join("other");
    dir.write("other/x.md", "#ailleurs\n");
    let data = dir.path().join("data");
    let index = TagIndex::new();
    assert_eq!(index.root(), None);
    assert!(!index.is_dirty());
    assert!(!index.save(&data).unwrap(), "nothing to save without a vault");

    reconcile(&index, &data, &vault);
    assert_eq!(index.root().as_deref(), vault.to_str());
    let first = fs::read(persisted_file(&data, &vault)).unwrap();

    // Reconciling another vault replaces the content: full build of that vault.
    let report = reconcile(&index, &data, &other);
    assert_eq!((report.files, report.reread, report.removed, report.persisted), (1, 1, 0, true));
    assert_eq!(index.root().as_deref(), other.to_str());
    assert_eq!(index.tags_list("").len(), 1);
    let tag_files = |data: &Path| all_files(data).into_iter().filter(|f| f.starts_with("tags/")).count();
    assert_eq!(tag_files(&data), 2);
    assert_eq!(fs::read(persisted_file(&data, &vault)).unwrap(), first, "the first vault's file is untouched");

    index.clear();
    assert_eq!(index.root(), None);
    assert!(index.tags_list("").is_empty());
    assert!(!index.save(&data).unwrap());
    assert_eq!(tag_files(&data), 2, "clear does not delete persisted files");
    // Without a vault, loose updates are accepted but never persisted.
    assert!(index.update_file(&dir.path().join("loose.md"), "#x"));
    assert!(!index.save(&data).unwrap());

    // And the first vault loads back as it was.
    assert!(index.load_persisted(&data, &vault).is_some());
    assert_eq!(dump(&index), fresh_dump(&vault));
}

#[test]
fn reconcile_report_json_is_camel_case() {
    let report = ReconcileReport { files: 1, tags: 2, occurrences: 3, ms: 4, reread: 5, removed: 6, unchanged: 7, persisted: true };
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::json!({"files": 1, "tags": 2, "occurrences": 3, "ms": 4, "reread": 5, "removed": 6, "unchanged": 7, "persisted": true})
    );
}
