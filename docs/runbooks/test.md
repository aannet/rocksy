# Runbook: test

Status: living
Last verified at: release 0.2.1 (branch kb-restructure; commands re-run during the restructuring: core tests, clippy, UI build, vitest, cross-compile build)

Run `python3 tools/check-consistency.py` from the repository root before declaring a change done: it checks that the command surface is identical in the contract, the handler, the build script, the capability file, the UI backend and the mock, that repository paths written in backticks in the documents exist, that normative and living documents carry their header, and (if a `.private-terms` file exists) that no tracked file contains a private term.

## Unit and integration tests

- Core: `cd app && CARGO_TARGET_DIR=~/.cache/rocksy-target/core cargo test -p rocksy-core` (244 tests at the last run), then `cargo clippy -p rocksy-core --all-targets` (zero warnings).
- UI: `cd app/ui && npm run build && npx vitest run` (124 tests at the last run).
- Tests that play a real filesystem watcher (`app/crates/rocksy-core/tests/watch.rs`) use inotify on Linux and the Windows API when run on Windows.

## Browser bench (UI only, simulated backend)

`app/ui/tools/run.sh <scenario>...` copies `dist/` and `tools/` to a work folder on Windows, then `app/ui/tools/drive.py` (Windows Python, standard library only) drives headless Edge over the DevTools protocol: real key presses, clicks, page captures. Scenarios are in `app/ui/tools/scenarios/`; results land in the work folder under `shots/`. The scenario list and what each checks is in `app/ui/README.md` (French). The scripts still hard-code the owner's Windows user folder and Python path: adapt them on another machine.

## Real app (WebView2 + Rust core)

`app/ui/tools/run-app.sh <scenario>...` plays the scenarios of `app/ui/tools/app/` against a work build. Rules:

- Always an isolated instance: launch with `ROCKSY_DATA_DIR=<new folder>`; such a run is its own instance (no single-instance plugin), so tests never land in the owner's running app. Without that variable the app is single-instance.
- Default exe is the one in the `wip` work folder, never the one in `bin` or a `release-*` folder. Override with `ROCKSY_EXE`, the test folder with `ROCKSY_IT`, the vault folder with `ROCKSY_VAULTS` (point it at copies, never at the reference vaults).
- To drive it by hand: launch with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`, then `tools/cdp.py` (`eval`, `evalfile`, `shot`, `key`), run with the Windows Python (the port only listens on Windows localhost). Captures are of the page, never of the desktop.
- Stop only the process you launched (by PID).
- What these runs cannot cover: the second launch absorbed by the running instance, native dialogs, drag and drop, the refusal of `add_vault` for a folder not picked in the dialog (see `docs/status.md`).

## Synthetic vaults

- Generate: `python3 tools/gen-vault.py --out <dir> --scale N` (1, 2, 5, 10, 20). Deterministic and prefix-stable (blocks of 488 notes and 465 attachments): rerunning with a larger scale in the same folder adds only the delta.
- The reference vaults `x1` and `x20` on Windows are read-only references for measurements: copy them (robocopy) before any test that writes.
- Writing thousands of files through `/mnt/c` from WSL is very slow: tar in WSL, extract with the Windows `tar.exe`.
- Test data lives outside the repository (`~/rocksy-vaults` in WSL, a work folder on Windows).

## Rules

- Never open the real vault. Never write into a reference vault.
- A run that matters gets a dated note under `runs/` (see `docs/runbooks/measure.md`).
