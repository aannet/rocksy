//! Tests of the tag rules, the block analyser, the frontmatter reader and
//! the in-memory index.

use super::parse::{split_lines, EXCERPT_MAX};
use super::*;
use crate::testutil::{all_files, TempDir};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn frags(text: &str) -> Vec<Fragment> {
    file_tags("/t.md", text)
}

/// (tag, line, col) of every tag.
fn tags(text: &str) -> Vec<(String, u32, u32)> {
    frags(text).into_iter().map(|f| (f.tag, f.tag_line, f.tag_col)).collect()
}

fn names(text: &str) -> Vec<String> {
    frags(text).into_iter().map(|f| f.tag).collect()
}

/// (start, end) of the block of `tag`.
fn span(text: &str, tag: &str) -> (u32, u32) {
    let all = frags(text);
    let f = all.iter().find(|f| f.tag == tag).unwrap_or_else(|| panic!("tag {tag} not found in {all:?}"));
    (f.start_line, f.end_line)
}

// ---------------------------------------------------------------------------
// Tag syntax
// ---------------------------------------------------------------------------

#[test]
fn simple_tags_and_positions() {
    assert_eq!(tags("hello #tag world"), [("tag".to_string(), 0, 6)]);
    assert_eq!(tags("#start of line"), [("start".to_string(), 0, 0)]);
    assert_eq!(
        tags("one #a two #b_c #d-e"),
        [("a".to_string(), 0, 4), ("b_c".to_string(), 0, 11), ("d-e".to_string(), 0, 16)]
    );
    assert_eq!(tags("line\n\nsecond #x"), [("x".to_string(), 2, 7)]);
    assert!(tags("no tag here").is_empty());
    assert!(tags("").is_empty());
}

#[test]
fn numeric_tags_are_rejected() {
    assert!(names("#123").is_empty());
    assert!(names("issue #42 is fixed, see #7.").is_empty());
    assert_eq!(names("#1a #a1 #2024-01 #v2 #1/2"), ["1a", "a1", "2024-01", "v2", "1/2"]);
    assert_eq!(names("#123 #abc"), ["abc"]);
}

#[test]
fn hash_must_follow_start_or_whitespace() {
    assert!(names("word#no").is_empty());
    assert!(names("(#no)").is_empty());
    assert!(names("a,#no").is_empty());
    assert!(names("\\#escaped").is_empty());
    assert_eq!(names("#one#two"), ["one"]);
    assert_eq!(names("a\t#tab"), ["tab"]);
    // Non-breaking and ideographic spaces count as whitespace.
    assert_eq!(names("a\u{a0}#nbsp b\u{3000}#wide"), ["nbsp", "wide"]);
    assert!(names("# not a tag").is_empty());
    assert!(names("a # b").is_empty());
    assert!(names("## #").is_empty());
}

#[test]
fn urls_are_not_tags() {
    assert!(names("see https://example.com/page#section now").is_empty());
    assert!(names("http://x.org/#/route and https://a.b/c#d-e_f").is_empty());
    assert!(names("<https://example.com/a #b>").is_empty());
    assert_eq!(names("https://example.com/#frag #real"), ["real"]);
}

#[test]
fn inline_code_is_skipped() {
    assert_eq!(names("`#no` then #yes"), ["yes"]);
    assert_eq!(names("``double #no ` still`` #yes"), ["yes"]);
    assert_eq!(names("`a` #mid `b #no`"), ["mid"]);
    // An unclosed backtick is literal text.
    assert_eq!(names("it`s #fine"), ["fine"]);
    assert_eq!(names("` #a"), ["a"]);
}

#[test]
fn code_blocks_are_skipped() {
    assert_eq!(names("```\n#no\n```\n#yes"), ["yes"]);
    assert_eq!(names("```rust\n#[derive(Debug)]\n# comment #no\n```\n\nafter #yes"), ["yes"]);
    assert_eq!(names("~~~\n#no\n```\n#still-no\n~~~\n#yes"), ["yes"]);
    // A longer fence is only closed by a fence at least as long.
    assert_eq!(names("````\n```\n#no\n```\n````\n#yes"), ["yes"]);
    // Unclosed fence: code up to the end.
    assert!(names("text\n```\n#no\nmore #no").is_empty());
    // Indented code block.
    assert_eq!(names("para\n\n    #no in code\n    more #no\n\n#yes"), ["yes"]);
    // Indented continuation of a paragraph is not code.
    assert_eq!(names("para\n    still para #yes"), ["yes"]);
    // Inline triple backticks are a code span, not a fence.
    assert_eq!(names("```#no``` #yes"), ["yes"]);
}

