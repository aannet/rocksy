# ADR 0011: Page access policy and strict content security policy

Status: accepted
Date: 2026-11-23

## Context

The security audit of 0.2.0 found that a booby-trapped canvas or note could run code in the app and that a compromised page could reach any file or register a vault with full access.

## Decision

The page is untrusted: every path it sends is checked by the backend. Readable: under a registered vault, an explicitly opened file (command line, second launch, `pick_file`, drag and drop, restored tab), and around such a file outside a vault its folder and subfolders for text files and images only. Writable: under a registered vault or an explicitly opened file; a file reached by a link from a note outside a vault stays read-only, also after restart. Network paths from the page are refused (except an opened file or a vault on a share); device paths always refused. `add_vault` accepts only a folder returned by the native dialog during the session, or an already registered vault, except in an isolated instance (`ROCKSY_DATA_DIR`, decided by whoever launches the process). The page is served with `default-src 'none'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'`, `connect-src ipc: http://ipc.localhost` and closed `object-src`, `base-uri`, `form-action`, `frame-src`, `media-src`, `worker-src`. Capabilities list only the app's own commands and event listen/unlisten; the page cannot emit events.

## Consequences

- Inline styles stay allowed because Tauri adds a nonce to `style-src` when one is present, which would make browsers ignore `'unsafe-inline'` and break style attributes of the virtual tree, Mermaid SVG and tables. The audit offered two fixes; the config applies the simpler one (`dangerousDisableAssetCspModification` for `style-src`). The stricter alternative is a code change not done.
- `asset:` requests to a network path are refused only after about 1.3 s (`docs/status.md`).
- Details: `docs/specs/file-access.md`, `docs/specs/ui-safety.md`.
- The audit analysis mentions the choice between two CSP fixes; no journal decision entry records the choice: the evidence is `app/src-tauri/tauri.conf.json`.

## Sources

Journal 2026-11-23 17:45 and 19:03; `docs/archive/audits/2026-11-23_coherence.md` section 2.4; `docs/archive/audits/2026-11-23_contrat-delta.md` section 1; `app/CONTRACT.md`; `app/src-tauri/tauri.conf.json`; `app/src-tauri/capabilities/default.json`.
