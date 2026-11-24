# ADR 0021: Mod architecture (accepted, not implemented)

Status: accepted
Date: 2026-11-23

## Context

The plugin system is the app's own and must not slow startup.

## Decision

Everything stays a plugin/mod with one manifest and an activation mode: `post-paint` (after the first paint, time-sliced in idle time), `on-file-type`, `on-command` or `on-event`, `admin` (launched from an admin panel), or `job` (a scheduled batch run by the Rust core, no UI). Nothing loads before the first render. Default-on mods: Custom Icons, Editing Toolbar, TagFolder (Outliner proposed). Excalidraw loads only when an `.excalidraw` or `.excalidraw.md` file opens. Tag Wrangler is admin-only. Templater is simple templates only (no `child_process`, no user scripts). Wispr Flow Sync becomes a native Rust `job`, next to a new read-only orphan-media scan job built on the persisted link graph. Jobs that write to the vault run on one designated machine only (Drive sync would duplicate their output). Mods that draw UI must not shift the layout when they load. Extra KPI: typing latency while mods load. Reference model: Herdr desktop (mods as JS describing native elements, one folder per mod, hot reload with fallback to the last valid version, `check`/`status`/`shot` CLI commands, generated `AGENTS.md` and `.d.ts`); Rocksy adds declared permissions (vault writes, network) in the manifest since agents will write mods.

## Consequences

- Not implemented: no mod code exists in the repository (`docs/status.md`: "Not implemented: mods and store").
- Full matrix: `docs/archive/m1-plugins-matrix.md` (French, proposed verdicts).

## Sources

The mod-architecture paragraph of the previous `CLAUDE.md` (in git history), recorded there as the owner's decisions of 2026-11-23; `docs/archive/m1-plugins-matrix.md`.
