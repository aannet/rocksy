import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree } from "@codemirror/language";
import { rocksyMarkdown } from "../src/editor/mdlang";
import { buildBlockDecorations, buildInlineDecorations } from "../src/editor/livepreview";

function setup(doc: string, cursor = 0) {
  const state = EditorState.create({ doc, selection: { anchor: cursor }, extensions: [rocksyMarkdown] });
  const tree = ensureSyntaxTree(state, doc.length, 5000)!;
  return { state, tree };
}

function nodes(doc: string): string[] {
  const { tree } = setup(doc);
  const out: string[] = [];
  tree.iterate({ enter: (n) => void out.push(`${n.name}:${n.from}-${n.to}`) });
  return out;
}

interface Deco {
  from: number;
  to: number;
  cls: string;
  hidden: boolean;
  widget: string | null;
}

function inline(doc: string, cursor = 0): Deco[] {
  const { state, tree } = setup(doc, cursor);
  return buildInlineDecorations(state, [{ from: 0, to: doc.length }], tree).map((r) => ({
    from: r.from,
    to: r.to,
    cls: (r.value.spec.class as string) ?? "",
    hidden: r.from < r.to && !r.value.spec.class && !r.value.spec.widget,
    widget: r.value.spec.widget ? r.value.spec.widget.constructor.name : null,
  }));
}
const hiddenText = (doc: string, cursor: number) =>
  inline(doc, cursor)
    .filter((d) => d.hidden)
    .map((d) => doc.slice(d.from, d.to));

describe("markdown grammar extensions", () => {
  it("parses frontmatter only at the top", () => {
    expect(nodes("---\ntags: [a]\n---\n\ntext")).toContain("Frontmatter:0-17");
    expect(nodes("text\n\n---\na: b\n---\n").some((n) => n.startsWith("Frontmatter"))).toBe(false);
  });
  it("parses wikilinks, embeds, highlight and tags", () => {
    const n = nodes("See [[Note|alias]] and ![[img.png]] ==hot== #a/b-c end");
    expect(n).toContain("Wikilink:4-18");
    expect(n).toContain("Embed:23-35");
    expect(n).toContain("Highlight:36-43");
    expect(n).toContain("Hashtag:44-50");
  });
  it("rejects tags that are numeric, glued to a word or inside code", () => {
    const has = (s: string) => nodes(s).some((x) => x.startsWith("Hashtag"));
    expect(has("issue #123")).toBe(false);
    expect(has("word#tag")).toBe(false);
    expect(has("`#code`")).toBe(false);
    expect(has("```\n#code\n```")).toBe(false);
    expect(has("http://x.org/#frag")).toBe(false);
    expect(has("#2024/plan")).toBe(true);
    expect(has("# Heading")).toBe(false);
  });
});

