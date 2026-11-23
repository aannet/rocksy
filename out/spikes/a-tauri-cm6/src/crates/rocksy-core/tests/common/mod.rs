//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

static N: AtomicU64 = AtomicU64::new(0);

/// Unique temporary directory removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("rocksy-core-it-{label}-{}-{n}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// State of one file: content hash, size, modification time (ns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub hash: u64,
    pub size: u64,
    pub mtime_ns: u128,
}

/// Every file and folder below `dir`, hidden ones included (folders have an
/// empty state), keyed by relative path.
pub fn fingerprint(dir: &Path) -> BTreeMap<String, Option<FileState>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<FileState>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.insert(format!("{rel}/"), None);
                walk(base, &path, out);
            } else {
                let bytes = fs::read(&path).unwrap();
                let meta = fs::metadata(&path).unwrap();
                let mtime_ns = meta.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                out.insert(
                    rel,
                    Some(FileState { hash: rocksy_core::paths::fnv1a64(&bytes), size: meta.len(), mtime_ns }),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A small vault exercising everything the core must leave alone.
pub fn make_vault(dir: &TempDir, name: &str) -> PathBuf {
    let files: &[(&str, &[u8])] = &[
        ("Accueil.md", b"---\ntags: [accueil, projet/rocksy]\n---\n# Accueil\n\nBienvenue #demo\n"),
        ("01 Travail/Journal.md", b"\xEF\xBB\xBF# Journal\r\n\r\n- lundi #journal/2026\r\n  - detail #todo\r\n"),
        ("01 Travail/Reunions/Point.md", b"> [!note] #reunion\n> contenu\n\n| a | b |\n|---|---|\n| x #table | y |\n"),
        ("02 Perso/Recettes.md", "Tarte #recette/dessert et #été\n".as_bytes()),
        ("02 Perso/vide.md", b""),
        ("02 Perso/latin1.md", b"caf\xE9 #x\n"),
        ("_ASSETS/image.png", b"\x89PNG\r\n\x1a\n\x00\x00binary\xff\xfe"),
        ("Tableau.canvas", b"{\"nodes\":[{\"id\":\"1\",\"type\":\"text\",\"text\":\"#canvas\"}],\"edges\":[]}"),
        (".obsidian/app.json", b"{\"promptDelete\":false}"),
        (".obsidian/workspace.json", b"{\"active\":\"abc\"}"),
        (".obsidian/core-plugins.json", b"[\"file-explorer\"]"),
        (".obsidian/community-plugins.json", b"[\"templater\"]"),
        (".obsidian/community-plugins (conflict 2026-11-02).json", b"[\"templater\",\"x\"]"),
        (".obsidian/plugins/demo/data.json", b"{\"apiKey\":\"not-a-real-key\"}"),
        (".obsidian/plugins/demo/main.js", b"module.exports = {};"),
        (".opencode/node_modules/pkg/index.js", b"// #not-a-tag\n"),
        (".trash/ancien.md", b"#corbeille\n"),
    ];
    for (rel, content) in files {
        dir.write(&format!("{name}/{rel}"), content);
    }
    fs::create_dir_all(dir.path().join(name).join("03 Vide")).unwrap();
    dir.path().join(name)
}
