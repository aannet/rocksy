// Links of the active note (sidebar): incoming links grouped by source note, outgoing links in text order.
// `backlinks` / `outgoing_links` are only called while this panel is visible.
import type { App } from "./app";
import type { LinkRef, OutLink } from "./types";
import { basename, dirname, toRel } from "./paths";
import { escapeHtml, groupByPath, segmentsHtml } from "./excerpt";
import { iconSvg } from "./icons";

export class LinksPanel {
  readonly body = document.createElement("div");
  private head = document.createElement("div");
  private content = document.createElement("div");
  private req = 0;
  private back: LinkRef[] = [];
  private out: OutLink[] = [];
  private folded = { in: false, out: false };
  private shownPath: string | null = null;
  /** What the last completed request was made for: vault, note, `linksGeneration`. */
  private shownKey: string | null = null;
  /** Signature of what is in the DOM (results and display state). */
  private shownSig: string | null = null;
  private dataSig = "";

  constructor(private app: App) {
    this.body.className = "sb-body links-panel";
    this.body.hidden = true;
    this.head.className = "links-head";
    this.content.className = "links-content";
    this.body.append(this.head, this.content);
    try {
      this.folded.in = localStorage.getItem("rocksy.linksIn") === "0";
      this.folded.out = localStorage.getItem("rocksy.linksOut") === "0";
    } catch {
      /* ignore */
    }
    this.content.addEventListener("mousedown", (e) => e.preventDefault());
    this.content.addEventListener("click", (e) => {
      const t = e.target as HTMLElement;
      const sec = t.closest(".links-sec") as HTMLElement | null;
      if (sec) {
        const k = sec.dataset.sec as "in" | "out";
        this.folded[k] = !this.folded[k];
        try {
          localStorage.setItem(k === "in" ? "rocksy.linksIn" : "rocksy.linksOut", this.folded[k] ? "0" : "1");
        } catch {
          /* ignore */
        }
        this.render();
        return;
      }
      const b = t.closest("[data-b]") as HTMLElement | null;
      if (b) {
        const ref = this.back[Number(b.dataset.b)];
        if (ref) void this.app.openAt(ref.path, ref.line, ref.col);
        return;
      }
      const o = t.closest("[data-o]") as HTMLElement | null;
      if (o) {
        const link = this.out[Number(o.dataset.o)];
        if (link?.resolved) void this.app.openPath(link.resolved, { heading: link.heading });
        else if (link) this.app.toast(`Note introuvable : ${link.target}`);
      }
    });
  }

  private message(text: string): void {
    this.back = [];
    this.out = [];
    this.shownKey = null;
    this.shownPath = null;
    const html = `<div class="search-msg">${text}</div>`;
    if (this.shownSig !== "msg:" + html) {
      this.shownSig = "msg:" + html;
      this.content.innerHTML = html;
    }
  }

  private setHead(html: string): void {
    if (this.head.dataset.h !== html) {
      this.head.dataset.h = html;
      this.head.innerHTML = html;
    }
  }

  /**
   * Shows the links of the active note (tab change, index-ready, panel shown). Nothing is asked again while the
   * vault, the note and `linksGeneration` are the ones already shown; `force` asks anyway. A save that
   * changes links is followed by an `index-ready` with a new generation. The DOM is only replaced when the result differs.
   */
  async refresh(force = false): Promise<void> {
    const doc = this.app.activeDoc();
    const root = this.app.vaultRoot;
    this.setHead(doc ? `${iconSvg("link", 14)}<span>${escapeHtml(doc.name)}</span>` : "");
    if (!root || !doc || doc.outside) {
      this.req++;
      this.content.classList.remove("is-loading");
      return this.message(!root ? "Aucun vault ouvert : les liens sont calculés dans un vault." : !doc ? "Aucune note active." : "Fichier hors vault : ses liens ne sont pas indexés.");
    }
    const info = this.app.indexInfo;
    const key = `${root}\n${doc.path}\n${info ? info.linksGeneration : "-"}`;
    if (!force && key === this.shownKey) {
      this.app.metrics.linksSkipped = (this.app.metrics.linksSkipped ?? 0) + 1;
      return;
    }
    const req = ++this.req;
    if (this.shownPath !== doc.path) this.content.classList.add("is-loading");
    try {
      const [back, out] = await Promise.all([this.app.backend.backlinks(doc.path), doc.kind === "md" ? this.app.backend.outgoingLinks(doc.path) : Promise.resolve([] as OutLink[])]);
      if (req !== this.req) return;
      this.back = back;
      this.out = out;
      this.dataSig = JSON.stringify([back, out]);
      this.shownPath = doc.path;
      this.shownKey = key;
      this.app.metrics.backlinks = back.length;
      this.app.metrics.outgoing = out.length;
      this.app.metrics.linksRequests = (this.app.metrics.linksRequests ?? 0) + 1;
      this.render();
    } catch (e) {
      if (req !== this.req) return;
      this.message(`Liens indisponibles : ${escapeHtml(String((e as { message?: string })?.message ?? e))}`);
    } finally {
      if (req === this.req) this.content.classList.remove("is-loading");
    }
  }

