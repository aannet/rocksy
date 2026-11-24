# ADR 0004: Startup order and the 20x scale requirement

Status: accepted
Date: 2026-11-22

## Context

Obsidian's baseline start on the owner's vault was 5,984 ms, 67 % from plugins. The first recursive listing of the real vault on Google Drive takes 57 s cold against about 1.3 s warm. On 2026-11-22 the owner required the same startup performance with 20x more files (about 10k notes, about 60 MB of text, about 19k files).

## Decision

Startup cost must not grow with vault size. Order: (1) read the file of the active leaf or the first command-line file and show it without waiting for index or plugins; (2) build the tree from a persisted snapshot, independent of the index, then reconcile in the background; (3) load the persisted index and reconcile in the background, rate-limited; (4) nothing else loads before the first render. `StartupInfo.firstFile` lets the backend read the first note while the WebView starts. A file passed on the command line opens alone; the session's vault stays noted and loads if one of its tabs is activated.

## Consequences

- The tree appears from the snapshot (4.6 ms for 20,747 entries on the core side in the M2 run) and the listing confirms it afterwards (`fromSnapshot`, `tree-updated`).
- Measured only on synthetic vaults on a local disk with a warm cache; the real vault and cold start are unverified (`docs/status.md`).
- Details: `docs/specs/startup.md`.

## Sources

Previous `CLAUDE.md` (audit findings and scale requirement, kept in git history); `app/ui/README.md` (startup order); `app/CONTRACT.md` (Startup); `docs/archive/m2-resultats.md`.
