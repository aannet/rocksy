# Spec: shutdown

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; invariants taken from the contract, the UI README and the tests as they stand)

## Scope

Closing the window: no text lost, no frozen window, index persisted. Rationale: `docs/decisions/0013-close-handshake.md`. Contract: "Closing" in `app/CONTRACT.md`.

## Sequence

1. The user closes the window; the backend does not close it and emits `close-requested`.
2. The UI calls `close_ack` at once. Without `close_ack` within 5 s, the backend closes the app.
3. The UI saves every modified document and waits for each save. If a save fails, a document is in conflict, or autosave is off with changes left, it asks (save / quit without saving / cancel) in an app dialog.
4. The UI calls `quit`. The backend stops the watcher, cancels the running indexation (waits 2 s at most), persists the index, exits.

## Invariants

| # | Invariant | Enforced by |
|---|---|---|
| SH-1 | The UI acknowledges first, saves every modified document, and only then quits | `acknowledges first, saves every modified document, and only then quits` (`app/ui/tests/sync.test.ts`) |
| SH-2 | A save already in flight is awaited | `waits for a save that was already in flight` |
| SH-3 | A failed save asks; cancelling keeps the app open and the text; "save and quit" failing asks again; quitting without saving happens only on request | `asks when a save fails; cancelling keeps the application open`, `asks again when 'save and quit' fails too, and quits without saving only on request` |
| SH-4 | With autosave off, nothing is written without asking | `with autosave off, nothing is written without asking` |
| SH-5 | A conflicting document is never overwritten on close: only "quit without saving" or "cancel" are offered | `never overwrites a conflicting document: only 'quit without saving' or 'cancel' are offered` |
| SH-6 | Repeated close requests are each acknowledged, with one dialog only | `several close requests in a row: each is acknowledged, one dialog only` |
| SH-7 | Closing a tab while its save is in flight waits for the outcome and keeps the text if the save fails | `waits for the outcome and keeps the text when the save fails`, `reports nothing left when the save in flight succeeds` |
| SH-8 | The 5 s guard, the watcher stop, the 2 s indexation cancel and the final persist are done by the backend | not covered by a unit test (the shell has none on Linux; constant `CLOSE_ACK_TIMEOUT` in `app/src-tauri/src/main.rs`). Real app: scenarios `a5-loss`, `a5-robust-x20-quit` (`app/ui/tools/app/`) |
| SH-9 | A cancelled indexation keeps a consistent, resumable index and removes nothing; pending search changes are committed when the engine is released | `a_cancelled_pass_removes_nothing_and_leaves_a_state_that_resumes`, `pending_search_changes_are_committed_when_the_engine_is_released`, `cancelled_reconcile_keeps_a_consistent_index_and_a_loadable_checkpoint` |
| SH-10 | The 0.2.1 integration run observed: close right after typing exits in 125 ms with the text on disk; closing during the first indexation of x20 exits in 365 ms and the next launch reloads the already indexed notes. These are observations on synthetic vaults, not tests | `runs/2026-11-23_2240_integration-audits/notes.md` sections 3 and 4 |

## Code

`app/ui/src/docsync.ts`, `app/ui/src/app.ts`, `app/src-tauri/src/main.rs` (`close_ack`, `quit`, `finalize`).
