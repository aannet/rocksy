# Rocksy 0.2.1 — installation and getting started

Status: living
Last verified at: release 0.2.1 (branch kb-restructure)

Rocksy 0.2.1 is a fast Markdown editor for Windows 11. It opens any `.md` (with or without a vault) and reads an Obsidian vault without converting it. It is not yet a replacement for Obsidian: see "Limits".

## Contents

| File | Role |
|---|---|
| `rocksy.exe` | The application |
| `WebView2Loader.dll` | Keep it **next to** `rocksy.exe` (the app does not start without it) |
| `install.ps1` | Install for the current user, no administrator rights |

Requirements: Windows 11 (WebView2 is included). The binaries are produced by `tools/build-win.sh` and dropped in `C:\Users\<you>\rocksy-test\bin\`; they are not versioned.

## Try without installing

```powershell
C:\Users\<you>\rocksy-test\bin\rocksy.exe "C:\path\to\a note.md"
```

## Install

```powershell
cd C:\Users\<you>\rocksy-test\bin
# copy install.ps1 from the repository's release/ folder into this folder, then:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -AddToPath
```

The script copies the app to `%LOCALAPPDATA%\Programs\Rocksy`, creates a Start menu shortcut and adds Rocksy to "Open with" for `.md` and `.markdown`, without touching the entries of other applications (Obsidian, VS Code...). It writes only to the user registry (`HKCU`), without administrator rights.

With `-AddToPath`, the folder is added to the user PATH (only once, even if the script is rerun; existing entries, including `%VARIABLE%` ones, are kept as they are): `rocksy file.md` works in a new terminal.

**Update**: rerun `install.ps1` with the new binaries. If the installed Rocksy is open, the script asks it to close, like a click on the close button, and waits 10 s (`-CloseTimeoutSec <seconds>` changes this delay). If it is still open, the script stops without changing anything: save (Ctrl+S), close Rocksy, rerun the script. `-ForceClose` kills the process; unsaved changes are then lost. To be safe, save before starting an update. A Rocksy launched from another folder (for example `rocksy-test\bin`) is not closed.

**Double-click on a `.md`**: Windows 11 does not let a script change the default application. Once: right-click a `.md` > Open with > Choose another app > Rocksy > Always.

**Uninstall**: `powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall`. The script closes Rocksy the same way, then removes the installation folder, the shortcut, Rocksy's registry keys (its only "Open with" entry, not those of other applications) and its PATH entry, the others staying unchanged. The data in `%APPDATA%\dev.rocksy.app` (vault list, session) and `%LOCALAPPDATA%\dev.rocksy.app` (indexes, hence the text of the notes) is kept: delete these two folders by hand to erase everything.

The script has still never been run on a machine (0.2.1 included): only its syntax and its PATH handling were checked. At first use, check the result ("Open with" on a `.md`, `rocksy` in a new terminal).

## Use

| Action | How |
|---|---|
| Open a file | Command-line argument, "Open with", drag and drop, or Ctrl+P > open a file |
| Second file | A second launch opens a tab in the existing window (single instance) |
| Add a vault | Ctrl+P > "Add a vault", choose the folder in the Windows dialog |
| Remove a vault | Vault picker > "Remove this vault from the list": the vault disappears from the list and its indexes are deleted; no file of the vault is touched |
| Go to a note | Ctrl+O |
| Commands | Ctrl+P |
| Save | Ctrl+S, and autosave 2 s after the last keystroke (can be turned off in the status bar) |
| Search the vault | Ctrl+Shift+F or the magnifier: search as you type, case and accents ignored, words combined with AND. `"exact phrase"`, `-word` to exclude, `path:text`, `tag:a/b` |
| Links of a note | "Links" tab of the side panel: incoming links (backlinks) and outgoing links of the active note; Ctrl+P > "Unresolved links" |
| Tags | Tag panel: filter, click a tag to see its fragments, click a fragment to open the note at the exact spot |
| Canvas, `.txt` and other non-Markdown files | Read-only (padlock on the tab) |
| Close | The close button first saves everything that is modified. If a save is impossible, or if autosave is turned off, Rocksy asks: save, quit without saving, or cancel |

## What's new in 0.2.1

Fix release, from three audits (security, robustness, performance). No new feature.

- **Security.** A booby-trapped canvas or note can no longer run code in the application. The page only reaches registered vaults and the files you opened yourself (command line, "Open with", dialog, drag and drop); around a file opened outside a vault, neighbouring notes and images are readable but not editable. A folder becomes a vault only if it was chosen in the Windows dialog. No network connection is possible from the page; remote images and those on a network share are not loaded (a message replaces them).
- **No more text loss on close.** Closing the window right after typing saves before quitting. A save that fails (file locked by Drive or an antivirus) is retried and reported; a file modified or deleted by another tool while you edit it is never overwritten without asking you.
- **Read-only for non-Markdown files.** Rocksy only saves `.md` and `.markdown`. A `.txt`, a canvas or a file reached by a link from a note outside a vault opens read-only.
- **Line endings.** A file with mixed line endings (LF and CRLF) keeps its own: only the modified lines change.
- **Index data in `%LOCALAPPDATA%`.** The indexes (which contain the text of the notes) leave the roaming profile. Those of a previous version are moved at first launch, without reindexing.
- **Removing a vault purges its indexes** (tree, tags, links, search).
- **Safer index.** A damaged search index is rebuilt by itself instead of crashing the application. If some notes could not be read or indexed (busy file), the status bar says so ("Index: N notes to reread...") and Rocksy retries on its own.
- Saving a large note is faster (the search index is updated with a delay, within 2 s), and a save no longer triggers a full walk of the vault.
- The exe goes from 11.6 to 14.1 MB.

## What's new in 0.2.0

- Full-text search across the whole vault, as you type, with highlighted excerpts; a click opens the note at the spot found.
- Incoming (backlinks) and outgoing links of the active note, including links to images; list of unresolved links. Resolution follows Obsidian's rules.
- One index per vault: each new or modified note is read only once for tags, links and search. After the first indexing, everything is available as soon as the vault opens.
- The exe goes from 6.7 to 11.6 MB. On disk, the search index takes roughly the size of the vault's text.

## What's new in 0.1.2

- Tracking of external changes: when another tool (Drive, AI cleaner, file explorer) creates, modifies, renames or deletes notes while Rocksy is open, the tree and the tags update, about half a second later on a local disk. Bursts are grouped and only modified notes are reread. Dot-folders are ignored.
- Safety net: when returning to the window, the vault is rechecked if the last check is older than 30 s (in case Google Drive does not report every change).

## What's new in 0.1.1

- Persisted tag index: after a first indexing, tags are available as soon as the vault opens. The tag panel shows "index saved, checking..." then "up to date".
- To change version: replace `rocksy.exe` (or rerun `install.ps1`). The application data is kept.

## What Rocksy writes, and where

- In a vault or next to a file: **only** the `.md` or `.markdown` you save (atomic write: a temporary file `.<name>.…rocksy-tmp` in the same folder, renamed immediately). Never in `.obsidian/`, never in a `.canvas`, never in any other file type.
- The only possible deletion in a vault: when it opens, the `…rocksy-tmp` temporary files left by an interrupted save of Rocksy itself, if they are more than one hour old.
- `%APPDATA%\dev.rocksy.app\` (follows a roaming profile): vault list, last session, list of files opened outside a vault (`opened.json`), timings log (`timings.jsonl`, no note content).
- `%LOCALAPPDATA%\dev.rocksy.app\` (stays on the machine): tree snapshots, tag and link indexes, search index. The search index contains the text of your notes: treat this folder like your notes if you back it up or share it.
- Deleting these two folders resets the app without touching the notes. Removing a vault from the list deletes its indexes.
- Nothing is sent over the network.

## Limits

- The **first** tag indexing of a vault reads all the notes in the background. On Google Drive when cold, this can take in the order of a minute; the editor and the tree stay usable meanwhile. Later openings reload the saved index without reading any note, then only reread the notes whose size or date changed.
- A note modified by another tool without a change of size or date is not seen again by the tag index before its next modification.
- No graph, tag renaming, link update on rename, note creation from an unresolved link, nor creation, renaming or deletion of files from the tree.
- Search: no OR, no stemming ("budget" does not find "budgetary", but "budg" at the end of the input does). The last word typed is a prefix only if it ends the query.
- No mods and no store. No automatic import of Obsidian's vaults (manual add).
- Tracking of external changes has not been verified on Google Drive: if it receives no notification, the update waits for the return to the window or for the vault to be reopened. A rename is seen as a deletion and an addition.
- UTF-8 only. Files larger than 50 MB are refused.
- Going back to an earlier exe after running 0.2.1 causes a full reindexing: the old version looks for its indexes in `%APPDATA%`, from where 0.2.1 moved them. Nothing is lost, but the first opening of the vault rereads all the notes.
- Images of a note outside a vault are only displayed if they are in the note's folder or below it. Images of a vault located on a network share are not displayed.
- Right after copying or syncing a large number of files, the first indexing can be slowed down and the search index completed in two steps (a few seconds): the status bar says so.
- `install.ps1` has never been run (see "Install").
- Not tested so far on the real Google Drive vault nor cold after a restart: see `docs/archive/m2-resultats.md`. The 0.2.1 tests (the integration-audits runs of 2026-11-23 under `runs/`) used synthetic vaults, on a local disk, in an isolated profile: the second launch (single instance), the Windows dialogs, drag and drop and the move of the indexes between the two real folders are not covered.
