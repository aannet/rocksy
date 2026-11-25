# Release 0.2.2: text-loss fix (SV-18), real-app runs

Runs played on 2026-11-25 between 22:35 and 22:53 (CET). The folder name keeps the time it was planned under (15:00), which the spec and `docs/status.md` already pointed to; the times below are the real ones.

## What was tested

- **0.2.2**: the sources of branch `fix/0.2.2` as committed together with this note (backport of the SV-18 fix, version bump, and the two review fixes below), built with `tools/build-win.sh` into a work folder of the Windows test area. Tested exe: 14,054,400 bytes, sha256 `6e0845371e93e0c8348f74766ca088deb8f6c796bb54a0a22b5d02c5a102928d`, built from the working tree before the commit (same `app/` content as the commit). The packaged exe is rebuilt from the commit itself; its hash cannot be written into the commit it is built from and is given with the release.
- **0.2.1 behaviour**: commit `a286d36` (the base of `fix/0.2.2`: 0.2.1 plus the tools and knowledge-base commits, no product change) extracted with `git archive` and built the same way. 14,135,296 bytes, sha256 `ba5b872d2ad02e18b9a0c68dfa604c2d749ed7cf4968f08920aae6dbf6ffdb0b`. Not byte-identical to the shipped 0.2.1 exe (the sources moved to `app/` after it); the shipped exe was not run.

Conditions: Windows 11, isolated instances only (`ROCKSY_DATA_DIR` under the work folder, a fresh data folder per scenario except the upgrade pair), driven over the DevTools protocol on port 9223 (`app/ui/tools/drive.py`, real key presses and clicks), each run under `flock` on the shared real-app lock. Before each scenario the port was checked free. The owner's own instance (another build, normal profile, started before these runs) kept running throughout: it has no debugging port and a different WebView2 profile folder, and it was not touched. Vaults: copies (robocopy) of the synthetic `x1` reference vault on `C:` (local disk, warm cache), plus two synthetic notes added to the copy used by the `fix-activetab` scenarios (`Self.md`, a note that links to itself; `Long.md`, 420 lines with headings every 50 lines and a unique word on line 351). Neither the real vault, Google Drive, the app data folders of a normal profile nor the reference vaults themselves were opened. Fixtures that earlier runs had left in the shared test folder (`files\img\local.png`, the `Test BOM`/`Test CRLF` notes of `it-save`) were recreated synthetically in the work folder.

Raw results: `results/` (paths of the work folder replaced by `<work>`). Upgrade scenarios: `upgrade-a-021.json`, `upgrade-b-022.json` in this folder.

## Review of the change

The backport was read against the release code (which differs from the branch the fix was written on: no file operations, no rename, no right panel). Paths checked: `Pane.show`, `stash`, `dispose`, `App.activate`, `closeDoc`, `splitRight`, `onUpdate`, `stateOf`/`viewOf` (saves, reloads, conflicts, close-requested), `applyDisk`, `openPath`/`openAt` (heading, lines, search results), `loadExtras`, `togglePreview`, `refreshPreviews`, `applySearchTerms`, the 120 ms empty-pane timer while a tab loads, and session restore at startup. No remaining path puts an older snapshot in an editor or saves a stale text: the text a save captures (`textOf`) is the view's when the view holds the note, the snapshot otherwise, and the snapshot is always copied from the view before the view is given another note or moved to another pane.

Two problems introduced by the backport, fixed in the same change:

1. **Keys typed after closing a tab could be saved into the closed file.** When the closed tab was the note held by the editor and the next tab was still being read, the view still showed the closed note's text and `onUpdate` treated it as that note's edit: dirty flag and autosave on a document no longer in any tab (0.2.1 dropped such keys). Fix: `EditorHolder` takes a predicate (the note still has a tab in the pane); a closed note is no longer held, so it gets no edit, save or stash. Test: `a closed note is no longer held: keys typed before the next tab is read are neither its edits nor its saves` in `app/ui/tests/editorholder.test.ts` (fails without the fix). The window is short on a local disk (the editor is hidden after 120 ms) and could not be reproduced in the real app.
2. **Preview refresh skipped a hidden editor.** `refreshPreviews` (tree or vault change) only reached panes whose active tab was a note; with a canvas or an image shown, the hidden editor kept the note it holds, and since showing that note again no longer gives the view a new state, its links could keep their old resolved/unresolved look until the next edit or cursor move. Fix: the refresh reaches every editor that holds a note. Visual only; not covered by a test.

Seen in both builds, unchanged: clicking the tab of the active note leaves the keyboard focus on the page body instead of the editor (`fix-activetab`, A2). Not a regression; not fixed here.

## Results

### The loss, before and after (`fix-activetab`, `fix-activetab-2`)

Each step types a marker in the active note, activates that same note by the path named, then reads the editor and, after the 2 s autosave, the file on disk.

