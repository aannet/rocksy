# Runbook: measure

Status: living
Last verified at: release 0.2.1 (branch kb-restructure; commands re-run during the restructuring: core tests, clippy, UI build, vitest, cross-compile build)

## Automatic startup timing

`tools/measure-auto.ps1` (Windows PowerShell) launches the exe N times in an isolated data folder (`-DataDir`, set as `ROCKSY_DATA_DIR`) and reads the milestones the app writes in its own `timings.jsonl`: window appearance, `editor-ready`, `tree-ready`, `tags-ready`. Parameters: `-Exe`, `-File` (a note to open, for the outside-vault case), `-DataDir`, `-Case`, `-Runs` (default 5), `-SettleMs`, `-RunsRoot` (where to write the run folder), `-KeepDataDir`. Run 1 of a series has the coldest cache.

These are instrumented measurements: reproducible, no human timing, but they do not include on-screen perception.

## Manual timers

`tools/measure-coldstart.ps1` (semi-manual, key F8 for T1 = keystroke accepted, F9 for T2 = tree visible). It writes `<RunsRoot>\YYYY-MM-DD_HHMM_<case>\` with `coldstart.csv` and `meta.json`. Used on the real vault for the baseline (Obsidian) and rerun on Rocksy at each milestone. Only the owner runs it on the real vault (see `docs/status.md`).

## Vault counters

`tools/vault-kpis.ps1` and `tools/scan-plugin-api.ps1` ask for the vault path (no default) and write counters only into `data/`. `vault-kpis.ps1` refuses to overwrite `data/vault-kpis.json`.

## Core measurement binary

`rocksy-cli` (in `app/crates/rocksy-core`): `cargo build -p rocksy-core --release`, then run it on a synthetic vault; see `app/crates/rocksy-core/README.md` (French).

## Run folders

- One folder per run: `runs/YYYY-MM-DD_HHMM_<case>/`. **Dated, never overwritten.**
- Put the raw values and a `notes.md` (what was run, with which exe and which sha256, which vault, conditions). Note the Drive cache state (cold or warm) in `notes.md`; on a synthetic vault on `C:` say so.
- State what the run does not cover, in the same file.
- Large page captures (PNG of the page, tens of MB) are kept on disk but not versioned: `.gitignore` covers `runs/*_integration/captures/` and `runs/*_integration-audits/captures/`; `*.mp4` and `*.mkv` under `runs/` are ignored too. Add a pattern for a new run kind that produces captures.
- Never present an estimate as a measurement.