#[test]
fn links_are_skipped() {
    assert_eq!(names("[[Note#Heading]] #ok"), ["ok"]);
    assert_eq!(names("[[Note #x|alias #y]] and ![[img.png #z]] #ok"), ["ok"]);
    assert_eq!(names("[text #a](http://x.org/p#b \"title #c\") #d"), ["d"]);
    assert_eq!(names("![alt #a](img.png) #d"), ["d"]);
    assert_eq!(names("[text #a][ref #b] #d"), ["d"]);
    assert_eq!(names("[a (b) #x](url(1) #y) #d"), ["d"]);
    // Plain brackets are ordinary text.
    assert_eq!(names("- [ ] todo #task"), ["task"]);
    assert_eq!(names("[note #inside] and [^1] #after"), ["inside", "after"]);
    assert_eq!(names("[[unclosed #a"), ["a"]);
}

#[test]
fn html_is_skipped() {
    assert_eq!(names("<span style=\"color: #fff\">x</span> #ok"), ["ok"]);
    assert_eq!(names("<!-- #hidden --> #ok"), ["ok"]);
    assert_eq!(names("a < b #ok"), ["ok"]);
}

#[test]
fn trailing_punctuation_ends_the_tag() {
    assert_eq!(names("Fin de phrase #tag. Puis #autre, et #oui! #quoi? #fin"), ["tag", "autre", "oui", "quoi", "fin"]);
    assert_eq!(names("#a: #b; #c) #d] #e\" #f' #g… #h»"), ["a", "b", "c", "d", "e", "f", "g", "h"]);
}

#[test]
fn nested_tags_and_slashes() {
    assert_eq!(names("#a/b/c"), ["a/b/c"]);
    assert_eq!(names("#a/b/ next"), ["a/b"]);
    assert_eq!(names("#a//b"), ["a"]);
    assert_eq!(names("#/lead"), ["lead"]);
    assert!(names("#/ #// #").is_empty());
    assert_eq!(names("#projet/rocksy/v0-1"), ["projet/rocksy/v0-1"]);
}

#[test]
fn unicode_tags() {
    assert_eq!(names("#café #été/août #日本語 #Ünï"), ["café", "été/août", "日本語", "Ünï"]);
    // Decomposed accent (e + combining acute).
    assert_eq!(names("#cafe\u{301} x"), ["cafe\u{301}"]);
    // Emoji tags, as in Obsidian.
    assert_eq!(names("#🔥 #idée💡 #👨‍👩‍👧"), ["🔥", "idée💡", "👨‍👩‍👧"]);
    // Typographic punctuation ends a tag.
    assert_eq!(names("#a’s #b—c #d”"), ["a", "b", "d"]);
}

#[test]
fn tag_col_is_in_utf16_units() {
    // U+1F389 is two UTF-16 units and four UTF-8 bytes.
    assert_eq!(tags("🎉 #fête"), [("fête".to_string(), 0, 3)]);
    // "é" is one unit (two bytes), "日本" two units (six bytes).
    assert_eq!(tags("é日本 #x 🎉🎉 #y"), [("x".to_string(), 0, 4), ("y".to_string(), 0, 12)]);
    // A byte-order mark counts for one unit on the first line.
    assert_eq!(tags("\u{feff}#x #y\n#z"), [("x".to_string(), 0, 1), ("y".to_string(), 0, 4), ("z".to_string(), 1, 0)]);
}

