//! Which paths the shell may read, write and watch on behalf of the page.
//!
//! The web page is not trusted: a script injected through a note could ask
//! for any path. [`AccessPolicy`] answers "is this path one the user chose?"
//! from two sources only: the registered vaults, and the files the user
//! opened explicitly (command line, second launch, file dialog, drag and
//! drop). That second list is kept and persisted by the shell
//! ([`load_opened`], [`save_opened`]), never taken from the page: after a
//! restart, [`AccessPolicy::restore_opened`] keeps only the entries that are
//! still tabs of the session, so a file merely reached through a link never
//! becomes writable.
//!
//! Everything here is computation on strings: no file-system access, no
//! `canonicalize` (touching a network path is precisely what must not happen
//! before the check). Paths are normalised lexically (`.` and `..` resolved,
//! separators unified, Windows case folded, `\\?\C:\` treated as `C:\`). The
//! Windows rules take a [`PathStyle`] so they are tested on any platform.
//!
//! Limits of a lexical check, accepted on purpose: a symbolic link or
//! junction inside an allowed folder is followed by the later file access,
//! and a Windows 8.3 short name is a different string than the long name
//! (the path is then refused, never wrongly accepted).

use crate::atomic::write_data_file;
use crate::error::{ApiError, ErrorCode, Result};
use crate::paths::key_is_inside;
use crate::session::Session;
use crate::vaults::VaultRef;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// File of the application data directory that holds the files opened
/// explicitly by the user.
pub const OPENED_FILE: &str = "opened.json";

/// Most files remembered as opened explicitly; the oldest are forgotten
/// first (they simply become read-only links again, or unreachable).
pub const MAX_OPENED: usize = 256;

/// Most folders remembered as chosen in the folder dialog during a session.
pub const MAX_PICKED: usize = 32;

/// Extensions (lowercase) readable around a file opened outside any vault:
/// the text files a link can point to and the images a note can show.
pub const READABLE_AROUND_EXTENSIONS: [&str; 12] =
    ["md", "markdown", "txt", "canvas", "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "avif"];

/// Syntax used to interpret path strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// `/` separator, case-sensitive, no network form.
    Unix,
    /// `\` and `/` separators, case-insensitive, drive letters, UNC and
    /// verbatim (`\\?\`) forms.
    Windows,
}

impl PathStyle {
    /// Style of the platform the program runs on.
    pub fn native() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Where a path string comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOrigin {
    /// Sent by the web page (command argument): untrusted.
    Page,
    /// Produced by the operating system on the user's behalf (command line,
    /// second launch, file dialog, drag and drop) or read from Rocksy's own
    /// persisted session: trusted.
    Os,
}

/// A path reduced to its comparison form.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Normalized {
    /// `/`-less root: `` (Unix), `c:` (drive) or `\\server\share` (network).
    root: String,
    components: Vec<String>,
    /// Root and components as written (case kept), for paths handed back
    /// to the caller.
    shown_root: String,
    shown: Vec<String>,
    network: bool,
    windows: bool,
}

impl Normalized {
    fn sep(&self) -> char {
        if self.windows {
            '\\'
        } else {
            '/'
        }
    }

    fn key_of(&self, components: &[String]) -> String {
        self.join(&self.root, components)
    }

    fn join(&self, root: &str, components: &[String]) -> String {
        let sep = self.sep();
        let mut key = root.to_string();
        if components.is_empty() {
            // Bare root: `/`, `c:\`; a share root has no trailing separator.
            if !self.network {
                key.push(sep);
            }
            return key;
        }
        for c in components {
            key.push(sep);
            key.push_str(c);
        }
        key
    }

    fn key(&self) -> String {
        self.key_of(&self.components)
    }

    /// Key of the containing folder, unless that folder is a bare root
    /// (a file at `C:\` or `/` must not open the whole volume for reading).
    fn parent_key(&self) -> Option<String> {
        match self.components.len() {
            0 | 1 => None,
            n => Some(self.key_of(&self.components[..n - 1])),
        }
    }

    /// The path in its normal form with the case as written.
    fn shown(&self) -> String {
        self.join(&self.shown_root, &self.shown)
    }

    /// The containing folder with the case as written (same rule as
    /// [`Normalized::parent_key`]).
    fn shown_parent(&self) -> Option<String> {
        match self.shown.len() {
            0 | 1 => None,
            n => Some(self.join(&self.shown_root, &self.shown[..n - 1])),
        }
    }

    /// Extension of the last component, lowercase (`a.tar.gz` -> `gz`;
    /// none for `.bashrc` or `README`).
    fn extension(&self) -> Option<String> {
        let name = self.shown.last()?;
        let dot = name.rfind('.')?;
        (dot > 0 && dot + 1 < name.len()).then(|| name[dot + 1..].to_lowercase())
    }
}

fn invalid(what: &str, raw: &str) -> ApiError {
    ApiError::invalid(format!("{what}: {raw}"))
}

/// A refusal the page is expected to recognise (`ApiError.code`).
fn refused(code: ErrorCode, what: &str, raw: &str) -> ApiError {
    invalid(what, raw).with_code(code)
}

/// Resolves `.` and `..` lexically. `..` never climbs above the root.
fn push_components<'a>(
    out: &mut Vec<String>,
    shown: &mut Vec<String>,
    parts: impl Iterator<Item = &'a str>,
    windows: bool,
    raw: &str,
) -> Result<()> {
    for part in parts {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
                shown.pop();
            }
            name => {
                if windows {
                    // Windows drops trailing dots and spaces and reads `a:b`
                    // as a data stream: such a name designates another file
                    // than the one compared here.
                    if name.ends_with('.') || name.ends_with(' ') {
                        return Err(invalid("path component ends with a dot or a space", raw));
                    }
                    if name.contains(':') {
                        return Err(invalid("path component contains ':'", raw));
                    }
                    out.push(name.to_lowercase());
                } else {
                    out.push(name.to_string());
                }
                shown.push(name.to_string());
            }
        }
    }
    Ok(())
}

