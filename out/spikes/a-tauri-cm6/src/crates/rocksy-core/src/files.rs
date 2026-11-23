//! Reading and atomically saving text files.
//!
//! Text is strict UTF-8. Nothing is normalised: the byte-order mark and the
//! line endings of the file are part of the returned text, and `save_file`
//! writes exactly the bytes of the text it is given.

use crate::atomic::write_atomic;
use crate::error::{ApiError, Result};
use crate::paths::{mtime_ms, path_str};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub text: String,
    pub mtime_ms: u64,
    pub size: u64,
}

/// Return value of `save_file`: `{ mtimeMs, size }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub mtime_ms: u64,
    pub size: u64,
}

/// Reads a UTF-8 text file.
///
/// Errors: `not_found`, `invalid` (a directory), `not_utf8` (no lossy
/// conversion is ever attempted), `io`.
pub fn read_file(path: &Path) -> Result<FileContent> {
    let path_string = path_str(path)?.to_string();
    let meta = fs::metadata(path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot open {path_string}")))?;
    if meta.is_dir() {
        return Err(ApiError::invalid(format!("{path_string} is a directory")));
    }
    let mut file = fs::File::open(path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot open {path_string}")))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize + 1);
    file.read_to_end(&mut bytes)
        .map_err(|e| ApiError::from_io(&e, format!("cannot read {path_string}")))?;
    // Metadata of the handle that was read, so mtime and content match.
    let meta = file.metadata().unwrap_or(meta);
    let size = bytes.len() as u64;
    let text = String::from_utf8(bytes).map_err(|e| {
        ApiError::not_utf8(format!(
            "{path_string} is not valid UTF-8 (first invalid byte at offset {})",
            e.utf8_error().valid_up_to()
        ))
    })?;
    Ok(FileContent { path: path_string, text, mtime_ms: mtime_ms(&meta), size })
}

/// Windows ignores trailing dots and spaces in a path component, so
/// `.obsidian.` or `x.canvas ` would reach the protected names.
fn strip_trailing_dots_spaces(name: &str) -> &str {
    name.trim_end_matches(['.', ' '])
}

/// Why writing to `path` is forbidden, if it is.
fn forbidden_reason(path: &Path) -> Option<&'static str> {
    for comp in path.components() {
        if let Component::Normal(name) = comp {
            let name = name.to_string_lossy();
            if strip_trailing_dots_spaces(&name).eq_ignore_ascii_case(".obsidian") {
                return Some("writing inside .obsidian is not allowed");
            }
        }
    }
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let file_name = strip_trailing_dots_spaces(&file_name);
    // An NTFS alternate data stream (`a.canvas:stream`) still targets the file.
    let file_name = file_name.split(':').next().unwrap_or(file_name);
    let file_name = strip_trailing_dots_spaces(file_name);
    if let Some(dot) = file_name.rfind('.') {
        if file_name[dot + 1..].eq_ignore_ascii_case("canvas") {
            return Some("writing a .canvas file is not allowed");
        }
    }
    None
}

/// Resolves the directory part (so a link into `.obsidian` cannot be used to
/// bypass the check) and a final symlink (so the link itself is not replaced
/// by a regular file). Falls back to the given path when the file system does
/// not support resolution (some virtual drives).
fn resolve_target(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid(format!("no parent directory in {}", path.display())))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| ApiError::invalid(format!("no file name in {}", path.display())))?;
    let real_parent = match fs::canonicalize(parent) {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(ApiError::not_found(format!(
                "directory does not exist: {}",
                parent.display()
            )));
        }
        Err(_) => parent.to_path_buf(),
    };
    let mut target = real_parent.join(file_name);
    if fs::symlink_metadata(&target).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        if let Ok(real) = fs::canonicalize(&target) {
            target = real;
        }
    }
    Ok(target)
}

