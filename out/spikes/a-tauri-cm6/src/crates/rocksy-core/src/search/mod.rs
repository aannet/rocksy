//! Full-text search of a vault.
//!
//! The engine is hidden behind [`SearchEngine`] so that it can be replaced:
//! the rest of the crate only sees documents going in (`upsert`, `delete`,
//! `commit`), a manifest of what is indexed (path -> size, mtime) and
//! [`SearchEngine::search`]. Tokenisation, the query language and excerpts
//! are engine-independent (`fold`, `query`, `highlight`).
//!
//! The current engine is tantivy: an inverted index on disk, memory-mapped,
//! with the text of every note stored compressed so that excerpts never need
//! the vault.

mod engine;
mod fold;
mod highlight;
mod query;

use serde::{Deserialize, Serialize};

pub(crate) use engine::{search_dir, SearchDoc, SearchEngine};
pub(crate) use query::parse_query;

/// One line of a file containing the query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    /// 0-based line (CodeMirror line model), in the text as `read_file`
    /// returns it.
    pub line: u32,
    /// UTF-16 column of the first occurrence on the line (a byte-order mark
    /// counts for one on line 0).
    pub col: u32,
    /// UTF-16 length of that first occurrence, in the text of the note.
    pub len: u32,
    /// The line, as a one-line excerpt of at most 240 characters.
    pub excerpt: String,
    /// Occurrences inside `excerpt` (UTF-16 units, end excluded).
    pub ranges: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Absolute path.
    pub path: String,
    pub score: f32,
    /// Every word of the query is in the file name.
    pub name_match: bool,
    /// Occurrences in the body.
    pub match_count: usize,
    /// At most three, in text order.
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub query: String,
    /// Number of matching files (can exceed `hits.len()`).
    pub total: usize,
    pub hits: Vec<SearchHit>,
    pub ms: f64,
    /// False while the full-text index is missing or still being built for
    /// the first time (the result is then empty or partial).
    pub search_ready: bool,
}
