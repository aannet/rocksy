// Types of the interface contract (../CONTRACT.md). Keep in sync, do not extend here.
export type StartupInfo = {
  argFiles: string[];
  vaults: VaultRef[];
  session: Session | null;
  dataDir: string;
  version: string;
  processStartMs: number;
  /** The note the UI will open first, read by the backend while the WebView starts; null when not available. */
  firstFile: FileContent | null;
};
export type VaultRef = { path: string; name: string };
export type Session = { activeVault: string | null; tabs: string[]; activeTab: string | null };
/** `readOnly`: `save_file` would be refused for this path (non-Markdown extension, or writing not allowed). */
export type FileContent = { path: string; text: string; mtimeMs: number; size: number; readOnly: boolean };
export type SaveResult = { mtimeMs: number; size: number };
export type Tree = { root: string; fromSnapshot: boolean; entries: TreeEntry[] };
export type TreeEntry = { path: string; isDir: boolean };
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
  /** Notes the last pass could not bring up to date (read failed); the backend reads them again by itself. Absent when the event does not come from a pass. */
  pending?: number;
  ms: number;
  /** Changes when a link of the vault changes (target, position) or when the set of files changes. */
  linksGeneration: number;
  /** Root of the vault the event is about (same form as `Tree.root`). */
  vault: string;
};
export type ApiErrorKind = "not_found" | "not_utf8" | "conflict" | "io" | "invalid";
/** Stable reason of an error, when the UI is expected to react to it (the message is English text and may change). */
export type ApiErrorCode = "too_large" | "network_path" | "not_allowed" | "read_only" | "gone";
export type ApiError = { kind: ApiErrorKind; message: string; code?: ApiErrorCode };

export type BackendEvents = {
  "open-files": { paths: string[] };
  "tree-updated": Tree;
  "tags-progress": { done: number; total: number; vault: string };
  "tags-ready": { files: number; tags: number; occurrences: number; ms: number; fromCache?: boolean; reread?: number; removed?: number; persisted?: boolean; vault: string };
  "index-ready": IndexReady;
  /** `deleted: true`: an open file disappeared (deleted or moved). */
  "file-changed": { path: string; mtimeMs: number; deleted?: boolean };
  /** The user asked to close the window; the window does not close by itself (answer with `close_ack`, then `quit`). */
  "close-requested": null;
};
