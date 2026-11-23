// Live Preview: decorations computed from the @lezer/markdown syntax tree.
// - inline decorations (ViewPlugin): limited to the visible ranges, rebuilt on doc/selection/viewport change;
// - block decorations (StateField): tables, folded frontmatter, folded callouts. Only top-level nodes are walked.
// A Markdown marker reappears when the cursor or the selection touches its element.
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { type EditorState, Facet, type Range, StateEffect, StateField, type Extension } from "@codemirror/state";
import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import type { Tree } from "@lezer/common";
import { iconSvg } from "../icons";
import { codeLabel } from "./langs";
import {
  calloutDefaultTitle,
  calloutStyle,
  parseCalloutHead,
  parseEmbed,
  parseTable,
  renderInlineHtml,
  touchesRange,
} from "./syntax";

export type LinkRef = { kind: "wiki"; inner: string } | { kind: "url"; href: string };
export type ImageRef = { kind: "url"; url: string } | { kind: "missing" } | { kind: "remote"; url: string };

/** Services the editor needs from the app shell. */
export interface LpHost {
  openLink(link: LinkRef, fromPath: string, newTab: boolean): void;
  resolveImage(src: string, fromPath: string): ImageRef;
  openTag(tag: string): void;
  /** null when unknown (no vault tree yet). */
  linkExists(inner: string, fromPath: string): boolean | null;
  /** Bumped whenever the vault tree changes, so image/link widgets are re-resolved. */
  epoch(): number;
  isImage(target: string): boolean;
}

export interface LpConfig {
  host: LpHost;
  path: string;
}

export const lpConfig = Facet.define<LpConfig, LpConfig | null>({ combine: (v) => v[0] ?? null });

export const refreshEffect = StateEffect.define<null>();
export const toggleCalloutEffect = StateEffect.define<number>();
export const setFrontmatterFoldEffect = StateEffect.define<boolean>();
export const flashEffect = StateEffect.define<{ from: number; to: number } | null>();

// ---------------------------------------------------------------- widgets

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-bullet";
    s.textContent = "•";
    return s;
  }
}

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean) {
    super();
  }
  eq(o: CheckboxWidget) {
    return o.checked === this.checked;
  }
  toDOM(view: EditorView) {
    const wrap = document.createElement("span");
    wrap.className = "cm-lp-checkwrap";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-lp-check";
    box.checked = this.checked;
    box.tabIndex = -1;
    box.addEventListener("mousedown", (e) => e.preventDefault());
    box.addEventListener("click", (e) => {
      e.preventDefault();
      const pos = view.posAtDOM(wrap);
      const cur = view.state.doc.sliceString(pos, pos + 3);
      if (/^\[[ xX]\]$/.test(cur)) view.dispatch({ changes: { from: pos + 1, to: pos + 2, insert: cur[1] === " " ? "x" : " " } });
    });
    wrap.appendChild(box);
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

class HrWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-hr";
    return s;
  }
}

class ImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly width: number | null,
    readonly height: number | null,
    readonly path: string,
    readonly epoch: number,
  ) {
    super();
  }
  eq(o: ImageWidget) {
    return o.src === this.src && o.width === this.width && o.height === this.height && o.path === this.path && o.epoch === this.epoch && o.alt === this.alt;
  }
  toDOM(view: EditorView) {
    const cfg = view.state.facet(lpConfig);
    const ref = cfg ? cfg.host.resolveImage(this.src, this.path) : ({ kind: "missing" } as ImageRef);
    if (ref.kind !== "url") {
      const chip = document.createElement("span");
      chip.className = "cm-lp-img-missing";
      chip.innerHTML = iconSvg("image", 14);
      chip.append(ref.kind === "remote" ? ` Image distante non chargée : ${this.src}` : ` Image introuvable : ${this.src}`);
      return chip;
    }
    const img = document.createElement("img");
    img.className = "cm-lp-img";
    img.alt = this.alt;
    img.draggable = false;
    if (this.width) img.style.width = this.width + "px";
    if (this.height) img.style.height = this.height + "px";
    img.addEventListener("load", () => view.requestMeasure());
    img.addEventListener("error", () => {
      img.classList.add("cm-lp-img-error");
      img.alt = `Image illisible : ${this.src}`;
      view.requestMeasure();
    });
    img.src = ref.url;
    return img;
  }
}

class EmbedChipWidget extends WidgetType {
  constructor(readonly inner: string) {
    super();
  }
  eq(o: EmbedChipWidget) {
    return o.inner === this.inner;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-embed-chip cm-lp-rendered";
    s.dataset.wikilink = this.inner;
    s.innerHTML = iconSvg("file", 13);
    s.append(" " + (parseEmbed(this.inner).alias ?? parseEmbed(this.inner).target));
    return s;
  }
}

