//! Reading and atomically saving text files.
//!
//! Text is strict UTF-8. Nothing is normalised: the byte-order mark and the
//! line endings of the file are part of the returned text, and `save_file`
//! writes exactly the bytes of the text it is given.

use crate::atomic::{write_atomic, RetryPolicy};
use crate::error::{ApiError, ErrorCode, Result};
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
    /// The save created the file (it did not exist right before the write).
    /// For the caller only: not part of the JSON of the contract.
    #[serde(skip)]
    pub created: bool,
}

/// Largest file `read_file` accepts, in bytes (50 MiB). The whole text goes
/// through memory, the IPC bridge and the editor: beyond this the process or
/// the WebView would run out of memory.
pub const MAX_READ_BYTES: u64 = 50 * 1024 * 1024;

/// Reads a UTF-8 text file.
///
/// Errors: `not_found`, `invalid` (a directory, or a file larger than
/// [`MAX_READ_BYTES`]), `not_utf8` (no lossy conversion is ever attempted),
/// `io`.
pub fn read_file(path: &Path) -> Result<FileContent> {
    read_file_limited(path, MAX_READ_BYTES)
}

fn too_large(path_string: &str, size: u64, limit: u64) -> ApiError {
    const MIB: f64 = 1024.0 * 1024.0;
    ApiError::invalid(format!(
        "{path_string} is too large to open ({:.1} MiB, limit {:.1} MiB)",
        size as f64 / MIB,
        limit as f64 / MIB
    ))
    .with_code(ErrorCode::TooLarge)
}

fn read_file_limited(path: &Path, limit: u64) -> Result<FileContent> {
    let path_string = path_str(path)?.to_string();
    let meta = fs::metadata(path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot open {path_string}")))?;
    if meta.is_dir() {
        return Err(ApiError::invalid(format!("{path_string} is a directory")));
    }
    // Checked on the size already known, before a single byte is read.
    if meta.len() > limit {
        return Err(too_large(&path_string, meta.len(), limit));
    }
    let mut file = fs::File::open(path)
        .map_err(|e| ApiError::from_io(&e, format!("cannot open {path_string}")))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize + 1);
    // The file may grow between the stat and the read: never read past the limit.
    (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| ApiError::from_io(&e, format!("cannot read {path_string}")))?;
    if bytes.len() as u64 > limit {
        let size = file.metadata().map(|m| m.len()).unwrap_or(bytes.len() as u64);
        return Err(too_large(&path_string, size, limit));
    }
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

/// Extensions `save_file` accepts (compared without case).
const WRITABLE_EXTENSIONS: [&str; 2] = ["md", "markdown"];

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
    let full_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let file_name = strip_trailing_dots_spaces(&full_name);
    // An NTFS alternate data stream (`a.canvas:stream`) still targets the file.
    let file_name = file_name.split(':').next().unwrap_or(file_name);
    let file_name = strip_trailing_dots_spaces(file_name);
    let extension = file_name.rfind('.').map(|dot| &file_name[dot + 1..]);
    if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("canvas")) {
        return Some("writing a .canvas file is not allowed");
    }
    // A leading dot alone (`.bashrc`) is a name, not an extension.
    let extension = extension.filter(|_| !file_name.starts_with('.') || file_name[1..].contains('.'));
    if !extension.is_some_and(|ext| WRITABLE_EXTENSIONS.iter().any(|w| ext.eq_ignore_ascii_case(w))) {
        return Some("only .md and .markdown files can be saved, other files are read-only");
    }
    // On Windows a colon in the last component names an alternate data
    // stream (`x.md:payload.cmd`), never a plain Markdown file.
    if cfg!(windows) && full_name.contains(':') {
        return Some("writing an alternate data stream is not allowed");
    }
    None
}

/// Whether the name and location of `path` allow `save_file` at all:
/// `.md` or `.markdown`, not a `.canvas`, not under `.obsidian`. Computed on
/// the string, without touching the file system (the checks that need the
/// disk, links and short names, are left to [`save_file`]). The shell uses
/// it, together with its access policy, for `FileContent.readOnly`.
pub fn is_savable_name(path: &Path) -> bool {
    forbidden_reason(path).is_none()
}

/// Checks a link the page asks to open in the system browser or mail
/// client: `http`, `https` and `mailto` only, and no `mailto:` that carries
/// an `attach`/`attachment` parameter (some mail clients attach the named
/// local file). Returns the trimmed URL to open.
///
/// Errors: `invalid`.
pub fn check_external_url(url: &str) -> Result<&str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(url);
    }
    if !lower.starts_with("mailto:") {
        return Err(ApiError::invalid(format!("unsupported link scheme: {url}")));
    }
    let query = lower.split_once('?').map_or("", |(_, q)| q);
    let attaches = query.split(['&', ';']).any(|pair| {
        let key = pair.split('=').next().unwrap_or("").trim();
        // `%61ttach` and the like: decode the few bytes that matter.
        let key = percent_decode_ascii(key);
        key == "attach" || key == "attachment"
    });
    if attaches {
        return Err(ApiError::invalid("mail links with an attachment are not opened"));
    }
    Ok(url)
}

