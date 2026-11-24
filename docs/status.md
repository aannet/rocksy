# Status

Status: living
Last verified at: release 0.2.1 (branch kb-restructure; contents compiled from the 0.2.1 sources listed at the end, nothing re-measured)

What was never verified, what is known to be wrong or limited, which audit findings are still open, and which measurements only the owner can take. Update this file in the same change as anything that adds to these lists.

## 1. Never verified

Conditions of every measurement so far: synthetic vaults on a local disk (`C:`), warm cache, isolated profile (`ROCKSY_DATA_DIR`), driven over CDP. Therefore not covered:

- The real vault on Google Drive, and any cold start after a Windows reboot (Drive cache cold). All performance numbers in the archive and in `runs/` are warm-cache numbers on synthetic data.
- Native dialogs (`pick_folder`, `pick_file`) and therefore adding a vault in a normal profile. The refusal of `add_vault` for a folder not picked in the dialog is covered by core tests and the mock backend only.
- Drag and drop of a file onto the window.
- Single-instance behaviour in a normal profile (a second launch opening a tab in the running window). An isolated profile is its own instance, so scenarios cannot exercise it.
- The real migration of index folders from `%APPDATA%` to `%LOCALAPPDATA%` on a real profile (the 0.2.1 release note says it was never tried on the owner's real profile).
- `release/install.ps1`: never executed on any machine. Only its syntax and its PATH handling were checked.
- Opening an external link in the system browser (the call is verified, the launch is not).
- IME (input method editors): no source records a check; treat as unverified.
- The watcher on Google Drive: the app relies on the focus safety net if Drive sends no notification, but notification delivery on Drive was not verified.
- The first real-vault timings with the manual timers (F8/F9), and the Obsidian baseline on a comparable scope (the old Obsidian baseline folder was an empty placeholder, now `docs/archive/obsidian-baseline-runs/`).

## 2. Known defects and limits (0.2.1)

From the "remaining defects" section of `runs/2026-11-23_2240_integration-audits/notes.md` and from `release/README.md` ("Limits"):

- `asset:` and network paths: a request to `http://asset.localhost/` followed by a network path is refused, but only after about 1.3 s (2 ms for other refusals), which indicates Tauri touches the path before testing its scope. The UI never makes this request; injected content could (the CSP allows `asset:` for images). Not fixed.
- CodeMirror logs "Viewport failed to stabilize" during tab switches chained by scripts (seen twice). No observed effect.
- Typing at the end of a single 216,000-character line: median 76 ms. Known extreme case, out of scope of the audit batch.
- A damaged search index is announced ready for about 0.25 s before its checksum verification, then rebuilt.
- Working set doubles at startup on the x20 vault (about 60 MB to 122 MB; private memory unchanged at 39 MB) because the search index checksums are verified at each open. Accepted regression, to revisit if it weighs on Drive.
- Tag index: a note changed by another tool without a change of size or mtime is not seen before its next modification (change detection is size + mtime only).
- Index writes can be refused on freshly copied files (suspected antivirus scan, an unproven hypothesis): the app retries and shows "Index: N notes to re-read..." (see `docs/specs/vault-index.md`).
- Not implemented: graph, tag rename, link update on rename, note creation from an unresolved link, file creation/rename/deletion from the tree, search OR operator and stemming, mods and store, automatic import of Obsidian vaults.
- UTF-8 only; files over 50 MiB refused. A rename is seen as a deletion and an addition.
- Going back to a 0.2.0 exe after running 0.2.1 forces a full reindex (0.2.0 looks for its indexes in `%APPDATA%`).
- Images of a note outside a vault load only from its folder or below; images of a vault on a network share are not shown.
- The exe grew from 11.6 to 14.1 MB between 0.2.0 and 0.2.1.

## 3. Open audit findings (not treated in 0.2.1)

Defined in `docs/archive/audits/` (French). Sources: the 0.2.1 session journal and report.

| Finding | Subject | Why open |
|---|---|---|
| PERF-11 | tantivy writer (3 threads, 48 MB budget) kept until close; the audit proposes freeing it after about 30 s idle | not treated |
| PERF-12 | one fewer `stat` in `read_file`; `save_file` to be measured on Drive first | not treated; gain unmeasured on Drive |
| PERF-15 | derive `file-changed` from watcher batches instead of polling open files every 2 s | not treated; the audit says to keep polling until Drive notifications are verified |
| PERF-6, second part | the audit lists (a) a form without `size`/`mtimeMs`, (b) `tree-updated` without the full tree, (c) a compact form. The contract shows `TreeEntry` is `{path, isDir}` (a) while `tree-updated` still carries the tree, so (b) and (c) are the likely open part. The journal only says "the second part": this reading is an inference | not treated |
| SEC-17 | dependency audit tool (`cargo-audit` or `cargo-deny`) to run before each release; not installed | not treated |

## 4. Measurements only the owner can take

- Manual cold-start timers on the real vault, once warm and once after a Windows restart (Drive cold): `tools/measure-coldstart.ps1` (see `docs/runbooks/measure.md`). The same on Obsidian for the baseline, which was never taken.
- The extended KPI script on the real vault: `tools/vault-kpis.ps1` (counters only; it asks for the vault path).
- Trying 0.2.1 on the real profile: index migration without reindexing, adding a vault through the dialog, drag and drop of a `.md`, second launch opening a tab, closing right after typing, saving a note whose name contains `~` followed by a digit.
- Any decision still open that needs the owner: code signing (`docs/handoffs/code-signing.md`), repository licence (none today).

## Sources

`release/README.md` ("Limits"), `runs/2026-11-23_2240_integration-audits/notes.md` (sections "Remaining defects and remarks" and "Not verified"), the "actionsAlec" list and the final report points in `docs/archive/session-2026-11-23/etat.js`.
