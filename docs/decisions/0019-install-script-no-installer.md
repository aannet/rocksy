# ADR 0019: Per-user install script, no installer

Status: accepted
Date: 2026-11-23

## Context

An installer needs extra tooling and adds nothing for personal use.

## Decision

Release 0.1 ships a PowerShell script (`release/install.ps1`): it copies the app to `%LOCALAPPDATA%\Programs\Rocksy`, creates a Start menu shortcut and adds Rocksy to "Open with" for `.md` and `.markdown`, writing only to `HKCU`, with no admin rights. `bundle.active` is false: no NSIS or MSI. Windows 11 forbids a script from changing the default application: the user picks "Always open with Rocksy" once by hand. The agent never runs this script (it writes to the user's registry).

## Consequences

- The script has never been executed on a machine (`docs/status.md`).
- Delivery is a zip uploaded by hand (`docs/runbooks/release.md`); the exe is unsigned (`docs/handoffs/code-signing.md`).

## Sources

Journal 2026-11-23 11:42; `release/README.md`.
