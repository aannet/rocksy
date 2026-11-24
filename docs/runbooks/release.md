# Runbook: release

Status: living
Last verified at: release 0.2.1 (branch kb-restructure; commands re-run during the restructuring: core tests, clippy, UI build, vitest, cross-compile build)

Sources: `release/README.md`, `docs/handoffs/code-signing.md`, and the 2026-11-23 session journal (`docs/archive/session-2026-11-23/etat.js`). The exact commands of past releases were not written down; what is not sourced below is marked as such.

## Version bump

The version `0.2.1` appears in these places (checked on this branch):

- `app/Cargo.toml` (workspace package version)
- `app/src-tauri/Cargo.toml`
- `app/src-tauri/tauri.conf.json`
- `app/ui/package.json`
- `release/README.md` (title and release notes)

Cargo lock files follow the manifests. Persisted formats carry their own version (see `docs/impacts.md`); a format bump forces a rebuild on first launch and belongs in `docs/status.md` and the release notes.

## Build and test

1. All checks of `AGENTS.md` "Done when" pass.
2. Build the exe with `tools/build-win.sh` into a work folder (see `docs/runbooks/build.md`).
3. Play the real-app scenarios on that exe in isolated instances (`docs/runbooks/test.md`). Write a dated run note under `runs/`.
4. Dependency audit (`cargo-audit` or `cargo-deny`) before each release is a recommendation of the security audit that was never set up (SEC-17 in `docs/status.md`).

## Package

- Delivery is a zip with `rocksy.exe`, `WebView2Loader.dll`, `install.ps1` and the README (`docs/handoffs/code-signing.md`, section 1). Binaries are not versioned in git.
- `rocksy.exe` is not signed (see the code-signing handoff).
- Per-release folders of the owner's test area (`release-<version>`) hold the shipped exe; they are read-only references.

## GitHub release

- The repository is public (`origin` on GitHub). Past versions are tagged `v0.1.1`, `v0.1.2`, `v0.2.0`, `v0.2.1`; a tag is created at the owner's request when the version is committed.
- The zip is uploaded by hand to a GitHub release (code-signing handoff, section 1). The GitHub CLI `gh` is installed in the user's home. No written trace of the exact `gh release` command or of the release notes template.
- No CI exists (no `.github/`).

## Public history rule

The public repository was published from a copy of the history cleaned beforehand: the vault audit removed, the name of the working note and of the vault replaced, the author address replaced by a noreply address (journal entry of 2026-11-23, 23:16). Consequences: never reintroduce those names, never add `in/audit-vault-obsidian.*`, and check `git log` authorship before any push. The cleaning procedure itself was not written down.
