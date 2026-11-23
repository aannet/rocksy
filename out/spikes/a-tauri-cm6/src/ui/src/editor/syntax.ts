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

const INLINE =
  /(`+)(.+?)\1(?!`)|!\[\[([^\]\n]+)\]\]|\[\[([^\]\n]+)\]\]|\[([^\]\n]+)\]\(([^)\s]+)[^)]*\)|\*\*(?=\S)(.+?)(?<=\S)\*\*|__(?=\S)(.+?)(?<=\S)__|\*(?=\S)(.+?)(?<=\S)\*|(?<![\p{L}\p{N}])_(?=\S)(.+?)(?<=\S)_(?![\p{L}\p{N}])|~~(.+?)~~|==(.+?)==|(?<=^|\s)#([\p{L}\p{N}_\-/]*[\p{L}_\-/][\p{L}\p{N}_\-/]*)|<br\s*\/?>|\\([|\\`*_{}[\]()#+\-.!~=])/gu;

/** Renders the inline Markdown subset used inside table cells to safe HTML. */
export function renderInlineHtml(text: string): string {
  let out = "";
  let last = 0;
  INLINE.lastIndex = 0;
  let m: RegExpExecArray | null;
  // The regex is global and this function recurses: collect matches first.
  const matches: RegExpExecArray[] = [];
  while ((m = INLINE.exec(text))) matches.push(m);
  for (m of matches) {
    out += escapeHtml(text.slice(last, m.index));
    last = m.index + m[0].length;
    if (m[1]) out += `<code class="cm-lp-code">${escapeHtml(m[2])}</code>`;
    else if (m[3] != null) out += `<span class="cm-lp-embed-chip" data-wikilink="${escapeHtml(m[3])}">${escapeHtml(m[3])}</span>`;
    else if (m[4] != null) {
      const bar = m[4].indexOf("|");
      const label = bar >= 0 ? m[4].slice(bar + 1) : m[4];
      out += `<span class="cm-lp-link cm-lp-wikilink cm-lp-rendered" data-wikilink="${escapeHtml(m[4])}">${escapeHtml(label)}</span>`;
    } else if (m[5] != null) out += `<span class="cm-lp-link cm-lp-rendered" data-href="${escapeHtml(m[6])}">${renderInlineHtml(m[5])}</span>`;
    else if (m[7] != null || m[8] != null) out += `<strong>${renderInlineHtml(m[7] ?? m[8])}</strong>`;
    else if (m[9] != null || m[10] != null) out += `<em>${renderInlineHtml(m[9] ?? m[10])}</em>`;
    else if (m[11] != null) out += `<s>${renderInlineHtml(m[11])}</s>`;
    else if (m[12] != null) out += `<mark class="cm-lp-hl">${renderInlineHtml(m[12])}</mark>`;
    else if (m[13] != null) out += `<span class="cm-lp-tag" data-tag="${escapeHtml(m[13])}">#${escapeHtml(m[13])}</span>`;
    else if (m[14] != null) out += escapeHtml(m[14]);
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
