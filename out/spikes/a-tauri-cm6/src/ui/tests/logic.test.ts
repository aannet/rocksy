import { describe, expect, it } from "vitest";
import { FileIndex, linkTextFor, parseWikilink, resolveImage, resolveLink } from "../src/resolve";
import { fuzzySearch, makeItem, scoreItem } from "../src/fuzzy";
import { buildTagTree, countTags, filterTags, tagMatches } from "../src/tagtree";
import { basename, dirname, joinAbs, normalizeRel, samePath, toRel } from "../src/paths";
import { listContinuation, parseCalloutHead, parseEmbed, parseTable, renderInlineHtml, splitTableRow } from "../src/editor/syntax";
import { scanTags } from "../src/mock/tagscan";
import { buildFileTree } from "../src/sidebar";
import type { Fragment, TreeEntry } from "../src/types";

const f = (path: string): TreeEntry => ({ path, isDir: false, size: 1, mtimeMs: 1 });
const d = (path: string): TreeEntry => ({ path, isDir: true, size: 0, mtimeMs: 1 });
const index = new FileIndex([
  d("Notes"),
  d("Projets"),
  d("_ASSETS"),
  f("Accueil.md"),
  f("Notes/Idée.md"),
  f("Notes/Plan.md"),
  f("Projets/Plan.md"),
  f("Projets/Alpha/Suivi.md"),
  f("_ASSETS/schema.png"),
  f("Notes/img/local.png"),
  f("Projets/Carte.canvas"),
]);

describe("paths", () => {
  it("handles Windows and POSIX paths", () => {
    expect(basename("C:\\a\\b.md")).toBe("b.md");
    expect(dirname("C:\\a\\b.md")).toBe("C:\\a");
    expect(dirname("/a/b.md")).toBe("/a");
    expect(joinAbs("C:\\Vault\\Notes", "../_ASSETS/x.png")).toBe("C:\\Vault\\_ASSETS\\x.png");
    expect(joinAbs("/home/u/v", "a/b.md")).toBe("/home/u/v/a/b.md");
    expect(toRel("C:\\Vault", "c:\\vault\\Notes\\A.md")).toBe("Notes/A.md");
    expect(toRel("C:\\Vault", "C:\\Vault2\\A.md")).toBeNull();
    expect(samePath("C:\\Vault\\", "c:/vault")).toBe(true);
    expect(normalizeRel("a/./b/../c")).toBe("a/c");
  });
});

describe("wikilinks", () => {
  it("parses target, heading and alias", () => {
    expect(parseWikilink("Note")).toEqual({ target: "Note", heading: null, alias: null });
    expect(parseWikilink("Note|alias")).toEqual({ target: "Note", heading: null, alias: "alias" });
    expect(parseWikilink("Note#Titre")).toEqual({ target: "Note", heading: "Titre", alias: null });
    expect(parseWikilink("Dossier/Note#T 2|a b")).toEqual({ target: "Dossier/Note", heading: "T 2", alias: "a b" });
    expect(parseWikilink("#Titre")).toEqual({ target: "", heading: "Titre", alias: null });
  });
  it("resolves by exact path, relative path, then name", () => {
    expect(resolveLink("Accueil", "Notes/Idée.md", index)).toBe("Accueil.md");
    expect(resolveLink("Projets/Alpha/Suivi", "Accueil.md", index)).toBe("Projets/Alpha/Suivi.md");
    expect(resolveLink("suivi", "Accueil.md", index)).toBe("Projets/Alpha/Suivi.md");
    expect(resolveLink("../Projets/Plan.md", "Notes/Idée.md", index)).toBe("Projets/Plan.md");
    expect(resolveLink("Inconnue", "Accueil.md", index)).toBeNull();
    expect(resolveLink("Carte.canvas", "Accueil.md", index)).toBe("Projets/Carte.canvas");
  });
  it("prefers the same folder for ambiguous names", () => {
    expect(resolveLink("Plan", "Notes/Idée.md", index)).toBe("Notes/Plan.md");
    expect(resolveLink("Plan", "Projets/Alpha/Suivi.md", index)).toBe("Notes/Plan.md");
    expect(resolveLink("Projets/Plan", "Notes/Idée.md", index)).toBe("Projets/Plan.md");
  });
  it("gives a link text that disambiguates duplicates", () => {
    expect(linkTextFor("Accueil.md", index)).toBe("Accueil");
    expect(linkTextFor("Notes/Plan.md", index)).toBe("Notes/Plan");
    expect(linkTextFor("_ASSETS/schema.png", index)).toBe("schema.png");
  });
});