/// Decodes `%XX` sequences that stand for ASCII characters (lowercased).
fn percent_decode_ascii(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                let value = hi * 16 + lo;
                if value.is_ascii() {
                    out.push(value.to_ascii_lowercase());
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a path component looks like a Windows 8.3 short name
/// (`BOARD~1.CAN`, `OBSIDI~1`): a `~` followed by a digit.
fn looks_like_short_name(name: &str) -> bool {
    name.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit())
}

/// Whether any component of `path` looks like an 8.3 short name.
fn has_short_name_component(path: &Path) -> bool {
    path.components().any(|comp| match comp {
        Component::Normal(name) => looks_like_short_name(&name.to_string_lossy()),
        _ => false,
    })
}

/// Checks that cannot be done on the name as given: under Windows an 8.3
/// short name (`BOARD~1.CAN`, `OBSIDI~1`) designates a file or folder whose
/// long name is protected.
///
/// * When the target exists, its fully canonicalized path (long names) goes
///   through the same rules. A canonical path is a verbatim path (`\\?\C:\…`)
///   on Windows; only its normal components are looked at, so the prefix is
///   never a reason to refuse.
/// * When the directory could not be canonicalized (`parent_resolved` is
///   false: some virtual drives), long names are unknown, so on Windows a
///   component that looks like a short name is refused.
fn forbidden_reason_on_disk(target: &Path, parent_resolved: bool) -> Option<&'static str> {
    match fs::canonicalize(target) {
        Ok(real) => forbidden_reason(&real),
        // Not there yet (a file being created under a resolved directory), or
        // a file system without resolution.
        Err(_) => {
            let unresolved = !parent_resolved || fs::symlink_metadata(target).is_ok();
            if cfg!(windows) && unresolved && has_short_name_component(target) {
                Some("writing through a short (8.3) file name is not allowed")
            } else {
                None
            }
        }
    }
}

