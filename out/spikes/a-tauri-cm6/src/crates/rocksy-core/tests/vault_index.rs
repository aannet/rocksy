//! The vault index end to end: link graph, full-text search, persistence
//! and the consistency between the two stores.

mod common;

use common::{fingerprint, TempDir};
use rocksy_core::{
    save_file, tags_index_path, BuildOptions, CancelToken, LinkKind, OutLink, ReconcileReport, SearchResult,
    TagIndex, VaultIndex,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn reconcile(index: &TagIndex, data: &Path, vault: &Path) -> ReconcileReport {
    index.reconcile(data, vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap()
}

/// What the application does when a vault opens: tags and links from the
/// persisted file, then the full-text index. Returns the number of notes.
fn load(index: &TagIndex, data: &Path, vault: &Path) -> Option<usize> {
    let ready = index.load_persisted(data, vault)?;
    index.open_search(data, vault);
    Some(ready.files)
}

fn p(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

/// Builds `root/rel` with native separators.
fn at(root: &Path, rel: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    rel.split('/').for_each(|c| path.push(c));
    path
}

fn slice16(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[start as usize..end as usize]).unwrap()
}

/// Lines of a text in CodeMirror's model.
fn cm_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(String::from).collect()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

fn link_vault(dir: &TempDir) -> PathBuf {
    dir.write(
        "v/Accueil.md",
        "# Accueil\n\n[[Projet]] et [[projets/Projet#Plan|le plan]] puis [[2026/Projet]]\n\
         [[Inconnu]] deux fois [[Inconnu]]\n![[schema.png]] ![[plan.pdf]]\n[doc](docs/Guide.md) [[nulle/part/Guide]]\n[[Accueil]] [[GUIDE]]\n",
    );
    dir.write("v/projets/Projet.md", "# Projet\n\nretour [[Accueil]]\n");
    dir.write("v/projets/2026/Projet.md", "projet 2026, voir [[Autre inconnu]]\n");
    dir.write("v/archive/Projet.md", "vieux projet\n");
    dir.write(
        "v/docs/Guide.md",
        "[retour](../Accueil.md) [espace](Mon%20fichier.md) [racine](projets/Projet.md)\n\
         [hors](../../dehors.md) ![fig](../assets/schema.png) [sans ext](Mon%20fichier)\n[[Inconnu]]\n",
    );
    dir.write("v/docs/Mon fichier.md", "rien\n");
    dir.write("v/assets/schema.png", [1u8, 2, 3]);
    dir.write("v/assets/sub/schema.png", [4u8]);
    dir.write("v/docs/plan.pdf", [5u8]);
    dir.write("v/.obsidian/Inconnu.md", "caché\n");
    dir.path().join("v")
}

fn resolved(links: &[OutLink], root: &Path) -> Vec<(String, Option<String>)> {
    let prefix = p(root).len() + 1;
    links.iter().map(|l| (l.target.clone(), l.resolved.as_ref().map(|r| r[prefix..].replace('\\', "/")))).collect()
}

fn r(target: &str, to: Option<&str>) -> (String, Option<String>) {
    (target.to_string(), to.map(String::from))
}

#[test]
fn link_resolution_follows_the_contract() {
    let dir = TempDir::new("links-resolve");
    let vault = link_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);

    let out = index.outgoing_links(&vault.join("Accueil.md"));
    assert_eq!(
        resolved(&out, &vault),
        [
            // Name only, three candidates: shortest path, then alphabetical.
            r("Projet", Some("archive/Projet.md")),
            // Exact path from the root.
            r("projets/Projet", Some("projets/Projet.md")),
            // Path suffix.
            r("2026/Projet", Some("projets/2026/Projet.md")),
            r("Inconnu", None),
            r("Inconnu", None),
            // Attachments: shortest path wins.
            r("schema.png", Some("assets/schema.png")),
            r("plan.pdf", Some("docs/plan.pdf")),
            // Markdown link from the root (the note is at the root).
            r("docs/Guide.md", Some("docs/Guide.md")),
            // A path that matches nothing falls back to the file name.
            r("nulle/part/Guide", Some("docs/Guide.md")),
            r("Accueil", Some("Accueil.md")),
            // Case is ignored.
            r("GUIDE", Some("docs/Guide.md")),
        ]
    );
    assert_eq!((out[1].heading.as_deref(), out[1].alias.as_deref()), (Some("Plan"), Some("le plan")));
    assert_eq!(out[5].kind, LinkKind::Embed);
    assert_eq!(out[0].kind, LinkKind::Link);
    assert_eq!((out[0].line, out[0].col), (2, 0));
    assert_eq!((out[2].line, out[2].col), (2, 51));
    assert!(Path::new(out[0].resolved.as_ref().unwrap()).is_file(), "usable as such by read_file");

    // Markdown links: relative to the note's folder, then from the root.
    let out = index.outgoing_links(&at(&vault, "docs/Guide.md"));
    assert_eq!(
        resolved(&out, &vault),
        [
            r("../Accueil.md", Some("Accueil.md")),
            r("Mon fichier.md", Some("docs/Mon fichier.md")),
            r("projets/Projet.md", Some("projets/Projet.md")),
            r("../../dehors.md", None),
            r("../assets/schema.png", Some("assets/schema.png")),
            r("Mon fichier", Some("docs/Mon fichier.md")),
            r("Inconnu", None),
        ]
    );
    assert!(index.outgoing_links(&vault.join("absent.md")).is_empty());
    assert!(index.outgoing_links(&at(&vault, "assets/schema.png")).is_empty());
}

#[test]
fn backlinks_to_notes_and_attachments() {
    let dir = TempDir::new("links-back");
    let vault = link_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    let report = reconcile(&index, &data, &vault);
    let ready = index.index_ready(false, report.ms, Some(&report));
    assert_eq!((ready.files, ready.links, ready.unresolved_links), (6, 20, 3));
    assert_eq!((ready.reread, ready.removed, ready.from_cache), (Some(6), Some(0), false));

    // To a note: from two notes, sorted by path then position; the link of
    // Accueil to itself is not a backlink.
    let back = index.backlinks(&vault.join("Accueil.md"));
    let got: Vec<(String, u32, u32)> = back.iter().map(|b| (b.path.clone(), b.line, b.col)).collect();
    assert_eq!(got, [(p(&at(&vault, "docs/Guide.md")), 0, 0), (p(&at(&vault, "projets/Projet.md")), 2, 7)]);
    assert_eq!(back[0].excerpt, "[retour](../Accueil.md) [espace](Mon%20fichier.md) [racine](projets/Projet.md)");
    assert_eq!(slice16(&back[0].excerpt, back[0].ranges[0].0, back[0].ranges[0].1), "[retour](../Accueil.md)");
    assert_eq!(back[0].alias.as_deref(), Some("retour"));
    assert_eq!(slice16(&back[1].excerpt, back[1].ranges[0].0, back[1].ranges[0].1), "[[Accueil]]");

    // Several links from the same note.
    let back = index.backlinks(&at(&vault, "docs/Guide.md"));
    assert_eq!(back.len(), 3);
    assert!(back.iter().all(|b| b.path == p(&vault.join("Accueil.md"))));
    assert_eq!(back.iter().map(|b| b.line).collect::<Vec<_>>(), [5, 5, 6]);
    let back = index.backlinks(&at(&vault, "projets/Projet.md"));
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].heading.as_deref(), Some("Plan"));

    // To an image and to a PDF.
    let back = index.backlinks(&at(&vault, "assets/schema.png"));
    assert_eq!(back.len(), 2);
    assert_eq!((back[0].kind, back[0].path.clone()), (LinkKind::Embed, p(&vault.join("Accueil.md"))));
    assert_eq!(slice16(&back[0].excerpt, back[0].ranges[0].0, back[0].ranges[0].1), "![[schema.png]]");
    assert_eq!(back[1].path, p(&at(&vault, "docs/Guide.md")));
    assert_eq!(index.backlinks(&at(&vault, "docs/plan.pdf")).len(), 1);
    assert!(index.backlinks(&at(&vault, "assets/sub/schema.png")).is_empty());
    assert!(index.backlinks(&vault.join("absent.md")).is_empty());

    // Unresolved targets: most frequent first, same text as the outgoing link.
    let missing = index.unresolved_links(None);
    assert_eq!(missing.iter().map(|u| (u.target.as_str(), u.count)).collect::<Vec<_>>(), [("Inconnu", 3), ("../../dehors.md", 1), ("Autre inconnu", 1)]);
    // Sources in vault tree order (folders first).
    assert_eq!(missing[0].sources, [p(&at(&vault, "docs/Guide.md")), p(&vault.join("Accueil.md"))]);
    for entry in &missing {
        for source in &entry.sources {
            let out = index.outgoing_links(Path::new(source));
            assert!(out.iter().any(|l| l.target == entry.target && l.resolved.is_none()), "{}", entry.target);
        }
    }
    assert_eq!(index.unresolved_links(Some(1)).len(), 1);
}

