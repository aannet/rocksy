# ADR 0020: Functional scope of version 1

Status: accepted
Date: 2026-11-24

## Context

The owner's top irritants of Obsidian are slow start and no way to use it as a plain Markdown reader without a vault.

## Decision

Live Preview is mandatory in v1 (eliminating criterion of the M2 comparison). Canvas is read-only in v1. No mobile for now (keep the core UI-independent and portable). Tag management must be as powerful as Obsidian's: nested tags, tag pane with counts, `tag:` search, TagFolder, global rename as an admin mod. Tags apply to fragments of a file: the index stores positions and results show the fragment, which is the enclosing block (paragraph, or list item with its children); clicking a result opens the note at that spot; search over the in-memory tag library is instant. Rocksy opens any Markdown file outside a vault (command-line argument and double click; single instance: a second launch opens a tab in the running process; no vault, index or `.obsidian/` created next to the file; fastest startup path). Obsidian Sync and Publish are out of scope.

## Consequences

- Tag rename and TagFolder are not implemented (`docs/status.md`).
- This is a status of the decision as written by the owner on 2026-11-24 in the project notes; the draft is `docs/archive/perimetre-fonctionnel.md`.

## Sources

The functional-scope paragraph of the previous `CLAUDE.md` (in git history); `docs/archive/perimetre-fonctionnel.md`; `app/CONTRACT.md` (Tag rules).
