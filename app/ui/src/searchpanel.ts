// Full-text search panel (sidebar). Calls `search` ~80 ms after the last keystroke; late answers are dropped.
import type { App } from "./app";
import type { SearchResult } from "./types";
import { basename, dirname, toRel } from "./paths";
import { escapeHtml, highlightTerms, segmentsHtml } from "./excerpt";
import { iconSvg } from "./icons";

const DEBOUNCE_MS = 80;
const LIMIT = 50;

interface Row {
  path: string;
  line: number;
  col: number;
  len: number;
}

export class SearchPanel {
  readonly body = document.createElement("div");
  readonly input = document.createElement("input");
  private info = document.createElement("div");
  private list = document.createElement("div");
  private hl = document.createElement("input");
  private rows: Row[] = [];
  private sel = -1;
  private req = 0;
  private timer = 0;
  private typedAt = 0;
  private lastQuery = "";
  private waiting = false;
  /** Keystroke → results painted, one sample per completed search (test hook). */
  samples: { total: number; call: number; backend: number; files: number }[] = [];

  constructor(private app: App) {
    this.body.className = "sb-body search-panel";
    this.body.hidden = true;
    const box = document.createElement("label");
    box.className = "tags-search";
    box.innerHTML = iconSvg("search", 14);
    this.input.type = "text";
    this.input.placeholder = "Rechercher dans le vault…";
    this.input.spellcheck = false;
    this.input.title = 'Syntaxe : mots (tous requis), "expression exacte", -mot, path:dossier, tag:projet/alpha';
    box.appendChild(this.input);
    const opts = document.createElement("label");
    opts.className = "search-opts";
    this.hl.type = "checkbox";
    this.hl.checked = this.pref();
    opts.append(this.hl, " Surligner les termes dans la note");
    this.info.className = "search-info";
    this.list.className = "search-list";
    this.body.append(box, opts, this.info, this.list);

    this.input.addEventListener("input", () => {
      this.typedAt = performance.now();
      clearTimeout(this.timer);
      this.timer = window.setTimeout(() => void this.run(), DEBOUNCE_MS);
    });
    this.input.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        this.select(Math.min(this.rows.length - 1, this.sel + 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        this.select(Math.max(-1, this.sel - 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        this.open(this.sel < 0 ? 0 : this.sel);
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.clear();
      }
    });
    this.hl.addEventListener("change", () => {
      try {
        localStorage.setItem("rocksy.searchHl", this.hl.checked ? "1" : "0");
      } catch {
        /* ignore */
      }
      this.app.setSearchTerms(this.hl.checked ? highlightTerms(this.input.value) : []);
    });
    this.list.addEventListener("mousedown", (e) => e.preventDefault());
    this.list.addEventListener("click", (e) => {
      const el = (e.target as HTMLElement).closest("[data-r]") as HTMLElement | null;
      if (el) this.open(Number(el.dataset.r));
    });
    this.render(null);
  }

  private pref(): boolean {
    try {
      return localStorage.getItem("rocksy.searchHl") !== "0";
    } catch {
      return true;
    }
  }

  /** Called when the panel becomes visible. */
  show(focus = true): void {
    this.input.disabled = !this.app.vaultRoot;
    if (!this.lastQuery && !this.input.value) this.render(null);
    if (focus && this.app.vaultRoot) {
      this.input.focus();
      this.input.select();
    }
  }

  /** Vault changed or closed. */
  reset(): void {
    this.req++;
    this.input.value = "";
    this.lastQuery = "";
    this.waiting = false;
    this.input.disabled = !this.app.vaultRoot;
    this.render(null);
  }

  /** `index-ready`: a query typed while the search index was not ready is replayed. */
  onIndex(): void {
    if (this.waiting && this.app.searchReady) void this.run();
    else if (!this.input.value) this.render(null);
  }

  clear(): void {
    this.req++;
    clearTimeout(this.timer);
    this.input.value = "";
    this.lastQuery = "";
    this.waiting = false;
    this.app.setSearchTerms([]);
    this.render(null);
  }

  async run(): Promise<void> {
    const query = this.input.value.trim();
    const req = ++this.req;
    this.lastQuery = query;
    if (!query || !this.app.vaultRoot) {
      this.waiting = false;
      this.app.setSearchTerms([]);
      this.render(null);
      return;
    }
    const t0 = performance.now();
    let res: SearchResult;
    try {
      res = await this.app.backend.search(query, LIMIT);
    } catch (e) {
      if (req !== this.req) return;
      this.rows = [];
      this.info.textContent = "";
      this.list.innerHTML = `<div class="search-msg">Recherche impossible : ${escapeHtml(String((e as { message?: string })?.message ?? e))}</div>`;
      return;
    }
    if (req !== this.req) return; // a newer query was sent meanwhile
    const call = performance.now() - t0;
    this.waiting = !res.searchReady;
    this.render(res);
    void this.list.offsetHeight;
    const now = performance.now();
    const sample = { total: Math.round((now - (this.typedAt || t0)) * 10) / 10, call: Math.round(call * 10) / 10, backend: res.ms, files: res.total };
    this.samples.push(sample);
    this.app.metrics.searchTotalMs = sample.total;
    this.app.metrics.searchCallMs = sample.call;
    this.app.metrics.searchFiles = res.total;
    if (this.hl.checked) this.app.setSearchTerms(res.searchReady ? highlightTerms(query) : []);
  }

  private render(res: SearchResult | null): void {
    this.rows = [];
    this.sel = -1;
    const root = this.app.vaultRoot;
    if (!root) {
      this.info.textContent = "";
      this.list.innerHTML = `<div class="search-msg">Aucun vault ouvert : la recherche porte sur les notes d'un vault.</div>`;
      return;
    }
    if (!res) {
      this.info.textContent = "";
      const prep = this.app.searchReady ? "" : `<div class="search-msg is-wait">Index de recherche en préparation…</div>`;
      this.list.innerHTML = `${prep}<div class="search-help"><div>Tous les mots sont requis ; le dernier mot est un début de mot.</div><div><code>"expression exacte"</code></div><div><code>-mot</code> pour exclure</div><div><code>path:dossier</code> pour restreindre aux chemins</div><div><code>tag:projet/alpha</code> pour restreindre à un tag</div></div>`;
      return;
    }
    if (!res.searchReady) {
      this.info.textContent = "";
      this.list.innerHTML = `<div class="search-msg is-wait">Index de recherche en préparation… La recherche se lancera dès qu'il sera prêt.</div>`;
      return;
    }
    const shown = res.hits.length;
    const n = res.total;
    this.info.textContent = `${n.toLocaleString("fr-FR")} fichier${n > 1 ? "s" : ""} · ${(res.ms < 10 ? Math.round(res.ms * 10) / 10 : Math.round(res.ms)).toLocaleString("fr-FR")} ms${n > shown ? ` · ${shown} premiers affichés` : ""}`;
    if (!shown) {
      this.list.innerHTML = `<div class="search-msg">Aucun résultat pour « ${escapeHtml(res.query)} »</div>`;
      return;
    }
    let html = "";
    for (const hit of res.hits) {
      const rel = toRel(root, hit.path) ?? hit.path;
      const name = basename(rel).replace(/\.md$/i, "");
      const dir = dirname(rel);
      const first = hit.matches[0];
      const r = this.rows.push({ path: hit.path, line: first ? first.line : 0, col: first ? first.col : 0, len: first ? (first.len ?? 0) : 0 }) - 1;
      html += `<div class="sr-file" data-r="${r}" title="${escapeHtml(rel)}"><span class="sr-name${hit.nameMatch ? " is-name" : ""}">${escapeHtml(name)}</span>${hit.matchCount ? `<span class="sr-count">${hit.matchCount.toLocaleString("fr-FR")}</span>` : ""}${dir ? `<div class="sr-dir">${escapeHtml(dir)}</div>` : ""}</div>`;
      for (const m of hit.matches.slice(0, 3)) {
        const i = this.rows.push({ path: hit.path, line: m.line, col: m.col, len: m.len ?? 0 }) - 1;
        html += `<div class="sr-match" data-r="${i}"><span class="sr-line">${m.line + 1}</span><span class="sr-text">${segmentsHtml(m.excerpt, m.ranges)}</span></div>`;
      }
    }
    this.list.innerHTML = html;
    this.list.scrollTop = 0;
  }

  private select(i: number): void {
    this.list.querySelector(".is-sel")?.classList.remove("is-sel");
    this.sel = i;
    if (i < 0) {
      this.list.scrollTop = 0;
      return;
    }
    const el = this.list.querySelector(`[data-r="${i}"]`) as HTMLElement | null;
    if (el) {
      el.classList.add("is-sel");
      el.scrollIntoView({ block: "nearest" });
    }
  }

  private open(i: number): void {
    const row = this.rows[i];
    if (!row) return;
    this.select(i);
    void this.app.openAt(row.path, row.line, row.col, row.len);
  }
}
