//! tantivy-backed search engine. Everything specific to tantivy is here.
//!
//! One document per note: exact path, folded path (for `path:`), file name
//! and body (tokenised with `fold::Tokens`, body stored LZ4-compressed),
//! tags with their ancestors (for `tag:`), and size + modification time as
//! fast fields. The last two form the manifest: what the index holds for
//! each note. The caller compares it with the disk exactly as it does for
//! the main index, so the two stores never need to be written atomically
//! together: after a crash, whatever is stale on either side is simply read
//! again.
//!
//! Segments are memory-mapped; nothing is loaded up front. The writer (three
//! threads, 48 MB) is created on first use and holds the directory lock
//! until the engine is dropped.

use super::fold::{fold_str, Tokens};
use super::highlight::{find_matches, name_matches};
use super::query::{Clause, ParsedQuery};
use super::SearchMatch;
use crate::paths::{fnv1a64, path_key};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    BooleanQuery, BoostQuery, EnableScoring, Explanation, Occur, PhrasePrefixQuery, PhraseQuery, Query,
    RegexQuery, Scorer, TermQuery, Weight,
};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, FAST, STORED, STRING,
};
use tantivy::store::Compressor;
use tantivy::tokenizer::{TextAnalyzer, Token, TokenStream, Tokenizer};
use tantivy::{
    DocId, DocSet, Index, IndexReader, IndexSettings, IndexWriter, ReloadPolicy, Score, SegmentReader,
    TantivyDocument, Term,
};

const TOKENIZER: &str = "rocksy";
const WRITER_THREADS: usize = 3;
const WRITER_MEMORY: usize = 48_000_000;
/// Weight of a match in the file name relative to the body.
const NAME_BOOST: f32 = 6.0;
/// Weight of the exact word relative to words it is only a prefix of.
const EXACT_BOOST: f32 = 2.0;
const COMPLETE: &str = "complete";
const PARTIAL: &str = "partial";

/// Folder of the full-text index of `root` inside `data_dir`.
pub(crate) fn search_dir(data_dir: &Path, root: &str) -> PathBuf {
    let hash = fnv1a64(path_key(root).as_bytes());
    data_dir.join("search").join(format!("{hash:016x}"))
}

// ---------------------------------------------------------------------------
// Tokenizer adapter
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FoldTokenizer;

struct FoldStream<'a> {
    tokens: Tokens<'a>,
    token: Token,
    position: usize,
}

impl Tokenizer for FoldTokenizer {
    type TokenStream<'a> = FoldStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> FoldStream<'a> {
        FoldStream { tokens: Tokens::new(text), token: Token::default(), position: 0 }
    }
}

