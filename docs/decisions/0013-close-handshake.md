# ADR 0013: Three-step close handshake with a 5 s guard

Status: accepted
Date: 2026-11-23

## Context

The robustness audit found text could be lost when closing right after typing, and the coherence analysis warned that a frozen UI would make the window impossible to close.

## Decision

Closing the window does not close it: the backend emits `close-requested`; the UI answers `close_ack` immediately, saves every modified document, asks for confirmation (save / quit without saving / cancel, in an app dialog, never `window.confirm`) only if a save failed, a document is in conflict, or autosave is off with unsaved changes, then calls `quit`. Without `close_ack` within 5 s the backend closes the app anyway. `quit` stops the watcher, cancels the running indexation (waiting 2 s at most), persists the index, then exits.

## Consequences

- Covered by the closing-sequence unit tests in `app/ui/tests/sync.test.ts`; the real-app check was done on synthetic vaults (`runs/2026-11-23_2240_integration-audits/notes.md`).
- Details: `docs/specs/shutdown.md`.

## Sources

Journal 2026-11-23 18:28; `docs/archive/audits/2026-11-23_contrat-delta.md` section 5; `app/CONTRACT.md` (Closing).