describe("inline decorations", () => {
  const doc = "# Title\n\nSome **bold** and *it* text with `code`.\n";
  it("hides markers when the cursor is elsewhere", () => {
    const far = doc.length;
    expect(hiddenText(doc, far)).toEqual(["# ", "**", "**", "*", "*", "`", "`"]);
  });
  it("shows the marker of the element under the cursor only", () => {
    const inBold = doc.indexOf("bold") + 1;
    expect(hiddenText(doc, inBold)).toEqual(["# ", "*", "*", "`", "`"]);
    expect(hiddenText(doc, 3)).toEqual(["**", "**", "*", "*", "`", "`"]);
  });
  it("reveals markers when a selection overlaps the element", () => {
    const state = EditorState.create({ doc, selection: { anchor: 0, head: doc.indexOf("bold") }, extensions: [rocksyMarkdown] });
    const tree = ensureSyntaxTree(state, doc.length, 5000)!;
    const hidden = buildInlineDecorations(state, [{ from: 0, to: doc.length }], tree)
      .filter((r) => r.from < r.to && !r.value.spec.class && !r.value.spec.widget)
      .map((r) => doc.slice(r.from, r.to));
    expect(hidden).toEqual(["*", "*", "`", "`"]);
  });
  it("renders links and wikilinks as links", () => {
    const d = "x [text](http://a.b) and [[Note#H|alias]] y\n";
    const far = d.length;
    expect(hiddenText(d, far)).toEqual(["[", "](http://a.b)", "[[Note#H|", "]]"]);
    const links = inline(d, far).filter((x) => x.cls.includes("cm-lp-link"));
    expect(links.map((l) => d.slice(l.from, l.to))).toEqual(["text", "alias"]);
    expect(links.every((l) => l.cls.includes("cm-lp-rendered"))).toBe(true);
  });
  it("turns tasks into checkboxes and bullets into dots", () => {
    const d = "- [ ] todo\n- [x] done\n- item\n\nend";
    const w = inline(d, d.length).filter((x) => x.widget);
    expect(w.map((x) => x.widget)).toEqual(["CheckboxWidget", "CheckboxWidget", "BulletWidget"]);
    expect(inline(d, d.length).some((x) => x.cls === "cm-lp-task-done")).toBe(true);
    // cursor on the marker of the first task: raw source for that one only
    const w2 = inline(d, 1).filter((x) => x.widget);
    expect(w2.map((x) => x.widget)).toEqual(["CheckboxWidget", "BulletWidget"]);
  });
  it("styles tags, quotes, rules and code blocks", () => {
    const d = "> quote #tag\n\n---\n\n```js\nlet a = 1\n```\n\nend";
    const all = inline(d, d.length);
    expect(all.some((x) => x.cls === "cm-lp-tag" && d.slice(x.from, x.to) === "#tag")).toBe(true);
    expect(all.some((x) => x.cls === "cm-lp-quote")).toBe(true);
    expect(all.some((x) => x.widget === "HrWidget")).toBe(true);
    expect(all.filter((x) => x.cls.startsWith("cm-lp-cb")).length).toBe(3);
    expect(all.some((x) => x.widget === "CodeLabelWidget")).toBe(true);
  });
  it("detects callouts", () => {
    const d = "> [!warning]- Attention\n> body\n\nend";
    const all = inline(d, d.length);
    expect(all.some((x) => x.cls.includes("cm-lp-co-orange") && x.cls.includes("cm-lp-co-head"))).toBe(true);
    expect(all.some((x) => x.widget === "CalloutIconWidget")).toBe(true);
    expect(all.some((x) => x.widget === "FoldToggleWidget")).toBe(true);
  });
  it("leaves huge single lines in source style", () => {
    const d = "**a** `b` [[c]] #t ".repeat(400) + "\n\n**x**\n";
    const all = inline(d, d.length);
    expect(all.filter((x) => x.from < d.length - 8).length).toBe(0);
    expect(all.some((x) => x.cls === "cm-lp-strong")).toBe(true);
  });
  it("only decorates the requested ranges", () => {
    const d = "**a**\n\n" + "filler line\n".repeat(50) + "**b**\n";
    const { state, tree } = setup(d, 10);
    const r = buildInlineDecorations(state, [{ from: d.length - 7, to: d.length }], tree);
    expect(r.length).toBeGreaterThan(0);
    expect(r.every((x) => x.from >= d.length - 7)).toBe(true);
  });
});

describe("block decorations", () => {
  const d = "---\na: 1\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> [!note]- Folded\n> hidden\n\nend";
  it("replaces tables and folded callouts when the cursor is outside", () => {
    const { state, tree } = setup(d, d.length);
    const r = buildBlockDecorations(state, tree, [], false);
    expect(r.map((x) => d.slice(x.from, x.to))).toEqual(["| a | b |\n|---|---|\n| 1 | 2 |", "> hidden"]);
  });
  it("shows the table source when the cursor is inside", () => {
    const { state, tree } = setup(d, d.indexOf("| 1"));
    const r = buildBlockDecorations(state, tree, [], false);
    expect(r.map((x) => d.slice(x.from, x.to))).toEqual(["> hidden"]);
  });
  it("folds the frontmatter on demand and honours callout toggles", () => {
    const { state, tree } = setup(d, d.length);
    const r = buildBlockDecorations(state, tree, [d.indexOf("> [!note]")], true);
    expect(r.map((x) => d.slice(x.from, x.to))).toEqual(["---\na: 1\n---", "| a | b |\n|---|---|\n| 1 | 2 |"]);
  });
});