#[test]
fn line_endings_follow_codemirror() {
    assert_eq!(split_lines("a\nb\r\nc\rd"), ["a", "b", "c", "d"]);
    assert_eq!(split_lines("a\n"), ["a"]);
    assert_eq!(split_lines("a\n\n"), ["a", ""]);
    assert_eq!(split_lines("\r\n\r\nx"), ["", "", "x"]);
    assert!(split_lines("").is_empty());
    let crlf = tags("a #x\r\nb\r\n\r\nc #y\r\n");
    assert_eq!(crlf, [("x".to_string(), 0, 2), ("y".to_string(), 3, 2)]);
    assert_eq!(tags("a\r#x"), [("x".to_string(), 1, 0)]);
    // The carriage return is not part of the tag.
    assert_eq!(names("#end\r\n"), ["end"]);
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[test]
fn paragraph_fragment() {
    let text = "intro\n\npara line 1\nline 2 #t\nline 3\n\nnext";
    assert_eq!(span(text, "t"), (2, 4));
    assert_eq!(span("only #one", "one"), (0, 0));
    // Two tags of one paragraph share the block.
    let text = "a #x\nb #y\n\nc";
    assert_eq!(span(text, "x"), (0, 1));
    assert_eq!(span(text, "y"), (0, 1));
    // A whitespace-only line separates paragraphs.
    assert_eq!(span("a\n  \nb #t\n\t\nc", "t"), (2, 2));
}

#[test]
fn list_item_includes_its_children() {
    let text = "\
- parent #p
  - child one
    - grandchild #g
  - child two #c
- sibling #s

after";
    assert_eq!(span(text, "p"), (0, 3));
    assert_eq!(span(text, "g"), (2, 2));
    assert_eq!(span(text, "c"), (3, 3));
    assert_eq!(span(text, "s"), (4, 4));
}

#[test]
fn list_item_with_tabs_ordered_markers_and_tasks() {
    let text = "1. one #o\n\t- sub a\n\t\t* deep #d\n\t- sub b\n2. two\n10) ten #t\n+ plus\n- [x] done #k";
    assert_eq!(span(text, "o"), (0, 3));
    assert_eq!(span(text, "d"), (2, 2));
    assert_eq!(span(text, "t"), (5, 5));
    assert_eq!(span(text, "k"), (7, 7));
}

#[test]
fn list_item_continuations() {
    // Indented paragraph after a blank line still belongs to the item.
    let text = "- item #a\n\n  continuation\n\n- next #b\n\npara";
    assert_eq!(span(text, "a"), (0, 2));
    assert_eq!(span(text, "b"), (4, 4));
    // Lazy continuation line (no indent, no blank line).
    let text = "- item\nlazy #l\n\npara #p";
    assert_eq!(span(text, "l"), (0, 1));
    assert_eq!(span(text, "p"), (3, 3));
    // A tag on a continuation line of a child belongs to the child.
    let text = "- parent\n  - child\n    more child #c\n  - other\n";
    assert_eq!(span(text, "c"), (1, 2));
    // A tag in the parent's own continuation covers the whole parent.
    let text = "- parent\n\n  parent text #p\n\n  - child\n- next";
    assert_eq!(span(text, "p"), (0, 4));
}

#[test]
fn list_ends_at_unindented_block() {
    assert_eq!(span("- a #x\n# Heading\ntext", "x"), (0, 0));
    assert_eq!(span("- a #x\n  - b\n\npara #p", "x"), (0, 1));
    assert_eq!(span("- a #x\n  - b\n\npara #p", "p"), (3, 3));
    assert_eq!(span("- a #x\n---\n- b", "x"), (0, 0));
    assert_eq!(span("- a #x\n> quote\n", "x"), (0, 0));
    // Dedent to an intermediate level closes the deeper items only.
    let text = "- a #a\n    - b #b\n  - c #c\n- d";
    assert_eq!(span(text, "a"), (0, 2));
    assert_eq!(span(text, "b"), (1, 1));
    assert_eq!(span(text, "c"), (2, 2));
}

#[test]
fn code_inside_a_list_item() {
    let text = "- item\n  ```\n  #no\n  ```\n  after #yes\n- next";
    assert_eq!(names(text), ["yes"]);
    assert_eq!(span(text, "yes"), (0, 4));
    // Fence opened on the marker line.
    assert!(names("- ```\n  #no\n  ```").is_empty());
}

#[test]
fn heading_is_its_own_fragment() {
    let text = "# Title #h1\ntext under #p\n\n### Sub #h3 ###\nmore";
    assert_eq!(span(text, "h1"), (0, 0));
    assert_eq!(span(text, "p"), (1, 1));
    assert_eq!(span(text, "h3"), (3, 3));
    assert_eq!(tags("## Title #t")[0], ("t".to_string(), 0, 9));
    // Seven hashes are not a heading, and not a tag either.
    assert!(names("####### x").is_empty());
}

#[test]
fn table_row_is_the_fragment() {
    let text = "\
| a | b #head |
|---|:-:|
| x #t1 | y |
| z | #t2 |

para #p";
    assert_eq!(span(text, "head"), (0, 0));
    assert_eq!(span(text, "t1"), (2, 2));
    assert_eq!(span(text, "t2"), (3, 3));
    assert_eq!(span(text, "p"), (5, 5));
    // A pipe without a delimiter row is just a paragraph.
    assert_eq!(span("a | b #x\nc | d", "x"), (0, 1));
    // The table ends at the first line without a pipe.
    let text = "h | i\n--|--\nr #r | s\nplain #q\nstill";
    assert_eq!(span(text, "r"), (2, 2));
    assert_eq!(span(text, "q"), (3, 4));
}

#[test]
fn quote_and_callout_are_whole_fragments() {
    let text = "\
text

> [!note] Title #c1
> body
> more #c2
>
> last

after #a";
    assert_eq!(span(text, "c1"), (2, 6));
    assert_eq!(span(text, "c2"), (2, 6));
    assert_eq!(span(text, "a"), (8, 8));
    // Tag right after the marker, nested quote, and code inside a quote.
    assert_eq!(names(">#direct\n> > #nested"), ["direct", "nested"]);
    let text = "> ```\n> #no\n> ```\n> #yes";
    assert_eq!(names(text), ["yes"]);
    assert_eq!(span(text, "yes"), (0, 3));
    // Two quotes separated by a blank line are two fragments.
    let text = "> one #q1\n\n> two #q2";
    assert_eq!(span(text, "q1"), (0, 0));
    assert_eq!(span(text, "q2"), (2, 2));
}

#[test]
fn thematic_breaks_split_blocks() {
    let text = "a #x\n***\nb #y\n- - -\nc #z";
    assert_eq!(span(text, "x"), (0, 0));
    assert_eq!(span(text, "y"), (2, 2));
    assert_eq!(span(text, "z"), (4, 4));
}

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

#[test]
fn frontmatter_block_list() {
    let text = "---\ntitle: x\ntags:\n  - alpha\n  - \"#beta\"\n  - gamma/delta\n---\n# Title\nbody #inline\n";
    let all = frags(text);
    let fm: Vec<_> = all.iter().filter(|f| f.source == TagSource::Frontmatter).collect();
    assert_eq!(fm.iter().map(|f| f.tag.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "gamma/delta"]);
    for f in &fm {
        assert_eq!((f.start_line, f.end_line), (0, 8), "whole file");
        assert_eq!(f.excerpt, "# Title body #inline");
    }
    assert_eq!((fm[0].tag_line, fm[0].tag_col), (3, 4));
    assert_eq!((fm[1].tag_line, fm[1].tag_col), (4, 5));
    assert_eq!((fm[2].tag_line, fm[2].tag_col), (5, 4));
    let inline = all.iter().find(|f| f.tag == "inline").unwrap();
    assert_eq!(inline.source, TagSource::Inline);
    assert_eq!((inline.start_line, inline.end_line, inline.tag_line, inline.tag_col), (8, 8, 8, 5));
    assert_eq!(all.len(), 4);
}

#[test]
fn frontmatter_inline_list() {
    let text = "---\ntags: [a, b/c, \"d\", '#e', 2024, v2]\n---\n";
    let all = frags(text);
    assert_eq!(all.iter().map(|f| f.tag.as_str()).collect::<Vec<_>>(), ["a", "b/c", "d", "e", "v2"]);
    assert!(all.iter().all(|f| f.source == TagSource::Frontmatter && f.start_line == 0 && f.end_line == 2));
    assert_eq!(all[0].tag_col, 7);
    assert_eq!(all[1].tag_col, 10);
    assert_eq!(all[2].tag_col, 16);
    assert_eq!(all[3].tag_col, 21);
    assert!(all.iter().all(|f| f.excerpt.is_empty()));
    // List spanning several lines, followed by another key.
    assert_eq!(names("---\ntags: [a,\n  b, # comment\n  c]\nother: [zzz]\n---\n"), ["a", "b", "c"]);
    assert!(names("---\ntags: []\n---\n").is_empty());
}

#[test]
fn frontmatter_comma_string() {
    assert_eq!(names("---\ntags: one, two,three\n---\ntext"), ["one", "two", "three"]);
    assert_eq!(names("---\ntags: \"x, y/z\"\n---\n"), ["x", "y/z"]);
    assert_eq!(names("---\ntags: 'p,q'\n---\n"), ["p", "q"]);
    assert_eq!(names("---\ntag: solo\n---\n"), ["solo"]);
    assert_eq!(names("---\nTags: Upper\n---\n"), ["Upper"]);
    assert_eq!(names("---\ntags: a b\n---\n"), ["a", "b"]);
    let f = &frags("---\ntags: été, 🎉x\n---\n")[1];
    assert_eq!((f.tag.as_str(), f.tag_line, f.tag_col), ("🎉x", 1, 11));
}

#[test]
fn frontmatter_yaml_comments_and_invalid_values() {
    // An unquoted `#` starts a YAML comment.
    assert!(names("---\ntags: #a\n---\n").is_empty());
    assert_eq!(names("---\ntags: a # comment #b\n---\n"), ["a"]);
    assert_eq!(names("---\ntags:\n  - a # note\n  # comment line\n  - b\n---\n"), ["a", "b"]);
    assert!(names("---\ntags:\n---\n").is_empty());
    assert!(names("---\ntags: 123, 4/5!\n---\n").is_empty());
    // Zero-indent block list, as written by Obsidian.
    assert_eq!(names("---\ntags:\n- a\n- b\naliases:\n- not-a-tag\n---\n"), ["a", "b"]);
}

#[test]
fn frontmatter_only_top_level_tags_key() {
    assert!(names("---\ntitle: x #notag\naliases: [tags, tag]\nmeta:\n  tags: [nested]\nmytags: [no]\n---\n").is_empty());
    // Inline tags are not searched inside the frontmatter.
    assert_eq!(names("---\ndescription: about #this\n---\nbody #that"), ["that"]);
}

#[test]
fn frontmatter_must_open_the_file_and_be_closed() {
    assert_eq!(names("---\ntags: a\n\ntext #b"), ["b"]);
    assert!(names("text\n---\ntags: a\n---\n").is_empty());
    assert!(names("\n---\ntags: a\n---\n").is_empty());
    assert_eq!(names("---\ntags: a\n...\nbody"), ["a"]);
    // Byte-order mark and CRLF line endings.
    let f = frags("\u{feff}---\r\ntags: [x]\r\n---\r\nbody\r\n");
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].tag.as_str(), f[0].start_line, f[0].end_line, f[0].tag_line, f[0].tag_col), ("x", 0, 3, 1, 7));
    assert_eq!(f[0].excerpt, "body");
}