  private render(): void {
    const root = this.app.vaultRoot ?? "";
    const doc = this.app.activeDoc();
    const preparing = !this.app.indexInfo;
    // Identical results in the same display state: the DOM is left alone.
    const sig = `${root}|${doc?.kind}|${preparing}|${this.folded.in}|${this.folded.out}|${this.dataSig}`;
    if (sig === this.shownSig) return;
    this.shownSig = sig;
    const rel = (p: string) => toRel(root, p) ?? p;
    const unresolved = this.out.filter((o) => !o.resolved).length;
    let html = `<div class="links-sec${this.folded.in ? "" : " is-open"}" data-sec="in"><span class="chev">${iconSvg("chevron", 14)}</span><span>Liens entrants</span><span class="tc">${this.back.length}</span></div>`;
    if (!this.folded.in) {
      if (!this.back.length) html += `<div class="links-empty">${preparing ? "Index en préparation…" : "Aucune note ne pointe vers celle-ci."}</div>`;
      let i = 0;
      for (const g of groupByPath(this.back)) {
        const r = rel(g.path);
        const dir = dirname(r);
        html += `<div class="sr-file" data-b="${i}" title="${escapeHtml(r)}"><span class="sr-name">${escapeHtml(basename(r).replace(/\.md$/i, ""))}</span><span class="sr-count">${g.items.length}</span>${dir ? `<div class="sr-dir">${escapeHtml(dir)}</div>` : ""}</div>`;
        for (const ref of g.items) {
          html += `<div class="sr-match" data-b="${i}"><span class="sr-line">${ref.line + 1}</span><span class="sr-text">${segmentsHtml(ref.excerpt, ref.ranges)}${ref.kind === "embed" ? ' <span class="lk-kind">embed</span>' : ""}</span></div>`;
          i++;
        }
      }
    }
    if (doc?.kind === "md") {
      html += `<div class="links-sec${this.folded.out ? "" : " is-open"}" data-sec="out"><span class="chev">${iconSvg("chevron", 14)}</span><span>Liens sortants</span>${unresolved ? `<span class="lk-missing">${unresolved} non résolu${unresolved > 1 ? "s" : ""}</span>` : ""}<span class="tc">${this.out.length}</span></div>`;
      if (!this.folded.out) {
        if (!this.out.length) html += `<div class="links-empty">${preparing ? "Index en préparation…" : "Cette note ne contient aucun lien."}</div>`;
        this.out.forEach((o, i) => {
          const label = o.target + (o.heading ? " > " + o.heading : "");
          const where = o.resolved ? dirname(rel(o.resolved)) : "non résolu";
          html += `<div class="lk-out${o.resolved ? "" : " is-missing"}" data-o="${i}" title="${escapeHtml(o.resolved ? rel(o.resolved) : "Cible introuvable dans le vault")}"><span class="lk-ico">${iconSvg(o.kind === "embed" ? "image" : o.resolved ? "link" : "unlink", 13)}</span><span class="lk-label">${escapeHtml(label)}${o.alias ? `<span class="lk-alias"> · ${escapeHtml(o.alias)}</span>` : ""}</span><span class="lk-where">${escapeHtml(where)}</span></div>`;
        });
      }
    }
    this.content.innerHTML = html;
    this.app.metrics.linksRenders = (this.app.metrics.linksRenders ?? 0) + 1;
  }
}