fn is_drive_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
}

fn normalize(raw: &str, style: PathStyle) -> Result<Normalized> {
    if raw.is_empty() {
        return Err(invalid("empty path", raw));
    }
    if raw.contains('\0') {
        return Err(invalid("path contains a NUL character", "(not shown)"));
    }
    match style {
        PathStyle::Unix => normalize_unix(raw),
        PathStyle::Windows => normalize_windows(raw),
    }
}

fn normalize_unix(raw: &str) -> Result<Normalized> {
    if !raw.starts_with('/') {
        return Err(invalid("path must be absolute", raw));
    }
    let (mut components, mut shown) = (Vec::new(), Vec::new());
    push_components(&mut components, &mut shown, raw.split('/'), false, raw)?;
    Ok(Normalized { root: String::new(), components, shown_root: String::new(), shown, network: false, windows: false })
}

fn normalize_windows(raw: &str) -> Result<Normalized> {
    // Win32 accepts both separators everywhere, `//?/` included.
    let s = raw.replace('/', "\\");
    let leading = s.len() - s.trim_start_matches('\\').len();
    let rest = &s[leading..];
    if leading >= 2 {
        let mut chars = rest.chars();
        let marker = chars.next();
        let after_marker = chars.as_str();
        if matches!(marker, Some('?' | '.')) && (after_marker.is_empty() || after_marker.starts_with('\\')) {
            let body = after_marker.strip_prefix('\\').unwrap_or("");
            if marker == Some('?') {
                if body.len() >= 4 && body[..4].eq_ignore_ascii_case("UNC\\") {
                    return network(&body[4..], raw);
                }
                if is_drive_absolute(body) {
                    return drive(body, raw);
                }
            }
            return Err(invalid("device path refused", raw));
        }
        return network(rest, raw);
    }
    if leading == 1 {
        if rest.starts_with("??\\") {
            return Err(invalid("device path refused", raw));
        }
        return Err(invalid("path must be absolute (drive letter missing)", raw));
    }
    if is_drive_absolute(&s) {
        return drive(&s, raw);
    }
    Err(invalid("path must be absolute", raw))
}

fn drive(s: &str, raw: &str) -> Result<Normalized> {
    let (mut components, mut shown) = (Vec::new(), Vec::new());
    push_components(&mut components, &mut shown, s[3..].split('\\'), true, raw)?;
    Ok(Normalized {
        root: s[..2].to_lowercase(),
        components,
        shown_root: s[..2].to_string(),
        shown,
        network: false,
        windows: true,
    })
}

/// `rest` is what follows the leading separators: `server\share\...`.
fn network(rest: &str, raw: &str) -> Result<Normalized> {
    let mut parts = rest.split('\\').filter(|p| !p.is_empty());
    let (Some(server), Some(share)) = (parts.next(), parts.next()) else {
        return Err(invalid("incomplete network path", raw));
    };
    if [server, share].iter().any(|p| *p == "." || *p == "..") {
        return Err(invalid("invalid network path", raw));
    }
    let (mut components, mut shown) = (Vec::new(), Vec::new());
    push_components(&mut components, &mut shown, parts, true, raw)?;
    Ok(Normalized {
        root: format!("\\\\{}\\{}", server.to_lowercase(), share.to_lowercase()),
        components,
        shown_root: format!("\\\\{server}\\{share}"),
        shown,
        network: true,
        windows: true,
    })
}

/// True if `path` designates a network location in `style` (`\\server\share`,
/// `//server/share`, `\\?\UNC\server\share`). A path that cannot be
/// interpreted is not a network path; the checks refuse it anyway.
pub fn is_network_path(path: &str, style: PathStyle) -> bool {
    normalize(path, style).map(|n| n.network).unwrap_or(false)
}

/// A registered vault root.
#[derive(Debug, Clone)]
struct VaultRoot {
    key: String,
    /// Normal form with the case as registered.
    shown: String,
    network: bool,
}

/// A file opened explicitly by the user.
#[derive(Debug, Clone)]
struct OpenedFile {
    key: String,
    /// The path as it was given (what is persisted and compared with tabs).
    path: String,
    /// Key of its folder, when that folder gives read access around the
    /// file: local, and not a bare root.
    dir_key: Option<String>,
    /// The same folder with the case as written.
    dir_shown: Option<String>,
}

/// Decides which paths the page may read and write. Pure computation: keep it
/// behind a `Mutex` in the shell and consult it before `read_file`,
/// `save_file`, `set_session` and before watching a tab.
#[derive(Debug, Clone)]
pub struct AccessPolicy {
    style: PathStyle,
    vaults: Vec<VaultRoot>,
    /// Files opened explicitly, oldest first, at most [`MAX_OPENED`].
    opened: Vec<OpenedFile>,
    /// Keys of the folders the folder dialog returned during this session.
    picked: Vec<String>,
    /// `add_vault` accepts any folder (isolated instance).
    free_vaults: bool,
}

impl AccessPolicy {
    /// Policy for the current platform with the given registered vaults.
    pub fn new(vaults: &[VaultRef]) -> Self {
        Self::with_style(PathStyle::native(), vaults)
    }

    /// Same as [`AccessPolicy::new`] with an explicit path syntax (tests).
    pub fn with_style(style: PathStyle, vaults: &[VaultRef]) -> Self {
        let mut policy = Self { style, vaults: Vec::new(), opened: Vec::new(), picked: Vec::new(), free_vaults: false };
        policy.set_vaults(vaults);
        policy
    }

    /// Replaces the list of registered vaults (after `add_vault` or
    /// `remove_vault`). The registry is never read from disk by the policy.
    pub fn set_vaults(&mut self, vaults: &[VaultRef]) {
        self.vaults = vaults
            .iter()
            .filter_map(|v| normalize(&v.path, self.style).ok())
            .map(|n| VaultRoot { key: n.key(), shown: n.shown(), network: n.network })
            .collect();
    }

