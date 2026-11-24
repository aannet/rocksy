# Spec: startup

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; invariants taken from the contract, the UI README and the tests as they stand)

## Scope

What happens from launch to an editable note and a navigable tree, and the targets. Rationale: `docs/decisions/0004-startup-order-and-scale.md`. UI order of operations: `app/ui/README.md` (French) and `app/ui/src/main.ts`. Contract: "Startup" in `app/CONTRACT.md`.

## Targets

The product targets are: last note editable in 2 s or less, tree navigable in 3 s or less, at the same level with a vault 20 times larger. These are targets, not tested invariants: no automated test fails when they are missed. The measured values (synthetic vaults, local disk, warm cache, isolated profile) are in `runs/`; the real vault, Google Drive and cold start were never measured (`docs/status.md`).

## Invariants

| # | Invariant | Enforced by |
|---|---|---|
| ST-1 | The UI reads and shows the first note before `open_vault` and `build_tags`; only then are vault, index, panels and extensions started | order in `app/ui/src/main.ts`; browser scenario `perf` (`app/ui/tools/scenarios/perf.json`) plays startup. No unit test of the order |
| ST-2 | `StartupInfo.firstFile` is the first command-line file, else the active session tab, read by the backend with the same access checks as `read_file`; `null` if none, if the read failed or if it is not finished when `startup_info` answers (the UI then falls back to `read_file`); provided once | mock: `startup_info carries the first file, identical to read_file` (`app/ui/tests/sync.test.ts`). The shell side (reading the file during WebView startup, once) is not covered by a unit test; the browser scenario `reload` also lists `firstFile` among its checks (`app/ui/README.md`) |
| ST-3 | A file passed on the command line opens alone: the session's vault is not loaded until one of its tabs is activated | browser scenario `misc` (`app/ui/tools/scenarios/misc.json`). Not covered by a unit test |
| ST-4 | The tree comes from a persisted snapshot when one exists (`fromSnapshot: true`), then a background listing emits `tree-updated` only if the tree changed; a damaged snapshot is ignored; a snapshot that expands out of proportion is rejected | `snapshot_round_trip`, `snapshot_encoding_handles_any_order_and_unicode`, `damaged_snapshot_is_ignored`, `a_snapshot_that_expands_out_of_proportion_is_rejected`, `open_then_reconcile_detects_additions_and_removals`, `reconcile_without_snapshot_and_with_missing_root` (`app/crates/rocksy-core/src/tree.rs`) |
| ST-5 | `open_vault` and `build_tags` of one vault may arrive in any order; the index always arrives; one listing serves tree and index (two listings only if `build_tags` precedes `open_vault` by more than 300 ms or the vault is not open) | `background_flow_as_the_tauri_glue_will_run_it` (`app/crates/rocksy-core/tests/contract.rs`); real app: scenario `a5-order` (`app/ui/tools/app/a5-order.json`), 8 call orders in `runs/2026-11-23_2240_integration-audits/notes.md` section 1 |
| ST-6 | The persisted index loads without reading any note; reconciliation then re-reads only what changed, in the background | `reconcile_builds_then_reads_nothing_when_nothing_changed`, `reconcile_reads_only_modified_added_and_renamed_files` (`app/crates/rocksy-core/src/tags/persist_tests.rs`) |
| ST-7 | A corrupted vault registry or session file never stops startup: the registry is tolerated and preserved, a corrupted session is ignored then overwritten | `corrupted_registry_is_tolerated_and_preserved` (`app/crates/rocksy-core/src/vaults.rs`), `corrupted_file_is_ignored_then_overwritten` (`app/crates/rocksy-core/src/session.rs`) |
| ST-8 | Opening a vault removes only Rocksy's own orphaned temporary files (`.<name>….rocksy-tmp`, older than one hour) in visible folders; nothing else is deleted or created | `opening_a_vault_removes_only_rocksys_old_temporary_files` (`app/crates/rocksy-core/tests/no_unwanted_writes.rs`) |
| ST-9 | A launch with `ROCKSY_DATA_DIR` set is its own instance (no single-instance plugin); without it the app is single-instance and a second launch opens a tab in the running window | the isolated case is exercised by every real-app scenario; the single-instance case in a normal profile is not verified (`docs/status.md`) |
| ST-10 | The app writes startup milestones to `timings.jsonl` (no note content): `editor-ready`, `editor-ready-since-navigation`, `editor-painted-since-navigation`, `tree-ready`, a batch of `ui-*` milestones | read by `tools/measure-auto.ps1`; no unit test |

## Code

`app/src-tauri/src/main.rs` (`startup_info`, setup), `app/ui/src/main.ts`, `app/crates/rocksy-core/src/tree.rs`, `app/crates/rocksy-core/src/appdata.rs`.