/// Resolves the directory part (so a link into `.obsidian` cannot be used to
/// bypass the check) and a final symlink (so the link itself is not replaced
/// by a regular file). Falls back to the given path when the file system does
/// not support resolution (some virtual drives); the boolean tells whether
/// the directory was resolved.
fn resolve_target(path: &Path) -> Result<(PathBuf, bool)> {
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::invalid(format!("no parent directory in {}", path.display())))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| ApiError::invalid(format!("no file name in {}", path.display())))?;
    let (real_parent, parent_resolved) = match fs::canonicalize(parent) {
        Ok(p) => (p, true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(ApiError::not_found(format!(
                "directory does not exist: {}",
                parent.display()
            )));
        }
        Err(_) => (parent.to_path_buf(), false),
    };
    let mut target = real_parent.join(file_name);
    if fs::symlink_metadata(&target).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        if let Ok(real) = fs::canonicalize(&target) {
            target = real;
        }
    }
    Ok((target, parent_resolved))
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
/// * Only `.md` and `.markdown` files (any case) can be saved: every other
///   extension, and a name without extension, is refused with `invalid`
///   (such files are read-only). Creating a new Markdown file is allowed.
/// * Paths with a `.obsidian` component, and `.canvas` files, are refused
///   with `invalid`, also when they are reached through a link or a Windows
///   short name. Missing parent directories are not created (`not_found`).
/// * A rename refused because another process holds the file (Windows) is
///   retried for about two seconds; the conflict check runs again before
///   every attempt.
pub fn save_file(path: &Path, text: &str, expected_mtime_ms: Option<u64>) -> Result<SaveResult> {
    let path_string = path_str(path)?.to_string();
    if !path.is_absolute() {
        return Err(ApiError::invalid(format!("path must be absolute: {path_string}")));
    }
    if path.file_name().is_none() {
        return Err(ApiError::invalid(format!("no file name in {path_string}")));
    }
    if let Some(reason) = forbidden_reason(path) {
        return Err(ApiError::invalid(format!("{reason}: {path_string}")).with_code(ErrorCode::ReadOnly));
    }
    let (target, parent_resolved) = resolve_target(path)?;
    if let Some(reason) = forbidden_reason(&target) {
        return Err(ApiError::invalid(format!("{reason}: {path_string}")).with_code(ErrorCode::ReadOnly));
    }
    if let Some(reason) = forbidden_reason_on_disk(&target, parent_resolved) {
        return Err(ApiError::invalid(format!("{reason}: {path_string}")).with_code(ErrorCode::ReadOnly));
    }

    // Whether the file existed at the last check, the one right before the
    // rename that put the new content in place.
    let existed = std::cell::Cell::new(false);
    let check = || -> Result<()> {
        existed.set(false);
        match fs::metadata(&target) {
            Ok(meta) => {
                existed.set(true);
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
                    Err(ApiError::conflict(format!("{path_string} no longer exists on disk")).with_code(ErrorCode::Gone))
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(ApiError::from_io(&e, format!("cannot stat {path_string}"))),
        }
    };

    // Fail fast before writing anything, then check again right before the
    // rename and before each new attempt of it.
    check()?;
    write_atomic(&target, text.as_bytes(), RetryPolicy::USER_FILE, check)?;

    let meta = fs::metadata(&target)
        .map_err(|e| ApiError::from_io(&e, format!("cannot stat {path_string} after saving")))?;
    Ok(SaveResult { mtime_ms: mtime_ms(&meta), size: meta.len(), created: !existed.get() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::testutil::{all_files, TempDir};

    #[test]
    fn savable_names_are_markdown_outside_obsidian() {
        for yes in ["/v/a.md", "/v/sub/B.MARKDOWN", "/v/.hidden.md", "/v/a.b.md"] {
            assert!(is_savable_name(Path::new(yes)), "{yes}");
        }
        for no in [
            "/v/a.txt",
            "/v/board.canvas",
            "/v/README",
            "/v/.bashrc",
            "/v/a.md.bak",
            "/v/.obsidian/notes.md",
            "/v/.OBSIDIAN/x/notes.md",
            "/v/image.png",
        ] {
            assert!(!is_savable_name(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn external_links_are_web_and_plain_mail_only() {
        for ok in [
            "https://example.org/a?attach=1",
            " http://example.org ",
            "HTTPS://EXAMPLE.ORG",
            "mailto:a@example.org",
            "mailto:a@example.org?subject=attach%20this&body=see%20attachment",
            "mailto:a@example.org?subject=x&cc=b@example.org",
        ] {
            assert_eq!(check_external_url(ok).unwrap(), ok.trim(), "{ok}");
        }
        for ko in [
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\System32\\calc.exe",
            "javascript:alert(1)",
            "ms-msdt:/id",
            "",
            "mailto:a@example.org?attach=C:\\Users\\a\\.ssh\\id_rsa",
            "mailto:a@example.org?subject=x&ATTACH=c:/secret.txt",
            "MAILTO:a@example.org?subject=x&attachment=c:/secret.txt",
            "mailto:a@example.org?%61ttach=c:/secret.txt",
            "mailto:a@example.org?subject=x; attach=c:/secret.txt",
            "mailto:a@example.org?att%61ch%6dent=x",
            "mailto:?attach=\u{e9}%",
        ] {
            assert_eq!(check_external_url(ko).unwrap_err().kind, ErrorKind::Invalid, "{ko}");
        }
    }

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
        let err = write_atomic(&dest, b"data", RetryPolicy::DATA_FILE, || Ok(())).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Io);
        assert_eq!(all_files(dir.path()), vec!["target/child"]);

        // An error from the pre-rename hook also cleans up.
        let p = dir.path().join("a.md");
        let err = write_atomic(&p, b"data", RetryPolicy::DATA_FILE, || Err(ApiError::conflict("changed"))).unwrap_err();
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

    #[test]
    fn read_refuses_a_file_over_the_limit_without_reading_it() {
        let dir = TempDir::new("read-limit");
        let p = dir.write("big.txt", "0123456789");
        assert_eq!(read_file_limited(&p, 10).unwrap().text, "0123456789");
        let err = read_file_limited(&p, 9).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert!(err.message.contains("too large"), "{}", err.message);
    }

    #[test]
    fn extension_rule_and_short_names() {
        for ok in ["/v/a.md", "/v/A.MD", "/v/a.Markdown", "/v/my.canvas.md", "/v/a.md.", "/v/.hidden.md"] {
            assert_eq!(forbidden_reason(Path::new(ok)), None, "{ok}");
        }
        for ko in [
            "/v/a.txt", "/v/a", "/v/a.md.bak", "/v/a.cmd", "/v/.bashrc", "/v/.md", "/v/a.mdx",
            "/v/x.cmd:a.md", "/v/BOARD~1.CAN", "/v/a.canvas",
        ] {
            assert!(forbidden_reason(Path::new(ko)).is_some(), "{ko}");
        }
        assert!(looks_like_short_name("BOARD~1.CAN"));
        assert!(looks_like_short_name("OBSIDI~1"));
        assert!(!looks_like_short_name("note~.md"));
        assert!(!looks_like_short_name("a ~ b.md"));
        assert!(has_short_name_component(Path::new("/v/OBSIDI~1/x.md")));
        assert!(!has_short_name_component(Path::new("/v/notes/x.md")));
    }

    #[cfg(unix)]
    #[test]
    fn save_checks_the_resolved_name_of_a_link() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("link-ext");
        let script = dir.write("run.sh", "echo v1\n");
        let link = dir.path().join("innocent.md");
        symlink(&script, &link).unwrap();
        let err = save_file(&link, "rm -rf\n", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid);
        assert_eq!(fs::read_to_string(&script).unwrap(), "echo v1\n");

        // A dangling link named .md that points at a non-Markdown name.
        let dangling = dir.path().join("new.md");
        symlink(dir.path().join("created.cmd"), &dangling).unwrap();
        let _ = save_file(&dangling, "x", None);
        assert!(!dir.path().join("created.cmd").exists());
    }
}
