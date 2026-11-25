// Application shell: panes, tabs, saving, vault state, status bar, commands.
// Panels that are not needed for the first paint (sidebar, pickers, canvas viewer, editor extras) are
// loaded with dynamic imports after the editor is usable.
import { Annotation, type EditorState, Text, Transaction } from "@codemirror/state";
import { EditorView, type ViewUpdate } from "@codemirror/view";
import type { Backend } from "./backend";
import { toApiError } from "./backend";
import type { FileContent, Fragment, IndexReady, Session, StartupInfo, TagCount, Tree, VaultRef } from "./types";
import { basename, dirname, extname, isAbsolute, isImagePath, isMarkdownPath, joinAbs, samePath, stripExt, toRel } from "./paths";
import { FileIndex, isNetworkSource, linkTextFor, parseWikilink, resolveCanvasFile, resolveImage, resolveLink, sameTreePaths, stripFileScheme } from "./resolve";
import { type CloseIssue, DocSync, openErrorMessage, type SyncDoc } from "./docsync";
import { decodeText } from "./eol";
import { askChoice, dialogOpen } from "./dialog";
import { isForVault } from "./vaultevents";
import { filterTagLibrary, makeTagLibrary, type TagLibrary } from "./tagtree";
import { type FuzzyItem, fuzzySearch, makeItem } from "./fuzzy";
import {
  createEditorState,
  createEditorView,
  type EditorHost,
  findHeadingLine,
  isPreviewOn,
  lockEffect,
  refreshPreview,
  revealLines,
  setExtras,
  setPreview,
  syncEffects,
} from "./editor/editor";
import type { ImageRef, LinkRef } from "./editor/livepreview";
import { iconSvg } from "./icons";
import type { Sidebar, SidebarTab } from "./sidebar";
import { setTermsEffect, termsField } from "./editor/termhl";
import type { PickerItem } from "./picker";
import { EditorHolder } from "./editorholder";

export type DocKind = "md" | "canvas" | "image";

/** Saving state (dirty, conflict, line endings…) is in `SyncDoc` and handled by `DocSync`. */
export interface Doc extends SyncDoc {
  kind: DocKind;
  pane: Pane;
  state: EditorState | null;
  loading: Promise<void> | null;
  scrollTop: number;
  outside: boolean;
  /** Raw text for non-Markdown viewers (canvas). */
  raw: string | null;
}

export interface OpenOptions {
  pane?: Pane;
  activate?: boolean;
  lines?: { start: number; end: number; col?: number };
  heading?: string | null;
}

export interface Command {
  id: string;
  title: string;
  keys?: string;
  run(): void;
}

const externalChange = Annotation.define<boolean>();
/** The backend caps `limit` at 1000 (contract). */
const UNRESOLVED_LIMIT = 1000;

function loadPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem("rocksy." + key) ?? fallback;
  } catch {
    return fallback;
  }
}
function savePref(key: string, value: string): void {
  try {
    localStorage.setItem("rocksy." + key, value);
  } catch {
    /* storage unavailable */
  }
}

class Pane {
  readonly el = document.createElement("section");
  readonly tabbar = document.createElement("div");
  readonly banner = document.createElement("div");
  readonly body = document.createElement("div");
  readonly editorHost = document.createElement("div");
  readonly viewer = document.createElement("div");
  readonly empty = document.createElement("div");
  view: EditorView | null = null;
  /** The document whose live state is in `view` (docs/specs/saving.md, SV-18); not always `active`. */
  readonly held: EditorHolder<Pane, Doc, EditorView> = new EditorHolder<Pane, Doc, EditorView>(this, syncEffects, (d) => this.tabs.includes(d));
  tabs: Doc[] = [];
  active: Doc | null = null;
  private viewerDispose: (() => void) | null = null;
  private disposed = false;
  private tabsHtml = "";
  private bannerHtml = "";

  constructor(readonly app: App) {
    this.el.className = "pane";
    this.tabbar.className = "tabbar";
    this.banner.className = "banner";
    this.banner.hidden = true;
    this.body.className = "pane-body";
    this.editorHost.className = "editor-host";
    this.viewer.hidden = true;
    this.empty.className = "empty";
    this.empty.hidden = true;
    this.body.append(this.editorHost, this.viewer, this.empty);
    this.el.append(this.tabbar, this.banner, this.body);
    this.tabbar.addEventListener("mousedown", (e) => {
      const tab = (e.target as HTMLElement).closest(".tab") as HTMLElement | null;
      app.setFocused(this);
      if (!tab) return;
      const doc = this.tabs[Number(tab.dataset.i)];
      if (!doc) return;
      if (e.button === 1) {
        e.preventDefault();
        void app.closeDoc(doc);
      } else if (e.button === 0 && !(e.target as HTMLElement).closest(".tab-close")) {
        void app.activate(doc);
      }
    });
    this.tabbar.addEventListener("click", (e) => {
      const close = (e.target as HTMLElement).closest(".tab-close");
      if (!close) return;
      const doc = this.tabs[Number((close.closest(".tab") as HTMLElement).dataset.i)];
      if (doc) void app.closeDoc(doc);
    });
    this.tabbar.addEventListener("auxclick", (e) => e.preventDefault());
    this.el.addEventListener("mousedown", () => app.setFocused(this), true);
    this.banner.addEventListener("click", (e) => {
      const act = (e.target as HTMLElement).dataset.act;
      const doc = this.active;
      if (!act || !doc) return;
      if (act === "reload") void app.reload(doc);
      else if (act === "overwrite" || act === "recreate") void app.save(doc, true);
      else if (act === "retry") void app.save(doc);
      else if (act === "close") void app.closeDoc(doc);
      else if (act === "dismiss") {
        doc.error = null;
        this.renderBanner();
      }
      // The button is about to disappear with the banner: give the keyboard back to the editor.
      if (act !== "close") this.focusEditor();
    });
  }

  renderTabs(): void {
    let html = "";
    for (let i = 0; i < this.tabs.length; i++) {
      const d = this.tabs[i];
      const cls = "tab" + (d === this.active ? " is-active" : "") + (d.dirty ? " is-dirty" : "") + (d.conflict ? " is-conflict" : "");
      const badge =
        (d.outside ? `<span class="tab-badge" title="Fichier hors vault">${iconSvg("external", 12)}</span>` : "") +
        (d.readOnly && d.kind === "md" ? `<span class="tab-badge is-readonly" title="Lecture seule">${iconSvg("lock", 12)}</span>` : "");
      html += `<div class="${cls}" data-i="${i}" title="${escapeAttr(d.path)}">${badge}<span class="tab-name">${escapeText(d.name)}</span><button class="tab-close" tabindex="-1" title="Fermer">${iconSvg("x", 13)}</button></div>`;
    }
    // Saves repaint the tabs often: leave the DOM alone when nothing changed (a click in progress keeps its target).
    if (html !== this.tabsHtml) {
      this.tabsHtml = html;
      this.tabbar.innerHTML = html;
    }
    const act = this.tabbar.querySelector(".tab.is-active") as HTMLElement | null;
    if (act && (act.offsetLeft < this.tabbar.scrollLeft || act.offsetLeft + act.offsetWidth > this.tabbar.scrollLeft + this.tabbar.clientWidth)) {
      this.tabbar.scrollLeft = act.offsetLeft - 20;
    }
  }

  renderBanner(): void {
    const d = this.active;
    let html = "";
    if (d && d.conflict === "deleted") {
      // The file is gone (deleted or moved by another tool): never recreate it silently at the old place.
      const editable = d.kind === "md" && !d.readOnly;
      html =
        `<span class="banner-text">Ce fichier a été supprimé ou déplacé sur le disque.${editable && d.dirty ? " Vos modifications ne sont pas enregistrées." : ""}</span>` +
        (editable ? `<button data-act="recreate">Enregistrer quand même à cet emplacement</button>` : "") +
        `<button class="primary" data-act="close">Fermer l'onglet</button>`;
    } else if (d && d.conflict) {
      const msg =
        d.conflict === "save"
          ? "Enregistrement refusé : ce fichier a changé sur le disque depuis son ouverture."
          : "Ce fichier a été modifié sur le disque alors que vous avez des changements non enregistrés.";
      html = `<span class="banner-text">${msg}</span><button data-act="reload">Recharger depuis le disque</button><button class="primary" data-act="overwrite">Écraser avec ma version</button>`;
    } else if (d && d.error && d.loaded) {
      // A failed save stays shown until a save succeeds; `io` failures are retried automatically.
      const tail = d.saveError === "io" ? " Nouvel essai automatique tant que le document reste modifié." : "";
      html = `<span class="banner-text">${escapeText(d.error)}${tail}</span>` + (d.saveError ? `<button data-act="retry">Réessayer maintenant</button>` : `<button data-act="dismiss">Fermer</button>`);
    }
    if (html !== this.bannerHtml) {
      this.bannerHtml = html;
      this.banner.innerHTML = html;
      this.banner.hidden = !html;
    }
  }

