//! In-memory content of the tag index: files, occurrences and the tag tree.
//!
//! Tags form a tree (`a/b/c` is a child of `a/b`); every node keeps the
//! number of occurrences of itself and of its descendants, so listing and
//! filtering never touch the fragments.

use super::parse::Parsed;
use super::TagSource;
use crate::paths::{key_rel, path_key};
use std::collections::HashMap;

pub(super) struct Occ {
    /// Tag node id (or, in a `FileParsed`, index into its `tags`).
    pub tag: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub tag_line: u32,
    pub tag_col: u32,
    /// Index into the file's excerpts.
    pub excerpt: u32,
    pub source: TagSource,
}

/// One Markdown file of the vault, with or without tags.
#[derive(Default)]
pub(super) struct FileData {
    /// Absolute path.
    pub path: String,
    /// Path relative to the vault root, `/`-separated (empty without a root).
    pub rel: String,
    /// (size, mtime in ms) of the content that was analysed; `None` when
    /// unknown, which forces a re-read at the next reconcile.
    pub meta: Option<(u64, u64)>,
    /// False once the file has been removed (ids are never reused).
    pub present: bool,
    /// Could not be read or is not UTF-8.
    pub unreadable: bool,
    pub occs: Vec<Occ>,
    pub excerpts: Vec<Box<str>>,
}

pub(super) struct TagNode {
    /// Spelling of the first occurrence.
    pub display: String,
    /// Lowercase comparison key.
    pub key: String,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    /// Occurrences of this tag and of its descendants.
    pub total: u32,
    /// (file, occurrence index) of the occurrences of this exact tag.
    pub postings: Vec<(u32, u32)>,
}

/// Result of analysing one file, detached from the file's text.
#[derive(Default)]
pub(super) struct FileParsed {
    pub tags: Vec<String>,
    pub occs: Vec<Occ>,
    pub excerpts: Vec<Box<str>>,
}

impl FileParsed {
    pub fn from_parsed(parsed: Parsed<'_>) -> Self {
        let mut tags: Vec<String> = Vec::new();
        let mut occs = Vec::with_capacity(parsed.occs.len());
        for o in &parsed.occs {
            let local = match tags.iter().position(|t| t == o.tag) {
                Some(i) => i,
                None => {
                    tags.push(o.tag.to_string());
                    tags.len() - 1
                }
            };
            occs.push(Occ {
                tag: local as u32,
                start_line: o.start_line,
                end_line: o.end_line,
                tag_line: o.tag_line,
                tag_col: o.tag_col,
                excerpt: o.excerpt,
                source: o.source,
            });
        }
        let excerpts = parsed.excerpts.into_iter().map(String::into_boxed_str).collect();
        Self { tags, occs, excerpts }
    }
}

#[derive(Default)]
pub(super) struct Inner {
    /// Vault root (display form) and its comparison key; `None` until a
    /// vault is built or loaded.
    pub root: Option<(String, String)>,
    pub files: Vec<FileData>,
    pub by_path: HashMap<String, u32>,
    pub tags: Vec<TagNode>,
    pub by_key: HashMap<String, u32>,
    pub occurrences: usize,
    /// Incremented by every change.
    pub generation: u64,
    /// Generation that is on disk, if any.
    pub saved_generation: Option<u64>,
    /// Identity of this content; changes whenever the content is replaced
    /// wholesale (build, load, clear), so that work prepared against one
    /// content is never applied to another.
    pub epoch: u64,
}

pub(super) fn has_md_extension(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() > 3 && b[b.len() - 3..].eq_ignore_ascii_case(b".md")
}

impl Inner {
    pub fn with_root(root: &str) -> Self {
        Self { root: Some((root.to_string(), path_key(root))), ..Self::default() }
    }

    pub fn root_key(&self) -> Option<&str> {
        self.root.as_ref().map(|(_, key)| key.as_str())
    }

    pub fn intern(&mut self, tag: &str) -> u32 {
        let key = tag.to_lowercase();
        if let Some(&id) = self.by_key.get(&key) {
            return id;
        }
        let parent = tag.rfind('/').map(|pos| self.intern(&tag[..pos]));
        let id = self.tags.len() as u32;
        self.tags.push(TagNode {
            display: tag.to_string(),
            key: key.clone(),
            parent,
            children: Vec::new(),
            total: 0,
            postings: Vec::new(),
        });
        if let Some(p) = parent {
            self.tags[p as usize].children.push(id);
        }
        self.by_key.insert(key, id);
        id
    }

    fn bump(&mut self, tag: u32, delta: i64) {
        let mut cur = Some(tag);
        while let Some(id) = cur {
            let node = &mut self.tags[id as usize];
            node.total = (i64::from(node.total) + delta).max(0) as u32;
            cur = node.parent;
        }
    }

