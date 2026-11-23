// Loaded after the first paint: in-note search/replace and completions (wikilinks, tags).
import type { Extension } from "@codemirror/state";
import { EditorState } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { search, searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, completionKeymap, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";

export interface CompletionHost {
  /** Files matching the text typed after `[[` (best first). `insert` is the link text. */
  completeFiles(query: string): { label: string; insert: string; detail: string }[];
  completeTags(query: string): Promise<{ tag: string; count: number }[]>;
}

const phrases = {
  Find: "Rechercher",
  Replace: "Remplacer",
  next: "suivant",
  previous: "précédent",
  all: "tout",
  "match case": "casse",
  regexp: "regex",
  "by word": "mot entier",
  replace: "remplacer",
  "replace all": "tout remplacer",
  close: "fermer",
  "current match": "occurrence courante",
  "replaced $ matches": "$ occurrences remplacées",
  "replaced match on line $": "occurrence remplacée ligne $",
  "on line": "ligne",
  "Go to line": "Aller à la ligne",
  go: "aller",
};

function inCodeNode(state: EditorState, pos: number): boolean {
  for (let n = syntaxTree(state).resolveInner(pos, -1) as { name: string; parent: unknown } | null; n; n = n.parent as typeof n) {
    if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "InlineCode") return true;
  }
  return false;
}

function wikilinkSource(host: CompletionHost) {
  return (cx: CompletionContext): CompletionResult | null => {
    const line = cx.state.doc.lineAt(cx.pos);
    const before = line.text.slice(0, cx.pos - line.from);
    const open = before.lastIndexOf("[[");
    if (open < 0) return null;
    const typed = before.slice(open + 2);
    if (typed.includes("]]") || typed.includes("|") || typed.includes("#")) return null;
    const from = line.from + open + 2;
    const closing = cx.state.sliceDoc(cx.pos, cx.pos + 2) === "]]";
    const files = host.completeFiles(typed);
    return {
      from,
      filter: false,
      options: files.map((f, i) => ({
        label: f.label,
        detail: f.detail,
        type: "file",
        boost: -i,
        apply: (view, _c, a, b) => {
          const insert = f.insert + (closing ? "" : "]]");
          view.dispatch({ changes: { from: a, to: b, insert }, selection: { anchor: a + f.insert.length + 2 } });
        },
      })),
    };
  };
}

function tagSource(host: CompletionHost) {
  return async (cx: CompletionContext): Promise<CompletionResult | null> => {
    const m = cx.matchBefore(/(?:^|(?<=\s))#[\p{L}\p{N}_\-/]*/u);
    if (!m || (m.from === m.to && !cx.explicit)) return null;
    if (inCodeNode(cx.state, cx.pos)) return null;
    // "# " is a heading being typed, not a tag.
    const typed = m.text.slice(1);
    if (!typed && !cx.explicit && cx.state.doc.lineAt(cx.pos).from === m.from) return null;
    const tags = await host.completeTags(typed);
    if (cx.aborted) return null;
    return {
      from: m.from + 1,
      filter: false,
      options: tags.slice(0, 60).map((t, i) => ({ label: t.tag, detail: String(t.count), type: "tag", boost: -i })),
    };
  };
}

export function extras(host: CompletionHost): Extension {
  return [
    EditorState.phrases.of(phrases),
    search({ top: true }),
    highlightSelectionMatches({ minSelectionLength: 2 }),
    autocompletion({ override: [wikilinkSource(host), tagSource(host)], icons: false, activateOnTyping: true, maxRenderedOptions: 40 }),
    keymap.of([...searchKeymap, ...completionKeymap]),
  ];
}