class CalloutIconWidget extends WidgetType {
  constructor(
    readonly icon: string,
    readonly title: string | null,
  ) {
    super();
  }
  eq(o: CalloutIconWidget) {
    return o.icon === this.icon && o.title === this.title;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-co-icon";
    s.innerHTML = iconSvg(this.icon, 17);
    if (this.title) {
      const t = document.createElement("span");
      t.textContent = this.title;
      s.appendChild(t);
    }
    return s;
  }
}

class FoldToggleWidget extends WidgetType {
  constructor(
    readonly folded: boolean,
    readonly kind: "callout" | "frontmatter",
    readonly label: string,
  ) {
    super();
  }
  eq(o: FoldToggleWidget) {
    return o.folded === this.folded && o.kind === this.kind && o.label === this.label;
  }
  toDOM(view: EditorView) {
    const s = document.createElement("span");
    s.className = "cm-lp-fold" + (this.folded ? " is-folded" : "") + (this.kind === "frontmatter" ? " cm-lp-fm-head" : "");
    s.title = this.folded ? "Déplier" : "Replier";
    if (this.label) {
      const t = document.createElement("span");
      t.textContent = this.label;
      s.appendChild(t);
    }
    s.insertAdjacentHTML("beforeend", iconSvg("chevron", 14));
    s.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (this.kind === "frontmatter") {
        view.dispatch({ effects: setFrontmatterFoldEffect.of(!this.folded) });
      } else {
        const line = view.state.doc.lineAt(view.posAtDOM(s));
        view.dispatch({ effects: toggleCalloutEffect.of(line.from) });
      }
    });
    return s;
  }
  ignoreEvent() {
    return true;
  }
}

class CodeLabelWidget extends WidgetType {
  constructor(readonly label: string) {
    super();
  }
  eq(o: CodeLabelWidget) {
    return o.label === this.label;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-lp-cb-label" + (this.label === "Mermaid" ? " is-mermaid" : "");
    s.textContent = this.label;
    return s;
  }
}