#[test]
fn frontmatter_and_inline_occurrences_are_both_reported() {
    let all = frags("---\ntags: [x]\n---\n\npara #x here\n");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].source, TagSource::Frontmatter);
    assert_eq!((all[0].start_line, all[0].end_line), (0, 4));
    assert_eq!(all[1].source, TagSource::Inline);
    assert_eq!((all[1].start_line, all[1].end_line), (4, 4));
}

// ---------------------------------------------------------------------------
// Excerpts and JSON
// ---------------------------------------------------------------------------

#[test]
fn excerpt_is_one_line_and_bounded() {
    let f = &frags("- item   with\tspaces #t\n  - child\n    line")[0];
    assert_eq!(f.excerpt, "- item with spaces #t - child line");

    let long = format!("start #t {}", "mot ".repeat(200));
    let f = &frags(&long)[0];
    assert_eq!(f.excerpt.chars().count(), EXCERPT_MAX);
    assert!(f.excerpt.starts_with("start #t mot"));
    assert!(f.excerpt.ends_with('…'));
    assert!(!f.excerpt.contains('\n'));

    // Exactly at the limit: no ellipsis.
    let exact = format!("#t {}", "x".repeat(EXCERPT_MAX - 3));
    assert_eq!(frags(&exact)[0].excerpt, exact);
    let over = format!("#t {}", "x".repeat(EXCERPT_MAX - 2));
    let e = &frags(&over)[0].excerpt;
    assert_eq!(e.chars().count(), EXCERPT_MAX);
    assert!(e.ends_with('…'));
}

