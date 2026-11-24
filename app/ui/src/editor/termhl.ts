// Highlights the terms of the last full-text search in the open note (visible ranges only).
import { type Extension, type Range, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { foldText, termRanges } from "../excerpt";

export const setTermsEffect = StateEffect.define<readonly string[]>();

export const termsField = StateField.define<readonly string[]>({
  create: () => [],
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setTermsEffect)) value = e.value;
    return value;
  },
});

const mark = Decoration.mark({ class: "cm-term-hl" });

function build(view: EditorView): DecorationSet {
  const terms = view.state.field(termsField);
  if (!terms.length) return Decoration.none;
  const out: Range<Decoration>[] = [];
  for (const { from, to } of view.visibleRanges) {
    const folded = foldText(view.state.sliceDoc(from, to));
    for (const t of terms) for (const [a, b] of termRanges(folded, t, false, 500)) out.push(mark.range(from + a, from + b));
  }
  return Decoration.set(out, true);
}

const plugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view);
    }
    update(u: ViewUpdate) {
      const changed = u.state.field(termsField) !== u.startState.field(termsField);
      if (changed || ((u.docChanged || u.viewportChanged) && u.state.field(termsField).length)) this.decorations = build(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

export function termHighlight(): Extension {
  return [termsField, plugin];
}
