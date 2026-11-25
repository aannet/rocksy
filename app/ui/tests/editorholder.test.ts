// The editor view of a pane holds the live state of one document (docs/specs/saving.md, SV-18): showing that document
// again keeps the editor as it is, giving the view another document stashes it first. Real editor states (undo
// history included), a stand-in for the view.
import { describe, expect, it } from "vitest";
import { undo } from "@codemirror/commands";
import { type EditorState, StateEffect, type TransactionSpec } from "@codemirror/state";
import { createEditorState } from "../src/editor/editor";
import { EditorHolder, type HeldDoc, type HeldView } from "../src/editorholder";

type Pane = { name: string };
type Doc = HeldDoc<Pane> & { path: string };

const lp = { epoch: () => 0, isImage: () => false, resolveImage: () => ({ kind: "missing" as const }), linkExists: () => null, openLink: () => undefined, openTag: () => undefined, onUpdate: () => undefined };

class FakeView implements HeldView {
  scrollDOM = { scrollTop: 0 };
  replaced = 0;
  constructor(public state: EditorState) {}
  setState(state: EditorState) {
    this.state = state;
    this.replaced++;
  }
  dispatch(spec: TransactionSpec) {
    this.state = this.state.update(spec).state;
  }
  /** A keystroke: text inserted at the end, as the editor does it (it goes into the undo history). */
  type(text: string) {
    this.dispatch({ changes: { from: this.state.doc.length, insert: text }, userEvent: "input.type" });
  }
  undo(): boolean {
    return undo({ state: this.state, dispatch: (tr) => (this.state = tr.state) });
  }
}

function setup() {
  const pane: Pane = { name: "left" };
  const doc = (path: string, text: string): Doc => ({ path, pane, kind: "md", loaded: true, state: createEditorState(text, path, lp), scrollTop: 0 });
  const holder = new EditorHolder<Pane, Doc, FakeView>(pane);
  let view: FakeView | null = null;
  /** What `Pane.show` does for a loaded note. */
  const show = (d: Doc, visible = true) => {
    const r = holder.give(view, d, visible, (s) => new FakeView(s));
    view = r.view;
    return r.replaced;
  };
  const text = (d: Doc) => holder.stateOf(view, d)!.doc.toString();
  return { pane, doc, holder, show, text, view: () => view! };
}