  /** Stores the live editor state back into the document the editor holds (the scroll position only while shown). */
  stash(): void {
    this.held.stash(this.view, !this.editorHost.hidden);
  }

  private showOnly(which: HTMLElement): void {
    for (const el of [this.editorHost, this.viewer, this.empty]) el.hidden = el !== which;
    if (which !== this.viewer && this.viewerDispose) {
      this.viewerDispose();
      this.viewerDispose = null;
      this.viewer.textContent = "";
    }
  }

  /** Releases what the pane holds outside its own DOM: the viewer's global listeners and timers, the editor view. */
  dispose(): void {
    this.disposed = true;
    this.viewerDispose?.();
    this.viewerDispose = null;
    this.viewer.textContent = "";
    this.view?.destroy();
    this.view = null;
    this.held.release();
  }

  showEmpty(html: string, error = false): void {
    this.empty.className = "empty" + (error ? " is-error" : "");
    this.empty.innerHTML = html;
    this.showOnly(this.empty);
  }

  /** Displays the active document (must be loaded or in error). */
  show(): void {
    const d = this.active;
    this.renderBanner();
    if (!d) {
      this.showEmpty(this.app.welcomeHtml());
      return;
    }
    if (d.error && !d.loaded) {
      this.showEmpty(`<h2>Impossible d'ouvrir ce fichier</h2><div>${escapeText(d.error)}</div><div>${escapeText(d.path)}</div>`, true);
      return;
    }
    if (!d.loaded) {
      this.showEmpty("");
      return;
    }
    if (d.kind === "md" && d.state) {
      // The note the editor already holds keeps its live state (text, history, scroll): never its older snapshot.
      const wasHidden = this.editorHost.hidden;
      const given = this.held.give(this.view, d, !wasHidden, (state) => createEditorView(this.editorHost, state));
      this.view = given.view;
      this.showOnly(this.editorHost);
      const view = this.view;
      if ((given.replaced || wasHidden) && d.scrollTop > 0) {
        view.scrollDOM.scrollTop = d.scrollTop;
        requestAnimationFrame(() => {
          if (this.active === d && view.scrollDOM.scrollTop !== d.scrollTop) view.scrollDOM.scrollTop = d.scrollTop;
        });
      }
      return;
    }
    this.showOnly(this.viewer);
    this.viewerDispose?.();
    this.viewerDispose = null;
    this.viewer.textContent = "";
    if (d.kind === "image") {
      this.viewer.className = "viewer-image";
      const img = document.createElement("img");
      img.src = this.app.backend.assetUrl(d.path);
      img.alt = d.name;
      this.viewer.appendChild(img);
    } else if (d.kind === "canvas") {
      this.viewer.className = "canvas-view";
      const host = this.viewer;
      void import("./canvas").then((m) => {
        // The pane may have been closed, or the tab changed, while the module loaded: mount nothing (no listener left behind).
        if (this.disposed || this.active !== d || host.hidden) return;
        this.viewerDispose?.();
        host.textContent = "";
        const t0 = performance.now();
        this.viewerDispose = m.mountCanvas(host, d.raw ?? "", this.app.canvasHost(d));
        this.app.metrics.canvasMs = Math.round((performance.now() - t0) * 10) / 10;
      });
    }
  }

  focusEditor(): void {
    if (this.active?.kind === "md" && this.view && !this.editorHost.hidden) this.view.focus();
  }
}

export type { Pane };

