# Spec: file access

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; invariants taken from the contract and the tests as they stand, the test suite passed at 244 core and 124 UI tests)

## Scope

Which paths the page (the WebView, untrusted) may read or write, how a vault is registered, and how errors are reported. Interface details (commands, error table) are in `app/CONTRACT.md` ("File access", "Errors"); this file lists the invariants and their tests. Rationale: `docs/decisions/0011-page-access-policy-and-csp.md`, `docs/decisions/0003-vault-read-only-own-registry.md`.

## Invariants

| # | Invariant | Enforced by |
|---|---|---|
| FA-1 | The backend keeps two lists the page cannot modify: registered vaults and explicitly opened files (command line, second launch, `pick_file`, drag and drop, restored tab). The opened list is kept and persisted by the backend, bounded, and after restart only entries still present in session tabs remain | `access_restores_only_what_the_backend_persisted_as_opened`, `opened_list_is_bounded_ordered_and_reports_changes`, `only_persisted_opened_files_that_are_still_tabs_are_restored` (`app/crates/rocksy-core/tests/access_policy.rs`, `app/crates/rocksy-core/src/access.rs`) |
| FA-2 | Read is allowed under a registered vault, for an opened file, and around an opened file outside a vault only for text and image files (`.md`, `.markdown`, `.txt`, `.canvas`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`, `.bmp`, `.avif`) in its folder and subfolders | `access_guards_read_and_save_around_vaults_and_opened_files`, `around_an_opened_file_only_text_and_images_are_readable` |
| FA-3 | A file at a volume root does not open the volume | `file_at_a_volume_root_does_not_open_the_volume` |
| FA-4 | Write is allowed only under a registered vault or for an explicitly opened file (and only `.md`/`.markdown`, see `docs/specs/saving.md`). A file reached by a link from a note outside a vault is read-only, also after restart | `opened_file_gives_read_on_its_folder_and_write_on_itself_only`, `access_guards_read_and_save_around_vaults_and_opened_files` |
| FA-5 | Network paths from the page (UNC, `//server/share`, `\\?\UNC\`) are refused unless exactly an opened file or under a vault registered on a share; device paths are always refused; malformed paths are `invalid` | `network_paths_from_the_page_are_refused`, `network_file_opened_by_the_user_is_allowed_alone`, `vault_on_a_network_share_is_allowed`, `malformed_and_device_paths_are_invalid` |
| FA-6 | Path comparison on Windows folds case, separators, dots and prefixes; on Unix it is case-sensitive and `..`-proof | `windows_keys_fold_case_separators_and_dots`, `network_forms_share_one_key`, `vault_paths_are_allowed_whatever_the_spelling`, `unix_vault_is_case_sensitive_and_dotdot_proof`, `access_windows_rules_hold_on_any_platform` |
| FA-7 | `add_vault` accepts only a folder returned by `pick_folder` during the current session (list bounded to 32, not persisted) or an already registered vault; in an isolated instance (`ROCKSY_DATA_DIR` set) it accepts any folder | `only_a_picked_folder_can_become_a_vault`, `an_isolated_instance_registers_any_folder` (`app/crates/rocksy-core/src/access.rs`); UI mock: `add_vault only accepts a folder returned by the folder dialog (or a vault already registered)` (`app/ui/tests/sync.test.ts`). Real-app dialogs: not covered (see `docs/status.md`) |
| FA-8 | `open_vault` and `build_tags` refuse a folder that is not a registered vault (`not_allowed`) | `refusals_carry_a_stable_code` (`app/crates/rocksy-core/src/access.rs`); end-to-end in real-app scenarios `a5-security` (`app/ui/tools/app/a5-security.json`) |
| FA-9 | `set_session` returns no error: it drops tabs the page may not read and an unregistered `activeVault` | `page_sessions_are_filtered` |
| FA-10 | Refusals carry a stable `code` (`too_large`, `network_path`, `not_allowed`, `read_only`, `gone`); the UI never matches `message` | `refusals_carry_a_stable_code`, `error_and_payload_json_shapes` (`app/crates/rocksy-core/tests/contract.rs`), `recognises errors by their code, whatever the wording of the message` |
| FA-11 | `read_file` refuses files over 50 MiB without reading them, and non-UTF-8 content with a readable error (no lossy conversion) | `read_refuses_a_file_over_the_limit_without_reading_it`, `read_refuses_files_over_the_size_limit` (`app/crates/rocksy-core/tests/files_integrity.rs`), `read_rejects_non_utf8_and_missing_and_directory` |
| FA-12 | `FileContent.readOnly` is true whenever `save_file` would be refused for that path | mock only: `the mock answers like the contract: readOnly, invalid paths, size limit, Markdown-only saves`. Core side: not covered by a dedicated test |
| FA-13 | The `asset:` protocol serves only registered vaults and folders of opened files, excludes `.obsidian/`, refuses network paths | roots: `asset_roots_are_local_vaults_and_folders_of_opened_files`. Protocol filtering in the shell: no unit test; checked in the 0.2.1 real-app run (`runs/2026-11-23_2240_integration-audits/notes.md` section 2), with the known 1.3 s delay on network paths |
| FA-14 | The page can only listen to events (capability `core:event:allow-listen`/`allow-unlisten`) and call the app's own commands from the main window | `tools/check-consistency.py` (command surface identical in capability file, handler, build script, contract); the capability semantics themselves are not covered by a test |
| FA-15 | `open_external` opens only `http`, `https` and `mailto` (a `mailto:` with an `attach` parameter is refused) | `external_links_are_web_and_plain_mail_only` (`app/crates/rocksy-core/src/files.rs`) |

## Code

`app/crates/rocksy-core/src/access.rs` (policy), `app/crates/rocksy-core/src/paths.rs` (path keys), `app/crates/rocksy-core/src/files.rs`, `app/crates/rocksy-core/src/vaults.rs`, `app/crates/rocksy-core/src/session.rs`, `app/src-tauri/src/main.rs` (command wrappers, `asset:`), `app/src-tauri/capabilities/default.json`.
