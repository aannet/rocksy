//! Robustness and cost of the index engine: damaged full-text index,
//! deferred commit, spaced-out persistence, link graph reuse, cancellation,
//! passes that do not wait for each other, one listing for tree and index.

mod common;

use common::TempDir;
use rocksy_core::{
    list_tree, load_tree_snapshot, open_tree, reconcile_tree, refresh_tree, save_file, tags_index_path,
    update_snapshot_entry, BuildOptions, CancelToken, ErrorKind, ReconcileOutcome, ReconcileReport, TagIndex,
    MAX_RESULTS, SEARCH_COMMIT_DELAY, TAGS_SAVE_DELAY,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn reconcile(index: &TagIndex, data: &Path, vault: &Path) -> ReconcileReport {
    index.reconcile(data, vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap()
}

/// Copies a folder as it is right now. Used on the data directory of a
/// live index to get what a killed process would leave behind; the index
/// may delete a file of its own meanwhile (segments merged in the
/// background), which is then simply not part of the copy.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_dir(&entry.path(), &target),
            Ok(_) => match fs::copy(entry.path(), &target) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let _ = fs::remove_file(&target);
                }
                Err(e) => panic!("copy of {}: {e}", entry.path().display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{}: {e}", entry.path().display()),
        }
    }
}

