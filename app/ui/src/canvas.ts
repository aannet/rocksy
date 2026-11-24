// Read-only viewer for JSON Canvas files (F6). Never writes anything: there is no save path here.
import { renderInlineHtml } from "./editor/syntax";
import { iconSvg } from "./icons";
import { isImagePath, safeRelPath } from "./paths";

export interface CNode {
  /** "" when the file gave no usable id (such a node cannot carry edges). */
  id: string;
  type: "text" | "file" | "link" | "group";
  x: number;
  y: number;
  width: number;
  height: number;
  text?: string;
  /** Checked relative path (no "..", never absolute): the only form handed to the host. */
  file?: string;
  /** Raw `file` value that was refused (leaves the vault / absolute / network): shown as text, never loaded. */
  blockedFile?: string;
  url?: string;
  label?: string;
  /** Validated CSS colour (`#hex`) or "". */
  color: string;
}
export type CSide = "top" | "right" | "bottom" | "left";
export interface CEdge {
  fromNode: string;
  toNode: string;
  fromSide?: CSide;
  toSide?: CSide;
  arrowEnd: boolean;
  arrowStart: boolean;
  label?: string;
  color: string;
}
export interface CanvasModel {
  nodes: CNode[];
  edges: CEdge[];
  /** Nodes and edges left out because a field was invalid. */
  dropped: number;
}

export interface CanvasHost {
  /** URL of an image of the canvas, or null when the path is refused or not a file of the vault. */
  assetUrl(rel: string): string | null;
  open(rel: string): void;
  readText(rel: string): Promise<string | null>;
}

const PRESET: Record<string, string> = { "1": "#f0717f", "2": "#f0a04b", "3": "#e8c454", "4": "#6fcf79", "5": "#53c8e0", "6": "#b28cf5" };
const colorOf = (c: unknown, fallback: string): string => {
  if (typeof c !== "string" || !c) return fallback;
  if (Object.prototype.hasOwnProperty.call(PRESET, c)) return PRESET[c];
  return /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(c) ? c : fallback;
};

// ---------------------------------------------------------------- parsing (pure, unit-tested)
// A canvas is untrusted input (synced vault, file received from someone): every field used below is typed here,
// and nothing read from the file ever reaches an HTML or CSS string.

const COORD_MAX = 1e7;
const SIDES = new Set(["top", "right", "bottom", "left"]);
const TYPES = new Set(["text", "file", "link", "group"]);

/** A finite number (or a plain decimal string such as "120"), clamped; null for anything else. */
function coord(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && /^-?\d{1,9}(\.\d{1,9})?$/.test(v) ? Number(v) : NaN;
  return Number.isFinite(n) ? Math.max(-COORD_MAX, Math.min(COORD_MAX, n)) : null;
}
const idOf = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" && Number.isFinite(v) ? String(v) : "");
const str = (v: unknown): string | undefined => (typeof v === "string" && v ? v : undefined);
const side = (v: unknown): CSide | undefined => (typeof v === "string" && SIDES.has(v) ? (v as CSide) : undefined);

