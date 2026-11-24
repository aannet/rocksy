# ADR 0010: Derived data in local app data, purged with the vault

Status: accepted
Date: 2026-11-23

## Context

Indexes contain note names and note text. Putting them in the roaming profile exposes them to roaming and backup; putting them in the vault would break the rule of ADR 0003.

## Decision

Two folders: the roaming one (`%APPDATA%\dev.rocksy.app`) holds the vault list, the session, the explicitly opened files list and `timings.jsonl` (no note content); the local one (`%LOCALAPPDATA%\dev.rocksy.app`) holds tree snapshots, the tags and links index and the search index. Data of an earlier version found in the roaming folder is moved at first launch without a rebuild. `ROCKSY_DATA_DIR` puts both in one folder (tests, measurements). `remove_vault` deletes the removed vault's derived data. The identifier `dev.rocksy.app` is provisional (journal: to review before publication).

## Consequences

- Going back to a 0.2.0 exe after 0.2.1 forces a full reindex.
- The real migration on a real profile was never tried (`docs/status.md`).
- The local folder must be treated like the notes if backed up or shared.

## Sources

Journal 2026-11-23 11:39 (data folder), 12:48 (index in data folder); `app/CONTRACT.md` (Application data); SEC-7 in `docs/archive/audits/2026-11-23_securite.md` and the contract delta section 3. The decision to move to the local folder has no journal decision entry of its own: it is recorded in the contract delta and the 0.2.1 release notes.
