# Knowledge base map

Status: living
Last verified at: release 0.2.1 (branch kb-restructure)

Start with `AGENTS.md`. Every normative or living document starts with two header fields: `Status:` and `Last verified at:`. ADRs carry their own `Status:` (accepted, or superseded by NNNN) and `Date:` and are never edited once accepted.

Status values: `normative` (the rule; code and tests follow it), `living` (kept current, describes the present state), `historical` (frozen, French, not authoritative).

| Document | Status | Last verified at | What it is |
|---|---|---|---|
| `AGENTS.md` | normative | release 0.2.1 | Entry point: rules, layout, commands, workflow, "done when" |
| `CLAUDE.md` | living | release 0.2.1 | Imports `AGENTS.md`, plus Claude Code notes |
| `app/CONTRACT.md` | normative | release 0.2.1 | The IPC contract between core, shell and UI: commands, events, types, errors |
| `docs/specs/file-access.md` | normative | release 0.2.1 | Who may read or write which path; vault registration |
| `docs/specs/saving.md` | normative | release 0.2.1 | Atomic save, conflicts, retries, line endings |
| `docs/specs/vault-index.md` | normative | release 0.2.1 | Tags, links, search, persistence, reconciliation |
| `docs/specs/external-changes.md` | normative | release 0.2.1 | Watcher, own writes, shared listing |
| `docs/specs/startup.md` | normative | release 0.2.1 | Startup order and targets |
| `docs/specs/shutdown.md` | normative | release 0.2.1 | Close handshake |
| `docs/specs/ui-safety.md` | normative | release 0.2.1 | Canvas, inline rendering limits, CSP |
| `docs/decisions/` | normative | release 0.2.1 | 25 ADRs, `0001` to `0025` (see the list below) |
| `docs/impacts.md` | normative | release 0.2.1 | If you touch X, also update Y and run Z |
| `docs/runbooks/build.md` | living | release 0.2.1 | Toolchain, cross-compilation |
| `docs/runbooks/test.md` | living | release 0.2.1 | Core, UI, browser bench, real app, synthetic vaults |
| `docs/runbooks/measure.md` | living | release 0.2.1 | Startup timing scripts, run folders |
| `docs/runbooks/release.md` | living | release 0.2.1 | Version bump, package, GitHub release |
| `docs/status.md` | living | release 0.2.1 | Unverified items, known defects, open findings, owner-only measurements |
| `docs/handoffs/code-signing.md` | living | not applicable | Open handoff: signing `rocksy.exe` |
| `release/README.md` | living | release 0.2.1 | Install and usage, shipped with the release |
| `app/crates/rocksy-core/README.md`, `app/ui/README.md` | living | 0.2.1 | French component readmes (no header), moved with the code, not translated |
| `docs/archive/` | historical | not applicable | First sessions: plan, audits, spikes, dashboard (French, untouched) |
| `runs/` | historical | not applicable | Dated measurement and integration runs, never overwritten |
| `data/` | historical | not applicable | Counters about the real vault (no note content) |
| `in/` | historical | not applicable | Inputs: project brief (French); the vault audit files are untracked on purpose |

"release 0.2.1" means the sources of version 0.2.1 (tag `v0.2.1`), as checked on the branch `kb-restructure`.

## Decisions (ADRs)

| ADR | Title |
|---|---|
| `docs/decisions/0001-stack-tauri-codemirror.md` | UI stack: Tauri 2 and CodeMirror 6 |
| `docs/decisions/0002-core-independent-of-ui.md` | Rust core independent of the UI and of Tauri |
| `docs/decisions/0003-vault-read-only-own-registry.md` | The vault is read-only, with an own vault registry |
| `docs/decisions/0004-startup-order-and-scale.md` | Startup order and the 20x scale requirement |
| `docs/decisions/0005-one-vault-index.md` | One index per vault, each changed note read once |
| `docs/decisions/0006-change-detection-size-mtime.md` | Change detection by size and mtime, without hashing |
| `docs/decisions/0007-watcher-trigger-only.md` | The folder watcher is a trigger only |
| `docs/decisions/0008-fulltext-search-tantivy.md` | Full-text search with tantivy, body stored, deferred commit |
| `docs/decisions/0009-link-resolution-obsidian-style.md` | Link resolution follows Obsidian |
| `docs/decisions/0010-derived-data-local-folders.md` | Derived data in local app data, purged with the vault |
| `docs/decisions/0011-page-access-policy-and-csp.md` | Page access policy and strict content security policy |
| `docs/decisions/0012-markdown-only-saves-and-line-endings.md` | Saves limited to Markdown; line endings preserved |
| `docs/decisions/0013-close-handshake.md` | Three-step close handshake with a 5 s guard |
| `docs/decisions/0014-panic-unwind-and-index-rebuild.md` | panic = unwind and automatic rebuild of a damaged search index |
| `docs/decisions/0015-own-saves-and-shared-listing.md` | The app ignores its own saves in the watcher; one listing is shared by tree and index |
| `docs/decisions/0016-stable-error-codes.md` | Stable error codes instead of message matching |
| `docs/decisions/0017-cross-compilation-from-wsl.md` | Build for Windows by cross-compiling from WSL |
| `docs/decisions/0018-testing-real-app-isolated-instance.md` | Real-app testing only as an isolated instance, driven over CDP |
| `docs/decisions/0019-install-script-no-installer.md` | Per-user install script, no installer |
| `docs/decisions/0020-functional-scope-v1.md` | Functional scope of version 1 |
| `docs/decisions/0021-mod-architecture.md` | Mod architecture (accepted, not implemented) |
| `docs/decisions/0022-public-repo-cleaned-history.md` | Public repository with a cleaned history |
| `docs/decisions/0023-english-knowledge-base.md` | English knowledge base, spec first, archive untouched |
| `docs/decisions/0024-mermaid-pele.md` | Mermaid rendered by Pele, loaded on demand |
| `docs/decisions/0025-open-external-limited.md` | External links open only http, https and mailto |
