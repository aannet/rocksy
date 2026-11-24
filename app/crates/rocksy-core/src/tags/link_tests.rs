//! Link extraction: every form of the contract, the traps, and positions.

use super::parse::{parse, RawLink};

fn links(text: &str) -> Vec<RawLink> {
    parse(text).links
}

/// (target, heading, alias, embed, markdown) of a link.
type Shape = (String, Option<String>, Option<String>, bool, bool);

fn shapes(text: &str) -> Vec<Shape> {
    links(text).into_iter().map(|l| (l.target, l.heading, l.alias, l.embed, l.markdown)).collect()
}

fn s(target: &str, heading: Option<&str>, alias: Option<&str>, embed: bool, markdown: bool) -> Shape {
    (target.into(), heading.map(Into::into), alias.map(Into::into), embed, markdown)
}

fn targets(text: &str) -> Vec<String> {
    links(text).into_iter().map(|l| l.target).collect()
}

fn slice16(text: &str, range: (u32, u32)) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[range.0 as usize..range.1 as usize]).unwrap()
}

#[test]
fn every_form_of_the_contract() {
    assert_eq!(shapes("[[cible]]"), [s("cible", None, None, false, false)]);
    assert_eq!(shapes("[[cible#titre]]"), [s("cible", Some("titre"), None, false, false)]);
    assert_eq!(shapes("[[cible#^bloc]]"), [s("cible", Some("^bloc"), None, false, false)]);
    assert_eq!(shapes("[[cible|alias]]"), [s("cible", None, Some("alias"), false, false)]);
    assert_eq!(shapes("[[dossier/Ma note#Un titre|un alias]]"), [s("dossier/Ma note", Some("Un titre"), Some("un alias"), false, false)]);
    assert_eq!(shapes("![[image.png]]"), [s("image.png", None, None, true, false)]);
    assert_eq!(shapes("![[note#section]] et ![[doc.pdf|200]]"), [s("note", Some("section"), None, true, false), s("doc.pdf", None, Some("200"), true, false)]);
    assert_eq!(shapes("[texte](chemin.md)"), [s("chemin.md", None, Some("texte"), false, true)]);
    assert_eq!(shapes("![alt](img/chemin.png)"), [s("img/chemin.png", None, Some("alt"), true, true)]);
    assert_eq!(shapes("[t](../a/Ma%20note.md#Titre \"infobulle\")"), [s("../a/Ma note.md", Some("Titre"), Some("t"), false, true)]);
    assert_eq!(shapes("![](photo.jpg)"), [s("photo.jpg", None, None, true, true)]);
}

#[test]
fn urls_and_in_note_links_are_ignored() {
    for text in [
        "[site](https://example.com/page.md)",
        "[site](http://example.com)",
        "[mail](mailto:a@b.c)",
        "![logo](https://example.com/logo.png)",
        "[app](obsidian://open?vault=x&file=y)",
        "[[#titre interne]]",
        "[[#^bloc|alias]]",
        "[ancre](#titre)",
        "[ref][1] et [^note]",
        "[texte] (pas un lien)",
        "[[]] [[ ]] [[|x]]",
        "<https://example.com/[[x]]>",
    ] {
        assert!(links(text).is_empty(), "{text}");
    }
    assert_eq!(targets("[url](https://x.org) puis [[Vrai]] et [local](a.md)"), ["Vrai", "a.md"]);
}

#[test]
fn code_is_not_scanned() {
    assert_eq!(targets("`[[code]]` puis [[vrai]]"), ["vrai"]);
    assert_eq!(targets("``[x](a.md) ` encore`` [[vrai]]"), ["vrai"]);
    assert_eq!(targets("```\n[[dans un bloc]]\n[x](y.md)\n```\n[[apres]]"), ["apres"]);
    assert_eq!(targets("~~~md\n![[img.png]]\n~~~\n\n![[vrai.png]]"), ["vrai.png"]);
    assert_eq!(targets("texte\n\n    [[code indenté]]\n\n[[vrai]]"), ["vrai"]);
    assert_eq!(targets("- item\n  ```\n  [[non]]\n  ```\n  [[oui]]"), ["oui"]);
    assert_eq!(targets("> ```\n> [[non]]\n> ```\n> [[oui]]"), ["oui"]);
    // Frontmatter is not body text.
    assert_eq!(targets("---\nlien: \"[[dans les propriétés]]\"\n---\n[[corps]]"), ["corps"]);
}

#[test]
fn links_are_found_in_every_kind_of_block() {
    let text = "# Titre [[h]]\n\npara [[p1]] et [[p2]]\n\n- item [[l1]]\n  - sous ![[l2.png]]\n\n> [!note] [[c1]]\n> suite [m](c2.md)\n\n| a | b |\n|---|---|\n| [[t1\\|alias]] | [[t2]] |\n\n1. [[o1]]";
    assert_eq!(targets(text), ["h", "p1", "p2", "l1", "l2.png", "c1", "c2.md", "t1", "t2", "o1"]);
    let all = links(text);
    assert_eq!(all[7].alias.as_deref(), Some("alias"), "escaped pipe in a table");
    assert_eq!(all.iter().map(|l| l.line).collect::<Vec<_>>(), [0, 2, 2, 4, 5, 7, 8, 12, 12, 14]);
    // Without any `#` in the text, links are still found.
    assert_eq!(targets("simple [[lien]]"), ["lien"]);
}

#[test]
fn tags_and_links_coexist() {
    let parsed = parse("voir [[Note#Titre]] #tag et [x](a.md#b) #autre\n");
    assert_eq!(parsed.occs.iter().map(|o| o.tag).collect::<Vec<_>>(), ["tag", "autre"]);
    assert_eq!(parsed.links.len(), 2);
    assert_eq!(parsed.links[0].heading.as_deref(), Some("Titre"));
    assert_eq!(parsed.links[1].heading.as_deref(), Some("b"));
}

#[test]
fn positions_are_utf16_columns_on_codemirror_lines() {
    // "🎉" is two units; the embed starts at its "!".
    let all = links("🎉 [[A]] et ![[b.png]]");
    assert_eq!((all[0].line, all[0].col), (0, 3));
    assert_eq!((all[1].line, all[1].col), (0, 12));

    // CRLF and lone CR end lines; a byte-order mark counts for one unit.
    let all = links("\u{feff}[[bom]] x\r\nligne\r\né [[crlf]]\rfin [m](cr.md)");
    assert_eq!(all.iter().map(|l| (l.target.as_str(), l.line, l.col)).collect::<Vec<_>>(), [("bom", 0, 1), ("crlf", 2, 2), ("cr.md", 3, 4)]);
}

#[test]
fn excerpt_shows_the_line_and_where_the_link_is() {
    let all = links("  - été 🎉 [[Ma note|alias]] puis ![img](a b.png) fin");
    assert_eq!(all[0].excerpt, "- été 🎉 [[Ma note|alias]] puis ![img](a b.png) fin");
    assert_eq!(slice16(&all[0].excerpt, all[0].range), "[[Ma note|alias]]");
    assert_eq!(slice16(&all[1].excerpt, all[1].range), "![img](a b.png)");
    assert_eq!(all[1].target, "a");

    // A link far down a very long line: windowed, still pointing at the link.
    let line = format!("{} [[loin]] {}", "é".repeat(700), "z".repeat(700));
    let link = &links(&line)[0];
    assert!(link.excerpt.chars().count() <= 240);
    assert_eq!(slice16(&link.excerpt, link.range), "[[loin]]");
    assert_eq!(link.col, 701);
}
