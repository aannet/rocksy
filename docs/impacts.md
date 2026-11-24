# Impact map

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; the command-surface row is enforced by `tools/check-consistency.py`, the other rows come from the contract, the tests and the audit analysis)

"If you touch X, also update Y and run Z." Seeded from what is known to break. Add a row when a change turns out to touch something that was not listed.

All commands run from the repository root unless noted. "Core tests" = `cd app && CARGO_TARGET_DIR=~/.cache/rocksy-target/core cargo test -p rocksy-core`; "UI tests" = `cd app/ui && npm run build && npx vitest run` (see `docs/runbooks/test.md`).

## Interface

| Change | Also update | Run |
|---|---|---|
| Add, remove or change an IPC command | 1. `app/CONTRACT.md` (commands table, types, errors) first; 2. the `#[tauri::command]` function and `generate_handler!` in `app/src-tauri/src/main.rs`; 3. `COMMANDS` in `app/src-tauri/build.rs`; 4. `app/src-tauri/capabilities/default.json` (`allow-<command>` with dashes); 5. `app/ui/src/backend.ts` (interface `Backend` and the Tauri implementation) and `app/ui/src/types.ts`; 6. the mock backend in `app/ui/src/mock/`; 7. a core type and `app/crates/rocksy-core/tests/contract.rs` if the JSON shape is new; 8. the matching spec in `docs/specs/` | `python3 tools/check-consistency.py` (command surface identical in the six places), core tests, UI tests, a real-app scenario if visible |
| Add or change an event or a payload type | `app/CONTRACT.md` (events table, types); the core type; `app/ui/src/backend.ts` and `app/ui/src/types.ts`; the mock (`app/ui/src/mock/`, which must emit it the same way); `app/crates/rocksy-core/tests/contract.rs` (JSON shapes are camelCase); events that concern a vault carry `vault` and the UI filters on it (`app/ui/src/vaultevents.ts`) | core tests, UI tests |
| Add a stable error code | `app/CONTRACT.md` (Errors table); the core; the UI recognises it by `code`, never by `message`; the mock | core tests (`error_and_payload_json_shapes`), UI tests |
| Change tag sorting or filtering (`tags_list`) | `app/testdata/tags_list_cases.json`, shared by a core test (`app/crates/rocksy-core/tests/tags_list_cases.rs`) and a UI test (`app/ui/tests/tagcases.test.ts`); the UI filters its cached list with the same rule (`app/ui/src/tagtree.ts` and the mock scanner); contract section "tags_list sorting and filtering" | core tests, UI tests |
| Change search semantics or excerpts | `app/CONTRACT.md` (Search); `app/crates/rocksy-core/src/search/`; the mock search in `app/ui/src/mock/` (parity is tested in `app/ui/tests/logic.test.ts`); `app/ui/src/excerpt.ts`; `docs/specs/vault-index.md` | core tests, UI tests |
| Change link resolution | `app/CONTRACT.md` (Links); `app/crates/rocksy-core/src/tags/graph.rs`; the UI resolver `app/ui/src/resolve.ts` (a separate implementation for in-note navigation); `docs/specs/vault-index.md` | core tests, UI tests |

## Safety

| Change | Also update | Run |
|---|---|---|
| Anything that writes in a vault, or a new write path | Do not add one without a spec: the only allowed write is `save_file` on `.md`/`.markdown`. Update `app/CONTRACT.md` hard rules, `docs/specs/saving.md`, `docs/decisions/` (new ADR), `app/crates/rocksy-core/tests/no_unwanted_writes.rs`; the extension rule is `WRITABLE_EXTENSIONS` in `app/crates/rocksy-core/src/files.rs` | core tests (`no_unwanted_writes`, `files_integrity`) and the real-app integrity scenario (`a5-integrity`) |
| Change who may read or write a path (access policy, `add_vault`, `asset:` scope) | `app/crates/rocksy-core/src/access.rs`; `app/CONTRACT.md` (File access); `app/src-tauri/tauri.conf.json` (asset scope); `docs/specs/file-access.md`; the mock backend, which applies the same refusals | core tests (`access_policy`), UI tests, real-app scenario `a5-security` |
| Change the CSP or what the UI injects as HTML or inline style | `app/src-tauri/tauri.conf.json` (the `style-src` choice is explained in `docs/decisions/0011-page-access-policy-and-csp.md`); `docs/specs/ui-safety.md`; `app/CONTRACT.md` section on the content security policy | UI tests (`security.test.ts`), real-app scenarios `a5-csp` and `a5-security` (zero CSP violation expected) |
| Capability or permission change | `app/src-tauri/capabilities/default.json` and `app/src-tauri/build.rs`; `docs/specs/file-access.md` (FA-14) | `python3 tools/check-consistency.py` |
| Cargo release profile (`panic`, `lto`) | both `app/Cargo.toml` and `app/src-tauri/Cargo.toml` must stay aligned (`panic = "unwind"`, `docs/decisions/0014-panic-unwind-and-index-rebuild.md`) | core tests, a Windows build, exe size noted in the run |

