// In-memory backend used in an ordinary browser and by the headless tests.
// It simulates a vault (tree, a ~320 KB note, tags, images) with realistic delays.
// URL parameters: tree=<entries> start=session|arg|fresh|novault open=<vault-relative path> delay=<factor> bigkb=<KB>
import type { Backend } from "../backend";
import type { ApiError, BackendEvents, FileContent, Fragment, Session, TagCount, Tree, TreeEntry, VaultRef } from "../types";
import { basename, joinAbs, samePath, toRel } from "../paths";
import { DEMO_BLOCKS, DEMO_CALLOUTS, DEMO_CANVAS, DEMO_LIVE_PREVIEW, DEMO_OUTSIDE, bigNote, hash, smallNote, svgImage, treeFiles } from "./gen";
import { scanTags } from "./tagscan";

const ROOT = "C:\\Vaults\\Demo";
const ROOT2 = "D:\\Vaults\\Second";
const OUTSIDE = "C:\\Temp\\Notes\\hors-vault.md";
const BIG_REL = "01__WORK/__DAILY NOTES DEMO.md";

interface MockFile {
  text: string;
  mtimeMs: number;
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
  emit<K extends keyof BackendEvents>(event: K, payload: BackendEvents[K]): void;
  tagsReady: boolean;
}

declare global {
  interface Window {
    rocksyMock?: MockControl;
  }
}

