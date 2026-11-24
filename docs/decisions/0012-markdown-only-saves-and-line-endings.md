# ADR 0012: Saves limited to Markdown; line endings preserved

Status: accepted
Date: 2026-11-23

## Context

A compromised page should be able to write as little as possible; users must not see their files change shape when only one line was edited.

## Decision

`save_file` accepts only `.md` and `.markdown`; `.obsidian/` and `.canvas` stay forbidden also through links or Windows short names. A `.txt`, a canvas or any other file opens read-only (`FileContent.readOnly`, padlock on the tab, no autosave). Files over 50 MiB are refused. UTF-8 only: a non-UTF-8 file gives a readable error. The backend never changes line endings; the UI applies the rule on save: a homogeneous file is rewritten with its own ending; in a mixed file unchanged lines at the head and tail keep their original ending and changed or added lines take the majority ending; the BOM is rewritten as it was. A failed save is retried (2 s, 5 s, 15 s, 30 s) on `io` errors; an external change or deletion is never overwritten without asking.

## Consequences

- A mixed file opened and saved unchanged is byte-identical (UI unit tests in `app/ui/tests/sync.test.ts`).
- Details: `docs/specs/saving.md`.

## Sources

Journal 2026-11-23 16:45 and 18:28; `app/CONTRACT.md` (File access, Line endings).
