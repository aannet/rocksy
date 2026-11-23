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
export type SearchResult = { query: string; total: number; hits: SearchHit[]; ms: number; searchReady: boolean };
export type SearchHit = {
  path: string;
  score: number;
  nameMatch: boolean;
  matchCount: number;
  matches: SearchMatch[];
};
export type SearchMatch = {
  line: number;
  col: number;
  len: number;
  excerpt: string;
  ranges: [number, number][];
};
export type LinkKind = "link" | "embed";
export type LinkRef = {
  path: string;
  kind: LinkKind;
  line: number;
  col: number;
  excerpt: string;
  ranges: [number, number][];
  heading: string | null;
  alias: string | null;
};
export type OutLink = {
  target: string;
  resolved: string | null;
  kind: LinkKind;
  line: number;
  col: number;
  heading: string | null;
  alias: string | null;
};
export type UnresolvedLink = { target: string; count: number; sources: string[] };
export type IndexReady = {
  fromCache: boolean;
  files: number;
  tags: number;
  occurrences: number;
  links: number;
  unresolvedLinks: number;
  searchReady: boolean;
  searchDocs: number;
  reread?: number;
  removed?: number;
  ms: number;
};
export type ApiErrorKind = "not_found" | "not_utf8" | "conflict" | "io" | "invalid";
export type ApiError = { kind: ApiErrorKind; message: string };

export type BackendEvents = {
  "open-files": { paths: string[] };
  "tree-updated": Tree;
  "tags-progress": { done: number; total: number };
  "tags-ready": { files: number; tags: number; occurrences: number; ms: number; fromCache?: boolean; reread?: number; removed?: number; persisted?: boolean };
  "index-ready": IndexReady;
  "file-changed": { path: string; mtimeMs: number };
};