describe("image resolution", () => {
  const ctx = { fileAbs: "C:\\Vault\\Notes\\Idée.md", vaultRoot: "C:\\Vault", index };
  it("tries the path relative to the file first, then the name in the tree", () => {
    expect(resolveImage("img/local.png", ctx)).toEqual({ kind: "local", abs: "C:\\Vault\\Notes\\img\\local.png" });
    expect(resolveImage("schema.png", ctx)).toEqual({ kind: "local", abs: "C:\\Vault\\_ASSETS\\schema.png" });
    expect(resolveImage("../_ASSETS/schema.png", ctx)).toEqual({ kind: "local", abs: "C:\\Vault\\_ASSETS\\schema.png" });
    expect(resolveImage("_ASSETS/schema.png", ctx)).toEqual({ kind: "local", abs: "C:\\Vault\\_ASSETS\\schema.png" });
    expect(resolveImage("absente.png", ctx)).toEqual({ kind: "missing" });
  });
  it("decodes URL escapes and flags remote images", () => {
    expect(resolveImage("img/local.png".replace("/", "%2F"), ctx).kind).toBe("local");
    expect(resolveImage("https://exemple.test/a.png", ctx)).toEqual({ kind: "remote", url: "https://exemple.test/a.png" });
  });
  it("outside a vault, resolves relative to the file without a tree", () => {
    expect(resolveImage("img/a.png", { fileAbs: "/tmp/n/x.md", vaultRoot: null, index: null })).toEqual({ kind: "local", abs: "/tmp/n/img/a.png" });
    expect(resolveImage("D:\\pics\\a.png", { fileAbs: "C:\\n\\x.md", vaultRoot: null, index: null })).toEqual({ kind: "local", abs: "D:\\pics\\a.png" });
  });
});

describe("fuzzy search", () => {
  const items = ["Projets/Plan de test.md", "Notes/Planning 2026.md", "Archives/vieux/plan.md", "Notes/Autre chose.md", "Projets/Alpha/Suivi lot.md"].map(makeItem);
  it("returns -1 when the query is not a subsequence", () => {
    expect(scoreItem("xyz", items[0])).toBe(-1);
    expect(scoreItem("", items[0])).toBe(0);
  });
  it("ranks basename and word-start matches first", () => {
    const hits = fuzzySearch("plan", items);
    expect(hits.map((h) => h.item.text)).toEqual(["Archives/vieux/plan.md", "Projets/Plan de test.md", "Notes/Planning 2026.md"]);
    expect(hits[0].positions.length).toBe(4);
  });
  it("matches all space-separated terms and subsequences", () => {
    expect(fuzzySearch("alpha suivi", items).map((h) => h.item.text)).toEqual(["Projets/Alpha/Suivi lot.md"]);
    expect(fuzzySearch("pdt", items)[0].item.text).toBe("Projets/Plan de test.md");
    expect(fuzzySearch("zzz", items)).toEqual([]);
  });
  it("keeps the best N of a large list", () => {
    const many = Array.from({ length: 5000 }, (_, i) => makeItem(`dir${i % 50}/note ${i}.md`));
    many.push(makeItem("cible/exacte.md"));
    const hits = fuzzySearch("exacte", many, 10);
    expect(hits[0].item.text).toBe("cible/exacte.md");
    expect(fuzzySearch("note", many, 10).length).toBe(10);
  });
});

describe("tag tree", () => {
  it("builds the hierarchy and keeps backend counts", () => {
    const tree = buildTagTree([
      { tag: "projet", count: 5 },
      { tag: "projet/alpha", count: 3 },
      { tag: "projet/alpha/suivi", count: 1 },
      { tag: "idee", count: 2 },
    ]);
    expect(tree.map((n) => n.name)).toEqual(["idee", "projet"]);
    expect(tree[1].count).toBe(5);
    expect(tree[1].children[0].full).toBe("projet/alpha");
    expect(tree[1].children[0].children[0].count).toBe(1);
  });
  it("synthesises missing parents (filtered lists)", () => {
    const tree = buildTagTree([
      { tag: "a/b/c", count: 2 },
      { tag: "a/b/d", count: 3 },
    ]);
    expect(tree[0].full).toBe("a");
    expect(tree[0].count).toBe(5);
    expect(tree[0].children[0].children.map((n) => n.name)).toEqual(["c", "d"]);
  });
  it("is case-insensitive on keys", () => {
    const tree = buildTagTree([
      { tag: "Projet", count: 2 },
      { tag: "projet/X", count: 1 },
    ]);
    expect(tree.length).toBe(1);
    expect(tree[0].name).toBe("Projet");
  });
  it("counts and filters fragments of a single file", () => {
    const frag = (tag: string): Fragment => ({ path: "x", tag, source: "inline", startLine: 0, endLine: 0, tagLine: 0, tagCol: 0, excerpt: "" });
    const counts = countTags([frag("a/b"), frag("a/b"), frag("a"), frag("c")]);
    expect(counts).toEqual([
      { tag: "a", count: 3 },
      { tag: "a/b", count: 2 },
      { tag: "c", count: 1 },
    ]);
    expect(filterTags(counts, "#B").map((t) => t.tag)).toEqual(["a/b"]);
    expect(tagMatches("a/b", "A")).toBe(true);
    expect(tagMatches("ab", "a")).toBe(false);
  });
});