#[test]
fn excerpt_is_centred_on_a_distant_tag() {
    // Far down a long paragraph.
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("ligne numéro {i} du paragraphe\n"));
    }
    text.push_str("la ligne avec #cible ici\nsuite");
    let f = &frags(&text)[0];
    assert_eq!((f.start_line, f.end_line, f.tag_line), (0, 31, 30));
    assert!(f.excerpt.starts_with("…la ligne avec #cible ici suite"), "{}", f.excerpt);

    // Far inside one very long line (multi-byte characters before the tag).
    let line = format!("{} avant #loin après {}", "é".repeat(600), "z".repeat(600));
    let f = &frags(&line)[0];
    assert!(f.excerpt.starts_with('…'));
    assert!(f.excerpt.contains("avant #loin après"), "{}", f.excerpt);
    assert!(f.excerpt.chars().count() <= EXCERPT_MAX);

    // A tag near the start and one far away in the same block.
    let text = format!("#near {} #far end", "w ".repeat(300));
    let all = frags(&text);
    assert!(all[0].excerpt.starts_with("#near w w"));
    assert!(all[1].excerpt.contains("#far end"));
}

#[test]
fn fragment_json_matches_the_contract() {
    let f = &file_tags("/v/n.md", "---\ntags: a\n---\nx #b/c")[..];
    let json = serde_json::to_value(f).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            {"path": "/v/n.md", "tag": "a", "source": "frontmatter", "startLine": 0, "endLine": 3,
             "tagLine": 1, "tagCol": 6, "excerpt": "x #b/c"},
            {"path": "/v/n.md", "tag": "b/c", "source": "inline", "startLine": 3, "endLine": 3,
             "tagLine": 3, "tagCol": 2, "excerpt": "x #b/c"}
        ])
    );
    let back: Vec<Fragment> = serde_json::from_value(json).unwrap();
    assert_eq!(back, f);
}

