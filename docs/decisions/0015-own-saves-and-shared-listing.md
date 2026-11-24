# ADR 0015: The app ignores its own saves in the watcher; one listing is shared by tree and index

Status: accepted
Date: 2026-11-23

## Context

A save by the app produces a rename event about 400 ms later, which used to trigger a full listing and reconciliation (performance audit PERF-1).

## Decision

The app records its own writes (path, size, mtime) and skips the listing when a batch contains only own writes that still match the file's stat. Any other path, a created file, a folder rename, an expired record or a changed stat sends the batch through the normal path. The records expire, are bounded and are compared with case-insensitive Windows path keys. The structure lives in the core (`app/crates/rocksy-core/src/own_writes.rs`) so it has unit tests. A save of an existing file triggers no listing and no `tree-updated`. One listing of the vault serves both the tree and the index reconcile.

## Consequences

- A real external change right after a save can be missed only if it keeps the same size and mtime, a hole that already exists elsewhere in the app (documented by the coherence analysis).
- Tests: `app/crates/rocksy-core/tests/own_writes_watch.rs`, `refresh_pass.rs`.
- Details: `docs/specs/external-changes.md`.

## Sources

`docs/archive/audits/2026-11-23_coherence.md` section 2.2; `docs/archive/audits/2026-11-23_contrat-delta.md` section 4; `app/CONTRACT.md` (External-change tracking). No journal decision entry of its own.
