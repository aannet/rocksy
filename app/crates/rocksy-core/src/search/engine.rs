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
//!
//! tantivy panics (assertions) on some damaged files instead of returning an
//! error. Every call into it that touches the files is wrapped in
//! `catch_unwind`: no panic leaves this module. An index that cannot be
//! opened is deleted and rebuilt; one that fails later is marked as broken
//! and rebuilt by the next reconcile.
//!
//! Changes are handed to the writer at once and committed later (a commit
//! costs tens of milliseconds and must not be paid by `save_file`). The
//! manifest counts a change as indexed as soon as the writer has it, so a
//! writer must never be dropped with uncommitted changes without the
//! manifest being read again from the committed index (`with_writer`).

use super::fold::{fold_str, Tokens};
use super::highlight::{find_matches, name_matches};
use super::query::{Clause, ParsedQuery};
use super::SearchMatch;
use crate::paths::{fnv1a64, path_key};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
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
/// Tries (and pause between them) to empty the folder of an index that is
/// being replaced.
const REMOVE_ATTEMPTS: u32 = 4;
const REMOVE_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);
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
    /// Lock order: `writer`, then `manifest` or `pending_since`.
    writer: Mutex<Option<IndexWriter>>,
    /// What the index holds, uncommitted changes included: path -> (size, mtime).
    manifest: Mutex<HashMap<String, (u64, u64)>>,
    /// Changes not committed yet. Only written with `writer` held, so that a
    /// change made during a commit is never taken for committed.
    dirty: AtomicBool,
    /// When the oldest uncommitted change was made.
    pending_since: Mutex<Option<Instant>>,
    /// A full pass over the vault has been committed at least once.
    complete: AtomicBool,
    /// tantivy panicked or failed on the files of this index: it must be
    /// deleted and built again (see `TagIndex::open_engine`).
    broken: AtomicBool,
    /// The checksums of the files have been verified (or the index was
    /// created empty by this process).
    verified: AtomicBool,
    /// Why the writer last failed (diagnostics).
    last_error: Mutex<Option<String>>,
}