#[test]
fn large_document_is_analysed_consistently() {
    // 20 000 lines mixing every construct; every 10th block carries a tag.
    let mut text = String::from("---\ntags: [big]\n---\n");
    let mut expected = 1;
    for i in 0..2_000 {
        text.push_str(&format!("## Section {i}\n\nParagraphe {i} sans tag\nsuite\n\n- item {i}\n  - enfant\n\n```\n#code{i}\n```\n\n"));
        if i % 10 == 0 {
            text.push_str(&format!("texte #t/{i} fin\n\n"));
            expected += 1;
        }
    }
    let all = frags(&text);
    assert_eq!(all.len(), expected);
    let lines = split_lines(&text);
    for f in &all[1..] {
        assert_eq!(f.start_line, f.tag_line);
        assert!(lines[f.tag_line as usize][f.tag_col as usize..].starts_with(&format!("#{}", f.tag)));
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

fn sample_vault(dir: &TempDir) -> std::path::PathBuf {
    dir.write("v/a.md", "---\ntags: [projet, Projet/Rocksy]\n---\n# A\n\n- todo #projet/rocksy/core\n  - sous #idee\n");
    dir.write("v/b/b1.md", "para #idee et #Idee/brillante\n\nautre #misc\n");
    dir.write("v/b/b2.md", "rien ici\n");
    dir.write("v/c.md", "#projet/autre\n\n> [!tip] #misc\n> #recette/dessert\n");
    dir.write("v/.obsidian/workspace.md", "#cache\n");
    dir.write("v/.trash/old.md", "#poubelle\n");
    dir.write("v/notes.txt", "#texte\n");
    dir.write("v/latin1.md", [b'#', b'x', b' ', 0xE9, b'\n']);
    dir.path().join("v")
}

fn build(index: &TagIndex, root: &std::path::Path) -> TagsReady {
    index.build(root, BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap().unwrap()
}

fn listed(index: &TagIndex, q: &str) -> Vec<(String, u32)> {
    index.tags_list(q).into_iter().map(|t| (t.tag, t.count)).collect()
}

fn s(pairs: &[(&str, u32)]) -> Vec<(String, u32)> {
    pairs.iter().map(|(t, c)| (t.to_string(), *c)).collect()
}

#[test]
fn index_build_counts_and_hierarchy() {
    let dir = TempDir::new("index");
    let root = sample_vault(&dir);
    let index = TagIndex::new();
    assert!(index.tags_list("").is_empty());
    assert!(index.tag_fragments("x", None).is_empty());

    let ready = build(&index, &root);
    // a.md, b1.md, b2.md, c.md, latin1.md (hidden folders and .txt excluded).
    assert_eq!(ready.files, 5);
    assert_eq!(ready.occurrences, 10);
    let stats = index.stats();
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.files, 3);
    assert_eq!(stats.occurrences, 10);
    assert_eq!(stats.tags, ready.tags);

    // Parents aggregate their descendants; case of the first occurrence
    // (tree order: folder b first, then a.md, c.md).
    assert_eq!(
        listed(&index, ""),
        s(&[
            ("projet", 4),
            ("idee", 3),
            ("misc", 2),
            ("Projet/Rocksy", 2),
            ("Idee/brillante", 1),
            ("projet/autre", 1),
            ("projet/rocksy/core", 1),
            ("recette", 1),
            ("recette/dessert", 1),
        ])
    );
    assert_eq!(ready.tags, 9);
}

#[test]
fn tags_list_filters_and_ranks() {
    let dir = TempDir::new("index-list");
    let root = sample_vault(&dir);
    let index = TagIndex::new();
    build(&index, &root);

    // Case-insensitive, `#` ignored, prefix matches first.
    assert_eq!(
        listed(&index, "PRO"),
        s(&[("projet", 4), ("Projet/Rocksy", 2), ("projet/autre", 1), ("projet/rocksy/core", 1)])
    );
    assert_eq!(listed(&index, "#idee"), s(&[("idee", 3), ("Idee/brillante", 1)]));
    // Prefix of the tag, then prefix of a level, then plain substring.
    assert_eq!(
        listed(&index, "r"),
        s(&[
            ("recette", 1),
            ("recette/dessert", 1),
            ("Projet/Rocksy", 2),
            ("projet/rocksy/core", 1),
            ("projet", 4),
            ("Idee/brillante", 1),
            ("projet/autre", 1),
        ])
    );
    assert_eq!(listed(&index, "dess"), s(&[("recette/dessert", 1)]));
    assert_eq!(listed(&index, "ocks"), s(&[("Projet/Rocksy", 2), ("projet/rocksy/core", 1)]));
    assert!(listed(&index, "zzz").is_empty());
    assert_eq!(listed(&index, "  misc "), s(&[("misc", 2)]));
}

#[test]
fn tag_fragments_include_descendants_and_respect_limit() {
    let dir = TempDir::new("index-frag");
    let root = sample_vault(&dir);
    let index = TagIndex::new();
    build(&index, &root);
    let rel = |f: &Fragment| f.path[root.to_str().unwrap().len() + 1..].replace('\\', "/");

    let all = index.tag_fragments("projet", None);
    let got: Vec<_> = all.iter().map(|f| (rel(f), f.tag.clone(), f.tag_line)).collect();
    assert_eq!(
        got,
        [
            ("a.md".to_string(), "projet".to_string(), 1),
            ("a.md".to_string(), "Projet/Rocksy".to_string(), 1),
            ("a.md".to_string(), "projet/rocksy/core".to_string(), 5),
            ("c.md".to_string(), "projet/autre".to_string(), 0),
        ]
    );
    assert_eq!(all[0].source, TagSource::Frontmatter);
    assert_eq!((all[0].start_line, all[0].end_line), (0, 6));
    assert_eq!((all[2].start_line, all[2].end_line, all[2].tag_col), (5, 6, 7));
    assert_eq!(all[2].excerpt, "- todo #projet/rocksy/core - sous #idee");
    assert!(std::path::Path::new(&all[0].path).is_absolute());

    // Case-insensitive lookup, `#` accepted, intermediate level.
    assert_eq!(index.tag_fragments("#PROJET/ROCKSY", None).len(), 2);
    assert_eq!(index.tag_fragments("idee", None).len(), 3);
    assert_eq!(index.tag_fragments("recette", None).len(), 1);
    assert!(index.tag_fragments("proj", None).is_empty(), "exact tag, not a prefix");

    // The limit keeps the first fragments in order.
    let two = index.tag_fragments("projet", Some(2));
    assert_eq!(two, all[..2]);
    assert!(index.tag_fragments("projet", Some(0)).is_empty());
    assert_eq!(index.tag_fragments("projet", Some(100)), all);

    // Counts in the list equal the number of fragments returned.
    for t in index.tags_list("") {
        assert_eq!(index.tag_fragments(&t.tag, None).len() as u32, t.count, "{}", t.tag);
    }
}

#[test]
fn update_file_reflects_a_save() {
    let dir = TempDir::new("index-update");
    let root = sample_vault(&dir);
    let index = TagIndex::new();
    build(&index, &root);
    let before = index.stats();

    // Rewrite c.md in memory: one tag removed, one added, one kept.
    let c = root.join("c.md");
    assert!(index.update_file(&c, "#projet/autre et #nouveau/tag\n"));
    assert_eq!(listed(&index, "nouveau"), s(&[("nouveau", 1), ("nouveau/tag", 1)]));
    assert_eq!(listed(&index, "misc"), s(&[("misc", 1)]));
    assert!(listed(&index, "recette").is_empty(), "tags without occurrence disappear");
    assert_eq!(listed(&index, "projet")[0], ("projet".to_string(), 4));
    assert_eq!(index.stats().occurrences, before.occurrences - 3 + 2);
    let f = index.tag_fragments("nouveau", None);
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].tag.as_str(), f[0].tag_col), ("nouveau/tag", 17));

    // Same content again: idempotent.
    assert!(index.update_file(&c, "#projet/autre et #nouveau/tag\n"));
    assert_eq!(index.stats().occurrences, before.occurrences - 1);

    // A file that had no tag gets one; a new file is added.
    assert!(index.update_file(&root.join("b/b2.md"), "maintenant #misc\n"));
    assert!(index.update_file(&root.join("new/fresh.md"), "- #misc\n"));
    assert_eq!(listed(&index, "misc"), s(&[("misc", 3)]));
    assert_eq!(index.tag_fragments("misc", None).len(), 3);

    // Emptying a file removes its occurrences.
    assert!(index.update_file(&c, ""));
    assert!(listed(&index, "nouveau").is_empty());
    index.remove_file(&root.join("new/fresh.md"));
    assert_eq!(listed(&index, "misc"), s(&[("misc", 2)]));

    // Files that do not belong in the index are refused.
    assert!(!index.update_file(&root.join("notes.txt"), "#x"));
    assert!(!index.update_file(&root.join(".obsidian/x.md"), "#x"));
    assert!(!index.update_file(&root.join("sub/.hidden/x.md"), "#x"));
    assert!(!index.update_file(&dir.path().join("outside.md"), "#x"));
    assert!(listed(&index, "x").is_empty());

    // Rebuilding from disk gives the original content back.
    let ready = build(&index, &root);
    assert_eq!(ready.occurrences, before.occurrences);
    assert_eq!(index.stats(), before);

    index.clear();
    assert!(index.tags_list("").is_empty());
}

