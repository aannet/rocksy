# ADR 0005: One index per vault, each changed note read once

Status: accepted
Date: 2026-11-23

## Context

On Google Drive, reading a note (about 155 ms) is the cost, not listing. Tags, links and full-text search would read each note three times if they had separate indexes.

## Decision

One vault index feeds tags (with fragments), the link graph and the full-text search. Each new or changed note is read once for all three. The index is persisted per vault in the local data folder and reconciled at open by size + mtime (ADR 0006). `build_tags` keeps its name but builds and maintains the whole index.

## Consequences

- Tags and links are available as soon as the persisted index loads; search may come slightly later (`searchReady`).
- After `save_file`, tags and links are updated before the command returns and search within 2 s (ADR 0008); disk is not re-read.
- Notes that cannot be read or indexed are counted in `IndexReady.pending` and retried automatically.
- Details: `docs/specs/vault-index.md`.

## Sources

Journal 2026-11-23 14:24; `app/CONTRACT.md` (Vault index: a single read pass).