| Path that activates the note already shown | 0.2.1 behaviour | 0.2.2 |
|---|---|---|
| Its own tab clicked (`fix-activetab` A) | marker gone from the editor, file without it, Ctrl+Z has nothing to undo | marker kept, saved, Ctrl+Z removes it and the undo is saved |
| Its row in the file tree (B) | lost | kept and saved |
| Quick switcher (C) | lost | kept and saved |
| A link to itself, Ctrl+click (D) | lost | kept and saved, undo works |
| Away to another note and back (E) | (nothing left to lose) | kept |
| Long note, own tab clicked with the cursor on line 301 (`fix-activetab-2` S) | lost, view jumps (scroll 7979 to 6274 px), cursor on line 1 | kept, scroll unchanged (7979 px), cursor on line 301 |
| Long note, away to another note and back (S4) | - | kept, scroll restored to within one line (7953 px) |
| Long note, a canvas shown then the note's tab (S5, editor hidden meanwhile) | - | kept, scroll restored (7953 px) |
| Search result in the same note (P) | lost | kept and saved, cursor on line 351, view scrolled there |
| Heading link in the same note, `[[Long#Section 6]]` (H) | lost | kept and saved, cursor on the heading line 300 |
| Two panes, right pane: its own tab clicked (T) | lost | kept |
| Two panes, left pane: its own tab clicked (L) | lost | kept |
| Back to the right pane (T3), files on disk 2 s later | neither marker on disk | both markers on disk, each in its own file |

Final files of the 0.2.2 runs: `Self.md` holds exactly the expected text; `Long.md` holds the four markers, still 421 lines, heading `## Section 6` and the line-351 marker intact; the daily note holds its marker.

### Integrity scenarios of the release, replayed on both builds

Same results on 0.2.1 behaviour and 0.2.2 except timings, in every scenario below. No CSP violation, no unexpected external request, every launched process gone at the end.

| Scenario | 0.2.2 result |
|---|---|
| `a5-integrity` | read-only tour (images, canvas, tags, links, search, 4,658 files): vault unchanged byte for byte; one save: exactly one file changed. "Viewport failed to stabilize" logged once (known, `docs/status.md`; also in the 0.2.1 run) |
| `a5-loss` | line endings LF, CRLF, BOM, BOM+CRLF, mixed: only the edited line changes; close right after typing: text on disk, exit 130 ms; autosave off: dialog, cancel keeps the text, save-and-quit saves, quit-without-saving does not; external change while dirty: conflict, disk untouched; file deleted: conflict, not recreated |
| `a5-csp` | full tour without CSP violation; injected script, handlers, `eval`, network calls blocked (the three blocked requests to `example.com` are the injection test itself, same in 0.2.1). A first attempt timed out waiting for the vault images (8 s), replayed at once without error; not seen in the 0.2.1 run |
| `a5-links` | save without link change: no `index-ready`; save adding a link: one event, panel 3 to 4 links |
| `a5-security` | trapped canvas inert, 8,000-character table cell responsive (p95 3.1 ms), network images not loaded, out-of-scope reads and writes refused, hostile session not persisted, `.txt` read-only, `asset:` scope enforced (network path refused after about 1.3 s, known) |
| `a5-robust` | corrupted search, meta and tag index files: rebuilt, search and tags back, app alive |
| `a5-order` | index events for first open, vault switch, return, raw command orders and restart: same sequences as 0.2.1 |
| `it-save` | Ctrl+S on LF, CRLF, BOM, BOM+CRLF without final newline: only the edited line changes; autosave after 2 s; undo/redo; external reload; save conflict; overwrite. Played again on fresh copies for both builds after the fixtures were recreated: identical except timings |
| `it-preview`, `it-canvas` | live preview, images, code, tables, callouts, frontmatter fold; canvases read-only and bytes unchanged |
| Browser bench `edit`, `close`, `reload` (UI with the simulated backend, headless Edge) | no error, results identical to the 0.2.1 UI apart from timings and the order of two asynchronous startup marks |

### Upgrade (`upgrade-a-021.json` then `upgrade-b-022.json`, same data folder)

- 0.2.1 behaviour: adds a synthetic vault, opens two notes, types and autosaves, closes normally. The data folder then holds `search`, `session.json`, `tags`, `timings.jsonl`, `trees`, `vaults.json`, `webview`.
- 0.2.2 on that data folder: reports version 0.2.2, restores the vault, both tabs and the active note with the 0.2.1 edit; tags and search come from the cache (`fromCache` events) followed by the usual reconciliation pass, no rebuild; a search finds the line-351 word; an edit followed by a click on the note's own tab is kept and saved. No new file in the data folder. No persisted format changed between 0.2.1 and 0.2.2.

## Not verified

- The real vault and Google Drive (cold or warm), a normal profile, and the owner's own data folder: an upgrade was checked only on a data folder written by a 0.2.1-behaviour build in an isolated profile.
- Keys typed while the next tab is still being read (they now go to the note still shown) and the closed-tab case above: unit tests only; a local read finishes before keys can be typed by the driver.
- A reload or a conflict arriving while another tab loads; files opened from outside the window (second launch, drag and drop); backlinks panel entries in the same note (same code path as a search result, `openAt`, not played).
- The preview refresh of a hidden editor (review fix 2): no test, not played.
- The packaged exe itself was not replayed if its hash differs from the tested exe (see the release report).
