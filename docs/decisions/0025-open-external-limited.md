# ADR 0025: External links open only http, https and mailto

Status: accepted
Date: 2026-11-23

## Context

`window.open` does nothing under Tauri, and a note must not be able to launch a program or a local file through a link.

## Decision

A command `open_external(url)` opens a link in the system browser or mail client, limited to `http`, `https` and `mailto` (a `mailto:` carrying an `attach` parameter is refused). Remote images are never loaded: no network call from the page.

## Consequences

- The actual launch of a link was not tested automatically (it would open a tab in the owner's browser); the call is verified, the launch is not.
- Test: `external_links_are_web_and_plain_mail_only` in `app/crates/rocksy-core/src/files.rs`.

## Sources

Journal 2026-11-23 12:16; `app/CONTRACT.md`.
