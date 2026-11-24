# ADR 0008: Full-text search with tantivy, body stored, deferred commit

Status: accepted
Date: 2026-11-23

## Context

The plan named `tantivy`. The risk was that it would not compile with the cross-compilation chain for Windows without MSVC. Search semantics were an open question of the brief.

## Decision

`tantivy` 0.27 was kept after an entry gate of 20 minutes (it compiles and runs on Windows with the cross chain, no C dependency, about 100 more crates, 4.6 MB more on a test exe). The body is stored in the index so excerpts need no file read. Semantics: case and accent insensitive, words combined with AND, last word a prefix (search as you type), `"exact phrase"`, `-word`, `path:`, `tag:` (with descendants); no OR, no stemming; file name weighs more than body; limits capped at 1000. Saves update the index through a deferred commit: saved text may show up in search up to 2 s late.

## Consequences

- Index size on disk is about the size of the vault text.
- A killed app between a save and the deferred commit costs one read per saved note at next launch (`app/crates/rocksy-core/tests/index_engine.rs`).
- Damaged index handling: ADR 0014.

## Sources

Journal 2026-11-23 14:24, 14:35, 16:45; `app/CONTRACT.md` (Search); `app/crates/rocksy-core/src/search/`.