/// Names of the notes found, sorted.
fn found(index: &TagIndex, query: &str) -> Vec<String> {
    let mut names: Vec<String> = index
        .search(query, Some(MAX_RESULTS))
        .hits
        .iter()
        .map(|h| Path::new(&h.path).file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A vault of `n` notes; note `i` holds the word `mot{i}`, a common word,
/// a tag and a link to the next one.
fn numbered_vault(dir: &TempDir, n: usize) -> PathBuf {
    for i in 0..n {
        let filler: String = (0..40).map(|k| format!("terme{} ", (i * 7 + k * 13) % 97)).collect();
        dir.write(
            &format!("v/d{}/n{i}.md", i % 5),
            format!("mot{i} commun {filler}\n\n#sujet/s{} voir [[n{}]]\n", i % 3, (i + 1) % n),
        );
    }
    dir.path().join("v")
}

/// Names and sizes of every file below `dir`.
fn listing(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => stack.push(entry.path()),
                Ok(meta) => out.push((entry.path(), meta.len())),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}

/// What a process killed now would leave in `data`: a copy taken while
/// nothing moved (the index merges segments in the background for a moment
/// after a commit; a copy that straddles a merge would mix two states,
/// which a kill cannot do).
fn killed_copy(data: &Path, to: &Path) {
    for _ in 0..200 {
        let before = listing(data);
        let _ = fs::remove_dir_all(to);
        copy_dir(data, to);
        if listing(data) == before {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the data directory never settled");
}

// ---------------------------------------------------------------------------
// BUG-3 / SEC-12: damaged full-text index
// ---------------------------------------------------------------------------

/// Damages each file of a committed full-text index in turn (truncated,
/// a byte flipped in the middle, tail zeroed) and opens the vault the way
/// the application does. tantivy panics on some of these; no panic may
/// leave the index, and after the reconcile the search answers as before.
#[test]
fn damaged_search_index_never_panics_and_is_rebuilt() {
    let dir = TempDir::new("damaged-search");
    let vault = numbered_vault(&dir, 120);
    let data = dir.path().join("data");
    let queries = ["mot1 commun", "mot17", "commun", "\"mot3 commun\"", "tag:sujet/s1", "terme5 terme18"];
    let healthy: Vec<Vec<String>> = {
        let index = TagIndex::new();
        reconcile(&index, &data, &vault);
        queries.iter().map(|q| found(&index, q)).collect()
    };
    assert_eq!(healthy[0], ["n1.md"]);
    assert_eq!(healthy[2].len(), 120);

    let search_root = data.join("search");
    let index_dir = fs::read_dir(&search_root).unwrap().next().unwrap().unwrap().path();
    let backup = dir.path().join("backup");
    copy_dir(&index_dir, &backup);
    let files: Vec<PathBuf> = fs::read_dir(&backup)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && !p.to_string_lossy().ends_with(".lock"))
        .collect();
    assert!(files.len() > 5, "segment files expected, found {files:?}");

    let (mut cases, mut rebuilt, mut failed_in_use) = (0, 0, 0);
    for file in &files {
        let name = file.file_name().unwrap().to_owned();
        let bytes = fs::read(file).unwrap();
        if bytes.len() < 4 {
            continue;
        }
        for mode in 0..3 {
            let _ = fs::remove_dir_all(&index_dir);
            copy_dir(&backup, &index_dir);
            let mut damaged = bytes.clone();
            match mode {
                0 => damaged.truncate(bytes.len() / 2),
                1 => damaged[bytes.len() / 2] ^= 0xFF,
                _ => damaged[bytes.len() * 3 / 4..].fill(0),
            }
            fs::write(index_dir.join(&name), &damaged).unwrap();
            cases += 1;
            let label = format!("{} mode {mode}", name.to_string_lossy());

            let outcome = std::panic::catch_unwind(|| {
                let index = TagIndex::new();
                assert!(index.load_persisted(&data, &vault).is_some());
                index.open_search(&data, &vault);
                // Whatever the state of the files, a search answers.
                for q in queries {
                    let result = index.search(q, None);
                    assert!(result.hits.len() <= result.total || result.total == 0);
                }
                let failed = index.search_needs_rebuild();
                if failed {
                    assert!(!index.search_ready(), "a failed index does not claim to be ready");
                    assert_eq!(index.search("commun", None).total, 0);
                }
                let report = reconcile(&index, &data, &vault);
                assert!(index.search_ready());
                assert!(!index.search_needs_rebuild());
                let answers: Vec<Vec<String>> = queries.iter().map(|q| found(&index, q)).collect();
                (failed, report.reread, answers)
            });
            let (failed, reread, answers) = outcome.unwrap_or_else(|_| panic!("a panic escaped the index: {label}"));
            assert_eq!(answers, healthy, "{label}");
            failed_in_use += usize::from(failed);
            if reread > 0 {
                assert_eq!(reread, 120, "a rebuilt index reads every note again: {label}");
                rebuilt += 1;
            }
        }
    }
    println!("damaged index: {cases} cases, {rebuilt} rebuilt, {failed_in_use} failed while searching");
    assert!(rebuilt > 0, "no damage was detected at all: the test does not test anything");

    // The repaired index is durable: a new session needs nothing.
    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();
    assert!(session.open_search(&data, &vault));
    assert_eq!(reconcile(&session, &data, &vault).reread, 0);
}

// ---------------------------------------------------------------------------
// PERF-3: the save does not wait for the full-text commit
// ---------------------------------------------------------------------------

/// The process dies after saves whose full-text commit is still pending
/// (the data directory is copied as it is at that moment). Whatever had
/// been written of the two stores, the next launch reads the saved notes
/// again, once, and both stores are right.
#[test]
fn a_kill_before_the_deferred_commit_costs_one_read_per_saved_note() {
    let dir = TempDir::new("kill-before-commit");
    let vault = numbered_vault(&dir, 30);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);

    let saved: Vec<PathBuf> = [3, 11, 24].iter().map(|i| vault.join(format!("d{}/n{i}.md", i % 5))).collect();
    let before = Instant::now();
    for (k, note) in saved.iter().enumerate() {
        let text = format!("texte neuf avec un okapi{k} #neuf/t{k}\n");
        let result = save_file(note, &text, None).unwrap();
        assert!(index.update_file_with_meta(note, &text, result.size, result.mtime_ms));
    }
    assert!(index.search_commit_pending());
    assert!(!index.commit_search_if_due(before));

    // Kill 1: nothing written since the saves (tags not persisted, search
    // not committed).
    let kill1 = dir.path().join("kill1");
    killed_copy(&data, &kill1);
    // Kill 2: tags persisted (the 60 s rule fired), search still not
    // committed: the tags file says the notes are known, the search index
    // does not hold them.
    assert!(index.save_if_due(&data, Instant::now() + TAGS_SAVE_DELAY).unwrap());
    assert!(index.search_commit_pending(), "persisting tags does not commit the search");
    let kill2 = dir.path().join("kill2");
    killed_copy(&data, &kill2);
    // Clean exit: `save` commits the search too.
    let _ = index.save(&data);
    assert!(!index.search_commit_pending());
    drop(index);

    for (label, data, expected_reread) in [("kill1", &kill1, 3), ("kill2", &kill2, 3), ("clean exit", &data, 0)] {
        let session = TagIndex::new();
        assert!(session.load_persisted(data, &vault).is_some(), "{label}");
        assert!(session.open_search(data, &vault), "{label}");
        if expected_reread > 0 {
            assert!(found(&session, "okapi0").is_empty(), "{label}: the commit never happened");
        }
        let report = reconcile(&session, data, &vault);
        assert_eq!((report.reread, report.removed), (expected_reread, 0), "{label}");
        for k in 0..3 {
            assert_eq!(found(&session, &format!("okapi{k}")).len(), 1, "{label}");
            assert_eq!(session.tag_fragments(&format!("neuf/t{k}"), None).len(), 1, "{label}");
        }
        assert!(found(&session, "mot3").is_empty(), "{label}: old text gone");
        assert_eq!(found(&session, "commun").len(), 27, "{label}");
        // And the repair is durable.
        drop(session);
        let again = TagIndex::new();
        again.load_persisted(data, &vault).unwrap();
        again.open_search(data, &vault);
        assert_eq!(reconcile(&again, data, &vault).reread, 0, "{label}");
    }
}

/// Changes that wait for a commit are not lost when the engine is let go
/// (another vault opened, vault closed).
#[test]
fn pending_search_changes_are_committed_when_the_engine_is_released() {
    let dir = TempDir::new("release-engine");
    let vault = numbered_vault(&dir, 12);
    dir.write("other/x.md", "autre vault\n");
    let other = dir.path().join("other");
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let note = vault.join("d2/n2.md");
    let text = "un tamanoir passe\n";
    let saved = save_file(&note, text, None).unwrap();
    index.update_file_with_meta(&note, text, saved.size, saved.mtime_ms);
    assert!(index.search_commit_pending());

    // The application switches vault without having ticked.
    index.save(&data).unwrap();
    reconcile(&index, &data, &other);
    drop(index);

    let session = TagIndex::new();
    session.load_persisted(&data, &vault).unwrap();
    assert!(session.open_search(&data, &vault));
    assert_eq!(found(&session, "tamanoir"), ["n2.md"]);
    assert_eq!(reconcile(&session, &data, &vault).reread, 0);

    // Same with `clear` alone (tags not saved: only the search is kept, and
    // the note is read again for the tags).
    let text = "un pangolin passe #pangolin\n";
    let saved = save_file(&note, text, None).unwrap();
    session.update_file_with_meta(&note, text, saved.size, saved.mtime_ms);
    session.clear();
    drop(session);
    let last = TagIndex::new();
    last.load_persisted(&data, &vault).unwrap();
    assert!(last.open_search(&data, &vault));
    assert_eq!(found(&last, "pangolin"), ["n2.md"]);
    assert_eq!(reconcile(&last, &data, &vault).reread, 1);
    assert_eq!(last.tag_fragments("pangolin", None).len(), 1);
}

/// A caller whose timer never runs still gets fresh results: a search
/// commits what has waited longer than the delay.
#[test]
fn search_commits_overdue_changes_itself() {
    let dir = TempDir::new("overdue");
    let vault = numbered_vault(&dir, 6);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let note = vault.join("d1/n1.md");
    let text = "un fourmilier\n";
    let saved = save_file(&note, text, None).unwrap();
    index.update_file_with_meta(&note, text, saved.size, saved.mtime_ms);
    std::thread::sleep(SEARCH_COMMIT_DELAY + Duration::from_millis(50));
    assert_eq!(found(&index, "fourmilier"), ["n1.md"]);
    assert!(!index.search_commit_pending());
}

// ---------------------------------------------------------------------------
// PERF-4: tags and links persisted once a minute at most while editing
// ---------------------------------------------------------------------------

#[test]
fn tags_are_persisted_at_most_once_per_delay_and_a_lost_write_costs_one_read() {
    let dir = TempDir::new("save-if-due");
    let vault = numbered_vault(&dir, 20);
    let data = dir.path().join("data");
    let file = tags_index_path(&data, vault.to_str().unwrap());
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let on_disk = fs::read(&file).unwrap();
    assert!(!index.save_if_due(&data, Instant::now() + TAGS_SAVE_DELAY * 10).unwrap(), "nothing to save");

    let save = |i: usize, word: &str| {
        let note = vault.join(format!("d{}/n{i}.md", i % 5));
        let text = format!("{word} #edit/{word}\n");
        let saved = save_file(&note, &text, None).unwrap();
        assert!(index.update_file_with_meta(&note, &text, saved.size, saved.mtime_ms));
    };
    let t0 = Instant::now();
    save(1, "un");
    save(2, "deux");
    save(3, "trois");
    let t1 = Instant::now();
    assert!(index.is_dirty());
    // Not before the delay has run since the first change...
    assert!(!index.save_if_due(&data, t0).unwrap());
    assert!(!index.save_if_due(&data, t0 + TAGS_SAVE_DELAY - Duration::from_secs(1)).unwrap());
    // ...and a pass that reads nothing does not write either.
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (0, 0, false));
    assert_eq!(fs::read(&file).unwrap(), on_disk, "file untouched while the delay runs");

    // Killed now: the three notes are read again, nothing else.
    let killed = dir.path().join("killed");
    killed_copy(&data, &killed);
    let session = TagIndex::new();
    session.load_persisted(&killed, &vault).unwrap();
    assert!(session.tag_fragments("edit/deux", None).is_empty(), "the write was lost");
    let report = reconcile(&session, &killed, &vault);
    assert_eq!((report.reread, report.unchanged), (3, 17));
    assert_eq!(session.tag_fragments("edit/deux", None).len(), 1);
    drop(session);

    // Due: written once, then quiet again for a whole delay.
    assert!(index.save_if_due(&data, t1 + TAGS_SAVE_DELAY).unwrap());
    assert!(!index.is_dirty());
    assert_ne!(fs::read(&file).unwrap(), on_disk);
    let t2 = Instant::now();
    save(4, "quatre");
    let t3 = Instant::now();
    assert!(!index.save_if_due(&data, t2 + TAGS_SAVE_DELAY - Duration::from_secs(1)).unwrap());
    let report = index.housekeeping(&data, t3 + TAGS_SAVE_DELAY);
    assert!(report.tags_saved && report.search_committed);
    let quiet = index.housekeeping(&data, t3 + TAGS_SAVE_DELAY * 3);
    assert!(!quiet.tags_saved && !quiet.search_committed);

    // A pass that does read a note persists at once.
    fs::write(vault.join("d0/n5.md"), "changé dehors #dehors\n").unwrap();
    save(6, "six");
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.persisted), (1, true));
    assert!(!index.is_dirty());
}

