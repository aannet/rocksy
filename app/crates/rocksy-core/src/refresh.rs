//! One refresh pass of an open vault, as the shell's worker runs it for each
//! batch of the watcher (and for the focus safety net).
//!
//! The order is fixed here so that it is tested: (1) a batch made only of
//! the application's own saves is dropped without touching the vault;
//! (2) otherwise the vault is listed **once**; (3) the registry of own
//! writes learns that a listing ran. The caller then shows the tree and
//! hands the same listing to
//! [`TagIndex::reconcile_with_tree`](crate::TagIndex::reconcile_with_tree):
//! every pass that lists also reconciles the index, whatever the batch
//! contained. A folder that was renamed or moved reports no `.md` path, yet
//! every note below it changed path; an attachment that appeared changes
//! what links resolve to. With the listing shared, that reconcile reads no
//! note unless one changed.

use crate::error::Result;
use crate::own_writes::{ChangedPaths, OwnWrites};
use crate::tree::{refresh_tree, Tree, TreeRefresh};
use std::path::Path;
use std::time::Instant;

/// What [`refresh_pass`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The changes were only saves made by the application, still intact on
    /// disk: nothing was listed, the tree and the index are already right.
    OwnWrites,
    /// `cancelled` answered `true` during the listing; nothing was written.
    Cancelled,
    /// The vault was listed. Show `tree` if `changed`, then reconcile the
    /// index with it.
    Listed(TreeRefresh),
}

/// Runs one pass on the vault `root`.
///
/// * `changes`: what the watcher reported since the last pass, merged
///   ([`ChangedPaths::merge`]); [`ChangedPaths::unknown`] for a request that
///   does not come from the watcher, which always lists.
/// * `known`: the tree of the last listing (or the snapshot shown at
///   opening) if the caller keeps it in memory; it saves reading the
///   snapshot back from `derived_dir` to compare.
/// * `own`: the registry fed by `save_file`.
///
/// Errors: those of the listing (drive not mounted); nothing was written.
pub fn refresh_pass(
    derived_dir: &Path,
    root: &Path,
    known: Option<&Tree>,
    changes: &ChangedPaths,
    own: &OwnWrites,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<RefreshOutcome> {
    if own.only_own_writes(changes) {
        return Ok(RefreshOutcome::OwnWrites);
    }
    // Taken before the listing starts: a file created by a save recorded
    // after this instant may be missing from the listing.
    let started = Instant::now();
    match refresh_tree(derived_dir, root, known, cancelled)? {
        Some(refresh) => {
            own.listing_started_at(started);
            Ok(RefreshOutcome::Listed(refresh))
        }
        None => Ok(RefreshOutcome::Cancelled),
    }
}
