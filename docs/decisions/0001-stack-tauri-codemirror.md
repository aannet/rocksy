# ADR 0001: UI stack: Tauri 2 and CodeMirror 6

Status: accepted
Date: 2026-11-23

## Context

The first plan assumed compatibility with Obsidian community plugins would drive the stack. On 2026-11-23 the owner made that compatibility secondary: the app gets its own plugin system with hot loading, and an Obsidian-plugin importer may come later. That reopened the UI choice between a WebView (Tauri 2 with CodeMirror 6) and a native GPU-rendered UI (Rune). Live Preview was an eliminating criterion for the comparison.

## Decision

Release 0.1 and later are built on Tauri 2 with CodeMirror 6. The Rune spike (spike B) was stopped at its entry gate, after a probe built and run on Windows. The rule applied: a stack that does not cover F1 (inline Live Preview) and F2 (blocks in the text flow) is dropped whatever its startup time. The owner validated the stack on 2026-11-23. Compatibility with Obsidian plugins stays optional and later (a shim of the public `obsidian` API or an importer); the app's own mod system comes first (see ADR 0021).

## Consequences

- The UI is HTML/CSS/JS in a WebView, without a UI framework (direct DOM). WebView2 startup is the main startup cost: the Rune probe created its window in about 233 ms against about 0.7 s for the assembled app (orders of magnitude, not a comparison at equal scope).
- Rune may be revisited if WebView2 startup becomes the limiting factor (`docs/archive/m2-resultats.md`).
- Mermaid is rendered in the WebView (ADR 0024).

## Sources

Journal 2026-11-23 11:35 and the M2 results in `docs/archive/m2-resultats.md` (section 1, owner validation); `docs/archive/session-2026-11-23/etat.js`.