    /// Records a file the user opened explicitly (command line, second
    /// launch, file dialog, drag and drop). Only call this with a path that
    /// comes from the operating system, never with one sent by the page.
    /// A file on a network share is accepted. Beyond [`MAX_OPENED`] files
    /// the oldest record is dropped; opening a file again makes it the most
    /// recent. Returns whether the list changed (it then has to be
    /// persisted, see [`save_opened`]).
    ///
    /// Errors: `invalid` (not absolute, device path, malformed).
    pub fn allow_opened(&mut self, path: &str) -> Result<bool> {
        let n = normalize(path, self.style)?;
        if n.components.is_empty() {
            return Err(invalid("not a file path", path));
        }
        let key = n.key();
        if self.opened.last().is_some_and(|o| o.key == key) {
            return Ok(false);
        }
        self.opened.retain(|o| o.key != key);
        let (dir_key, dir_shown) = if n.network { (None, None) } else { (n.parent_key(), n.shown_parent()) };
        self.opened.push(OpenedFile { key, path: path.to_string(), dir_key, dir_shown });
        if self.opened.len() > MAX_OPENED {
            let excess = self.opened.len() - MAX_OPENED;
            self.opened.drain(..excess);
        }
        Ok(true)
    }

    /// The files opened explicitly, oldest first, as they were given: what
    /// [`save_opened`] persists.
    pub fn opened_files(&self) -> Vec<String> {
        self.opened.iter().map(|o| o.path.clone()).collect()
    }

    /// At startup: gives back the status "opened explicitly" to the entries
    /// of `persisted` (the list written by [`save_opened`] during the last
    /// run) that are tabs of `session`, and to those only. A tab that is not
    /// in the persisted list (a file reached through a link, or a tab the
    /// page slipped into the session) gains nothing. Returns the number of
    /// files restored; call [`AccessPolicy::opened_files`] to persist the
    /// pruned list.
    pub fn restore_opened(&mut self, persisted: &[String], session: Option<&Session>) -> usize {
        let tabs: HashSet<String> = session
            .map(|s| s.tabs.iter().filter_map(|t| normalize(t, self.style).ok()).map(|n| n.key()).collect())
            .unwrap_or_default();
        let mut restored = 0;
        for path in persisted {
            let is_tab = normalize(path, self.style).is_ok_and(|n| tabs.contains(&n.key()));
            if is_tab && self.allow_opened(path).is_ok() {
                restored += 1;
            }
        }
        restored
    }

    fn in_vault(&self, key: &str) -> bool {
        let windows = self.style == PathStyle::Windows;
        self.vaults.iter().any(|v| key_is_inside(key, &v.key, windows))
    }

    fn is_opened(&self, key: &str) -> bool {
        self.opened.iter().any(|o| o.key == key)
    }

    fn in_opened_dir(&self, key: &str) -> bool {
        let windows = self.style == PathStyle::Windows;
        self.opened.iter().filter_map(|o| o.dir_key.as_deref()).any(|d| key_is_inside(key, d, windows))
    }

    /// May `path` be read?
    ///
    /// From the page: yes if it lies under a registered vault, is a file
    /// opened explicitly, or is a text file or an image
    /// ([`READABLE_AROUND_EXTENSIONS`]) in the folder of such a file or
    /// below it (relative links and images of a note outside any vault). A
    /// network path from the page is accepted only if it is itself an
    /// opened file or lies under a registered vault. From the OS: any
    /// well-formed absolute path.
    ///
    /// Errors: `invalid`.
    pub fn check_read(&self, path: &str, origin: PathOrigin) -> Result<()> {
        let n = normalize(path, self.style)?;
        if origin == PathOrigin::Os {
            return Ok(());
        }
        let key = n.key();
        if self.in_vault(&key) || self.is_opened(&key) {
            return Ok(());
        }
        if n.network {
            return Err(refused(ErrorCode::NetworkPath, "network path refused", path));
        }
        if self.in_opened_dir(&key) {
            return match n.extension() {
                Some(ext) if READABLE_AROUND_EXTENSIONS.contains(&ext.as_str()) => Ok(()),
                _ => Err(refused(
                    ErrorCode::NotAllowed,
                    "path not allowed (only text files and images are readable around a file opened by the user)",
                    path,
                )),
            };
        }
        Err(refused(
            ErrorCode::NotAllowed,
            "path not allowed (outside the registered vaults and the files opened by the user)",
            path,
        ))
    }

    /// May `path` be written? Only a file opened explicitly or a path under a
    /// registered vault; the folder of an opened file gives no write access.
    /// The extension and the `.obsidian`/`.canvas` rules are checked
    /// elsewhere (`save_file`, [`crate::files::is_savable_name`]).
    ///
    /// Errors: `invalid`.
    pub fn check_write(&self, path: &str, origin: PathOrigin) -> Result<()> {
        let n = normalize(path, self.style)?;
        if n.components.is_empty() {
            return Err(invalid("not a file path", path));
        }
        if origin == PathOrigin::Os {
            return Ok(());
        }
        let key = n.key();
        if self.in_vault(&key) || self.is_opened(&key) {
            return Ok(());
        }
        if n.network {
            return Err(refused(ErrorCode::NetworkPath, "network path refused", path));
        }
        Err(refused(
            ErrorCode::NotAllowed,
            "writing not allowed (outside the registered vaults and the files opened by the user)",
            path,
        ))
    }

    /// Is `path` exactly the root of a registered vault? (`open_vault`,
    /// `build_tags`, `Session.activeVault`.)
    ///
    /// Errors: `invalid`.
    pub fn check_vault(&self, path: &str) -> Result<()> {
        let key = normalize(path, self.style)?.key();
        if self.vaults.iter().any(|v| v.key == key) {
            Ok(())
        } else {
            Err(refused(ErrorCode::NotAllowed, "not a registered vault", path))
        }
    }