#[test]
fn build_reports_progress_and_can_be_cancelled() {
    let dir = TempDir::new("index-progress");
    for i in 0..300 {
        dir.write(&format!("v/d{}/n{i}.md", i % 7), format!("note {i} #t/{}\n", i % 5));
    }
    let root = dir.path().join("v");
    let index = TagIndex::new();

    let events = Mutex::new(Vec::new());
    let ready = index
        .build(&root, BuildOptions { threads: Some(3) }, &CancelToken::new(), |p| {
            events.lock().unwrap().push(p)
        })
        .unwrap()
        .unwrap();
    assert_eq!((ready.files, ready.occurrences, ready.tags), (300, 300, 6));
    let events = events.into_inner().unwrap();
    assert_eq!(events.first(), Some(&TagsProgress { done: 0, total: 300 }));
    assert_eq!(events.last(), Some(&TagsProgress { done: 300, total: 300 }));
    assert!(events.len() > 3);
    assert!(events.windows(2).all(|w| w[0].done <= w[1].done));
    assert!(events.iter().all(|p| p.total == 300));

    // Cancelled before it starts: nothing replaced.
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(index.build(&root, BuildOptions::default(), &cancel, |_| {}).unwrap(), None);
    assert_eq!(index.stats().occurrences, 300);

    // Cancelled from the progress callback, midway.
    let cancel = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let res = index
        .build(&root, BuildOptions { threads: Some(2) }, &cancel, |_| {
            if calls.fetch_add(1, Ordering::Relaxed) == 2 {
                cancel.cancel();
            }
        })
        .unwrap();
    assert_eq!(res, None);
    assert_eq!(index.stats().occurrences, 300, "previous content kept");

    // A missing vault is an error, the index is untouched.
    let err = index.build(&dir.path().join("nope"), BuildOptions::default(), &CancelToken::new(), |_| {});
    assert_eq!(err.unwrap_err().kind, crate::ErrorKind::NotFound);
    assert_eq!(index.stats().occurrences, 300);
}