impl TokenStream for FoldStream<'_> {
    fn advance(&mut self) -> bool {
        loop {
            let Some(t) = self.tokens.next_token() else { return false };
            let position = self.position;
            self.position += 1;
            if t.folded.is_empty() {
                continue; // too long: keeps its position, is not indexed
            }
            self.token.text.clear();
            self.token.text.push_str(t.folded);
            self.token.offset_from = t.start;
            self.token.offset_to = t.end;
            self.token.position = position;
            self.token.position_length = 1;
            return true;
        }
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

// ---------------------------------------------------------------------------
// Phrase scorer guard
// ---------------------------------------------------------------------------

/// Wraps a phrase query so that its scorer uses the default `seek_danger`.
///
/// tantivy 0.27's phrase scorer overrides `seek_danger` and asserts (debug
/// builds) that the target is not below its current document. A phrase
/// scorer starts on its first match, so the assertion fires as soon as it is
/// a non-driving member of an intersection whose first candidate is lower.
/// The default implementation handles that case (it answers with a lower
/// bound), and this wrapper simply does not forward the override.
#[derive(Debug)]
struct Guarded(Box<dyn Query>);

impl Clone for Guarded {
    fn clone(&self) -> Self {
        Guarded(self.0.box_clone())
    }
}

impl Query for Guarded {
    fn weight(&self, enable_scoring: EnableScoring<'_>) -> tantivy::Result<Box<dyn Weight>> {
        Ok(Box::new(GuardedWeight(self.0.weight(enable_scoring)?)))
    }

    fn query_terms<'a>(&'a self, visitor: &mut dyn FnMut(&'a Term, bool)) {
        self.0.query_terms(visitor)
    }
}

struct GuardedWeight(Box<dyn Weight>);

impl Weight for GuardedWeight {
    fn scorer(&self, reader: &SegmentReader, boost: Score) -> tantivy::Result<Box<dyn Scorer>> {
        Ok(Box::new(GuardedScorer(self.0.scorer(reader, boost)?)))
    }

    fn explain(&self, reader: &SegmentReader, doc: DocId) -> tantivy::Result<Explanation> {
        self.0.explain(reader, doc)
    }
}

struct GuardedScorer(Box<dyn Scorer>);

impl DocSet for GuardedScorer {
    fn advance(&mut self) -> DocId {
        self.0.advance()
    }

    fn seek(&mut self, target: DocId) -> DocId {
        // `seek` has the same precondition; stay where we are if already past.
        if self.0.doc() >= target {
            self.0.doc()
        } else {
            self.0.seek(target)
        }
    }

    fn doc(&self) -> DocId {
        self.0.doc()
    }

    fn size_hint(&self) -> u32 {
        self.0.size_hint()
    }
}

impl Scorer for GuardedScorer {
    fn score(&mut self) -> Score {
        self.0.score()
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Fields {
    path: Field,
    path_fold: Field,
    name: Field,
    body: Field,
    tags: Field,
    size: Field,
    mtime: Field,
}

const FIELD_NAMES: [&str; 7] = ["path", "path_fold", "name", "body", "tags", "size", "mtime"];

fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let text = TextFieldIndexing::default()
        .set_tokenizer(TOKENIZER)
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    let fields = Fields {
        path: builder.add_text_field("path", STRING | STORED | FAST),
        path_fold: builder.add_text_field("path_fold", STRING),
        name: builder.add_text_field("name", TextOptions::default().set_indexing_options(text.clone())),
        body: builder.add_text_field("body", TextOptions::default().set_indexing_options(text).set_stored()),
        tags: builder.add_text_field("tags", STRING),
        size: builder.add_u64_field("size", FAST),
        mtime: builder.add_u64_field("mtime", FAST),
    };
    (builder.build(), fields)
}

/// A note to index.
pub(crate) struct SearchDoc<'a> {
    /// Path relative to the vault root, `/`-separated.
    pub rel: &'a str,
    pub text: &'a str,
    pub size: u64,
    pub mtime_ms: u64,
    /// Lowercased tags with their ancestors.
    pub tags: &'a [String],
}

pub(crate) struct RawHit {
    pub rel: String,
    pub score: f32,
    pub name_match: bool,
    pub match_count: usize,
    pub matches: Vec<SearchMatch>,
}

pub(crate) struct SearchEngine {
    dir: PathBuf,
    index: Index,
    reader: IndexReader,
    fields: Fields,
    writer: Mutex<Option<IndexWriter>>,
    /// What the index holds, uncommitted changes included: path -> (size, mtime).
    manifest: Mutex<HashMap<String, (u64, u64)>>,
    /// Changes not committed yet.
    dirty: AtomicBool,
    /// A full pass over the vault has been committed at least once.
    complete: AtomicBool,
}

fn file_stem(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rfind('.') {
        Some(pos) if pos > 0 => &name[..pos],
        _ => name,
    }
}

impl SearchEngine {
    /// Opens the index in `dir`. With `create`, a missing, unreadable or
    /// incompatible index is replaced by an empty one; without, `None`.
    pub(crate) fn open(dir: &Path, create: bool) -> Option<SearchEngine> {
        let (schema, fields) = schema();
        let existing = if dir.join("meta.json").is_file() {
            Index::open_in_dir(dir).ok().filter(|index| {
                let found = index.schema();
                found.fields().count() == FIELD_NAMES.len()
                    && FIELD_NAMES.iter().all(|name| found.get_field(name).is_ok())
            })
        } else {
            None
        };
        let index = match existing {
            Some(index) => index,
            None if create => {
                // Start from a clean folder: leftovers of a damaged index
                // would otherwise be picked up again.
                let _ = std::fs::remove_dir_all(dir);
                std::fs::create_dir_all(dir).ok()?;
                let settings = IndexSettings { docstore_compression: Compressor::Lz4, ..IndexSettings::default() };
                Index::builder().schema(schema).settings(settings).create_in_dir(dir).ok()?
            }
            None => return None,
        };
        index.tokenizers().register(TOKENIZER, TextAnalyzer::builder(FoldTokenizer).build());
        let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into().ok()?;
        let complete = index.load_metas().ok()?.payload.as_deref() == Some(COMPLETE);
        let engine = SearchEngine {
            dir: dir.to_path_buf(),
            index,
            reader,
            fields,
            writer: Mutex::new(None),
            manifest: Mutex::new(HashMap::new()),
            dirty: AtomicBool::new(false),
            complete: AtomicBool::new(complete),
        };
        match engine.load_manifest() {
            Some(manifest) => *engine.manifest.lock().unwrap_or_else(|e| e.into_inner()) = manifest,
            None if create => {
                // Unreadable segments: start over.
                drop(engine);
                let _ = std::fs::remove_dir_all(dir);
                return SearchEngine::open(dir, true);
            }
            None => return None,
        }
        Some(engine)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reads (path, size, mtime) of every committed document from the fast
    /// fields: no stored document is decompressed.
    fn load_manifest(&self) -> Option<HashMap<String, (u64, u64)>> {
        let searcher = self.reader.searcher();
        let mut manifest = HashMap::with_capacity(searcher.num_docs() as usize);
        let mut path = String::new();
        for segment in searcher.segment_readers() {
            let fast = segment.fast_fields();
            let Some(paths) = fast.str("path").ok()? else { continue };
            let sizes = fast.u64("size").ok()?;
            let mtimes = fast.u64("mtime").ok()?;
            for doc in segment.doc_ids_alive() {
                let Some(ord) = paths.ords().first(doc) else { continue };
                path.clear();
                if !paths.ord_to_str(ord, &mut path).ok()? {
                    continue;
                }
                let meta = (sizes.first(doc).unwrap_or(u64::MAX), mtimes.first(doc).unwrap_or(0));
                // The same path twice should not happen; if it does, make
                // sure the note is indexed again.
                if manifest.insert(path.clone(), meta).is_some() {
                    manifest.insert(path.clone(), (u64::MAX, 0));
                }
            }
        }
        Some(manifest)
    }

    fn with_writer<R>(&self, f: impl FnOnce(&mut IndexWriter) -> tantivy::Result<R>) -> Option<R> {
        let mut guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            // Fails if another writer holds the folder (another process or
            // another index object on the same data directory).
            *guard = self.index.writer_with_num_threads(WRITER_THREADS, WRITER_MEMORY).ok();
        }
        f(guard.as_mut()?).ok()
    }

    /// Whether this engine can modify the index.
    pub(crate) fn writable(&self) -> bool {
        self.with_writer(|_| Ok(())).is_some()
    }

    /// (size, mtime) of the indexed content of a note, if it is indexed.
    pub(crate) fn meta(&self, rel: &str) -> Option<(u64, u64)> {
        self.manifest.lock().unwrap_or_else(|e| e.into_inner()).get(rel).copied()
    }

    /// Paths of every indexed note.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.manifest.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect()
    }

    /// Adds or replaces a note. Visible to searches after the next commit.
    pub(crate) fn upsert(&self, doc: &SearchDoc<'_>) -> bool {
        let f = self.fields;
        let mut document = TantivyDocument::default();
        document.add_text(f.path, doc.rel);
        document.add_text(f.path_fold, fold_str(doc.rel));
        document.add_text(f.name, file_stem(doc.rel));
        document.add_text(f.body, doc.text);
        for tag in doc.tags {
            document.add_text(f.tags, tag);
        }
        document.add_u64(f.size, doc.size);
        document.add_u64(f.mtime, doc.mtime_ms);
        let done = self
            .with_writer(|writer| {
                writer.delete_term(Term::from_field_text(f.path, doc.rel));
                writer.add_document(document)?;
                Ok(())
            })
            .is_some();
        if done {
            self.manifest
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(doc.rel.to_string(), (doc.size, doc.mtime_ms));
            self.dirty.store(true, Ordering::Release);
        }
        done
    }

    pub(crate) fn delete(&self, rel: &str) -> bool {
        let f = self.fields;
        let done = self
            .with_writer(|writer| {
                writer.delete_term(Term::from_field_text(f.path, rel));
                Ok(())
            })
            .is_some();
        if done {
            self.manifest.lock().unwrap_or_else(|e| e.into_inner()).remove(rel);
            self.dirty.store(true, Ordering::Release);
        }
        done
    }

    /// Makes the pending changes durable and visible. `complete` records
    /// that a full pass over the vault is finished.
    pub(crate) fn commit(&self, complete: bool) -> bool {
        let becomes_complete = complete && !self.complete.load(Ordering::Acquire);
        if !self.dirty.load(Ordering::Acquire) && !becomes_complete {
            return true;
        }
        let flag = complete || self.complete.load(Ordering::Acquire);
        let done = self
            .with_writer(|writer| {
                let mut prepared = writer.prepare_commit()?;
                prepared.set_payload(if flag { COMPLETE } else { PARTIAL });
                prepared.commit()?;
                Ok(())
            })
            .is_some();
        if done {
            self.dirty.store(false, Ordering::Release);
            self.complete.store(flag, Ordering::Release);
            let _ = self.reader.reload();
        }
        done
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    /// Committed documents.
    pub(crate) fn num_docs(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    fn term(&self, field: Field, text: &str) -> Box<dyn Query> {
        Box::new(TermQuery::new(Term::from_field_text(field, text), IndexRecordOption::WithFreqs))
    }

    /// One word or phrase on one field.
    fn clause_on(&self, field: Field, clause: &Clause) -> Option<Box<dyn Query>> {
        let terms: Vec<Term> = clause.terms.iter().map(|t| Term::from_field_text(field, t)).collect();
        let last = clause.terms.last()?;
        Some(match (terms.len(), clause.prefix) {
            (1, false) => self.term(field, last),
            (1, true) => {
                // Tokens are letters and digits only: nothing to escape.
                let prefix = RegexQuery::from_pattern(&format!("{last}.*"), field).ok()?;
                Box::new(BooleanQuery::new(vec![
                    (Occur::Should, Box::new(BoostQuery::new(self.term(field, last), EXACT_BOOST)) as Box<dyn Query>),
                    (Occur::Should, Box::new(prefix)),
                ]))
            }
            (_, false) => Box::new(Guarded(Box::new(PhraseQuery::new(terms)))),
            (_, true) => Box::new(Guarded(Box::new(PhrasePrefixQuery::new(terms)))),
        })
    }

    /// One word or phrase, in the body or (weighted more) in the file name.
    fn clause(&self, clause: &Clause) -> Option<Box<dyn Query>> {
        let body = self.clause_on(self.fields.body, clause)?;
        let name = self.clause_on(self.fields.name, clause)?;
        Some(Box::new(BooleanQuery::new(vec![
            (Occur::Should, body),
            (Occur::Should, Box::new(BoostQuery::new(name, NAME_BOOST))),
        ])))
    }

    fn build(&self, query: &ParsedQuery) -> Option<Box<dyn Query>> {
        let mut parts: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for clause in &query.must {
            parts.push((Occur::Must, self.clause(clause)?));
        }
        for clause in &query.must_not {
            parts.push((Occur::MustNot, self.clause(clause)?));
        }
        for (negated, text) in &query.paths {
            let mut pattern = String::with_capacity(text.len() + 8);
            pattern.push_str(".*");
            for c in text.chars() {
                if "\\.+*?()|[]{}^$#&-~\"<>@".contains(c) {
                    pattern.push('\\');
                }
                pattern.push(c);
            }
            pattern.push_str(".*");
            let filter = RegexQuery::from_pattern(&pattern, self.fields.path_fold).ok()?;
            parts.push((if *negated { Occur::MustNot } else { Occur::Must }, Box::new(filter)));
        }
        for (negated, tag) in &query.tags {
            let filter = self.term(self.fields.tags, tag);
            parts.push((if *negated { Occur::MustNot } else { Occur::Must }, filter));
        }
        Some(Box::new(BooleanQuery::new(parts)))
    }

    /// Runs a query: number of matching notes and the `limit` best ones with
    /// their excerpts, taken from the stored text.
    pub(crate) fn search(&self, query: &ParsedQuery, limit: usize) -> (usize, Vec<RawHit>) {
        if query.is_empty() {
            return (0, Vec::new());
        }
        let Some(built) = self.build(query) else { return (0, Vec::new()) };
        let searcher = self.reader.searcher();
        let collector = (TopDocs::with_limit(limit.max(1)).order_by_score(), Count);
        let Ok((top, total)) = searcher.search(&built, &collector) else { return (0, Vec::new()) };
        let mut hits = Vec::with_capacity(top.len().min(limit));
        for (score, address) in top.into_iter().take(limit) {
            let Ok(document) = searcher.doc::<TantivyDocument>(address) else { continue };
            let Some(rel) = document.get_first(self.fields.path).and_then(|v| v.as_str()) else { continue };
            let body = document.get_first(self.fields.body).and_then(|v| v.as_str()).unwrap_or("");
            let (match_count, matches) = find_matches(body, query);
            hits.push(RawHit {
                rel: rel.to_string(),
                score,
                name_match: name_matches(file_stem(rel), query),
                match_count,
                matches,
            });
        }
        (total, hits)
    }
}
