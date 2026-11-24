//! Housekeeping of Rocksy's own data: where the derived data lives, moving
//! it there, deleting it with its vault, and removing the temporary files
//! an interrupted atomic write left behind.
//!
//! Two directories are involved. The roaming one holds what is small and
//! follows the user (`vaults.json`, `session.json`, `opened.json`). The local
//! one holds what is derived from the vaults and large: tree snapshots
//! (`trees/`), tags and links (`tags/`), full-text index (`search/`), which
//! contain file names and the text of the notes and must not travel with a
//! roaming profile. Every function of the crate that reads or writes those
//! three takes the directory returned by [`migrate_derived_data`].

use crate::atomic::TMP_SUFFIX;
use crate::error::{ApiError, Result};
use crate::paths::{absolute_string, path_key};
use crate::search::search_dir;
use crate::tags::tags_index_path;
use crate::tree::{tree_snapshot_path, Tree};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Sub-folders of the data directory that hold derived data.
pub const DERIVED_DIRS: [&str; 3] = ["trees", "tags", "search"];

/// Age beyond which a temporary file of the atomic write is an orphan: no
/// save takes an hour.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);

/// Result of [`migrate_derived_data`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedData {
    /// Directory to hand to every function that uses `trees/`, `tags/` or
    /// `search/` during this run.
    pub dir: PathBuf,
    /// Folders moved from the roaming directory to the local one.
    pub moved: Vec<&'static str>,
    /// Stale roaming copies deleted because the local directory already had
    /// that folder.
    pub dropped: Vec<&'static str>,
    /// Why the move could not be done, if it could not.
    pub error: Option<String>,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (absolute_string(a), absolute_string(b)) {
        (Ok(a), Ok(b)) => path_key(&a) == path_key(&b),
        _ => a == b,
    }
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).map(|mut entries| entries.next().is_none()).unwrap_or(false)
}

/// Moves the derived data of earlier versions (`trees/`, `tags/`, `search/`
/// under the roaming directory) to the local directory, by renaming each
/// folder: nothing is copied, read or rebuilt. Call it once at launch,
/// before anything reads those folders.
///
/// * Same directory twice (`ROCKSY_DATA_DIR`), or nothing to move: the
///   local directory is used and nothing is touched.
/// * A folder that already exists (and is not empty) in the local directory
///   is the one in use: the roaming copy is stale and is deleted.
/// * If a rename fails (the two directories are on different volumes, a
///   file is held open), nothing is lost: the folders already moved are
///   moved back and this run keeps using the **roaming** directory, as
///   before. The move is tried again at the next launch.
pub fn migrate_derived_data(roaming: &Path, local: &Path) -> DerivedData {
    let mut out = DerivedData { dir: local.to_path_buf(), moved: Vec::new(), dropped: Vec::new(), error: None };
    if same_dir(roaming, local) {
        return out;
    }
    let pending: Vec<&'static str> = DERIVED_DIRS.into_iter().filter(|name| roaming.join(name).is_dir()).collect();
    if pending.is_empty() {
        return out;
    }
    if let Err(e) = fs::create_dir_all(local) {
        out.dir = roaming.to_path_buf();
        out.error = Some(format!("cannot create {}: {e}", local.display()));
        return out;
    }
    for name in pending {
        let (from, to) = (roaming.join(name), local.join(name));
        if to.is_dir() {
            if is_empty_dir(&to) {
                let _ = fs::remove_dir(&to);
            } else {
                if fs::remove_dir_all(&from).is_ok() {
                    out.dropped.push(name);
                }
                continue;
            }
        }
        match fs::rename(&from, &to) {
            Ok(()) => out.moved.push(name),
            Err(e) => {
                out.error = Some(format!("cannot move {} to {}: {e}", from.display(), to.display()));
                // All or nothing: one directory serves the three folders.
                let moved = std::mem::take(&mut out.moved);
                for back in moved {
                    if fs::rename(local.join(back), roaming.join(back)).is_err() {
                        out.moved.push(back);
                    }
                }
                out.dir = roaming.to_path_buf();
                return out;
            }
        }
    }
    out
}