/** Parses and validates a JSON Canvas. Throws only when the text is not JSON. Invalid nodes and edges are dropped. */
export function normalizeCanvas(json: string): CanvasModel {
  const data: unknown = JSON.parse(json || "{}");
  const obj = (data && typeof data === "object" ? data : {}) as { nodes?: unknown; edges?: unknown };
  const rawNodes = Array.isArray(obj.nodes) ? obj.nodes : [];
  const rawEdges = Array.isArray(obj.edges) ? obj.edges : [];
  const nodes: CNode[] = [];
  const edges: CEdge[] = [];
  const ids = new Set<string>();
  let dropped = 0;
  for (const raw of rawNodes) {
    const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
    const x = coord(r.x);
    const y = coord(r.y);
    const width = coord(r.width);
    const height = coord(r.height);
    if (x == null || y == null || width == null || height == null || width < 0 || height < 0) {
      dropped++;
      continue;
    }
    const type = (typeof r.type === "string" && TYPES.has(r.type) ? r.type : "text") as CNode["type"];
    let id = idOf(r.id);
    if (ids.has(id)) id = "";
    if (id) ids.add(id);
    const n: CNode = { id, type, x, y, width, height, color: colorOf(r.color, "") };
    if (type === "group") n.label = str(r.label);
    else if (type === "link") n.url = str(r.url);
    else if (type === "file") {
      const file = str(r.file);
      const safe = file ? safeRelPath(file) : null;
      if (safe) n.file = safe;
      else if (file) n.blockedFile = file;
    } else n.text = str(r.text);
    nodes.push(n);
  }
  for (const raw of rawEdges) {
    const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
    const fromNode = idOf(r.fromNode);
    const toNode = idOf(r.toNode);
    if (!fromNode || !toNode || !ids.has(fromNode) || !ids.has(toNode)) {
      dropped++;
      continue;
    }
    edges.push({
      fromNode,
      toNode,
      fromSide: side(r.fromSide),
      toSide: side(r.toSide),
      arrowEnd: r.toEnd !== "none",
      arrowStart: r.fromEnd === "arrow",
      label: str(r.label),
      color: colorOf(r.color, "#6b7a99"),
    });
  }
  return { nodes, edges, dropped };
}

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

function anchor(n: CNode, side: CSide | undefined, toward: CNode): [number, number, number, number] {
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

/** Path data of an edge (numbers only) and the position of its label. */
export function edgeGeometry(a: CNode, b: CNode, e: CEdge): { d: string; labelX: number; labelY: number } {
  const [x1, y1, dx1, dy1] = anchor(a, e.fromSide, b);
  const [x2, y2, dx2, dy2] = anchor(b, e.toSide, a);
  const k = Math.max(40, Math.hypot(x2 - x1, y2 - y1) * 0.4);
  return { d: `M${x1} ${y1} C${x1 + dx1 * k} ${y1 + dy1 * k} ${x2 + dx2 * k} ${y2 + dy2 * k} ${x2} ${y2}`, labelX: (x1 + x2) / 2, labelY: (y1 + y2) / 2 - 6 };
}

const SVG_NS = "http://www.w3.org/2000/svg";
function svgEl(name: string, attrs: Record<string, string | number>): SVGElement {
  const el = document.createElementNS(SVG_NS, name);
  for (const k in attrs) el.setAttribute(k, String(attrs[k]));
  return el;
}
function htmlEl<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (className) el.className = className;
  if (text != null) el.textContent = text;
  return el;
}

