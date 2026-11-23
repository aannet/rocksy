// Single access point to the Rust core. Commands, events and types follow ../CONTRACT.md to the letter.
import type {
  ApiError,
  BackendEvents,
  FileContent,
  Fragment,
  LinkRef,
  OutLink,
  SearchResult,
  UnresolvedLink,
  SaveResult,
  Session,
  StartupInfo,
  TagCount,
  Tree,
  VaultRef,
} from "./types";

export interface Backend {
  readonly kind: "tauri" | "mock";
  startupInfo(): Promise<StartupInfo>;
  readFile(path: string): Promise<FileContent>;
  saveFile(path: string, text: string, expectedMtimeMs?: number): Promise<SaveResult>;
  listVaults(): Promise<VaultRef[]>;
  addVault(path: string): Promise<VaultRef[]>;
  removeVault(path: string): Promise<VaultRef[]>;
  openVault(path: string): Promise<Tree>;
  buildTags(vaultPath: string): Promise<void>;
  tagsList(query: string): Promise<TagCount[]>;
  tagFragments(tag: string, limit?: number): Promise<Fragment[]>;
  fileTags(path: string, text: string): Promise<Fragment[]>;
  /** Full-text search in the indexed vault (`limit`: number of files, default 50). */
  search(query: string, limit?: number): Promise<SearchResult>;
  backlinks(path: string): Promise<LinkRef[]>;
  outgoingLinks(path: string): Promise<OutLink[]>;
  unresolvedLinks(limit?: number): Promise<UnresolvedLink[]>;
  setSession(session: Session): Promise<void>;
  pickFolder(): Promise<string | null>;
  pickFile(): Promise<string | null>;
  logTiming(name: string, ms: number): Promise<void>;
  /** Opens an http(s)/mailto link in the system browser or mail client (`open_external`); rejects with `invalid` otherwise. */
  openExternal(url: string): Promise<void>;
  /** Subscribes to a core event. Returns the unsubscribe function. */
  on<K extends keyof BackendEvents>(event: K, cb: (payload: BackendEvents[K]) => void): () => void;
  /** URL usable in `<img src>` for a local absolute path. */
  assetUrl(absPath: string): string;
  /** Files dropped on the window in browser mode (HTML5 drop). Under Tauri the core emits `open-files` instead. */
  onFileDrop(cb: (paths: string[]) => void): () => void;
}

export function toApiError(e: unknown): ApiError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as ApiError;
  if (typeof e === "string") {
    try {
      const o = JSON.parse(e);
      if (o && typeof o === "object" && "kind" in o) return o as ApiError;
    } catch {
      /* plain message */
    }
    return { kind: "io", message: e };
  }
  return { kind: "io", message: e instanceof Error ? e.message : String(e) };
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

interface TauriGlobal {
  core: {
    invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
    convertFileSrc(path: string, protocol?: string): string;
  };
  event: { listen<T>(event: string, cb: (e: { payload: T }) => void): Promise<() => void> };
}

/** Uses the global API (`withGlobalTauri`): nothing from @tauri-apps/api is bundled. */
function tauriBackend(): Backend {
  const tauri = (window as unknown as { __TAURI__: TauriGlobal }).__TAURI__;
  const { invoke, convertFileSrc } = tauri.core;
  const { listen } = tauri.event;
  const call = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
    invoke<T>(cmd, args).catch((e) => {
      throw toApiError(e);
    });
  const subscribe = <T>(event: string, cb: (payload: T) => void): (() => void) => {
    let off: (() => void) | null = null;
    let cancelled = false;
    void listen<T>(event, (e) => cb(e.payload)).then((un) => {
      if (cancelled) un();
      else off = un;
    });
    return () => {
      cancelled = true;
      off?.();
    };
  };
  return {
    kind: "tauri",
    startupInfo: () => call("startup_info"),
    readFile: (path) => call("read_file", { path }),
    saveFile: (path, text, expectedMtimeMs) => call("save_file", expectedMtimeMs == null ? { path, text } : { path, text, expectedMtimeMs }),
    listVaults: () => call("list_vaults"),
    addVault: (path) => call("add_vault", { path }),
    removeVault: (path) => call("remove_vault", { path }),
    openVault: (path) => call("open_vault", { path }),
    buildTags: (vaultPath) => call("build_tags", { vaultPath }),
    tagsList: (query) => call("tags_list", { query }),
    tagFragments: (tag, limit) => call("tag_fragments", limit == null ? { tag } : { tag, limit }),
    fileTags: (path, text) => call("file_tags", { path, text }),
    search: (query, limit) => call("search", limit == null ? { query } : { query, limit }),
    backlinks: (path) => call("backlinks", { path }),
    outgoingLinks: (path) => call("outgoing_links", { path }),
    unresolvedLinks: (limit) => call("unresolved_links", limit == null ? {} : { limit }),
    setSession: (session) => call("set_session", { session }),
    pickFolder: () => call("pick_folder"),
    pickFile: () => call("pick_file"),
    logTiming: (name, ms) => call("log_timing", { name, ms }),
    openExternal: (url) => call("open_external", { url }),
    on: (event, cb) => subscribe(event, cb),
    assetUrl: (absPath) => convertFileSrc(absPath),
    // Under Tauri the core intercepts drops and emits `open-files`: nothing to do here.
    onFileDrop: () => () => undefined,
  };
}

/** Tauri when running inside the app, the in-memory mock in an ordinary browser and in tests. */
export async function getBackend(): Promise<Backend> {
  if (window.__TAURI_INTERNALS__) return tauriBackend();
  const { createMockBackend } = await import("./mock/mock");
  return createMockBackend(new URLSearchParams(location.search));
}
