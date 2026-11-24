# Interface contract

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure)

Source of truth between the Rust core (`app/crates/rocksy-core`), the Tauri glue (`app/src-tauri`) and the frontend (`app/ui`).
Any change goes through the main session. Identifiers are in English, JSON is `camelCase`.

Revision of 23/11/2026: integrates `docs/archive/audits/2026-11-23_contrat-delta.md` (fixes from the three audits), then the final integration (0.2.1): `ApiError.code`, the `add_vault` rule, free ordering of `open_vault` and `build_tags`, `index-ready` after a save that changes links, `IndexReady.pending`, the rules for mixed line endings and for `tags_list`, the page content security policy.

## Layout

```
app/
├── Cargo.toml            workspace (crates/rocksy-core; src-tauri is excluded from it)
├── crates/rocksy-core/   core with no UI dependency (no tauri, no mandatory tokio)
├── src-tauri/            Tauri 2 commands = thin wrappers around rocksy-core
└── ui/                   Vite + TypeScript + CodeMirror 6, output in ui/dist
```

## Hard rules

- Never write into `.obsidian/`, into a `.canvas`, or next to a file outside a vault. The only write into a vault is `save_file` on a `.md` or `.markdown` (atomic: temporary file in the same folder + rename; the temporary file starts with `.`, ends with `.rocksy-tmp` and is deleted on failure).
- Only exception: when a vault is opened (`open_vault`), Rocksy may delete its own orphaned atomic-write temporary files (name starting with `.` and ending with `.rocksy-tmp`), older than one hour, in the visible folders of the vault. Nothing else is deleted, nothing is created.
- Rocksy's own data: never in the vault, never computed from the vault. Two folders (see "Application data"): the vault list and the session in the roaming data folder; derived and bulky data (tree snapshots, indexes) in the local data folder.
- The web page is not trusted: every path it sends is checked by the backend (see "File access"), and no command lets it widen its own rights (see "What the page cannot do").
- The page is served with a strict content security policy (see "Content security policy"): no inline script, no `eval`, no network connection.
- Folders and files starting with `.` are ignored in the tree.
- Paths: OS-absolute in commands; tree entries are relative to the vault root with `/` as separator.
- UTF-8 only; a non-UTF-8 file returns a readable error (no silent conversion). The original line endings and BOM are preserved on save (see "Line endings").

## Commands (`invoke`)

| Command | Arguments | Return |
|---|---|---|
| `startup_info` | — | `StartupInfo` |
| `read_file` | `path` | `FileContent`; `invalid` if the page has no right to read this path, or beyond 50 MiB |
| `save_file` | `path`, `text`, `expectedMtimeMs?` | `{ mtimeMs, size }`; `conflict` if the file changed on disk since `expectedMtimeMs`; `invalid` if the write is not allowed or the extension is neither `.md` nor `.markdown`; `io` if the file stays locked (the replacement is retried for about 2 s, the conflict check is redone before each attempt) |
| `list_vaults` | — | `VaultRef[]` |
| `add_vault` | `path` | `VaultRef[]`; `invalid` (code `not_allowed`) if `path` is neither a folder returned by `pick_folder` during this session nor an already registered vault (see "File access") |
| `remove_vault` | `path` | `VaultRef[]`; closes the vault if it is open and deletes its derived data (tree snapshot, tag and link index, search index); touches no file of the vault |
| `open_vault` | `path` | `Tree`: from the snapshot if there is one (`fromSnapshot: true`), otherwise by listing. Then starts the background check → `tree-updated` event if the tree changed. Closes the previously open vault (like `close_vault`). `invalid` if `path` is not a registered vault |
| `close_vault` | `path` | `void`: stops watching the folder, persists then releases the index of this vault. No effect if this vault is not the open one |
| `build_tags` | `vaultPath` | `void`; `invalid` if it is not a registered vault. May be called before, during or after the `open_vault` of the same vault (see "Vault index"). Makes the vault index available in the background: (1) if a persisted index exists, it is loaded without reading any note → `tags-ready` with `fromCache: true`; (2) reconciliation by size + mtime that rereads only new or modified notes → `tags-progress` (`total` = notes to reread) then `tags-ready` with `fromCache: false`. Persistence: see "Vault index" |
| `tags_list` | `query` (may be empty) | filtered and sorted `TagCount[]` (see "Sorting and filtering of `tags_list`"); a parent `a` aggregates `a/b` |
| `tag_fragments` | `tag`, `limit?` (default 500, cap 1000) | `Fragment[]` (the tag and its descendants) |
| `file_tags` | `path`, `text` | `Fragment[]` for an in-memory text (file outside a vault or not registered) |
| `search` | `query`, `limit?` (default 50 files, cap 1000) | `SearchResult`: full-text search in the indexed vault (see "Search") |
| `backlinks` | `path` | `LinkRef[]`: links and embeds from the other notes of the vault that point to this file (note or attachment), sorted by path then line |
| `outgoing_links` | `path` | `OutLink[]`: links and embeds contained in this note, in text order, resolved or not |
| `unresolved_links` | `limit?` (default 500, cap 1000) | `UnresolvedLink[]`: targets not found in the vault, with their number of occurrences |
| `set_session` | `session: Session` | `void` (persisted in the data folder); silently ignores the tabs the page has no right to read and an unregistered `activeVault` |
| `pick_folder` / `pick_file` | — | `string \| null`; the file chosen by `pick_file` becomes an explicitly opened file; the folder chosen by `pick_folder` becomes registrable by `add_vault` until the end of the session |
| `log_timing` | `name`, `ms` | `void` (startup milestones measured by the UI) |
| `open_external` | `url` | `void`: opens a link in the system browser or mail client; `http`, `https` and `mailto` only (a `mailto:` carrying an `attach` parameter is refused), otherwise `invalid` error |
| `close_ack` | — | `void`: acknowledgement of `close-requested` (see "Closing") |
| `quit` | — | `void`: closes the application cleanly (see "Closing") |

