// In-memory search and link index of the mock backend. Same semantics as the contract ("Recherche", "Liens").
import type { LinkKind, LinkRef, OutLink, SearchHit, SearchMatch, SearchResult, UnresolvedLink } from "../types";
import { excerptAround, foldText, parseQuery, termRanges } from "../excerpt";
import { FileIndex, resolveLink } from "../resolve";
import { joinAbs } from "../paths";

interface RawLink {
  target: string;
  heading: string | null;
  alias: string | null;
  kind: LinkKind;
  line: number;
  col: number;
  len: number;
  markdown: boolean;
}

interface Doc {
  rel: string;
  abs: string;
  text: string;
  folded: string;
  foldedName: string;
  foldedRel: string;
  tags: string[];
  links: RawLink[];
}

function mask(line: string): string {
  return line.replace(/(`+)[^`]*?\1/g, (m) => " ".repeat(m.length));
}

export function parseLinks(text: string): RawLink[] {
  const out: RawLink[] = [];
  const lines = text.split(/\r?\n/);
  let fence = false;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    if (/^[ \t]*(`{3,}|~{3,})/.test(raw)) {
      fence = !fence;
      continue;
    }
    if (fence || raw.indexOf("[") < 0) continue;
    const line = mask(raw);
    const re = /(!?)\[\[([^\]\n]+)\]\]|(!?)\[([^\]\n]*)\]\(([^)\s]+)[^)]*\)/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line))) {
      if (m[2] != null) {
        let inner = m[2];
        let alias: string | null = null;
        const bar = inner.indexOf("|");
        if (bar >= 0) {
          alias = inner.slice(bar + 1).trim() || null;
          inner = inner.slice(0, bar);
        }
        let heading: string | null = null;
        const hash = inner.indexOf("#");
        if (hash >= 0) {
          heading = inner.slice(hash + 1).trim() || null;
          inner = inner.slice(0, hash);
        }
        const target = inner.trim();
        if (!target) continue; // [[#titre]] : internal link, ignored
        out.push({ target, heading, alias, kind: m[1] ? "embed" : "link", line: i, col: m.index, len: m[0].length, markdown: false });
      } else {
        let dest = m[5];
        if (/^[a-z][a-z0-9+.-]*:/i.test(dest) || dest.startsWith("#")) continue;
        let heading: string | null = null;
        const hash = dest.indexOf("#");
        if (hash >= 0) {
          heading = dest.slice(hash + 1) || null;
          dest = dest.slice(0, hash);
        }
        try {
          dest = decodeURIComponent(dest);
        } catch {
          /* keep raw */
        }
        out.push({ target: dest, heading, alias: m[4] || null, kind: m[3] ? "embed" : "link", line: i, col: m.index, len: m[0].length, markdown: true });
      }
    }
  }
  return out;
}

export class MockIndex {
  private docs = new Map<string, Doc>();
  private files: FileIndex | null = null;
  private root = "";
  private back: Map<string, { doc: Doc; link: RawLink }[]> | null = null;
  private unresolvedCache: UnresolvedLink[] | null = null;
  private resolvedCount = 0;
  searchReady = false;

  reset(root: string, files: FileIndex): void {
    this.docs.clear();
    this.root = root;
    this.files = files;
    this.back = null;
    this.unresolvedCache = null;
    this.searchReady = false;
  }

  setFiles(files: FileIndex): void {
    this.files = files;
    this.back = null;
    this.unresolvedCache = null;
  }

  add(rel: string, text: string, tags: string[]): void {
    const name = rel.slice(rel.lastIndexOf("/") + 1).replace(/\.md$/i, "");
    this.docs.set(rel.toLowerCase(), {
      rel,
      abs: joinAbs(this.root, rel),
      text,
      folded: foldText(text),
      foldedName: foldText(name),
      foldedRel: foldText(rel),
      tags: tags.map((t) => t.toLowerCase()),
      links: parseLinks(text),
    });
    this.back = null;
    this.unresolvedCache = null;
  }

  get size(): number {
    return this.docs.size;
  }

  private resolve(doc: Doc, link: RawLink): string | null {
    if (!this.files) return null;
    return resolveLink(link.target, doc.rel, this.files);
  }

  private ensureBack(): void {
    if (this.back) return;
    const back = new Map<string, { doc: Doc; link: RawLink }[]>();
    const missing = new Map<string, UnresolvedLink>();
    let resolved = 0;
    for (const doc of this.docs.values()) {
      for (const link of doc.links) {
        const rel = this.resolve(doc, link);
        if (rel) {
          resolved++;
          const key = rel.toLowerCase();
          if (key === doc.rel.toLowerCase()) continue;
          const list = back.get(key);
          if (list) list.push({ doc, link });
          else back.set(key, [{ doc, link }]);
        } else {
          const key = link.target.toLowerCase();
          const u = missing.get(key);
          if (u) {
            u.count++;
            if (u.sources.length < 5 && !u.sources.includes(doc.abs)) u.sources.push(doc.abs);
          } else missing.set(key, { target: link.target, count: 1, sources: [doc.abs] });
        }
      }
    }
    for (const list of back.values()) list.sort((a, b) => a.doc.rel.localeCompare(b.doc.rel) || a.link.line - b.link.line || a.link.col - b.link.col);
    this.back = back;
    this.resolvedCount = resolved;
    this.unresolvedCache = [...missing.values()].sort((a, b) => b.count - a.count || a.target.localeCompare(b.target));
  }

  stats(): { links: number; unresolvedLinks: number } {
    this.ensureBack();
    let unresolved = 0;
    for (const u of this.unresolvedCache!) unresolved += u.count;
    return { links: this.resolvedCount + unresolved, unresolvedLinks: unresolved };
  }

  backlinks(rel: string): LinkRef[] {
    this.ensureBack();
    const list = this.back!.get(rel.toLowerCase()) ?? [];
    return list.map(({ doc, link }) => {
      const lineText = doc.text.split(/\r?\n/, link.line + 1)[link.line] ?? "";
      const ex = excerptAround(lineText, link.col, link.col + link.len);
      return {
        path: doc.abs,
        kind: link.kind,
        line: link.line,
        col: link.col,
        excerpt: ex.text,
        ranges: [[link.col - ex.shift, Math.min(ex.text.length, link.col + link.len - ex.shift)]],
        heading: link.heading,
        alias: link.alias,
      };
    });
  }

  outgoing(rel: string): OutLink[] {
    const doc = this.docs.get(rel.toLowerCase());
    if (!doc) return [];
    return doc.links.map((link) => {
      const r = this.resolve(doc, link);
      return { target: link.target, resolved: r ? joinAbs(this.root, r) : null, kind: link.kind, line: link.line, col: link.col, heading: link.heading, alias: link.alias };
    });
  }

  unresolved(limit = 500): UnresolvedLink[] {
    this.ensureBack();
    return this.unresolvedCache!.slice(0, limit);
  }

  search(query: string, limit = 50): SearchResult {
    const t0 = performance.now();
    if (!this.searchReady) return { query, total: 0, hits: [], ms: 0, searchReady: false };
    const q = parseQuery(query);
    const positive = q.words.length + q.phrases.length;
    const hits: SearchHit[] = [];
    if (!positive && !q.paths.length && !q.tags.length) return { query, total: 0, hits: [], ms: 0, searchReady: true };
    for (const doc of this.docs.values()) {
      if (q.paths.length && !q.paths.every((p) => doc.foldedRel.includes(p))) continue;
      if (q.tags.length && !q.tags.every((t) => doc.tags.some((d) => d === t || d.startsWith(t + "/")))) continue;
      const ft = doc.folded;
      let skip = false;
      for (const ex of q.exclude) {
        if (termRanges(ft, ex, true, 1).length || termRanges(doc.foldedName, ex, true, 1).length) {
          skip = true;
          break;
        }
      }
      if (skip) continue;
      const occ: [number, number][] = [];
      let score = 0;
      let nameHits = 0;
      let ok = true;
      for (let i = 0; i < q.words.length; i++) {
        const w = q.words[i];
        const whole = !(q.lastIsPrefix && i === q.words.length - 1);
        if (ft.indexOf(w) < 0 && doc.foldedName.indexOf(w) < 0) {
          ok = false;
          break;
        }
        const body = termRanges(ft, w, whole, 200);
        const inName = termRanges(doc.foldedName, w, whole, 1).length > 0;
        if (!body.length && !inName) {
          ok = false;
          break;
        }
        if (inName) nameHits++;
        score += Math.min(body.length, 20) + (inName ? 25 : 0);
        for (const r of body) occ.push(r);
      }
      if (!ok) continue;
      for (const p of q.phrases) {
        const body = termRanges(ft, p, false, 200);
        const inName = doc.foldedName.includes(p);
        if (!body.length && !inName) {
          ok = false;
          break;
        }
        if (inName) nameHits++;
        score += Math.min(body.length, 20) * 2 + (inName ? 30 : 0);
        for (const r of body) occ.push(r);
      }
      if (!ok) continue;
      occ.sort((a, b) => a[0] - b[0]);
      const matches: SearchMatch[] = [];
      if (occ.length) {
        // Group occurrences by line; keep the first three lines.
        const text = doc.text;
        let lineNo = 0;
        let lineStart = 0;
        let cursor = 0;
        let oi = 0;
        while (oi < occ.length && matches.length < 3) {
          const pos = occ[oi][0];
          for (let nl = text.indexOf("\n", cursor); nl >= 0 && nl < pos; nl = text.indexOf("\n", cursor)) {
            lineNo++;
            lineStart = nl + 1;
            cursor = nl + 1;
          }
          let lineEnd = text.indexOf("\n", lineStart);
          if (lineEnd < 0) lineEnd = text.length;
          const rawLine = text.slice(lineStart, lineEnd).replace(/\r$/, "");
          const col = pos - lineStart;
          const ex = excerptAround(rawLine, col, occ[oi][1] - lineStart);
          const firstLen = occ[oi][1] - occ[oi][0];
          const ranges: [number, number][] = [];
          while (oi < occ.length && occ[oi][0] < lineEnd) {
            const a = occ[oi][0] - lineStart - ex.shift;
            const b = occ[oi][1] - lineStart - ex.shift;
            if (a >= 0 && b <= ex.text.length) ranges.push([a, b]);
            oi++;
          }
          matches.push({ line: lineNo, col, len: firstLen, excerpt: ex.text, ranges });
          cursor = lineStart;
        }
      }
      hits.push({ path: doc.abs, score: score + (positive ? 0 : 1), nameMatch: positive > 0 && nameHits === positive, matchCount: occ.length, matches });
    }
    hits.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
    return { query, total: hits.length, hits: hits.slice(0, limit), ms: Math.round((performance.now() - t0) * 10) / 10, searchReady: true };
  }
}
