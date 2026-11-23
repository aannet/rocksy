// Types of the interface contract (../CONTRACT.md). Keep in sync, do not extend here.
export type StartupInfo = {
  argFiles: string[];
  vaults: VaultRef[];
  session: Session | null;
  dataDir: string;
  version: string;
  processStartMs: number;
};
export type VaultRef = { path: string; name: string };
export type Session = { activeVault: string | null; tabs: string[]; activeTab: string | null };
export type FileContent = { path: string; text: string; mtimeMs: number; size: number };
export type SaveResult = { mtimeMs: number; size: number };
export type Tree = { root: string; fromSnapshot: boolean; entries: TreeEntry[] };
export type TreeEntry = { path: string; isDir: boolean; size: number; mtimeMs: number };
export type TagCount = { tag: string; count: number };
export type Fragment = {
  path: string;
  tag: string;
  source: "inline" | "frontmatter";
  startLine: number;
  endLine: number;
  tagLine: number;
  tagCol: number;
  excerpt: string;
};
export type ApiErrorKind = "not_found" | "not_utf8" | "conflict" | "io" | "invalid";
export type ApiError = { kind: ApiErrorKind; message: string };

export type BackendEvents = {
  "open-files": { paths: string[] };
  "tree-updated": Tree;
  "tags-progress": { done: number; total: number };
  "tags-ready": { files: number; tags: number; occurrences: number; ms: number; fromCache?: boolean; reread?: number; removed?: number };
  "file-changed": { path: string; mtimeMs: number };
};