#[test]
fn resolution_follows_the_files_without_reading_the_notes_again() {
    let dir = TempDir::new("links-recalc");
    let vault = link_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);
    let accueil = vault.join("Accueil.md");
    let target_of = |index: &TagIndex, n: usize| resolved(&index.outgoing_links(&accueil), &vault)[n].1.clone();

    // The missing note appears: one file read (the new one), three links resolved.
    fs::write(vault.join("Inconnu.md"), "me voilà\n").unwrap();
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (1, 0, true));
    assert_eq!(target_of(&index, 3).as_deref(), Some("Inconnu.md"));
    assert_eq!(index.backlinks(&vault.join("Inconnu.md")).len(), 3);
    assert_eq!(index.unresolved_links(None).len(), 2);

    // The winner of an ambiguity disappears: nothing read, the link moves on.
    assert_eq!(target_of(&index, 0).as_deref(), Some("archive/Projet.md"));
    fs::remove_file(at(&vault, "archive/Projet.md")).unwrap();
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (0, 1, true));
    assert_eq!(target_of(&index, 0).as_deref(), Some("projets/Projet.md"));
    assert!(index.backlinks(&at(&vault, "archive/Projet.md")).is_empty());

    // A note is renamed: the new name is read, the sources are not.
    fs::rename(at(&vault, "docs/Guide.md"), at(&vault, "docs/Manuel.md")).unwrap();
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed), (1, 1));
    assert_eq!(target_of(&index, 10), None, "[[GUIDE]] no longer exists");
    assert_eq!(index.backlinks(&accueil).len(), 2);
    assert_eq!(index.backlinks(&accueil)[0].path, p(&at(&vault, "docs/Manuel.md")));

    // An attachment moves: no note read at all, embeds follow.
    fs::create_dir_all(at(&vault, "assets/plus-loin")).unwrap();
    fs::rename(at(&vault, "assets/schema.png"), at(&vault, "assets/plus-loin/schema.png")).unwrap();
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed, report.unchanged, report.persisted), (0, 0, 6, true));
    assert_eq!(target_of(&index, 5).as_deref(), Some("assets/sub/schema.png"), "now the shortest path");
    assert_eq!(index.backlinks(&at(&vault, "assets/sub/schema.png")).len(), 1);
    // Nothing changed: nothing read, nothing written.
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (0, 0, false));
}