## Persisted data

| Change | Also update | Run |
|---|---|---|
| Persisted formats: tags and links file (`RKTAGIDX`, `VERSION` in `app/crates/rocksy-core/src/tags/persist.rs`), tree snapshot (`app/crates/rocksy-core/src/tree.rs`), search index (`app/crates/rocksy-core/src/search/engine.rs`) | Bump the version or the manifest so an old file is ignored and rebuilt (a damaged or other-version file must be ignored, see the persistence tests); state the rebuild cost in `docs/status.md` and in the release notes (`release/README.md`); remember that reverting to an older exe forces a full reindex | core tests (`persist_tests`, `index_engine`), then a real-app run on x1 and x20 copies noting reindex time |
| Data folder layout (`vaults.json`, `session.json`, `opened.json`, `timings.jsonl`, `trees/`, `tags/`, `search/`) | `app/CONTRACT.md` (Application data); `app/crates/rocksy-core/src/appdata.rs`; `remove_vault` must purge any new derived folder; `docs/decisions/0010-derived-data-local-folders.md` | core tests, `vaults.rs` tests |

## Shared code zones

Where several concerns meet. From the audit analysis (`docs/archive/audits/2026-11-23_coherence.md`, part 3: finding identifiers are historical and defined there). Do not give two agents the same zone in parallel.

| Zone | Concerns |
|---|---|
| `app/src-tauri/src/main.rs`: commands `read_file`, `save_file`, `startup_info`, `set_session`, `open_vault`, `build_tags`, `search`, `open_external`, setup, handler | access checks, startup order, close handshake, refresh worker, own writes, poller, index persistence. The shell has no Linux unit tests: put new logic in the core |
| `app/crates/rocksy-core/src/tags/index.rs`, `tags/store.rs`, `search/engine.rs` | persistence, deferred commit, damaged index, cancellation, memory |
| `app/crates/rocksy-core/src/tree.rs` | snapshot, listing threads (a panic must fail the listing, never give a partial one), cancellation |
| `app/crates/rocksy-core/src/files.rs`, `atomic.rs` | save conflicts, retries, extension rule, symlinks, size limit |
| `app/crates/rocksy-core/src/watch.rs`, `own_writes.rs`, `refresh.rs` | batches, own saves, shared listing |
| `app/crates/rocksy-core/src/vaults.rs`, `session.rs`, `access.rs` | registry, purge of derived data, opened-files list |
| `app/ui/src/app.ts` | save, reload, load, close of documents and panes, tabs, vault open and close, tree updates, events; `app/ui/src/main.ts` for startup order |
| `app/ui/src/sidebar.ts`, `app/ui/src/linkspanel.ts` | virtual tree, tag panel, links panel; inline `style=` attributes depend on the CSP choice |
| `app/ui/src/canvas.ts` | hostile canvas data, file references, dispose |
| `app/ui/src/editor/syntax.ts`, `app/ui/src/editor/livepreview.ts` | inline rendering limits, table and Mermaid blocks, `style` attributes |
| `app/ui/src/backend.ts`, `app/ui/src/types.ts`, `app/ui/src/mock/` | every contract change, done in one go after the contract |
| `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json` | CSP, asset scope, permissions: change last, after the UI is final |
| `app/ui/tools/app/*.json`, `app/ui/tools/scenarios/*.json` | any visible fix needs its scenarios adapted |
| `app/CONTRACT.md` | one editor at a time |

## Release

| Change | Also update | Run |
|---|---|---|
| Version bump | the places listed in `docs/runbooks/release.md` (four manifests and `release/README.md`) | core tests, UI tests, Windows build |
| New unverified item, known defect or owner-only measurement | `docs/status.md` | `python3 tools/check-consistency.py` |
| Move or rename a document | `docs/README.md` and every backticked path in `AGENTS.md`, `docs/` and `app/CONTRACT.md` | `python3 tools/check-consistency.py` |