/// Deletes what Rocksy derived from the vault `root` and stored in
/// `derived_dir`: its tree snapshot, its tags and links, its full-text
/// index. Nothing in the vault is touched, and the data of other vaults
/// stays. Close the index of that vault first (the full-text index keeps
/// files open). Returns the number of items deleted (0 to 3).
///
/// Errors: `io` if something exists and could not be deleted (the rest is
/// deleted anyway).
pub fn purge_vault_data(derived_dir: &Path, root: &Path) -> Result<usize> {
    let root = absolute_string(root)?;
    let mut removed = 0;
    let mut failure = None;
    let mut done = |path: PathBuf, result: std::io::Result<()>| match result {
        Ok(()) => removed += 1,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => failure = Some(ApiError::from_io(&e, format!("cannot delete {}", path.display()))),
    };
    let tree = tree_snapshot_path(derived_dir, &root);
    let result = fs::remove_file(&tree);
    done(tree, result);
    let tags = tags_index_path(derived_dir, &root);
    let result = fs::remove_file(&tags);
    done(tags, result);
    let search = search_dir(derived_dir, &root);
    let result = fs::remove_dir_all(&search);
    done(search, result);
    match failure {
        Some(e) => Err(e),
        None => Ok(removed),
    }
}

/// Whether `name` is the name of a temporary file of Rocksy's atomic write:
/// it starts with `.` and ends with Rocksy's own suffix (`.rocksy-tmp`).
pub fn is_temp_file_name(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(TMP_SUFFIX) && name.len() > 1 + TMP_SUFFIX.len()
}

