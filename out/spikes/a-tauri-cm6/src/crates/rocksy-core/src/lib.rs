//! Rocksy core: everything the app does that is not user interface.
//!
//! The crate has no UI dependency (no tauri, no async runtime). Every function
//! that persists Rocksy's own data takes the application data directory
//! (`data_dir`) explicitly; nothing is ever derived from, or written next to,
//! a vault. The only write that can land inside a vault is [`save_file`];
//! the only deletion is that of its own orphaned temporary files
//! ([`clean_vault_temp_files`]). Small data that follows the user (vault
//! list, session) and large derived data (tree snapshots, indexes) can live
//! in two directories: see [`appdata`].
//! The only dependency beyond serialization is `notify`, for [`watch_vault`].
//!
//! All public data types serialize to the camelCase JSON described in
//! `CONTRACT.md`.

pub mod access;
pub mod appdata;
mod atomic;
mod excerpt;
pub mod error;
pub mod files;
pub mod own_writes;
pub mod paths;
pub mod refresh;
pub mod search;
pub mod session;
pub mod tags;
pub mod tree;
pub mod vaults;
pub mod watch;

#[cfg(test)]
pub(crate) mod testutil;

pub use access::{load_opened, save_opened, AccessPolicy, PathOrigin, PathStyle};
pub use appdata::{
    clean_vault_temp_files, migrate_derived_data, purge_vault_data, remove_stale_temp_files, DerivedData,
};
pub use error::{ApiError, ErrorCode, ErrorKind, Result};
pub use files::{check_external_url, is_savable_name, read_file, save_file, FileContent, SaveResult};
pub use own_writes::{ChangedPaths, FileStamp, OwnWrites};
pub use refresh::{refresh_pass, RefreshOutcome};
pub use search::{SearchHit, SearchMatch, SearchResult};
pub use session::{load_session, save_session, Session};
pub use tags::{
    file_tags, tags_index_path, BuildOptions, CancelToken, Fragment, Housekeeping, IndexReady, LinkKind,
    LinkRef, OutLink, ReconcileOutcome, ReconcileReport, TagCount, TagIndex, TagSource, TagStats,
    TagsProgress, TagsReady, UnresolvedLink, VaultIndex, MAX_RESULTS, SEARCH_COMMIT_DELAY, TAGS_SAVE_DELAY,
};
pub use tree::{
    list_tree, list_tree_cancellable, load_tree_snapshot, open_tree, reconcile_tree, refresh_tree,
    save_tree_snapshot, tree_snapshot_path, update_snapshot_entry, IpcTree, Tree, TreeEntry, TreeRefresh,
};
pub use vaults::{
    add_vault, find_vault, list_vaults, remove_vault, vault_for_path, VaultRef,
};

pub use watch::{watch_vault, ChangeBatch, VaultWatcher, WatchOptions};

/// Crate version (the app version reported in `StartupInfo.version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