A command never leaves a promise unanswered: an internal backend failure yields an `io` error.

## Events (core → UI)

| Event | Payload |
|---|---|
| `open-files` | `{ paths: string[] }`: second launch (single instance), file association or drag-and-drop onto the window. These paths are already explicitly opened files when the event arrives |
| `tree-updated` | `Tree` |
| `tags-progress` | `{ vault, done, total }` |
| `tags-ready` | `{ vault, files, tags, occurrences, ms, fromCache, reread?, removed?, persisted? }`; may be emitted twice per `build_tags` (persisted index, then reconciled) |
| `index-ready` | `IndexReady`: state of the vault index (tags, links, search). Emitted by `build_tags` when the persisted index is loaded (`fromCache: true`), after reconciliation, when full-text search becomes available (`searchReady: true`), after each update caused by an external change, and after a `save_file` that changed the links of the vault (different `linksGeneration`; neither `reread`, nor `removed`, nor `pending` in that case; no event if the links did not move) |
| `file-changed` | `{ path, mtimeMs, deleted?: boolean }`: external change of an open file; `deleted: true` (and `mtimeMs: 0`) when the file has disappeared (deleted or moved) |
| `close-requested` | — (no payload): the user asks to close the window (see "Closing") |

`tags-progress`, `tags-ready` and `index-ready` carry `vault`: the root of the vault concerned, in the same form as `Tree.root`. The interface ignores an event whose vault is not the active vault.

## File access

The backend keeps two lists, which the page cannot modify directly: the registered vaults, and the **explicitly opened files** of the user (command-line argument, second launch, `pick_file` dialog, drag-and-drop onto the window, restored tab that had been opened that way). The list of explicitly opened files is kept and persisted by the backend (bounded; on restart, only those still present in the session tabs remain); it is never supplied by the page.

