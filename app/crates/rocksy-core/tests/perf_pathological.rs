//! Pathological inputs: one very long line must cost time proportional to
//! its length, in the tag and link analyser as in the search highlighter.
//!
//! The time limits are wide (an unoptimised build on a busy machine passes
//! them several times over) but far below what the quadratic versions took:
//! seconds for each of these inputs.

mod common;

use common::TempDir;
use rocksy_core::{file_tags, BuildOptions, CancelToken, LinkKind, VaultIndex};
use std::path::Path;
use std::time::{Duration, Instant};

fn timed<T>(what: &str, limit_ms: u64, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = run();
    let took = start.elapsed();
    println!("{what}: {took:?}");
    assert!(took < Duration::from_millis(limit_ms), "{what} took {took:?}, limit {limit_ms} ms");
    out
}

fn reconcile(index: &VaultIndex, data: &Path, vault: &Path) {
    index.reconcile(data, vault, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap();
}

fn slice16(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[start as usize..end as usize]).unwrap()
}

// ---- Tags and links ---------------------------------------------------------

/// 160 KB on one line, with nothing ever closed.
#[test]
fn unclosed_openers_on_one_line_are_parsed_in_linear_time() {
    const KB160: usize = 160 * 1024;
    let cases: [(&str, String); 9] = [
        ("160 KB of '['", "[".repeat(KB160)),
        ("160 KB of '[a'", "[a".repeat(KB160 / 2)),
        ("160 KB of '[a]('", "[a](".repeat(KB160 / 4)),
        ("160 KB of '[a][' ", "[a][".repeat(KB160 / 4)),
        ("160 KB of '[ ['", "[ [".repeat(KB160 / 3)),
        ("160 KB of '(['", "([".repeat(KB160 / 2)),
        ("160 KB of '<a '", "<a ".repeat(KB160 / 3)),
        ("160 KB of '[[a] '", "[[a] ".repeat(KB160 / 5)),
        ("160 KB of backtick runs of every length", (1..560).map(|n| format!("{} ", "`".repeat(n))).collect()),
    ];
    for (what, line) in &cases {
        assert!(line.len() >= KB160 * 9 / 10 && !line.contains('\n'), "{what}: {} bytes", line.len());
        let fragments = timed(what, 200, || file_tags("/v/x.md", line));
        assert!(fragments.is_empty(), "{what}");
    }
    // The same line with a tag at each end: both are still found.
    let line = format!("#debut {} #fin", "[a".repeat(KB160 / 2));
    let fragments = timed("160 KB of '[a' between two tags", 200, || file_tags("/v/x.md", &line));
    assert_eq!(fragments.iter().map(|f| f.tag.as_str()).collect::<Vec<_>>(), ["debut", "fin"]);
    assert_eq!(fragments[1].tag_col as usize, line.len() - 4);
}

/// Second half of the defect: many tags or links on one long line, each
/// with its column and its excerpt.
#[test]
fn many_tags_and_links_on_one_line_are_parsed_in_linear_time() {
    // Tags, with characters outside ASCII so that columns must be counted.
    let line = "é #tag/été ".repeat(20_000);
    assert!(line.len() > 250_000);
    let fragments = timed("20 000 tags on one line of 260 KB", 1500, || file_tags("/v/x.md", &line));
    assert_eq!(fragments.len(), 20_000);
    for k in [0usize, 1, 17, 18, 19, 7_000, 19_999] {
        let f = &fragments[k];
        assert_eq!((f.tag.as_str(), f.tag_line, f.tag_col as usize), ("tag/été", 0, k * 11 + 2), "tag {k}");
        if k >= 19 {
            // Too far from the start of the block: excerpt centred on the tag.
            assert!(f.excerpt.starts_with('…') && f.excerpt.contains("#tag/été"), "tag {k}: {}", f.excerpt);
        } else {
            assert!(f.excerpt.starts_with("é #tag/été é #tag/été"), "tag {k}: {}", f.excerpt);
        }
        assert!(f.excerpt.chars().count() <= 240);
    }

    // Links: through the index, as the application reads them.
    let dir = TempDir::new("perf-links");
    let vault = dir.path().join("vault");
    let data = dir.path().join("data");
    let line = "é [[Cible|été]] ".repeat(20_000);
    assert!(line.len() > 350_000);
    let note = dir.write("vault/Longue.md", &line);
    dir.write("vault/Cible.md", "cible\n");
    let index = VaultIndex::new();
    // Listing, reading and the full-text index are in this time too.
    timed("reconcile with 20 000 links on one line of 360 KB", 4000, || reconcile(&index, &data, &vault));

    let out = index.outgoing_links(&note);
    assert_eq!(out.len(), 20_000);
    for k in [0usize, 1, 10, 11, 12, 7_000, 19_999] {
        assert_eq!((out[k].target.as_str(), out[k].line, out[k].col as usize), ("Cible", 0, k * 16 + 2), "link {k}");
        assert_eq!(out[k].alias.as_deref(), Some("été"));
    }
    let back = index.backlinks(&vault.join("Cible.md"));
    assert_eq!(back.len(), 20_000);
    for k in [0usize, 1, 10, 11, 12, 7_000, 19_999] {
        let link = &back[k];
        assert_eq!((link.kind, link.line, link.col as usize), (LinkKind::Link, 0, k * 16 + 2), "link {k}");
        assert!(link.excerpt.chars().count() <= 240);
        assert_eq!(link.ranges.len(), 1, "link {k}");
        assert_eq!(slice16(&link.excerpt, link.ranges[0].0, link.ranges[0].1), "[[Cible|été]]", "link {k}");
        assert_eq!(link.excerpt.starts_with('…'), k >= 12, "link {k}: {}", link.excerpt);
    }
}