#[test]
fn build_is_deterministic_across_thread_counts() {
    let dir = TempDir::new("index-det");
    for i in 0..120 {
        let case = if i % 2 == 0 { "Sujet/Alpha" } else { "sujet/alpha" };
        dir.write(&format!("v/n{i:03}.md"), format!("---\ntags: [{case}]\n---\n- a #x/{}\n  - b #Y\n", i % 4));
    }
    let root = dir.path().join("v");
    let snapshot = |threads: usize| {
        let index = TagIndex::new();
        index.build(&root, BuildOptions { threads: Some(threads) }, &CancelToken::new(), |_| {}).unwrap();
        (index.tags_list(""), index.tag_fragments("sujet", Some(50)), index.tag_fragments("x", None))
    };
    let one = snapshot(1);
    assert_eq!(one.0[0], TagCount { tag: "Sujet".into(), count: 120 });
    for threads in [2, 8, 32] {
        assert_eq!(snapshot(threads), one, "{threads} threads");
    }
}

#[test]
fn updates_during_a_build_are_not_lost() {
    let dir = TempDir::new("index-race");
    for i in 0..200 {
        dir.write(&format!("v/n{i}.md"), format!("#base/{}\n", i % 3));
    }
    let root = dir.path().join("v");
    let index = TagIndex::new();
    let target = root.join("n5.md");
    let fired = AtomicUsize::new(0);
    // The save arrives while the build is running (from its progress callback).
    index
        .build(&root, BuildOptions { threads: Some(2) }, &CancelToken::new(), |p| {
            if p.done > 0 && fired.fetch_add(1, Ordering::Relaxed) == 0 {
                index.update_file(&target, "#saved/during-build\n");
                index.remove_file(&root.join("n6.md"));
            }
        })
        .unwrap();
    assert_eq!(listed(&index, "saved"), s(&[("saved", 1), ("saved/during-build", 1)]));
    // n5 (base/2) replaced, n6 (base/0) removed.
    assert_eq!(index.stats().occurrences, 199);
    assert_eq!(listed(&index, "base")[0], ("base".to_string(), 198));
}

#[test]
fn index_never_writes_to_the_vault() {
    let dir = TempDir::new("index-ro");
    let root = sample_vault(&dir);
    let before = all_files(&root);
    let index = TagIndex::new();
    build(&index, &root);
    index.update_file(&root.join("c.md"), "#x");
    index.tags_list("");
    index.tag_fragments("projet", None);
    assert_eq!(all_files(&root), before);
}