    /// Records a folder the user chose in the system folder dialog
    /// (`pick_folder`). Only call this with the path the dialog returned,
    /// never with one sent by the page. Kept for the session, not persisted;
    /// beyond [`MAX_PICKED`] folders the oldest is forgotten.
    ///
    /// Errors: `invalid` (not absolute, device path, malformed).
    pub fn allow_picked_folder(&mut self, path: &str) -> Result<()> {
        let key = normalize(path, self.style)?.key();
        self.picked.retain(|k| *k != key);
        self.picked.push(key);
        if self.picked.len() > MAX_PICKED {
            let excess = self.picked.len() - MAX_PICKED;
            self.picked.drain(..excess);
        }
        Ok(())
    }

    /// Lets `add_vault` register any folder. Decided by whoever launches the
    /// process (an isolated instance for tests and measurements, see
    /// `ROCKSY_DATA_DIR`), never by the page.
    pub fn set_free_vault_registration(&mut self, free: bool) {
        self.free_vaults = free;
    }

    /// May the page register `path` as a vault (`add_vault`)? Registering a
    /// vault gives read and write access to everything below it, so the
    /// page cannot choose the folder: it must be one the system folder
    /// dialog returned during this session
    /// ([`AccessPolicy::allow_picked_folder`]), or a vault that is already
    /// registered (no new right). See also
    /// [`AccessPolicy::set_free_vault_registration`].
    ///
    /// Errors: `invalid` (malformed path, or code `not_allowed`).
    pub fn check_add_vault(&self, path: &str) -> Result<()> {
        let key = normalize(path, self.style)?.key();
        if self.free_vaults || self.picked.contains(&key) || self.vaults.iter().any(|v| v.key == key) {
            Ok(())
        } else {
            Err(refused(
                ErrorCode::NotAllowed,
                "not a folder chosen in the folder dialog during this session",
                path,
            ))
        }
    }

    /// The part of a session sent by the page that may be persisted and
    /// watched: tabs the page may read, the active tab if it is one of them,
    /// the active vault if it is registered.
    pub fn sanitize_session(&self, session: &Session) -> Session {
        let tabs: Vec<String> = session
            .tabs
            .iter()
            .filter(|tab| self.check_read(tab, PathOrigin::Page).is_ok())
            .cloned()
            .collect();
        let active_tab = session.active_tab.clone().filter(|t| tabs.contains(t));
        let active_vault = session.active_vault.clone().filter(|v| self.check_vault(v).is_ok());
        Session { active_vault, tabs, active_tab }
    }