/// Deletes, directly in `dir` (not below), the temporary files of Rocksy's
/// atomic write ([`is_temp_file_name`]) that were last modified more than
/// [`STALE_TEMP_AGE`] before `now`. Only regular files: a folder or a link
/// with such a name is left alone. Returns the number of files deleted; a
/// folder that cannot be read counts for nothing.
pub fn remove_stale_temp_files(dir: &Path, now: SystemTime) -> usize {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_temp_file_name(name) {
            continue;
        }
        // `DirEntry::metadata` does not follow links.
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let stale = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age > STALE_TEMP_AGE);
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The one exception to "the only write in a vault is `save_file`": deletes
/// the orphaned temporary files of Rocksy's own atomic writes (a process
/// killed between the write and the rename) in the root of the vault and in
/// the folders `tree` lists, hidden folders excluded like everywhere else.
/// Same rules as [`remove_stale_temp_files`]: Rocksy's suffix, regular file,
/// older than an hour. Nothing else is deleted and nothing is created.
///
/// One folder read per folder of the tree: run it in the background, after
/// the vault is shown. `cancelled` is asked before each folder. Returns the
/// number of files deleted.
pub fn clean_vault_temp_files(tree: &Tree, now: SystemTime, cancelled: &dyn Fn() -> bool) -> usize {
    let root = Path::new(&tree.root);
    let mut removed = 0;
    if cancelled() {
        return 0;
    }
    removed += remove_stale_temp_files(root, now);
    for entry in tree.entries.iter().filter(|e| e.is_dir) {
        if cancelled() {
            break;
        }
        removed += remove_stale_temp_files(&root.join(&entry.path), now);
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_files, TempDir};
    use crate::tree::{list_tree, save_tree_snapshot};

    fn old(path: &Path, age: Duration) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn migration_moves_the_three_folders_and_nothing_else() {
        let dir = TempDir::new("migrate");
        let roaming = dir.path().join("roaming");
        let local = dir.path().join("local");
        dir.write("roaming/vaults.json", "{}");
        dir.write("roaming/session.json", "{}");
        dir.write("roaming/trees/a.tree", "tree");
        dir.write("roaming/tags/a.tags", "tags");
        dir.write("roaming/search/a/meta.json", "search");
        dir.write("roaming/webview/x", "not ours to move");

        let m = migrate_derived_data(&roaming, &local);
        assert_eq!(m.dir, local);
        assert_eq!(m.moved, ["trees", "tags", "search"]);
        assert!(m.dropped.is_empty() && m.error.is_none(), "{m:?}");
        assert_eq!(all_files(&roaming), ["session.json", "vaults.json", "webview/x"]);
        assert_eq!(all_files(&local), ["search/a/meta.json", "tags/a.tags", "trees/a.tree"]);
        assert_eq!(fs::read_to_string(local.join("search/a/meta.json")).unwrap(), "search");

        // Second launch: nothing left to do.
        let again = migrate_derived_data(&roaming, &local);
        assert_eq!(again, DerivedData { dir: local.clone(), moved: vec![], dropped: vec![], error: None });
    }

    #[test]
    fn migration_is_a_no_op_for_one_directory_or_a_fresh_profile() {
        let dir = TempDir::new("migrate-same");
        let data = dir.path().join("data");
        dir.write("data/trees/a.tree", "tree");
        let m = migrate_derived_data(&data, &data.join(".").join(""));
        assert_eq!((m.dir.clone(), m.moved.len(), m.error), (data.join(".").join(""), 0, None));
        assert_eq!(all_files(&data), ["trees/a.tree"]);

        // Fresh profile: the local directory is not even created.
        let (roaming, local) = (dir.path().join("r"), dir.path().join("l"));
        let m = migrate_derived_data(&roaming, &local);
        assert_eq!(m.dir, local);
        assert!(!local.exists() && !roaming.exists());
    }

    #[test]
    fn migration_prefers_what_the_local_directory_already_holds() {
        let dir = TempDir::new("migrate-both");
        let (roaming, local) = (dir.path().join("roaming"), dir.path().join("local"));
        dir.write("roaming/trees/stale.tree", "stale");
        dir.write("roaming/tags/moved.tags", "moved");
        dir.write("local/trees/current.tree", "current");
        fs::create_dir_all(local.join("tags")).unwrap(); // empty: replaced

        let m = migrate_derived_data(&roaming, &local);
        assert_eq!(m.dir, local);
        assert_eq!(m.moved, ["tags"]);
        assert_eq!(m.dropped, ["trees"]);
        assert_eq!(all_files(&local), ["tags/moved.tags", "trees/current.tree"]);
        assert!(!roaming.join("trees").exists() && !roaming.join("tags").exists());
    }

    #[test]
    fn failed_migration_keeps_everything_in_the_roaming_directory() {
        let dir = TempDir::new("migrate-fail");
        let roaming = dir.path().join("roaming");
        dir.write("roaming/trees/a.tree", "tree");
        dir.write("roaming/tags/a.tags", "tags");
        dir.write("roaming/search/a/meta.json", "search");

        // The local "directory" cannot be created: a file is in the way.
        let blocked = dir.write("blocked", "a file");
        let m = migrate_derived_data(&roaming, &blocked);
        assert_eq!(m.dir, roaming);
        assert!(m.error.is_some() && m.moved.is_empty());
        assert_eq!(all_files(&roaming), ["search/a/meta.json", "tags/a.tags", "trees/a.tree"]);

        // A rename that fails half-way: what was moved comes back.
        let local = dir.path().join("local");
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("tags"), "a file where the folder should go").unwrap();
        let m = migrate_derived_data(&roaming, &local);
        assert_eq!(m.dir, roaming, "{m:?}");
        assert!(m.error.as_deref().is_some_and(|e| e.contains("tags")), "{m:?}");
        assert!(m.moved.is_empty());
        assert_eq!(all_files(&roaming), ["search/a/meta.json", "tags/a.tags", "trees/a.tree"]);
        assert_eq!(all_files(&local), ["tags"]);
    }

    #[test]
    fn purge_deletes_the_data_of_one_vault_only() {
        let dir = TempDir::new("purge");
        let data = dir.path().join("data");
        let (v1, v2) = (dir.path().join("v1"), dir.path().join("v2"));
        for v in ["v1", "v2"] {
            dir.write(&format!("{v}/a.md"), "#tag [[b]] words\n");
            dir.write(&format!("{v}/b.md"), "text\n");
        }
        let before: Vec<String> = all_files(dir.path()).into_iter().filter(|f| f.starts_with('v')).collect();
        for v in [&v1, &v2] {
            save_tree_snapshot(&data, &list_tree(v).unwrap()).unwrap();
            let index = crate::TagIndex::new();
            index.reconcile(&data, v, Default::default(), &crate::CancelToken::new(), |_| {}).unwrap().unwrap();
            index.save(&data).unwrap();
            index.clear();
        }
        let v1_root = absolute_string(&v1).unwrap();
        let v2_root = absolute_string(&v2).unwrap();
        assert!(tree_snapshot_path(&data, &v1_root).is_file());
        assert!(tags_index_path(&data, &v1_root).is_file());
        assert!(search_dir(&data, &v1_root).is_dir());

        assert_eq!(purge_vault_data(&data, &v1).unwrap(), 3);
        assert!(!tree_snapshot_path(&data, &v1_root).exists());
        assert!(!tags_index_path(&data, &v1_root).exists());
        assert!(!search_dir(&data, &v1_root).exists());
        // The other vault keeps its data, and no vault file was touched.
        assert!(tree_snapshot_path(&data, &v2_root).is_file());
        assert!(tags_index_path(&data, &v2_root).is_file());
        assert!(search_dir(&data, &v2_root).is_dir());
        let after: Vec<String> = all_files(dir.path()).into_iter().filter(|f| f.starts_with('v')).collect();
        assert_eq!(after, before);
        // Nothing left: purging again deletes nothing and is not an error.
        assert_eq!(purge_vault_data(&data, &v1).unwrap(), 0);
    }

    #[test]
    fn temp_file_names_are_recognised_strictly() {
        for yes in [".a.md.1234-0.rocksy-tmp", ".x.rocksy-tmp", ".Journal 2026.md.99-12.rocksy-tmp"] {
            assert!(is_temp_file_name(yes), "{yes}");
        }
        for no in [
            ".rocksy-tmp",
            "a.md.1234-0.rocksy-tmp",
            ".a.md.1234-0.rocksy-tmp.bak",
            ".a.md.1234-0.ROCKSY-TMP",
            ".a.md.tmp",
            ".obsidian",
            "note.md",
            "~$doc.docx",
        ] {
            assert!(!is_temp_file_name(no), "{no}");
        }
    }

    #[test]
    fn only_old_temp_files_of_rocksy_are_removed_from_a_vault() {
        let dir = TempDir::new("stale-tmp");
        let hour = Duration::from_secs(3600);
        let stale = [
            dir.write("v/.a.md.111-0.rocksy-tmp", "orphan"),
            dir.write("v/sub/deep/.b.md.222-7.rocksy-tmp", "orphan"),
        ];
        let kept = [
            // Recent: maybe a save in progress (ours or another instance's).
            dir.write("v/.recent.md.333-1.rocksy-tmp", "in progress"),
            dir.write("v/sub/.almost.md.444-1.rocksy-tmp", "59 minutes"),
            // Old, but not Rocksy's temporary files.
            dir.write("v/note.md", "#note\n"),
            dir.write("v/sub/visible.md.1-0.rocksy-tmp", "no leading dot"),
            dir.write("v/sub/.other.tmp", "another tool"),
            dir.write("v/sub/.a.md.1-0.rocksy-tmp.bak", "suffix not last"),
            // Hidden folders are never visited.
            dir.write("v/.obsidian/.app.json.5-0.rocksy-tmp", "not ours to visit"),
            dir.write("v/.trash/.c.md.6-0.rocksy-tmp", "not ours to visit"),
        ];
        // A folder named like a temporary file is not a file.
        fs::create_dir_all(dir.path().join("v/.folder.9-9.rocksy-tmp")).unwrap();
        for path in stale.iter().chain(&kept) {
            old(path, hour * 3);
        }
        old(&kept[0], Duration::from_secs(5));
        old(&kept[1], Duration::from_secs(59 * 60));

        let vault = dir.path().join("v");
        let tree = list_tree(&vault).unwrap();
        let before = all_files(&vault);
        assert_eq!(clean_vault_temp_files(&tree, SystemTime::now(), &|| false), 2);
        let after = all_files(&vault);
        let gone: Vec<&String> = before.iter().filter(|f| !after.contains(f)).collect();
        assert_eq!(gone, [".a.md.111-0.rocksy-tmp", "sub/deep/.b.md.222-7.rocksy-tmp"]);
        for path in &kept {
            assert!(path.exists(), "{}", path.display());
        }
        assert!(dir.path().join("v/.folder.9-9.rocksy-tmp").is_dir());
        assert_eq!(fs::read_to_string(&kept[2]).unwrap(), "#note\n");
        // Nothing more the second time, and a cancelled pass does nothing.
        assert_eq!(clean_vault_temp_files(&tree, SystemTime::now(), &|| false), 0);
        old(&kept[0], hour * 2);
        assert_eq!(clean_vault_temp_files(&tree, SystemTime::now(), &|| true), 0);
        assert!(kept[0].exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_named_like_a_temp_file_is_left_alone() {
        let dir = TempDir::new("stale-link");
        let target = dir.write("outside/precious.md", "keep me");
        dir.write("v/a.md", "x");
        let link = dir.path().join("v/.precious.md.1-0.rocksy-tmp");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        old(&target, Duration::from_secs(3 * 3600));
        assert_eq!(remove_stale_temp_files(&dir.path().join("v"), SystemTime::now()), 0);
        assert!(target.exists() && link.symlink_metadata().is_ok());
    }
}
