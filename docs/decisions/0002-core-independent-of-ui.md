# ADR 0002: Rust core independent of the UI and of Tauri

Status: accepted
Date: 2026-11-23

## Context

The interface between core, shell and UI had to be fixed so the three could be developed in parallel, and the core had to stay portable (no mobile for now, but not excluded) and testable on Linux where the Tauri dependency tree cannot be built.

## Decision

`app/crates/rocksy-core` has no UI dependency (no `tauri`). `app/src-tauri` holds thin command wrappers and is deliberately outside the Cargo workspace (its own `Cargo.lock`). `app/CONTRACT.md` is the single source for commands, events and JSON types between the three; it is changed first, then the three sides.

## Consequences

- The core builds and tests on Linux; the shell has no unit tests there. Logic that would otherwise live in the shell is put in the core so it is testable (the audit analysis asked for this for the own-writes structure, ADR 0015).
- The core's direct dependencies are few (`serde`, `serde_json`, `memchr`, `notify`, `tantivy`).
- Any IPC change touches several places (`docs/impacts.md`).

## Sources

Journal 2026-11-23 10:56 (contract decision); `app/CONTRACT.md`; `docs/archive/audits/2026-11-23_coherence.md` section 2.2 (point 7).