describe("editor holder (SV-18)", () => {
  it("showing the note it holds again keeps the text, the undo history and the scroll position", () => {
    const s = setup();
    const a = s.doc("C:\\v\\A.md", "# A\n");
    expect(s.show(a)).toBe(true);
    const view = s.view();
    view.type("typed since the note was shown");
    view.scrollDOM.scrollTop = 480;
    // Its own tab clicked, or reopened from the tree, the quick switcher, a link, a search result.
    expect(s.show(a)).toBe(false);
    expect(view.replaced).toBe(0);
    expect(view.state.doc.toString()).toBe("# A\ntyped since the note was shown");
    expect(view.scrollDOM.scrollTop).toBe(480);
    // What a save writes.
    expect(s.text(a)).toBe("# A\ntyped since the note was shown");
    expect(view.undo()).toBe(true);
    expect(view.state.doc.toString()).toBe("# A\n");
  });

  it("switching to another note and back keeps what was typed, with its undo history", () => {
    const s = setup();
    const a = s.doc("C:\\v\\A.md", "# A\n");
    const b = s.doc("C:\\v\\B.md", "# B\n");
    s.show(a);
    s.view().type("one");
    s.view().scrollDOM.scrollTop = 300;
    s.show(b);
    expect(a.state!.doc.toString()).toBe("# A\none");
    expect(a.scrollTop).toBe(300);
    expect(s.text(a)).toBe("# A\none");
    expect(s.view().state.doc.toString()).toBe("# B\n");
    expect(s.show(a)).toBe(true);
    expect(s.view().state.doc.toString()).toBe("# A\none");
    expect(s.view().undo()).toBe(true);
    expect(s.view().state.doc.toString()).toBe("# A\n");
  });

  it("while the next tab loads, the keys typed in the editor belong to the note it still holds", () => {
    const s = setup();
    const a = s.doc("C:\\v\\A.md", "# A\n");
    const b: Doc = { path: "C:\\v\\B.md", pane: s.pane, kind: "md", loaded: false, state: null, scrollTop: 0 };
    s.show(a);
    // B is the active tab now, still being read: the view keeps A.
    s.view().type("k");
    expect(s.holder.holder()).toBe(a);
    expect(s.holder.stateOf(s.view(), b)).toBeNull();
    expect(s.holder.viewOf(s.view(), b)).toBeNull();
    expect(s.text(a)).toBe("# A\nk");
    b.loaded = true;
    b.state = createEditorState("# B\n", b.path, lp);
    s.show(b);
    expect(a.state!.doc.toString()).toBe("# A\nk");
  });

  it("a hidden editor does not overwrite the scroll position it had when shown", () => {
    const s = setup();
    const a = s.doc("C:\\v\\A.md", "# A\n");
    const b = s.doc("C:\\v\\B.md", "# B\n");
    s.show(a);
    s.view().scrollDOM.scrollTop = 700;
    s.holder.stash(s.view(), true);
    // An image or a canvas was shown meanwhile: the scroller of the hidden editor reads 0.
    s.view().scrollDOM.scrollTop = 0;
    s.show(b, false);
    expect(a.scrollTop).toBe(700);
  });

  it("a note that went to another pane is never stashed nor dispatched to by its old pane", () => {
    const s = setup();
    const a = s.doc("C:\\v\\A.md", "# A\n");
    const b = s.doc("C:\\v\\B.md", "# B\n");
    s.show(a);
    s.holder.stash(s.view(), true);
    a.pane = { name: "right" };
    a.state = createEditorState("# A edited in the other pane\n", a.path, lp);
    expect(s.holder.holder()).toBeNull();
    expect(s.holder.viewOf(s.view(), a)).toBeNull();
    s.show(b);
    expect(a.state!.doc.toString()).toBe("# A edited in the other pane\n");
  });

  it("a closed note is no longer held: keys typed before the next tab is read are neither its edits nor its saves", () => {
    const pane: Pane = { name: "left" };
    const tabs: Doc[] = [];
    const holder = new EditorHolder<Pane, Doc, FakeView>(pane, () => [], (d) => tabs.includes(d));
    const a: Doc = { path: "C:\\v\\A.md", pane, kind: "md", loaded: true, state: createEditorState("# A\n", "C:\\v\\A.md", lp), scrollTop: 0 };
    const b: Doc = { path: "C:\\v\\B.md", pane, kind: "md", loaded: false, state: null, scrollTop: 0 };
    tabs.push(a, b);
    const view = holder.give(null, a, true, (st) => new FakeView(st)).view;
    view.type("saved");
    holder.stash(view, true);
    // Its tab is closed while B, the next tab, is still being read: the view still shows the text of A.
    tabs.splice(tabs.indexOf(a), 1);
    expect(holder.holder()).toBeNull();
    view.type(" typed after the close");
    expect(holder.viewOf(view, a)).toBeNull();
    expect(holder.stateOf(view, a)!.doc.toString()).toBe("# A\nsaved");
    b.loaded = true;
    b.state = createEditorState("# B\n", b.path, lp);
    expect(holder.give(view, b, true, () => view).replaced).toBe(true);
    expect(a.state!.doc.toString()).toBe("# A\nsaved");
    expect(view.state.doc.toString()).toBe("# B\n");
  });

  it("gives the compartment updates to the live view, or to the snapshot it puts in the view", () => {
    const mark = StateEffect.define<null>();
    let pending = false;
    const seen: StateEffect<unknown>[][] = [];
    const s = setup();
    const holder = new EditorHolder<Pane, Doc, FakeView>(s.pane, () => (pending ? [mark.of(null)] : []));
    const a = s.doc("C:\\v\\A.md", "# A\n");
    const created = holder.give(null, a, true, (st) => new FakeView(st)).view;
    const dispatch = created.dispatch.bind(created);
    created.dispatch = (spec) => {
      seen.push([spec.effects ?? []].flat() as StateEffect<unknown>[]);
      dispatch(spec);
    };
    created.type("x");
    seen.length = 0;
    pending = true;
    expect(holder.give(created, a, true, () => created).replaced).toBe(false);
    expect(seen).toHaveLength(1);
    expect(seen[0][0].is(mark)).toBe(true);
    expect(created.state.doc.toString()).toBe("# A\nx");
  });
});
