// Path helpers that work for both Windows ("C:\a\b") and POSIX ("/a/b") absolute paths.

export function sepOf(abs: string): string {
  return abs.includes("\\") && !abs.startsWith("/") ? "\\" : "/";
}

export function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i < 0 ? p : p.slice(i + 1);
}

export function dirname(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  if (i < 0) return "";
  if (i === 0) return p.slice(0, 1);
  return p.slice(0, i);
}

export function extname(p: string): string {
  const b = basename(p);
  const i = b.lastIndexOf(".");
  return i <= 0 ? "" : b.slice(i + 1).toLowerCase();
}

export function stripExt(name: string): string {
  const i = name.lastIndexOf(".");
  return i <= 0 ? name : name.slice(0, i);
}

/** Normalises "a/./b/../c" style relative paths (forward slashes). Leading ".." are kept. */
export function normalizeRel(rel: string): string {
  const out: string[] = [];
  for (const part of rel.replace(/\\/g, "/").split("/")) {
    if (!part || part === ".") continue;
    if (part === ".." && out.length && out[out.length - 1] !== "..") out.pop();
    else out.push(part);
  }
  return out.join("/");
}

/** Joins an absolute OS directory and a "/"-separated relative path, resolving ".." segments. */
export function joinAbs(dir: string, rel: string): string {
  const sep = sepOf(dir);
  const lead = dir.startsWith("/") ? "/" : "";
  const parts = dir.split(/[\\/]/).filter(Boolean);
  for (const part of rel.split(/[\\/]/)) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (parts.length > 1 || lead) parts.pop();
    } else parts.push(part);
  }
  return lead + parts.join(sep);
}

function cmpKey(p: string): string {
  return p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function samePath(a: string, b: string): boolean {
  return a === b || cmpKey(a) === cmpKey(b);
}

/** Vault-relative path ("/" separators) of `abs`, or null when `abs` is not inside `root`. */
export function toRel(root: string, abs: string): string | null {
  const r = cmpKey(root);
  const a = abs.replace(/\\/g, "/");
  if (a.length <= r.length + 1) return null;
  if (a.slice(0, r.length).toLowerCase() !== r || a[r.length] !== "/") return null;
  return a.slice(r.length + 1);
}

export function isAbsolute(p: string): boolean {
  return /^([a-zA-Z]:[\\/]|[\\/])/.test(p);
}

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "avif"]);
export function isImagePath(p: string): boolean {
  return IMAGE_EXT.has(extname(p));
}
export function isMarkdownPath(p: string): boolean {
  const e = extname(p);
  return e === "md" || e === "markdown";
}