class TableWidget extends WidgetType {
  constructor(readonly src: string) {
    super();
  }
  eq(o: TableWidget) {
    return o.src === this.src;
  }
  get estimatedHeight() {
    return this.src.split("\n").length * 34;
  }
  toDOM(view: EditorView) {
    const wrap = document.createElement("div");
    wrap.className = "cm-lp-table-wrap";
    const model = parseTable(this.src);
    if (!model) {
      wrap.textContent = this.src;
      return wrap;
    }
    const cell = (tag: string, c: { text: string; off: number } | undefined, i: number) => {
      const a = model.align[i];
      return `<${tag}${a ? ` style="text-align:${a}"` : ""} data-off="${c ? c.off : -1}">${c ? renderInlineHtml(c.text) : ""}</${tag}>`;
    };
    const cols = model.align.length;
    let html = "<table class=\"cm-lp-table\"><thead><tr>";
    for (let i = 0; i < cols; i++) html += cell("th", model.header[i], i);
    html += "</tr></thead><tbody>";
    for (const row of model.rows) {
      html += "<tr>";
      for (let i = 0; i < cols; i++) html += cell("td", row[i], i);
      html += "</tr>";
    }
    wrap.innerHTML = html + "</tbody></table>";
    wrap.addEventListener("mousedown", (e) => {
      const t = e.target as HTMLElement;
      const special = t.closest?.("[data-href],[data-wikilink]") as HTMLElement | null;
      if (special && (e.button === 0 || e.button === 1)) {
        e.preventDefault();
        const cfg = view.state.facet(lpConfig);
        if (cfg) {
          const link: LinkRef = special.dataset.href != null ? { kind: "url", href: special.dataset.href } : { kind: "wiki", inner: special.dataset.wikilink! };
          cfg.host.openLink(link, cfg.path, e.button === 1 || e.ctrlKey || e.metaKey);
        }
        return;
      }
      if (e.button !== 0) return;
      e.preventDefault();
      const from = view.posAtDOM(wrap);
      const c = t.closest?.("[data-off]") as HTMLElement | null;
      const off = c ? Number(c.dataset.off) : 0;
      view.dispatch({ selection: { anchor: from + Math.max(0, off) }, scrollIntoView: false });
      view.focus();
    });
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

const mermaidCache = new Map<string, { svg: string } | { error: string }>();

/** Mermaid block rendered with Pele (obsidianmd/pele), loaded on first use. No network: images and links are disabled. */
class MermaidWidget extends WidgetType {
  constructor(readonly src: string) {
    super();
  }
  eq(o: MermaidWidget) {
    return o.src === this.src;
  }
  get estimatedHeight() {
    return 220;
  }
  toDOM(view: EditorView) {
    const wrap = document.createElement("div");
    wrap.className = "cm-lp-mermaid";
    const label = document.createElement("span");
    label.className = "cm-lp-cb-label is-mermaid";
    label.textContent = "Mermaid";
    const body = document.createElement("div");
    body.className = "cm-lp-mermaid-body pele";
    wrap.append(label, body);
    const show = (r: { svg: string } | { error: string }) => {
      if ("svg" in r) body.innerHTML = r.svg;
      else {
        body.classList.add("is-error");
        body.textContent = r.error;
      }
      view.requestMeasure();
    };
    const cached = mermaidCache.get(this.src);
    if (cached) show(cached);
    else {
      body.textContent = "Rendu du diagramme…";
      const src = this.src;
      import("pele/lazy")
        .then((m) => m.renderAsync(src, { images: false, links: false, maxWidth: Math.max(320, view.contentDOM.clientWidth - 40) }))
        .then(
          (r) => ({ svg: r.svg }),
          (e: { message?: string; line?: number; column?: number }) => ({
            error: `Diagramme Mermaid invalide${e && e.line ? ` (ligne ${e.line}, colonne ${e.column})` : ""} : ${e?.message ?? e}`,
          }),
        )
        .then((r) => {
          if (mermaidCache.size > 200) mermaidCache.clear();
          mermaidCache.set(src, r);
          show(r);
        });
    }
    wrap.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const from = view.posAtDOM(wrap);
      const line = view.state.doc.lineAt(from);
      view.dispatch({ selection: { anchor: Math.min(view.state.doc.length, line.to + 1) } });
      view.focus();
    });
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

class FrontmatterFoldedWidget extends WidgetType {
  constructor(readonly count: number) {
    super();
  }
  eq(o: FrontmatterFoldedWidget) {
    return o.count === this.count;
  }
  get estimatedHeight() {
    return 30;
  }
  toDOM(view: EditorView) {
    const d = document.createElement("div");
    d.className = "cm-lp-fm-folded";
    d.innerHTML = `<span>Propriétés</span><span class="cm-lp-fm-count">${this.count}</span>${iconSvg("chevron", 14)}`;
    d.title = "Déplier les propriétés";
    d.addEventListener("mousedown", (e) => {
      e.preventDefault();
      view.dispatch({ effects: setFrontmatterFoldEffect.of(false) });
    });
    return d;
  }
  ignoreEvent() {
    return true;
  }
}

// ---------------------------------------------------------------- shared decorations

const hideDeco = Decoration.replace({});
const dimMark = Decoration.mark({ class: "cm-lp-mark" });
const emDeco = Decoration.mark({ class: "cm-lp-em" });
const strongDeco = Decoration.mark({ class: "cm-lp-strong" });
const strikeDeco = Decoration.mark({ class: "cm-lp-strike" });
const hlDeco = Decoration.mark({ class: "cm-lp-hl" });
const codeDeco = Decoration.mark({ class: "cm-lp-code" });
const gapDeco = Decoration.mark({ class: "cm-lp-gap" });
const rawBulletDeco = Decoration.mark({ class: "cm-lp-ulraw" });
const bulletDeco = Decoration.replace({ widget: new BulletWidget() });
const hrDeco = Decoration.replace({ widget: new HrWidget() });
const checkOn = Decoration.replace({ widget: new CheckboxWidget(true) });
const checkOff = Decoration.replace({ widget: new CheckboxWidget(false) });
const urlDim = Decoration.mark({ class: "cm-lp-url" });

const lineCache = new Map<string, Decoration>();
function lineDeco(cls: string, style?: string): Decoration {
  const key = style ? cls + "|" + style : cls;
  let d = lineCache.get(key);
  if (!d) {
    d = Decoration.line(style ? { class: cls, attributes: { style } } : { class: cls });
    if (lineCache.size < 2000) lineCache.set(key, d);
  }
  return d;
}
const markCache = new Map<string, Decoration>();
function styledMark(cls: string, style: string): Decoration {
  const key = cls + "|" + style;
  let d = markCache.get(key);
  if (!d) {
    d = Decoration.mark({ class: cls, attributes: { style } });
    if (markCache.size < 2000) markCache.set(key, d);
  }
  return d;
}

const INDENT_EM = 1.6;
const MARK_EM = 1.35;
const GAP_EM = 0.3;

/** Lines longer than this keep their inline Markdown in source style (see buildInlineDecorations). */
export const HUGE_LINE = 6000;
const INLINE_NODES = new Set([
  "Emphasis", "StrongEmphasis", "Strikethrough", "Highlight", "InlineCode", "EmphasisMark", "StrikethroughMark", "HighlightMark",
  "CodeMark", "Escape", "Link", "Image", "Embed", "Wikilink", "Hashtag", "URL",
]);

export interface InlineOptions {
  /** Callout fold lookup (from the block field). */
  calloutFolded?: (pos: number, marker: "+" | "-") => boolean;
  cfg?: LpConfig | null;
}

/**
 * Computes the inline/line decorations for the given ranges. Pure with respect to the DOM:
 * it only needs a state and a syntax tree, so it is unit-testable in Node.
 */
export function buildInlineDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[],
  tree: Tree,
  opts: InlineOptions = {},
): Range<Decoration>[] {
  const doc = state.doc;
  const sel = state.selection.ranges;
  const out: Range<Decoration>[] = [];
  const cfg = opts.cfg ?? null;
  const epoch = cfg ? cfg.host.epoch() : 0;
  const path = cfg ? cfg.path : "";
  const touches = (from: number, to: number) => touchesRange(sel, from, to);
  const hide = (from: number, to: number) => {
    if (to > from) out.push(hideDeco.range(from, to));
  };
  const spaceAfter = (pos: number) => {
    const c = doc.sliceString(pos, pos + 1);
    return c === " " || c === "\t";
  };

  let hugeFrom = -1;
  let hugeTo = -1;
  let huge = false;
  for (let ri = 0; ri < ranges.length; ri++) {
    const rFrom = ranges[ri].from;
    const rTo = ranges[ri].to;
    const prevTo = ri > 0 ? ranges[ri - 1].to : -1;
    tree.iterate({
      from: rFrom,
      to: rTo,
      enter: (n) => {
        const name = n.name;
        // Inline nodes that started in the previous visible range were handled there.
        const seen = n.from < prevTo;
        // A single huge line (tens of thousands of characters) with thousands of inline elements makes every
        // keystroke re-render thousands of widgets: such lines stay in source style.
        if (INLINE_NODES.has(name)) {
          if (n.from < hugeFrom || n.from > hugeTo) {
            const l = doc.lineAt(n.from);
            hugeFrom = l.from;
            hugeTo = l.to;
            huge = l.length > HUGE_LINE;
          }
          if (huge) return false;
        }
        switch (name) {
          case "ATXHeading1":
          case "ATXHeading2":
          case "ATXHeading3":
          case "ATXHeading4":
          case "ATXHeading5":
          case "ATXHeading6": {
            if (seen) return;
            const level = name.charCodeAt(10) - 48;
            const line = doc.lineAt(n.from);
            out.push(lineDeco("cm-lp-h cm-lp-h" + level).range(line.from));
            const mark = n.node.firstChild;
            if (mark && mark.name === "HeaderMark") {
              if (!touches(line.from, line.to)) hide(mark.from, Math.min(line.to, mark.to + (spaceAfter(mark.to) ? 1 : 0)));
              else out.push(dimMark.range(mark.from, mark.to));
            }
            return;
          }
          case "SetextHeading1":
          case "SetextHeading2": {
            if (seen) return;
            const level = name === "SetextHeading1" ? 1 : 2;
            const a = doc.lineAt(n.from).number;
            const b = doc.lineAt(n.to).number;
            for (let ln = a; ln < b; ln++) out.push(lineDeco("cm-lp-h cm-lp-h" + level).range(doc.line(ln).from));
            return;
          }
          case "Blockquote": {
            const outer = n.node.parent?.name !== "Blockquote";
            const first = doc.lineAt(n.from);
            const callout = outer ? parseCalloutHead(first.text) : null;
            const a = doc.lineAt(Math.max(n.from, rFrom)).number;
            const b = doc.lineAt(Math.min(n.to, rTo)).number;
            const lastNo = doc.lineAt(n.to).number;
            let folded = false;
            let base = "cm-lp-quote";
            if (callout) {
              const st = calloutStyle(callout.type);
              base = "cm-lp-callout cm-lp-co-" + st.color;
              if (callout.fold && opts.calloutFolded) folded = opts.calloutFolded(first.from, callout.fold);
              if (first.from >= rFrom && first.from >= prevTo) {
                if (!touches(first.from, first.to)) {
                  const title = callout.title ? null : calloutDefaultTitle(callout.type);
                  out.push(
                    Decoration.replace({ widget: new CalloutIconWidget(st.icon, title) }).range(
                      first.from + callout.prefixLen,
                      first.from + callout.headLen,
                    ),
                  );
                }
                if (callout.fold) {
                  out.push(Decoration.widget({ widget: new FoldToggleWidget(folded, "callout", ""), side: 1 }).range(first.to));
                }
              }
            }
            for (let ln = a; ln <= b; ln++) {
              const line = doc.line(ln);
              let cls = base;
              if (callout) {
                if (ln === first.number) cls += " cm-lp-co-head";
                if (ln === lastNo || (folded && ln === first.number)) cls += " cm-lp-co-last";
              }
              out.push(lineDeco(cls).range(line.from));
            }
            return;
          }
          case "QuoteMark": {
            const line = doc.lineAt(n.from);
            if (!touches(line.from, line.to)) hide(n.from, Math.min(line.to, n.to + (spaceAfter(n.to) ? 1 : 0)));
            else out.push(dimMark.range(n.from, n.to));
            return;
          }
          case "ListMark": {
            const item = n.node.parent;
            const list = item?.parent;
            if (!item || !list) return;
            const line = doc.lineAt(n.from);
            let depth = 0;
            for (let p = list.parent; p; p = p.parent) if (p.name === "ListItem") depth++;
            const ordered = list.name === "OrderedList";
            const markLen = n.to - n.from;
            const markW = ordered ? Math.max(MARK_EM, markLen * 0.62 + 0.15) : MARK_EM;
            const leading = doc.sliceString(line.from, n.from);
            const plainIndent = /^[ \t]*$/.test(leading);
            const hasGap = spaceAfter(n.to) && n.to < line.to;
            if (plainIndent) {
              const pad = Math.round((depth * INDENT_EM + markW + (hasGap ? GAP_EM : 0)) * 100) / 100;
              out.push(lineDeco("cm-lp-li", `--lp-pad:${pad}em`).range(line.from));
              if (n.from > line.from) out.push(styledMark("cm-lp-indent", `width:${depth * INDENT_EM}em`).range(line.from, n.from));
            }
            const next = n.node.nextSibling;
            const marker = next && next.name === "Task" ? next.firstChild : null;
            if (marker && marker.name === "TaskMarker" && !ordered) {
              const checked = doc.sliceString(marker.from + 1, marker.from + 2) !== " ";
              if (checked) out.push(lineDeco("cm-lp-task-done").range(line.from));
              if (!touches(n.from, marker.to)) {
                hide(n.from, marker.from);
                out.push((checked ? checkOn : checkOff).range(marker.from, marker.to));
                if (spaceAfter(marker.to) && marker.to < line.to) out.push(gapDeco.range(marker.to, marker.to + 1));
              } else {
                out.push(dimMark.range(n.from, marker.to));
              }
              return;
            }
            if (ordered) {
              out.push(styledMark("cm-lp-olmark", `width:${markW}em`).range(n.from, n.to));
            } else if (!touches(n.from, n.to)) {
              out.push(bulletDeco.range(n.from, n.to));
            } else {
              out.push(rawBulletDeco.range(n.from, n.to));
            }
            if (hasGap) out.push(gapDeco.range(n.to, n.to + 1));
            return;
          }
          case "HorizontalRule": {
            if (seen) return;
            const line = doc.lineAt(n.from);
            if (!touches(line.from, line.to)) out.push(hrDeco.range(n.from, n.to));
            else out.push(dimMark.range(n.from, n.to));
            return;
          }
          case "FencedCode": {
            const first = doc.lineAt(n.from);
            const last = doc.lineAt(n.to);
            const infoNode = n.node.getChild("CodeInfo");
            const info = infoNode ? doc.sliceString(infoNode.from, infoNode.to) : "";
            const closed = last.number > first.number && /^[ \t>]*(`{3,}|~{3,})[ \t]*$/.test(last.text);
            const active = touches(n.from, n.to);
            const a = doc.lineAt(Math.max(n.from, rFrom)).number;
            const b = doc.lineAt(Math.min(n.to, rTo)).number;
            for (let ln = a; ln <= b; ln++) {
              let cls = "cm-lp-cb";
              if (ln === first.number) cls += " cm-lp-cb-first";
              if (ln === last.number) cls += " cm-lp-cb-last";
              if (!active && (ln === first.number || (closed && ln === last.number))) cls += " cm-lp-cb-fence";
              out.push(lineDeco(cls).range(doc.line(ln).from));
            }
            const label = codeLabel(info);
            if (first.from >= rFrom && first.from >= prevTo) {
              if (!active) {
                out.push(Decoration.replace({ widget: new CodeLabelWidget(label) }).range(n.from, first.to));
              } else if (label) {
                out.push(Decoration.widget({ widget: new CodeLabelWidget(label), side: 1 }).range(first.to));
              }
            }
            if (closed && !active && last.from <= rTo && last.from >= prevTo) {
              const m = /(`{3,}|~{3,})[ \t]*$/.exec(last.text)!;
              hide(last.from + m.index, last.to);
            }
            return false;
          }
          case "CodeBlock": {
            const a = doc.lineAt(Math.max(n.from, rFrom)).number;
            const b = doc.lineAt(Math.min(n.to, rTo)).number;
            const f = doc.lineAt(n.from).number;
            const l = doc.lineAt(n.to).number;
            for (let ln = a; ln <= b; ln++) {
              out.push(lineDeco("cm-lp-cb" + (ln === f ? " cm-lp-cb-first" : "") + (ln === l ? " cm-lp-cb-last" : "")).range(doc.line(ln).from));
            }
            return false;
          }
          case "Frontmatter": {
            const first = doc.lineAt(n.from);
            const last = doc.lineAt(n.to);
            const a = doc.lineAt(Math.max(n.from, rFrom)).number;
            const b = doc.lineAt(Math.min(n.to, rTo)).number;
            for (let ln = a; ln <= b; ln++) {
              out.push(lineDeco("cm-lp-fm" + (ln === first.number ? " cm-lp-fm-first" : "") + (ln === last.number ? " cm-lp-fm-last" : "")).range(doc.line(ln).from));
            }
            if (first.from >= rFrom && !touches(first.from, first.to) && first.to > first.from) {
              out.push(Decoration.replace({ widget: new FoldToggleWidget(false, "frontmatter", "Propriétés") }).range(first.from, first.to));
            }
            return false;
          }
          case "Table": {
            const a = doc.lineAt(Math.max(n.from, rFrom)).number;
            const b = doc.lineAt(Math.min(n.to, rTo)).number;
            for (let ln = a; ln <= b; ln++) out.push(lineDeco("cm-lp-tsrc").range(doc.line(ln).from));
            return false;
          }
          case "Emphasis":
            if (!seen) out.push(emDeco.range(n.from, n.to));
            return;
          case "StrongEmphasis":
            if (!seen) out.push(strongDeco.range(n.from, n.to));
            return;
          case "Strikethrough":
            if (!seen) out.push(strikeDeco.range(n.from, n.to));
            return;
          case "Highlight":
            if (!seen) out.push(hlDeco.range(n.from, n.to));
            return;
          case "InlineCode":
            if (!seen) out.push(codeDeco.range(n.from, n.to));
            return;
          case "EmphasisMark":
          case "StrikethroughMark":
          case "HighlightMark":
          case "CodeMark": {
            if (seen) return;
            const p = n.node.parent;
            if (!p || (name === "CodeMark" && p.name !== "InlineCode")) return;
            if (!touches(p.from, p.to)) hide(n.from, n.to);
            else out.push(dimMark.range(n.from, n.to));
            return;
          }
          case "Escape": {
            if (!seen && !touches(n.from, n.to)) hide(n.from, n.from + 1);
            return;
          }
          case "Link": {
            if (seen) return;
            const node = n.node;
            const url = node.getChild("URL");
            if (!url) return;
            const marks = node.getChildren("LinkMark");
            if (marks.length < 2) return;
            const textFrom = marks[0].to;
            const textTo = marks[1].from;
            if (textTo <= textFrom) return;
            const href = doc.sliceString(url.from, url.to);
            const active = touches(n.from, n.to);
            out.push(
              Decoration.mark({ class: "cm-lp-link" + (active ? "" : " cm-lp-rendered"), attributes: { "data-href": href } }).range(textFrom, textTo),
            );
            if (!active) {
              hide(n.from, textFrom);
              hide(textTo, n.to);
            } else {
              out.push(dimMark.range(n.from, textFrom));
              out.push(urlDim.range(textTo, n.to));
            }
            return;
          }
          case "Image": {
            if (seen) return false;
            const node = n.node;
            const url = node.getChild("URL");
            if (!url) return false;
            const marks = node.getChildren("LinkMark");
            const alt = marks.length >= 2 ? doc.sliceString(marks[0].to, marks[1].from) : "";
            const src = doc.sliceString(url.from, url.to);
            const sz = /^(.*)\|(\d+)(?:x(\d+))?$/.exec(alt);
            const w = new ImageWidget(src, sz ? sz[1] : alt, sz ? Number(sz[2]) : null, sz && sz[3] ? Number(sz[3]) : null, path, epoch);
            if (touches(n.from, n.to)) out.push(Decoration.widget({ widget: w, side: 1 }).range(n.to));
            else out.push(Decoration.replace({ widget: w }).range(n.from, n.to));
            return false;
          }
          case "Embed": {
            if (seen) return false;
            const inner = doc.sliceString(n.from + 3, n.to - 2);
            const e = parseEmbed(inner);
            const isImg = cfg ? cfg.host.isImage(e.target) : /\.(png|jpe?g|gif|svg|webp|bmp|avif)$/i.test(e.target);
            const w = isImg ? new ImageWidget(e.target, e.alias ?? e.target, e.width, e.height, path, epoch) : new EmbedChipWidget(inner);
            if (touches(n.from, n.to)) {
              out.push(dimMark.range(n.from, n.from + 3));
              out.push(dimMark.range(n.to - 2, n.to));
              if (isImg) out.push(Decoration.widget({ widget: w, side: 1 }).range(n.to));
            } else out.push(Decoration.replace({ widget: w }).range(n.from, n.to));
            return false;
          }
          case "Wikilink": {
            if (seen) return false;
            const inner = doc.sliceString(n.from + 2, n.to - 2);
            const bar = inner.indexOf("|");
            const active = touches(n.from, n.to);
            const dispFrom = bar >= 0 && bar < inner.length - 1 ? n.from + 2 + bar + 1 : n.from + 2;
            const dispTo = n.to - 2;
            const exists = cfg ? cfg.host.linkExists(inner, path) : null;
            const cls = "cm-lp-link cm-lp-wikilink" + (active ? "" : " cm-lp-rendered") + (exists === false ? " cm-lp-unresolved" : "");
            out.push(Decoration.mark({ class: cls, attributes: { "data-wikilink": inner } }).range(active ? n.from + 2 : dispFrom, dispTo));
            if (!active) {
              hide(n.from, dispFrom);
              hide(dispTo, n.to);
            } else {
              out.push(dimMark.range(n.from, n.from + 2));
              out.push(dimMark.range(dispTo, n.to));
            }
            return false;
          }
          case "Hashtag": {
            if (seen) return false;
            out.push(Decoration.mark({ class: "cm-lp-tag", attributes: { "data-tag": doc.sliceString(n.from + 1, n.to) } }).range(n.from, n.to));
            return false;
          }
          case "URL": {
            if (seen) return;
            const p = n.node.parent?.name;
            if (p === "Link" || p === "Image") return;
            const href = doc.sliceString(n.from, n.to);
            out.push(Decoration.mark({ class: "cm-lp-link cm-lp-barelink", attributes: { "data-href": href } }).range(n.from, n.to));
            return;
          }
        }
        return;
      },
    });
  }
  return out;
}

