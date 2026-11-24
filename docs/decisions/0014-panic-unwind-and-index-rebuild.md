# ADR 0014: panic = unwind and automatic rebuild of a damaged search index

Status: accepted
Date: 2026-11-23

## Context

With `panic = "abort"` a panic in a background thread (tantivy on a damaged index) kills the app. The robustness audit (BUG-3) recommended `unwind` with `catch_unwind`.

## Decision

Release profiles use `panic = "unwind"` (root `app/Cargo.toml` for `rocksy-cli` and `app/src-tauri/Cargo.toml`, kept aligned). Command bodies and background steps are wrapped so a panic ends the step, not the thread. A damaged search index never gives an error or a crash: `searchReady` is false while it is rebuilt automatically. A panic in a listing thread must fail the listing and remove nothing (no partial listing). The integrity (checksums) of the search index is verified when it is opened.

## Consequences

- Cost measured by the coherence analysis on `rocksy-cli` under Linux: about +735 KB (+12.4 %) with unwind; not measured on the Windows exe at that time.
- The integrity check reads the whole search index at each open: the working set about doubles on x20 (accepted, `docs/status.md`).
- A damaged index is announced ready for about 0.25 s before its checksum check.
- No journal decision entry records this choice; the evidence is the audit analysis, the comments in both `Cargo.toml` and the tests `damaged_search_index_never_panics_and_is_rebuilt` and `a_panic_in_a_listing_thread_fails_the_listing_and_removes_nothing`.

## Sources

`docs/archive/audits/2026-11-23_coherence.md` section 2.7; `app/Cargo.toml`; `app/src-tauri/Cargo.toml`; journal 2026-11-23 22:58 (accepted memory regression).