// ---------------------------------------------------------------------------
// PERF-7: the link graph is rebuilt only when links or files change
// ---------------------------------------------------------------------------

#[test]
fn link_graph_is_reused_when_a_save_leaves_the_links_alone() {
    let dir = TempDir::new("graph-reuse");
    dir.write("v/A.md", "# A\n\nvers [[B]] et [[C]]\n");
    dir.write("v/B.md", "retour [[A]]\n");
    dir.write("v/C.md", "rien\n");
    dir.write("v/D.md", "rien non plus\n");
    let vault = dir.path().join("v");
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let (a, b, c, d) = (vault.join("A.md"), vault.join("B.md"), vault.join("C.md"), vault.join("D.md"));
    // What a from-scratch index of the disk answers.
    let truth = |path: &Path| {
        let fresh = TagIndex::new();
        fresh.build(&vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
        (fresh.backlinks(path), fresh.outgoing_links(path), fresh.unresolved_links(None))
    };
    let answers = |path: &Path| (index.backlinks(path), index.outgoing_links(path), index.unresolved_links(None));
    let save = |path: &Path, text: &str| {
        let saved = save_file(path, text, None).unwrap();
        assert!(index.update_file_with_meta(path, text, saved.size, saved.mtime_ms));
    };

    assert_eq!(index.backlinks(&b).len(), 1);
    let builds = index.graph_builds();
    let generation = index.links_generation();
    assert_eq!(index.backlinks(&c).len(), 1);
    assert_eq!(index.graph_builds(), builds, "cached between two queries");

    // 1. Text changes, links untouched: no rebuild, same generation.
    save(&a, "# A\n\nvers [[B]] et [[C]]\nune phrase de plus, un #tag aussi\n");
    for path in [&a, &b, &c, &d] {
        assert_eq!(answers(path), truth(path));
    }
    assert_eq!(index.graph_builds(), builds, "a save without link change must not rebuild the graph");
    assert_eq!(index.links_generation(), generation);

    // 2. Links move to other lines: the graph still holds, the positions
    // shown are the new ones, and the generation says so.
    save(&a, "# A\n\nune ligne avant\n\nvers [[B]] et [[C]]\n");
    assert_eq!(index.backlinks(&b)[0].line, 4);
    for path in [&a, &b, &c, &d] {
        assert_eq!(answers(path), truth(path));
    }
    assert_eq!(index.graph_builds(), builds);
    let moved = index.links_generation();
    assert_ne!(moved, generation);

    // 3. One link changes: rebuilt, once.
    save(&a, "# A\n\nune ligne avant\n\nvers [[B]] et [[D]] et [[Inconnu]]\n");
    assert!(index.backlinks(&c).is_empty());
    assert_eq!(index.backlinks(&d).len(), 1);
    assert_eq!(index.unresolved_links(None).len(), 1);
    for path in [&a, &b, &c, &d] {
        assert_eq!(answers(path), truth(path));
    }
    assert_eq!(index.graph_builds(), builds + 1);
    assert_ne!(index.links_generation(), moved);

    // 4. A file appears (it resolves a link) or disappears: rebuilt.
    let generation = index.links_generation();
    save(&vault.join("Inconnu.md"), "me voilà\n");
    assert!(index.unresolved_links(None).is_empty());
    assert_eq!(index.graph_builds(), builds + 2);
    assert_ne!(index.links_generation(), generation);
    fs::remove_file(&d).unwrap();
    index.remove_file(&d);
    assert_eq!(index.unresolved_links(None).len(), 1);
    assert_eq!(index.graph_builds(), builds + 3);

    // 5. A reconcile that finds nothing new keeps graph and generation.
    let generation = index.links_generation();
    assert_eq!(reconcile(&index, &data, &vault).reread, 0);
    let _ = index.backlinks(&b);
    assert_eq!(index.graph_builds(), builds + 3);
    assert_eq!(index.links_generation(), generation);
    // An attachment appears: resolution may change.
    fs::write(vault.join("image.png"), [1u8]).unwrap();
    reconcile(&index, &data, &vault);
    let _ = index.backlinks(&b);
    assert_eq!(index.graph_builds(), builds + 4);
    assert_ne!(index.links_generation(), generation);
}

// ---------------------------------------------------------------------------
// BUG-13 / PERF-2: tree first, index next, and no waiting behind a long pass
// ---------------------------------------------------------------------------

#[test]
fn a_refresh_does_not_wait_behind_a_running_pass() {
    let dir = TempDir::new("busy");
    let vault = numbered_vault(&dir, 10);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    open_tree(&data, &vault).unwrap();

    // A first indexing, held in the middle (as if the drive were slow).
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let release_rx = std::sync::Mutex::new(release_rx);
            let entered_tx = std::sync::Mutex::new(entered_tx);
            let once = std::sync::atomic::AtomicBool::new(false);
            index
                .reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {
                    if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        entered_tx.lock().unwrap().send(()).unwrap();
                        release_rx.lock().unwrap().recv().unwrap();
                    }
                })
                .unwrap()
                .unwrap()
        });
        entered_rx.recv().unwrap();
        assert!(index.is_reconciling());

        // Meanwhile a file appears: the tree is refreshed at once...
        fs::write(vault.join("nouveau.md"), "mot nouveau\n").unwrap();
        let started = Instant::now();
        let refresh = refresh_tree(&data, &vault, None, &|| false).unwrap().unwrap();
        assert!(refresh.changed && refresh.snapshot_saved);
        assert!(refresh.tree.entries.iter().any(|e| e.path == "nouveau.md"));
        // ...and the index pass answers "busy" instead of blocking.
        let outcome = index
            .try_reconcile_with_tree(&data, &refresh.tree, BuildOptions::default(), &CancelToken::new(), |_| {})
            .unwrap();
        assert_eq!(outcome, ReconcileOutcome::Busy);
        assert!(started.elapsed() < Duration::from_secs(2), "the refresh waited for the running pass");

        release_tx.send(()).unwrap();
        let report = first.join().unwrap();
        assert_eq!(report.reread, 10);
    });
    assert!(!index.is_reconciling());
    // The long pass is over: its owner learns that one was skipped, once.
    assert!(index.take_missed_reconcile());
    assert!(!index.take_missed_reconcile());

    // The pass that was skipped, with one listing for tree and index.
    let refresh = refresh_tree(&data, &vault, None, &|| false).unwrap().unwrap();
    assert!(!refresh.changed, "the tree was already refreshed");
    let outcome = index
        .try_reconcile_with_tree(&data, &refresh.tree, BuildOptions::default(), &CancelToken::new(), |_| {})
        .unwrap();
    let ReconcileOutcome::Done(report) = outcome else { panic!("{outcome:?}") };
    assert_eq!((report.reread, report.unchanged), (1, 10));
    assert_eq!(found(&index, "nouveau"), ["nouveau.md"]);
    assert!(!index.take_missed_reconcile());
}

