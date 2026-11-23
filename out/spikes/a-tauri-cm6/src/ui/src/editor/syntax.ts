// Pure helpers for the Live Preview (no DOM, no CodeMirror view): unit-tested.

export interface CalloutHead {
  /** Length of the quote prefix (`> `). */
  prefixLen: number;
  /** Length of prefix + `[!type]` + fold marker + following blanks. */
  headLen: number;
  type: string;
  fold: "" | "+" | "-";
  title: string;
}

const CALLOUT = /^(\s*>[ \t]?)\[!([^\]\n]+)\]([+-]?)[ \t]*/;

export function parseCalloutHead(lineText: string): CalloutHead | null {
  const m = CALLOUT.exec(lineText);
  if (!m) return null;
  return {
    prefixLen: m[1].length,
    headLen: m[0].length,
    type: m[2].trim().toLowerCase(),
    fold: m[3] as CalloutHead["fold"],
    title: lineText.slice(m[0].length).trim(),
  };
}

export interface CalloutStyle {
  color: string;
  icon: string;
}

const CALLOUT_STYLES: Record<string, CalloutStyle> = {};
function def(names: string, color: string, icon: string) {
  for (const n of names.split(" ")) CALLOUT_STYLES[n] = { color, icon };
}
def("note", "blue", "pencil");
def("info todo", "blue", "info");
def("abstract summary tldr", "cyan", "clipboard");
def("tip hint important", "teal", "flame");
def("success check done", "green", "check");
def("question help faq", "yellow", "help");
def("warning caution attention", "orange", "warning");
def("failure fail missing", "red", "xcircle");
def("danger error", "red", "zap");
def("bug", "red", "bug");
def("example", "purple", "list");
def("quote cite", "grey", "quote");

export function calloutStyle(type: string): CalloutStyle {
  return CALLOUT_STYLES[type] ?? CALLOUT_STYLES.note;
}

export function calloutDefaultTitle(type: string): string {
  return type ? type[0].toUpperCase() + type.slice(1) : "Note";
}

// ---------------------------------------------------------------- tables

export interface TableCell {
  text: string;
  /** Offset of the cell text from the start of the table source. */
  off: number;
}
export type Align = "left" | "center" | "right" | null;
export interface TableModel {
  header: TableCell[];
  align: Align[];
  rows: TableCell[][];
}

/** Splits one table row on unescaped pipes (pipes inside inline code are kept). */
export function splitTableRow(line: string, lineOff: number): TableCell[] {
  const cells: TableCell[] = [];
  let i = 0;
  const n = line.length;
  while (i < n && (line[i] === " " || line[i] === "\t")) i++;
  if (line[i] === "|") i++;
  let start = i;
  let inCode = false;
  const pushCell = (end: number) => {
    let a = start;
    let b = end;
    while (a < b && (line[a] === " " || line[a] === "\t")) a++;
    while (b > a && (line[b - 1] === " " || line[b - 1] === "\t")) b--;
    cells.push({ text: line.slice(a, b), off: lineOff + a });
  };
  for (; i < n; i++) {
    const c = line[i];
    if (c === "\\" && i + 1 < n) {
      i++;
    } else if (c === "`") {
      inCode = !inCode;
    } else if (c === "|" && !inCode) {
      pushCell(i);
      start = i + 1;
    }
  }
  // Trailing content after the last pipe is a cell only when non-blank.
  if (line.slice(start).trim() !== "") pushCell(n);
  return cells;
}

/** Parses a GFM table source (header, delimiter row, body rows). Returns null if it is not a table. */
export function parseTable(src: string): TableModel | null {
  const lines = src.split("\n");
  if (lines.length < 2) return null;
  const delim = splitTableRow(lines[1], 0);
  if (!delim.length || !delim.every((c) => /^:?-+:?$/.test(c.text))) return null;
  const align: Align[] = delim.map((c) => {
    const l = c.text.startsWith(":");
    const r = c.text.endsWith(":");
    return l && r ? "center" : r ? "right" : l ? "left" : null;
  });
  const header = splitTableRow(lines[0], 0);
  const rows: TableCell[][] = [];
  let off = lines[0].length + 1 + lines[1].length + 1;
  for (let i = 2; i < lines.length; i++) {
    if (lines[i].trim() !== "") rows.push(splitTableRow(lines[i], off));
    off += lines[i].length + 1;
  }
  return { header, align, rows };
}