- **Read allowed** (`read_file`): under a registered vault; an explicitly opened file; and, around such a file outside a vault, the text files and images of its folder and subfolders: `.md`, `.markdown`, `.txt`, `.canvas`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`, `.bmp`, `.avif` (case-insensitive). A file placed at the root of a volume does not open the volume.
- **Write allowed** (`save_file`): under a registered vault, or in an explicitly opened file. A file reached by a link from a note outside a vault is read-only, including after a restart.
- **Network paths** (`\\server\share`, `//server/share`, `\\?\UNC\…`) coming from the page: refused, unless it is exactly an explicitly opened file or a path under a vault registered on a share. Images and links of a note located on a network share are not loaded. Device paths (`\\.\…`, `\\?\Volume{…}`, `\??\…`) are always refused.
- **Registering a vault** (`add_vault`) gives the page read and write access to the whole folder: the page therefore does not choose this folder. `add_vault` only accepts a folder returned by the native `pick_folder` dialog during the current session (list kept by the backend, bounded to 32 folders, not persisted), or an already registered vault (no new right). Only exception, for automated tests: in an isolated instance (`ROCKSY_DATA_DIR` set, hence decided by whoever launches the process and not by the page), `add_vault` accepts any folder.
- Refusals are the `invalid` error: `read_file`, `save_file` for a path not allowed; `open_vault` and `build_tags` for a folder that is not a registered vault; `add_vault` for a folder not chosen in the dialog. Those the interface must recognise carry a `code` (see "Errors"). `set_session` returns no error: it removes what is not allowed.
- `save_file` only accepts `.md` and `.markdown`; `.obsidian/` and `.canvas` remain forbidden, including through a link or a Windows short name.
- `FileContent.readOnly` is true when `save_file` would be refused for this path (non-Markdown extension, or write not allowed). The interface then opens the document read-only: no editing, no autosave.
- Open files are only watched (`file-changed`) if they are readable according to these rules: the backend never touches a path that is not allowed on behalf of the page.
- `asset:` protocol (images): limited to registered vaults and to the folders of explicitly opened files; `.obsidian/` excluded; no network path (images of a vault placed on a network share are not displayed).

### What the page cannot do

Review of all commands (0.2.1): none lets the page obtain access it does not already have.

- Obtaining a right on a file or folder always goes through a user gesture outside the page: native dialog (`pick_file`, `pick_folder`), command line, second launch, drag-and-drop. `add_vault`, `open_vault`, `build_tags`, `read_file`, `save_file` and `set_session` only check these rights.
- `remove_vault` and `close_vault` remove rights or derived data, never a user file. `quit` and `close_ack` close the application.
- `tags_list`, `tag_fragments`, `search`, `backlinks`, `outgoing_links`, `unresolved_links` only read the index of the open vault; `file_tags` reads no file (text supplied).
- `open_external` only opens `http`, `https` and `mailto`; `log_timing` adds one bounded line to `timings.jsonl`; `startup_info` and `list_vaults` only read.
- The page can only listen to events (`listen`/`unlisten`); it cannot emit them. Commands can only be called from the main window (capability `default`).

## Content security policy

`default-src 'none'`; `script-src 'self'` (neither `'unsafe-inline'` nor `'unsafe-eval'`); `style-src 'self' 'unsafe-inline'` (style attributes of the virtualised tree, CodeMirror, diagrams and tables); `img-src 'self' asset: http://asset.localhost data: blob:`; `font-src 'self' data:`; `connect-src ipc: http://ipc.localhost` (IPC only); `object-src`, `base-uri`, `form-action`, `frame-src`, `media-src`, `worker-src`: `'none'`. A remote image (`http`, `https`) is therefore never loaded. The interface records every violation in `window.rocksyCsp` (diagnostics and tests): none is expected in normal use.

## Errors

`ApiError.kind` gives the nature of the error; `message` is a human-readable text in English, which may be reworded at any time. When the interface must react to a specific cause, the error carries a stable `code`:

| `code` | `kind` | Cause |
|---|---|---|
| `too_large` | `invalid` | `read_file`: file larger than 50 MiB |
| `network_path` | `invalid` | `read_file`, `save_file`: network path not allowed |
| `not_allowed` | `invalid` | `read_file`, `save_file`: path outside the registered vaults and the opened files; `open_vault`, `build_tags`: folder that is not a registered vault; `add_vault`: folder not chosen in the dialog |
| `read_only` | `invalid` | `save_file`: extension other than `.md`/`.markdown`, `.canvas`, `.obsidian/` |
| `gone` | `conflict` | `save_file` with `expectedMtimeMs`: the file no longer exists on disk |

Other errors have no `code` (malformed path, folder instead of a file, date conflict, I/O error…). The interface never recognises an error from the text of `message`.

## Line endings