    /// Id of the entry for `path`, created (as present) if needed.
    fn file_id(&mut self, path: &str, rel: &str) -> u32 {
        let key = path_key(path);
        if let Some(&id) = self.by_path.get(&key) {
            self.files[id as usize].present = true;
            return id;
        }
        let id = self.files.len() as u32;
        self.files.push(FileData {
            path: path.to_string(),
            rel: rel.to_string(),
            present: true,
            ..FileData::default()
        });
        self.by_path.insert(key, id);
        id
    }

    fn clear_occs(&mut self, file: u32) {
        let occs = std::mem::take(&mut self.files[file as usize].occs);
        self.files[file as usize].excerpts = Vec::new();
        if occs.is_empty() {
            return;
        }
        let mut touched: Vec<u32> = Vec::new();
        for o in &occs {
            self.bump(o.tag, -1);
            if !touched.contains(&o.tag) {
                touched.push(o.tag);
            }
        }
        for tag in touched {
            self.tags[tag as usize].postings.retain(|&(f, _)| f != file);
        }
        self.occurrences -= occs.len();
    }

    /// Attaches occurrences whose `tag` is already a node id.
    pub fn attach(&mut self, file: u32, occs: Vec<Occ>, excerpts: Vec<Box<str>>) {
        for (i, o) in occs.iter().enumerate() {
            self.tags[o.tag as usize].postings.push((file, i as u32));
            self.bump(o.tag, 1);
        }
        self.occurrences += occs.len();
        let data = &mut self.files[file as usize];
        data.occs = occs;
        data.excerpts = excerpts;
    }

    /// Records the analysed content of a file, replacing what was known.
    pub fn put_file(
        &mut self,
        path: &str,
        rel: &str,
        meta: Option<(u64, u64)>,
        unreadable: bool,
        parsed: FileParsed,
    ) {
        let file = self.file_id(path, rel);
        self.clear_occs(file);
        let ids: Vec<u32> = parsed.tags.iter().map(|t| self.intern(t)).collect();
        let mut occs = parsed.occs;
        for o in &mut occs {
            o.tag = ids[o.tag as usize];
        }
        self.attach(file, occs, parsed.excerpts);
        let data = &mut self.files[file as usize];
        data.meta = meta;
        data.unreadable = unreadable;
        self.generation += 1;
    }

    /// Moves a file out of `old` into `self`, keeping its metadata. Tag ids
    /// are translated through `tag_map` (old id -> new id, filled lazily).
    pub fn adopt_file(&mut self, old: &mut Inner, old_file: u32, path: &str, rel: &str, tag_map: &mut [u32]) {
        let src = &mut old.files[old_file as usize];
        let mut occs = std::mem::take(&mut src.occs);
        let excerpts = std::mem::take(&mut src.excerpts);
        let (meta, unreadable) = (src.meta, src.unreadable);
        for o in &mut occs {
            let slot = &mut tag_map[o.tag as usize];
            if *slot == u32::MAX {
                *slot = self.intern(&old.tags[o.tag as usize].display);
            }
            o.tag = *slot;
        }
        let file = self.file_id(path, rel);
        self.attach(file, occs, excerpts);
        let data = &mut self.files[file as usize];
        data.meta = meta;
        data.unreadable = unreadable;
    }

    /// Forgets a deleted file. Returns whether it was known.
    pub fn drop_file(&mut self, path: &str) -> bool {
        let Some(&file) = self.by_path.get(&path_key(path)) else { return false };
        if !self.files[file as usize].present {
            return false;
        }
        self.clear_occs(file);
        let data = &mut self.files[file as usize];
        data.present = false;
        data.meta = None;
        data.unreadable = false;
        self.generation += 1;
        true
    }

    pub fn live_tags(&self) -> usize {
        self.tags.iter().filter(|t| t.total > 0).count()
    }

    /// Markdown files known to the index (with or without tags).
    pub fn md_files(&self) -> usize {
        self.files.iter().filter(|f| f.present).count()
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_generation != Some(self.generation)
    }

    /// Whether `path` is a Markdown file this index should hold: inside the
    /// vault, not below a hidden folder.
    pub fn accepts(&self, path: &str) -> bool {
        if !has_md_extension(path) {
            return false;
        }
        let Some(root_key) = self.root_key() else { return true };
        match key_rel(&path_key(path), root_key, cfg!(windows)) {
            Some(rel) => !rel.is_empty() && !rel.split(['/', '\\']).any(|c| c.starts_with('.')),
            None => false,
        }
    }

    /// Path of `path` relative to the root, `/`-separated.
    pub fn rel_of(&self, path: &str) -> String {
        let Some((root, root_key)) = &self.root else { return String::new() };
        let rel = match path.strip_prefix(root.as_str()) {
            Some(rest) => rest.trim_start_matches(['/', '\\']).to_string(),
            // Same folder spelled differently (case, separators): fall back
            // to the comparison key, which still resolves on Windows.
            None => key_rel(&path_key(path), root_key, cfg!(windows)).unwrap_or_default().to_string(),
        };
        if cfg!(windows) {
            rel.replace('\\', "/")
        } else {
            rel
        }
    }
}
