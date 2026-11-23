import type { Fragment, TagCount } from "./types";

export interface TagNode {
  /** Last segment, original case. */
  name: string;
  /** Full tag without '#'. */
  full: string;
  count: number;
  children: TagNode[];
}

/**
 * Builds the tag hierarchy from the flat list returned by `tags_list`.
 * A parent `a` already aggregates `a/b` (contract); a parent missing from the list is synthesised
 * with the sum of its direct children.
 */
const collator = new Intl.Collator(undefined, { sensitivity: "base" });

function depthOf(tag: string): number {
  let d = 0;
  for (let i = 0; i < tag.length; i++) if (tag.charCodeAt(i) === 47) d++;
  return d;
}

export function buildTagTree(tags: readonly TagCount[]): TagNode[] {
  const roots: TagNode[] = [];
  const byKey = new Map<string, TagNode>();
  const listed = new Set<TagNode>();
  // Parents before children: bucket by depth (cheaper than sorting several thousand strings).
  const buckets: TagCount[][] = [];
  for (const t of tags) {
    if (!t.tag) continue;
    const d = depthOf(t.tag);
    (buckets[d] ??= []).push(t);
  }
  const ensure = (full: string): TagNode => {
    const key = full.toLowerCase();
    let node = byKey.get(key);
    if (node) return node;
    const cut = full.lastIndexOf("/");
    node = { name: cut < 0 ? full : full.slice(cut + 1), full, count: 0, children: [] };
    byKey.set(key, node);
    if (cut < 0) roots.push(node);
    else ensure(full.slice(0, cut)).children.push(node);
    return node;
  };
  for (const bucket of buckets) {
    if (!bucket) continue;
    for (const t of bucket) {
      const node = ensure(t.tag);
      node.count = t.count;
      listed.add(node);
    }
  }
  const byName = (a: TagNode, b: TagNode) => collator.compare(a.name, b.name);
  const fix = (nodes: TagNode[]) => {
    for (const n of nodes) {
      if (n.children.length) fix(n.children);
      if (!listed.has(n)) n.count = n.children.reduce((s, c) => s + c.count, 0);
    }
    if (nodes.length > 1) nodes.sort(byName);
  };
  fix(roots);
  return roots;
}

/** Aggregated tag counts from fragments (used for a single file: `file_tags`). Parents aggregate children. */
export function countTags(fragments: readonly Fragment[]): TagCount[] {
  const counts = new Map<string, TagCount>();
  for (const f of fragments) {
    const parts = f.tag.split("/");
    for (let i = 1; i <= parts.length; i++) {
      const full = parts.slice(0, i).join("/");
      const key = full.toLowerCase();
      const c = counts.get(key);
      if (c) c.count++;
      else counts.set(key, { tag: full, count: 1 });
    }
  }
  return [...counts.values()].sort((a, b) => collator.compare(a.tag, b.tag));
}

/** Case-insensitive filter used for the single-file mode (the vault mode asks the backend). */
export function filterTags(tags: readonly TagCount[], query: string): TagCount[] {
  const q = query.trim().replace(/^#/, "").toLowerCase();
  if (!q) return [...tags];
  return tags.filter((t) => t.tag.toLowerCase().includes(q));
}

export function tagMatches(fragmentTag: string, tag: string): boolean {
  const a = fragmentTag.toLowerCase();
  const b = tag.toLowerCase();
  return a === b || a.startsWith(b + "/");
}