/// An opener that is never closed hides nothing that follows it.
#[test]
fn a_real_link_after_unclosed_openers_is_still_found() {
    let dir = TempDir::new("perf-semantics");
    let vault = dir.path().join("vault");
    let data = dir.path().join("data");
    dir.write("vault/b.md", "b\n");
    dir.write("vault/c.md", "c\n");
    let cases: [(&str, &[(&str, u32)]); 10] = [
        ("[ voir [a](b.md)", &[("b.md", 7)]),
        ("[[ voir [a](b.md)", &[("b.md", 8)]),
        ("[ ( [ voir [a](b.md) puis [[c]]", &[("b.md", 11), ("c", 26)]),
        ("[x [y [z [a](b.md) fin", &[("b.md", 9)]),
        // Nested brackets in the text: the outer link wins.
        ("[x [a [b](c.md)](b.md)", &[("b.md", 3)]),
        ("[a [b]](b.md) et [ [c](c.md)", &[("b.md", 0), ("c.md", 19)]),
        // Escaped brackets neither open nor close.
        ("\\[ [a](b.md) \\] [c](c.md", &[("b.md", 3)]),
        ("[a\\]](b.md) [", &[("b.md", 0)]),
        // Parentheses in the destination, then an unclosed one.
        ("[a](b(1).md) [c](c(.md)", &[("b(1).md", 0)]),
        ("![ ![a](b.md) ![[c]]", &[("b.md", 3), ("c", 14)]),
    ];
    let mut notes = Vec::new();
    for (k, (text, _)) in cases.iter().enumerate() {
        // Each case alone, then drowned in a long line of unclosed openers.
        notes.push(dir.write(&format!("vault/cas{k}.md"), text));
        notes.push(dir.write(&format!("vault/long{k}.md"), format!("{}{text}", "[ ".repeat(50_000))));
    }
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);
    for (k, (text, expected)) in cases.iter().enumerate() {
        let alone: Vec<(String, u32)> = index.outgoing_links(&notes[2 * k]).into_iter().map(|l| (l.target, l.col)).collect();
        let want: Vec<(String, u32)> = expected.iter().map(|&(t, col)| (t.to_string(), col)).collect();
        assert_eq!(alone, want, "{text}");
        let long: Vec<(String, u32)> = index.outgoing_links(&notes[2 * k + 1]).into_iter().map(|l| (l.target, l.col)).collect();
        let want: Vec<(String, u32)> = expected.iter().map(|&(t, col)| (t.to_string(), col + 100_000)).collect();
        assert_eq!(long, want, "after 50 000 unclosed '[': {text}");
    }
}

// ---- Search highlighting ----------------------------------------------------

/// The third matching line of a note is 800 KB long and full of the word.
#[test]
fn search_in_a_very_long_line_is_linear_and_keeps_its_positions() {
    let dir = TempDir::new("perf-search");
    let vault = dir.path().join("vault");
    let data = dir.path().join("data");
    let words = 200_000;
    let long = format!("mot\nété mot\nété 🎉 {}fin\nmot après\n", "mot ".repeat(words));
    assert!(long.len() > 800_000);
    dir.write("vault/long.md", &long);
    let index = VaultIndex::new();
    reconcile(&index, &data, &vault);
    assert!(index.search_ready());

    index.search("mot", Some(50)); // first query: opens the reader
    let result = timed("search of a word present 200 000 times on a line of 800 KB", 1500, || index.search("mot", Some(50)));
    assert_eq!(result.hits.len(), 1);
    let hit = &result.hits[0];
    assert_eq!(hit.match_count, words + 3, "every occurrence is counted, the one after the long line too");
    let at: Vec<(u32, u32, u32)> = hit.matches.iter().map(|m| (m.line, m.col, m.len)).collect();
    assert_eq!(at, [(0, 0, 3), (1, 4, 3), (2, 7, 3)], "the emoji counts for two units");
    let third = &hit.matches[2];
    assert!(third.excerpt.starts_with("été 🎉 mot mot") && third.excerpt.ends_with('…'));
    assert_eq!(third.excerpt.chars().count(), 240);
    // Every occurrence visible in the excerpt is highlighted, none beyond.
    assert_eq!(third.ranges.len(), third.excerpt.matches("mot").count());
    assert!(third.ranges.len() > 50);
    assert!(third.ranges.iter().all(|&(a, b)| slice16(&third.excerpt, a, b) == "mot"));

    // A phrase and a second word on the same long line.
    let result = timed("phrase search on the same line", 1500, || index.search("\"mot mot\" fin", Some(50)));
    let hit = &result.hits[0];
    assert_eq!(hit.match_count, words);
    assert_eq!((hit.matches[0].line, hit.matches[0].col, hit.matches[0].len), (2, 7, 7));
}
