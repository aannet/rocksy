// Pure helpers for search results and link excerpts (unit-tested): highlighted segments from UTF-16 ranges,
// accent/case folding that preserves string length, query parsing, grouping.

export interface Segment {
  text: string;
  hit: boolean;
}

const isHigh = (c: number) => c >= 0xd800 && c <= 0xdbff;
const isLow = (c: number) => c >= 0xdc00 && c <= 0xdfff;

/**
 * Splits `excerpt` into plain and highlighted segments. `ranges` are [start, end) in UTF-16 units;
 * they may be unsorted, overlapping or out of bounds, and never split a surrogate pair in the output.
 */
export function segments(excerpt: string, ranges: readonly (readonly [number, number])[]): Segment[] {
  const n = excerpt.length;
  const clean: [number, number][] = [];
  for (const r of ranges) {
    let a = Math.max(0, Math.min(n, Math.floor(r[0])));
    let b = Math.max(0, Math.min(n, Math.floor(r[1])));
    if (!(b > a)) continue;
    if (a > 0 && isLow(excerpt.charCodeAt(a)) && isHigh(excerpt.charCodeAt(a - 1))) a--;
    if (b < n && isLow(excerpt.charCodeAt(b)) && isHigh(excerpt.charCodeAt(b - 1))) b++;
    clean.push([a, b]);
  }
  clean.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
  const out: Segment[] = [];
  let pos = 0;
  let i = 0;
  while (i < clean.length) {
    const a = clean[i][0];
    let b = clean[i][1];
    while (i + 1 < clean.length && clean[i + 1][0] <= b) b = Math.max(b, clean[++i][1]);
    i++;
    if (a > pos) out.push({ text: excerpt.slice(pos, a), hit: false });
    out.push({ text: excerpt.slice(a, b), hit: true });
    pos = b;
  }
  if (pos < n) out.push({ text: excerpt.slice(pos), hit: false });
  return out;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : "&quot;"));
}

export function segmentsHtml(excerpt: string, ranges: readonly (readonly [number, number])[]): string {
  let html = "";
  for (const s of segments(excerpt, ranges)) html += s.hit ? `<mark>${escapeHtml(s.text)}</mark>` : escapeHtml(s.text);
  return html;
}

const FOLD: Record<string, string> = {};
for (let c = 0xc0; c <= 0x24f; c++) {
  const ch = String.fromCharCode(c);
  const base = ch.normalize("NFD")[0];
  if (base !== ch && /[a-zA-Z]/.test(base)) FOLD[ch] = base.toLowerCase();
}
// Ligatures cannot be folded without changing the length (the core maps œ to "oe"): they are kept,
// and highlightTerms adds the ligature spelling of a term instead.
delete FOLD["œ"];
delete FOLD["æ"];

/** Lower-cases and strips accents, one UTF-16 unit for one unit, so offsets stay valid in the original. */
export function foldText(s: string): string {
  const lower = s.toLowerCase();
  if (lower.length === s.length) return lower.replace(/[\u00c0-\u024f]/g, (c) => FOLD[c] ?? c);
  let out = "";
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    const l = c.toLowerCase();
    const one = l.length === 1 ? l : c;
    out += FOLD[one] ?? one;
  }
  return out;
}

export interface ParsedQuery {
  /** Positive words, folded. The last one is a prefix when `lastIsPrefix`. */
  words: string[];
  lastIsPrefix: boolean;
  phrases: string[];
  exclude: string[];
  paths: string[];
  tags: string[];
}

/** Parses the search syntax of the contract: words (AND), "phrase", -word, path:text, tag:a/b. */
export function parseQuery(query: string): ParsedQuery {
  const q: ParsedQuery = { words: [], lastIsPrefix: false, phrases: [], exclude: [], paths: [], tags: [] };
  const re = /(-?)(path:|tag:)?(?:"([^"]*)"?|(\S+))/gi;
  let m: RegExpExecArray | null;
  let lastKind = "";
  while ((m = re.exec(query))) {
    const neg = m[1] === "-";
    const key = (m[2] ?? "").toLowerCase();
    const quoted = m[3] != null;
    const raw = quoted ? m[3] : m[4];
    const value = foldText(raw).trim();
    if (!value) continue;
    if (key === "path:") q.paths.push(value);
    else if (key === "tag:") q.tags.push(value.replace(/^#/, ""));
    else if (neg) q.exclude.push(value);
    else if (quoted) q.phrases.push(value);
    else {
      // Same tokenisation as the core: runs of letters and digits.
      const parts = value.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
      q.words.push(...parts);
    }
    lastKind = key || (neg ? "-" : quoted ? '"' : "w");
  }
  q.lastIsPrefix = lastKind === "w" && !/\s$/.test(query) && q.words.length > 0;
  return q;
}

/** Terms worth highlighting in the opened note for a query (no filters, no exclusions). */
export function highlightTerms(query: string): string[] {
  const q = parseQuery(query);
  const out: string[] = [];
  const add = (t: string) => {
    if (t.length >= 2 && !out.includes(t)) out.push(t);
  };
  for (const raw of [...q.phrases, ...q.words]) {
    const t = raw.replace(/œ/g, "oe").replace(/æ/g, "ae");
    add(t);
    // The text keeps its ligatures after folding: also look for that spelling.
    if (t.includes("oe")) add(t.replace(/oe/g, "œ"));
    if (t.includes("ae")) add(t.replace(/ae/g, "æ"));
  }
  return out;
}

const WORD = /[\p{L}\p{N}]/u;

/** Occurrences of `term` in folded text that start at a word boundary. Returns [from, to) pairs. */
export function termRanges(folded: string, term: string, wholeWord = false, max = 2000): [number, number][] {
  const out: [number, number][] = [];
  if (!term) return out;
  const startsWithWord = WORD.test(term[0]);
  const endsWithWord = WORD.test(term[term.length - 1]);
  let i = folded.indexOf(term);
  while (i >= 0 && out.length < max) {
    const end = i + term.length;
    const okBefore = !startsWithWord || i === 0 || !WORD.test(folded[i - 1]);
    const okAfter = !wholeWord || !endsWithWord || end >= folded.length || !WORD.test(folded[end]);
    if (okBefore && okAfter) out.push([i, end]);
    i = folded.indexOf(term, i + 1);
  }
  return out;
}

export interface PathGroup<T> {
  path: string;
  items: T[];
}

/** Groups consecutive-or-not items by `path`, keeping the order of first appearance. */
export function groupByPath<T extends { path: string }>(items: readonly T[]): PathGroup<T>[] {
  const map = new Map<string, PathGroup<T>>();
  const out: PathGroup<T>[] = [];
  for (const it of items) {
    let g = map.get(it.path);
    if (!g) {
      g = { path: it.path, items: [] };
      map.set(it.path, g);
      out.push(g);
    }
    g.items.push(it);
  }
  return out;
}

/** Cuts a long line to at most `max` characters around [from, to), returning the excerpt and the shift applied. */
export function excerptAround(line: string, from: number, to: number, max = 240): { text: string; shift: number } {
  if (line.length <= max) return { text: line, shift: 0 };
  let start = Math.max(0, Math.min(from - 60, line.length - max));
  if (start > 0 && isLow(line.charCodeAt(start))) start++;
  let end = Math.min(line.length, Math.max(start + max, to));
  if (end - start > max) end = start + max;
  if (end < line.length && isLow(line.charCodeAt(end))) end--;
  return { text: line.slice(start, end), shift: start };
}
