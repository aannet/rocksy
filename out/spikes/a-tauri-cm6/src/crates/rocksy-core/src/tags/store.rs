//! In-memory content of the tag index: files, occurrences and the tag tree.
//!
//! Tags form a tree (`a/b/c` is a child of `a/b`); every node keeps the
//! number of occurrences of itself and of its descendants, so listing and
//! filtering never touch the fragments.

use super::parse::{Parsed, RawLink};
use super::TagSource;
use crate::paths::{key_rel, path_key};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Source of `Inner::epoch` and `Inner::links_generation` values: never the
/// same value twice in the life of the process.
static STAMP: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_stamp() -> u64 {
    STAMP.fetch_add(1, Ordering::Relaxed)
}

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
    /// Links and embeds, in document order.
    pub links: Vec<RawLink>,
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
    pub links: Vec<RawLink>,
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
        Self { tags, occs, excerpts, links: parsed.links }
    }

    /// Lowercased tags of the file together with all their ancestors
    /// (`a/b/c` gives `a/b/c`, `a/b`, `a`), without duplicates.
    pub fn tags_with_ancestors(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for tag in &self.tags {
            let mut t = tag.to_lowercase();
            loop {
                if !out.contains(&t) {
                    out.push(t.clone());
                }
                match t.rfind('/') {
                    Some(pos) => t.truncate(pos),
                    None => break,
                }
            }
        }
        out
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
    /// Relative paths of the other visible files of the vault (attachments),
    /// sorted; link targets are resolved against notes and these.
    pub others: Vec<String>,
    pub occurrences: usize,
    /// Incremented by every change.
    pub generation: u64,
    /// Incremented when what the link graph is built from changes: the
    /// targets of the links of a note, the set of notes, the other files.
    /// A save that leaves the links of the note alone does not change it,
    /// so the graph is not rebuilt.
    pub graph_generation: u64,
    /// Changes (to a value never used before) whenever anything a list of
    /// links can show changes: the above, plus the position and the line of
    /// each link. Lets a caller skip refreshing such a list.
    pub links_generation: u64,
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
        let known = self.by_path.get(&path_key(path)).is_some_and(|&id| self.files[id as usize].present);
        let file = self.file_id(path, rel);
        self.clear_occs(file);
        let ids: Vec<u32> = parsed.tags.iter().map(|t| self.intern(t)).collect();
        let mut occs = parsed.occs;
        for o in &mut occs {
            o.tag = ids[o.tag as usize];
        }
        self.attach(file, occs, parsed.excerpts);
        let data = &mut self.files[file as usize];
        // A new note changes what links resolve to, whatever it contains.
        let same_links = known && data.links == parsed.links;
        let same_targets = same_links
            || (known
                && data.links.len() == parsed.links.len()
                && data.links.iter().zip(&parsed.links).all(|(a, b)| a.markdown == b.markdown && a.target == b.target));
        data.links = parsed.links;
        data.meta = meta;
        data.unreadable = unreadable;
        self.generation += 1;
        if !same_targets {
            self.graph_generation += 1;
        }
        if !same_links {
            self.links_generation = next_stamp();
        }
    }

    /// Marks the content as another one than before (replaced wholesale, or
    /// its file ids renumbered): nothing prepared against the previous one
    /// applies any more.
    pub fn renew(&mut self) {
        self.epoch = next_stamp();
        self.links_generation = next_stamp();
    }

    /// Moves a file out of `old` into `self`, keeping its metadata. Tag ids
    /// are translated through `tag_map` (old id -> new id, filled lazily).
    pub fn adopt_file(&mut self, old: &mut Inner, old_file: u32, path: &str, rel: &str, tag_map: &mut [u32]) {
        let src = &mut old.files[old_file as usize];
        let mut occs = std::mem::take(&mut src.occs);
        let excerpts = std::mem::take(&mut src.excerpts);
        let links = std::mem::take(&mut src.links);
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
        data.links = links;
        data.meta = meta;
        data.unreadable = unreadable;
    }

    /// Replaces the list of non-Markdown files. Returns whether it changed.
    pub fn set_others(&mut self, mut others: Vec<String>) -> bool {
        others.sort_unstable();
        if others == self.others {
            return false;
        }
        self.others = others;
        self.generation += 1;
        self.graph_generation += 1;
        self.links_generation = next_stamp();
        true
    }

    /// Forgets a deleted file. Returns whether it was known.
    pub fn drop_file(&mut self, path: &str) -> bool {
        let Some(&file) = self.by_path.get(&path_key(path)) else { return false };
        if !self.files[file as usize].present {
            return false;
        }
        self.clear_occs(file);
        let data = &mut self.files[file as usize];
        data.links = Vec::new();
        data.present = false;
        data.meta = None;
        data.unreadable = false;
        self.generation += 1;
        self.graph_generation += 1;
        self.links_generation = next_stamp();
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