#[test]
fn links_survive_persistence_and_follow_saves() {
    let dir = TempDir::new("links-persist");
    let vault = link_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);
    let accueil = vault.join("Accueil.md");
    let guide = at(&vault, "docs/Guide.md");
    let snapshot = |index: &TagIndex| {
        (index.outgoing_links(&accueil), index.outgoing_links(&guide), index.backlinks(&accueil), index.backlinks(&at(&vault, "assets/schema.png")), index.unresolved_links(None))
    };
    let expected = snapshot(&index);
    drop(index);

    // New session: links are available from the file alone, no note read.
    let session = VaultIndex::new();
    let ready = session.load_persisted(&data, &vault).unwrap();
    assert_eq!(ready.files, 6);
    session.open_search(&data, &vault);
    assert!(session.open_search(&data, &vault));
    assert_eq!(snapshot(&session), expected);
    let state = session.index_ready(true, ready.ms, None);
    assert_eq!((state.from_cache, state.links, state.unresolved_links, state.reread), (true, 20, 3, None));
    assert_eq!(reconcile(&session, &data, &vault).reread, 0);

    // A save: links reflect the text in memory as soon as the call returns.
    let text = "[[Projet]] seulement, et [[Nouveau lien]]\n";
    let saved = save_file(&accueil, text, None).unwrap();
    assert!(session.update_file_with_meta(&accueil, text, saved.size, saved.mtime_ms));
    assert_eq!(resolved(&session.outgoing_links(&accueil), &vault), [r("Projet", Some("archive/Projet.md")), r("Nouveau lien", None)]);
    assert!(session.backlinks(&guide).is_empty(), "Accueil no longer links to the guide");
    assert_eq!(session.backlinks(&at(&vault, "archive/Projet.md")).len(), 1);
    assert!(session.unresolved_links(None).iter().any(|u| u.target == "Nouveau lien"));
    assert_eq!(reconcile(&session, &data, &vault).reread, 0, "the saved note is not read again");
    session.remove_file(&guide);
    assert!(session.backlinks(&accueil).iter().all(|b| b.path != p(&guide)));
    drop(session);

    // A file of the previous format (version 1) is ignored, then rebuilt.
    let file = tags_index_path(&data, vault.to_str().unwrap());
    let mut old = fs::read(&file).unwrap();
    assert_eq!(u32::from_le_bytes(old[8..12].try_into().unwrap()), 2);
    old[8..12].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&file, &old).unwrap();
    let again = VaultIndex::new();
    assert!(load(&again, &data, &vault).is_none());
    let report = reconcile(&again, &data, &vault);
    assert_eq!((report.reread, report.persisted), (6, true));
    assert_eq!(again.outgoing_links(&accueil).len(), 2);
    assert_eq!(u32::from_le_bytes(fs::read(&file).unwrap()[8..12].try_into().unwrap()), 2);
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn search_vault(dir: &TempDir) -> PathBuf {
    dir.write("v/Résumé.md", "# Résumé\n\nLe résumé de l'architecture du projet Rocksy.\nDeuxième ligne : été comme hiver.\n");
    dir.write("v/notes/Architecture.md", "Plan général du logiciel.\n\n- cœur en Rust\n- interface web\n");
    dir.write("v/notes/Divers.md", "Architecture logicielle et archives du projet. #projet/rocksy\n\nUn brouillon à jeter.\n");
    dir.write("v/journal/2026-01.md", "---\ntags: [journal, Travail/Réunion]\n---\nRESUME court de la réunion sur l'ARCHITECTURE.\n");
    dir.write("v/journal/bom.md", "\u{feff}🎉 projet en tête\r\nsuite\r\ndeux projets : projet un, projet deux\r\n");
    dir.write("v/notes/Vide.md", "");
    dir.write("v/notes/latin1.md", [b'p', b'r', b'o', b'j', b'e', b't', b' ', 0xE9]);
    dir.write("v/.obsidian/cache.md", "architecture cachée\n");
    dir.write("v/image.png", [1u8, 2]);
    dir.path().join("v")
}