#[test]
fn reconcile_with_a_shared_listing_matches_reconcile() {
    let dir = TempDir::new("shared-listing");
    let vault = numbered_vault(&dir, 15);
    let (data_a, data_b) = (dir.path().join("a"), dir.path().join("b"));
    let (alone, shared) = (TagIndex::new(), TagIndex::new());
    let options = BuildOptions::default();
    let cancel = CancelToken::new();

    let first = reconcile(&alone, &data_a, &vault);
    let refresh = refresh_tree(&data_b, &vault, None, &|| false).unwrap().unwrap();
    assert!(refresh.changed, "no snapshot yet");
    assert_eq!(load_tree_snapshot(&data_b, &vault).unwrap().entries, refresh.tree.entries);
    let report = shared.reconcile_with_tree(&data_b, &refresh.tree, options, &cancel, |_| {}).unwrap().unwrap();
    assert_eq!((report.files, report.tags, report.occurrences, report.reread), (first.files, first.tags, first.occurrences, first.reread));

    // Changes on disk: one listing serves the tree and both stores.
    fs::write(vault.join("d0/n0.md"), "contenu changé avec un lama #lama\n").unwrap();
    fs::remove_file(vault.join("d1/n1.md")).unwrap();
    fs::write(vault.join("ajout.md"), "un ajout\n").unwrap();
    let before = reconcile(&alone, &data_a, &vault);
    // The caller kept the previous tree in memory: no snapshot read.
    let again = refresh_tree(&data_b, &vault, Some(&refresh.tree), &|| false).unwrap().unwrap();
    assert!(again.changed);
    let report = shared.reconcile_with_tree(&data_b, &again.tree, options, &cancel, |_| {}).unwrap().unwrap();
    assert_eq!((report.reread, report.removed, report.unchanged), (before.reread, before.removed, before.unchanged));
    assert_eq!((report.reread, report.removed), (2, 1));
    for query in ["lama", "commun", "mot1", "tag:lama"] {
        assert_eq!(found(&shared, query), found(&alone, query), "{query}");
    }
    assert_eq!(shared.tags_list(""), alone.tags_list(""));
    let same = refresh_tree(&data_b, &vault, Some(&again.tree), &|| false).unwrap().unwrap();
    assert!(!same.changed);

    // A tree read from the snapshot is the past, not a listing: refused,
    // and nothing is removed on its word.
    fs::write(vault.join("plus tard.md"), "créé après l'instantané\n").unwrap();
    reconcile(&shared, &data_b, &vault);
    let stale = load_tree_snapshot(&data_b, &vault).unwrap();
    assert!(stale.from_snapshot);
    let err = shared.reconcile_with_tree(&data_b, &stale, options, &cancel, |_| {}).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid);
    let err = shared.try_reconcile_with_tree(&data_b, &stale, options, &cancel, |_| {}).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid);
    assert_eq!(found(&shared, "instantané"), ["plus tard.md"]);
}