// ---------------------------------------------------------------- block field

interface BlockState {
  decos: DecorationSet;
  /** Line starts of callouts whose fold state was toggled by the user. */
  toggled: readonly number[];
  fmFolded: boolean;
  tree: Tree;
}

let frontmatterFoldedDefault = false;
try {
  frontmatterFoldedDefault = localStorage.getItem("rocksy.fmFolded") === "1";
} catch {
  /* storage unavailable */
}

/** Computes block-level replacements. Exported for tests. */
export function buildBlockDecorations(state: EditorState, tree: Tree, toggled: readonly number[], fmFolded: boolean): Range<Decoration>[] {
  const doc = state.doc;
  const sel = state.selection.ranges;
  const out: Range<Decoration>[] = [];
  const c = tree.cursor();
  if (!c.firstChild()) return out;
  do {
    const name = c.name;
    if (name === "Table") {
      const from = doc.lineAt(c.from).from;
      const to = doc.lineAt(c.to).to;
      if (!touchesRange(sel, from, to)) {
        out.push(Decoration.replace({ widget: new TableWidget(doc.sliceString(from, to)), block: true }).range(from, to));
      }
    } else if (name === "Blockquote") {
      const first = doc.lineAt(c.from);
      const end = doc.lineAt(c.to).to;
      if (first.to < end) {
        const co = parseCalloutHead(first.text);
        if (co && co.fold) {
          const folded = (co.fold === "-") !== toggled.includes(first.from);
          if (folded && !touchesRange(sel, first.to + 1, end)) out.push(Decoration.replace({ block: true }).range(first.to + 1, end));
        }
      }
    } else if (name === "FencedCode") {
      const first = doc.lineAt(c.from);
      const last = doc.lineAt(c.to);
      if (last.number - first.number >= 2 && /^[ \t]*(`{3,}|~{3,})[ \t]*mermaid[ \t]*$/i.test(first.text) && /^[ \t]*(`{3,}|~{3,})[ \t]*$/.test(last.text)) {
        if (!touchesRange(sel, first.from, last.to)) {
          const src = doc.sliceString(first.to + 1, last.from - 1);
          if (src.trim()) out.push(Decoration.replace({ widget: new MermaidWidget(src), block: true }).range(first.from, last.to));
        }
      }
    } else if (name === "Frontmatter") {
      if (fmFolded && !touchesRange(sel, c.from, c.to)) {
        const lines = doc.lineAt(c.to).number - 2;
        out.push(Decoration.replace({ widget: new FrontmatterFoldedWidget(Math.max(0, lines)), block: true }).range(c.from, doc.lineAt(c.to).to));
      }
    }
  } while (c.nextSibling());
  return out;
}

