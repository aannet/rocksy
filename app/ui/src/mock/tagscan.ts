// Tag scanner used by the mock backend only (the real one lives in rocksy-core).
// Follows the "Règles des tags" section of the contract closely enough to exercise the UI.
import type { Fragment } from "../types";

const TAG = /(^|\s)#([\p{L}\p{N}_\-/]+)/gu;

function blank(s: string): string {
  return " ".repeat(s.length);
}

/** Masks inline code, wikilinks, markdown link targets/titles and URLs so tags inside are ignored. */
function mask(line: string): string {
  return line
    .replace(/(`+)[^`]*?\1/g, blank)
    .replace(/!?\[\[[^\]]*\]\]/g, blank)
    .replace(/\]\([^)]*\)/g, blank)
    .replace(/\b[a-z][a-z0-9+.-]*:\/\/\S+/gi, blank);
}

function excerpt(lines: string[], from: number, to: number): string {
  let s = "";
  for (let i = from; i <= to && s.length < 260; i++) s += (s ? " " : "") + lines[i].trim();
  s = s.replace(/\s+/g, " ");
  return s.length > 240 ? s.slice(0, 239) + "…" : s;
}

const indentOf = (l: string) => /^[ \t]*/.exec(l)![0].replace(/\t/g, "    ").length;
const isList = (l: string) => /^[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]+/.test(l);
const isQuote = (l: string) => /^[ \t]{0,3}>/.test(l);
const isHeading = (l: string) => /^[ \t]{0,3}#{1,6}[ \t]/.test(l);
const isTableRow = (l: string) => /^[ \t]*\|/.test(l);
const isFence = (l: string) => /^[ \t]*(`{3,}|~{3,})/.test(l);

export function scanTags(path: string, text: string): Fragment[] {
  const lines = text.split(/\r?\n/);
  const out: Fragment[] = [];
  let body = 0;
  if (lines[0]?.trim() === "---") {
    let end = -1;
    for (let i = 1; i < lines.length; i++) {
      if (/^(---|\.\.\.)\s*$/.test(lines[i])) {
        end = i;
        break;
      }
    }
    if (end > 0) {
      body = end + 1;
      const add = (raw: string, line: number) => {
        const tag = raw.trim().replace(/^["']|["']$/g, "").replace(/^#/, "");
        if (!tag || /^\d+$/.test(tag) || !/^[\p{L}\p{N}_\-/]+$/u.test(tag)) return;
        out.push({
          path,
          tag,
          source: "frontmatter",
          startLine: 0,
          endLine: lines.length - 1,
          tagLine: line,
          tagCol: Math.max(0, lines[line].indexOf(tag)),
          excerpt: excerpt(lines, body, Math.min(lines.length - 1, body + 6)),
        });
      };
      for (let i = 1; i < end; i++) {
        const m = /^(tags?)[ \t]*:[ \t]*(.*)$/i.exec(lines[i]);
        if (!m) continue;
        const v = m[2].trim();
        if (v) v.replace(/^\[|\]$/g, "").split(",").forEach((x) => add(x, i));
        else for (let j = i + 1; j < end && /^[ \t]*-[ \t]+/.test(lines[j]); j++) add(lines[j].replace(/^[ \t]*-[ \t]+/, ""), j);
      }
    }
  }
  let inFence = false;
  for (let i = body; i < lines.length; i++) {
    const line = lines[i];
    if (isFence(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence || !line.includes("#")) continue;
    const masked = mask(line);
    TAG.lastIndex = 0;
    let m: RegExpExecArray | null;
    let block: [number, number] | null = null;
    while ((m = TAG.exec(masked))) {
      const tag = m[2].replace(/[/]+$/, "");
      if (!tag || /^\d+$/.test(tag)) continue;
      if (isHeading(line) && m.index === 0 && m[1] === "") continue;
      block ??= blockAround(lines, i, body);
      out.push({
        path,
        tag,
        source: "inline",
        startLine: block[0],
        endLine: block[1],
        tagLine: i,
        tagCol: m.index + m[1].length,
        excerpt: excerpt(lines, block[0], block[1]),
      });
    }
  }
  return out;
}

/** Enclosing block of line `i`: heading alone, table row, whole quote, list item with children, paragraph. */
function blockAround(lines: string[], i: number, body: number): [number, number] {
  const line = lines[i];
  if (isHeading(line) || isTableRow(line)) return [i, i];
  if (isQuote(line)) {
    let a = i;
    let b = i;
    while (a > body && isQuote(lines[a - 1])) a--;
    while (b + 1 < lines.length && isQuote(lines[b + 1])) b++;
    return [a, b];
  }
  // List item: walk up to the item start, then down through deeper-indented lines.
  let start = i;
  while (start > body && !isList(lines[start]) && lines[start].trim() !== "" && indentOf(lines[start]) > 0 && !isHeading(lines[start])) start--;
  if (isList(lines[start])) {
    const ind = indentOf(lines[start]);
    let b = start;
    for (let j = start + 1; j < lines.length; j++) {
      const l = lines[j];
      if (l.trim() === "") {
        if (j + 1 < lines.length && lines[j + 1].trim() !== "" && indentOf(lines[j + 1]) > ind) continue;
        break;
      }
      if (indentOf(l) <= ind) break;
      b = j;
    }
    if (i <= b) return [start, b];
  }
  let a = i;
  let b = i;
  const stop = (l: string) => l.trim() === "" || isHeading(l) || isList(l) || isQuote(l) || isFence(l) || isTableRow(l);
  while (a > body && !stop(lines[a - 1])) a--;
  while (b + 1 < lines.length && !stop(lines[b + 1])) b++;
  return [a, b];
}