export function mountCanvas(host: HTMLElement, json: string, api: CanvasHost): () => void {
  let model: CanvasModel;
  try {
    model = normalizeCanvas(json);
  } catch (e) {
    const box = htmlEl("div", "empty is-error");
    box.append(htmlEl("h2", "", "Canvas illisible"), htmlEl("div", "", String((e as Error).message)));
    host.replaceChildren(box);
    return () => undefined;
  }
  const { nodes, edges } = model;
  const world = htmlEl("div", "canvas-world");
  const byId = new Map<string, CNode>();
  for (const n of nodes) if (n.id) byId.set(n.id, n);

  const svg = svgEl("svg", { width: 1, height: 1 });
  const defs = svgEl("defs", {});
  const marker = svgEl("marker", { id: "cv-arrow", viewBox: "0 0 10 10", refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: "auto-start-reverse" });
  marker.appendChild(svgEl("path", { d: "M0 0 10 5 0 10z", fill: "context-stroke" }));
  defs.appendChild(marker);
  svg.appendChild(defs);
  for (const e of edges) {
    const a = byId.get(e.fromNode);
    const b = byId.get(e.toNode);
    if (!a || !b) continue;
    const g = edgeGeometry(a, b, e);
    const path = svgEl("path", { d: g.d, fill: "none", stroke: e.color, "stroke-width": 2.2 });
    if (e.arrowEnd) path.setAttribute("marker-end", "url(#cv-arrow)");
    if (e.arrowStart) path.setAttribute("marker-start", "url(#cv-arrow)");
    svg.appendChild(path);
    if (e.label) {
      const label = svgEl("text", { class: "cv-edge-label", x: g.labelX, y: g.labelY, "text-anchor": "middle" });
      label.textContent = e.label;
      svg.appendChild(label);
    }
  }
  world.appendChild(svg);

  // Groups first (behind), then cards.
  const ordered = [...nodes].sort((a, b) => (a.type === "group" ? 0 : 1) - (b.type === "group" ? 0 : 1));
  for (const n of ordered) {
    const el = document.createElement("div");
    el.style.left = n.x + "px";
    el.style.top = n.y + "px";
    el.style.width = n.width + "px";
    el.style.height = n.height + "px";
    const col = n.color;
    if (col) el.style.borderColor = col;
    if (n.type === "group") {
      el.className = "cv-group";
      // Alpha suffix only on #rrggbb (the other validated forms would become an invalid or different colour).
      if (col.length === 7) el.style.background = col + "14";
      if (n.label) el.appendChild(htmlEl("div", "cv-group-label", n.label));
    } else if (n.type === "file" && n.file) {
      const file = n.file;
      if (isImagePath(file)) {
        el.className = "cv-node cv-image";
        const url = api.assetUrl(file);
        if (url) {
          const img = document.createElement("img");
          img.src = url;
          img.alt = file;
          img.draggable = false;
          el.appendChild(img);
        } else el.appendChild(htmlEl("div", "cv-excerpt", `Image non chargée (introuvable dans le vault) : ${file.slice(0, 200)}`));
      } else {
        el.className = "cv-node cv-file";
        const name = file.slice(file.lastIndexOf("/") + 1).replace(/\.md$/i, "");
        const title = htmlEl("div", "cv-title");
        title.innerHTML = iconSvg("file", 14); // constant markup, no canvas data
        title.dataset.open = file;
        title.appendChild(htmlEl("span", "", name));
        const excerpt = htmlEl("div", "cv-excerpt");
        el.append(title, excerpt);
        if (/\.md$/i.test(file)) {
          void api.readText(file).then((t) => {
            if (t) excerpt.textContent = t.replace(/^---\n[\s\S]*?\n---\n/, "").trim().slice(0, 500);
          });
        }
      }
    } else if (n.type === "file" && n.blockedFile) {
      // Path refused by safeRelPath: shown, never opened, read or loaded as an image.
      el.className = "cv-node cv-file";
      const title = htmlEl("div", "cv-title");
      title.innerHTML = iconSvg("file", 14);
      title.style.cursor = "default";
      title.appendChild(htmlEl("span", "", n.blockedFile.slice(0, 200)));
      el.append(title, htmlEl("div", "cv-excerpt", "Chemin refusé (hors du vault) : fichier non chargé."));
    } else if (n.type === "link") {
      el.className = "cv-node cv-link";
      const body = htmlEl("div", "cv-body");
      body.innerHTML = iconSvg("external", 14);
      body.appendChild(htmlEl("span", "", n.url ?? ""));
      el.appendChild(body);
    } else {
      el.className = "cv-node cv-text";
      const body = htmlEl("div", "cv-body");
      body.innerHTML = cardHtml(n.text ?? ""); // escaped by renderInlineHtml / escapeHtml
      el.appendChild(body);
    }
    world.appendChild(el);
  }

  const hud = htmlEl("div", "cv-hud");
  const hudButton = (z: string, title: string, text: string) => {
    const b = htmlEl("button", "", text);
    b.dataset.z = z;
    b.title = title;
    return b;
  };
  const zoomLabel = htmlEl("span", "cv-zoom", "100 %");
  hud.append(hudButton("out", "Zoom arrière", "−"), zoomLabel, hudButton("in", "Zoom avant", "+"), hudButton("fit", "Ajuster", "⤢"));
  const badge = htmlEl("div", "cv-badge");
  badge.textContent = `Canvas en lecture seule · ${nodes.length} nœuds · ${edges.length} liens` + (model.dropped ? ` · ${model.dropped} éléments invalides ignorés` : "");
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
    zoomLabel.textContent = Math.round(scale * 100) + " %";
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
