# ADR 0003: The vault is read-only, with an own vault registry

Status: accepted
Date: 2026-11-23

## Context

The vault is the owner's real personal data, synced by Google Drive across machines, and must stay openable in real Obsidian at all times. The brief rules out vault conversion. The owner also wants Rocksy to open any Markdown file outside a vault, and decided (2026-11-24, functional scope) that a vault is recognised only if registered in Rocksy's own list.

## Decision

No vault conversion. The only write Rocksy makes in a vault is `save_file` on a `.md` or `.markdown` (atomic: temporary file in the same folder, then rename). Never in `.obsidian/`, never in a `.canvas`, never next to a file outside a vault. The one deletion allowed is Rocksy's own orphaned temporary files older than one hour, at `open_vault`. Rocksy's own data (vault list, session, indexes) never goes in the vault; it lives in its app data folders (ADR 0010). Vaults are registered in Rocksy's own list (via an "Add vault" command and a first-launch offer to import Obsidian's vaults, read-only, per the scope draft); a file inside an unregistered vault folder opens as an isolated file. Folders and files starting with `.` are ignored in the tree.

## Consequences

- Enforced by tests that fingerprint every file of a test vault before and after (`app/crates/rocksy-core/tests/no_unwanted_writes.rs`).
- Any feature that writes in a vault (note creation, link update on rename, tag rename) needs its own spec and tests first; the 0.2 work explicitly left them out.
- Removing a vault from the list purges its derived data and touches no vault file.
- The automatic import of Obsidian's vault list is not implemented (`docs/status.md`).

## Sources

`app/CONTRACT.md` (hard rules); `docs/archive/perimetre-fonctionnel.md` and the 2026-11-24 scope paragraph of the previous `CLAUDE.md` (kept in git history); journal 2026-11-23 10:56.
