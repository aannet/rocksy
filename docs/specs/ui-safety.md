# Spec: UI safety

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; invariants taken from the contract, the UI README and the tests as they stand)

## Scope

What the page must never do with hostile or pathological content: canvas, inline Markdown rendering, remote and network resources, content security policy. Rationale: `docs/decisions/0011-page-access-policy-and-csp.md`. File access is in `docs/specs/file-access.md`.

## Content security policy

`default-src 'none'`; `script-src 'self'` (no `'unsafe-inline'`, no `'unsafe-eval'`); `style-src 'self' 'unsafe-inline'`; `img-src 'self' asset: http://asset.localhost data: blob:`; `font-src 'self' data:`; `connect-src ipc: http://ipc.localhost`; `object-src`, `base-uri`, `form-action`, `frame-src`, `media-src`, `worker-src` all `'none'`. The configuration is in `app/src-tauri/tauri.conf.json`. The UI records any violation in `window.rocksyCsp`; none is expected in normal use.

## Invariants

| # | Invariant | Enforced by |
|---|---|---|
| UI-1 | No markup is built from canvas data; nodes and edges are normalised to typed values; legitimate canvases are unchanged | `never builds markup from canvas data`, `normalizes nodes and edges to typed values`, `keeps legitimate canvases intact` (`app/ui/tests/security.test.ts`) |
| UI-2 | Canvas file references stay under their base folder; an escaping path is never handed to the host (no read, no image load, no click target) | `refuses paths that climb out, are absolute or name a drive`, `never hands an escaping path to the host (no read, no image load, no click target)`; `a canvas file is resolved by the vault index and stays under the vault root`, `outside a vault, a canvas file stays under the folder of the canvas` (`app/ui/tests/sync.test.ts`) |
| UI-3 | Network sources (UNC, device, `file://host`) are never resolved as loadable images or links; local absolute paths and `file:` URLs still resolve | `resolveImage refuses UNC, device and file://host sources`, `still resolves local absolute paths and file: URLs`, `isNetworkPath / isAbsolute`, `recognises network sources of links and images` |
| UI-4 | Inline rendering of a hostile table cell stays fast (8,000 characters in under 100 ms, as escaped plain text; the link alternative is not cubic; Markdown links with balanced parentheses still render) | `renders an 8 000 character cell in under 100 ms, as escaped plain text`, `the link alternative itself is not cubic (cell just under the 2 000 character cap)`, `keeps the rendering of Markdown links, with balanced parentheses in the destination` |
| UI-5 | A single huge line is left in source style rather than decorated | `leaves huge single lines in source style` (`app/ui/tests/preview.test.ts`). Typing latency at the end of a single 216,000-character line is a known limit: median 76 ms (`docs/status.md`) |
| UI-6 | Decorations are limited to the requested ranges (viewport) | `only decorates the requested ranges` |
| UI-7 | Remote images (`http`, `https`) are never loaded; a message replaces them | no unit test; real app: scenario `a5-security` and the 0.2.1 run (`runs/2026-11-23_2240_integration-audits/notes.md` section 2) |
| UI-8 | No CSP violation occurs when browsing the editor, Live Preview, vault images, tables, the 9 Mermaid types of the synthetic vault, a dense canvas, the panels and dialogs; injected script, `eval`, `fetch` and similar are blocked | real app only: scenarios `a5-csp` and `a5-security` (`app/ui/tools/app/a5-csp.json`); the harness reports `window.rocksyCsp` at the end of every scenario. Not a unit test |
| UI-9 | Every visible HTML string built by the UI from file content is escaped | `renders inline markdown to escaped HTML` (`app/ui/tests/logic.test.ts`); `escapes HTML around the marks`. Not covered by a test for every call site |
| UI-10 | Opening an external link goes through `open_external` (Tauri); `window.open` only in the plain-browser bench | not covered by a test |
| UI-11 | The WebView spell checker is off in the editor; UI preferences are in `localStorage`, never in the vault | not covered by a test |

## Code

`app/ui/src/canvas.ts`, `app/ui/src/resolve.ts`, `app/ui/src/paths.ts`, `app/ui/src/editor/syntax.ts`, `app/ui/src/editor/livepreview.ts`, `app/src-tauri/tauri.conf.json`.