// ---------------------------------------------------------------------------
// BUG-6: closing does not wait for an indexing
// ---------------------------------------------------------------------------

#[test]
fn a_cancelled_pass_removes_nothing_and_leaves_a_state_that_resumes() {
    let dir = TempDir::new("cancel-resume");
    let vault = numbered_vault(&dir, 40);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);

    // Cancelled before or during the listing: nothing is touched, not even
    // the notes that really disappeared.
    fs::remove_file(vault.join("d0/n0.md")).unwrap();
    fs::remove_file(vault.join("d1/n1.md")).unwrap();
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(index.reconcile(&data, &vault, BuildOptions::default(), &cancelled, |_| {}).unwrap().is_none());
    let tree = list_tree(&vault).unwrap();
    assert!(index.reconcile_with_tree(&data, &tree, BuildOptions::default(), &cancelled, |_| {}).unwrap().is_none());
    assert_eq!(index.index_ready(false, 0, None).files, 40, "a cancelled pass removed notes");
    assert_eq!(found(&index, "mot0"), ["n0.md"]);
    assert_eq!(reconcile(&index, &data, &vault).removed, 2);
    drop(index);

    // A first indexing interrupted after a few notes, then the application
    // closes: `save` writes what is in memory.
    let data = dir.path().join("data2");
    let index = TagIndex::new();
    let cancel = CancelToken::new();
    let options = BuildOptions { threads: Some(1) };
    let outcome = index
        .reconcile(&data, &vault, options, &cancel, |p| {
            if p.done >= 16 {
                cancel.cancel();
            }
        })
        .unwrap();
    assert!(outcome.is_none());
    let _ = index.save(&data);
    let known = index.index_ready(false, 0, None).files;
    assert!((16..38).contains(&known), "interrupted part-way: {known}");
    assert!(!index.search_ready(), "an unfinished index does not claim to be complete");
    drop(index);

    // Next launch: resumes where it stopped, and ends up right.
    let session = TagIndex::new();
    assert_eq!(session.load_persisted(&data, &vault).unwrap().files, known);
    assert!(!session.open_search(&data, &vault));
    let report = reconcile(&session, &data, &vault);
    assert_eq!(report.reread, 38 - known);
    assert_eq!(report.unchanged, known);
    assert!(session.search_ready());
    assert_eq!(found(&session, "commun").len(), 38);
    let complete = TagIndex::new();
    complete.build(&vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
    assert_eq!(session.tags_list(""), complete.tags_list(""));
    for tag in ["sujet/s0", "sujet/s1", "sujet"] {
        assert_eq!(session.tag_fragments(tag, None), complete.tag_fragments(tag, None), "{tag}: order after resuming");
    }
}

