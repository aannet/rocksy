// Side panel: virtualised file tree and tags panel. Loaded after the editor is usable.
import type { App } from "./app";
import type { Fragment, TagCount, Tree } from "./types";
import { basename, extname, joinAbs, toRel } from "./paths";
import { buildTagTree, countTags, filterTagLibrary, filterTags, tagMatches, type TagNode } from "./tagtree";
import { iconSvg } from "./icons";
import { SearchPanel } from "./searchpanel";
import { LinksPanel } from "./linkspanel";

export type SidebarTab = "files" | "tags" | "search" | "links";

const ROW_H = 26;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : "&quot;"));

interface TNode {
  path: string;
  name: string;
  key: string;
  isDir: boolean;
  depth: number;
  children: TNode[] | null;
}

/** Builds the folder hierarchy from the flat entries; each folder lists sub-folders first, case-insensitive. */
export function buildFileTree(tree: Tree): TNode[] {
  const roots: TNode[] = [];
  const dirs = new Map<string, TNode>();
  const ensureDir = (path: string): TNode => {
    let n = dirs.get(path);
    if (n) return n;
    const cut = path.lastIndexOf("/");
    const name = cut < 0 ? path : path.slice(cut + 1);
    n = { path, name, key: name.toLowerCase(), isDir: true, depth: 0, children: [] };
    dirs.set(path, n);
    if (cut < 0) roots.push(n);
    else {
      const parent = ensureDir(path.slice(0, cut));
      n.depth = parent.depth + 1;
      parent.children!.push(n);
    }
    return n;
  };
  for (const e of tree.entries) {
    if (e.isDir) {
      ensureDir(e.path);
      continue;
    }
    const cut = e.path.lastIndexOf("/");
    const name = cut < 0 ? e.path : e.path.slice(cut + 1);
    const node: TNode = { path: e.path, name, key: name.toLowerCase(), isDir: false, depth: 0, children: null };
    if (cut < 0) roots.push(node);
    else {
      const parent = ensureDir(e.path.slice(0, cut));
      node.depth = parent.depth + 1;
      parent.children!.push(node);
    }
  }
  const cmp = (a: TNode, b: TNode) => (a.isDir !== b.isDir ? (a.isDir ? -1 : 1) : a.key < b.key ? -1 : a.key > b.key ? 1 : 0);
  const sortAll = (list: TNode[]) => {
    list.sort(cmp);
    for (const n of list) if (n.children) sortAll(n.children);
  };
  sortAll(roots);
  return roots;
}

export class Sidebar {
  private head = document.createElement("div");
  private vaultBtn = document.createElement("button");
  private tabBtns: Record<SidebarTab, HTMLButtonElement>;
  readonly search: SearchPanel;
  readonly links: LinksPanel;
  private filesBody = document.createElement("div");
  private tagsBody = document.createElement("div");
  // tree
  private treeEl = document.createElement("div");
  private sizer = document.createElement("div");
  private treeFoot = document.createElement("div");
  private roots: TNode[] = [];
  private rows: TNode[] = [];
  private expanded = new Set<string>();
  private treeRoot: string | null = null;
  private fileCount = 0;
  private activeRel: string | null = null;
  private raf = 0;
  private renderedFrom = -1;
  private renderedTo = -1;
  // tags
  private tagInput = document.createElement("input");
  private modeEl = document.createElement("div");
  private progressEl = document.createElement("div");
  private progressLabel = document.createElement("div");
  private tagList = document.createElement("div");
  private fragHead = document.createElement("div");
  private fragList = document.createElement("div");
  private tagMode: "vault" | "file" = "vault";
  private tagExpanded = new Set<string>();
  private tagNodes: TagNode[] = [];
  private selectedTag: string | null = null;
  private fragments: Fragment[] = [];
  private fileFragments: Fragment[] = [];
  private listReq = 0;
  private fragReq = 0;
  private progressRefresh = 0;
  tab: SidebarTab = "files";
  private searchRoot: string | null = null;

