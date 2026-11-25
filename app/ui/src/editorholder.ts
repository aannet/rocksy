// Which document's live state the editor view of a pane holds (docs/specs/saving.md, SV-18). No DOM here: the pane
// (app.ts) supplies its view; the tests drive this module with real editor states and a stand-in for the view.
//
// A document that is not in the view keeps a snapshot in `doc.state`. The document whose state is in the view is
// recorded here when the view is given it, never deduced from the active tab: the active tab can be a note still
// loading (the view still holds the previous one), and the note shown again must not get its snapshot back.
import type { EditorState, StateEffect, TransactionSpec } from "@codemirror/state";

export interface HeldDoc<P> {
  pane: P;
  kind: string;
  loaded: boolean;
  state: EditorState | null;
  scrollTop: number;
}

/** What the holder needs of an `EditorView`. */
export interface HeldView {
  readonly state: EditorState;
  setState(state: EditorState): void;
  dispatch(spec: TransactionSpec): void;
  readonly scrollDOM: { scrollTop: number };
}

export class EditorHolder<P, D extends HeldDoc<P>, V extends HeldView> {
  private held: D | null = null;

  constructor(
    private readonly pane: P,
    /** Compartment updates a state must get before it is shown (preview mode, extras). */
    private readonly syncEffects: (state: EditorState) => StateEffect<unknown>[] = () => [],
    /** Whether the document still has a tab in the pane: a closed note gets no edit, save or stash from the view. */
    private readonly isOpen: (doc: D) => boolean = () => true,
  ) {}

  /** The document whose live state is in the view (null when none, when that document went to another pane, or was closed). */
  holder(): D | null {
    const d = this.held;
    return d && d.pane === this.pane && d.kind === "md" && d.loaded && d.state && this.isOpen(d) ? d : null;
  }

  holds(doc: D): boolean {
    return this.holder() === doc;
  }

  /** The current state of `doc`: the view's when the view holds it, its snapshot otherwise. */
  stateOf(view: V | null, doc: D): EditorState | null {
    return view && this.holds(doc) ? view.state : doc.state;
  }

  /** The view to dispatch to for `doc`, or null when its state is the snapshot. */
  viewOf(view: V | null, doc: D): V | null {
    return view && this.holds(doc) ? view : null;
  }

  /** Copies the live state back into the document the view holds; the scroll position only when the editor is shown. */
  stash(view: V | null, visible: boolean): void {
    const d = this.holder();
    if (!view || !d) return;
    d.state = view.state;
    if (visible) d.scrollTop = view.scrollDOM.scrollTop;
  }

  /**
   * Puts `doc` (a loaded Markdown note) in the view, created by `create` when there is none. The document the view
   * already holds is left as it is; another one is stashed first. `replaced`: the view got a new state (its scroll
   * position is then to be restored by the caller).
   */
  give(view: V | null, doc: D, visible: boolean, create: (state: EditorState) => V): { view: V; replaced: boolean } {
    if (!doc.state) throw new Error("give: a document without state");
    if (view && this.holds(doc)) {
      const fx = this.syncEffects(view.state);
      if (fx.length) view.dispatch({ effects: fx });
      return { view, replaced: false };
    }
    this.stash(view, visible);
    const fx = this.syncEffects(doc.state);
    if (fx.length) doc.state = doc.state.update({ effects: fx }).state;
    if (view) view.setState(doc.state);
    else view = create(doc.state);
    this.held = doc;
    return { view, replaced: true };
  }

  /** The view was destroyed, or the document it held went elsewhere. */
  release(): void {
    this.held = null;
  }
}