/// Relative paths of the hits, in order.
fn hits(result: &SearchResult, root: &Path) -> Vec<String> {
    let prefix = p(root).len() + 1;
    result.hits.iter().map(|h| h.path[prefix..].replace('\\', "/")).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn found(index: &TagIndex, root: &Path, query: &str) -> Vec<String> {
    let result = index.search(query, None);
    assert_eq!(result.total, result.hits.len(), "{query}");
    assert_eq!(result.query, query);
    sorted(hits(&result, root))
}

/// Checks every excerpt of a result against the real file: `line`, `col`
/// and `len` designate the first occurrence in the text, and every range of
/// the excerpt covers one occurrence of `needle` (compared folded).
fn check_matches(result: &SearchResult, needle: &str) {
    let fold = |s: &str| s.to_lowercase().replace(['é', 'è', 'ê'], "e").replace('œ', "oe");
    for hit in &result.hits {
        let text = fs::read_to_string(&hit.path).unwrap();
        let lines = cm_lines(&text);
        assert!(hit.matches.len() <= 3);
        assert!(hit.matches.windows(2).all(|w| w[0].line < w[1].line), "text order");
        for m in &hit.matches {
            let line = &lines[m.line as usize];
            let first = slice16(line, m.col, m.col + m.len);
            assert!(fold(&first).starts_with(needle), "{first:?} in {} line {}", hit.path, m.line);
            assert!(!m.ranges.is_empty());
            assert!(m.excerpt.chars().count() <= 240 && !m.excerpt.contains('\n'));
            for &(a, b) in &m.ranges {
                assert!(fold(&slice16(&m.excerpt, a, b)).starts_with(needle), "{:?}", m.excerpt);
            }
        }
        if !hit.matches.is_empty() {
            assert!(hit.match_count >= hit.matches.len());
        }
    }
}

#[test]
fn search_semantics() {
    let dir = TempDir::new("search-sem");
    let vault = search_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();

    // Before anything is indexed: empty, not ready, no error.
    let nothing = index.search("architecture", None);
    assert_eq!((nothing.total, nothing.hits.len(), nothing.search_ready), (0, 0, false));
    assert!(!index.search_ready());

    let report = reconcile(&index, &data, &vault);
    assert_eq!(report.files, 7);
    assert!(index.search_ready());
    let ready = index.index_ready(false, report.ms, Some(&report));
    assert_eq!((ready.search_ready, ready.search_docs), (true, 6), "the non-UTF-8 note is not searchable");

    // Case and accents are ignored, both ways.
    assert_eq!(found(&index, &vault, "resume "), ["Résumé.md", "journal/2026-01.md"]);
    assert_eq!(found(&index, &vault, "RÉSUMÉ"), ["Résumé.md", "journal/2026-01.md"]);
    assert_eq!(found(&index, &vault, "ete"), ["Résumé.md"]);
    assert_eq!(found(&index, &vault, "coeur"), ["notes/Architecture.md"]);
    assert_eq!(found(&index, &vault, "reunion"), ["journal/2026-01.md"]);

    // Words are combined with AND.
    assert_eq!(found(&index, &vault, "architecture projet"), ["Résumé.md", "notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "architecture projet rocksy logicielle"), ["notes/Divers.md"]);
    assert!(found(&index, &vault, "architecture inexistant").is_empty());

    // The last word is a prefix; the others are whole words.
    assert_eq!(found(&index, &vault, "archi"), ["Résumé.md", "journal/2026-01.md", "notes/Architecture.md", "notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "archiv"), ["notes/Divers.md"]);
    assert!(found(&index, &vault, "archi projet").is_empty(), "archi is not a whole word");
    assert_eq!(found(&index, &vault, "projet archi"), ["Résumé.md", "notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "a"), ["Résumé.md", "journal/2026-01.md", "notes/Architecture.md", "notes/Divers.md"]);

    // Exact phrase.
    assert_eq!(found(&index, &vault, "\"architecture du projet\""), ["Résumé.md"]);
    assert_eq!(found(&index, &vault, "\"archives du projet\""), ["notes/Divers.md"]);
    assert!(found(&index, &vault, "\"projet du architecture\"").is_empty());
    assert_eq!(found(&index, &vault, "l'architecture"), ["Résumé.md", "journal/2026-01.md"]);

    // Exclusion.
    assert_eq!(found(&index, &vault, "architecture -brouillon"), ["Résumé.md", "journal/2026-01.md", "notes/Architecture.md"]);
    assert_eq!(found(&index, &vault, "architecture -\"du projet\""), ["journal/2026-01.md", "notes/Architecture.md"]);
    assert!(found(&index, &vault, "-brouillon").is_empty(), "nothing positive asked");

    // Path filter, alone or combined; folded like the rest.
    assert_eq!(found(&index, &vault, "path:notes architecture"), ["notes/Architecture.md", "notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "path:JOURNAL"), ["journal/2026-01.md", "journal/bom.md"]);
    assert_eq!(found(&index, &vault, "path:resume"), ["Résumé.md"]);
    assert_eq!(found(&index, &vault, "path:notes/arch"), ["notes/Architecture.md"]);
    assert_eq!(found(&index, &vault, "architecture -path:notes"), ["Résumé.md", "journal/2026-01.md"]);
    assert_eq!(found(&index, &vault, "path:.md -path:o"), ["Résumé.md"]);

    // Tag filter, with descendants, from inline tags and frontmatter.
    assert_eq!(found(&index, &vault, "tag:projet"), ["notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "tag:#projet/rocksy architecture"), ["notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "tag:TRAVAIL"), ["journal/2026-01.md"]);
    assert_eq!(found(&index, &vault, "tag:travail/réunion resume"), ["journal/2026-01.md"]);
    assert!(found(&index, &vault, "tag:rocksy").is_empty(), "a level is not a tag");
    assert_eq!(found(&index, &vault, "-tag:journal -tag:projet archi"), ["Résumé.md", "notes/Architecture.md"]);
    // A word followed by a filter is no longer the word being typed.
    assert!(found(&index, &vault, "archi -tag:journal").is_empty());

    // Empty queries.
    for query in ["", "   ", "\"\"", "--- !!"] {
        let result = index.search(query, None);
        assert_eq!((result.total, result.hits.len(), result.search_ready), (0, 0, true), "{query:?}");
    }
    // Hidden folders and non-Markdown files are not indexed.
    assert!(found(&index, &vault, "cachee").is_empty());
}

#[test]
fn search_ranking_limits_and_excerpts() {
    let dir = TempDir::new("search-rank");
    let vault = search_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);

    // The file name weighs more than the body.
    let result = index.search("architecture", None);
    assert_eq!(result.total, 4);
    assert_eq!(hits(&result, &vault)[0], "notes/Architecture.md");
    assert!(result.hits[0].name_match);
    assert_eq!(result.hits[0].match_count, 0, "only in the name");
    assert!(result.hits[0].matches.is_empty());
    assert!(result.hits[1..].iter().all(|h| !h.name_match && h.match_count == 1));
    assert!(result.hits.windows(2).all(|w| w[0].score >= w[1].score), "best first");
    assert!(result.ms >= 0.0);
    check_matches(&result, "architecture");

    // In both the name and the body.
    let result = index.search("resume", None);
    assert_eq!(hits(&result, &vault), ["Résumé.md", "journal/2026-01.md"]);
    assert!(result.hits[0].name_match && result.hits[0].match_count == 2);
    assert_eq!(result.hits[0].matches.iter().map(|m| (m.line, m.col, m.len)).collect::<Vec<_>>(), [(0, 2, 6), (2, 3, 6)]);
    assert_eq!(result.hits[0].matches[1].excerpt, "Le résumé de l'architecture du projet Rocksy.");
    assert_eq!(result.hits[0].matches[1].ranges, [(3, 9)]);
    check_matches(&result, "resume");

    // `total` counts files, `limit` bounds the hits.
    let result = index.search("archi", Some(2));
    assert_eq!((result.total, result.hits.len()), (4, 2));
    let result = index.search("archi", Some(0));
    assert_eq!((result.total, result.hits.len()), (4, 0));

    // Positions follow the text returned by read_file: byte-order mark,
    // CRLF, and an emoji before the match.
    let result = index.search("path:bom projet", None);
    assert_eq!(result.hits.len(), 1);
    let hit = &result.hits[0];
    assert_eq!(hit.match_count, 4, "the last word is a prefix: 'projets' counts too");
    assert_eq!(hit.matches.iter().map(|m| (m.line, m.col, m.len)).collect::<Vec<_>>(), [(0, 4, 6), (2, 5, 7)]);
    assert_eq!(hit.matches[0].excerpt, "🎉 projet en tête");
    assert_eq!(hit.matches[0].ranges, [(3, 9)]);
    assert_eq!(hit.matches[1].ranges, [(5, 12), (15, 21), (26, 32)]);
    let text = rocksy_core::read_file(Path::new(&hit.path)).unwrap().text;
    assert_eq!(slice16(&cm_lines(&text)[0], 4, 10), "projet");

    // A phrase is highlighted as a whole.
    let result = index.search("\"architecture du projet\"", None);
    let m = &result.hits[0].matches[0];
    assert_eq!(slice16(&m.excerpt, m.ranges[0].0, m.ranges[0].1), "architecture du projet");
    assert_eq!((m.line, m.col, m.len), (2, 15, 22));

    // Filters only: the notes are listed, without excerpts.
    let result = index.search("tag:journal", None);
    assert_eq!(hits(&result, &vault), ["journal/2026-01.md"]);
    assert!(result.hits[0].matches.is_empty() && !result.hits[0].name_match);
}

#[test]
fn search_follows_saves_and_reconciles() {
    let dir = TempDir::new("search-update");
    let vault = search_vault(&dir);
    let data = dir.path().join("data");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);

    // A save: searchable when the call returns, old words gone, tags too.
    let note = at(&vault, "notes/Divers.md");
    let text = "Texte entièrement nouveau sur les baleines. #mer/océan\n";
    let saved = save_file(&note, text, None).unwrap();
    assert!(index.update_file_with_meta(&note, text, saved.size, saved.mtime_ms));
    assert_eq!(found(&index, &vault, "baleines"), ["notes/Divers.md"]);
    assert!(found(&index, &vault, "brouillon").is_empty());
    assert_eq!(found(&index, &vault, "tag:mer"), ["notes/Divers.md"]);
    assert!(found(&index, &vault, "tag:projet").is_empty());
    assert_eq!(index.index_ready(false, 0, None).search_docs, 6);
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.persisted), (0, true), "the saved note is read by neither store");

    // A note the application removes.
    fs::remove_file(at(&vault, "notes/Architecture.md")).unwrap();
    index.remove_file(&at(&vault, "notes/Architecture.md"));
    assert_eq!(found(&index, &vault, "architecture"), ["Résumé.md", "journal/2026-01.md"]);

    // External changes, seen by reconcile: added, modified, removed.
    fs::write(at(&vault, "notes/Ajout.md"), "une girafe apparaît\n").unwrap();
    fs::write(vault.join("Résumé.md"), "plus rien sur le sujet, seulement un zèbre\n").unwrap();
    fs::remove_file(at(&vault, "journal/2026-01.md")).unwrap();
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.removed), (2, 1));
    assert_eq!(found(&index, &vault, "girafe"), ["notes/Ajout.md"]);
    assert_eq!(found(&index, &vault, "zebre"), ["Résumé.md"]);
    assert!(found(&index, &vault, "architecture").is_empty());
    assert!(found(&index, &vault, "reunion").is_empty());
    assert_eq!(index.index_ready(false, 0, None).search_docs, 5);

    // Text that is not on disk (old signature, unsaved): searchable now,
    // replaced by the disk content at the next reconcile.
    assert!(index.update_file(&note, "un brouillon avec un narval\n"));
    assert_eq!(found(&index, &vault, "narval"), ["notes/Divers.md"]);
    assert_eq!(reconcile(&index, &data, &vault).reread, 1);
    assert!(found(&index, &vault, "narval").is_empty());
    assert_eq!(found(&index, &vault, "baleines"), ["notes/Divers.md"]);

    // A new session answers from the persisted index, without any read.
    drop(index);
    let session = VaultIndex::new();
    assert!(session.load_persisted(&data, &vault).is_some());
    assert!(!session.search_ready(), "tags and links first");
    assert_eq!(session.search("girafe", None).hits.len(), 0);
    assert!(session.open_search(&data, &vault));
    assert!(session.search_ready());
    assert_eq!(found(&session, &vault, "girafe"), ["notes/Ajout.md"]);
    let report = reconcile(&session, &data, &vault);
    assert_eq!((report.reread, report.removed, report.persisted), (0, 0, false));

    // Closing the vault closes the search.
    session.clear();
    assert!(!session.search_ready());
    assert_eq!(session.search("girafe", None).hits.len(), 0);
}

// ---------------------------------------------------------------------------
// Consistency between the two stores
// ---------------------------------------------------------------------------

#[test]
fn stores_repair_each_other_after_a_crash_between_the_two_writes() {
    let dir = TempDir::new("search-crash");
    let vault = search_vault(&dir);
    let data = dir.path().join("data");
    let tags_file = tags_index_path(&data, vault.to_str().unwrap());
    let search_root = data.join("search");
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);
    drop(index);

    // State A of both stores, kept aside.
    let tags_a = fs::read(&tags_file).unwrap();
    let search_a = dir.path().join("search-a");
    copy_dir(&search_root, &search_a);

    // The vault changes and both stores move to state B.
    fs::write(at(&vault, "notes/Divers.md"), "contenu B avec un okapi\n").unwrap();
    fs::write(at(&vault, "notes/Nouveau.md"), "note B avec un tapir\n").unwrap();
    let index = VaultIndex::new();
    load(&index, &data, &vault).unwrap();
    assert_eq!(reconcile(&index, &data, &vault).reread, 2);
    drop(index);
    let tags_b = fs::read(&tags_file).unwrap();

    // Crash 1: the full-text index was committed, the tag file was not.
    fs::write(&tags_file, &tags_a).unwrap();
    let index = VaultIndex::new();
    load(&index, &data, &vault).unwrap();
    assert!(index.tags_list("projet").len() == 2, "old tags, from state A");
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.unchanged), (2, 6), "only the two stale notes are read");
    assert!(index.tags_list("projet").is_empty());
    assert_eq!(found(&index, &vault, "okapi"), ["notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "tapir"), ["notes/Nouveau.md"]);
    drop(index);
    assert_eq!(fs::read(&tags_file).unwrap(), tags_b, "back to state B");

    // Crash 2: the tag file was written, the full-text index is one state behind.
    fs::remove_dir_all(&search_root).unwrap();
    copy_dir(&search_a, &search_root);
    let index = VaultIndex::new();
    load(&index, &data, &vault).unwrap();
    assert!(found(&index, &vault, "okapi").is_empty(), "search still in state A");
    assert_eq!(found(&index, &vault, "brouillon"), ["notes/Divers.md"]);
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.unchanged, report.persisted), (2, 6, false), "tags were already right");
    assert_eq!(found(&index, &vault, "okapi"), ["notes/Divers.md"]);
    assert_eq!(found(&index, &vault, "tapir"), ["notes/Nouveau.md"]);
    assert!(found(&index, &vault, "brouillon").is_empty());
    assert_eq!(reconcile(&index, &data, &vault).reread, 0);
    drop(index);

    // The full-text index is deleted by hand: rebuilt, tags untouched.
    fs::remove_dir_all(&search_root).unwrap();
    let index = VaultIndex::new();
    assert!(load(&index, &data, &vault).is_some());
    assert!(!index.search_ready());
    assert_eq!(index.search("okapi", None).hits.len(), 0);
    assert_eq!(index.tags_list("").len(), 3, "tags and links are there without the search");
    let report = reconcile(&index, &data, &vault);
    assert_eq!((report.reread, report.unchanged, report.persisted), (7, 1, false), "every readable note, once");
    assert!(index.search_ready());
    assert_eq!(found(&index, &vault, "okapi"), ["notes/Divers.md"]);
    drop(index);

    // The full-text index is damaged: replaced, then rebuilt.
    let meta = fs::read_dir(&search_root).unwrap().next().unwrap().unwrap().path().join("meta.json");
    fs::write(&meta, "{ not json").unwrap();
    let index = VaultIndex::new();
    assert!(load(&index, &data, &vault).is_some());
    assert!(!index.search_ready());
    assert_eq!(reconcile(&index, &data, &vault).reread, 7);
    assert_eq!(found(&index, &vault, "tapir"), ["notes/Nouveau.md"]);
    drop(index);

    // Both stores lost: full build.
    fs::remove_dir_all(&data).unwrap();
    let index = VaultIndex::new();
    assert!(load(&index, &data, &vault).is_none());
    assert_eq!(reconcile(&index, &data, &vault).reread, 8);
    assert_eq!(found(&index, &vault, "okapi"), ["notes/Divers.md"]);
}

#[test]
fn interrupted_first_indexing_resumes_and_becomes_ready() {
    let dir = TempDir::new("search-resume");
    for i in 0..300 {
        dir.write(&format!("v/d{}/n{i:03}.md", i % 7), format!("note {i} commune, mot{i} #t/{}\n[[n{:03}]]\n", i % 5, (i + 1) % 300));
    }
    let vault = dir.path().join("v");
    let data = dir.path().join("data");

    let index = VaultIndex::new();
    let cancel = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let res = index
        .reconcile(&data, &vault, BuildOptions { threads: Some(2) }, &cancel, |_| {
            if calls.fetch_add(1, Ordering::Relaxed) == 3 {
                cancel.cancel();
            }
        })
        .unwrap();
    assert!(res.is_none());
    let partial = index.search("commune", Some(1000)).total;
    assert!(partial > 0 && partial < 300, "{partial}");
    assert!(!index.search_ready(), "a partial first indexing is not ready");
    assert!(!index.search("commune", None).search_ready);
    drop(index);

    // The process stops; the next one resumes from the checkpoint.
    let session = VaultIndex::new();
    assert_eq!(load(&session, &data, &vault).unwrap(), partial);
    assert!(!session.search_ready());
    assert_eq!(session.search("commune", Some(1000)).total, partial, "what was done is searchable");
    let report = reconcile(&session, &data, &vault);
    assert_eq!((report.reread, report.unchanged), (300 - partial, partial));
    assert!(session.search_ready());
    assert_eq!(session.search("commune", Some(10)).total, 300);
    assert_eq!(session.search("mot123", None).total, 1);
    let state = session.index_ready(false, report.ms, Some(&report));
    assert_eq!((state.files, state.links, state.unresolved_links, state.search_docs, state.search_ready), (300, 300, 0, 300, true));
    assert_eq!(session.backlinks(&at(&vault, "d1/n001.md")).len(), 1);
}

// ---------------------------------------------------------------------------
// F7
// ---------------------------------------------------------------------------

#[test]
fn links_and_search_never_modify_the_vault() {
    let dir = TempDir::new("search-f7");
    let vault = common::make_vault(&dir, "vault");
    dir.write("vault/Liens.md", "[[Accueil]] ![[image.png]] [[Absent]] [j](01%20Travail/Journal.md)\n");
    let data = dir.path().join("appdata");
    let before = fingerprint(&vault);

    let index = VaultIndex::new();
    let report = reconcile(&index, &data, &vault);
    assert_eq!(report.files, 7);
    index.search("bienvenue", None);
    index.search("tag:journal lundi", None);
    assert_eq!(index.backlinks(&vault.join("Accueil.md")).len(), 1);
    assert_eq!(index.backlinks(&at(&vault, "_ASSETS/image.png")).len(), 1);
    assert_eq!(index.backlinks(&at(&vault, "01 Travail/Journal.md")).len(), 1);
    assert_eq!(index.outgoing_links(&vault.join("Liens.md")).len(), 4);
    assert_eq!(index.unresolved_links(None).len(), 1);
    index.update_file(&vault.join("Accueil.md"), "#modifie en memoire [[Liens]]\n");
    index.update_file_with_meta(&at(&vault, "02 Perso/vide.md"), "texte fantôme\n", 15, 1);
    index.remove_file(&at(&vault, "02 Perso/Recettes.md"));
    index.save(&data).unwrap();
    index.index_ready(false, 0, Some(&report));
    drop(index);

    let session = VaultIndex::new();
    load(&session, &data, &vault).unwrap();
    session.search("fantome", None);
    reconcile(&session, &data, &vault);
    assert_eq!(session.search("fantome", None).total, 0, "the disk wins");
    session.clear();
    drop(session);

    assert_eq!(fingerprint(&vault), before, "vault untouched, nothing created in it");
    let data_files = fingerprint(&data);
    assert!(data_files.keys().all(|n| n.starts_with("tags/") || n.starts_with("search/")), "{:?}", data_files.keys());
}