#[test]
fn save_does_not_wait_for_a_running_pass() {
    let dir = TempDir::new("save-while-indexing");
    let vault = numbered_vault(&dir, 12);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    // An edit that is neither persisted nor committed, then a pass that hangs.
    let note = vault.join("d3/n3.md");
    let text = "un tapir #tapir\n";
    let saved = save_file(&note, text, None).unwrap();
    index.update_file_with_meta(&note, text, saved.size, saved.mtime_ms);
    fs::write(vault.join("d4/n4.md"), "changé dehors\n").unwrap();

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let pass = scope.spawn(|| {
            let release_rx = std::sync::Mutex::new(release_rx);
            let entered_tx = std::sync::Mutex::new(entered_tx);
            let once = std::sync::atomic::AtomicBool::new(false);
            index.reconcile(&data, &vault, BuildOptions::default(), &CancelToken::new(), |_| {
                if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    entered_tx.lock().unwrap().send(()).unwrap();
                    let _ = release_rx.lock().unwrap().recv();
                }
            })
        });
        entered_rx.recv().unwrap();
        // The window closes now.
        let started = Instant::now();
        assert!(index.save(&data).unwrap());
        assert!(started.elapsed() < Duration::from_secs(2), "save waited for the pass: {:?}", started.elapsed());
        // What a process killed right after that `save` leaves behind.
        killed_copy(&data, &dir.path().join("closed"));
        release_tx.send(()).unwrap();
        pass.join().unwrap().unwrap();
    });
    drop(index);

    let closed = dir.path().join("closed");
    let session = TagIndex::new();
    session.load_persisted(&closed, &vault).unwrap();
    assert!(session.open_search(&closed, &vault));
    assert_eq!(session.tag_fragments("tapir", None).len(), 1, "the edit was persisted at close");
    assert_eq!(found(&session, "tapir"), ["n3.md"], "and committed to the search");
    let report = reconcile(&session, &closed, &vault);
    assert_eq!(report.reread, 1, "only the note the interrupted pass had not read yet");
}

