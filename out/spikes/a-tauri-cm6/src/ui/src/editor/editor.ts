// Editor state factory: the minimal set of extensions needed for the first paint.
// Search and autocompletion live in extras.ts and are loaded after the editor is usable.
import { Compartment, EditorSelection, EditorState, Prec, type Extension, type StateEffect, type Text } from "@codemirror/state";
import { EditorView, keymap, type Command, type ViewUpdate } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentLess, indentMore, insertTab } from "@codemirror/commands";
import { HighlightStyle, indentUnit, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { rocksyMarkdown } from "./mdlang";
import { enterBlock, flashEffect, flashField, livePreview, lpConfig, refreshEffect, type LpHost } from "./livepreview";
import { listContinuation } from "./syntax";
import { termHighlight } from "./termhl";

export interface EditorHost extends LpHost {
  onUpdate(update: ViewUpdate): void;
}

const highlight = HighlightStyle.define([
  { tag: t.heading, fontWeight: "600" },
  { tag: t.strong, fontWeight: "700" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: [t.link, t.url], color: "var(--link)" },
  { tag: t.monospace, fontFamily: "var(--font-mono)" },
  { tag: [t.processingInstruction, t.meta, t.contentSeparator], color: "var(--faint)" },
  { tag: [t.keyword, t.modifier, t.operatorKeyword, t.controlKeyword, t.definitionKeyword, t.moduleKeyword], color: "#c792ea" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "#a5d6a0" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "#f0a36b" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "#6b7a94", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], color: "#7fb4f5" },
  { tag: [t.typeName, t.className, t.namespace], color: "#6fd3c7" },
  { tag: [t.propertyName, t.attributeName], color: "#8fc7ee" },
  { tag: t.tagName, color: "#f07f8f" },
  { tag: [t.operator, t.punctuation, t.bracket], color: "#9fb0cc" },
  { tag: t.definition(t.variableName), color: "#dfe6f2" },
  { tag: t.invalid, color: "#ff7b7b" },
]);

function inCode(state: EditorState, pos: number): boolean {
  for (let n: ReturnType<typeof syntaxTree>["topNode"] | null = syntaxTree(state).resolveInner(pos, -1); n; n = n.parent) {
    if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "Frontmatter") return true;
  }
  return false;
}

/** Enter inside a list item, task or quote continues it; on an empty item it removes the marker. */
const continueMarkup: Command = (view) => {
  const { state } = view;
  const sel = state.selection.main;
  if (!sel.empty || state.selection.ranges.length > 1) return false;
  const line = state.doc.lineAt(sel.head);
  const pre = listContinuation(line.text);
  if (!pre || sel.head < line.from + pre.len || inCode(state, sel.head)) return false;
  if (line.text.length === pre.len) {
    view.dispatch({ changes: { from: line.from, to: line.to, insert: "" }, selection: { anchor: line.from }, userEvent: "delete" });
    return true;
  }
  const insert = state.lineBreak + pre.next;
  view.dispatch({
    changes: { from: sel.head, insert },
    selection: { anchor: sel.head + insert.length },
    scrollIntoView: true,
    userEvent: "input",
  });
  return true;
};

const tabKey: Command = (view) => {
  const { state } = view;
  const line = state.doc.lineAt(state.selection.main.head);
  if (!state.selection.main.empty || listContinuation(line.text)) return indentMore(view);
  return insertTab(view);
};

function toggleWrap(mark: string): Command {
  return (view) => {
    const { state } = view;
    const n = mark.length;
    view.dispatch(
      state.changeByRange((r) => {
        const before = state.sliceDoc(r.from - n, r.from);
        const after = state.sliceDoc(r.to, r.to + n);
        if (before === mark && after === mark) {
          return {
            changes: [
              { from: r.from - n, to: r.from },
              { from: r.to, to: r.to + n },
            ],
            range: EditorSelection.range(r.from - n, r.to - n),
          };
        }
        return {
          changes: [
            { from: r.from, insert: mark },
            { from: r.to, insert: mark },
          ],
          range: EditorSelection.range(r.from + n, r.to + n),
        };
      }),
    );
    return true;
  };
}