function computeBlocks(state: EditorState, toggled: readonly number[], fmFolded: boolean): BlockState {
  const tree = syntaxTree(state);
  return { decos: Decoration.set(buildBlockDecorations(state, tree, toggled, fmFolded)), toggled, fmFolded, tree };
}

export const blockField = StateField.define<BlockState>({
  create: (state) => computeBlocks(state, [], frontmatterFoldedDefault),
  update(value, tr) {
    let toggled = value.toggled;
    let fmFolded = value.fmFolded;
    let dirty = tr.docChanged || tr.selection !== undefined || syntaxTree(tr.state) !== value.tree;
    if (tr.docChanged && toggled.length) toggled = toggled.map((p) => tr.changes.mapPos(p, -1));
    for (const e of tr.effects) {
      if (e.is(toggleCalloutEffect)) {
        toggled = toggled.includes(e.value) ? toggled.filter((p) => p !== e.value) : [...toggled, e.value];
        dirty = true;
      } else if (e.is(setFrontmatterFoldEffect)) {
        fmFolded = e.value;
        frontmatterFoldedDefault = e.value;
        try {
          localStorage.setItem("rocksy.fmFolded", e.value ? "1" : "0");
        } catch {
          /* ignore */
        }
        dirty = true;
      }
    }
    return dirty ? computeBlocks(tr.state, toggled, fmFolded) : value;
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.decos),
});

