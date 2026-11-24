# ADR 0006: Change detection by size and mtime, without hashing

Status: accepted
Date: 2026-11-23

## Context

Mtimes change in bulk (an AI cleaner runs on one of the synced machines and can rewrite or move many files at once; 321 of 488 notes showed November 2026 in the audit), and reading a file is what costs on Drive.

## Decision

Changes are detected by size + mtime; no content hash is computed. The persisted index file is written after a reconciliation that re-read or removed notes, at vault or app close, and otherwise at most every 60 s. At close the app cancels the running indexation, waits for it, then saves what was already read, so an interrupted first indexation does not restart from zero.

## Consequences

- Accepted limit: a file modified without any change of size or mtime is not seen before its next modification.
- Tags are persisted in the local data folder, one file per vault (ADR 0010). It contains note excerpts (to show fragments without re-reading).
- If the app is killed between persistences, the affected notes are simply re-read at the next launch.

## Sources

Journal 2026-11-23 12:48 and 13:10; `app/CONTRACT.md` (Vault index); `app/crates/rocksy-core/src/tags/persist.rs` (format header comment).