function escapeText(s: string): string {
  return s.replace(/[&<>]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"));
}
function escapeAttr(s: string): string {
  return escapeText(s).replace(/"/g, "&quot;");
}

export class App implements EditorHost {
  vaults: VaultRef[];
  vaultRoot: string | null = null;
  /** Vault of the last session, not loaded because a file was passed on the command line (standalone start). */
  dormantVault: string | null = null;
  tree: Tree | null = null;
  index: FileIndex | null = null;
  panes: Pane[] = [];
  focused: Pane;
  autosave = loadPref("autosave", "1") === "1";
  sidebarOpen = loadPref("sidebar", "1") === "1";
  sidebarTab: SidebarTab = "files";
  /** Last `index-ready` payload of the open vault (tags, links, search). */
  indexInfo: IndexReady | null = null;
  private searchTerms: readonly string[] = [];
  /** History of `index-ready` payloads since launch (diagnostics and tests). */
  indexEvents: (IndexReady & { atMs: number })[] = [];
  sidebar: Sidebar | null = null;
  tagsProgress: { done: number; total: number } | null = null;
  tagsInfo: { files: number; tags: number; occurrences: number; ms: number; fromCache?: boolean; reread?: number; removed?: number } | null = null;
  /** Measurements exposed to the test driver. */
  metrics: Record<string, number> = {};
  commands: Command[] = [];

  /** Saving, reloading, conflicts and the closing sequence (logic without DOM, see docsync.ts). */
  readonly sync: DocSync<Doc>;
  /** `StartupInfo.firstFile`, consumed by the first load of that path. */
  private primed: FileContent | null = null;
  private tagGen = 0;
  private tagLib: Promise<TagLibrary> | null = null;
  private marksSent = false;
  private pendingMarks: [string, number][] = [];
  private epochN = 0;
  private sessionTimer = 0;
  private statusTimer = 0;
  private toastTimer = 0;
  private quickItems: (FuzzyItem & { rel: string })[] | null = null;
  private linkItems: (FuzzyItem & { rel: string })[] | null = null;
  private pickerOpen = false;
  private readonly ws = document.getElementById("workspace")!;
  private readonly sidebarEl = document.getElementById("sidebar")!;
  private readonly st = {
    path: document.createElement("span"),
    toast: document.createElement("span"),
    tags: document.createElement("span"),
    words: document.createElement("span"),
    save: document.createElement("span"),
    auto: document.createElement("button"),
    mode: document.createElement("button"),
    vault: document.createElement("button"),
  };
  private lat: { active: boolean; t0: number; update: number[]; layout: number[] } = { active: false, t0: 0, update: [], layout: [] };

  constructor(
    readonly backend: Backend,
    readonly info: StartupInfo,
  ) {
    this.vaults = info.vaults;
    this.sync = new DocSync<Doc>({
      backend,
      autosave: () => this.autosave,
      textOf: (d) => (d.kind === "md" ? (this.stateOf(d)?.doc ?? null) : null),
      applyDisk: (d, text) => this.applyDisk(d, text),
      refresh: (d) => this.refreshDoc(d),
      saved: () => {
        // Tags are up to date when save_file returns; no event says so. Links: `index-ready` follows when they changed.
        this.invalidateTags();
      },
      toast: (m) => this.toast(m),
    });
    this.focused = this.addPane();
    this.buildStatusBar();
    this.buildCommands();
    window.addEventListener("keydown", (e) => this.onKey(e), true);
    // No `beforeunload` save: it was not awaited. Closing goes through `close-requested` (see requestClose).
  }

  /** Startup milestone, in ms since navigation; sent to `log_timing` in one batch once the editor is painted. */
  mark(name: string, at = performance.now()): void {
    const ms = Math.round(at);
    this.metrics["mark:" + name] = ms;
    if (this.marksSent) void this.backend.logTiming("ui-" + name, ms).catch(() => undefined);
    else this.pendingMarks.push([name, ms]);
  }

  flushMarks(): void {
    this.marksSent = true;
    for (const [name, ms] of this.pendingMarks.splice(0)) void this.backend.logTiming("ui-" + name, ms).catch(() => undefined);
  }

  /** Hands over the file read by the backend during startup (`StartupInfo.firstFile`). */
  primeFile(fc: FileContent): void {
    this.primed = fc;
  }

  // ------------------------------------------------------------ panes & docs

  private addPane(): Pane {
    const p = new Pane(this);
    this.panes.push(p);
    this.ws.appendChild(p.el);
    p.renderTabs();
    this.markFocus();
    return p;
  }

  private markFocus(): void {
    for (const p of this.panes) p.el.classList.toggle("is-focused", p === this.focused);
  }

  setFocused(p: Pane): void {
    if (this.focused === p) return;
    this.focused = p;
    this.markFocus();
    this.updateStatus();
    this.queueSession();
    this.sidebar?.onActiveDoc();
  }

  docs(): Doc[] {
    const out: Doc[] = [];
    for (const p of this.panes) out.push(...p.tabs);
    return out;
  }

  findDoc(path: string): Doc | null {
    for (const p of this.panes) for (const d of p.tabs) if (samePath(d.path, path)) return d;
    return null;
  }

  activeDoc(): Doc | null {
    return this.focused.active;
  }

  /** The current state of a note: its editor's when the editor of its pane holds it, its snapshot otherwise (SV-18). */
  stateOf(doc: Doc): EditorState | null {
    return doc.pane.held.stateOf(doc.pane.view, doc);
  }

  /** The editor to dispatch to for a note, or null when its state is the snapshot (SV-18). */
  viewOf(doc: Doc): EditorView | null {
    return doc.pane.held.viewOf(doc.pane.view, doc);
  }

  private isOutside(path: string): boolean {
    return !this.vaultRoot || toRel(this.vaultRoot, path) == null;
  }

  private kindOf(path: string): DocKind | null {
    const ext = extname(path);
    if (isMarkdownPath(path) || ext === "txt" || ext === "") return "md";
    if (ext === "canvas") return "canvas";
    if (isImagePath(path)) return "image";
    return null;
  }

  /** Adds a tab without loading its content (session restore). */
  addTab(path: string, pane: Pane = this.focused): Doc | null {
    const existing = this.findDoc(path);
    if (existing) return existing;
    const kind = this.kindOf(path);
    if (!kind) return null;
    const name = basename(path);
    const doc: Doc = {
      path,
      name: kind === "md" && isMarkdownPath(path) ? stripExt(name) : name,
      kind,
      pane,
      state: null,
      loaded: false,
      loading: null,
      error: null,
      readOnly: false,
      saveError: null,
      retries: 0,
      mtimeMs: 0,
      eol: "\n",
      eolMixed: null,
      bom: false,
      dirty: false,
      saving: null,
      conflict: null,
      pendingExternal: null,
      scrollTop: 0,
      timer: 0,
      outside: this.isOutside(path),
      raw: null,
    };
    pane.tabs.push(doc);
    pane.renderTabs();
    return doc;
  }

  async openPath(path: string, opts: OpenOptions = {}): Promise<Doc | null> {
    let doc = this.findDoc(path);
    if (!doc) {
      doc = this.addTab(path, opts.pane ?? this.focused);
      if (!doc) {
        this.toast(`Type de fichier non pris en charge : ${basename(path)}`);
        return null;
      }
      this.queueSession();
    }
    if (opts.activate !== false) {
      await this.activate(doc);
      if (doc.loaded && doc.kind === "md") {
        const view = this.viewOf(doc);
        if (view) {
          if (opts.lines) revealLines(view, opts.lines.start, opts.lines.end, opts.lines.col ?? 0);
          else if (opts.heading) {
            const line = findHeadingLine(view.state.doc, opts.heading);
            if (line >= 0) revealLines(view, line, line);
            else this.toast(`Titre introuvable : ${opts.heading}`);
          }
        }
      }
    }
    return doc;
  }

  private load(doc: Doc): Promise<void> {
    if (doc.loaded) return Promise.resolve();
    if (doc.loading) return doc.loading;
    doc.error = null;
    const job = (async () => {
      try {
        if (doc.kind === "image") {
          doc.loaded = true;
          return;
        }
        // The backend may have read the first note while the WebView was starting: no second round trip then.
        let fc = this.primed;
        this.primed = null;
        const prefetched = !!fc && samePath(fc.path, doc.path);
        if (!fc || !prefetched) fc = await this.backend.readFile(doc.path);
        const first = this.metrics["mark:file-read"] === undefined;
        if (first) {
          this.mark("file-read");
          this.metrics.firstFilePrefetched = prefetched ? 1 : 0;
        }
        const dec = decodeText(fc.text);
        doc.bom = dec.bom;
        doc.eol = dec.eol;
        doc.eolMixed = dec.mixed;
        doc.mtimeMs = fc.mtimeMs;
        doc.readOnly = !!fc.readOnly;
        if (doc.kind === "md") doc.state = createEditorState(dec.text, doc.path, this, doc.readOnly);
        else doc.raw = dec.text;
        if (first) this.mark("state-created");
        doc.dirty = false;
        doc.loaded = true;
      } catch (e) {
        doc.error = openErrorMessage(toApiError(e));
      } finally {
        doc.loading = null;
      }
    })();
    doc.loading = job;
    return job;
  }

  async activate(doc: Doc): Promise<void> {
    const pane = doc.pane;
    if (pane.active !== doc) {
      pane.stash();
      pane.active = doc;
      pane.renderTabs();
    }
    if (this.focused !== pane) {
      this.focused = pane;
      this.markFocus();
    }
    // A restored tab of the dormant vault wakes that vault up (tree, links, tags).
    if (!this.vaultRoot && this.dormantVault && toRel(this.dormantVault, doc.path) != null) {
      const v = this.dormantVault;
      this.dormantVault = null;
      void this.openVault(v);
    }
    if (!doc.loaded && !doc.error) {
      const pending = this.load(doc);
      // Avoid flashing an empty pane for fast reads.
      let shown = false;
      const timer = window.setTimeout(() => {
        if (pane.active === doc && !doc.loaded) {
          shown = true;
          pane.show();
        }
      }, 120);
      await pending;
      clearTimeout(timer);
      void shown;
      // What the tab shows may come with the content (read-only badge).
      pane.renderTabs();
    }
    if (pane.active !== doc) return;
    pane.show();
    pane.focusEditor();
    this.applySearchTerms();
    this.updateStatus();
    this.queueSession();
    this.sidebar?.onActiveDoc();
  }

  async closeDoc(doc: Doc): Promise<void> {
    // A save in flight is awaited before deciding: if it fails, the text is still here and the tab stays.
    if (this.autosave) await this.sync.flush(doc);
    else await this.sync.settle(doc);
    if (this.sync.unsaved(doc)) {
      const why = doc.conflict === "deleted" ? "Le fichier a été supprimé ou déplacé sur le disque." : doc.conflict ? "Le fichier a changé sur le disque (conflit)." : doc.saveError ? (doc.error ?? "L'enregistrement a échoué.") : "L'enregistrement automatique est désactivé.";
      const canSave = !doc.conflict;
      const choice = await askChoice({
        title: `« ${doc.name} » a des modifications non enregistrées`,
        lines: [why],
        buttons: [
          { id: "cancel", label: "Annuler" },
          { id: "discard", label: "Fermer sans enregistrer", kind: "danger" },
          ...(canSave ? [{ id: "save", label: "Enregistrer et fermer", kind: "primary" as const }] : []),
        ],
        cancelId: "cancel",
      });
      if (choice === "cancel") return;
      if (choice === "save") {
        await this.sync.save(doc);
        if (this.sync.unsaved(doc)) {
          this.toast(`« ${doc.name} » n'a pas pu être enregistré : l'onglet reste ouvert`);
          return;
        }
      }
    }
    this.sync.cancelTimer(doc);
    const pane = doc.pane;
    const i = pane.tabs.indexOf(doc);
    if (i < 0) return;
    pane.tabs.splice(i, 1);
    if (pane.active === doc) {
      pane.active = null;
      const next = pane.tabs[Math.min(i, pane.tabs.length - 1)] ?? null;
      if (next) {
        pane.renderTabs();
        await this.activate(next);
      } else if (this.panes.length > 1) {
        this.removePane(pane);
      } else {
        if (pane.view) {
          pane.view.destroy();
          pane.view = null;
          pane.held.release();
        }
        pane.renderTabs();
        pane.show();
      }
    } else pane.renderTabs();
    this.updateStatus();
    this.queueSession();
  }

  private removePane(pane: Pane): void {
    const i = this.panes.indexOf(pane);
    if (i < 0 || this.panes.length === 1) return;
    pane.dispose();
    pane.el.remove();
    this.panes.splice(i, 1);
    if (this.focused === pane) this.focused = this.panes[Math.max(0, i - 1)];
    this.markFocus();
    this.focused.focusEditor();
  }

  /** Two panes side by side: the active tab moves to the new pane when the current one keeps another tab. */
  splitRight(): void {
    if (this.panes.length >= 2) {
      const other = this.panes.find((p) => p !== this.focused)!;
      this.setFocused(other);
      other.focusEditor();
      return;
    }
    const from = this.focused;
    const doc = from.active;
    const pane = this.addPane();
    if (doc && from.tabs.length > 1) {
      from.stash();
      from.held.release();
      const i = from.tabs.indexOf(doc);
      from.tabs.splice(i, 1);
      from.active = null;
      from.renderTabs();
      doc.pane = pane;
      pane.tabs.push(doc);
      void this.activate(from.tabs[Math.min(i, from.tabs.length - 1)]).then(() => this.activate(doc));
    } else {
      this.focused = pane;
      this.markFocus();
      pane.show();
    }
    this.queueSession();
  }

  welcomeHtml(): string {
    const vault = this.vaultRoot ? "" : `<button data-cmd="vault.add">Ajouter un vault</button>`;
    return `<h2>Rocksy</h2><div class="row"><kbd>Ctrl</kbd>+<kbd>O</kbd> ouvrir une note</div><div class="row"><kbd>Ctrl</kbd>+<kbd>P</kbd> palette de commandes</div><div>ou glissez un fichier <b>.md</b> dans la fenêtre</div><div class="row"><button data-cmd="file.open">Ouvrir un fichier…</button>${vault}</div>`;
  }

  // ------------------------------------------------------------ saving

  /** Saves a document; resolves when the backend has answered (see DocSync.save). `force`: without the mtime check. */
  save(doc: Doc, force = false): Promise<void> {
    return this.sync.save(doc, force);
  }

  async reload(doc: Doc): Promise<void> {
    await this.sync.reload(doc);
  }

  onFileChanged(path: string, mtimeMs: number, deleted = false): void {
    const doc = this.findDoc(path);
    if (doc) this.sync.onFileChanged(doc, mtimeMs, deleted);
  }

  /** Puts the content read from disk in the editor (reload). Not an edit: no history entry, not dirty. */
  private applyDisk(doc: Doc, text: string): void {
    if (doc.kind === "md") {
      const state = this.stateOf(doc);
      if (!state) return;
      const fresh = Text.of(text.split(/\r\n?|\n/));
      const lock = lockEffect(state, doc.readOnly);
      const tr = {
        changes: { from: 0, to: state.doc.length, insert: fresh },
        selection: { anchor: Math.min(state.selection.main.head, fresh.length) },
        effects: lock ? [lock] : [],
        annotations: [externalChange.of(true), Transaction.addToHistory.of(false)],
        // A read-only document refuses every change through a filter; this one comes from the disk.
        filter: false,
      };
      const view = this.viewOf(doc);
      if (view) view.dispatch(tr);
      else doc.state = state.update(tr).state;
      this.queueStatus();
    } else {
      doc.raw = text;
      if (doc.pane.active === doc) doc.pane.show();
    }
  }

  private refreshDoc(doc: Doc): void {
    if (!doc.pane.tabs.includes(doc)) return;
    doc.pane.renderTabs();
    if (doc.pane.active === doc) doc.pane.renderBanner();
    this.updateSaveState();
  }

  /**
   * `close-requested`: acknowledge, save everything and wait, ask in an application dialog when something
   * cannot be saved silently, then `quit`. Requests received meanwhile are acknowledged and do not stack dialogs.
   */
  async requestClose(): Promise<"quit" | "cancelled" | "busy"> {
    try {
      return await this.sync.requestClose({
        docs: () => this.docs(),
        closeAck: () => this.backend.closeAck(),
        quit: () => this.backend.quit(),
        ask: (issues, canSave) => this.askBeforeQuit(issues, canSave),
      });
    } catch (e) {
      this.toast(`Fermeture impossible : ${toApiError(e).message}`);
      return "cancelled";
    }
  }

  private async askBeforeQuit(issues: CloseIssue<Doc>[], canSave: boolean): Promise<"save" | "discard" | "cancel"> {
    const label = (i: CloseIssue<Doc>) =>
      `${i.doc.name} — ${i.reason === "conflict" ? (i.doc.conflict === "deleted" ? "fichier supprimé ou déplacé sur le disque" : "conflit avec la version du disque") : i.reason === "failed" ? (i.doc.error ?? "échec de l'enregistrement") : "modifications non enregistrées (enregistrement automatique désactivé)"}`;
    const onlyConflicts = !canSave;
    const choice = await askChoice({
      title: issues.length > 1 ? `${issues.length} documents ne sont pas enregistrés` : "Un document n'est pas enregistré",
      lines: [
        onlyConflicts
          ? "Ces documents sont en conflit avec le disque : ils ne peuvent pas être enregistrés automatiquement. Annulez pour choisir dans le bandeau de chaque document, ou quittez sans les enregistrer."
          : "Quitter maintenant perdrait ces modifications." + (issues.some((i) => i.reason === "conflict") ? " Les documents en conflit ne seront pas écrasés par « Enregistrer et quitter »." : ""),
      ],
      items: issues.map(label),
      buttons: [
        { id: "cancel", label: "Annuler" },
        { id: "discard", label: "Quitter sans enregistrer", kind: "danger" },
        ...(canSave ? [{ id: "save", label: "Enregistrer et quitter", kind: "primary" as const }] : []),
      ],
      cancelId: "cancel",
    });
    return choice as "save" | "discard" | "cancel";
  }

  // ------------------------------------------------------------ editor host

  onUpdate(u: ViewUpdate): void {
    let pane: Pane | null = null;
    for (const p of this.panes) if (p.view === u.view) pane = p;
    // The note the editor holds, which is not the active tab while that one is still being read (SV-18).
    const doc = pane?.held.holder();
    if (!pane || !doc) return;
    if (u.docChanged) {
      let external = false;
      for (const tr of u.transactions) if (tr.annotation(externalChange)) external = true;
      if (!external) {
        if (!doc.dirty) {
          doc.dirty = true;
          pane.renderTabs();
          this.updateSaveState();
        }
        this.sync.scheduleAutosave(doc);
      }
      this.queueStatus();
      if (this.lat.active && this.lat.t0) {
        const t1 = performance.now();
        this.lat.update.push(t1 - this.lat.t0);
        void u.view.contentDOM.offsetHeight;
        this.lat.layout.push(performance.now() - this.lat.t0);
        this.lat.t0 = 0;
      }
    }
    if (u.focusChanged && u.view.hasFocus) this.setFocused(pane);
  }

  epoch(): number {
    return this.epochN;
  }

  isImage(target: string): boolean {
    return isImagePath(target);
  }

  resolveImage(src: string, fromPath: string): ImageRef {
    const t = resolveImage(src, { fileAbs: fromPath, vaultRoot: this.isOutside(fromPath) ? null : this.vaultRoot, index: this.index });
    if (t.kind === "local") return { kind: "url", url: this.backend.assetUrl(t.abs) };
    if (t.kind === "remote") return /^data:/i.test(t.url) ? { kind: "url", url: t.url } : { kind: "remote", url: t.url };
    // A network source is refused, not missing: say so (nothing was looked up, no connection was opened).
    return isNetworkSource(src) ? { kind: "network" } : { kind: "missing" };
  }

  linkExists(inner: string, fromPath: string): boolean | null {
    if (!this.index || !this.vaultRoot) return null;
    const rel = toRel(this.vaultRoot, fromPath);
    if (rel == null) return null;
    const l = parseWikilink(inner);
    if (!l.target) return true;
    return resolveLink(l.target, rel, this.index) != null;
  }

  openLink(link: LinkRef, fromPath: string, newTab: boolean): void {
    void newTab;
    if (link.kind === "url") {
      let href = link.href.replace(/^<|>$/g, "");
      if (isNetworkSource(href)) {
        this.toastNetwork(href);
        return;
      }
      // A `file:` URL to a local path is a local link (network ones were refused above).
      if (/^file:/i.test(href)) href = stripFileScheme(href) ?? href;
      if (/^[a-z][a-z0-9+.-]*:/i.test(href) && !/^[a-zA-Z]:[\\/]/.test(href)) {
        this.backend.openExternal(href).catch((e) => this.toast(`Lien non ouvert : ${toApiError(e).message || href}`));
        return;
      }
      const [file, anchor] = href.split("#");
      this.openTarget(decodeSafe(file), anchor ? decodeSafe(anchor) : null, fromPath);
      return;
    }
    const l = parseWikilink(link.inner);
    this.openTarget(l.target, l.heading, fromPath);
  }

  private toastNetwork(target: string): void {
    this.toast(`Lien vers un chemin réseau : non ouvert (${target.slice(0, 120)})`);
  }

  private openTarget(target: string, heading: string | null, fromPath: string): void {
    if (!target) {
      void this.openPath(fromPath, { heading });
      return;
    }
    if (isNetworkSource(target)) {
      this.toastNetwork(target);
      return;
    }
    const rel = this.vaultRoot ? toRel(this.vaultRoot, fromPath) : null;
    if (this.index && this.vaultRoot && rel != null) {
      const hit = resolveLink(target, rel, this.index);
      if (hit) void this.openPath(joinAbs(this.vaultRoot, hit), { heading });
      else this.toast(`Note introuvable : ${target}`);
      return;
    }
    // Outside a vault: resolve relative to the file.
    const abs = isAbsolute(target) ? target : joinAbs(dirname(fromPath), /\.[a-z0-9]{1,8}$/i.test(target) ? target : target + ".md");
    void this.openPath(abs, { heading }).then((d) => {
      if (d && d.error && !d.loaded) {
        this.toast(`Lien non ouvert : ${d.error}`);
        void this.closeDoc(d);
      }
    });
  }

  openTag(tag: string): void {
    void this.showSidebar("tags").then(() => this.sidebar?.selectTag(tag));
  }

  /** A file referenced by a canvas: resolved by the vault index and kept under the vault root (see resolveCanvasFile). */
  private canvasFile(doc: Doc, ref: string): string | null {
    return resolveCanvasFile(ref, { fileAbs: doc.path, vaultRoot: doc.outside ? null : this.vaultRoot, index: this.index });
  }

  canvasHost(doc: Doc) {
    return {
      assetUrl: (rel: string) => {
        const abs = this.canvasFile(doc, rel);
        return abs ? this.backend.assetUrl(abs) : null;
      },
      open: (rel: string) => {
        const abs = this.canvasFile(doc, rel);
        if (abs) void this.openPath(abs);
        else this.toast(`Fichier du canvas introuvable dans ${this.vaultRoot && !doc.outside ? "le vault" : "son dossier"} : ${rel.slice(0, 120)}`);
      },
      readText: (rel: string) => {
        const abs = this.canvasFile(doc, rel);
        if (!abs) return Promise.resolve(null);
        return this.backend.readFile(abs).then(
          (f) => f.text,
          () => null,
        );
      },
    };
  }

  // ------------------------------------------------------------ completion host (extras)

  completeFiles(query: string): { label: string; insert: string; detail: string }[] {
    const index = this.index;
    if (!index) return [];
    if (!this.linkItems) {
      this.linkItems = index.files.map((rel) => ({ ...makeItem(/\.md$/i.test(rel) ? rel.slice(0, -3) : rel), rel }));
    }
    return fuzzySearch(query, this.linkItems, 30).map((h) => ({
      label: basename(h.item.text),
      insert: linkTextFor(h.item.rel, index),
      detail: dirname(h.item.rel),
    }));
  }

  async completeTags(query: string): Promise<TagCount[]> {
    if (this.vaultRoot) return filterTagLibrary(await this.allTags(), query);
    const doc = this.activeDoc();
    const state = doc && this.stateOf(doc);
    if (!doc || !state) return [];
    const { countTags, filterTags } = await import("./tagtree");
    return filterTags(countTags(await this.backend.fileTags(doc.path, state.doc.toString())), query);
  }

  async loadExtras(): Promise<void> {
    const m = await import("./editor/extras");
    setExtras(m.extras(this));
    for (const p of this.panes) {
      if (p.view && p.active?.kind === "md") {
        const fx = syncEffects(p.view.state);
        if (fx.length) p.view.dispatch({ effects: fx });
      }
    }
  }

  togglePreview(): void {
    setPreview(!isPreviewOn());
    for (const p of this.panes) {
      if (p.view && p.active?.kind === "md" && !p.editorHost.hidden) {
        const fx = syncEffects(p.view.state);
        if (fx.length) p.view.dispatch({ effects: fx });
      }
    }
    this.updateStatus();
  }

  // ------------------------------------------------------------ vault

  /** Reserves the sidebar area right away so the editor does not shift when the tree arrives. */
  reserveSidebar(): void {
    const show = this.sidebarOpen && !!this.vaultRoot;
    this.sidebarEl.hidden = !show;
    const w = Number(loadPref("sidebarWidth", "290"));
    document.documentElement.style.setProperty("--sidebar-w", Math.min(600, Math.max(180, w)) + "px");
    if (show && !this.sidebar && !this.sidebarEl.firstChild) {
      this.sidebarEl.innerHTML = `<div class="sb-head"></div><div class="sb-skeleton">${"<i></i>".repeat(9).replace(/<i>/g, () => `<i style="width:${40 + Math.floor(Math.random() * 50)}%">`)}</div>`;
    }
    document.getElementById("rb-sidebar")?.classList.toggle("is-on", show);
  }

  async ensureSidebar(): Promise<Sidebar> {
    if (!this.sidebar) {
      const m = await import("./sidebar");
      if (!this.sidebar) {
        this.sidebarEl.textContent = "";
        this.sidebar = new m.Sidebar(this.sidebarEl, this);
      }
    }
    return this.sidebar;
  }

  async showSidebar(tab?: SidebarTab): Promise<void> {
    this.sidebarOpen = true;
    savePref("sidebar", "1");
    if (tab) this.sidebarTab = tab;
    const sb = await this.ensureSidebar();
    this.sidebarEl.hidden = false;
    document.getElementById("rb-sidebar")?.classList.add("is-on");
    sb.setTab(this.sidebarTab);
  }

  toggleSidebar(): void {
    if (!this.sidebarEl.hidden) {
      this.sidebarOpen = false;
      savePref("sidebar", "0");
      this.sidebarEl.hidden = true;
      document.getElementById("rb-sidebar")?.classList.remove("is-on");
    } else void this.showSidebar();
  }

  async openVault(path: string): Promise<void> {
    const t0 = performance.now();
    const previous = this.vaultRoot;
    // Changing vault: the backend stops following the previous one and releases its index.
    if (previous && !samePath(previous, path)) void this.backend.closeVault(previous).catch(() => undefined);
    this.vaultRoot = path;
    this.dormantVault = null;
    this.tree = null;
    this.index = null;
    this.quickItems = null;
    this.linkItems = null;
    this.tagsInfo = null;
    this.tagsProgress = null;
    this.indexInfo = null;
    this.invalidateTags();
    this.refreshOutside();
    this.reserveSidebar();
    this.updateStatus();
    // The index is requested at the same time as the tree, in either order: the backend makes one listing serve
    // both and never drops the index pass of the vault being opened (see CONTRACT.md, `build_tags`).
    const opening = this.backend.openVault(path);
    const building = this.backend.buildTags(path).then(
      () => null,
      (e: unknown) => toApiError(e),
    );
    try {
      const tree = await opening;
      if (!this.vaultRoot || !samePath(this.vaultRoot, path)) return;
      this.metrics.openVaultMs = Math.round(performance.now() - t0);
      this.mark("tree-received");
      this.setTree(tree);
      this.mark("tree-applied");
      if (this.sidebarOpen) await this.showSidebar();
      this.metrics.treeReadyMs = Math.round(performance.now());
      void this.backend.logTiming("tree-ready", Date.now() - this.info.processStartMs);
      const failed = await building;
      if (failed && this.vaultRoot && samePath(this.vaultRoot, path)) this.toast(`Index du vault indisponible : ${failed.message}`);
    } catch (e) {
      if (this.vaultRoot && samePath(this.vaultRoot, path)) {
        this.toast(`Impossible d'ouvrir le vault : ${toApiError(e).message}`);
        this.vaultRoot = null;
        this.reserveSidebar();
        this.refreshOutside();
        void this.backend.closeVault(path).catch(() => undefined);
      }
    }
    this.queueSession();
  }

  closeVault(): void {
    const closed = this.vaultRoot;
    this.vaultRoot = null;
    this.dormantVault = null;
    this.tree = null;
    this.index = null;
    this.quickItems = null;
    this.linkItems = null;
    this.tagsInfo = null;
    this.tagsProgress = null;
    this.indexInfo = null;
    this.invalidateTags();
    this.epochN++;
    this.refreshOutside();
    this.reserveSidebar();
    this.refreshPreviews();
    this.sidebar?.setTree(null);
    this.updateStatus();
    this.queueSession();
    // The backend stops the watcher and the reconciliations of this vault, persists and releases its index.
    if (closed) void this.backend.closeVault(closed).catch(() => undefined);
  }

  setTree(tree: Tree): void {
    const previous = this.tree;
    const samePaths = !!previous && !!this.index && samePath(previous.root, tree.root) && sameTreePaths(previous.entries, tree.entries);
    this.tree = tree;
    if (samePaths) {
      // Same set of files: links and images resolve as before. Nothing is rebuilt, images are not recreated.
      this.sidebar?.setTree(tree, true);
      return;
    }
    const hadIndex = !!this.index;
    this.index = new FileIndex(tree.entries);
    this.quickItems = null;
    this.linkItems = null;
    this.epochN++;
    this.sidebar?.setTree(tree);
    this.refreshPreviews();
    // A canvas shown before the tree was known could not resolve its files: mount it again, once.
    if (!hadIndex) for (const p of this.panes) if (p.active?.kind === "canvas" && p.active.loaded && !p.active.outside) p.show();
  }

  /** Forgets the cached tag list (`tags-ready`, `index-ready`, a save, a vault change): the next use asks the backend once. */
  invalidateTags(): void {
    this.tagGen++;
    this.tagLib = null;
  }

  /** Whole tag list of the open vault, fetched once and filtered in memory (no backend call per keystroke). */
  allTags(): Promise<TagLibrary> {
    if (!this.tagLib) {
      const gen = this.tagGen;
      const p = this.backend.tagsList("").then(makeTagLibrary);
      p.catch(() => {
        if (this.tagGen === gen) this.tagLib = null;
      });
      this.tagLib = p;
    }
    return this.tagLib;
  }

  private refreshPreviews(): void {
    // Also a hidden editor (a canvas or an image is shown): the note it holds is shown again without a new state (SV-18).
    for (const p of this.panes) if (p.view && p.held.holder()) refreshPreview(p.view);
  }

  private refreshOutside(): void {
    for (const p of this.panes) {
      for (const d of p.tabs) d.outside = this.isOutside(d.path);
      p.renderTabs();
    }
  }

  async addVault(): Promise<void> {
    const dir = await this.backend.pickFolder();
    if (!dir) return;
    try {
      this.vaults = await this.backend.addVault(dir);
      this.sidebarOpen = true;
      await this.openVault(dir);
    } catch (e) {
      this.toast(`Ajout du vault impossible : ${toApiError(e).message}`);
    }
  }

  async removeVault(path: string): Promise<void> {
    // Close first: `remove_vault` also deletes the derived data of the vault.
    if (this.vaultRoot && samePath(this.vaultRoot, path)) this.closeVault();
    this.vaults = await this.backend.removeVault(path);
    this.toast("Vault retiré de la liste (aucun fichier modifié)");
  }

  async openFileDialog(): Promise<void> {
    const f = await this.backend.pickFile();
    if (f) await this.openPath(f);
  }

  // ------------------------------------------------------------ pickers

  private async picker() {
    return import("./picker");
  }

  quickItemsList(): (FuzzyItem & { rel: string })[] {
    if (!this.quickItems) {
      const files = this.index ? this.index.files : [];
      const items: (FuzzyItem & { rel: string })[] = [];
      for (const rel of files) if (this.kindOf(rel)) items.push({ ...makeItem(rel), rel });
      this.quickItems = items;
    }
    return this.quickItems;
  }

  async quickOpen(): Promise<void> {
    if (this.pickerOpen) return;
    this.pickerOpen = true;
    const { openPicker } = await this.picker();
    const root = this.vaultRoot;
    const all = this.quickItemsList();
    const openTabs: PickerItem[] = this.docs().map((d) => ({
      ...makeItem(root && toRel(root, d.path) != null ? toRel(root, d.path)! : d.path),
      icon: "file",
      run: () => void this.activate(d),
    }));
    const iconOf = (rel: string) => (isImagePath(rel) ? "image" : rel.endsWith(".canvas") ? "canvas" : "file");
    openPicker({
      placeholder: root ? "Ouvrir une note…" : "Aucun vault ouvert : onglets ouverts, ou Entrée pour choisir un fichier",
      split: true,
      search: (q) => {
        if (!q.trim()) {
          const seen = new Set(openTabs.map((t) => t.text.toLowerCase()));
          const rest: PickerItem[] = [];
          for (let i = 0; i < all.length && rest.length < 40; i++) {
            if (!seen.has(all[i].rel.toLowerCase()) && /\.md$/i.test(all[i].rel)) rest.push({ ...all[i], icon: "file", run: () => void this.openPath(joinAbs(root!, all[i].rel)) });
          }
          return [...openTabs, ...rest].map((item) => ({ item, score: 0, positions: [] }));
        }
        const t0 = performance.now();
        const hits = fuzzySearch(q, all, 50);
        this.metrics.quickOpenFilterMs = Math.round((performance.now() - t0) * 100) / 100;
        this.metrics.quickOpenItems = all.length;
        return hits.map((h) => ({ ...h, item: { ...h.item, icon: iconOf(h.item.rel), run: () => void this.openPath(joinAbs(root!, h.item.rel)) } }));
      },
      emptyText: root ? "Aucun fichier ne correspond" : "Aucun vault ouvert",
      onEmptyEnter: root ? undefined : () => void this.openFileDialog(),
      footer: root ? `${all.length.toLocaleString("fr-FR")} fichiers` : "",
      onClose: () => {
        this.pickerOpen = false;
        this.focused.focusEditor();
      },
      onRender: (ms) => (this.metrics.quickOpenRenderMs = ms),
    });
  }

  async palette(): Promise<void> {
    if (this.pickerOpen) return;
    this.pickerOpen = true;
    const { openPicker } = await this.picker();
    const items: PickerItem[] = this.commands.map((c) => ({ ...makeItem(c.title), icon: "command", detail: c.keys, run: c.run }));
    openPicker({
      placeholder: "Exécuter une commande…",
      search: (q) => fuzzySearch(q, items, 50),
      emptyText: "Aucune commande",
      onClose: () => {
        this.pickerOpen = false;
        this.focused.focusEditor();
      },
    });
  }

  async chooseVault(): Promise<void> {
    if (this.pickerOpen) return;
    this.pickerOpen = true;
    const { openPicker } = await this.picker();
    const items: PickerItem[] = this.vaults.map((v) => ({
      ...makeItem(v.name),
      icon: "vault",
      detail: v.path + (this.vaultRoot && samePath(v.path, this.vaultRoot) ? "  (ouvert)" : ""),
      run: () => void this.openVaultFromUi(v.path),
    }));
    items.push({ ...makeItem("Ajouter un vault…"), icon: "plus", run: () => void this.addVault() });
    if (this.vaultRoot) {
      const cur = this.vaultRoot;
      items.push({ ...makeItem("Fermer le vault (éditeur seul)"), icon: "x", run: () => this.closeVault() });
      items.push({ ...makeItem("Retirer ce vault de la liste"), icon: "x", detail: "ne supprime aucun fichier", run: () => void this.removeVault(cur) });
    }
    openPicker({
      placeholder: "Choisir un vault…",
      search: (q) => fuzzySearch(q, items, 50),
      emptyText: "Aucun vault",
      onClose: () => {
        this.pickerOpen = false;
        this.focused.focusEditor();
      },
    });
  }

  private async openVaultFromUi(path: string): Promise<void> {
    this.sidebarOpen = true;
    await this.openVault(path);
  }

  // ------------------------------------------------------------ commands & keys

  private buildCommands(): void {
    const c = (id: string, title: string, run: () => void, keys?: string) => this.commands.push({ id, title, run, keys });
    c("file.quickopen", "Ouvrir une note (sélecteur rapide)", () => void this.quickOpen(), "Ctrl+O");
    c("file.open", "Ouvrir un fichier Markdown…", () => void this.openFileDialog());
    c("file.save", "Enregistrer", () => this.saveActive(), "Ctrl+S");
    c("file.autosave", "Activer / désactiver l'enregistrement automatique", () => this.toggleAutosave());
    c("tab.close", "Fermer l'onglet", () => this.activeDoc() && void this.closeDoc(this.activeDoc()!), "Ctrl+W");
    c("tab.next", "Onglet suivant", () => this.cycleTab(1), "Ctrl+Tab");
    c("pane.split", "Fractionner à droite / changer de volet", () => this.splitRight(), "Ctrl+\\");
    c("vault.add", "Ajouter un vault…", () => void this.addVault());
    c("vault.choose", "Changer de vault…", () => void this.chooseVault());
    c("vault.close", "Fermer le vault", () => this.closeVault());
    c("view.sidebar", "Afficher / masquer le panneau latéral", () => this.toggleSidebar());
    c("view.files", "Afficher l'arbre des fichiers", () => void this.showSidebar("files"));
    c("view.tags", "Afficher les tags", () => void this.showSidebar("tags"));
    c("view.search", "Rechercher dans le vault", () => void this.showSidebar("search"), "Ctrl+Maj+F");
    c("view.links", "Afficher les liens de la note (entrants et sortants)", () => void this.showSidebar("links"));
    c("links.unresolved", "Liens non résolus", () => void this.showUnresolved());
    c("search.clear", "Effacer le surlignage de recherche", () => this.setSearchTerms([]));
    c("view.preview", "Basculer Live Preview / mode source", () => this.togglePreview(), "Ctrl+E");
    c("edit.find", "Rechercher dans la note", () => this.findInNote(), "Ctrl+F");
    document.addEventListener("click", (e) => {
      const id = (e.target as HTMLElement).dataset?.cmd;
      if (id) this.runCommand(id);
    });
  }

  runCommand(id: string): void {
    this.commands.find((c) => c.id === id)?.run();
  }

  private saveActive(): void {
    const d = this.activeDoc();
    if (!d) return;
    if (d.kind !== "md") {
      this.toast("Lecture seule : ce type de fichier ne s'enregistre pas dans Rocksy");
      return;
    }
    if (d.readOnly) {
      this.toast(`Lecture seule : ${this.readOnlyReason(d)}`);
      return;
    }
    if (d.conflict === "deleted") {
      this.toast("Fichier supprimé ou déplacé sur le disque : choisissez dans le bandeau");
      return;
    }
    if (d.conflict) {
      this.toast("Conflit : choisissez « Recharger » ou « Écraser » dans le bandeau");
      return;
    }
    void this.save(d);
  }

  /** Why a read-only document cannot be saved (`FileContent.readOnly`). */
  readOnlyReason(d: Doc): string {
    return isMarkdownPath(d.path)
      ? "fichier atteint par un lien depuis une note hors vault (Rocksy n'écrit que dans un vault ou dans un fichier ouvert par vous)"
      : "Rocksy n'enregistre que les fichiers .md et .markdown";
  }

  toggleAutosave(): void {
    this.autosave = !this.autosave;
    savePref("autosave", this.autosave ? "1" : "0");
    if (this.autosave) {
      for (const d of this.docs()) if (d.dirty && !d.saveError) this.sync.scheduleAutosave(d);
    } else {
      // A pending retry of a failed save is kept: that save was asked for.
      for (const d of this.docs()) if (!d.saveError) this.sync.cancelTimer(d);
    }
    this.updateStatus();
    this.toast(this.autosave ? "Enregistrement automatique activé" : "Enregistrement automatique désactivé");
  }

  private cycleTab(dir: number): void {
    const p = this.focused;
    if (p.tabs.length < 2 || !p.active) return;
    const i = (p.tabs.indexOf(p.active) + dir + p.tabs.length) % p.tabs.length;
    void this.activate(p.tabs[i]);
  }

  private findInNote(): void {
    const view = this.focused.view;
    if (!view) return;
    void import("@codemirror/search").then((m) => {
      view.focus();
      m.openSearchPanel(view);
    });
  }

  private onKey(e: KeyboardEvent): void {
    if (dialogOpen()) return;
    if (!(e.ctrlKey || e.metaKey) || e.altKey) {
      if (this.lat.active && !e.ctrlKey && !e.metaKey && (e.key.length === 1 || e.key === "Enter" || e.key === "Backspace")) this.lat.t0 = performance.now();
      return;
    }
    const k = e.key.toLowerCase();
    let cmd: string | null = null;
    if (k === "f" && e.shiftKey) cmd = "view.search";
    else if (k === "s") cmd = "file.save";
    else if (k === "o") cmd = "file.quickopen";
    else if (k === "p") cmd = "__palette";
    else if (k === "w") cmd = "tab.close";
    else if (k === "e") cmd = "view.preview";
    else if (k === "\\") cmd = "pane.split";
    else if (e.key === "Tab") cmd = e.shiftKey ? "__prev" : "tab.next";
    if (!cmd) return;
    e.preventDefault();
    e.stopPropagation();
    if (cmd === "__palette") void this.palette();
    else if (cmd === "__prev") this.cycleTab(-1);
    else this.runCommand(cmd);
  }

  // ------------------------------------------------------------ ribbon & status bar

  buildRibbon(): void {
    const rb = document.getElementById("ribbon")!;
    const btn = (id: string, icon: string, title: string, run: () => void) => {
      const b = document.createElement("button");
      b.id = id;
      b.title = title;
      b.innerHTML = iconSvg(icon, 18);
      b.addEventListener("click", run);
      rb.appendChild(b);
      return b;
    };
    btn("rb-sidebar", "sidebar", "Panneau latéral", () => this.toggleSidebar());
    btn("rb-files", "files", "Fichiers", () => void this.showSidebar("files"));
    btn("rb-search", "search", "Rechercher dans le vault (Ctrl+Maj+F)", () => void this.showSidebar("search"));
    btn("rb-tags", "tag", "Tags", () => void this.showSidebar("tags"));
    btn("rb-links", "link", "Liens de la note", () => void this.showSidebar("links"));
    btn("rb-open", "quick", "Ouvrir une note (Ctrl+O)", () => void this.quickOpen());
    btn("rb-palette", "command", "Palette de commandes (Ctrl+P)", () => void this.palette());
    const sp = document.createElement("div");
    sp.className = "spacer";
    rb.appendChild(sp);
    btn("rb-vault", "vault", "Vaults", () => void this.chooseVault());
    document.getElementById("rb-sidebar")?.classList.toggle("is-on", !this.sidebarEl.hidden);
  }

  private buildStatusBar(): void {
    const bar = document.getElementById("statusbar")!;
    const s = this.st;
    s.path.className = "st-path";
    s.toast.className = "st-toast";
    const spacer = document.createElement("span");
    spacer.className = "st-spacer";
    s.save.className = "st-save";
    s.auto.className = "st-auto";
    s.auto.title = "Enregistrement automatique (2 s après la dernière frappe)";
    s.auto.addEventListener("click", () => this.toggleAutosave());
    s.mode.title = "Basculer Live Preview / source (Ctrl+E)";
    s.mode.addEventListener("click", () => this.togglePreview());
    s.vault.title = "Changer de vault";
    s.vault.addEventListener("click", () => void this.chooseVault());
    bar.append(s.path, s.toast, spacer, s.tags, s.words, s.save, s.auto, s.mode, s.vault);
  }

  toast(msg: string): void {
    this.st.toast.textContent = msg;
    clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => (this.st.toast.textContent = ""), 4500);
  }

  private queueStatus(): void {
    clearTimeout(this.statusTimer);
    this.statusTimer = window.setTimeout(() => this.updateStatus(), 350);
  }

  private updateSaveState(): void {
    const d = this.activeDoc();
    const el = this.st.save;
    if (!d || d.kind !== "md" || !d.loaded) {
      el.textContent = d && d.kind !== "md" ? "Lecture seule" : "";
      el.className = "st-save";
      el.title = "";
      return;
    }
    const [text, cls] =
      d.conflict === "deleted"
        ? ["Fichier supprimé", "is-conflict"]
        : d.conflict
          ? ["Conflit", "is-conflict"]
          : d.readOnly
            ? ["Lecture seule", "is-readonly"]
            : d.saving
              ? ["Enregistrement…", "is-dirty"]
              : d.saveError === "io"
                ? ["Échec de l'enregistrement · nouvel essai", "is-conflict"]
                : d.error
                  ? ["Erreur d'enregistrement", "is-conflict"]
                  : d.dirty
                    ? ["Modifié", "is-dirty"]
                    : ["Enregistré", "is-saved"];
    el.textContent = text;
    el.className = "st-save " + cls;
    el.title = d.readOnly ? `Lecture seule : ${this.readOnlyReason(d)}` : d.error ? d.error : d.eolMixed ? "Fins de ligne mixtes : conservées telles quelles" : "";
  }

  updateStatus(): void {
    const d = this.activeDoc();
    const s = this.st;
    if (d) {
      const rel = this.vaultRoot ? toRel(this.vaultRoot, d.path) : null;
      s.path.textContent = rel ?? d.path;
      s.path.title = d.path;
    } else s.path.textContent = "";
    const state = d && d.kind === "md" ? this.stateOf(d) : null;
    if (state) {
      const text = state.doc.toString();
      let words = 0;
      const re = /[\p{L}\p{N}][\p{L}\p{N}'’_-]*/gu;
      while (re.exec(text)) words++;
      s.words.textContent = `${words.toLocaleString("fr-FR")} mots · ${text.length.toLocaleString("fr-FR")} car.`;
    } else s.words.textContent = "";
    this.updateSaveState();
    s.auto.textContent = this.autosave ? "Auto ✓" : "Auto ✗";
    s.auto.className = "st-auto" + (this.autosave ? " is-on" : "");
    s.mode.textContent = isPreviewOn() ? "Live Preview" : "Source";
    const v = this.vaultRoot ? (this.vaults.find((x) => samePath(x.path, this.vaultRoot!))?.name ?? basename(this.vaultRoot)) : "Aucun vault";
    s.vault.textContent = v;
    this.updateTagsStatus();
  }

  /** Global index indicator: reading progress, then what is still missing (links, search), then nothing. */
  updateTagsStatus(): void {
    const p = this.tagsProgress;
    const el = this.st.tags;
    const info = this.indexInfo;
    const n = (x: number) => x.toLocaleString("fr-FR");
    if (!this.vaultRoot) {
      el.textContent = "";
      el.title = "";
    } else if (p) {
      el.textContent = `Index : ${n(p.done)} / ${n(p.total)} notes`;
      el.title = "Lecture des notes nouvelles ou modifiées";
    } else if (!info) {
      el.textContent = this.tagsInfo ? "Index : tags ✓ · liens et recherche…" : this.tree ? "Index : en préparation…" : "";
      el.title = "Tags, liens et recherche plein texte du vault";
    } else {
      const pending = info.pending ?? 0;
      el.textContent = pending ? `Index : ${n(pending)} note${pending > 1 ? "s" : ""} à relire…` : info.searchReady ? (info.fromCache ? "Index : vérification…" : "Index ✓") : "Index : tags ✓ liens ✓ · recherche…";
      const retry = pending ? `${n(pending)} note${pending > 1 ? "s n'ont pas pu être lues" : " n'a pas pu être lue"} (fichier occupé ou indisponible) : nouvelle tentative automatique · ` : "";
      el.title = `${retry}${n(info.files)} notes · ${n(info.tags)} tags · ${n(info.links)} liens (${n(info.unresolvedLinks)} non résolus) · recherche : ${info.searchReady ? n(info.searchDocs) + " notes" : "en préparation"}`;
    }
  }

  get searchReady(): boolean {
    return !!this.indexInfo?.searchReady;
  }

  // ------------------------------------------------------------ session & events

  queueSession(): void {
    clearTimeout(this.sessionTimer);
    this.sessionTimer = window.setTimeout(() => {
      const session: Session = {
        activeVault: this.vaultRoot ?? this.dormantVault,
        tabs: this.docs().map((d) => d.path),
        activeTab: this.activeDoc()?.path ?? null,
      };
      void this.backend.setSession(session).catch(() => undefined);
    }, 200);
  }

  wireEvents(): void {
    const b = this.backend;
    b.on("open-files", (p) => void this.openFiles(p.paths));
    b.on("tree-updated", (tree) => {
      if (this.vaultRoot && samePath(tree.root, this.vaultRoot)) this.setTree(tree);
    });
    b.on("file-changed", (p) => this.onFileChanged(p.path, p.mtimeMs, p.deleted === true));
    b.on("close-requested", () => void this.requestClose());
    // Index events name their vault: the end of a pass on a vault that was just left must not touch this one.
    const mine = (p: { vault: string }) => isForVault(this.vaultRoot, p);
    b.on("tags-progress", (p) => {
      if (!mine(p)) return;
      this.tagsProgress = p;
      this.updateTagsStatus();
      this.sidebar?.onTagsProgress(p);
    });
    b.on("tags-ready", (p) => {
      if (!mine(p)) return;
      this.tagsProgress = null;
      this.tagsInfo = p;
      this.invalidateTags();
      this.updateTagsStatus();
      this.sidebar?.onTagsReady();
    });
    b.on("index-ready", (p) => {
      if (!mine(p)) return;
      // Only a pass knows what it could not read: the event that follows a save (links changed) keeps the count.
      if (p.pending == null && !p.fromCache && this.indexInfo?.pending) p = { ...p, pending: this.indexInfo.pending };
      this.indexInfo = p;
      this.invalidateTags();
      if (this.indexEvents.length < 50) this.indexEvents.push({ ...p, atMs: Math.round(performance.now()) });
      this.updateTagsStatus();
      this.sidebar?.onIndexReady();
    });
    b.onFileDrop((paths) => void this.openFiles(paths));
  }

  async openFiles(paths: string[]): Promise<void> {
    let last: Doc | null = null;
    for (const p of paths) {
      if (!this.kindOf(p)) {
        this.toast(`Type de fichier non pris en charge : ${basename(p)}`);
        continue;
      }
      last = this.addTab(p) ?? last;
    }
    if (last) {
      this.queueSession();
      await this.activate(last);
    }
  }

  /** Opens a note at a position (search result, backlink): scrolls, flashes the line, selects `len` units from `col`. */
  async openAt(path: string, line: number, col: number, len = 0): Promise<void> {
    const doc = await this.openPath(path, { activate: false });
    if (!doc) return;
    await this.activate(doc);
    // Positions follow the exact text of read_file, BOM included; the editor text has no BOM.
    const fix = doc.bom && line === 0 ? 1 : 0;
    const view = this.viewOf(doc);
    if (view) revealLines(view, line, line, Math.max(0, col - fix), len);
  }

  /** Terms of the last vault search, highlighted in the open notes (empty list: no highlight). */
  setSearchTerms(terms: readonly string[]): void {
    this.searchTerms = terms;
    this.applySearchTerms();
  }

  private applySearchTerms(): void {
    for (const p of this.panes) {
      const v = p.view;
      if (!v || p.active?.kind !== "md" || p.editorHost.hidden) continue;
      const cur = v.state.field(termsField, false);
      if (cur === undefined || cur === this.searchTerms || (cur.length === 0 && this.searchTerms.length === 0)) continue;
      v.dispatch({ effects: setTermsEffect.of(this.searchTerms) });
    }
  }

  /** Palette-like list of link targets that do not exist in the vault; picking one opens its first source. */
  async showUnresolved(): Promise<void> {
    if (!this.vaultRoot) {
      this.toast("Aucun vault ouvert");
      return;
    }
    if (this.pickerOpen) return;
    this.pickerOpen = true;
    const root = this.vaultRoot;
    let list: Awaited<ReturnType<Backend["unresolvedLinks"]>> = [];
    try {
      list = await this.backend.unresolvedLinks(UNRESOLVED_LIMIT);
    } catch (e) {
      this.toast(`Liens non résolus indisponibles : ${toApiError(e).message}`);
    }
    const { openPicker } = await this.picker();
    const items: PickerItem[] = list.map((u) => ({
      ...makeItem(u.target),
      icon: "unlink",
      detail: `${u.count} lien${u.count > 1 ? "s" : ""} · ${u.sources.length ? (toRel(root, u.sources[0]) ?? u.sources[0]).replace(/\.md$/i, "") : ""}${u.sources.length > 1 ? ` (+${u.sources.length - 1})` : ""}`,
      run: () => void this.openUnresolvedSource(u.target, u.sources[0]),
    }));
    openPicker({
      placeholder: `Liens non résolus (${list.length >= UNRESOLVED_LIMIT ? "les " + UNRESOLVED_LIMIT.toLocaleString("fr-FR") + " cibles les plus citées" : list.length.toLocaleString("fr-FR") + " cibles"})…`,
      search: (q) => fuzzySearch(q, items, 100),
      emptyText: this.indexInfo ? "Aucun lien non résolu" : "Index en préparation…",
      footer: "Entrée : ouvrir la première note source",
      onClose: () => {
        this.pickerOpen = false;
        this.focused.focusEditor();
      },
    });
  }

  private async openUnresolvedSource(target: string, source: string | undefined): Promise<void> {
    if (!source) return;
    let line = -1;
    let col = 0;
    try {
      const out = await this.backend.outgoingLinks(source);
      const hit = out.find((o) => !o.resolved && o.target.toLowerCase() === target.toLowerCase());
      if (hit) {
        line = hit.line;
        col = hit.col;
      }
    } catch {
      /* open the note anyway */
    }
    if (line >= 0) await this.openAt(source, line, col);
    else await this.openPath(source);
  }

  /** Opens a tag fragment: scrolls to its block and flashes it. */
  async openFragment(f: Fragment): Promise<void> {
    await this.openPath(f.path, { lines: { start: f.startLine, end: f.endLine, col: 0 } });
  }

  // ------------------------------------------------------------ test hooks

  /** Times the quick-switcher filter over the current tree for a list of queries (ms per query). */
  benchQuickOpen(queries: string[]): { items: number; ms: number[]; hits: number[] } {
    const all = this.quickItemsList();
    const ms: number[] = [];
    const hits: number[] = [];
    for (const q of queries) {
      const t0 = performance.now();
      hits.push(fuzzySearch(q, all, 50).length);
      ms.push(Math.round((performance.now() - t0) * 100) / 100);
    }
    return { items: all.length, ms, hits };
  }

  /** Renders one Mermaid source with Pele and reports the time (test hook; same lazy chunk as the editor widget). */
  async renderMermaid(src: string): Promise<{ ok: boolean; ms: number; type?: string; width?: number; error?: string }> {
    const m = await import("pele/lazy");
    const t0 = performance.now();
    try {
      const r = await m.renderAsync(src, { images: false, links: false, maxWidth: 740 });
      return { ok: true, ms: Math.round((performance.now() - t0) * 100) / 100, type: r.mermaidType, width: Math.round(r.width) };
    } catch (e) {
      return { ok: false, ms: Math.round((performance.now() - t0) * 100) / 100, error: String((e as Error)?.message ?? e).slice(0, 200) };
    }
  }

  latencyStart(): void {
    this.lat = { active: true, t0: 0, update: [], layout: [] };
  }

  latencyStop(): { n: number; update: { median: number; p95: number; max: number }; withLayout: { median: number; p95: number; max: number } } {
    this.lat.active = false;
    const stat = (a: number[]) => {
      const s = [...a].sort((x, y) => x - y);
      const q = (p: number) => (s.length ? Math.round(s[Math.min(s.length - 1, Math.floor(p * s.length))] * 100) / 100 : 0);
      return { median: q(0.5), p95: q(0.95), max: q(1) };
    };
    return { n: this.lat.update.length, update: stat(this.lat.update), withLayout: stat(this.lat.layout) };
  }
}

function decodeSafe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}
