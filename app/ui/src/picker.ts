// Modal fuzzy picker shared by the quick switcher (Ctrl+O), the command palette (Ctrl+P) and the vault selector.
import type { FuzzyHit, FuzzyItem } from "./fuzzy";
import { iconSvg } from "./icons";

export interface PickerItem extends FuzzyItem {
  icon?: string;
  detail?: string;
  run(): void;
}

export interface PickerOptions {
  placeholder: string;
  search(query: string): FuzzyHit<PickerItem>[];
  emptyText: string;
  /** Show the basename as the main label and the folder as detail (file paths). */
  split?: boolean;
  footer?: string;
  onEmptyEnter?: () => void;
  onClose?: () => void;
  onRender?: (ms: number) => void;
}

function esc(s: string): string {
  return s.replace(/[&<>]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"));
}

function highlight(text: string, positions: number[], offset: number): string {
  let out = "";
  let pi = 0;
  while (pi < positions.length && positions[pi] < offset) pi++;
  let open = false;
  for (let i = 0; i < text.length; i++) {
    const hit = pi < positions.length && positions[pi] === i + offset;
    if (hit) pi++;
    if (hit && !open) {
      out += "<b>";
      open = true;
    } else if (!hit && open) {
      out += "</b>";
      open = false;
    }
    const c = text[i];
    out += c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : c;
  }
  return open ? out + "</b>" : out;
}

export function openPicker(opts: PickerOptions): void {
  const backdrop = document.createElement("div");
  backdrop.className = "picker-backdrop";
  backdrop.innerHTML = `<div class="picker" role="dialog"><div class="picker-input">${iconSvg("search", 16)}<input type="text" spellcheck="false" autocomplete="off" /></div><div class="picker-list"></div><div class="picker-foot"><span><kbd>↑</kbd> <kbd>↓</kbd> naviguer</span><span><kbd>Entrée</kbd> ouvrir</span><span><kbd>Échap</kbd> fermer</span><span></span></div></div>`;
  const input = backdrop.querySelector("input")!;
  const list = backdrop.querySelector(".picker-list") as HTMLElement;
  const foot = backdrop.querySelector(".picker-foot span:last-child") as HTMLElement;
  input.placeholder = opts.placeholder;
  let hits: FuzzyHit<PickerItem>[] = [];
  let sel = 0;
  let closed = false;

  const close = () => {
    if (closed) return;
    closed = true;
    backdrop.remove();
    opts.onClose?.();
  };
  const paintSel = () => {
    const prev = list.querySelector(".is-sel");
    prev?.classList.remove("is-sel");
    const el = list.children[sel] as HTMLElement | undefined;
    if (el && el.classList.contains("picker-item")) {
      el.classList.add("is-sel");
      el.scrollIntoView({ block: "nearest" });
    }
  };
  const render = () => {
    const t0 = performance.now();
    hits = opts.search(input.value);
    sel = 0;
    if (!hits.length) {
      list.innerHTML = `<div class="picker-empty">${esc(opts.emptyText)}</div>`;
    } else {
      let html = "";
      for (let i = 0; i < hits.length; i++) {
        const { item, positions } = hits[i];
        let main: string;
        let detail: string;
        if (opts.split) {
          main = highlight(item.text.slice(item.baseStart), positions, item.baseStart);
          detail = item.baseStart > 0 ? highlight(item.text.slice(0, item.baseStart - 1), positions, 0) : "";
        } else {
          main = highlight(item.text, positions, 0);
          detail = esc(item.detail ?? "");
        }
        html += `<div class="picker-item${i === 0 ? " is-sel" : ""}" data-i="${i}"><span class="pi-icon">${iconSvg(item.icon ?? "file", 15)}</span><span class="pi-main">${main}</span><span class="pi-detail"><bdi>${detail}</bdi></span></div>`;
      }
      list.innerHTML = html;
    }
    list.scrollTop = 0;
    foot.textContent = opts.footer ?? "";
    opts.onRender?.(Math.round((performance.now() - t0) * 100) / 100);
  };
  const run = (i: number) => {
    const hit = hits[i];
    close();
    if (hit) hit.item.run();
    else opts.onEmptyEnter?.();
  };

  input.addEventListener("input", render);
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
      e.preventDefault();
      if (hits.length) sel = (sel + 1) % hits.length;
      paintSel();
    } else if (e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey)) {
      e.preventDefault();
      if (hits.length) sel = (sel - 1 + hits.length) % hits.length;
      paintSel();
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(sel);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  });
  list.addEventListener("mousedown", (e) => e.preventDefault());
  list.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest(".picker-item") as HTMLElement | null;
    if (el) run(Number(el.dataset.i));
  });
  backdrop.addEventListener("mousedown", (e) => {
    if (e.target === backdrop) close();
  });
  document.body.appendChild(backdrop);
  input.focus();
  render();
}
