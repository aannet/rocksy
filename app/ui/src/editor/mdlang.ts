// Markdown language for Rocksy: @lezer/markdown + GFM + Obsidian-flavoured extensions.
// Built directly on @lezer/markdown (not @codemirror/lang-markdown) to keep the HTML/CSS/JS
// grammars out of the startup bundle; code-block languages are loaded on demand (langs.ts).
import { parser as baseParser, GFM, parseCode, type MarkdownConfig } from "@lezer/markdown";
import { tags as t } from "@lezer/highlight";
import { Language, defineLanguageFacet, languageDataProp, foldNodeProp, ParseContext } from "@codemirror/language";
import type { Parser } from "@lezer/common";
import { findCodeLanguage } from "./langs";

const PUNCT = /[!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~\xA1‐-‧]/;

/** YAML frontmatter: a `---` fence on the very first line, closed by `---` or `...`. */
const Frontmatter: MarkdownConfig = {
  defineNodes: [{ name: "Frontmatter", block: true, style: t.meta }],
  parseBlock: [
    {
      name: "Frontmatter",
      before: "HorizontalRule",
      parse(cx, line) {
        if (cx.lineStart !== 0 || !/^---[ \t]*$/.test(line.text)) return false;
        let end = line.text.length;
        while (cx.nextLine()) {
          end = cx.lineStart + line.text.length;
          if (/^(---|\.\.\.)[ \t]*$/.test(line.text)) {
            cx.nextLine();
            break;
          }
        }
        cx.addElement(cx.elt("Frontmatter", 0, end));
        return true;
      },
    },
  ],
};

const HighlightDelim = { resolve: "Highlight", mark: "HighlightMark" };

/** `==highlight==` */
const Highlight: MarkdownConfig = {
  defineNodes: [
    { name: "Highlight", style: { "Highlight/...": t.special(t.content) } },
    { name: "HighlightMark", style: t.processingInstruction },
  ],
  parseInline: [
    {
      name: "Highlight",
      parse(cx, next, pos) {
        if (next !== 61 /* = */ || cx.char(pos + 1) !== 61 || cx.char(pos + 2) === 61) return -1;
        const before = cx.slice(pos - 1, pos);
        const after = cx.slice(pos + 2, pos + 3);
        const sBefore = /\s|^$/.test(before);
        const sAfter = /\s|^$/.test(after);
        const pBefore = PUNCT.test(before);
        const pAfter = PUNCT.test(after);
        return cx.addDelimiter(
          HighlightDelim,
          pos,
          pos + 2,
          !sAfter && (!pAfter || sBefore || pBefore),
          !sBefore && (!pBefore || sAfter || pAfter),
        );
      },
      after: "Emphasis",
    },
  ],
};

function wikiEnd(cx: { slice(a: number, b: number): string; end: number }, from: number): number {
  // `from` points at the first `[`. Returns the position after `]]`, or -1.
  const text = cx.slice(from + 2, Math.min(cx.end, from + 2 + 400));
  const close = text.indexOf("]]");
  if (close <= 0) return -1;
  const inner = text.slice(0, close);
  if (inner.includes("\n") || inner.includes("[")) return -1;
  return from + 2 + close + 2;
}

/** `[[note]]`, `[[note|alias]]`, `[[note#heading]]` and embeds `![[file]]`. */
const Wikilink: MarkdownConfig = {
  defineNodes: [
    { name: "Wikilink", style: t.link },
    { name: "Embed", style: t.link },
  ],
  parseInline: [
    {
      name: "Embed",
      parse(cx, next, pos) {
        if (next !== 33 /* ! */ || cx.char(pos + 1) !== 91 || cx.char(pos + 2) !== 91) return -1;
        const end = wikiEnd(cx, pos + 1);
        return end < 0 ? -1 : cx.addElement(cx.elt("Embed", pos, end));
      },
      before: "Image",
    },
    {
      name: "Wikilink",
      parse(cx, next, pos) {
        if (next !== 91 /* [ */ || cx.char(pos + 1) !== 91) return -1;
        const end = wikiEnd(cx, pos);
        return end < 0 ? -1 : cx.addElement(cx.elt("Wikilink", pos, end));
      },
      before: "Link",
    },
  ],
};

/** Body of a tag: letters, digits, `_`, `-`, `/`; at least one non-digit (contract, Obsidian syntax). */
export const TAG_BODY = /^[\p{L}\p{N}_\-/]+/u;

/** `#tag`, `#a/b`: `#` at line start or after whitespace. */
const Hashtag: MarkdownConfig = {
  defineNodes: [{ name: "Hashtag", style: t.tagName }],
  parseInline: [
    {
      name: "Hashtag",
      parse(cx, next, pos) {
        if (next !== 35 /* # */) return -1;
        if (pos > cx.offset && !/\s/.test(cx.slice(pos - 1, pos))) return -1;
        const m = TAG_BODY.exec(cx.slice(pos + 1, Math.min(cx.end, pos + 1 + 120)));
        if (!m || /^\d+$/.test(m[0])) return -1;
        return cx.addElement(cx.elt("Hashtag", pos, pos + 1 + m[0].length));
      },
    },
  ],
};

const data = defineLanguageFacet({ commentTokens: { block: { open: "<!--", close: "-->" } } });

function codeParser(info: string): Parser | null {
  const desc = findCodeLanguage(info);
  if (!desc) return null;
  if (desc.support) return desc.support.language.parser;
  // Not loaded yet: skip the block for now, the parse restarts when the import resolves.
  return ParseContext.getSkippingParser(desc.load());
}

const mdParser = baseParser.configure([
  GFM,
  Frontmatter,
  Highlight,
  Wikilink,
  Hashtag,
  parseCode({ codeParser }),
  {
    props: [
      languageDataProp.add({ Document: data }),
      foldNodeProp.add({ FencedCode: (node, state) => ({ from: state.doc.lineAt(node.from).to, to: node.to }) }),
    ],
  },
]);

export const rocksyMarkdown = new Language(data, mdParser, [], "markdown");