// ---------------------------------------------------------------- inline markdown -> HTML (table cells)

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** Longest text rendered as inline Markdown; longer input is shown as escaped plain text (bounds the regex cost). */
export const INLINE_MAX = 2000;

// Groups: 1-2 code, 3 embed, 4 wikilink, 5 label of a Markdown link (the `(destination)` part is scanned by
// `scanLinkTail`, not by the regex: a regex for it backtracks catastrophically on `[a](` repeated), 6-7 strong,
// 8-9 emphasis, 10 strike, 11 highlight, 12 tag, 13 escaped character.
const INLINE =
  /(`+)(.+?)\1(?!`)|!\[\[([^\]\n]+)\]\]|\[\[([^\]\n]+)\]\]|\[([^\]\n]+)\]\(|\*\*(?=\S)(.+?)(?<=\S)\*\*|__(?=\S)(.+?)(?<=\S)__|\*(?=\S)(.+?)(?<=\S)\*|(?<![\p{L}\p{N}])_(?=\S)(.+?)(?<=\S)_(?![\p{L}\p{N}])|~~(.+?)~~|==(.+?)==|(?<=^|\s)#([\p{L}\p{N}_\-/]*[\p{L}_\-/][\p{L}\p{N}_\-/]*)|<br\s*\/?>|\\([|\\`*_{}[\]()#+\-.!~=])/gu;

const isSpace = (c: string) => c === " " || c === "\t" || c === "\n" || c === "\r" || c === "\f" || c === "\v" || c === "\u00a0";

/**
 * Scans the `destination "title")` part of a Markdown link, `from` being the index just after `(`.
 * The destination may hold balanced parentheses (`a(b).md`) or be wrapped in `<…>`; an optional title follows a blank.
 * Returns the destination and the index after the closing `)`, or null when this is not a link. One pass, no backtracking.
 */
export function scanLinkTail(text: string, from: number): { href: string; end: number } | null {
  const n = text.length;
  let j = from;
  let balanced = false;
  if (text[from] === "<") {
    const gt = text.indexOf(">", from + 1);
    if (gt > from + 1 && (text[gt + 1] === ")" || isSpace(text[gt + 1] ?? ""))) {
      j = gt + 1;
      balanced = true;
    }
  }
  if (!balanced) {
    let depth = 0;
    for (; j < n; j++) {
      const c = text[j];
      if (c === "(") depth++;
      else if (c === ")") {
        if (depth === 0) break;
        depth--;
      } else if (isSpace(c)) break;
    }
    if (j >= n || depth !== 0) {
      // Unbalanced: fall back to "up to the first blank or closing parenthesis".
      for (j = from; j < n && text[j] !== ")" && !isSpace(text[j]); j++);
      if (j >= n) return null;
    }
  }
  if (j === from) return null;
  const href = text.slice(from, j);
  if (text[j] === ")") return { href, end: j + 1 };
  // Title: a quoted string closed right before `)`, else anything up to the first `)`.
  let k = j;
  while (k < n && isSpace(text[k])) k++;
  if (text[k] === '"' || text[k] === "'") {
    let q = text.indexOf(text[k], k + 1);
    if (q > 0) {
      q++;
      while (q < n && isSpace(text[q])) q++;
      if (text[q] === ")") return { href, end: q + 1 };
    }
  }
  const close = text.indexOf(")", j);
  return close < 0 ? null : { href, end: close + 1 };
}

/** Renders the inline Markdown subset used inside table cells to safe HTML. */
export function renderInlineHtml(text: string): string {
  if (text.length > INLINE_MAX) return escapeHtml(text);
  let out = "";
  let last = 0;
  INLINE.lastIndex = 0;
  let m: RegExpExecArray | null;
  // The regex is global and this function recurses: collect matches first.
  const matches: { m: RegExpExecArray; end: number; href: string }[] = [];
  const lastClose = text.lastIndexOf(")");
  while ((m = INLINE.exec(text))) {
    if (m[5] != null) {
      const link = INLINE.lastIndex <= lastClose ? scanLinkTail(text, INLINE.lastIndex) : null;
      if (!link) {
        // Not a link: the `[` is plain text, look for other marks right after it.
        INLINE.lastIndex = m.index + 1;
        continue;
      }
      INLINE.lastIndex = link.end;
      matches.push({ m, end: link.end, href: link.href });
    } else matches.push({ m, end: m.index + m[0].length, href: "" });
  }
  for (const t of matches) {
    m = t.m;
    out += escapeHtml(text.slice(last, m.index));
    last = t.end;
    if (m[1]) out += `<code class="cm-lp-code">${escapeHtml(m[2])}</code>`;
    else if (m[3] != null) out += `<span class="cm-lp-embed-chip" data-wikilink="${escapeHtml(m[3])}">${escapeHtml(m[3])}</span>`;
    else if (m[4] != null) {
      const bar = m[4].indexOf("|");
      const label = bar >= 0 ? m[4].slice(bar + 1) : m[4];
      out += `<span class="cm-lp-link cm-lp-wikilink cm-lp-rendered" data-wikilink="${escapeHtml(m[4])}">${escapeHtml(label)}</span>`;
    } else if (m[5] != null) out += `<span class="cm-lp-link cm-lp-rendered" data-href="${escapeHtml(t.href)}">${renderInlineHtml(m[5])}</span>`;
    else if (m[6] != null || m[7] != null) out += `<strong>${renderInlineHtml(m[6] ?? m[7])}</strong>`;
    else if (m[8] != null || m[9] != null) out += `<em>${renderInlineHtml(m[8] ?? m[9])}</em>`;
    else if (m[10] != null) out += `<s>${renderInlineHtml(m[10])}</s>`;
    else if (m[11] != null) out += `<mark class="cm-lp-hl">${renderInlineHtml(m[11])}</mark>`;
    else if (m[12] != null) out += `<span class="cm-lp-tag" data-tag="${escapeHtml(m[12])}">#${escapeHtml(m[12])}</span>`;
    else if (m[13] != null) out += escapeHtml(m[13]);
    else out += "<br>";
  }
  return out + escapeHtml(text.slice(last));
}

// ---------------------------------------------------------------- lists

export interface ListPrefix {
  /** Full prefix length (indent + marker + blanks + optional task box). */
  len: number;
  /** Prefix to insert on the next line. */
  next: string;
}

const LIST = /^([ \t]*(?:>[ \t]?)*[ \t]*)(?:([-*+])|(\d{1,9})([.)]))([ \t]+)(\[[ xX]\][ \t]+)?/;
const QUOTE = /^([ \t]*(?:>[ \t]?)+)/;