function calloutFoldedIn(state: EditorState) {
  const bs = state.field(blockField, false);
  return (pos: number, marker: "+" | "-") => (marker === "-") !== (bs ? bs.toggled.includes(pos) : false);
}

// ---------------------------------------------------------------- inline plugin

const inlinePlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    tree: Tree;
    epoch: number;
    constructor(view: EditorView) {
      this.tree = syntaxTree(view.state);
      this.epoch = 0;
      this.decorations = this.build(view);
    }
    update(u: ViewUpdate) {
      const tree = syntaxTree(u.state);
      let force = false;
      for (const tr of u.transactions) for (const e of tr.effects) if (e.is(refreshEffect) || e.is(toggleCalloutEffect) || e.is(setFrontmatterFoldEffect)) force = true;
      if (u.docChanged || u.viewportChanged || u.selectionSet || tree !== this.tree || force) {
        this.decorations = this.build(u.view);
      }
    }
    build(view: EditorView): DecorationSet {
      const state = view.state;
      const tree = ensureSyntaxTree(state, view.viewport.to, 25) ?? syntaxTree(state);
      this.tree = syntaxTree(state);
      try {
        return Decoration.set(
          buildInlineDecorations(state, view.visibleRanges, tree, { calloutFolded: calloutFoldedIn(state), cfg: state.facet(lpConfig) }),
          true,
        );
      } catch (err) {
        console.error("live preview", err);
        return Decoration.none;
      }
    }
  },
  { decorations: (v) => v.decorations },
);

