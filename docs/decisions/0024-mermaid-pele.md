# ADR 0024: Mermaid rendered by Pele, loaded on demand

Status: accepted
Date: 2026-11-23

## Context

Obsidian 1.15 replaced Mermaid.js with Pele (`obsidianmd/pele`, MIT, zero dependencies, synchronous text to SVG, all Mermaid 12.1.0 types except ZenUML). It is trivial in a WebView.

## Decision

Mermaid blocks are rendered by Pele 1.13 (npm package, MIT), loaded on demand. Canvas read-only view and two panes were included in 0.1 although they were planned later; they pose no risk to the vault (no write into a `.canvas`, checked byte for byte).

## Consequences

- Pele emits SVG with `style` attributes: this is why `style-src` keeps `'unsafe-inline'` (ADR 0011).
- The 9 Mermaid types of the synthetic vault rendered without errors in the 0.2.1 integration run.

## Sources

Journal 2026-11-23 12:00 and 12:27; `runs/2026-11-23_2240_integration-audits/notes.md`; previous `CLAUDE.md` (Mermaid).