// ---------------------------------------------------------------------------
// SEC-15: limits
// ---------------------------------------------------------------------------

#[test]
fn an_absurd_limit_is_capped() {
    let dir = TempDir::new("limits");
    let vault = numbered_vault(&dir, 30);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    for limit in [usize::MAX, usize::MAX / 2, 1 << 40] {
        let result = index.search("commun", Some(limit));
        assert_eq!((result.total, result.hits.len()), (30, 30));
        assert_eq!(index.tag_fragments("sujet", Some(limit)).len(), 30);
    }
    assert_eq!(index.search("commun", Some(7)).hits.len(), 7);
    assert_eq!(index.search("commun", Some(0)).hits.len(), 0);

    // More matches than the cap.
    let big = TempDir::new("limits-big");
    for i in 0..(MAX_RESULTS + 40) {
        big.write(&format!("v/n{i}.md"), "commun #t\n");
    }
    let (vault, data) = (big.path().join("v"), big.path().join("data"));
    let index = TagIndex::new();
    reconcile(&index, &data, &vault);
    let result = index.search("commun", Some(usize::MAX));
    assert_eq!((result.total, result.hits.len()), (MAX_RESULTS + 40, MAX_RESULTS));
    assert_eq!(index.tag_fragments("t", Some(usize::MAX)).len(), MAX_RESULTS);
    assert_eq!(index.tag_fragments("t", None).len(), MAX_RESULTS + 40, "no limit asked: everything");
}

// ---------------------------------------------------------------------------
// Supports for the shell: snapshot entry after a save, `created`
// ---------------------------------------------------------------------------