// ---------------------------------------------------------------- flash (tag fragments)

const flashLine = Decoration.line({ class: "cm-lp-flash" });
export const flashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(flashEffect)) {
        if (!e.value) value = Decoration.none;
        else {
          const doc = tr.state.doc;
          const a = doc.lineAt(Math.min(e.value.from, doc.length)).number;
          const b = doc.lineAt(Math.min(e.value.to, doc.length)).number;
          const r: Range<Decoration>[] = [];
          for (let ln = a; ln <= b; ln++) r.push(flashLine.range(doc.line(ln).from));
          value = Decoration.set(r);
        }
      }
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

// ---------------------------------------------------------------- events

const clickHandlers = EditorView.domEventHandlers({
  mousedown(e, view) {
    const t = e.target as HTMLElement | null;
    const el = t?.closest?.("[data-href],[data-wikilink],[data-tag]") as HTMLElement | null;
    if (!el || !view.contentDOM.contains(el)) return false;
    const cfg = view.state.facet(lpConfig);
    if (!cfg) return false;
    const mod = e.ctrlKey || e.metaKey;
    if (el.dataset.tag != null) {
      if (!(mod && e.button === 0)) return false;
      e.preventDefault();
      cfg.host.openTag(el.dataset.tag);
      return true;
    }
    const rendered = el.classList.contains("cm-lp-rendered");
    if (e.button === 1 || (e.button === 0 && (mod || rendered))) {
      e.preventDefault();
      const link: LinkRef = el.dataset.href != null ? { kind: "url", href: el.dataset.href } : { kind: "wiki", inner: el.dataset.wikilink! };
      cfg.host.openLink(link, cfg.path, e.button === 1);
      return true;
    }
    return false;
  },
});

/** Moves the cursor into a rendered block (table, folded frontmatter) instead of jumping over it. */
export function enterBlock(view: EditorView, dir: 1 | -1): boolean {
  const state = view.state;
  const sel = state.selection.main;
  if (!sel.empty) return false;
  const bs = state.field(blockField, false);
  if (!bs) return false;
  const line = state.doc.lineAt(sel.head);
  let target = -1;
  if (dir > 0 && line.to < state.doc.length) {
    const next = line.to + 1;
    bs.decos.between(next, next, (from, to, d) => {
      if (from === next && d.spec.widget) target = from;
      void to;
    });
  } else if (dir < 0 && line.from > 0) {
    const prev = line.from - 1;
    bs.decos.between(prev, prev, (from, to, d) => {
      if (to === prev && d.spec.widget) target = state.doc.lineAt(to).from;
      void from;
    });
  }
  if (target < 0) return false;
  view.dispatch({ selection: { anchor: target }, scrollIntoView: true });
  return true;
}

export function livePreview(): Extension {
  return [blockField, inlinePlugin, clickHandlers];
}
