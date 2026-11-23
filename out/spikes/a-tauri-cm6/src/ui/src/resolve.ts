// Link and image resolution (pure logic, unit-tested).
import type { TreeEntry } from "./types";
import { basename, dirname, isAbsolute, joinAbs, normalizeRel, stripExt, toRel } from "./paths";

export interface WikiLink {
  target: string;
  heading: string | null;
  alias: string | null;
}

/** Parses the inside of `[[...]]`: `note`, `note|alias`, `note#heading`, `note#heading|alias`. */
export function parseWikilink(inner: string): WikiLink {
  let alias: string | null = null;
  let rest = inner;
  const bar = inner.indexOf("|");
  if (bar >= 0) {
    alias = inner.slice(bar + 1).trim() || null;
    rest = inner.slice(0, bar);
  }
  let heading: string | null = null;
  const hash = rest.indexOf("#");
  if (hash >= 0) {
    heading = rest.slice(hash + 1).trim() || null;
    rest = rest.slice(0, hash);
  }
  return { target: rest.trim(), heading, alias };
}

export function wikilinkDisplay(l: WikiLink): string {
  if (l.alias) return l.alias;
  if (l.heading) return l.target ? `${l.target} > ${l.heading}` : l.heading;
  return l.target;
}

/** Lookup structure over the files of a vault tree. */
export class FileIndex {
  /** lower-cased relative path -> original relative path */
  private byPath = new Map<string, string>();
  /** lower-cased basename (with extension) -> relative paths */
  private byName = new Map<string, string[]>();
  readonly files: string[] = [];

  constructor(entries: readonly TreeEntry[]) {
    for (const e of entries) {
      if (e.isDir) continue;
      this.files.push(e.path);
      this.byPath.set(e.path.toLowerCase(), e.path);
      const name = basename(e.path).toLowerCase();
      const list = this.byName.get(name);
      if (list) list.push(e.path);
      else this.byName.set(name, [e.path]);
    }
  }

  has(rel: string): string | null {
    return this.byPath.get(rel.toLowerCase()) ?? null;
  }

  named(name: string): string[] {
    return this.byName.get(name.toLowerCase()) ?? [];
  }
}

function pickClosest(candidates: string[], fromDir: string): string {
  if (candidates.length === 1) return candidates[0];
  const same = candidates.find((c) => dirname(c).toLowerCase() === fromDir.toLowerCase());
  if (same) return same;
  // Shortest path wins (Obsidian's "shortest path when possible"), then alphabetical for stability.
  return [...candidates].sort((a, b) => a.split("/").length - b.split("/").length || a.localeCompare(b))[0];
}

/**
 * Resolves a wikilink / markdown link target to a vault-relative path.
 * `fromRel` is the vault-relative path of the file containing the link ("" when unknown).
 * Order: exact path from the vault root, path relative to the file, then lookup by file name.
 * A target without extension means a Markdown note.
 */
export function resolveLink(target: string, fromRel: string, index: FileIndex): string | null {
  let t = target.trim().replace(/\\/g, "/");
  if (!t) return fromRel || null;
  try {
    t = decodeURIComponent(t);
  } catch {
    /* keep raw */
  }
  const fromDir = dirname(fromRel);
  const hasExt = /\.[a-zA-Z0-9]{1,10}$/.test(basename(t));
  const variants = hasExt ? [t] : [t + ".md", t];
  for (const v of variants) {
    const clean = normalizeRel(v);
    if (!clean.startsWith("..")) {
      const exact = index.has(clean);
      if (exact) return exact;
    }
    const rel = normalizeRel(fromDir ? fromDir + "/" + v : v);
    if (!rel.startsWith("..")) {
      const hit = index.has(rel);
      if (hit) return hit;
    }
  }
  for (const v of variants) {
    const candidates = index.named(basename(v));
    if (candidates.length) {
      // When a folder part was given, keep candidates whose path ends with it.
      const suffix = "/" + normalizeRel(v).toLowerCase();
      const narrowed = v.includes("/") ? candidates.filter((c) => ("/" + c.toLowerCase()).endsWith(suffix)) : candidates;
      if (narrowed.length) return pickClosest(narrowed, fromDir);
    }
  }
  return null;
}

export interface ImageContext {
  /** Absolute path of the note containing the image. */
  fileAbs: string;
  /** Root of the open vault, or null (file opened outside a vault). */
  vaultRoot: string | null;
  index: FileIndex | null;
}

export type ImageTarget = { kind: "local"; abs: string } | { kind: "remote"; url: string } | { kind: "missing" };

/**
 * Resolves an image source (`![[x.png]]` or `![alt](rel.png)`).
 * Order: path relative to the file, then (in a vault) path from the vault root, then lookup by name in the tree.
 * Outside a vault there is no tree: the path relative to the file is returned as is.
 */
export function resolveImage(src: string, ctx: ImageContext): ImageTarget {
  let s = src.trim();
  if (!s) return { kind: "missing" };
  if (/^(https?:|data:)/i.test(s)) return { kind: "remote", url: s };
  if (s.startsWith("<") && s.endsWith(">")) s = s.slice(1, -1);
  try {
    s = decodeURIComponent(s);
  } catch {
    /* keep raw */
  }
  s = s.replace(/^file:\/\/\/?/i, "");
  if (isAbsolute(s)) return { kind: "local", abs: s };
  const fileDir = dirname(ctx.fileAbs);
  const fileRel = ctx.vaultRoot ? toRel(ctx.vaultRoot, ctx.fileAbs) : null;
  if (!ctx.vaultRoot || !ctx.index || fileRel == null) return { kind: "local", abs: joinAbs(fileDir, s) };
  const rel = resolveLink(s, fileRel, ctx.index);
  if (rel) return { kind: "local", abs: joinAbs(ctx.vaultRoot, rel) };
  return { kind: "missing" };
}

/** Text shown in the wikilink completion list for a file (name without `.md`). */
export function linkTextFor(rel: string, index: FileIndex): string {
  const name = basename(rel);
  const isMd = /\.md$/i.test(name);
  const short = isMd ? stripExt(name) : name;
  return index.named(name).length > 1 ? (isMd ? stripExt(rel) : rel) : short;
}