export function createMockBackend(params: URLSearchParams): Backend {
  const treeCount = Number(params.get("tree") ?? 1200);
  const start = params.get("start") ?? "session";
  const delayFactor = Number(params.get("delay") ?? 1);
  const bigBytes = Number(params.get("bigkb") ?? 320) * 1000;
  const t0 = Date.now() - Math.round(performance.now());

  const sleep = (ms: number) => new Promise<void>((r) => (ms * delayFactor > 0 ? setTimeout(r, ms * delayFactor) : r()));
  const fail = (kind: ApiError["kind"], message: string): never => {
    throw { kind, message } satisfies ApiError;
  };

  const files = new Map<string, MockFile>();
  const key = (abs: string) => abs.replace(/\//g, "\\").toLowerCase();
  const images = new Map<string, string>();
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  const emit = <K extends keyof BackendEvents>(event: K, payload: BackendEvents[K]) => {
    listeners.get(event)?.forEach((cb) => cb(payload));
  };
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
    for (const d of dirs) entries.push({ path: d, isDir: true, size: 0, mtimeMs: baseMtime });
    for (const r of rels) {
      const h = hash(r);
      entries.push({ path: r, isDir: false, size: r === BIG_REL ? bigBytes : 800 + (h % 2600), mtimeMs: baseMtime - (h % 9_000_000) * 100 });
    }
    // Contract order: directories first, case-insensitive alphabetical.
    entries.sort((a, b) => (a.isDir === b.isDir ? a.path.toLowerCase().localeCompare(b.path.toLowerCase()) : a.isDir ? -1 : 1));
    const byRel = new Map<string, TreeEntry>();
    for (const e of entries) if (!e.isDir) byRel.set(e.path.toLowerCase(), e);
    const built = { rels, entries, byRel };
    trees.set(key(root), built);
    control.genMs["tree:" + basename(root)] = Math.round((performance.now() - t) * 10) / 10;
    return built;
  }

  function vaultOf(abs: string): { root: string; rel: string } | null {
    for (const root of [ROOT, ROOT2, ...vaults.map((v) => v.path)]) {
      const rel = toRel(root, abs);
      if (rel != null) return { root, rel };
    }
    return null;
  }

  function load(abs: string): MockFile | null {
    const k = key(abs);
    const hit = files.get(k);
    if (hit) return hit;
    const v = vaultOf(abs);
    if (!v) return null;
    const tree = buildTree(v.root);
    const entry = tree.byRel.get(v.rel.toLowerCase());
    if (!entry) return null;
    const text = samePath(v.root, ROOT) && fixed[entry.path] ? fixed[entry.path]() : /\.(md|canvas)$/i.test(entry.path) ? (entry.path.endsWith(".canvas") ? DEMO_CANVAS : smallNote(entry.path)) : "";
    const f = { text, mtimeMs: entry.mtimeMs };
    files.set(k, f);
    return f;
  }

  // ---- tags (in-memory library)
  const fragsByFile = new Map<string, Fragment[]>();
  let tagCounts: TagCount[] | null = null;
  let tagRun = 0;

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
    tagCounts = [...m.values()].sort((a, b) => a.tag.toLowerCase().localeCompare(b.tag.toLowerCase()));
    return tagCounts;
  }

  async function runTagBuild(root: string) {
    const run = ++tagRun;
    const started = performance.now();
    fragsByFile.clear();
    tagCounts = null;
    control.tagsReady = false;
    const tree = buildTree(root);
    const notes = tree.rels.filter((r) => /\.md$/i.test(r));
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
          // Generated notes are not kept in memory once scanned (10k notes would cost ~20 MB).
          if (!fixed[notes[done]] && samePath(root, ROOT)) files.delete(key(abs));
        }
        done++;
      }
      tagCounts = null;
      emit("tags-progress", { done, total: notes.length });
      await new Promise((r) => setTimeout(r, 4));
      if (run !== tagRun) return;
    }
    control.tagsReady = true;
    emit("tags-ready", { files: notes.length, tags: counts().length, occurrences, ms: Math.round(performance.now() - started) });
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
    touch(path, text) {
      const f = load(path);
      if (!f) return;
      if (text != null) f.text = text;
      f.mtimeMs = Math.max(Date.now(), f.mtimeMs + 1);
      emit("file-changed", { path, mtimeMs: f.mtimeMs });
    },
    emit,
  };
  window.rocksyMock = control;

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

  const backend: Backend = {
    kind: "mock",
    async startupInfo() {
      control.calls.push("startup_info");
      await sleep(2);
      return {
        argFiles: start === "arg" ? [OUTSIDE] : [],
        vaults,
        session: start === "fresh" || start === "novault" ? null : defaultSession,
        dataDir: "C:\\Users\\demo\\AppData\\Roaming\\Rocksy",
        version: "0.1.0-mock",
        processStartMs: t0,
      };
    },
    async readFile(path): Promise<FileContent> {
      control.calls.push("read_file");
      const f = load(path);
      if (!f) return fail("not_found", `Fichier introuvable : ${path}`);
      await sleep(3 + f.text.length / 60_000);
      return { path, text: f.text, mtimeMs: f.mtimeMs, size: f.text.length };
    },
    async saveFile(path, text, expectedMtimeMs) {
      control.calls.push("save_file");
      await sleep(12);
      if (/\.canvas$/i.test(path) || /[\\/]\.obsidian[\\/]/i.test(path)) return fail("invalid", "Écriture refusée sur ce type de fichier");
      let f = load(path);
      if (f && expectedMtimeMs != null && f.mtimeMs !== expectedMtimeMs) return fail("conflict", "Le fichier a changé sur le disque");
      if (!f) {
        f = { text, mtimeMs: 0 };
        files.set(key(path), f);
      }
      f.text = text;
      f.mtimeMs = Math.max(Date.now(), f.mtimeMs + 1);
      control.saves.push({ path, size: text.length, expectedMtimeMs });
      if (fragsByFile.size && vaultOf(path)) {
        fragsByFile.set(key(path), scanTags(path, text));
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
      if (!vaults.some((v) => samePath(v.path, path))) vaults = [...vaults, { path, name: basename(path) }];
      return vaults;
    },
    async removeVault(path) {
      control.calls.push("remove_vault");
      vaults = vaults.filter((v) => !samePath(v.path, path));
      return vaults;
    },
    async openVault(path): Promise<Tree> {
      control.calls.push("open_vault");
      const tree = buildTree(path);
      await sleep(25 + tree.entries.length / 1500);
      setTimeout(() => {
        // Background reconciliation: one file appeared since the snapshot.
        const extra: TreeEntry = { path: "00__DEMO/Ajoutée par la réconciliation.md", isDir: false, size: 420, mtimeMs: Date.now() };
        const entries = samePath(path, ROOT) ? [...tree.entries, extra] : tree.entries;
        if (samePath(path, ROOT) && !tree.rels.includes(extra.path)) {
          tree.rels.push(extra.path);
          tree.entries = entries;
          tree.byRel.set(extra.path.toLowerCase(), extra);
        }
        emit("tree-updated", { root: path, fromSnapshot: false, entries });
      }, 700 * delayFactor);
      return { root: path, fromSnapshot: true, entries: tree.entries };
    },
    async buildTags(vaultPath) {
      control.calls.push("build_tags");
      void runTagBuild(vaultPath);
    },
    async tagsList(query) {
      const q = query.trim().replace(/^#/, "").toLowerCase();
      const all = counts();
      return q ? all.filter((t) => t.tag.toLowerCase().includes(q)) : all;
    },
    async tagFragments(tag, limit = 200) {
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
    async fileTags(path, text) {
      control.calls.push("file_tags");
      return scanTags(path, text);
    },
    async setSession(session) {
      control.sessions.push(session);
    },
    async pickFolder() {
      control.calls.push("pick_folder");
      await sleep(20);
      return ROOT2;
    },
    async pickFile() {
      control.calls.push("pick_file");
      await sleep(20);
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
  return backend;
}