- The backend never transforms line endings: `read_file` returns the text as is, `save_file` writes the received text as is. The rule below is applied by the interface at save time.
- Homogeneous file (a single kind of line ending: LF, CRLF or CR): all lines are written with that ending. With no line break at all: LF.
- Mixed file: the ending of each line is remembered at read time. On save, unchanged lines at the head and tail of the document keep their original ending; lines modified or added in between take the file's majority ending (on a tie: LF, then CRLF). A mixed file opened then saved without modification is therefore byte-for-byte identical. After a successful save, the endings written become the reference.
- The original BOM is rewritten at the start; it is never added or removed.

## Sorting and filtering of `tags_list`

- Query: leading and trailing spaces removed, all leading `#` removed, lowercase. Empty query: all tags.
- Filter: the tag (compared in lowercase, without accent folding: `ecole` does not find `école`) contains the query.
- Sort, in three groups: (0) the tag starts with the query; (1) a level of the tag starts with it (an occurrence follows a `/`, occurrences being searched without overlap from left to right); (2) the others. Within each group: occurrence count descending (descendants included), then lowercase key in code point order.
- A parent that exists only through its children is listed. The displayed name is that of the first occurrence in the order of the vault tree.
- The interface keeps the full list (`tags_list("")`) in memory and applies this filter itself on each keystroke. The two implementations are kept identical by a shared set of cases, `app/testdata/tags_list_cases.json`, played by `app/crates/rocksy-core/tests/tags_list_cases.rs` and by `app/ui/tests/tagcases.test.ts`.

## Startup

`StartupInfo.firstFile` is the note the interface will open first: the first file passed as an argument, otherwise the active tab of the session. It is read on the backend side during the WebView startup, with the same access checks and the same effects as `read_file` (watching included). `null` if there is none, if the read failed, or if it is not finished when `startup_info` answers: the interface then falls back to `read_file`, which gives the readable error if any. The value is supplied only once (at the first `startup_info`).

## Vault index: a single read pass

`build_tags` (name kept) builds and maintains **the whole** index of a vault: tags, links, full-text search. Reading a note is the costly operation (Google Drive): each new or modified note is read **only once** and feeds all three. Tags and links are available as soon as the persisted index is loaded; full-text search may be available a little later (`searchReady`). `tags-ready` is still emitted as before.

- `open_vault` and `build_tags` of the same vault may arrive in any order (the interface sends them together): the index always arrives, and the listing made for the tree also serves the index (a single listing of the vault). The only case with two listings: `build_tags` called more than 300 ms before `open_vault`, or for a vault that is not open. The `close_vault` of the old vault, when switching vaults, may also arrive before or after: it cancels neither the opening nor the indexing of the new one.
- After `save_file`: tags and links up to date when the command returns; full-text search up to date within 2 s. The disk is not reread. If the save changed the links of the vault, an `index-ready` follows (new `linksGeneration`).
- Unread or unindexed notes: a pass may fail to read a note (file held by another program, synced disk unavailable) or lose writes of the search index (file creation refused in the index folder). These notes are counted in `IndexReady.pending`; the backend then relaunches a pass itself 2 s, then 5 s, 15 s, 30 s and 60 s later, without waiting for a change in the vault, and each pass rereads only what is missing. As long as `pending` is not zero, the interface does not show the index as up to date. Each occurrence leaves an `index-pending` line in `timings.jsonl` (number of notes, failed reads, search engine error).
- Index persistence: after a reconciliation that reread or removed notes, when the vault or the application is closed, otherwise at most every 60 s. If the application is killed in the meantime, the notes concerned are simply reread at the next launch.
- Damaged search index: never an error or a crash; `searchReady: false` during the automatic rebuild.
- `IndexReady.linksGeneration` changes when a link of the vault changes (target, position) or when the set of files changes. The interface can avoid requesting or redrawing the links again when the value has not moved.

## Search

