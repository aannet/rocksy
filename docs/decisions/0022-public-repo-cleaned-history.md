# ADR 0022: Public repository with a cleaned history

Status: accepted
Date: 2026-11-23

## Context

The repository went public. It had held a vault audit, the real names of the vault and of the working note, and a personal author address.

## Decision

The public repository was published, at the owner's request, from a copy of the history cleaned beforehand: the vault audit removed, the working note name and the vault name replaced by placeholders, the author address replaced by a noreply address. Tracked documents use the placeholders `G:\Mon Drive\MonVault` and `01__WORK/daily-notes.md`. The vault audit files stay on disk, untracked on purpose (`.git/info/exclude`). `tools/check-consistency.py` can verify that no tracked file contains private terms listed in an untracked `.private-terms` file.

## Consequences

- Never `git add -f` the audit files; never write private names in tracked files, including in tests and fixtures.
- The cleaning procedure itself was not written down; only its outcome is recorded.
- The licence of the repository is not chosen (`docs/handoffs/code-signing.md`).

## Sources

Journal 2026-11-23 23:16; `docs/archive/PLAN.md` (pre-publication checklist).
