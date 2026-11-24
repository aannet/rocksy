# ADR 0007: The folder watcher is a trigger only

Status: accepted
Date: 2026-11-23

## Context

It is not guaranteed that the virtual drive of Google Drive emits reliable notifications, and that could not be tested. Dot-folders hold 79 % of the files of the real vault, and the AI cleaner rewrites hundreds of files at once.

## Decision

The `notify` watcher only triggers a check. On each batch the backend lists the vault once (at most one pass per second), emits `tree-updated` if the tree changed, then reconciles the index against that same listing. The listing stays the source of truth. Events of dot-folders are ignored; bursts are grouped (400 ms of silence, 3 s at most). A safety net reruns the same pass when the window regains focus if the last one is older than 30 s. Renames are not tracked: a rename is a deletion plus an addition. Open files are polled every 2 s for `file-changed`.

## Consequences

- A missed event delays an update instead of losing it.
- The behaviour on Google Drive itself was not verified (`docs/status.md`).
- Details: `docs/specs/external-changes.md`.

## Sources

Journal 2026-11-23 11:39 (polling), 13:22 (trigger only, bursts, renames) and 13:37 (notify added); `app/CONTRACT.md` (External-change tracking).