- Case- and accent-insensitive (`resume` finds `Résumé`). Words are combined with AND. The last word typed is a prefix (search as you type: `archi` finds `architecture`).
- `"exact phrase"`; `-word` excludes; `path:text` restricts to paths containing this text; `tag:a/b` (or `tag:#a/b`) restricts to notes carrying this tag or a descendant. A query made only of filters lists the matching notes.
- The file name counts more than the body. Results sorted by relevance.
- Excerpts come from the index, without rereading the files.
- As long as search is not available, `search` returns no error: empty `hits`, `total` 0, `searchReady: false`. An empty query returns an empty result.
- `line`, `col` and `len` follow the exact text returned by `read_file` (BOM included), like tag positions.
- `limit` is capped at 1000 (`search`, `tag_fragments`, `unresolved_links`).

## Links

- Recognised: `[[target]]`, `[[target#heading]]`, `[[target#^block]]`, `[[target|alias]]`, embeds `![[...]]`, Markdown links `[text](path.md)` and images `![alt](path.png)` to a local file (`http:`, `https:`, `mailto:` etc. URLs are ignored). Not in inline code or code blocks. `[[#heading]]` (internal link to the note) is ignored.
- Resolution of a wikilink target, like Obsidian: exact path from the vault root, otherwise path suffix, otherwise file name (`.md` extension optional), case-insensitive; in case of ambiguity, the shortest path, then alphabetical order. A Markdown link is resolved relative to the note's folder, then from the root. Resolution covers all visible files of the vault (notes and attachments) and is recomputed when files appear or disappear, without rereading the notes.

## Tracking external changes

After `open_vault`, the backend listens to the vault folder (dot-folders ignored, bursts grouped: 400 ms of silence, 3 s at most). The watcher is only a trigger: on each batch, the backend makes **a single** listing of the vault (at most one pass per second), emits `tree-updated` if the tree changed, then reconciles the index with that same listing and emits `tags-ready` (`fromCache: false`, `reread`, `removed`) and `index-ready` if notes were reread or removed.

- A save made by Rocksy triggers neither a listing nor `tree-updated`, unless it creates a file.
- A batch that touches no `.md` but contains a folder or a path without a known file extension (renamed or moved folder) also relaunches the index reconciliation.
- During an indexing in progress, an external change refreshes the tree immediately; tags, links and search follow at the end of the indexing.
- Safety net: the same pass is relaunched when the window returns to the foreground, if the last one is more than 30 s old.
- Open files remain tracked by `file-changed` (polling every 2 s); an open file that disappears is reported by `deleted: true` after two consecutive polls without finding it.
- `close_vault`, opening another vault and `remove_vault` stop this tracking.

## Closing

1. The user asks to close the window (cross, Alt+F4). The window does not close by itself: the backend emits `close-requested`.
2. The interface calls `close_ack` on receipt, to signal that it is alive. Without `close_ack` within 5 s, the backend closes the application (frozen interface or page not loaded). Repeated close requests during the wait change nothing.
3. The interface saves all modified documents, waiting for the end of each save. If a save fails, if a document is in conflict, or if autosave is disabled and modifications remain, it asks for confirmation (save / quit without saving / cancel) in an application dialog (not `window.confirm`).
4. When everything is saved or the user has confirmed, the interface calls `quit`. The backend stops the watcher, cancels the indexing in progress (waiting 2 s at most), persists the index, then exits. If the user cancels, the interface calls nothing and the application stays open; a new close request restarts the sequence.

## Application data

- Roaming folder (`%APPDATA%\dev.rocksy.app`, `StartupInfo.dataDir`): `vaults.json`, `session.json`, the list of explicitly opened files (`opened.json`), `timings.jsonl` (startup milestones, one `vault-listing` line per vault listing, one `index-pending` line per incomplete pass; no note content; reset beyond 1 MiB).
- Local folder (`%LOCALAPPDATA%\dev.rocksy.app`): `trees/` (tree snapshots), `tags/` (tags and links), `search/` (full text). This data contains file names and note text: it does not travel with a roaming profile. Data that existed in the roaming folder is moved at first launch, without rebuilding.
- `ROCKSY_DATA_DIR` (tests, measurements) sets both folders to the same place.
- `remove_vault` deletes the derived data of the removed vault.

## Types