    /// Folders whose images the page may load through the asset protocol:
    /// the registered vaults and the folders of the files opened
    /// explicitly, without duplicates. Never a network location (the asset
    /// protocol touches a path before checking it against its scope) and
    /// never a bare volume root.
    pub fn asset_roots(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let vaults = self.vaults.iter().filter(|v| !v.network).map(|v| (&v.key, &v.shown));
        let dirs = self.opened.iter().filter_map(|o| Some((o.dir_key.as_ref()?, o.dir_shown.as_ref()?)));
        vaults.chain(dirs).filter(|(key, _)| seen.insert((*key).clone())).map(|(_, shown)| shown.clone()).collect()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OpenedList {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    files: Vec<String>,
}

/// The files opened explicitly during the previous runs, oldest first, as
/// persisted by [`save_opened`] in `data_dir`. A missing or corrupted file is
/// an empty list. At most [`MAX_OPENED`] entries are returned (the most
/// recent ones).
pub fn load_opened(data_dir: &Path) -> Vec<String> {
    let Ok(bytes) = fs::read(data_dir.join(OPENED_FILE)) else { return Vec::new() };
    let mut files = serde_json::from_slice::<OpenedList>(&bytes).map(|l| l.files).unwrap_or_default();
    if files.len() > MAX_OPENED {
        files.drain(..files.len() - MAX_OPENED);
    }
    files
}

/// Persists the list of files opened explicitly (atomic write in
/// `data_dir`). Only the shell calls this, with
/// [`AccessPolicy::opened_files`]: the page has no way to add an entry.
pub fn save_opened(data_dir: &Path, files: &[String]) -> Result<()> {
    let start = files.len().saturating_sub(MAX_OPENED);
    let list = OpenedList { version: 1, files: files[start..].to_vec() };
    let json = serde_json::to_vec_pretty(&list)
        .map_err(|e| ApiError::io(format!("cannot serialize the list of opened files: {e}")))?;
    write_data_file(&data_dir.join(OPENED_FILE), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use PathOrigin::{Os, Page};
    use PathStyle::{Unix, Windows};

    fn vault(path: &str) -> VaultRef {
        VaultRef { path: path.into(), name: "v".into() }
    }

    fn key(path: &str, style: PathStyle) -> String {
        normalize(path, style).unwrap().key()
    }

    fn refused(r: Result<()>) -> String {
        let e = r.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Invalid);
        e.message
    }

    #[test]
    fn only_a_picked_folder_can_become_a_vault() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\Vaults\Known")]);
        // Sent by the page: refused, with the stable code.
        let e = p.check_add_vault(r"C:\Users\me").unwrap_err();
        assert_eq!((e.kind, e.code), (ErrorKind::Invalid, Some(ErrorCode::NotAllowed)));
        assert!(p.check_add_vault(r"C:\").is_err());
        // Already registered: nothing is gained, accepted (idempotent add).
        p.check_add_vault(r"c:/vaults/known/").unwrap();
        // A parent or a child of a picked folder is another folder.
        p.allow_picked_folder(r"C:\Users\me\Notes").unwrap();
        p.check_add_vault(r"c:\users\ME\notes\").unwrap();
        p.check_add_vault(r"\\?\C:\Users\me\x\..\Notes").unwrap();
        assert!(p.check_add_vault(r"C:\Users\me").is_err());
        assert!(p.check_add_vault(r"C:\Users\me\Notes\sub").is_err());
        // Malformed and device paths stay refused.
        assert!(p.allow_picked_folder(r"\\.\PhysicalDrive0").is_err());
        assert!(p.check_add_vault("relative").is_err());
        // The list is bounded: the oldest pick is forgotten.
        for i in 0..MAX_PICKED {
            p.allow_picked_folder(&format!(r"D:\p{i}")).unwrap();
        }
        assert!(p.check_add_vault(r"C:\Users\me\Notes").is_err());
        p.check_add_vault(r"D:\p0").unwrap();
        // Replacing the vault list does not forget the picks.
        p.set_vaults(&[]);
        p.check_add_vault(r"D:\p1").unwrap();
        assert!(p.check_add_vault(r"C:\Vaults\Known").is_err());
    }

    #[test]
    fn an_isolated_instance_registers_any_folder() {
        let mut p = AccessPolicy::with_style(Unix, &[]);
        assert!(p.check_add_vault("/home/me/notes").is_err());
        p.set_free_vault_registration(true);
        p.check_add_vault("/home/me/notes").unwrap();
        // Still a well-formed absolute path.
        assert!(p.check_add_vault("notes").is_err());
        p.set_free_vault_registration(false);
        assert!(p.check_add_vault("/home/me/notes").is_err());
    }

    #[test]
    fn refusals_carry_a_stable_code() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\V")]);
        p.allow_opened(r"D:\docs\a.md").unwrap();
        let code = |r: Result<()>| r.unwrap_err().code;
        assert_eq!(code(p.check_read(r"C:\other\x.md", Page)), Some(ErrorCode::NotAllowed));
        assert_eq!(code(p.check_read(r"D:\docs\x.exe", Page)), Some(ErrorCode::NotAllowed));
        assert_eq!(code(p.check_read(r"\\srv\share\x.md", Page)), Some(ErrorCode::NetworkPath));
        assert_eq!(code(p.check_write(r"D:\docs\b.md", Page)), Some(ErrorCode::NotAllowed));
        assert_eq!(code(p.check_write(r"\\srv\share\x.md", Page)), Some(ErrorCode::NetworkPath));
        assert_eq!(code(p.check_vault(r"C:\other")), Some(ErrorCode::NotAllowed));
        // Malformed paths have no code: nothing the page should react to.
        assert_eq!(code(p.check_read("relative.md", Page)), None);
    }

    #[test]
    fn windows_keys_fold_case_separators_and_dots() {
        let k = key(r"G:\Mon Drive\PKM\a.md", Windows);
        assert_eq!(k, r"g:\mon drive\pkm\a.md");
        assert_eq!(key("g:/mon drive/PKM/a.md", Windows), k);
        assert_eq!(key(r"G:\Mon Drive\.\PKM\\x\..\a.md", Windows), k);
        assert_eq!(key(r"\\?\G:\Mon Drive\PKM\a.md", Windows), k);
        assert_eq!(key("//?/G:/Mon Drive/PKM/a.md", Windows), k);
        assert_eq!(key(r"C:\", Windows), r"c:\");
        // `..` never climbs above the root.
        assert_eq!(key(r"C:\..\..\x", Windows), r"c:\x");
    }

    #[test]
    fn network_forms_share_one_key() {
        let k = key(r"\\Srv\Share\d\a.md", Windows);
        assert_eq!(k, r"\\srv\share\d\a.md");
        for form in [
            "//srv/share/d/a.md",
            r"\\?\UNC\srv\share\d\a.md",
            r"\\?\unc\SRV\share\d\a.md",
            "//?/UNC/srv/share/d/a.md",
            r"\\\srv\share\d\a.md",
            r"\/srv/share\d\x\..\a.md",
        ] {
            assert_eq!(key(form, Windows), k, "{form}");
            assert!(is_network_path(form, Windows), "{form}");
        }
        // `..` stops at the share.
        assert_eq!(key(r"\\srv\share\..\..\x", Windows), r"\\srv\share\x");
        assert_eq!(key(r"\\srv\share", Windows), r"\\srv\share");
        assert!(!is_network_path(r"\\?\C:\x", Windows));
        assert!(!is_network_path(r"C:\x", Windows));
        // On Unix `//a/b` is the local `/a/b`.
        assert!(!is_network_path("//srv/share/a.md", Unix));
        assert_eq!(key("//srv/share/a.md", Unix), "/srv/share/a.md");
    }

    #[test]
    fn malformed_and_device_paths_are_invalid() {
        for bad in [
            "",
            "a.md",
            r"sub\a.md",
            "C:a.md",
            "C:",
            r"\Windows\x",
            r"\\.\C:\x",
            r"\\.\pipe\x",
            r"\\?\Volume{1}\x",
            r"\\?\GLOBALROOT\Device\x",
            r"\??\C:\x",
            r"\\srv",
            r"\\srv\",
            r"\\?\UNC\srv",
            r"\\..\share\x",
            r"C:\v\a.md.",
            r"C:\v\a.md ",
            r"C:\v\dir.\a.md",
            r"C:\v\a.md::$DATA",
            r"C:\v\a.md:stream",
            "C:\\v\\a\0.md",
        ] {
            let e = normalize(bad, Windows).unwrap_err();
            assert_eq!(e.kind, ErrorKind::Invalid, "{bad:?}");
        }
        for bad in ["", "a.md", "./a.md", "~/a.md", r"C:\a.md", "/a\0b"] {
            assert!(normalize(bad, Unix).is_err(), "{bad:?}");
        }
        // Unix keeps case, and backslash, colon, trailing dot are ordinary characters.
        assert_ne!(key("/V/a.md", Unix), key("/v/a.md", Unix));
        assert_eq!(key("/v/./x/../a:b. /", Unix), "/v/a:b. ");
        assert_eq!(key("/../..", Unix), "/");
    }

    #[test]
    fn vault_paths_are_allowed_whatever_the_spelling() {
        let p = AccessPolicy::with_style(Windows, &[vault(r"G:\Mon Drive\PKM")]);
        for ok in [
            r"G:\Mon Drive\PKM\a.md",
            r"g:\mon drive\pkm\Sub\B.MD",
            "G:/Mon Drive/PKM/sub/b.md",
            r"\\?\G:\Mon Drive\PKM\a.md",
            r"G:\Mon Drive\PKM\sub\..\a.md",
            r"G:\Mon Drive\PKM",
        ] {
            p.check_read(ok, Page).unwrap();
            p.check_write(ok, Page).unwrap();
        }
        for ko in [
            r"G:\Mon Drive\PKM\..\secret.md",
            r"G:\Mon Drive\PKM\sub\..\..\secret.md",
            r"G:\Mon Drive\PKM2\a.md",
            r"G:\Mon Drive\PK",
            r"G:\Mon Drive",
            r"C:\Mon Drive\PKM\a.md",
            r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.cmd",
        ] {
            assert!(refused(p.check_read(ko, Page)).contains("not allowed"), "{ko}");
            assert!(refused(p.check_write(ko, Page)).contains("not allowed"), "{ko}");
        }
    }

    #[test]
    fn unix_vault_is_case_sensitive_and_dotdot_proof() {
        let p = AccessPolicy::with_style(Unix, &[vault("/home/a/vault/")]);
        p.check_read("/home/a/vault/n.md", Page).unwrap();
        p.check_write("/home/a/vault/d/../n.md", Page).unwrap();
        refused(p.check_read("/home/a/Vault/n.md", Page));
        refused(p.check_read("/home/a/vault/../.ssh/id_rsa", Page));
        refused(p.check_write("/home/a/vault2/n.md", Page));
        refused(p.check_read("vault/n.md", Page));
    }

    #[test]
    fn opened_file_gives_read_on_its_folder_and_write_on_itself_only() {
        let mut p = AccessPolicy::with_style(Windows, &[]);
        refused(p.check_read(r"D:\docs\note.md", Page));
        p.allow_opened(r"D:\docs\note.md").unwrap();

        p.check_read(r"d:/DOCS/Note.md", Page).unwrap();
        p.check_write(r"D:\docs\note.md", Page).unwrap();
        p.check_write(r"\\?\D:\docs\x\..\NOTE.md", Page).unwrap();
        // Relative links and images of that note.
        p.check_read(r"D:\docs\other.md", Page).unwrap();
        p.check_read(r"D:\docs\img\a.png", Page).unwrap();
        assert!(refused(p.check_write(r"D:\docs\other.md", Page)).contains("writing not allowed"));
        assert!(refused(p.check_write(r"D:\docs\img\a.png", Page)).contains("writing not allowed"));
        // Not above, not beside.
        refused(p.check_read(r"D:\docs\..\secret.md", Page));
        refused(p.check_read(r"D:\docs2\a.md", Page));
        refused(p.check_read(r"D:\a.md", Page));
    }

    #[test]
    fn file_at_a_volume_root_does_not_open_the_volume() {
        let mut p = AccessPolicy::with_style(Windows, &[]);
        p.allow_opened(r"D:\note.md").unwrap();
        p.check_read(r"D:\note.md", Page).unwrap();
        p.check_write(r"D:\note.md", Page).unwrap();
        refused(p.check_read(r"D:\other.md", Page));
        refused(p.check_read(r"D:\Users\a\x.md", Page));

        let mut u = AccessPolicy::with_style(Unix, &[]);
        u.allow_opened("/note.md").unwrap();
        u.check_read("/note.md", Page).unwrap();
        refused(u.check_read("/etc/passwd", Page));
        assert!(u.allow_opened("/").is_err());
        assert!(p.allow_opened(r"C:\").is_err());
        assert!(p.allow_opened("note.md").is_err());
    }

    #[test]
    fn network_paths_from_the_page_are_refused() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\vault")]);
        p.allow_opened(r"C:\docs\note.md").unwrap();
        for unc in [
            r"\\attacker\s\a.png",
            "//attacker.example/s/a.png",
            r"\\?\UNC\attacker\s\a.png",
            "//?/UNC/attacker/s/a.md",
            r"\\\attacker\s\a.md",
            r"\\10.0.0.1\c$\x.md",
            // Lexically "inside" an allowed folder, yet a network path.
            r"\\attacker\s\..\..\C:\vault\a.md",
        ] {
            let r = p.check_read(unc, Page);
            assert!(r.is_err(), "{unc}");
            assert!(p.check_write(unc, Page).is_err(), "{unc}");
            // The same strings are legitimate when the OS hands them over.
            if normalize(unc, Windows).is_ok() {
                assert!(refused(r).contains("network path refused"), "{unc}");
                p.check_read(unc, Os).unwrap();
            }
        }
        // The verbatim form of a local path is the local path.
        p.check_read(r"\\?\C:\vault\a.md", Page).unwrap();
        p.check_write(r"\\?\C:\docs\note.md", Page).unwrap();
        refused(p.check_read(r"\\?\C:\other\a.md", Page));
        // Device paths are refused from any origin.
        refused(p.check_read(r"\\.\C:\vault\a.md", Page));
        refused(p.check_read(r"\\.\C:\vault\a.md", Os));
    }

    #[test]
    fn network_file_opened_by_the_user_is_allowed_alone() {
        let mut p = AccessPolicy::with_style(Windows, &[]);
        p.allow_opened(r"\\nas\docs\notes\a.md").unwrap();
        p.check_read("//NAS/docs/notes/a.md", Page).unwrap();
        p.check_write(r"\\?\UNC\nas\docs\notes\a.md", Page).unwrap();
        // No folder rule on the network: nothing else on the share, no other server.
        assert!(refused(p.check_read(r"\\nas\docs\notes\b.md", Page)).contains("network"));
        assert!(refused(p.check_read(r"\\nas\docs\notes\img\a.png", Page)).contains("network"));
        refused(p.check_read(r"\\other\docs\notes\a.md", Page));
        refused(p.check_write(r"\\nas\docs\notes\b.md", Page));
    }

    #[test]
    fn vault_on_a_network_share_is_allowed() {
        let p = AccessPolicy::with_style(Windows, &[vault(r"\\nas\vaults\pkm")]);
        p.check_read("//nas/vaults/PKM/a.md", Page).unwrap();
        p.check_write(r"\\?\UNC\nas\vaults\pkm\d\a.md", Page).unwrap();
        refused(p.check_read(r"\\nas\vaults\pkm\..\other\a.md", Page));
        refused(p.check_read(r"\\nas\vaults\other\a.md", Page));
        refused(p.check_read(r"\\evil\vaults\pkm\a.md", Page));
        p.check_vault(r"\\NAS\vaults\pkm\").unwrap();
    }

    #[test]
    fn vault_list_can_be_replaced() {
        let mut p = AccessPolicy::with_style(Unix, &[vault("/v1")]);
        p.check_read("/v1/a.md", Page).unwrap();
        refused(p.check_read("/v2/a.md", Page));
        p.set_vaults(&[vault("/v2"), vault("not absolute, skipped")]);
        refused(p.check_read("/v1/a.md", Page));
        p.check_write("/v2/a.md", Page).unwrap();
        p.check_vault("/v2/").unwrap();
        refused(p.check_vault("/v2/sub"));
        refused(p.check_vault("/v1"));
        p.set_vaults(&[]);
        refused(p.check_read("/v2/a.md", Page));
    }

    #[test]
    fn only_persisted_opened_files_that_are_still_tabs_are_restored() {
        let stored = Session {
            active_vault: Some(r"C:\vault".into()),
            tabs: vec![
                r"C:\vault\a.md".into(),
                r"D:\loose\b.md".into(),
                r"D:\loose\linked.md".into(),
                r"\\nas\docs\c.md".into(),
                r"E:\smuggled\x.md".into(),
                "garbage".into(),
            ],
            active_tab: Some(r"D:\loose\b.md".into()),
        };
        // What the backend persisted itself: b.md and c.md were opened by the
        // user, and so was closed.md, whose tab is gone.
        let persisted: Vec<String> =
            [r"d:\LOOSE\b.md", r"\\nas\docs\c.md", r"D:\other\closed.md", "garbage"].map(String::from).to_vec();
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\vault")]);
        refused(p.check_read(r"D:\loose\b.md", Page));
        assert_eq!(p.restore_opened(&persisted, Some(&stored)), 2);
        assert_eq!(p.opened_files(), [r"d:\LOOSE\b.md", r"\\nas\docs\c.md"]);

        p.check_read(r"D:\loose\b.md", Page).unwrap();
        p.check_write(r"D:\loose\b.md", Page).unwrap();
        p.check_read(r"\\nas\docs\c.md", Page).unwrap();
        // The tab reached through a link is readable again, never writable.
        p.check_read(r"D:\loose\linked.md", Page).unwrap();
        assert!(refused(p.check_write(r"D:\loose\linked.md", Page)).contains("writing not allowed"));
        // A tab that is not in the backend's list gains nothing, and a file
        // of the list whose tab was closed is forgotten.
        refused(p.check_read(r"E:\smuggled\x.md", Page));
        refused(p.check_read(r"D:\other\closed.md", Page));

        // Without a session nothing is restored.
        let mut q = AccessPolicy::with_style(Windows, &[]);
        assert_eq!(q.restore_opened(&persisted, None), 0);
        assert!(q.opened_files().is_empty());
    }

    #[test]
    fn page_sessions_are_filtered() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\vault")]);
        p.allow_opened(r"D:\loose\b.md").unwrap();
        p.allow_opened(r"\\nas\docs\c.md").unwrap();
        let sent = Session {
            active_vault: Some(r"C:\Users\a".into()),
            tabs: vec![
                r"C:\vault\a.md".into(),
                r"D:\loose\b.md".into(),
                r"D:\loose\linked.md".into(),
                r"D:\loose\tool.exe".into(),
                r"\\nas\docs\c.md".into(),
                r"\\attacker\s\x.md".into(),
                r"C:\Users\a\.ssh\id_rsa".into(),
                r"C:\vault\..\Users\a\x.md".into(),
            ],
            active_tab: Some(r"\\attacker\s\x.md".into()),
        };
        let kept = p.sanitize_session(&sent);
        assert_eq!(
            kept.tabs,
            [r"C:\vault\a.md", r"D:\loose\b.md", r"D:\loose\linked.md", r"\\nas\docs\c.md"]
        );
        assert_eq!(kept.active_tab, None);
        assert_eq!(kept.active_vault, None);

        let fine = Session {
            active_vault: Some("c:/VAULT/".into()),
            tabs: vec![r"C:\vault\a.md".into(), r"D:\loose\b.md".into()],
            active_tab: Some(r"D:\loose\b.md".into()),
        };
        let kept = p.sanitize_session(&fine);
        assert_eq!(kept.active_vault.as_deref(), Some("c:/VAULT/"));
        assert_eq!(kept.active_tab.as_deref(), Some(r"D:\loose\b.md"));
        assert_eq!(kept.tabs.len(), 2);
    }

    #[test]
    fn around_an_opened_file_only_text_and_images_are_readable() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"C:\vault")]);
        p.allow_opened(r"D:\docs\note.md").unwrap();
        for ok in [
            r"D:\docs\other.MD",
            r"D:\docs\a.markdown",
            r"D:\docs\readme.txt",
            r"D:\docs\board.canvas",
            r"D:\docs\img\a.PNG",
            r"D:\docs\img\deep\b.jpeg",
            r"D:\docs\c.jpg",
            r"D:\docs\d.gif",
            r"D:\docs\e.svg",
            r"D:\docs\f.webp",
            r"D:\docs\g.bmp",
            r"D:\docs\h.avif",
        ] {
            p.check_read(ok, Page).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for ko in [
            r"D:\docs\id_rsa",
            r"D:\docs\.env",
            r"D:\docs\.ssh\config",
            r"D:\docs\secrets.json",
            r"D:\docs\Login Data.sqlite",
            r"D:\docs\script.ps1",
            r"D:\docs\archive.md.zip",
            r"D:\docs\sub",
            r"D:\docs",
            r"D:\docs\note.",
        ] {
            assert!(p.check_read(ko, Page).is_err(), "{ko}");
        }
        assert!(refused(p.check_read(r"D:\docs\secrets.json", Page)).contains("only text files and images"));
        // The rule does not apply to the opened file itself, nor to a vault.
        p.allow_opened(r"D:\conf\settings.json").unwrap();
        p.check_read(r"D:\conf\settings.json", Page).unwrap();
        p.check_read(r"C:\vault\data\export.json", Page).unwrap();
        p.check_read(r"C:\vault\no-extension", Page).unwrap();

        let mut u = AccessPolicy::with_style(Unix, &[]);
        u.allow_opened("/home/a/docs/note.md").unwrap();
        u.check_read("/home/a/docs/img/a.png", Page).unwrap();
        refused(u.check_read("/home/a/docs/.git/config", Page));
        refused(u.check_read("/home/a/docs/key.pem", Page));
    }

    #[test]
    fn opened_list_is_bounded_ordered_and_reports_changes() {
        let mut p = AccessPolicy::with_style(Unix, &[]);
        assert!(p.allow_opened("/d/a.md").unwrap());
        assert!(!p.allow_opened("/d/a.md").unwrap(), "already the most recent: nothing to persist");
        assert!(p.allow_opened("/d/b.md").unwrap());
        assert!(p.allow_opened("/d/./a.md").unwrap(), "opened again: moves to the end");
        assert_eq!(p.opened_files(), ["/d/b.md", "/d/./a.md"]);

        for i in 0..MAX_OPENED + 10 {
            p.allow_opened(&format!("/many/{i}/n.md")).unwrap();
        }
        let files = p.opened_files();
        assert_eq!(files.len(), MAX_OPENED);
        assert_eq!(files[0], "/many/10/n.md");
        assert_eq!(files[MAX_OPENED - 1], format!("/many/{}/n.md", MAX_OPENED + 9));
        // The forgotten ones lost their rights, folder included.
        refused(p.check_read("/d/a.md", Page));
        refused(p.check_read("/many/9/n.md", Page));
        refused(p.check_read("/many/9/img.png", Page));
        p.check_write("/many/10/n.md", Page).unwrap();
    }

    #[test]
    fn asset_roots_are_local_vaults_and_folders_of_opened_files() {
        let mut p = AccessPolicy::with_style(Windows, &[vault(r"G:\Mon Drive\PKM\"), vault(r"\\nas\vaults\pkm")]);
        p.allow_opened(r"D:\Docs\Note.md").unwrap();
        p.allow_opened(r"D:\Docs\Other.md").unwrap();
        p.allow_opened(r"d:\docs\sub\..\third.md").unwrap();
        p.allow_opened(r"E:\at-root.md").unwrap();
        p.allow_opened(r"\\nas\docs\notes\a.md").unwrap();
        p.allow_opened(r"G:\Mon Drive\PKM\in-vault.md").unwrap();
        assert_eq!(p.asset_roots(), [r"G:\Mon Drive\PKM", r"D:\Docs"]);
        for root in p.asset_roots() {
            assert!(!is_network_path(&root, Windows));
        }

        let mut u = AccessPolicy::with_style(Unix, &[vault("/home/a/vault")]);
        u.allow_opened("/home/a/Docs/n.md").unwrap();
        u.allow_opened("/n.md").unwrap();
        assert_eq!(u.asset_roots(), ["/home/a/vault", "/home/a/Docs"]);
    }

    #[test]
    fn opened_list_round_trips_through_the_data_directory() {
        let dir = crate::testutil::TempDir::new("opened");
        let data = dir.path().join("data");
        assert!(load_opened(&data).is_empty());
        let files: Vec<String> = ["/d/a.md", "/d/b.md"].map(String::from).to_vec();
        save_opened(&data, &files).unwrap();
        assert_eq!(load_opened(&data), files);
        assert_eq!(crate::testutil::all_files(&data), vec![OPENED_FILE]);

        // Bounded on both sides, most recent kept.
        let many: Vec<String> = (0..MAX_OPENED + 5).map(|i| format!("/m/{i}.md")).collect();
        save_opened(&data, &many).unwrap();
        let back = load_opened(&data);
        assert_eq!(back.len(), MAX_OPENED);
        assert_eq!(back[0], "/m/5.md");

        // A corrupted file is an empty list, then overwritten.
        fs::write(data.join(OPENED_FILE), "{ not json").unwrap();
        assert!(load_opened(&data).is_empty());
        save_opened(&data, &files).unwrap();
        assert_eq!(load_opened(&data), files);
    }

    #[test]
    fn os_origin_accepts_any_well_formed_path() {
        let p = AccessPolicy::with_style(Windows, &[]);
        p.check_read(r"C:\anywhere\a.md", Os).unwrap();
        p.check_write(r"\\nas\docs\a.md", Os).unwrap();
        refused(p.check_read("relative.md", Os));
        refused(p.check_write(r"C:\", Os));
        // Checking with the OS origin grants nothing to the page.
        refused(p.check_read(r"C:\anywhere\a.md", Page));
    }

    #[test]
    fn native_policy_uses_the_platform_style() {
        let p = AccessPolicy::new(&[]);
        assert_eq!(p.style, if cfg!(windows) { Windows } else { Unix });
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AccessPolicy>();
    }
}
