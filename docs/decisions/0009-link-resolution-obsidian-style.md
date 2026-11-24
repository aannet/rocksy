# ADR 0009: Link resolution follows Obsidian

Status: accepted
Date: 2026-11-23

## Context

The vault must behave as in Obsidian when opened in Rocksy.

## Decision

Wikilink targets resolve by exact path from the vault root, then path suffix, then file name (`.md` optional), case-insensitively; on ambiguity the shortest path wins, then alphabetical order. Markdown links resolve relative to the note's folder, then from the root. Attachments get backlinks too (this prepares the orphan-media scan job). Not done in this stage: creating a note from an unresolved link, and updating links on rename (they are vault writes, to be specified and tested separately).

## Consequences

- Resolution covers all visible files (notes and attachments) and is recomputed when files appear or disappear, without re-reading notes.
- Details: `docs/specs/vault-index.md`.

## Sources

Journal 2026-11-23 14:24; `app/CONTRACT.md` (Links); tests in `app/crates/rocksy-core/tests/vault_index.rs`.