describe("live preview helpers", () => {
  it("parses callout heads", () => {
    expect(parseCalloutHead("> [!note] Titre")).toMatchObject({ type: "note", fold: "", title: "Titre", prefixLen: 2, headLen: 10 });
    expect(parseCalloutHead("> [!WARNING]- Replié")).toMatchObject({ type: "warning", fold: "-" });
    expect(parseCalloutHead(">[!tip]+")).toMatchObject({ type: "tip", fold: "+", title: "" });
    expect(parseCalloutHead("> pas un callout")).toBeNull();
  });
  it("parses tables with alignment, escapes and offsets", () => {
    const src = "| a | b \\| c | `x|y` |\n| :-- | :-: | --: |\n| 1 | 2 | 3 |";
    const t = parseTable(src)!;
    expect(t.header.map((c) => c.text)).toEqual(["a", "b \\| c", "`x|y`"]);
    expect(t.align).toEqual(["left", "center", "right"]);
    expect(t.rows[0].map((c) => c.text)).toEqual(["1", "2", "3"]);
    expect(src.slice(t.rows[0][1].off, t.rows[0][1].off + 1)).toBe("2");
    expect(parseTable("pas | une table\nligne")).toBeNull();
    expect(splitTableRow("a | b", 0).map((c) => c.text)).toEqual(["a", "b"]);
  });
  it("renders inline markdown to escaped HTML", () => {
    expect(renderInlineHtml("**g** *i* `c<d>` ~~s~~ ==h==")).toBe(
      '<strong>g</strong> <em>i</em> <code class="cm-lp-code">c&lt;d&gt;</code> <s>s</s> <mark class="cm-lp-hl">h</mark>',
    );
    expect(renderInlineHtml("<script>x</script>")).toBe("&lt;script&gt;x&lt;/script&gt;");
    expect(renderInlineHtml("[[Note|alias]]")).toContain('data-wikilink="Note|alias">alias<');
    expect(renderInlineHtml('[t](http://a"b)')).toContain('data-href="http://a&quot;b"');
    expect(renderInlineHtml("snake_case_name")).toBe("snake_case_name");
  });
  it("continues lists, tasks and quotes", () => {
    expect(listContinuation("- item")).toEqual({ len: 2, next: "- " });
    expect(listContinuation("\t- [x] fait")).toEqual({ len: 7, next: "\t- [ ] " });
    expect(listContinuation("9. neuf")).toEqual({ len: 3, next: "10. " });
    expect(listContinuation("> cite")).toEqual({ len: 2, next: "> " });
    expect(listContinuation("texte")).toBeNull();
  });
  it("parses embed sizes", () => {
    expect(parseEmbed("a.png|300")).toEqual({ target: "a.png", width: 300, height: null, alias: null });
    expect(parseEmbed("a.png|300x200")).toMatchObject({ width: 300, height: 200 });
    expect(parseEmbed("Note|alias")).toMatchObject({ target: "Note", alias: "alias" });
  });
});

describe("file tree", () => {
  it("nests entries, folders first, case-insensitive, whatever the input order", () => {
    const roots = buildFileTree({ root: "/v", fromSnapshot: true, entries: [f("b.md"), f("Zeta/x.md"), d("alpha"), f("alpha/B.md"), f("alpha/a.md"), d("alpha/sub"), f("A.md")] });
    expect(roots.map((n) => n.name)).toEqual(["alpha", "Zeta", "A.md", "b.md"]);
    expect(roots[0].children!.map((n) => n.name)).toEqual(["sub", "a.md", "B.md"]);
    expect(roots[0].children![1].depth).toBe(1);
  });
});

describe("mock tag scanner (contract rules)", () => {
  const text = ["---", "tags: [alpha, beta/b1]", "---", "# Titre #dans/titre", "", "Paragraphe ligne 1", "suite avec #para/tag ici", "", "- item A", "\t- enfant #liste/tag", "\t\t- petit-enfant", "- item B", "", "> cite #q", "> suite", "", "`#code` et [[Note#ancre]] http://x.y/#frag #123", "", "| a | #cell |"].join("\n");
  const frags = scanTags("/v/n.md", text);
  const by = (tag: string) => frags.find((x) => x.tag === tag)!;
  it("reads frontmatter tags as whole-file fragments", () => {
    expect(by("alpha")).toMatchObject({ source: "frontmatter", startLine: 0, endLine: 18 });
    expect(by("beta/b1").source).toBe("frontmatter");
  });
  it("uses the enclosing block as the fragment", () => {
    expect(by("dans/titre")).toMatchObject({ startLine: 3, endLine: 3 });
    expect(by("para/tag")).toMatchObject({ startLine: 5, endLine: 6, tagLine: 6, tagCol: 11 });
    expect(by("liste/tag")).toMatchObject({ startLine: 9, endLine: 10 });
    expect(by("q")).toMatchObject({ startLine: 13, endLine: 14 });
    expect(by("cell")).toMatchObject({ startLine: 18, endLine: 18 });
  });
  it("ignores code, wikilinks, URLs and numeric tags", () => {
    expect(frags.map((x) => x.tag).sort()).toEqual(["alpha", "beta/b1", "cell", "dans/titre", "liste/tag", "para/tag", "q"]);
  });
});