#[test]
fn a_save_recorded_in_the_snapshot_is_not_reported_by_the_next_listing() {
    let dir = TempDir::new("snapshot-entry");
    let vault = numbered_vault(&dir, 8);
    let data = dir.path().join("data");
    open_tree(&data, &vault).unwrap();
    let note = vault.join("d2/n2.md");

    // Without the update, the next listing sees the application's own save
    // as a change of the tree.
    let saved = save_file(&note, "premier enregistrement\n", None).unwrap();
    assert!(!saved.created);
    assert!(reconcile_tree(&data, &vault, |_| {}).unwrap());

    // With it: nothing to report.
    let saved = save_file(&note, "second enregistrement, plus long\n", None).unwrap();
    assert!(update_snapshot_entry(&data, &vault, &note, saved.size, saved.mtime_ms).unwrap());
    let entry = load_tree_snapshot(&data, &vault).unwrap().entries.into_iter().find(|e| e.path == "d2/n2.md").unwrap();
    assert_eq!((entry.size, entry.mtime_ms), (saved.size, saved.mtime_ms));
    assert!(!reconcile_tree(&data, &vault, |_| panic!("the save was reported as a change")).unwrap());
    // Same values again: nothing rewritten, still true.
    let snapshot_file = rocksy_core::tree_snapshot_path(&data, vault.to_str().unwrap());
    let stamp = fs::metadata(&snapshot_file).unwrap().modified().unwrap();
    assert!(update_snapshot_entry(&data, &vault, &note, saved.size, saved.mtime_ms).unwrap());
    assert_eq!(fs::metadata(&snapshot_file).unwrap().modified().unwrap(), stamp);

    // In memory, for a caller that keeps the tree.
    let mut tree = list_tree(&vault).unwrap();
    let saved = save_file(&note, "troisième\n", None).unwrap();
    assert!(tree.set_file_meta(&note, saved.size, saved.mtime_ms));
    let files = |t: &rocksy_core::Tree| t.entries.iter().filter(|e| !e.is_dir).cloned().collect::<Vec<_>>();
    assert_eq!(files(&tree), files(&list_tree(&vault).unwrap()));
    assert!(!tree.set_file_meta(&vault.join("d2"), 1, 1), "a folder");
    assert!(!tree.set_file_meta(&dir.path().join("ailleurs.md"), 1, 1), "outside the vault");
    assert!(!tree.set_file_meta(&vault, 1, 1), "the root");

    // A note the save created is not in the snapshot: the caller is told,
    // and the real listing reports it.
    let created = vault.join("d2/nouvelle.md");
    let saved = save_file(&created, "toute neuve\n", None).unwrap();
    assert!(saved.created);
    assert!(!update_snapshot_entry(&data, &vault, &created, saved.size, saved.mtime_ms).unwrap());
    assert!(!tree.set_file_meta(&created, saved.size, saved.mtime_ms));
    assert!(reconcile_tree(&data, &vault, |_| {}).unwrap());
    assert!(!save_file(&created, "réécrite\n", Some(saved.mtime_ms)).unwrap().created);
    // No snapshot at all.
    assert!(!update_snapshot_entry(&dir.path().join("vide"), &vault, &note, 1, 1).unwrap());

    // `created` stays out of the JSON of the contract.
    let json = serde_json::to_string(&saved).unwrap();
    assert_eq!(json, format!("{{\"mtimeMs\":{},\"size\":{}}}", saved.mtime_ms, saved.size));
}

#[test]
fn a_note_that_could_not_be_read_is_reported_pending_and_caught_up() {
    let dir = TempDir::new("pending-read");
    let vault = numbered_vault(&dir, 12);
    let data = dir.path().join("data");
    let index = TagIndex::new();
    // The listing sees 12 notes; two cannot be read when their turn comes
    // (held by another program, or gone since: the read fails either way).
    let listing = list_tree(&vault).unwrap();
    let lost: Vec<PathBuf> = listing
        .entries
        .iter()
        .filter(|e| !e.is_dir && e.path.ends_with(".md"))
        .take(2)
        .map(|e| vault.join(&e.path))
        .collect();
    let kept: Vec<Vec<u8>> = lost.iter().map(|p| fs::read(p).unwrap()).collect();
    for path in &lost {
        fs::remove_file(path).unwrap();
    }
    let report = index
        .reconcile_with_tree(&data, &listing, BuildOptions::default(), &CancelToken::new(), |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(report.files, 12);
    assert_eq!(report.pending, 2, "the two notes that were not read are not counted as up to date");
    let ready = index.index_ready(false, 0, Some(&report));
    assert_eq!((ready.search_docs, ready.pending), (10, Some(2)));
    // No pass is implied by an event that does not come from a reconcile.
    assert_eq!(index.index_ready(true, 0, None).pending, None);

    // The files are readable again: the next pass reads exactly those two.
    for (path, bytes) in lost.iter().zip(&kept) {
        fs::write(path, bytes).unwrap();
    }
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.pending), (2, 0));
    assert_eq!(index.index_ready(false, 0, Some(&report)).search_docs, 12);
    // And a pass with nothing to do reports nothing pending.
    assert_eq!(reconcile(&index, &data, &vault).pending, 0);
}
