# ADR 0023: English knowledge base, spec first, archive untouched

Status: accepted
Date: 2026-11-24

## Context

Knowledge was scattered between a long `CLAUDE.md`, French documents that looked current but were historical, and a prototype path for the product. Several LLMs are expected to keep developing the project over time.

## Decision

A short normative English knowledge base: `AGENTS.md` as the entry point, `app/CONTRACT.md` and `docs/specs/` as the specifications, `docs/decisions/` for decisions (immutable once accepted), `docs/impacts.md` for what else to update, `docs/runbooks/`, `docs/status.md`. Folder and document names are English; UI strings stay French. The product moved to `app/`. Historical documents are archived as they are, in French, untranslated, under `docs/archive/`. Work is spec first; a script (`tools/check-consistency.py`) catches broken rules instead of prose.

## Consequences

- Paths inside archived documents and runs refer to the old layout and are not rewritten.
- An accepted ADR is replaced only by a new one.

## Sources

The restructuring plan approved by the owner on 2026-11-24 (outside the repository).
