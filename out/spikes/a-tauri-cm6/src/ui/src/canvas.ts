// Read-only viewer for JSON Canvas files (F6). Never writes anything: there is no save path here.
import { renderInlineHtml } from "./editor/syntax";
import { iconSvg } from "./icons";
import { isImagePath } from "./paths";

interface CNode {
  id: string;
  type: string;
  x: number;
  y: number;
  width: number;
  height: number;
  text?: string;
  file?: string;
  url?: string;
  label?: string;
  color?: string;
}
interface CEdge {
  id: string;
  fromNode: string;
  toNode: string;
  fromSide?: string;
  toSide?: string;
  toEnd?: string;
  fromEnd?: string;
  label?: string;
  color?: string;
}

export interface CanvasHost {
  assetUrl(rel: string): string;
  open(rel: string): void;
  readText(rel: string): Promise<string | null>;
}

const PRESET: Record<string, string> = { "1": "#f0717f", "2": "#f0a04b", "3": "#e8c454", "4": "#6fcf79", "5": "#53c8e0", "6": "#b28cf5" };
const colorOf = (c: string | undefined, fallback: string) => (c ? (PRESET[c] ?? (/^#[0-9a-f]{3,8}$/i.test(c) ? c : fallback)) : fallback);

/** Minimal block Markdown for text cards: headings, lists, paragraphs (inline via renderInlineHtml). */
function cardHtml(text: string): string {
  let html = "";
  let inList = false;
  for (const line of text.split("\n")) {
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    const li = /^\s*[-*+]\s+(.*)$/.exec(line);
    if (li) {
      if (!inList) html += "<ul>";
      inList = true;
      html += `<li>${renderInlineHtml(li[1])}</li>`;
      continue;
    }
    if (inList) {
      html += "</ul>";
      inList = false;
    }
    if (h) html += `<h${Math.min(3, h[1].length)}>${renderInlineHtml(h[2])}</h${Math.min(3, h[1].length)}>`;
    else if (line.trim()) html += `<p>${renderInlineHtml(line)}</p>`;
  }
  return html + (inList ? "</ul>" : "");
}

function anchor(n: CNode, side: string | undefined, toward: CNode): [number, number, number, number] {
  let s = side;
  if (!s) {
    const dx = toward.x + toward.width / 2 - (n.x + n.width / 2);
    const dy = toward.y + toward.height / 2 - (n.y + n.height / 2);
    s = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "bottom" : "top";
  }
  if (s === "top") return [n.x + n.width / 2, n.y, 0, -1];
  if (s === "bottom") return [n.x + n.width / 2, n.y + n.height, 0, 1];
  if (s === "left") return [n.x, n.y + n.height / 2, -1, 0];
  return [n.x + n.width, n.y + n.height / 2, 1, 0];
}

export function mountCanvas(host: HTMLElement, json: string, api: CanvasHost): () => void {
  let nodes: CNode[] = [];
  let edges: CEdge[] = [];
  try {
    const data = JSON.parse(json || "{}");
    nodes = Array.isArray(data.nodes) ? data.nodes : [];
    edges = Array.isArray(data.edges) ? data.edges : [];
  } catch (e) {
    host.innerHTML = `<div class="empty is-error"><h2>Canvas illisible</h2><div>${String((e as Error).message).replace(/</g, "&lt;")}</div></div>`;
    return () => undefined;
  }
  const world = document.createElement("div");
  world.className = "canvas-world";
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const esc = (s: string) => s.replace(/[&<>"]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : "&quot;"));

  // Groups first (behind), then cards.
  const ordered = [...nodes].sort((a, b) => (a.type === "group" ? 0 : 1) - (b.type === "group" ? 0 : 1));
  for (const n of ordered) {
    const el = document.createElement("div");
    el.style.cssText = `left:${n.x}px;top:${n.y}px;width:${n.width}px;height:${n.height}px`;
    const col = colorOf(n.color, "");
    if (col) el.style.borderColor = col;
    if (n.type === "group") {
      el.className = "cv-group";
      if (col) el.style.background = col + "14";
      if (n.label) el.innerHTML = `<div class="cv-group-label">${esc(n.label)}</div>`;
    } else if (n.type === "file" && n.file) {
      const file = n.file;
      if (isImagePath(file)) {
        el.className = "cv-node cv-image";
        const img = document.createElement("img");
        img.src = api.assetUrl(file);
        img.alt = file;
        img.draggable = false;
        el.appendChild(img);
      } else {
        el.className = "cv-node cv-file";
        const name = file.slice(file.lastIndexOf("/") + 1).replace(/\.md$/i, "");
        el.innerHTML = `<div class="cv-title" data-open="${esc(file)}">${iconSvg("file", 14)}<span>${esc(name)}</span></div><div class="cv-excerpt"></div>`;
        if (/\.md$/i.test(file)) {
          void api.readText(file).then((t) => {
            const ex = el.querySelector(".cv-excerpt");
            if (ex && t) ex.textContent = t.replace(/^---\n[\s\S]*?\n---\n/, "").trim().slice(0, 500);
          });
        }
      }
    } else if (n.type === "link") {
      el.className = "cv-node cv-link";
      el.innerHTML = `<div class="cv-body">${iconSvg("external", 14)}<span>${esc(n.url ?? "")}</span></div>`;
    } else {
      el.className = "cv-node cv-text";
      el.innerHTML = `<div class="cv-body">${cardHtml(n.text ?? "")}</div>`;
    }
    world.appendChild(el);
  }

  let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><defs><marker id="cv-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 10 5 0 10z" fill="context-stroke"/></marker></defs>`;
  for (const e of edges) {
    const a = byId.get(e.fromNode);
    const b = byId.get(e.toNode);
    if (!a || !b) continue;
    const [x1, y1, dx1, dy1] = anchor(a, e.fromSide, b);
    const [x2, y2, dx2, dy2] = anchor(b, e.toSide, a);
    const k = Math.max(40, Math.hypot(x2 - x1, y2 - y1) * 0.4);
    const col = colorOf(e.color, "#6b7a99");
    const end = e.toEnd === "none" ? "" : ' marker-end="url(#cv-arrow)"';
    const startM = e.fromEnd === "arrow" ? ' marker-start="url(#cv-arrow)"' : "";
    svg += `<path d="M${x1} ${y1} C${x1 + dx1 * k} ${y1 + dy1 * k} ${x2 + dx2 * k} ${y2 + dy2 * k} ${x2} ${y2}" fill="none" stroke="${col}" stroke-width="2.2"${end}${startM}/>`;
    if (e.label) svg += `<text class="cv-edge-label" x="${(x1 + x2) / 2}" y="${(y1 + y2) / 2 - 6}" text-anchor="middle">${esc(e.label)}</text>`;
  }
  world.insertAdjacentHTML("afterbegin", svg + "</svg>");

  const hud = document.createElement("div");
  hud.className = "cv-hud";
  hud.innerHTML = `<button data-z="out" title="Zoom arrière">−</button><span class="cv-zoom">100 %</span><button data-z="in" title="Zoom avant">+</button><button data-z="fit" title="Ajuster">⤢</button>`;
  const badge = document.createElement("div");
  badge.className = "cv-badge";
  badge.textContent = `Canvas en lecture seule · ${nodes.length} nœuds · ${edges.length} liens`;
  host.append(world, hud, badge);

  let scale = 1;
  let tx = 0;
  let ty = 0;
  let settle = 0;
  const apply = () => {
    // Composite the world as one layer while zooming/panning (cheap frames), re-rasterise crisply once idle.
    world.style.willChange = "transform";
    clearTimeout(settle);
    settle = window.setTimeout(() => (world.style.willChange = ""), 250);
    world.style.transform = `translate(${tx}px,${ty}px) scale(${scale})`;
    host.style.backgroundSize = `${24 * scale}px ${24 * scale}px`;
    host.style.backgroundPosition = `${tx}px ${ty}px`;
    (hud.querySelector(".cv-zoom") as HTMLElement).textContent = Math.round(scale * 100) + " %";
    host.dataset.scale = scale.toFixed(3);
    host.dataset.tx = String(Math.round(tx));
  };
  const fit = () => {
    if (!nodes.length) return apply();
    let x0 = Infinity;
    let y0 = Infinity;
    let x1 = -Infinity;
    let y1 = -Infinity;
    for (const n of nodes) {
      x0 = Math.min(x0, n.x);
      y0 = Math.min(y0, n.y - (n.type === "group" ? 30 : 0));
      x1 = Math.max(x1, n.x + n.width);
      y1 = Math.max(y1, n.y + n.height);
    }
    const w = host.clientWidth || 800;
    const h = host.clientHeight || 600;
    scale = Math.min(1.2, Math.min((w - 80) / (x1 - x0), (h - 80) / (y1 - y0)));
    tx = (w - (x1 - x0) * scale) / 2 - x0 * scale;
    ty = (h - (y1 - y0) * scale) / 2 - y0 * scale;
    apply();
  };
  const zoomAt = (cx: number, cy: number, factor: number) => {
    const next = Math.min(4, Math.max(0.08, scale * factor));
    const f = next / scale;
    tx = cx - (cx - tx) * f;
    ty = cy - (cy - ty) * f;
    scale = next;
    apply();
  };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const r = host.getBoundingClientRect();
    zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-e.deltaY * 0.0015));
  };
  let drag: { x: number; y: number; moved: boolean } | null = null;
  const onDown = (e: MouseEvent) => {
    if (e.button > 1 || (e.target as HTMLElement).closest(".cv-hud")) return;
    drag = { x: e.clientX, y: e.clientY, moved: false };
    host.classList.add("is-panning");
    e.preventDefault();
  };
  const onMove = (e: MouseEvent) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    tx += dx;
    ty += dy;
    drag.x = e.clientX;
    drag.y = e.clientY;
    apply();
  };
  const onUp = (e: MouseEvent) => {
    const d = drag;
    drag = null;
    host.classList.remove("is-panning");
    if (d && !d.moved) {
      const open = (e.target as HTMLElement).closest?.("[data-open]") as HTMLElement | null;
      if (open) api.open(open.dataset.open!);
    }
  };
  const onHud = (e: MouseEvent) => {
    const z = (e.target as HTMLElement).dataset.z;
    if (z === "fit") fit();
    else if (z) zoomAt(host.clientWidth / 2, host.clientHeight / 2, z === "in" ? 1.25 : 0.8);
  };
  host.addEventListener("wheel", onWheel, { passive: false });
  host.addEventListener("mousedown", onDown);
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
  hud.addEventListener("click", onHud);
  fit();
  return () => {
    clearTimeout(settle);
    host.removeEventListener("wheel", onWheel);
    host.removeEventListener("mousedown", onDown);
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    delete host.dataset.scale;
    delete host.dataset.tx;
    host.style.backgroundSize = "";
    host.style.backgroundPosition = "";
  };
}
