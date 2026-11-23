//! Tags and fragments (F9).
//!
//! * [`file_tags`] analyses one text and returns its [`Fragment`]s.
//! * [`TagIndex`] is the in-memory tag library of a vault, built in the
//!   background, queried by [`TagIndex::tags_list`] and
//!   [`TagIndex::tag_fragments`], and kept current with
//!   [`TagIndex::update_file`]. It can be persisted in the application data
//!   directory and reloaded without reading any note.
//!
//! Line numbers are 0-based and follow CodeMirror's line model (`\n`, `\r\n`
//! and a lone `\r` each end a line). Columns are UTF-16 code units counted in
//! the text exactly as given (a leading byte-order mark counts for one).

mod frontmatter;
mod index;
mod parse;
mod persist;
mod store;
#[cfg(test)]
mod persist_tests;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use index::{BuildOptions, CancelToken, TagIndex};
pub use persist::tags_index_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagSource {
    Inline,
    Frontmatter,
}

/// One occurrence of a tag together with the block that contains it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fragment {
    /// Absolute path of the file.
    pub path: String,
    /// Tag without `#`.
    pub tag: String,
    pub source: TagSource,
    /// First line of the enclosing block (0-based, inclusive).
    pub start_line: u32,
    /// Last line of the enclosing block (0-based, inclusive).
    pub end_line: u32,
    /// Line of the tag.
    pub tag_line: u32,
    /// Column of the tag in UTF-16 code units: position of the `#` for an
    /// inline tag, of the first character of the value for a frontmatter tag.
    pub tag_col: u32,
    /// Single-line excerpt of the block, at most 240 characters.
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    /// Tag without `#`, spelled as in its first occurrence.
    pub tag: String,
    /// Occurrences of the tag and of all its descendants (`a` counts `a/b`).
    pub count: u32,
}

/// Payload of the `tags-progress` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagsProgress {
    pub done: usize,
    pub total: usize,
}

/// Payload of the `tags-ready` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagsReady {
    /// Markdown files indexed.
    pub files: usize,
    /// Distinct tags (implicit parents included).
    pub tags: usize,
    pub occurrences: usize,
    pub ms: u64,
}

/// Result of [`TagIndex::reconcile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    /// Markdown files in the vault.
    pub files: usize,
    /// Distinct tags (implicit parents included).
    pub tags: usize,
    pub occurrences: usize,
    pub ms: u64,
    /// Files read and analysed again (new or changed).
    pub reread: usize,
    /// Files that disappeared from the vault.
    pub removed: usize,
    /// Files reused without being read.
    pub unchanged: usize,
    /// Whether the persisted file was (re)written.
    pub persisted: bool,
}

/// Current content of the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagStats {
    /// Files holding at least one tag.
    pub files: usize,
    pub tags: usize,
    pub occurrences: usize,
    /// Files skipped by the last build (unreadable or not UTF-8).
    pub skipped: usize,
}

/// Fragments of a text held in memory (a file outside any vault, or unsaved
/// content). `path` is only copied into the result.
pub fn file_tags(path: &str, text: &str) -> Vec<Fragment> {
    let parsed = parse::parse(text);
    parsed
        .occs
        .iter()
        .map(|o| Fragment {
            path: path.to_string(),
            tag: o.tag.to_string(),
            source: o.source,
            start_line: o.start_line,
            end_line: o.end_line,
            tag_line: o.tag_line,
            tag_col: o.tag_col,
            excerpt: parsed.excerpts[o.excerpt as usize].clone(),
        })
        .collect()
}
