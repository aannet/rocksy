//! Path helpers shared by the modules.
//!
//! Paths cross the API boundary as strings (JSON), so everything here works on
//! UTF-8 strings. The Windows rules (case-insensitive comparison, `\`
//! separators, `\\?\` verbatim prefixes) are implemented as pure functions
//! taking a `windows` flag so they can be tested on any platform.

use crate::error::{ApiError, Result};
use std::fs::Metadata;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// 64-bit FNV-1a. Used for snapshot file names and snapshot checksums
/// (integrity against truncation, not security).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Modification time in milliseconds since the Unix epoch (0 if unavailable
/// or before 1970).
pub fn mtime_ms(meta: &Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Removes a Windows verbatim prefix: `\\?\C:\x` -> `C:\x`,
/// `\\?\UNC\server\share` -> `\\server\share`. Other strings are returned
/// unchanged.
pub fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return rest.to_string();
        }
    }
    s.to_string()
}

fn is_sep(c: char, windows: bool) -> bool {
    c == '/' || (windows && c == '\\')
}

/// Trims trailing separators but keeps a bare root (`/`, `C:\`).
pub(crate) fn trim_trailing_sep(s: &str, windows: bool) -> &str {
    let trimmed = s.trim_end_matches(|c| is_sep(c, windows));
    if trimmed.is_empty() || (windows && trimmed.ends_with(':')) {
        // Keep exactly one separator after the root.
        let keep = trimmed.len() + s[trimmed.len()..].chars().next().map_or(0, char::len_utf8);
        &s[..keep]
    } else {
        trimmed
    }
}

/// Absolute path as a display string: made absolute without touching the file
/// system (no symlink resolution, so no `\\?\` prefix appears on Windows),
/// verbatim prefix removed, trailing separator trimmed.
pub fn absolute_string(path: &Path) -> Result<String> {
    let abs = std::path::absolute(path)
        .map_err(|e| ApiError::invalid(format!("invalid path {}: {e}", path.display())))?;
    let s = abs
        .to_str()
        .ok_or_else(|| ApiError::invalid(format!("path is not valid Unicode: {}", abs.display())))?;
    let s = strip_verbatim(s);
    Ok(trim_trailing_sep(&s, cfg!(windows)).to_string())
}

/// Comparison key of a path string for the current platform.
pub(crate) fn path_key(s: &str) -> String {
    path_key_for(s, cfg!(windows))
}

/// Comparison key: on Windows, verbatim prefix removed, `/` -> `\`, lowercase;
/// trailing separators trimmed everywhere.
pub(crate) fn path_key_for(s: &str, windows: bool) -> String {
    if windows {
        let s = strip_verbatim(s).replace('/', "\\");
        trim_trailing_sep(&s, true).to_lowercase()
    } else {
        trim_trailing_sep(s, false).to_string()
    }
}

/// True if `child_key` is `parent_key` itself or lies below it. Both must be
/// keys produced by [`path_key_for`] with the same `windows` flag.
pub(crate) fn key_is_inside(child_key: &str, parent_key: &str, windows: bool) -> bool {
    key_rel(child_key, parent_key, windows).is_some()
}

/// Remainder of `child_key` below `parent_key` (empty when equal).
pub(crate) fn key_rel<'a>(child_key: &'a str, parent_key: &str, windows: bool) -> Option<&'a str> {
    let rest = child_key.strip_prefix(parent_key)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let sep = if windows { '\\' } else { '/' };
    if parent_key.ends_with(sep) {
        // Parent is a bare root ("/" or "c:\").
        return Some(rest);
    }
    rest.strip_prefix(sep)
}

/// Joins an absolute root and a `/`-separated relative path using the native
/// separator of the current platform.
pub fn join_rel(root: &str, rel: &str) -> String {
    join_rel_for(root, rel, cfg!(windows))
}

pub(crate) fn join_rel_for(root: &str, rel: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    let mut out = String::with_capacity(root.len() + 1 + rel.len());
    out.push_str(root);
    if !out.ends_with(|c| is_sep(c, windows)) {
        out.push(sep);
    }
    if windows {
        out.extend(rel.chars().map(|c| if c == '/' { '\\' } else { c }));
    } else {
        out.push_str(rel);
    }
    out
}

/// Last component of a path string (either separator accepted).
pub(crate) fn last_component(s: &str) -> &str {
    let t = s.trim_end_matches(['/', '\\']);
    match t.rfind(['/', '\\']) {
        Some(i) => &t[i + 1..],
        None => t,
    }
}

/// `path` as a `&str`, or an `invalid` error.
pub(crate) fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| ApiError::invalid(format!("path is not valid Unicode: {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_removed() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\a"), r"C:\Users\a");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(strip_verbatim(r"C:\Users\a"), r"C:\Users\a");
        assert_eq!(strip_verbatim("/home/a"), "/home/a");
        // Verbatim paths that are not drive paths are left alone.
        assert_eq!(strip_verbatim(r"\\?\Volume{1}\x"), r"\\?\Volume{1}\x");
    }

    #[test]
    fn windows_keys_ignore_case_and_separator() {
        let a = path_key_for(r"G:\Mon Drive\PKM\", true);
        let b = path_key_for("g:/mon drive/pkm", true);
        assert_eq!(a, b);
        assert_eq!(path_key_for(r"\\?\G:\Mon Drive", true), r"g:\mon drive");
        assert_eq!(path_key_for(r"C:\", true), r"c:\");
        assert_eq!(path_key_for("/", false), "/");
        assert_eq!(path_key_for("/a/b/", false), "/a/b");
        // Unix keeps case.
        assert_ne!(path_key_for("/A", false), path_key_for("/a", false));
    }

    #[test]
    fn inside_checks_whole_components() {
        assert!(key_is_inside("/v/a.md", "/v", false));
        assert!(key_is_inside("/v", "/v", false));
        assert!(!key_is_inside("/vault2/a.md", "/v", false));
        assert!(!key_is_inside("/v2", "/v", false));
        assert!(key_is_inside("/a", "/", false));
        assert!(key_is_inside(r"c:\v\a.md", r"c:\v", true));
        assert!(!key_is_inside(r"c:\vv\a.md", r"c:\v", true));
        assert!(key_is_inside(r"c:\v\a.md", r"c:\", true));
        assert_eq!(key_rel("/v/a/b.md", "/v", false), Some("a/b.md"));
    }

    #[test]
    fn join_uses_native_separator() {
        assert_eq!(join_rel_for(r"G:\Mon Drive\V", "a/b.md", true), r"G:\Mon Drive\V\a\b.md");
        assert_eq!(join_rel_for(r"C:\", "a.md", true), r"C:\a.md");
        assert_eq!(join_rel_for("/v", "a/b.md", false), "/v/a/b.md");
        assert_eq!(join_rel_for("/", "a.md", false), "/a.md");
    }

    #[test]
    fn last_component_handles_both_separators() {
        assert_eq!(last_component(r"G:\Mon Drive\PKM"), "PKM");
        assert_eq!(last_component("/home/a/vault/"), "vault");
        assert_eq!(last_component("vault"), "vault");
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
