//! Rocksy core: everything the app does that is not user interface.
//!
//! The crate has no UI dependency (no tauri, no async runtime). Every function
//! that persists Rocksy's own data takes the application data directory
//! (`data_dir`) explicitly; nothing is ever derived from, or written next to,
//! a vault. The only write that can land inside a vault is [`save_file`].
//! The only dependency beyond serialization is `notify`, for [`watch_vault`].
//!
//! All public data types serialize to the camelCase JSON described in
//! `CONTRACT.md`.

mod atomic;
mod excerpt;
pub mod error;
pub mod files;
pub mod paths;
pub mod search;
pub mod session;
pub mod tags;
pub mod tree;
pub mod vaults;
pub mod watch;

#[cfg(test)]
pub(crate) mod testutil;

pub use error::{ApiError, ErrorKind, Result};
pub use files::{read_file, save_file, FileContent, SaveResult};
pub use search::{SearchHit, SearchMatch, SearchResult};
pub use session::{load_session, save_session, Session};
pub use tags::{
    file_tags, tags_index_path, BuildOptions, CancelToken, Fragment, IndexReady, LinkKind, LinkRef,
    OutLink, ReconcileReport, TagCount, TagIndex, TagSource, TagStats, TagsProgress, TagsReady,
    UnresolvedLink, VaultIndex,
};
pub use tree::{
    list_tree, load_tree_snapshot, open_tree, reconcile_tree, save_tree_snapshot,
    tree_snapshot_path, Tree, TreeEntry,
};
pub use vaults::{
    add_vault, find_vault, list_vaults, remove_vault, vault_for_path, VaultRef,
};

pub use watch::{watch_vault, ChangeBatch, VaultWatcher, WatchOptions};

/// Crate version (the app version reported in `StartupInfo.version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