const previewCompartment = new Compartment();
const extrasCompartment = new Compartment();
let previewExt: Extension = livePreview();
let previewOn = true;
let extrasExt: Extension = [];

/** Installs lazily-loaded extensions (search, completion). Existing states pick them up via `syncEffects`. */
export function setExtras(ext: Extension): void {
  extrasExt = ext;
}

export function isPreviewOn(): boolean {
  return previewOn;
}

export function setPreview(on: boolean): void {
  previewOn = on;
  previewExt = on ? livePreview() : [];
}

/** Effects that bring a (possibly stale) state up to date with the global editor configuration. */
export function syncEffects(state: EditorState): StateEffect<unknown>[] {
  const out: StateEffect<unknown>[] = [];
  if (extrasCompartment.get(state) !== extrasExt) out.push(extrasCompartment.reconfigure(extrasExt));
  if (previewCompartment.get(state) !== previewExt) out.push(previewCompartment.reconfigure(previewExt));
  return out;
}

const sharedByHost = new WeakMap<EditorHost, Extension>();

function shared(host: EditorHost): Extension {
  let ext = sharedByHost.get(host);
  if (!ext) {
    ext = [
      history(),
      EditorView.lineWrapping,
      rocksyMarkdown,
      syntaxHighlighting(highlight),
      indentUnit.of("\t"),
      EditorState.tabSize.of(4),
      flashField,
      termHighlight(),
      Prec.high(
        keymap.of([
          { key: "Enter", run: continueMarkup },
          { key: "Tab", run: tabKey, shift: indentLess },
          { key: "Mod-b", run: toggleWrap("**") },
          { key: "Mod-i", run: toggleWrap("*") },
          { key: "ArrowDown", run: (v) => enterBlock(v, 1) },
          { key: "ArrowUp", run: (v) => enterBlock(v, -1) },
        ]),
      ),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      EditorView.updateListener.of((u) => host.onUpdate(u)),
      EditorView.contentAttributes.of({ spellcheck: "false", autocorrect: "off", autocapitalize: "off" }),
      EditorView.theme({}, { dark: true }),
    ];
    sharedByHost.set(host, ext);
  }
  return ext;
}

export function createEditorState(text: string | Text, path: string, host: EditorHost): EditorState {
  return EditorState.create({
    doc: text,
    extensions: [lpConfig.of({ host, path }), shared(host), previewCompartment.of(previewExt), extrasCompartment.of(extrasExt)],
  });
}

export function createEditorView(parent: HTMLElement, state: EditorState): EditorView {
  return new EditorView({ state, parent });
}

/** Scrolls to a 0-based line range and flashes it (tag fragments, heading links). */
export function revealLines(view: EditorView, startLine: number, endLine: number, col = 0, len = 0): void {
  const doc = view.state.doc;
  const a = doc.line(Math.min(doc.lines, Math.max(1, startLine + 1)));
  const b = doc.line(Math.min(doc.lines, Math.max(1, endLine + 1)));
  const anchor = Math.min(a.to, a.from + col);
  const head = Math.min(a.to, anchor + Math.max(0, len));
  view.dispatch({
    selection: { anchor, head },
    effects: [EditorView.scrollIntoView(a.from, { y: "start", yMargin: 80 }), flashEffect.of({ from: a.from, to: b.to })],
  });
  // A newer reveal on the same view must not be cut short by the timer of the previous one.
  clearTimeout(flashTimers.get(view));
  flashTimers.set(
    view,
    window.setTimeout(() => {
      try {
        view.dispatch({ effects: flashEffect.of(null) });
      } catch {
        /* view destroyed */
      }
    }, 2600),
  );
}

const flashTimers = new WeakMap<EditorView, number>();

export function refreshPreview(view: EditorView): void {
  view.dispatch({ effects: refreshEffect.of(null) });
}

/** Finds the 0-based line of a heading by its text (for `[[note#heading]]`). */
export function findHeadingLine(doc: Text, heading: string): number {
  const want = heading.trim().toLowerCase();
  for (let i = 1; i <= doc.lines; i++) {
    const m = /^#{1,6}[ \t]+(.*?)[ \t]*#*$/.exec(doc.line(i).text);
    if (m && m[1].trim().toLowerCase() === want) return i - 1;
  }
  return -1;
}
