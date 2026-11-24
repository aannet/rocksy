# Spec: external changes

Status: normative
Last verified at: release 0.2.1 (branch kb-restructure; invariants taken from the contract and the tests as they stand, the test suite passed at 244 core and 124 UI tests)

## Scope

Detection of changes made by other tools (Google Drive, an AI cleaner, the file explorer) while Rocksy is open, and the rule that the app's own saves do not trigger work. Rationale: `docs/decisions/0007-watcher-trigger-only.md`, `docs/decisions/0015-own-saves-and-shared-listing.md`. Contract section: "External-change tracking" in `app/CONTRACT.md`.

## Invariants

| # | Invariant | Enforced by |
|---|---|---|
| EC-1 | The watcher reports only visible files: events in dot-folders and outside the root are ignored; create, modify, remove and rename of visible files produce a batch; a folder created after start is watched too | `changes_in_dot_folders_produce_no_batch`, `create_modify_remove_rename_of_visible_files`, `folder_created_after_start_is_watched_too`, `hidden_outside_and_access_events_are_ignored` (`app/crates/rocksy-core/tests/watch.rs`, `app/crates/rocksy-core/src/watch.rs`) |
| EC-2 | Bursts are debounced into few batches that still carry every path; continuous activity still emits batches at the maximum wait; beyond the cap a batch is truncated and the watcher settles | `burst_of_300_writes_gives_few_batches_and_every_path`, `continuous_activity_still_emits_batches_every_max_wait`, `burst_beyond_the_cap_yields_truncated_batches_and_settles` (`app/crates/rocksy-core/tests/watch_burst.rs`), `path_count_is_capped` |
| EC-3 | An atomic save reports only the saved note | `atomic_save_reports_only_the_saved_note` |
| EC-4 | The watcher never modifies the vault; it can be dropped at any time, including during a burst and from its own callback; a removed root does not panic; start errors are clean | `watching_a_vault_does_not_modify_it`, `drop_stops_immediately_even_during_a_burst`, `watcher_can_be_dropped_from_its_own_callback`, `root_removed_while_watching_does_not_panic`, `start_errors_are_clean` |
| EC-5 | The watcher is only a trigger: each batch leads to one listing of the vault (at most one pass per second), and the index is reconciled against that same listing | `own_save_skips_the_listing_and_anything_else_lists_once`, `reconcile_with_a_shared_listing_matches_reconcile`. The one-pass-per-second limit and the batch window values live in the shell and `app/crates/rocksy-core/src/watch.rs`: the 400 ms and 3 s values are not covered by a dedicated test |
| EC-6 | A save by the app triggers no listing and no `tree-updated` unless it creates a file; a batch with any other path, a created file, a folder rename or an expired record goes the normal way; own-write records expire (10 s), are bounded (256) and compared with Windows path keys | `own_writes_let_the_worker_skip_the_listing_after_an_app_save`, `own_writes_never_hide_an_external_change`, `own_writes_do_not_skip_a_file_created_by_the_app`, `own_writes_do_not_hide_a_folder_rename`, `own_writes_expire_and_follow_the_real_stat` (`app/crates/rocksy-core/tests/own_writes_watch.rs`), `records_expire`, `registry_is_bounded_and_drops_the_oldest`, `windows_paths_match_whatever_the_case_separator_or_prefix` (`app/crates/rocksy-core/src/own_writes.rs`) |
| EC-7 | A batch that touches no `.md` but holds a folder or an extensionless path (renamed or moved folder) also reconciles the index | `folder_rename_is_reindexed_from_the_shared_listing` (`app/crates/rocksy-core/tests/refresh_pass.rs`), `folder_filter_drops_folder_modifications_only` |
| EC-8 | The tree sent to the page carries no size or mtime (`TreeEntry` is `{path, isDir}`) | `tree_sent_to_the_page_has_no_size_nor_mtime` |
| EC-9 | During an indexation in progress, an external change refreshes the tree at once; tags, links and search follow when the indexation ends | not covered by a test (stated in the contract) |
| EC-10 | Safety net: the same pass is rerun when the window regains focus if the last one is older than 30 s | not covered by a test (shell behaviour) |
| EC-11 | Open files are polled every 2 s and reported by `file-changed`; a vanished file is reported with `deleted: true` after two consecutive polls without finding it | not covered by a test (shell); UI side: `is flagged and never recreated silently` (`app/ui/tests/sync.test.ts`) |
| EC-12 | `close_vault`, opening another vault and `remove_vault` stop the tracking, and events of a vault other than the active one are ignored by the UI | `every index event names its vault, and events of a vault that is not the active one are ignored`, `close_vault stops the events of that vault (BUG-16)` |
| EC-13 | Listing: sorted, dot entries skipped, symlinks neither followed nor listed; a listing thread panic fails the listing and removes nothing; a cancelled listing gives no tree | `listing_is_sorted_and_skips_hidden_entries`, `symlinks_are_not_followed_nor_listed`, `a_panic_in_a_listing_thread_fails_the_listing_and_removes_nothing`, `a_cancelled_listing_gives_no_tree` (`app/crates/rocksy-core/src/tree.rs`) |

Unverified: behaviour on Google Drive (notification delivery), see `docs/status.md`.

## Code

`app/crates/rocksy-core/src/watch.rs`, `app/crates/rocksy-core/src/own_writes.rs`, `app/crates/rocksy-core/src/refresh.rs`, `app/crates/rocksy-core/src/tree.rs`, `app/src-tauri/src/main.rs` (refresh worker, file poller, focus safety net).