/// Saves `text` to `path` atomically and returns the new `{ mtimeMs, size }`.
///
/// * The bytes written are exactly `text.as_bytes()`.
/// * A hidden temporary file (`.<name>.<pid>-<n>.rocksy-tmp`) is written and
///   synced in the same directory, then renamed over the destination; it is
///   removed on every failure path.
/// * With `expected_mtime_ms`, the save fails with `conflict` if the file on
///   disk has a different modification time, or no longer exists. Pass `None`
///   to create a file or to overwrite unconditionally.
/// * Paths with a `.obsidian` component, and `.canvas` files, are refused
///   with `invalid`. Missing parent directories are not created
///   (`not_found`).
pub fn save_file(path: &Path, text: &str, expected_mtime_ms: Option<u64>) -> Result<SaveResult> {
    let path_string = path_str(path)?.to_string();
    if !path.is_absolute() {
        return Err(ApiError::invalid(format!("path must be absolute: {path_string}")));
    }
    if path.file_name().is_none() {
        return Err(ApiError::invalid(format!("no file name in {path_string}")));
    }
    if let Some(reason) = forbidden_reason(path) {
        return Err(ApiError::invalid(format!("{reason}: {path_string}")));
    }
    let target = resolve_target(path)?;
    if let Some(reason) = forbidden_reason(&target) {
        return Err(ApiError::invalid(format!("{reason}: {path_string}")));
    }

    let check = || -> Result<()> {
        match fs::metadata(&target) {
            Ok(meta) => {
                if meta.is_dir() {
                    return Err(ApiError::invalid(format!("{path_string} is a directory")));
                }
                if let Some(expected) = expected_mtime_ms {
                    let actual = mtime_ms(&meta);
                    if actual != expected {
                        return Err(ApiError::conflict(format!(
                            "{path_string} changed on disk (expected mtime {expected}, found {actual})"
                        )));
                    }
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if expected_mtime_ms.is_some() {
                    Err(ApiError::conflict(format!("{path_string} no longer exists on disk")))
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(ApiError::from_io(&e, format!("cannot stat {path_string}"))),
        }
    };

    // Fail fast before writing anything, then check again right before the rename.
    check()?;
    write_atomic(&target, text.as_bytes(), check)?;

    let meta = fs::metadata(&target)
        .map_err(|e| ApiError::from_io(&e, format!("cannot stat {path_string} after saving")))?;
    Ok(SaveResult { mtime_ms: mtime_ms(&meta), size: meta.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};

    #[test]
    fn read_returns_exact_text_with_bom_and_crlf() {
        let dir = TempDir::new("read");
        let raw = "\u{feff}line 1\r\nline 2\r\n\r\nétoile ★\r\n";
        let p = dir.write("a.md", raw);
        let fc = read_file(&p).unwrap();
        assert_eq!(fc.text, raw);
        assert_eq!(fc.size, raw.len() as u64);
        assert!(fc.mtime_ms > 0);
        assert_eq!(fc.path, p.to_str().unwrap());
    }

    #[test]
    fn read_rejects_non_utf8_and_missing_and_directory() {
        let dir = TempDir::new("read-err");
        let p = dir.write("latin1.md", [b'c', b'a', b'f', 0xE9, b'\n']);
        let err = read_file(&p).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotUtf8);
        assert!(err.message.contains("offset 3"), "{}", err.message);

        let err = read_file(&dir.path().join("missing.md")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);

        let err = read_file(dir.path()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
    }

    #[test]
    fn save_writes_exact_bytes_and_leaves_no_temporary() {
        let dir = TempDir::new("save");
        let p = dir.write("note.md", "old");
        let text = "\u{feff}# Titre\r\n\r\nligne CRLF\r\nmixte LF\nfin sans saut";
        let res = save_file(&p, text, None).unwrap();
        assert_eq!(fs::read(&p).unwrap(), text.as_bytes());
        assert_eq!(res.size, text.len() as u64);
        assert_eq!(res.mtime_ms, mtime_ms(&fs::metadata(&p).unwrap()));
        assert_eq!(all_files(dir.path()), vec!["note.md"]);
        // Round trip through read_file is lossless.
        assert_eq!(read_file(&p).unwrap().text, text);
    }

    #[test]
    fn save_creates_new_file_but_not_directories() {
        let dir = TempDir::new("save-new");
        let p = dir.path().join("new.md");
        save_file(&p, "hello\n", None).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "hello\n");

        let err = save_file(&dir.path().join("nodir/x.md"), "x", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(!dir.path().join("nodir").exists());
        assert_eq!(all_files(dir.path()), vec!["new.md"]);
    }

    #[test]
    fn save_detects_conflict_and_keeps_disk_content() {
        let dir = TempDir::new("conflict");
        let p = dir.write("note.md", "v1");
        let m1 = read_file(&p).unwrap().mtime_ms;

        // Matching mtime: accepted.
        let res = save_file(&p, "v2", Some(m1)).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "v2");

        // Stale mtime: refused, content untouched, no temporary left behind.
        let stale = res.mtime_ms + 5_000;
        let err = save_file(&p, "v3", Some(stale)).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert_eq!(fs::read_to_string(&p).unwrap(), "v2");
        assert_eq!(all_files(dir.path()), vec!["note.md"]);

        // The mtime returned by a save is the one to pass to the next save.
        save_file(&p, "v4", Some(res.mtime_ms)).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "v4");

        // File deleted since it was read: conflict, nothing recreated.
        let m = read_file(&p).unwrap().mtime_ms;
        fs::remove_file(&p).unwrap();
        let err = save_file(&p, "v5", Some(m)).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert!(all_files(dir.path()).is_empty());
    }

    #[test]
    fn save_refuses_obsidian_and_canvas() {
        let dir = TempDir::new("forbidden");
        let conf = dir.write(".obsidian/app.json", "{}");
        let nested = dir.write("sub/.obsidian/plugins/x/data.json", "{}");
        let canvas = dir.write("board.canvas", "{\"nodes\":[]}");
        let before = all_files(dir.path());

        for p in [
            conf.clone(),
            nested.clone(),
            canvas.clone(),
            dir.path().join(".obsidian/new.md"),
            dir.path().join(".OBSIDIAN/new.md"),
            dir.path().join("New.CANVAS"),
            dir.path().join("trailing.canvas."),
            dir.path().join(".obsidian./x.md"),
            dir.path().join("stream.canvas:alt"),
        ] {
            let err = save_file(&p, "x", None).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Invalid, "{}", p.display());
        }
        assert_eq!(all_files(dir.path()), before);
        assert_eq!(fs::read_to_string(&conf).unwrap(), "{}");
        assert_eq!(fs::read_to_string(&canvas).unwrap(), "{\"nodes\":[]}");

        // Names that merely look similar are fine.
        save_file(&dir.path().join("canvas.md"), "ok", None).unwrap();
        save_file(&dir.path().join("my.canvas.md"), "ok", None).unwrap();
    }

    #[test]
    fn save_rejects_relative_path_and_directory_target() {
        let dir = TempDir::new("invalid");
        let err = save_file(Path::new("relative.md"), "x", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        fs::create_dir(dir.path().join("d.md")).unwrap();
        let err = save_file(&dir.path().join("d.md"), "x", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert!(all_files(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn save_follows_symlinks_and_blocks_links_into_obsidian() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = TempDir::new("symlink");
        let real = dir.write("real.md", "v1");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.md");
        symlink(&real, &link).unwrap();

        save_file(&link, "v2", None).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "v2");
        // Permissions of the replaced file are kept.
        assert_eq!(fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);

        // A directory link that leads into .obsidian does not bypass the rule.
        dir.write(".obsidian/app.json", "{}");
        symlink(dir.path().join(".obsidian"), dir.path().join("conf")).unwrap();
        let err = save_file(&dir.path().join("conf/app.json"), "x", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert_eq!(fs::read_to_string(dir.path().join(".obsidian/app.json")).unwrap(), "{}");
    }

    #[cfg(unix)]
    #[test]
    fn failed_save_removes_its_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("cleanup");
        // The destination is a non-empty directory: the rename itself fails
        // after the temporary file has been written. The directory check is
        // bypassed by calling the low-level writer directly.
        let dest = dir.path().join("target");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("child"), "x").unwrap();
        let err = write_atomic(&dest, b"data", || Ok(())).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Io);
        assert_eq!(all_files(dir.path()), vec!["target/child"]);

        // An error from the pre-rename hook also cleans up.
        let p = dir.path().join("a.md");
        let err = write_atomic(&p, b"data", || Err(ApiError::conflict("changed"))).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert_eq!(all_files(dir.path()), vec!["target/child"]);

        // Read-only directory: the temporary cannot even be created.
        let ro = dir.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let res = save_file(&ro.join("x.md"), "x", None);
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        // Root ignores directory permissions; only assert when it failed.
        if let Err(e) = res {
            assert_eq!(e.kind, ErrorKind::Io);
            assert!(fs::read_dir(&ro).unwrap().next().is_none());
        }
    }
}
