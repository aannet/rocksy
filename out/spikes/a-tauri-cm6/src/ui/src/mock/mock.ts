// In-memory backend used in an ordinary browser and by the headless tests.
// It simulates a vault (tree, a ~320 KB note, tags, images) with realistic delays.
// It follows the contract, including what the backend refuses: paths outside the registered vaults and the files
// opened explicitly (`invalid`), saving anything but `.md` / `.markdown`, files over 50 MiB, `limit` capped at 1000.
// URL parameters: tree=<entries> start=session|arg|fresh|novault open=<vault-relative path> delay=<factor> bigkb=<KB>
// firstfile=0 (no `StartupInfo.firstFile`)
import type { Backend } from "../backend";
import type { ApiError, BackendEvents, FileContent, Fragment, Session, TagCount, Tree, TreeEntry, VaultRef } from "../types";
import { basename, dirname, extname, isImagePath, isMarkdownPath, isNetworkPath, joinAbs, samePath, toRel } from "../paths";
import { DEMO_BLOCKS, DEMO_CALLOUTS, DEMO_CANVAS, DEMO_LIVE_PREVIEW, DEMO_OUTSIDE, bigNote, hash, smallNote, svgImage, treeFiles } from "./gen";
import { scanTags } from "./tagscan";
import { MockIndex, parseLinks } from "./index";
import { FileIndex } from "../resolve";

const ROOT = "C:\\Vaults\\Demo";
const ROOT2 = "D:\\Vaults\\Second";
const OUTSIDE = "C:\\Temp\\Notes\\hors-vault.md";
/** Next to OUTSIDE: readable through a link (folder of an opened file), never writable. */
const NEIGHBOUR = "C:\\Temp\\Notes\\voisin.md";
const NEIGHBOUR_TXT = "C:\\Temp\\Notes\\memo.txt";
/** Exists but lies outside every vault and every opened folder: `invalid`. */
const FORBIDDEN = "C:\\Prive\\secret.md";
const MAX_READ_BYTES = 50 * 1024 * 1024;
const MAX_RESULTS = 1000;
const BIG_REL = "01__WORK/__DAILY NOTES DEMO.md";

interface MockFile {
  text: string;
  mtimeMs: number;
  /** Size reported instead of the text length (to simulate a huge file without allocating it). */
  size?: number;
}

export interface MockControl {
  root: string;
  outside: string;
  bigPath: string;
  timings: { name: string; ms: number }[];
  saves: { path: string; size: number; expectedMtimeMs?: number }[];
  sessions: Session[];
  calls: string[];
  genMs: Record<string, number>;
  /** Simulates an external modification of a file (other machine, Drive sync). */
  touch(path: string, text?: string): void;
  /** Simulates the deletion (or move) of a file by another tool: `file-changed` with `deleted: true`. */
  remove(path: string): void;
  /** Creates or replaces a file; `size` overrides the reported size (limit tests). */
  setFile(path: string, text: string, size?: number): void;
  /** Content as "on disk" (what `save_file` received last), or null. */
  textOf(path: string): string | null;
  /** Records a path as opened explicitly by the user (command line, dialog, drop). */
  allowOpened(path: string): void;
  emit<K extends keyof BackendEvents>(event: K, payload: BackendEvents[K]): void;
  tagsReady: boolean;
  searchReady: boolean;
  /** The next saves fail with these errors, in order (then saves work again). */
  failSaves: ApiError[];
  /** Extra delay of `save_file` / `read_file`, in ms (a locked file is retried for about 2 s by the real backend). */
  saveDelayMs: number;
  readDelayMs: number;
  closeAcks: number;
  quits: number;
  closedVaults: string[];
  /** Saves that were in flight or done when `quit` was called: [done, started]. */
  savesAtQuit: [number, number] | null;
}

declare global {
  interface Window {
    rocksyMock?: MockControl;
  }
}

export function createMockBackend(params: URLSearchParams): Backend {
  return createMock(params).backend;
}