  constructor(
    private el: HTMLElement,
    private app: App,
  ) {
    this.head.className = "sb-head";
    this.vaultBtn.className = "sb-vault";
    this.vaultBtn.title = "Changer de vault";
    this.vaultBtn.addEventListener("click", () => void app.chooseVault());
    const spacer = document.createElement("span");
    spacer.className = "sb-spacer";
    const tabs = document.createElement("div");
    tabs.className = "sb-tabs";
    const mk = (tab: SidebarTab, icon: string, title: string) => {
      const b = document.createElement("button");
      b.innerHTML = iconSvg(icon, 16);
      b.title = title;
      b.dataset.tab = tab;
      b.addEventListener("click", () => this.setTab(tab));
      tabs.appendChild(b);
      return b;
    };
    this.tabBtns = {
      files: mk("files", "files", "Fichiers"),
      search: mk("search", "search", "Recherche (Ctrl+Maj+F)"),
      tags: mk("tags", "tag", "Tags"),
      links: mk("links", "link", "Liens de la note"),
    };
    this.search = new SearchPanel(app);
    this.links = new LinksPanel(app);
    this.head.append(this.vaultBtn, spacer, tabs);

    this.filesBody.className = "sb-body";
    this.treeEl.className = "tree";
    this.treeEl.tabIndex = -1;
    this.sizer.className = "tree-sizer";
    this.treeEl.appendChild(this.sizer);
    this.treeFoot.className = "tree-foot";
    this.filesBody.append(this.treeEl, this.treeFoot);
    this.treeEl.addEventListener("scroll", () => this.queueRender(), { passive: true });
    this.treeEl.addEventListener("mousedown", (e) => {
      if (e.button === 1) e.preventDefault();
    });
    this.treeEl.addEventListener("click", (e) => this.onTreeClick(e));
    new ResizeObserver(() => this.queueRender(true)).observe(this.treeEl);

    this.tagsBody.className = "sb-body";
    this.tagsBody.hidden = true;
    const search = document.createElement("label");
    search.className = "tags-search";
    search.innerHTML = iconSvg("search", 14);
    this.tagInput.type = "text";
    this.tagInput.placeholder = "Filtrer les tags…";
    this.tagInput.spellcheck = false;
    search.appendChild(this.tagInput);
    this.modeEl.className = "tags-mode";
    this.modeEl.innerHTML = `<button data-mode="vault">Vault</button><button data-mode="file">Ce fichier</button>`;
    this.progressEl.className = "tags-progress";
    this.progressEl.innerHTML = "<i></i>";
    this.progressEl.hidden = true;
    this.progressLabel.className = "tags-progress-label";
    this.progressLabel.hidden = true;
    this.tagList.className = "tags-list";
    this.fragHead.className = "frag-head";
    this.fragList.className = "frag-list";
    this.tagsBody.append(search, this.modeEl, this.progressEl, this.progressLabel, this.tagList, this.fragHead, this.fragList);
    this.tagInput.addEventListener("input", () => void this.refreshTags());
    this.tagInput.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        this.tagInput.value = "";
        void this.refreshTags();
      }
    });
    this.modeEl.addEventListener("click", (e) => {
      const m = (e.target as HTMLElement).dataset.mode as "vault" | "file" | undefined;
      if (m) {
        this.tagMode = m;
        this.selectedTag = null;
        void this.refreshTags(true);
      }
    });
    this.tagList.addEventListener("click", (e) => this.onTagClick(e));
    this.fragList.addEventListener("click", (e) => {
      const el = (e.target as HTMLElement).closest(".frag") as HTMLElement | null;
      if (!el) return;
      const f = this.fragments[Number(el.dataset.i)];
      if (!f) return;
      this.fragList.querySelector(".is-active")?.classList.remove("is-active");
      el.classList.add("is-active");
      void this.app.openFragment(f);
    });

    const resizer = document.createElement("div");
    resizer.className = "sb-resizer";
    resizer.addEventListener("mousedown", (e) => {
      e.preventDefault();
      const startX = e.clientX;
      const startW = this.el.offsetWidth;
      const move = (ev: MouseEvent) => {
        const w = Math.min(600, Math.max(180, startW + ev.clientX - startX));
        document.documentElement.style.setProperty("--sidebar-w", w + "px");
      };
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        try {
          localStorage.setItem("rocksy.sidebarWidth", String(this.el.offsetWidth));
        } catch {
          /* ignore */
        }
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
    });
    this.el.append(this.head, this.filesBody, this.tagsBody, this.search.body, this.links.body, resizer);
    this.setTree(app.tree);
    if (app.tagsProgress) this.onTagsProgress(app.tagsProgress);
  }

  setTab(tab: SidebarTab, focus = true): void {
    this.tab = tab;
    this.app.sidebarTab = tab;
    this.filesBody.hidden = tab !== "files";
    this.tagsBody.hidden = tab !== "tags";
    this.search.body.hidden = tab !== "search";
    this.links.body.hidden = tab !== "links";
    for (const k of ["files", "tags", "search", "links"] as const) {
      this.tabBtns[k].classList.toggle("is-on", tab === k);
      document.getElementById("rb-" + k)?.classList.toggle("is-on", tab === k);
    }
    if (tab === "files") this.queueRender(true);
    else if (tab === "tags") void this.refreshTags(true);
    else if (tab === "search") this.search.show(focus);
    else void this.links.refresh();
  }

  /** `index-ready`: search may have become available, links may have changed (the panel checks `linksGeneration`). */
  onIndexReady(): void {
    this.search.onIndex();
    if (this.tab === "links") void this.links.refresh();
  }

  // ------------------------------------------------------------ file tree

  private foldKey(): string {
    return "rocksy.tree." + (this.treeRoot ?? "");
  }

  /** `samePaths`: the tree lists the same files as the one shown (only its freshness changed): nothing is rebuilt. */
  setTree(tree: Tree | null, samePaths = false): void {
    if (tree && samePaths && this.treeRoot === tree.root) {
      this.renderFoot(tree);
      return;
    }
    const root = this.app.vaultRoot;
    if (root !== this.searchRoot) {
      this.searchRoot = root;
      this.search.reset();
      if (this.tab === "links") void this.links.refresh();
    }
    this.vaultBtn.innerHTML = `${iconSvg("vault", 15)}<span>${esc(root ? (this.app.vaults.find((v) => v.path === root)?.name ?? basename(root)) : "Aucun vault")}</span>`;
    if (!tree) {
      this.roots = [];
      this.rows = [];
      this.treeRoot = null;
      this.sizer.style.height = "0px";
      this.sizer.innerHTML = "";
      this.treeFoot.textContent = "";
      this.renderedFrom = this.renderedTo = -1;
      return;
    }
    const t0 = performance.now();
    if (this.treeRoot !== tree.root) {
      this.treeRoot = tree.root;
      this.expanded = new Set();
      try {
        const saved = localStorage.getItem(this.foldKey());
        if (saved) this.expanded = new Set(JSON.parse(saved) as string[]);
      } catch {
        /* ignore */
      }
      this.treeEl.scrollTop = 0;
      this.activeRel = null;
    }
    this.roots = buildFileTree(tree);
    this.fileCount = 0;
    for (const e of tree.entries) if (!e.isDir) this.fileCount++;
    this.renderFoot(tree);
    this.syncActive(false);
    this.flatten();
    this.queueRender(true);
    this.app.metrics.treeBuildMs = Math.round((performance.now() - t0) * 10) / 10;
    this.app.metrics.treeEntries = tree.entries.length;
  }

  private renderFoot(tree: Tree): void {
    this.treeFoot.innerHTML = `<span>${this.fileCount.toLocaleString("fr-FR")} fichiers</span><span>${tree.fromSnapshot ? "instantané, vérification…" : "à jour"}</span>`;
  }

  private flatten(): void {
    const rows: TNode[] = [];
    const walk = (list: TNode[]) => {
      for (const n of list) {
        rows.push(n);
        if (n.children && this.expanded.has(n.path)) walk(n.children);
      }
    };
    walk(this.roots);
    this.rows = rows;
    this.sizer.style.height = rows.length * ROW_H + 6 + "px";
  }

  private queueRender(force = false): void {
    if (force) this.renderedFrom = this.renderedTo = -1;
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.renderRows();
    });
  }

  /** Renders only the rows in (and slightly around) the viewport. */
  renderRows(): void {
    if (this.filesBody.hidden) return;
    const top = this.treeEl.scrollTop;
    const h = this.treeEl.clientHeight || 600;
    const from = Math.max(0, Math.floor(top / ROW_H) - 8);
    const to = Math.min(this.rows.length, Math.ceil((top + h) / ROW_H) + 8);
    if (from >= this.renderedFrom && to <= this.renderedTo && this.renderedFrom >= 0) return;
    const a = Math.max(0, from - 12);
    const b = Math.min(this.rows.length, to + 12);
    let html = "";
    for (let i = a; i < b; i++) {
      const n = this.rows[i];
      const pad = 6 + n.depth * 16;
      if (n.isDir) {
        html += `<div class="tree-row is-dir${this.expanded.has(n.path) ? " is-open" : ""}" data-i="${i}" style="top:${i * ROW_H}px;padding-left:${pad}px"><span class="chev">${iconSvg("chevron", 14)}</span><span class="tn">${esc(n.name)}</span></div>`;
      } else {
        const ext = extname(n.name);
        const md = ext === "md";
        const label = md ? n.name.slice(0, -3) : n.name;
        const icon = md ? "file" : ext === "canvas" ? "canvas" : /^(png|jpe?g|gif|svg|webp|bmp|avif)$/.test(ext) ? "image" : "file";
        html += `<div class="tree-row${n.path === this.activeRel ? " is-active" : ""}" data-i="${i}" style="top:${i * ROW_H}px;padding-left:${pad + 19}px" title="${esc(n.path)}"><span class="ti">${iconSvg(icon, 14)}</span><span class="tn">${esc(label)}</span>${md ? "" : `<span class="tx">${esc(ext)}</span>`}</div>`;
      }
    }
    this.sizer.innerHTML = html;
    this.renderedFrom = a;
    this.renderedTo = b;
    this.app.metrics.treeRowsInDom = b - a;
  }

  private onTreeClick(e: MouseEvent): void {
    const row = (e.target as HTMLElement).closest(".tree-row") as HTMLElement | null;
    if (!row) return;
    const n = this.rows[Number(row.dataset.i)];
    if (!n) return;
    if (n.isDir) {
      if (this.expanded.has(n.path)) this.expanded.delete(n.path);
      else this.expanded.add(n.path);
      this.saveFolds();
      this.flatten();
      this.queueRender(true);
    } else if (this.treeRoot) {
      void this.app.openPath(joinAbs(this.treeRoot, n.path));
    }
  }

  private saveFolds(): void {
    try {
      localStorage.setItem(this.foldKey(), JSON.stringify([...this.expanded].slice(0, 5000)));
    } catch {
      /* ignore */
    }
  }

  /** Highlights the active file; expands its ancestors and scrolls to it when asked. */
  private syncActive(reveal: boolean): boolean {
    const doc = this.app.activeDoc();
    const rel = doc && this.treeRoot ? toRel(this.treeRoot, doc.path) : null;
    const changed = rel !== this.activeRel;
    this.activeRel = rel;
    if (!rel) return changed;
    let opened = false;
    let i = rel.indexOf("/");
    while (i >= 0) {
      const dir = rel.slice(0, i);
      if (!this.expanded.has(dir)) {
        this.expanded.add(dir);
        opened = true;
      }
      i = rel.indexOf("/", i + 1);
    }
    if (opened) this.saveFolds();
    if (reveal) {
      if (opened) this.flatten();
      const idx = this.rows.findIndex((r) => r.path === rel);
      if (idx >= 0) {
        const y = idx * ROW_H;
        const top = this.treeEl.scrollTop;
        const h = this.treeEl.clientHeight;
        if (y < top || y + ROW_H > top + h) this.treeEl.scrollTop = Math.max(0, y - h / 2);
      }
    }
    return changed || opened;
  }

  onActiveDoc(): void {
    if (this.syncActive(true)) this.queueRender(true);
    if (this.tab === "links") void this.links.refresh();
    if (this.tab === "tags" && (this.tagMode === "file" || !this.app.vaultRoot)) void this.refreshTags(true);
  }

  // ------------------------------------------------------------ tags

  onTagsProgress(p: { done: number; total: number }): void {
    this.progressEl.hidden = false;
    this.progressLabel.hidden = false;
    (this.progressEl.firstChild as HTMLElement).style.width = (p.total ? (100 * p.done) / p.total : 0) + "%";
    this.progressLabel.textContent = `Lecture des notes modifiées : ${p.done.toLocaleString("fr-FR")} / ${p.total.toLocaleString("fr-FR")}`;
    if (this.tab === "tags" && this.tagMode === "vault" && !this.progressRefresh) {
      this.progressRefresh = window.setTimeout(() => {
        this.progressRefresh = 0;
        // The list grows while notes are read: fetch it again (twice a second at most).
        this.app.invalidateTags();
        void this.refreshTags();
      }, 500);
    }
  }

  onTagsReady(): void {
    this.progressEl.hidden = true;
    const info = this.app.tagsInfo;
    this.progressLabel.hidden = !info;
    if (info) {
      const counts = `${info.tags.toLocaleString("fr-FR")} tags · ${info.occurrences.toLocaleString("fr-FR")} occurrences · ${info.files.toLocaleString("fr-FR")} notes`;
      // The persisted index is usable at once; a background pass then rereads only changed notes.
      const state = info.fromCache
        ? "index enregistré, vérification en cours…"
        : info.reread === undefined
          ? `${info.ms} ms`
          : info.reread || info.removed
            ? `à jour : ${info.reread.toLocaleString("fr-FR")} relues, ${(info.removed ?? 0).toLocaleString("fr-FR")} retirées (${info.ms} ms)`
            : `à jour, aucune note modifiée (${info.ms} ms)`;
      this.progressLabel.textContent = `${counts} — ${state}`;
    }
    if (this.tab === "tags") void this.refreshTags(true);
  }

  async selectTag(tag: string): Promise<void> {
    const clean = tag.replace(/^#/, "");
    this.tagInput.value = "";
    const parts = clean.split("/");
    for (let i = 1; i < parts.length; i++) this.tagExpanded.add(parts.slice(0, i).join("/").toLowerCase());
    this.selectedTag = clean;
    await this.refreshTags(true);
    this.tagList.querySelector(".is-active")?.scrollIntoView({ block: "nearest" });
  }

  private effectiveMode(): "vault" | "file" {
    return this.app.vaultRoot ? this.tagMode : "file";
  }

  /** Shows the tag list for the current filter. Vault mode filters the cached list in memory (same result as `tags_list(query)`). */
  async refreshTags(reloadFragments = false): Promise<void> {
    const req = ++this.listReq;
    const t0 = performance.now();
    const query = this.tagInput.value.trim().replace(/^#/, "");
    const mode = this.effectiveMode();
    this.modeEl.style.display = this.app.vaultRoot ? "" : "none";
    for (const b of this.modeEl.children) b.classList.toggle("is-on", (b as HTMLElement).dataset.mode === mode);
    let tags: TagCount[] = [];
    try {
      if (mode === "vault") tags = filterTagLibrary(await this.app.allTags(), query);
      else {
        const doc = this.app.activeDoc();
        const state = doc && doc.kind === "md" ? this.app.stateOf(doc) : null;
        this.fileFragments = doc && state ? await this.app.backend.fileTags(doc.path, state.doc.toString()) : [];
        tags = filterTags(countTags(this.fileFragments), query);
      }
    } catch {
      tags = [];
    }
    if (req !== this.listReq) return;
    this.tagNodes = buildTagTree(tags);
    this.renderTags(query !== "", mode);
    this.app.metrics.tagsListMs = Math.round((performance.now() - t0) * 100) / 100;
    this.app.metrics.tagsListCount = tags.length;
    if (reloadFragments || mode === "file") void this.loadFragments();
  }

  private renderTags(filtering: boolean, mode: "vault" | "file"): void {
    let html = "";
    let shown = 0;
    let hidden = 0;
    const sel = this.selectedTag?.toLowerCase() ?? null;
    const walk = (nodes: TagNode[], depth: number) => {
      for (const n of nodes) {
        if (shown >= 400) {
          hidden++;
          continue;
        }
        const key = n.full.toLowerCase();
        const open = n.children.length > 0 && (filtering || this.tagExpanded.has(key));
        html += `<div class="tag-row${open ? " is-open" : ""}${key === sel ? " is-active" : ""}" data-tag="${esc(n.full)}" style="padding-left:${6 + depth * 15}px"><span class="chev">${n.children.length ? iconSvg("chevron", 14) : ""}</span><span class="tn"><span class="hash">#</span>${esc(n.name)}</span><span class="tc">${n.count.toLocaleString("fr-FR")}</span></div>`;
        shown++;
        if (open) walk(n.children, depth + 1);
      }
    };
    walk(this.tagNodes, 0);
    if (!shown) {
      const indexing = mode === "vault" && this.app.tagsProgress;
      html = `<div class="tags-empty">${indexing ? "Indexation en cours…" : filtering ? "Aucun tag ne correspond" : mode === "file" ? "Aucun tag dans ce fichier" : "Aucun tag dans ce vault"}</div>`;
    } else if (hidden) html += `<div class="tags-empty">… ${hidden} autres : affinez le filtre</div>`;
    this.tagList.innerHTML = html;
  }

  private onTagClick(e: MouseEvent): void {
    const row = (e.target as HTMLElement).closest(".tag-row") as HTMLElement | null;
    if (!row) return;
    const tag = row.dataset.tag!;
    const key = tag.toLowerCase();
    const onChevron = !!(e.target as HTMLElement).closest(".chev");
    const hasChildren = !!row.querySelector(".chev svg");
    if (hasChildren && (onChevron || this.selectedTag?.toLowerCase() === key)) {
      if (this.tagExpanded.has(key)) this.tagExpanded.delete(key);
      else this.tagExpanded.add(key);
    } else if (hasChildren) this.tagExpanded.add(key);
    this.selectedTag = tag;
    this.renderTags(this.tagInput.value.trim() !== "", this.effectiveMode());
    void this.loadFragments();
  }

  private findCount(tag: string): number {
    const key = tag.toLowerCase();
    const walk = (nodes: TagNode[]): number => {
      for (const n of nodes) {
        if (n.full.toLowerCase() === key) return n.count;
        if (key.startsWith(n.full.toLowerCase() + "/")) {
          const c = walk(n.children);
          if (c >= 0) return c;
        }
      }
      return -1;
    };
    return walk(this.tagNodes);
  }

  private async loadFragments(): Promise<void> {
    const tag = this.selectedTag;
    const req = ++this.fragReq;
    if (!tag) {
      this.fragments = [];
      this.fragHead.innerHTML = `<span>Cliquez sur un tag pour voir ses fragments</span>`;
      this.fragList.innerHTML = "";
      return;
    }
    const t0 = performance.now();
    let frags: Fragment[] = [];
    try {
      frags = this.effectiveMode() === "vault" ? await this.app.backend.tagFragments(tag) : this.fileFragments.filter((f) => tagMatches(f.tag, tag));
    } catch {
      frags = [];
    }
    if (req !== this.fragReq) return;
    this.fragments = frags;
    const total = Math.max(this.findCount(tag), frags.length);
    const label = frags.length < total ? `${frags.length.toLocaleString("fr-FR")} premiers sur ${total.toLocaleString("fr-FR")}` : `${frags.length.toLocaleString("fr-FR")} fragment${frags.length > 1 ? "s" : ""}`;
    this.fragHead.innerHTML = `${iconSvg("tag", 13)}<b>#${esc(tag)}</b><span>${label}</span>`;
    const root = this.app.vaultRoot;
    const tagRe = /(^|\s)(#[\p{L}\p{N}_\-/]+)/gu;
    let html = "";
    for (let i = 0; i < frags.length; i++) {
      const f = frags[i];
      const rel = root ? (toRel(root, f.path) ?? f.path) : basename(f.path);
      const text = esc(f.excerpt).replace(tagRe, (_m, pre: string, t: string) => (tagMatches(t.slice(1), tag) ? `${pre}<mark>${t}</mark>` : pre + t));
      html += `<div class="frag" data-i="${i}"><div class="frag-text">${text || "<i>(bloc vide)</i>"}</div><div class="frag-path"><span>${esc(rel.replace(/\.md$/i, ""))}</span><span>${f.source === "frontmatter" ? '<span class="fm">frontmatter</span>' : "l. " + (f.startLine + 1) + (f.endLine > f.startLine ? "–" + (f.endLine + 1) : "")}</span></div></div>`;
    }
    this.fragList.innerHTML = html || `<div class="tags-empty">Aucun fragment</div>`;
    this.fragList.scrollTop = 0;
    this.app.metrics.tagFragmentsMs = Math.round((performance.now() - t0) * 100) / 100;
    this.app.metrics.tagFragmentsCount = frags.length;
  }
}
