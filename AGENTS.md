# AGENTS.md

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure)

Entry point for any LLM working on this repository. Read this file first; it points to the authoritative document for each topic (`docs/README.md` has the full map).

## What Rocksy is

A fast Obsidian replacement for Windows 11: Rust core (`app/crates/rocksy-core`), Tauri 2 shell (`app/src-tauri`), CodeMirror 6 UI (`app/ui`, UI strings in French). Personal project of the owner, public repository. Current version: 0.2.2.

Targets (measured on synthetic vaults with a warm cache; the real vault on Google Drive, cold start and the manual timers were never measured, see `docs/status.md`):
- last note editable in 2 s or less, file tree navigable in 3 s or less, cold start;
- the same performance with 20x more files (about 10k notes): startup cost must not grow with vault size.

## Hard rules

1. The vault is the owner's real personal data. Treat it as read-only. Never touch the real vault (`G:`), the app data folders (`%APPDATA%\dev.rocksy.app`, `%LOCALAPPDATA%\dev.rocksy.app`), the installed or released builds, or the reference vaults `x1` and `x20`.
2. The only write the app may make in a vault is `save_file` on a `.md` or `.markdown` file. Never in `.obsidian/`, never in a `.canvas`. Derived data (indexes, snapshots) lives in the app data folders, never in the vault. See `docs/specs/saving.md` and `docs/specs/file-access.md`.
3. Public repository hygiene: never write a real vault folder name, a real note name or an email address in a tracked file. Use the placeholders `G:\Mon Drive\MonVault` and `01__WORK/daily-notes.md`. The files `in/audit-vault-obsidian.md` and `in/audit-vault-obsidian.json` are on disk but untracked on purpose: do not open them, copy from them or add them. Vault findings stay as counters only. Plugin `data.json` values (they can hold secrets) never go in a document, a log or a fixture.
4. Commit or push only when the owner asks. Never stop a `rocksy.exe` you did not launch. Do not run `release/install.ps1` (it writes to the user registry).
5. Never claim as verified what was not. Never present an estimate as a measurement. `docs/status.md` lists what is unverified.
6. No product behaviour change without a spec change first (see Workflow).

## Layout

```
AGENTS.md  CLAUDE.md   entry points
app/       the product: crates/rocksy-core, src-tauri, ui, testdata, CONTRACT.md
tools/     build, measurement and driving scripts (build-win.sh, cdp.py, measure-*.ps1, gen-vault.py, check-consistency.py)
release/   README.md and install.ps1 shipped with a release
runs/      dated, never overwritten measurement and integration runs
data/      counters about the real vault (no note content)
in/        inputs (project brief, French)
docs/      knowledge base: specs/, decisions/ (ADRs), runbooks/, impacts.md, status.md, handoffs/, archive/
```

## Commands

Details in `docs/runbooks/`. Everything is built in WSL and cross-compiled to Windows; there is no sudo and no Rust, Node or MSVC on Windows.
- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH`.
- Core tests: `cd app && CARGO_TARGET_DIR=~/.cache/rocksy-target/core cargo test -p rocksy-core`, then `cargo clippy -p rocksy-core --all-targets`.
- UI: `cd app/ui && npm run build && npx vitest run`.
- Windows build (never into a release or user folder): `tools/build-win.sh --skip-ui --dest <work folder>`. See `docs/runbooks/build.md`.
- Real app: only as an isolated instance (`ROCKSY_DATA_DIR`) driven over CDP. See `docs/runbooks/test.md`.
- Consistency check: `python3 tools/check-consistency.py` (run from the repository root).

## Workflow

1. Spec first: change `app/CONTRACT.md` (interface: commands, events, types) or the relevant file in `docs/specs/` before the code. One change at a time on the contract.
2. Check `docs/impacts.md` for everything else the change touches.
3. Tests, then code. Every invariant of a spec is meant to be enforced by a test; if it is not, the spec says "not covered by a test".
4. Real-app check when the change is visible: a dated run note under `runs/<YYYY-MM-DD_HHMM_case>/` (never overwrite a run; note the Drive cache state).
5. A decision that changes an earlier one is a new ADR in `docs/decisions/`; accepted ADRs are never edited.

## Done when

- core tests and clippy pass (zero warnings), `npm run build` and `npx vitest run` pass;
- the spec, `app/CONTRACT.md` and `docs/impacts.md` match the code in the same change;
- `python3 tools/check-consistency.py` exits 0;
- `docs/status.md` is updated if something new is unverified, known broken or measurable only by the owner;
- nothing is committed or pushed unless asked.

## Which document is authoritative

| Topic | Document |
|---|---|
| IPC surface (commands, events, types, errors) | `app/CONTRACT.md` |
| Behaviour by domain, with the tests that enforce it | `docs/specs/` |
| Why things are the way they are | `docs/decisions/` |
| What else to update when you touch X | `docs/impacts.md` |
| How to build, test, measure, release | `docs/runbooks/` |
| Unverified items, known limits, owner-only measurements | `docs/status.md` |
| User-facing install and usage | `release/README.md` |
| History (French, not authoritative) | `docs/archive/` |