```ts
type StartupInfo = {
  argFiles: string[];          // files passed on the command line
  vaults: VaultRef[];
  session: Session | null;     // last session (active vault, tabs)
  firstFile: FileContent | null; // note to open first, already read (see "Startup")
  dataDir: string;
  version: string;
  processStartMs: number;      // epoch ms at the start of main()
};
type VaultRef = { path: string; name: string };
type Session = { activeVault: string | null; tabs: string[]; activeTab: string | null };
type FileContent = { path: string; text: string; mtimeMs: number; size: number; readOnly: boolean };
type Tree = { root: string; fromSnapshot: boolean; entries: TreeEntry[] };   // sorted: folders first, case-insensitive alphabetical order
type TreeEntry = { path: string; isDir: boolean };
type TagCount = { tag: string; count: number };      // tag without '#', original case of the 1st occurrence; case-insensitive comparison
type Fragment = {
  path: string;                // absolute
  tag: string;
  source: "inline" | "frontmatter";
  startLine: number;           // 0-based, inclusive: start of the block
  endLine: number;             // 0-based, inclusive: end of the block
  tagLine: number;             // line of the tag
  tagCol: number;              // column in UTF-16 units (like CodeMirror)
  excerpt: string;             // ≤ 240 characters, one line
};
type SearchResult = { query: string; total: number; hits: SearchHit[]; ms: number; searchReady: boolean };  // total = number of matching files (may exceed hits.length)
type SearchHit = {
  path: string;                // absolute
  score: number;
  nameMatch: boolean;          // the query matches the file name
  matchCount: number;          // occurrences in the body
  matches: SearchMatch[];      // at most 3 per file, in text order
};
type SearchMatch = {
  line: number;                // 0-based, CodeMirror line model
  col: number;                 // UTF-16 units, start of the first occurrence on the line
  len: number;                 // length of this occurrence in the line, in UTF-16 units (to select it)
  excerpt: string;             // ≤ 240 characters, one line
  ranges: [number, number][];  // ranges to highlight in excerpt (UTF-16 units, end exclusive)
};
type LinkKind = "link" | "embed";
type LinkRef = {               // an incoming link
  path: string;                // source note, absolute
  kind: LinkKind;
  line: number; col: number;   // position of the link in the source (0-based, UTF-16)
  excerpt: string;             // ≤ 240 characters, one line
  ranges: [number, number][];  // range of the link in excerpt
  heading: string | null;      // targeted heading or block (`#heading`, `#^block`), without the `#`
  alias: string | null;
};
type OutLink = {
  target: string;              // target as written, without heading or alias
  resolved: string | null;     // absolute path of the targeted file, null if not found
  kind: LinkKind;
  line: number; col: number;
  heading: string | null;
  alias: string | null;
};
type UnresolvedLink = { target: string; count: number; sources: string[] };  // sources: at most 5 paths
type TagsProgress = { vault: string; done: number; total: number };
type TagsReady = {
  vault: string;
  files: number; tags: number; occurrences: number; ms: number;
  fromCache: boolean;
  reread?: number; removed?: number; persisted?: boolean;
};
type IndexReady = {
  vault: string;               // root of the vault concerned, same form as Tree.root
  fromCache: boolean;
  files: number;               // indexed notes
  tags: number; occurrences: number;
  links: number; unresolvedLinks: number;
  linksGeneration: number;     // changes when a link or the set of files changes
  searchReady: boolean; searchDocs: number;
  reread?: number; removed?: number;
  pending?: number;            // notes the last pass could not update (reread automatically); absent outside a pass
  ms: number;
};
type FileChanged = { path: string; mtimeMs: number; deleted?: boolean };
type ApiError = {
  kind: "not_found" | "not_utf8" | "conflict" | "io" | "invalid";
  message: string;             // English, for humans; never parsed by the interface
  code?: "too_large" | "network_path" | "not_allowed" | "read_only" | "gone";  // see "Errors"
};
```

## Tag rules

- Obsidian syntax: `#` preceded by a line start or a space, followed by letters, digits, `_`, `-`, `/`; at least one non-numeric character; not in inline code, code blocks, URLs, wikilinks or link titles.
- Frontmatter: keys `tags` / `tag`, YAML list or comma-separated string; `#` optional. Fragment = whole file (`startLine: 0`, `endLine` last line).
- Fragment of an inline tag = enclosing block: paragraph; list item **with its sub-items**; lone heading; table row; whole quote/callout.