/** Prefix that continues the list item / task / quote of `lineText` on the next line, or null. */
export function listContinuation(lineText: string): ListPrefix | null {
  const m = LIST.exec(lineText);
  if (m) {
    const marker = m[2] ? m[2] : String(Number(m[3]) + 1) + m[4];
    return { len: m[0].length, next: m[1] + marker + m[5] + (m[6] ? "[ ] " : "") };
  }
  const q = QUOTE.exec(lineText);
  if (q) return { len: q[0].length, next: q[1] };
  return null;
}

/** Splits `![[file.png|300]]` / `![[file.png|300x200]]` inner text into target and size. */
export function parseEmbed(inner: string): { target: string; width: number | null; height: number | null; alias: string | null } {
  const bar = inner.indexOf("|");
  if (bar < 0) return { target: inner.trim(), width: null, height: null, alias: null };
  const target = inner.slice(0, bar).trim();
  const rest = inner.slice(bar + 1).trim();
  const m = /^(\d+)(?:x(\d+))?$/.exec(rest);
  if (m) return { target, width: Number(m[1]), height: m[2] ? Number(m[2]) : null, alias: null };
  return { target, width: null, height: null, alias: rest || null };
}

/** True when a selection range [selFrom, selTo] touches the node [from, to] (inclusive on both ends). */
export function touchesRange(sel: readonly { from: number; to: number }[], from: number, to: number): boolean {
  for (let i = 0; i < sel.length; i++) if (sel[i].from <= to && sel[i].to >= from) return true;
  return false;
}