fn file_stem(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rfind('.') {
        Some(pos) if pos > 0 => &name[..pos],
        _ => name,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f`; a panic becomes `None`. tantivy asserts on the content of its
/// files, so a damaged index panics instead of returning an error: every
/// call that reads or writes the index goes through here.
fn guarded<R>(f: impl FnOnce() -> R) -> Option<R> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

impl SearchEngine {
    /// Opens the index in `dir`. With `create`, a missing, unreadable,
    /// damaged or incompatible index is replaced by an empty one; without,
    /// `None`. Never panics, and tries each of the two ways at most once.
    pub(crate) fn open(dir: &Path, create: bool) -> Option<SearchEngine> {
        if let Some(engine) = Self::open_existing(dir) {
            return Some(engine);
        }
        if create {
            Self::recreate(dir)
        } else {
            None
        }
    }

    /// The index as it is on disk, if every segment can be opened and its
    /// manifest read.
    fn open_existing(dir: &Path) -> Option<SearchEngine> {
        if !dir.join("meta.json").is_file() {
            return None;
        }
        guarded(|| {
            let index = Index::open_in_dir(dir).ok()?;
            let found = index.schema();
            let compatible = found.fields().count() == FIELD_NAMES.len()
                && FIELD_NAMES.iter().all(|name| found.get_field(name).is_ok());
            if !compatible {
                return None;
            }
            Self::from_index(dir, index)
        })
        .flatten()
    }

    /// Deletes whatever is in `dir` and starts an empty index there. `None`
    /// if the folder cannot be emptied (files held by another process) or
    /// written.
    pub(crate) fn recreate(dir: &Path) -> Option<SearchEngine> {
        // Start from a clean folder: leftovers of a damaged index would
        // otherwise be picked up again. A file may still be held for a
        // moment by a search that was running on the previous engine
        // (Windows refuses to delete a mapped file): a few tries, no more.
        for attempt in 0..REMOVE_ATTEMPTS {
            if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
                break;
            }
            if attempt + 1 < REMOVE_ATTEMPTS {
                std::thread::sleep(REMOVE_PAUSE);
            }
        }
        std::fs::create_dir_all(dir).ok()?;
        guarded(|| {
            let (schema, _) = schema();
            let settings = IndexSettings { docstore_compression: Compressor::Lz4, ..IndexSettings::default() };
            let index = Index::builder().schema(schema).settings(settings).create_in_dir(dir).ok()?;
            let engine = Self::from_index(dir, index)?;
            engine.verified.store(true, Ordering::Release);
            Some(engine)
        })
        .flatten()
    }

    fn from_index(dir: &Path, index: Index) -> Option<SearchEngine> {
        let (_, fields) = schema();
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
            pending_since: Mutex::new(None),
            complete: AtomicBool::new(complete),
            broken: AtomicBool::new(false),
            verified: AtomicBool::new(false),
            last_error: Mutex::new(None),
        };
        let manifest = engine.load_manifest()?;
        *lock(&engine.manifest) = manifest;
        Some(engine)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Verifies, once, the checksum tantivy stores at the end of each file
    /// of the committed index. Some damage neither prevents the index from
    /// opening nor makes a query fail: a flipped byte in a term dictionary
    /// simply gives wrong results. Returns `false`, and marks the engine as
    /// broken, if a file does not match its checksum.
    ///
    /// Reads the whole index (tens of megabytes on a large vault): for a
    /// background thread, before the writer is created (a merge could
    /// delete a file while it is being read).
    pub(crate) fn verify(&self) -> bool {
        if !self.verified.swap(true, Ordering::AcqRel) {
            let sound = guarded(|| self.index.validate_checksum().is_ok_and(|damaged| damaged.is_empty()));
            if sound != Some(true) {
                self.broken.store(true, Ordering::Release);
            }
        }
        !self.is_broken()
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

    /// Runs `f` on the writer, created on first use. `None` if there is no
    /// writer to be had, or if `f` failed or panicked.
    ///
    /// A writer that failed is never kept: tantivy kills it for good after
    /// an error in one of its threads, and every later call would fail
    /// silently for the rest of the session. It is dropped, which loses the
    /// changes it had not committed, so the manifest is read again from what
    /// is committed: the notes concerned no longer look indexed and the next
    /// reconcile reads them again. The next call creates a new writer.
    fn with_writer<R>(&self, f: impl FnOnce(&mut IndexWriter) -> tantivy::Result<R>) -> Option<R> {
        let mut guard = lock(&self.writer);
        if guard.is_none() {
            // Fails if another writer holds the folder (another process or
            // another index object on the same data directory).
            *guard = guarded(|| self.index.writer_with_num_threads(WRITER_THREADS, WRITER_MEMORY).ok()).flatten();
        }
        let Some(writer) = guard.as_mut() else {
            *lock(&self.last_error) = Some("no writer (index folder held by another writer?)".to_string());
            return None;
        };
        match guarded(|| f(writer)) {
            Some(Ok(value)) => Some(value),
            outcome => {
                let mut reason = match &outcome {
                    Some(Err(e)) => e.to_string(),
                    _ => "panic in the full-text engine".to_string(),
                };
                // When one of its threads failed, the writer only says that
                // it "was killed"; the error of that thread comes out when
                // the threads are joined, which preparing a commit does
                // (nothing is committed: the writer is dropped right after).
                if let Some(writer) = guard.as_mut() {
                    let cause = guarded(|| writer.prepare_commit().err().map(|e| e.to_string())).flatten();
                    if let Some(cause) = cause.filter(|cause| *cause != reason) {
                        reason = format!("{reason} Cause: {cause}");
                    }
                }
                *lock(&self.last_error) = Some(reason);
                if outcome.is_none() {
                    self.broken.store(true, Ordering::Release);
                }
                // Dropping a writer waits for its threads; a panic there
                // must not escape either.
                let failed = guard.take();
                let _ = guarded(move || drop(failed));
                self.forget_uncommitted();
                None
            }
        }
    }

    /// After the writer was dropped with changes it had not committed: the
    /// manifest goes back to what the committed index holds. Called with
    /// `writer` held.
    fn forget_uncommitted(&self) {
        let committed = guarded(|| {
            let _ = self.reader.reload();
            self.load_manifest()
        })
        .flatten();
        let mut manifest = lock(&self.manifest);
        match committed {
            Some(committed) => *manifest = committed,
            None => {
                // Unreadable: nothing is known to be indexed any more.
                manifest.clear();
                self.broken.store(true, Ordering::Release);
            }
        }
        self.dirty.store(false, Ordering::Release);
        *lock(&self.pending_since) = None;
    }

    /// Called with `writer` held, after a change was handed to it.
    fn mark_pending(&self) {
        self.dirty.store(true, Ordering::Release);
        lock(&self.pending_since).get_or_insert_with(Instant::now);
    }

    /// Why a write to the index last failed, if one did (diagnostics);
    /// forgotten once read.
    pub(crate) fn take_last_error(&self) -> Option<String> {
        lock(&self.last_error).take()
    }

    /// Whether this engine can modify the index.
    pub(crate) fn writable(&self) -> bool {
        self.with_writer(|_| Ok(())).is_some()
    }

    /// (size, mtime) of the indexed content of a note, if it is indexed.
    pub(crate) fn meta(&self, rel: &str) -> Option<(u64, u64)> {
        lock(&self.manifest).get(rel).copied()
    }

    /// Paths of every indexed note.
    pub(crate) fn paths(&self) -> Vec<String> {
        lock(&self.manifest).keys().cloned().collect()
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
        self.with_writer(|writer| {
            writer.delete_term(Term::from_field_text(f.path, doc.rel));
            writer.add_document(document)?;
            lock(&self.manifest).insert(doc.rel.to_string(), (doc.size, doc.mtime_ms));
            self.mark_pending();
            Ok(())
        })
        .is_some()
    }

    pub(crate) fn delete(&self, rel: &str) -> bool {
        let f = self.fields;
        self.with_writer(|writer| {
            writer.delete_term(Term::from_field_text(f.path, rel));
            lock(&self.manifest).remove(rel);
            self.mark_pending();
            Ok(())
        })
        .is_some()
    }

    /// Makes the pending changes durable and visible. `complete` records
    /// that a full pass over the vault is finished. Returns `false` if the
    /// commit failed: the changes are then lost for the index, and the notes
    /// concerned are read again by the next reconcile (see `with_writer`).
    pub(crate) fn commit(&self, complete: bool) -> bool {
        let becomes_complete = complete && !self.complete.load(Ordering::Acquire);
        if !self.dirty.load(Ordering::Acquire) && !becomes_complete {
            return true;
        }
        let done = self
            .with_writer(|writer| {
                let flag = complete || self.complete.load(Ordering::Acquire);
                let mut prepared = writer.prepare_commit()?;
                prepared.set_payload(if flag { COMPLETE } else { PARTIAL });
                prepared.commit()?;
                self.dirty.store(false, Ordering::Release);
                *lock(&self.pending_since) = None;
                self.complete.store(flag, Ordering::Release);
                Ok(())
            })
            .is_some();
        if done && guarded(|| self.reader.reload()).is_none() {
            self.broken.store(true, Ordering::Release);
        }
        done
    }

    /// True while changes wait for a commit.
    pub(crate) fn has_pending(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// When the oldest change that waits for a commit was made.
    pub(crate) fn pending_since(&self) -> Option<Instant> {
        *lock(&self.pending_since)
    }

    /// The files of this index made tantivy panic or fail: it answers
    /// nothing reliable any more and must be rebuilt.
    pub(crate) fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Acquire)
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
    /// their excerpts, taken from the stored text. `None` if the index could
    /// not be read (tantivy failed or panicked on a damaged file): the
    /// engine is then marked as broken.
    pub(crate) fn search(&self, query: &ParsedQuery, limit: usize) -> Option<(usize, Vec<RawHit>)> {
        if query.is_empty() {
            return Some((0, Vec::new()));
        }
        let Some(built) = self.build(query) else { return Some((0, Vec::new())) };
        let outcome = guarded(|| self.run(&*built, query, limit)).flatten();
        if outcome.is_none() {
            self.broken.store(true, Ordering::Release);
        }
        outcome
    }

    fn run(&self, built: &dyn Query, query: &ParsedQuery, limit: usize) -> Option<(usize, Vec<RawHit>)> {
        let searcher = self.reader.searcher();
        let collector = (TopDocs::with_limit(limit.max(1)).order_by_score(), Count);
        let (top, total) = searcher.search(built, &collector).ok()?;
        let mut hits = Vec::with_capacity(top.len().min(limit));
        for (score, address) in top.into_iter().take(limit) {
            // A stored document that cannot be read back is a damaged file.
            let document = searcher.doc::<TantivyDocument>(address).ok()?;
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
        Some((total, hits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::parse_query;
    use crate::testutil::TempDir;

    fn doc<'a>(rel: &'a str, text: &'a str, mtime_ms: u64) -> SearchDoc<'a> {
        SearchDoc { rel, text, size: text.len() as u64, mtime_ms, tags: &[] }
    }

    fn total(engine: &SearchEngine, query: &str) -> usize {
        engine.search(&parse_query(query), 10).unwrap().0
    }

    /// Makes the folder read-only (or writable again). `false` if that does
    /// not prevent writing (tests run as root): the test is then skipped.
    #[cfg(unix)]
    fn set_read_only(dir: &Path, read_only: bool) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if read_only { 0o555 } else { 0o755 })).unwrap();
        if !read_only {
            return true;
        }
        let probe = dir.join("probe");
        let blocked = std::fs::write(&probe, b"x").is_err();
        let _ = std::fs::remove_file(&probe);
        blocked
    }

    #[test]
    fn changes_wait_for_the_commit_and_the_manifest_counts_them_at_once() {
        let dir = TempDir::new("engine-pending");
        let engine = SearchEngine::open(dir.path(), true).unwrap();
        assert!(!engine.has_pending() && engine.pending_since().is_none());
        let before = Instant::now();
        assert!(engine.upsert(&doc("a.md", "un wombat", 1)));
        assert!(engine.has_pending());
        assert!(engine.pending_since().unwrap() >= before);
        assert_eq!(engine.meta("a.md"), Some((9, 1)));
        assert_eq!(total(&engine, "wombat"), 0, "not visible before the commit");
        let first = engine.pending_since();
        assert!(engine.upsert(&doc("b.md", "deux wombats", 2)));
        assert_eq!(engine.pending_since(), first, "the delay runs from the oldest change");
        assert!(engine.commit(false));
        assert!(!engine.has_pending() && engine.pending_since().is_none());
        assert_eq!(total(&engine, "wombat"), 2);
        assert!(engine.delete("a.md"));
        assert!(engine.has_pending());
        assert!(engine.commit(false));
        assert_eq!(total(&engine, "wombat"), 1);
        assert!(!engine.is_broken());
    }

    /// BUG-22. A commit that fails (here: the folder cannot be written)
    /// must not leave a dead writer behind, nor a manifest that claims the
    /// lost changes are indexed.
    #[cfg(unix)]
    #[test]
    fn a_failed_commit_drops_the_writer_and_forgets_what_it_held() {
        let dir = TempDir::new("engine-failed-commit");
        let engine = SearchEngine::open(dir.path(), true).unwrap();
        assert!(engine.upsert(&doc("kept.md", "un quokka", 1)));
        assert!(engine.commit(true));

        if !set_read_only(dir.path(), true) {
            set_read_only(dir.path(), false);
            return;
        }
        // The writer's threads fail in the background as soon as they try
        // to create a file: depending on when, the second change is refused
        // (tantivy: "an index writer was killed") or the commit fails.
        let first = engine.upsert(&doc("lost.md", "un numbat", 2));
        let second = engine.upsert(&doc("kept.md", "un quokka modifié", 3));
        let committed = engine.commit(false);
        assert!(first, "handing a change to a new writer does not touch the disk");
        assert!(!(second && committed), "nothing can have been written");
        set_read_only(dir.path(), false);

        // The manifest is back to what is committed: the two notes will be
        // read again by the next reconcile.
        assert_eq!(engine.meta("lost.md"), None);
        assert_eq!(engine.meta("kept.md"), Some((9, 1)));
        assert!(!engine.has_pending());
        assert!(engine.is_complete(), "what was committed stays usable");
        assert_eq!(total(&engine, "quokka"), 1);

        // And the engine works again at once, with a new writer.
        assert!(engine.upsert(&doc("lost.md", "un numbat", 2)), "the failed writer was kept");
        assert!(engine.commit(false));
        assert_eq!(total(&engine, "numbat"), 1);
        assert_eq!(engine.meta("lost.md"), Some((9, 2)));
    }

    /// BUG-23. An index that can be neither read nor replaced: `open`
    /// answers `None`; it used to call itself again without limit.
    #[cfg(unix)]
    #[test]
    fn open_gives_up_when_a_damaged_index_cannot_be_replaced() {
        let dir = TempDir::new("engine-stuck");
        let index_dir = dir.path().join("idx");
        {
            let engine = SearchEngine::open(&index_dir, true).unwrap();
            for i in 0..20 {
                assert!(engine.upsert(&doc(&format!("n{i}.md"), "un texte quelconque", i)));
            }
            assert!(engine.commit(true));
        }
        // Every segment file cut in half; meta.json still lists them.
        for entry in std::fs::read_dir(&index_dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name != "meta.json" && !name.starts_with('.') {
                let bytes = std::fs::read(&path).unwrap();
                std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
            }
        }
        if !set_read_only(&index_dir, true) {
            set_read_only(&index_dir, false);
            return;
        }
        assert!(SearchEngine::open(&index_dir, false).is_none());
        assert!(SearchEngine::open(&index_dir, true).is_none(), "nothing can be created in a read-only folder");
        set_read_only(&index_dir, false);
        // Once the folder can be emptied, the index is replaced.
        let engine = SearchEngine::open(&index_dir, true).unwrap();
        assert!(!engine.is_complete());
        assert!(engine.paths().is_empty());
    }
}