/** The mock backend with its control handle (tests under Node have no `window.rocksyMock`). */
export function createMock(params: URLSearchParams): { backend: Backend; control: MockControl } {
  const treeCount = Number(params.get("tree") ?? 1200);
  const start = params.get("start") ?? "session";
  const delayFactor = Number(params.get("delay") ?? 1);
  const bigBytes = Number(params.get("bigkb") ?? 320) * 1000;
  const t0 = Date.now() - Math.round(performance.now());

  const sleep = (ms: number) => new Promise<void>((r) => (ms * delayFactor > 0 ? setTimeout(r, ms * delayFactor) : r()));
  /** Same `kind`, wording and `code` as the core. */
  const fail = (kind: ApiError["kind"], message: string, code?: ApiError["code"]): never => {
    throw (code ? { kind, message, code } : { kind, message }) satisfies ApiError;
  };

  const files = new Map<string, MockFile>();
  const key = (abs: string) => abs.replace(/\//g, "\\").toLowerCase();
  const images = new Map<string, string>();
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  /** Files opened explicitly by the user: kept by the backend, never supplied by the page. */
  const opened = new Set<string>();
  const deleted = new Set<string>();
  /** Folders returned by `pick_folder` during this session: the only ones `add_vault` accepts. */
  const pickedFolders = new Set<string>();
  const emit = <K extends keyof BackendEvents>(event: K, payload: BackendEvents[K]) => {
    // `open-files` comes from the OS (second launch, drop): those paths become opened files.
    if (event === "open-files") for (const p of (payload as BackendEvents["open-files"]).paths) opened.add(key(p));
    listeners.get(event)?.forEach((cb) => cb(payload));
  };
  let savesStarted = 0;
  const baseMtime = Date.UTC(2026, 10, 20, 8, 0, 0);

  const fixed: Record<string, () => string> = {
    "00__DEMO/Live Preview.md": () => DEMO_LIVE_PREVIEW,
    "00__DEMO/Tableaux et code.md": () => DEMO_BLOCKS,
    "00__DEMO/Callouts.md": () => DEMO_CALLOUTS,
    "00__DEMO/Carte.canvas": () => DEMO_CANVAS,
    "00__DEMO/Journal.md": () => "# Journal\n\nNote courte liée depuis la démonstration. #demo\n",
    [BIG_REL]: () => {
      const t = performance.now();
      const s = bigNote(bigBytes);
      control.genMs.bigNote = Math.round((performance.now() - t) * 10) / 10;
      return s;
    },
  };
  images.set(key(joinAbs(ROOT, "_ASSETS/schema.png")), svgImage("schema.png (image fabriquée)", 640, 300, 215));
  images.set(key(joinAbs(ROOT, "_ASSETS/photo.png")), svgImage("photo.png (image fabriquée)", 480, 300, 160));
  images.set(key("C:\\Temp\\Notes\\img\\local.png"), svgImage("img/local.png, relative au fichier", 420, 220, 280));
  files.set(key(OUTSIDE), { text: DEMO_OUTSIDE, mtimeMs: baseMtime });
  files.set(key(NEIGHBOUR), { text: "# Voisin\n\nNote atteinte par un lien depuis un fichier hors vault : lecture seule.\n", mtimeMs: baseMtime });
  files.set(key(NEIGHBOUR_TXT), { text: "Fichier texte : Rocksy n'enregistre que le Markdown.\n", mtimeMs: baseMtime });
  files.set(key(FORBIDDEN), { text: "# Hors périmètre\n", mtimeMs: baseMtime });
  opened.add(key(OUTSIDE));

  let vaults: VaultRef[] = start === "fresh" ? [] : [{ path: ROOT, name: "Demo" }];
  interface MockTree {
    rels: string[];
    entries: TreeEntry[];
    byRel: Map<string, TreeEntry>;
  }
  const trees = new Map<string, MockTree>();

  function buildTree(root: string): MockTree {
    const cached = trees.get(key(root));
    if (cached) return cached;
    const t = performance.now();
    let rels: string[];
    if (samePath(root, ROOT)) {
      rels = [...Object.keys(fixed), "_ASSETS/schema.png", "_ASSETS/photo.png", ...treeFiles(Math.max(0, treeCount - 12))];
    } else {
      rels = ["Accueil.md", "Notes/Idées en vrac.md", "Notes/Lecture.md", "Projets/Plan.md", "Projets/Budget.md", "Images/croquis.png"];
    }
    const dirs = new Set<string>();
    for (const r of rels) {
      let i = r.indexOf("/");
      while (i >= 0) {
        dirs.add(r.slice(0, i));
        i = r.indexOf("/", i + 1);
      }
    }
    const entries: TreeEntry[] = [];
    for (const d of dirs) entries.push({ path: d, isDir: true });
    for (const r of rels) entries.push({ path: r, isDir: false });
    // Contract order: directories first, case-insensitive alphabetical.
    entries.sort((a, b) => (a.isDir === b.isDir ? a.path.toLowerCase().localeCompare(b.path.toLowerCase()) : a.isDir ? -1 : 1));
    const byRel = new Map<string, TreeEntry>();
    for (const e of entries) if (!e.isDir) byRel.set(e.path.toLowerCase(), e);
    const built = { rels, entries, byRel };
    trees.set(key(root), built);
    control.genMs["tree:" + basename(root)] = Math.round((performance.now() - t) * 10) / 10;
    return built;
  }

  /** Registered vault containing `abs`, if any. */
  function vaultOf(abs: string): { root: string; rel: string } | null {
    for (const v of vaults) {
      const rel = toRel(v.path, abs);
      if (rel != null) return { root: v.path, rel };
    }
    return null;
  }

  const isVault = (path: string) => vaults.some((v) => samePath(v.path, path));

  /** Contract, "Accès aux fichiers": may the page read this path? */
  function canRead(abs: string): boolean {
    const k = key(abs);
    if (vaultOf(abs) || opened.has(k)) return true;
    if (isNetworkPath(abs)) return false;
    const ext = extname(abs);
    if (!(isMarkdownPath(abs) || ext === "txt" || ext === "canvas" || isImagePath(abs))) return false;
    for (const o of opened) if (k.startsWith(key(dirname(o)) + "\\")) return true;
    return false;
  }

  /** Contract: writing is allowed under a registered vault or in a file opened explicitly, `.md` / `.markdown` only. */
  const canWrite = (abs: string) => !!vaultOf(abs) || opened.has(key(abs));
  const writable = (abs: string) => canWrite(abs) && isMarkdownPath(abs) && !/[\\/]\.obsidian[\\/]/i.test(abs);

  function load(abs: string): MockFile | null {
    const k = key(abs);
    if (deleted.has(k)) return null;
    const hit = files.get(k);
    if (hit) return hit;
    const v = vaultOf(abs);
    if (!v) return null;
    const tree = buildTree(v.root);
    const entry = tree.byRel.get(v.rel.toLowerCase());
    if (!entry) return null;
    const text = samePath(v.root, ROOT) && fixed[entry.path] ? fixed[entry.path]() : /\.(md|canvas)$/i.test(entry.path) ? (entry.path.endsWith(".canvas") ? DEMO_CANVAS : smallNote(entry.path)) : "";
    const f = { text, mtimeMs: baseMtime - (hash(entry.path) % 9_000_000) * 100 };
    files.set(k, f);
    return f;
  }

  // ---- tags (in-memory library)
  const fragsByFile = new Map<string, Fragment[]>();
  let tagCounts: TagCount[] | null = null;
  let tagRun = 0;
  // Search and links: fed by the same pass as the tags (one read per note), search usable a little later.
  const index = new MockIndex();
  const searchDelay = Number(params.get("searchdelay") ?? 900);
  let indexRoot = "";
  // Changes when a link changes or when the set of files changes (contract: `IndexReady.linksGeneration`).
  let linksGeneration = 0;
  const linkSigs = new Map<string, string>();
  const noteLinks = (rel: string, text: string) => {
    const sig = JSON.stringify(parseLinks(text));
    const k = rel.toLowerCase();
    if (linkSigs.get(k) !== sig) {
      linkSigs.set(k, sig);
      linksGeneration++;
    }
  };
  const indexInfo = (ms: number, extra: Record<string, number> = {}) => {
    let occurrences = 0;
    for (const list of fragsByFile.values()) occurrences += list.length;
    return { fromCache: false, files: index.size, tags: counts().length, occurrences, ...index.stats(), searchReady: index.searchReady, searchDocs: index.searchReady ? index.size : 0, ms, ...extra, linksGeneration, vault: indexRoot };
  };

  function counts(): TagCount[] {
    if (tagCounts) return tagCounts;
    const m = new Map<string, TagCount>();
    for (const list of fragsByFile.values()) {
      for (const f of list) {
        const parts = f.tag.split("/");
        for (let i = 1; i <= parts.length; i++) {
          const full = parts.slice(0, i).join("/");
          const k = full.toLowerCase();
          const c = m.get(k);
          if (c) c.count++;
          else m.set(k, { tag: full, count: 1 });
        }
      }
    }
    tagCounts = [...m.values()];
    return tagCounts;
  }

  async function runTagBuild(root: string) {
    const run = ++tagRun;
    const started = performance.now();
    fragsByFile.clear();
    tagCounts = null;
    control.tagsReady = false;
    control.searchReady = false;
    const tree = buildTree(root);
    const notes = tree.rels.filter((r) => /\.md$/i.test(r));
    indexRoot = root;
    index.reset(root, new FileIndex(tree.entries));
    linkSigs.clear();
    linksGeneration++;
    let done = 0;
    let occurrences = 0;
    while (done < notes.length) {
      const sliceEnd = performance.now() + 6;
      while (done < notes.length && performance.now() < sliceEnd) {
        const abs = joinAbs(root, notes[done]);
        const f = load(abs);
        if (f) {
          const fr = scanTags(abs, f.text);
          if (fr.length) fragsByFile.set(key(abs), fr);
          occurrences += fr.length;
          index.add(notes[done], f.text, fr.map((x) => x.tag));
          noteLinks(notes[done], f.text);
          // Generated notes are not kept in memory once scanned (10k notes would cost ~20 MB).
          if (!fixed[notes[done]] && samePath(root, ROOT)) files.delete(key(abs));
        }
        done++;
      }
      tagCounts = null;
      emit("tags-progress", { done, total: notes.length, vault: root });
      await new Promise((r) => setTimeout(r, 4));
      if (run !== tagRun) return;
    }
    control.tagsReady = true;
    const ms = Math.round(performance.now() - started);
    emit("tags-ready", { files: notes.length, tags: counts().length, occurrences, ms, fromCache: false, vault: root });
    emit("index-ready", indexInfo(ms));
    // The full-text index becomes available after tags and links.
    setTimeout(() => {
      if (run !== tagRun) return;
      index.searchReady = true;
      control.searchReady = true;
      emit("index-ready", indexInfo(Math.round(performance.now() - started)));
    }, searchDelay * delayFactor);
  }

  const control: MockControl = {
    root: ROOT,
    outside: OUTSIDE,
    bigPath: joinAbs(ROOT, BIG_REL),
    timings: [],
    saves: [],
    sessions: [],
    calls: [],
    genMs: {},
    tagsReady: false,
    searchReady: false,
    failSaves: [],
    saveDelayMs: 0,
    readDelayMs: 0,
    closeAcks: 0,
    quits: 0,
    closedVaults: [],
    savesAtQuit: null,
    touch(path, text) {
      const back = deleted.delete(key(path));
      const f = load(path);
      if (!f) return;
      if (text != null) f.text = text;
      if (!back) f.mtimeMs = Math.max(Date.now(), f.mtimeMs + 1);
      emit("file-changed", { path, mtimeMs: f.mtimeMs });
    },
    remove(path) {
      const f = load(path);
      deleted.add(key(path));
      emit("file-changed", { path, mtimeMs: f?.mtimeMs ?? 0, deleted: true });
    },
    setFile(path, text, size) {
      deleted.delete(key(path));
      files.set(key(path), { text, mtimeMs: Math.max(Date.now(), baseMtime), size });
    },
    textOf(path) {
      return load(path)?.text ?? null;
    },
    allowOpened(path) {
      opened.add(key(path));
    },
    emit,
  };
  if (typeof window !== "undefined") window.rocksyMock = control;

  const defaultSession: Session = {
    activeVault: ROOT,
    tabs: [joinAbs(ROOT, BIG_REL), joinAbs(ROOT, "00__DEMO/Live Preview.md")],
    activeTab: joinAbs(ROOT, BIG_REL),
  };
  const open = params.get("open");
  if (open) {
    const abs = /^[a-zA-Z]:/.test(open) ? open : joinAbs(ROOT, open);
    defaultSession.tabs = [abs];
    defaultSession.activeTab = abs;
  }
  // Tabs of the restored session that had been opened explicitly stay so (kept by the backend across restarts).
  if (start !== "fresh" && start !== "novault") for (const t of defaultSession.tabs) if (!vaultOf(t)) opened.add(key(t));

  const fileContent = (path: string, f: MockFile): FileContent => ({ path, text: f.text, mtimeMs: f.mtimeMs, size: f.size ?? f.text.length, readOnly: !writable(path) });
  /** Same checks and errors as `read_file` (messages in the core's wording). */
  function readChecked(path: string): FileContent {
    if (isNetworkPath(path) && !canRead(path)) return fail("invalid", `network path refused: ${path}`, "network_path");
    if (!canRead(path)) return fail("invalid", `path not allowed (outside the registered vaults and the files opened by the user): ${path}`, "not_allowed");
    const f = load(path);
    if (!f) return fail("not_found", `${path} does not exist`);
    const size = f.size ?? f.text.length;
    if (size > MAX_READ_BYTES) return fail("invalid", `${path} is too large to open (${(size / 1048576).toFixed(1)} MiB, limit 50.0 MiB)`, "too_large");
    return fileContent(path, f);
  }

  const backend: Backend = {
    kind: "mock",
    async startupInfo() {
      control.calls.push("startup_info");
      await sleep(2);
      const session = start === "fresh" || start === "novault" ? null : defaultSession;
      const argFiles = start === "arg" ? [OUTSIDE] : [];
      // The note the UI will open first, read while the WebView starts (null when it cannot be read).
      const firstPath = argFiles[0] ?? session?.activeTab ?? null;
      let firstFile: FileContent | null = null;
      if (firstPath && params.get("firstfile") !== "0") {
        try {
          firstFile = readChecked(firstPath);
        } catch {
          firstFile = null;
        }
      }
      return {
        argFiles,
        vaults,
        session,
        dataDir: "C:\\Users\\demo\\AppData\\Roaming\\Rocksy",
        version: "0.2.1-mock",
        processStartMs: t0,
        firstFile,
      };
    },
    async readFile(path): Promise<FileContent> {
      control.calls.push("read_file");
      const fc = readChecked(path);
      await sleep(3 + fc.text.length / 60_000);
      if (control.readDelayMs) await new Promise((r) => setTimeout(r, control.readDelayMs));
      // The content is read at the end of the delay, as a slow disk would return it.
      return readChecked(path);
    },
    async saveFile(path, text, expectedMtimeMs) {
      control.calls.push("save_file");
      savesStarted++;
      await sleep(12);
      if (control.saveDelayMs) await new Promise((r) => setTimeout(r, control.saveDelayMs));
      if (isNetworkPath(path) && !canWrite(path)) return fail("invalid", `network path refused: ${path}`, "network_path");
      if (!canWrite(path)) return fail("invalid", `writing not allowed (outside the registered vaults and the files opened by the user): ${path}`, "not_allowed");
      if (/[\\/]\.obsidian[\\/]/i.test(path)) return fail("invalid", `writing inside .obsidian is not allowed: ${path}`, "read_only");
      if (!isMarkdownPath(path)) return fail("invalid", `only .md and .markdown files can be saved, other files are read-only: ${path}`, "read_only");
      const injected = control.failSaves.shift();
      if (injected) return fail(injected.kind, injected.message, injected.code);
      let f = load(path);
      if (!f && expectedMtimeMs != null) return fail("conflict", `${path} no longer exists on disk`, "gone");
      if (f && expectedMtimeMs != null && f.mtimeMs !== expectedMtimeMs) return fail("conflict", `${path} changed on disk (expected mtime ${expectedMtimeMs}, found ${f.mtimeMs})`);
      if (!f) {
        f = { text, mtimeMs: 0 };
        deleted.delete(key(path));
        files.set(key(path), f);
      }
      f.text = text;
      f.size = undefined;
      f.mtimeMs = Math.max(Date.now(), f.mtimeMs + 1);
      control.saves.push({ path, size: text.length, expectedMtimeMs });
      if (fragsByFile.size && vaultOf(path)) {
        const fr = scanTags(path, text);
        const rel = indexRoot ? toRel(indexRoot, path) : null;
        if (rel != null) {
          index.add(rel, text, fr.map((x) => x.tag));
          const before = linksGeneration;
          noteLinks(rel, text);
          // As the app: an `index-ready` follows a save only when it changed the links of the vault.
          if (linksGeneration !== before) setTimeout(() => emit("index-ready", indexInfo(0)), 30);
        }
        fragsByFile.set(key(path), fr);
        tagCounts = null;
      }
      return { mtimeMs: f.mtimeMs, size: text.length };
    },
    async listVaults() {
      return vaults;
    },
    async addVault(path) {
      control.calls.push("add_vault");
      await sleep(5);
      // As the app: only a folder returned by the folder dialog during this session (or an already registered vault).
      if (!pickedFolders.has(key(path)) && !vaults.some((v) => samePath(v.path, path)))
        return fail("invalid", `not a folder chosen in the folder dialog during this session: ${path}`, "not_allowed");
      if (!vaults.some((v) => samePath(v.path, path))) vaults = [...vaults, { path, name: basename(path) }];
      return vaults;
    },
    async removeVault(path) {
      control.calls.push("remove_vault");
      vaults = vaults.filter((v) => !samePath(v.path, path));
      return vaults;
    },
    async closeVault(path) {
      control.calls.push("close_vault");
      control.closedVaults.push(path);
      if (samePath(path, indexRoot)) {
        // Stops the index pass of that vault: no more events about it.
        tagRun++;
        indexRoot = "";
      }
    },
    async closeAck() {
      control.calls.push("close_ack");
      control.closeAcks++;
    },
    async quit() {
      control.calls.push("quit");
      control.quits++;
      control.savesAtQuit = [control.saves.length, savesStarted];
    },
    async openVault(path): Promise<Tree> {
      control.calls.push("open_vault");
      if (!isVault(path)) return fail("invalid", `not a registered vault: ${path}`);
      const tree = buildTree(path);
      await sleep(25 + tree.entries.length / 1500);
      setTimeout(() => {
        // Background reconciliation: one file appeared since the snapshot.
        const extra: TreeEntry = { path: "00__DEMO/Ajoutée par la réconciliation.md", isDir: false };
        const entries = samePath(path, ROOT) ? [...tree.entries, extra] : tree.entries;
        if (samePath(path, ROOT) && !tree.rels.includes(extra.path)) {
          tree.rels.push(extra.path);
          tree.entries = entries;
          tree.byRel.set(extra.path.toLowerCase(), extra);
        }
        emit("tree-updated", { root: path, fromSnapshot: false, entries });
        if (samePath(path, indexRoot)) {
          index.setFiles(new FileIndex(entries));
          linksGeneration++;
        }
      }, 700 * delayFactor);
      return { root: path, fromSnapshot: true, entries: tree.entries };
    },
    async buildTags(vaultPath) {
      control.calls.push("build_tags");
      if (!isVault(vaultPath)) return fail("invalid", `not a registered vault: ${vaultPath}`);
      void runTagBuild(vaultPath);
    },
    async tagsList(query) {
      control.calls.push("tags_list");
      // Same rule as the core: case-insensitive substring; tags starting with the query, then tags with a level
      // starting with it, then the other matches; inside each group by decreasing count, then alphabetically.
      const q = query.trim().replace(/^#+/, "").toLowerCase();
      const ranked: { rank: number; key: string; t: TagCount }[] = [];
      for (const t of counts()) {
        const k = t.tag.toLowerCase();
        let rank = 0;
        if (q && !k.startsWith(q)) {
          const first = k.indexOf(q);
          if (first < 0) continue;
          rank = 2;
          for (let p = first; p >= 0; p = k.indexOf(q, p + q.length)) if (k[p - 1] === "/") rank = 1;
        }
        ranked.push({ rank, key: k, t });
      }
      ranked.sort((a, b) => a.rank - b.rank || b.t.count - a.t.count || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
      return ranked.map((r) => ({ tag: r.t.tag, count: r.t.count }));
    },
    async tagFragments(tag, limit = 200) {
      limit = Math.min(limit, MAX_RESULTS);
      const t = tag.toLowerCase();
      const out: Fragment[] = [];
      for (const list of fragsByFile.values()) {
        for (const f of list) {
          const ft = f.tag.toLowerCase();
          if (ft === t || ft.startsWith(t + "/")) {
            out.push(f);
            if (out.length >= limit) return out;
          }
        }
      }
      return out;
    },
    async search(query, limit) {
      control.calls.push("search");
      await sleep(2);
      return index.search(query, limit == null ? undefined : Math.min(limit, MAX_RESULTS));
    },
    async backlinks(path) {
      control.calls.push("backlinks");
      await sleep(2);
      const rel = indexRoot ? toRel(indexRoot, path) : null;
      return rel == null ? [] : index.backlinks(rel);
    },
    async outgoingLinks(path) {
      control.calls.push("outgoing_links");
      await sleep(2);
      const rel = indexRoot ? toRel(indexRoot, path) : null;
      return rel == null ? [] : index.outgoing(rel);
    },
    async unresolvedLinks(limit) {
      control.calls.push("unresolved_links");
      await sleep(2);
      return index.unresolved(limit == null ? undefined : Math.min(limit, MAX_RESULTS));
    },
    async fileTags(path, text) {
      control.calls.push("file_tags");
      return scanTags(path, text);
    },
    async setSession(session) {
      // Tabs the page may not read and an unregistered vault are dropped silently.
      const tabs = session.tabs.filter((t) => canRead(t));
      control.sessions.push({
        activeVault: session.activeVault && isVault(session.activeVault) ? session.activeVault : null,
        tabs,
        activeTab: session.activeTab && tabs.includes(session.activeTab) ? session.activeTab : null,
      });
    },
    async pickFolder() {
      control.calls.push("pick_folder");
      await sleep(20);
      pickedFolders.add(key(ROOT2));
      return ROOT2;
    },
    async pickFile() {
      control.calls.push("pick_file");
      await sleep(20);
      opened.add(key(OUTSIDE));
      return OUTSIDE;
    },
    async logTiming(name, ms) {
      control.timings.push({ name, ms });
    },
    async openExternal(url) {
      control.calls.push("open_external");
      if (!/^(https?:|mailto:)/i.test(url)) return fail("invalid", `Lien refusé : ${url}`);
      // Browser mode: let the browser open it.
      window.open(url, "_blank", "noopener");
    },
    on(event, cb) {
      let set = listeners.get(event);
      if (!set) listeners.set(event, (set = new Set()));
      set.add(cb as (p: unknown) => void);
      return () => set!.delete(cb as (p: unknown) => void);
    },
    assetUrl(absPath) {
      return images.get(key(absPath)) ?? (/croquis|capture|schema|photo|export/i.test(absPath) ? svgImage(basename(absPath), 520, 260, hash(absPath) % 360) : "data:image/png;base64,AAAA");
    },
    onFileDrop(cb) {
      const over = (e: DragEvent) => e.preventDefault();
      const drop = (e: DragEvent) => {
        e.preventDefault();
        const list = [...(e.dataTransfer?.files ?? [])].filter((f) => /\.(md|markdown|txt)$/i.test(f.name));
        Promise.all(
          list.map(async (f) => {
            const abs = "C:\\Depot\\" + f.name;
            files.set(key(abs), { text: await f.text(), mtimeMs: f.lastModified || Date.now() });
            opened.add(key(abs));
            return abs;
          }),
        ).then((paths) => paths.length && cb(paths));
      };
      window.addEventListener("dragover", over);
      window.addEventListener("drop", drop);
      return () => {
        window.removeEventListener("dragover", over);
        window.removeEventListener("drop", drop);
      };
    },
  };
  return { backend, control };
}
