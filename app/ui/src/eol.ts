// Line endings and BOM of a file, kept from open to save (pure logic, unit-tested).
//
// A file with one kind of line ending is written back with that ending everywhere. A file with mixed endings
// is kept as it is: the ending of every line is remembered at open; at save, the unchanged lines at the head
// and at the tail of the document keep their own ending, the lines changed or added in between take the
// majority ending of the file.
import type { Text } from "@codemirror/state";

export type Eol = "\n" | "\r\n" | "\r";

/** Per-line endings of a mixed file as last read or written: `ends[i]` follows `lines[i]` (`lines.length - 1` entries). */
export interface MixedEol {
  lines: string[];
  ends: Eol[];
}

export interface Decoded {
  /** Text without the BOM, line endings untouched. */
  text: string;
  bom: boolean;
  /** The only ending of the file, or the majority one when mixed (LF when there is no line break). */
  eol: Eol;
  mixed: MixedEol | null;
}

export interface EolState {
  bom: boolean;
  eol: Eol;
  eolMixed: MixedEol | null;
}

/** Same line model as CodeMirror: `\r\n`, `\n` and a lone `\r` each end a line. */
const BREAK = /\r\n?|\n/;

export function decodeText(raw: string): Decoded {
  const bom = raw.charCodeAt(0) === 0xfeff;
  const text = bom ? raw.slice(1) : raw;
  let lf = 0;
  let crlf = 0;
  let cr = 0;
  for (let i = text.indexOf("\r"); i >= 0; i = text.indexOf("\r", i + 1)) {
    if (text.charCodeAt(i + 1) === 10) crlf++;
    else cr++;
  }
  if (crlf + cr === 0) return { text, bom, eol: "\n", mixed: null };
  for (let i = text.indexOf("\n"); i >= 0; i = text.indexOf("\n", i + 1)) lf++;
  lf -= crlf;
  const eol: Eol = lf >= crlf && lf >= cr ? "\n" : crlf >= cr ? "\r\n" : "\r";
  if ((lf > 0 ? 1 : 0) + (crlf > 0 ? 1 : 0) + (cr > 0 ? 1 : 0) <= 1) return { text, bom, eol, mixed: null };
  const ends: Eol[] = [];
  const re = /\r\n?|\n/g;
  for (let m = re.exec(text); m; m = re.exec(text)) ends.push(m[0] as Eol);
  return { text, bom, eol, mixed: { lines: text.split(BREAK), ends } };
}

/**
 * Text to hand to `save_file` for the document `doc`. For a mixed file, also returns the per-line endings
 * as written: they become the new reference once the save has succeeded.
 */
export function encodeText(doc: Text, st: EolState): { text: string; mixed: MixedEol | null } {
  const head = st.bom ? "﻿" : "";
  const base = st.eolMixed;
  if (!base) return { text: head + doc.sliceString(0, undefined, st.eol), mixed: null };
  const cur = doc.toJSON();
  const n = cur.length;
  const m = base.lines.length;
  let p = 0;
  while (p < n && p < m && cur[p] === base.lines[p]) p++;
  let s = 0;
  while (s < n - p && s < m - p && cur[n - 1 - s] === base.lines[m - 1 - s]) s++;
  const ends: Eol[] = new Array(Math.max(0, n - 1));
  let out = head;
  for (let i = 0; i < n; i++) {
    out += cur[i];
    if (i === n - 1) break;
    // Unchanged head line that already had a line after it, or unchanged tail line: its own ending.
    const end = i < p && i < m - 1 ? base.ends[i] : i >= n - s ? base.ends[i - n + m] : st.eol;
    ends[i] = end;
    out += end;
  }
  return { text: out, mixed: { lines: cur, ends } };
}
