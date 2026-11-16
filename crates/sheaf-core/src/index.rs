//! The tantivy index over the notes folder.
//!
//! The index is a cache: everything in it comes from the note files and it can be
//! deleted at any time. One process writes it ([`SearchIndex::open_or_create`], then
//! [`SearchIndex::writer`]); any number of processes read it
//! ([`SearchIndex::open_read_only`]). Besides the searchable text it stores every field
//! of [`NoteMeta`] and the note text, so lists, cards and snippets never open a file.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;

use jiff::{SignedDuration, Timestamp, ZonedDateTime};
use tantivy::indexer::IndexWriterOptions;
use tantivy::postings::TermInfo;
use tantivy::query::{AllQuery, BooleanQuery, EnableScoring, Query, RangeQuery, TermQuery, Weight};
use tantivy::schema::{
    FAST, Field, INDEXED, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::termdict::TermDictionary;
use tantivy::{
    DocAddress, DocId, DocSet, Index, IndexReader, IndexWriter, InvertedIndexReader, ReloadPolicy,
    Searcher, SegmentReader, TERMINATED, TantivyDocument, TantivyError, Term,
};
use unicode_normalization::UnicodeNormalization;

use crate::analysis::{self, fold};
use crate::error::{Error, Result};
use crate::model::{NoteDoc, NoteId, NoteMeta};
use crate::query::{Feature, Filter, Period};
use crate::snippet::body_without_title;

/// Bumped whenever the schema or the analysis changes. An index with another version
/// is thrown away and rebuilt from the note files.
const SCHEMA_VERSION: u32 = 1;
const VERSION_FILE: &str = "sheaf-index-version";

/// Tantivy's minimum. The writer lives in a service that has to stay small, and a
/// note folder is indexed in seconds even with this budget.
const WRITER_MEMORY_BYTES: usize = 15_000_000;

const ID: &str = "id";
const PATH: &str = "path";
const CREATED: &str = "created";
pub(crate) const MODIFIED: &str = "modified";
const TITLE_SORT: &str = "title_sort";

/// Handles of the schema's fields.
#[derive(Clone, Debug)]
pub(crate) struct Fields {
    pub id: Field,
    pub path: Field,
    pub folder: Field,
    /// Every run of segments of the folder path, folded; see [`folder_runs`].
    pub folder_key: Field,
    pub title: Field,
    pub title_prefix: Field,
    /// The whole title as one term, see [`analysis::key`].
    pub title_key: Field,
    pub title_sort: Field,
    pub headings: Field,
    pub headings_prefix: Field,
    pub tags: Field,
    /// Folded tags and their ancestors, for the `#tag` filter.
    pub tag_key: Field,
    /// Tags and their ancestors as written, for the list of tags.
    pub tag_name: Field,
    /// The text below the title line, indexed but not stored.
    pub body: Field,
    /// The whole Markdown text, stored but not indexed.
    pub markdown: Field,
    pub tasks_open: Field,
    pub tasks_total: Field,
    pub has_image: Field,
    pub has_table: Field,
    pub has_checklist: Field,
    pub has_attachment: Field,
    pub created: Field,
    pub modified: Field,
    pub pinned: Field,
    pub conflict: Field,
    pub links: Field,
}

fn schema() -> (Schema, Fields) {
    let indexed = |tokenizer: &str, record: IndexRecordOption| {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(tokenizer)
                .set_index_option(record)
                .set_fieldnorms(false),
        )
    };
    let raw = || indexed("raw", IndexRecordOption::Basic);
    let words = || indexed(analysis::WORDS, IndexRecordOption::WithFreqsAndPositions);
    let grams = || indexed(analysis::EDGE_NGRAMS, IndexRecordOption::Basic);

    let mut builder = Schema::builder();
    let fields = Fields {
        id: builder.add_text_field(ID, raw().set_stored().set_fast("raw")),
        path: builder.add_text_field(PATH, raw().set_stored().set_fast("raw")),
        folder: builder.add_text_field("folder", raw().set_stored()),
        folder_key: builder.add_text_field("folder_key", raw()),
        title: builder.add_text_field("title", words().set_stored()),
        title_prefix: builder.add_text_field("title_prefix", grams()),
        title_key: builder.add_text_field("title_key", raw()),
        title_sort: builder.add_text_field(TITLE_SORT, TextOptions::default().set_fast("raw")),
        headings: builder.add_text_field("headings", words()),
        headings_prefix: builder.add_text_field("headings_prefix", grams()),
        tags: builder.add_text_field("tags", words().set_stored()),
        tag_key: builder.add_text_field("tag_key", raw()),
        tag_name: builder.add_text_field("tag_name", raw()),
        body: builder.add_text_field("body", words()),
        markdown: builder.add_text_field("markdown", STORED),
        tasks_open: builder.add_u64_field("tasks_open", STORED | FAST),
        tasks_total: builder.add_u64_field("tasks_total", STORED),
        has_image: builder.add_bool_field("has_image", INDEXED | STORED),
        has_table: builder.add_bool_field("has_table", INDEXED | STORED),
        has_checklist: builder.add_bool_field("has_checklist", INDEXED),
        has_attachment: builder.add_bool_field("has_attachment", INDEXED | STORED),
        // Nanoseconds since the epoch rather than tantivy dates: the service compares
        // `modified` with file times, so it has to come back exactly as it went in.
        created: builder.add_i64_field(CREATED, STORED | FAST),
        modified: builder.add_i64_field(MODIFIED, STORED | FAST),
        pinned: builder.add_bool_field("pinned", INDEXED | STORED),
        conflict: builder.add_bool_field("conflict", INDEXED | STORED),
        links: builder.add_text_field("links", raw()),
    };
    (builder.build(), fields)
}

impl From<TantivyError> for Error {
    fn from(error: TantivyError) -> Self {
        Error::Index(error.to_string())
    }
}

/// An index of notes, on disk or in memory.
pub struct SearchIndex {
    index: Index,
    reader: Reader,
    read_only: bool,
}

impl fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchIndex")
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl SearchIndex {
    /// Opens the index in `dir` for the process that writes it, creating the directory
    /// and the index as needed. An index that cannot be opened or has another schema
    /// version is deleted and created empty; the caller then indexes the folder again
    /// (an empty [`Reader::manifest`] makes every file look new).
    pub fn open_or_create(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir)?;
        let index = match open_existing(dir) {
            Some(index) => index,
            None => {
                for entry in fs::read_dir(dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        fs::remove_dir_all(path)?;
                    } else {
                        fs::remove_file(path)?;
                    }
                }
                let index = Index::create_in_dir(dir, schema().0)?;
                // Written last, so that a half-created index is not taken for a good one.
                fs::write(dir.join(VERSION_FILE), SCHEMA_VERSION.to_string())?;
                index
            }
        };
        // The writing process reloads after its own commits. `OnCommitWithDelay` would
        // add a thread that polls the directory twice a second, which a service that
        // must not wake up while idle cannot afford.
        Self::new(index, ReloadPolicy::Manual, false)
    }

    /// Opens the index in `dir` without ever writing to it. `None` if there is no usable
    /// index there (yet): the directory is missing, the index is incomplete or it has
    /// another schema version. The reader follows the commits of the writing process.
    pub fn open_read_only(dir: impl AsRef<Path>) -> Result<Option<Self>> {
        match open_existing(dir.as_ref()) {
            Some(index) => Self::new(index, ReloadPolicy::OnCommitWithDelay, true).map(Some),
            None => Ok(None),
        }
    }

    /// An index that lives in memory, for tests and for the CLI without a service.
    pub fn in_memory() -> Result<Self> {
        Self::new(
            Index::create_in_ram(schema().0),
            ReloadPolicy::Manual,
            false,
        )
    }

    fn new(index: Index, reload_policy: ReloadPolicy, read_only: bool) -> Result<Self> {
        analysis::register(index.tokenizers());
        let reader = Reader {
            inner: index
                .reader_builder()
                .reload_policy(reload_policy)
                .try_into()?,
            fields: Arc::new(schema().1),
        };
        Ok(SearchIndex {
            index,
            reader,
            read_only,
        })
    }

    /// Takes the writer lock. There is one writer per index, across processes; a second
    /// call fails until the first writer is dropped.
    ///
    /// The writer holds a few megabytes while open. A service that wants to stay small
    /// calls [`Writer::finish`] after a burst of changes and asks for a new writer when
    /// the next change arrives.
    pub fn writer(&self) -> Result<Writer> {
        if self.read_only {
            return Err(Error::Invalid("the index was opened read-only".to_owned()));
        }
        let options = IndexWriterOptions::builder()
            .num_worker_threads(1)
            .num_merge_threads(1)
            .memory_budget_per_thread(WRITER_MEMORY_BYTES)
            .build();
        let writer = self
            .index
            .writer_with_options(options)
            .map_err(|error| match error {
                TantivyError::LockFailure(..) => Error::Index(
                    "the index is locked by another writer (is sheaf-searchd running?)".to_owned(),
                ),
                other => other.into(),
            })?;
        Ok(Writer {
            writer,
            reader: self.reader.clone(),
        })
    }

    /// A handle for searching and listing. Cheap; clone it freely.
    pub fn reader(&self) -> Reader {
        self.reader.clone()
    }
}

/// Opens the index in `dir` if it is there, complete and of the current version.
fn open_existing(dir: &Path) -> Option<Index> {
    let version = fs::read_to_string(dir.join(VERSION_FILE)).ok()?;
    if version.trim() != SCHEMA_VERSION.to_string() {
        return None;
    }
    let index = Index::open_in_dir(dir).ok()?;
    (index.schema() == schema().0).then_some(index)
}

/// Adds, replaces and removes notes. Nothing is visible to readers, and nothing
/// survives a drop of the writer, before [`commit`](Writer::commit).
pub struct Writer {
    writer: IndexWriter,
    reader: Reader,
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer").finish_non_exhaustive()
    }
}

impl Writer {
    /// Adds the note, replacing the one indexed under the same `rel_path`.
    pub fn upsert(&mut self, note: &NoteDoc) -> Result<()> {
        let fields = &self.reader.fields;
        self.writer
            .delete_term(Term::from_field_text(fields.path, &note.meta.rel_path));
        self.writer.add_document(document(fields, note))?;
        Ok(())
    }

    /// Removes the note indexed under `rel_path`, if there is one.
    pub fn remove_path(&mut self, rel_path: &str) {
        self.writer
            .delete_term(Term::from_field_text(self.reader.fields.path, rel_path));
    }

    /// Removes every note.
    pub fn clear(&mut self) -> Result<()> {
        self.writer.delete_all_documents()?;
        Ok(())
    }

    /// Makes all changes since the last commit durable and visible. Readers of this
    /// process see them when the call returns, readers in other processes shortly after.
    pub fn commit(&mut self) -> Result<()> {
        self.writer.commit()?;
        self.reader.reload()
    }

    /// Commits, waits for segment merges to finish and releases the writer lock and the
    /// writer's memory. Prefer this over dropping the writer: a drop cancels running
    /// merges, and an index whose merges never finish gets slower with every commit.
    pub fn finish(mut self) -> Result<()> {
        self.commit()?;
        let Writer { writer, reader } = self;
        writer.wait_merging_threads()?;
        reader.reload()
    }
}

fn document(fields: &Fields, note: &NoteDoc) -> TantivyDocument {
    let meta = &note.meta;
    let mut doc = TantivyDocument::default();
    doc.add_text(fields.id, meta.id.as_str());
    doc.add_text(fields.path, &meta.rel_path);
    doc.add_text(fields.folder, &meta.folder);
    for run in folder_runs(&meta.folder) {
        doc.add_text(fields.folder_key, run);
    }
    doc.add_text(fields.title, &meta.title);
    doc.add_text(fields.title_prefix, &meta.title);
    let title_key = analysis::key(&meta.title);
    for form in title_key.forms() {
        doc.add_text(fields.title_key, form);
    }
    doc.add_text(fields.title_sort, &title_key.primary);
    for heading in &note.headings {
        doc.add_text(fields.headings, heading);
        doc.add_text(fields.headings_prefix, heading);
    }
    for tag in &meta.tags {
        doc.add_text(fields.tags, tag);
    }
    for (key, name) in tag_paths(&meta.tags) {
        doc.add_text(fields.tag_key, key);
        doc.add_text(fields.tag_name, name);
    }
    doc.add_text(fields.body, body_without_title(&note.body));
    doc.add_text(fields.markdown, &note.body);
    doc.add_u64(fields.tasks_open, u64::from(meta.tasks_open));
    doc.add_u64(fields.tasks_total, u64::from(meta.tasks_total));
    doc.add_bool(fields.has_image, meta.has_image);
    doc.add_bool(fields.has_table, meta.has_table);
    doc.add_bool(fields.has_checklist, meta.has_checklist());
    doc.add_bool(fields.has_attachment, meta.has_attachment);
    doc.add_i64(fields.created, to_nanos(meta.created));
    doc.add_i64(fields.modified, to_nanos(meta.modified));
    doc.add_bool(fields.pinned, meta.pinned);
    doc.add_bool(fields.conflict, meta.conflict);
    for link in &note.links {
        doc.add_text(fields.links, link_key(link));
    }
    doc
}

fn to_nanos(timestamp: Timestamp) -> i64 {
    // Out of range only before 1678 or after 2262.
    timestamp
        .as_nanosecond()
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn from_nanos(nanos: i64) -> Timestamp {
    Timestamp::from_nanosecond(i128::from(nanos)).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// The folded segments of a folder path or of a `folder:` value.
fn folder_segments(path: &str) -> Vec<String> {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| fold(segment).primary)
        .collect()
}

/// Every run of consecutive segments of `folder`, so that `folder:b` and `folder:b/c`
/// find a note in `a/b/c` with one term lookup. Runs that start at the root are also
/// listed with a leading `/`, for anchored values.
fn folder_runs(folder: &str) -> Vec<String> {
    let segments = folder_segments(folder);
    let mut runs = Vec::new();
    for start in 0..segments.len() {
        for end in start + 1..=segments.len() {
            let run = segments[start..end].join("/");
            if start == 0 {
                runs.push(format!("/{run}"));
            }
            runs.push(run);
        }
    }
    runs
}

fn folder_key(value: &str) -> String {
    let run = folder_segments(value).join("/");
    if value.starts_with('/') {
        format!("/{run}")
    } else {
        run
    }
}

fn tag_key(tag: &str) -> String {
    fold(tag.trim_matches('/')).primary
}

/// Every tag and each of its ancestors (`a/b/c` → `a`, `a/b`, `a/b/c`) as folded key
/// and as written, once per key.
fn tag_paths(tags: &[String]) -> Vec<(String, &str)> {
    let mut paths: Vec<(String, &str)> = Vec::new();
    for tag in tags {
        let tag = tag.trim_matches('/');
        let ends = tag
            .match_indices('/')
            .map(|(index, _)| index)
            .chain([tag.len()]);
        for end in ends {
            let name = &tag[..end];
            let key = fold(name).primary;
            if !key.is_empty() && paths.iter().all(|(seen, _)| *seen != key) {
                paths.push((key, name));
            }
        }
    }
    paths
}

/// Wiki-link targets are compared case-insensitively, nothing more: a link is a
/// reference to one title, not a search.
fn link_key(target: &str) -> String {
    target.trim().nfc().collect::<String>().to_lowercase()
}

/// How a listing is ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    /// Most recently modified first.
    Modified,
    /// Most recently created first.
    Created,
    /// By title, ignoring case and accents.
    Title,
}

/// Searches and lists the index. All methods see the state of the last reload.
#[derive(Clone)]
pub struct Reader {
    pub(crate) inner: IndexReader,
    pub(crate) fields: Arc<Fields>,
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader").finish_non_exhaustive()
    }
}

impl Reader {
    /// Picks up the latest commit now. A read-only index does this by itself within
    /// about a second of a commit; call it when that is not soon enough.
    pub fn reload(&self) -> Result<()> {
        Ok(self.inner.reload()?)
    }

    /// Number of notes in the index.
    pub fn count(&self) -> usize {
        self.inner.searcher().num_docs() as usize
    }

    /// The notes that pass all `filters`, for browsing without search words. `now` is
    /// what [`Filter::Modified`] is measured against.
    pub fn list(
        &self,
        filters: &[Filter],
        sort: Sort,
        limit: Option<usize>,
        now: &ZonedDateTime,
    ) -> Result<Vec<NoteMeta>> {
        let searcher = self.inner.searcher();
        let query = self
            .filter_query(filters.iter(), now)
            .unwrap_or_else(|| Box::new(AllQuery));
        let (addresses, _) = self.sorted(&searcher, query.as_ref(), sort, limit)?;
        self.metas(&searcher, &addresses)
    }

    /// The most recently modified notes.
    pub fn recent(&self, limit: usize) -> Result<Vec<NoteMeta>> {
        let searcher = self.inner.searcher();
        let (addresses, _) = self.sorted(&searcher, &AllQuery, Sort::Modified, Some(limit))?;
        self.metas(&searcher, &addresses)
    }

    pub fn get(&self, id: &NoteId) -> Result<Option<NoteMeta>> {
        self.get_by(Term::from_field_text(self.fields.id, id.as_str()))
    }

    /// The note at `rel_path` (relative to the notes folder, `/`-separated).
    pub fn get_by_path(&self, rel_path: &str) -> Result<Option<NoteMeta>> {
        self.get_by(Term::from_field_text(self.fields.path, rel_path))
    }

    fn get_by(&self, term: Term) -> Result<Option<NoteMeta>> {
        let searcher = self.inner.searcher();
        match first_match(&searcher, &term)? {
            Some(address) => Ok(Some(self.meta(&searcher.doc(address)?)?)),
            None => Ok(None),
        }
    }

    /// The Markdown text of a note as it was indexed, without frontmatter.
    pub fn body(&self, id: &NoteId) -> Result<Option<String>> {
        let searcher = self.inner.searcher();
        let term = Term::from_field_text(self.fields.id, id.as_str());
        let Some(address) = first_match(&searcher, &term)? else {
            return Ok(None);
        };
        let doc: TantivyDocument = searcher.doc(address)?;
        Ok(Some(text(&doc, self.fields.markdown).to_owned()))
    }

    /// Every tag in use, including the parents of nested tags, with the number of
    /// notes the `#tag` filter finds for it. Tags that differ only in case or accents
    /// are one tag, shown in their most frequent spelling. Sorted by name.
    pub fn tags(&self) -> Result<Vec<(String, usize)>> {
        let mut spellings: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for (name, count) in self.term_counts(self.fields.tag_name)? {
            spellings
                .entry(tag_key(&name))
                .or_default()
                .insert(name, count);
        }
        Ok(spellings
            .into_values()
            .filter_map(|names| {
                let total = names.values().sum();
                let (name, _) = names.into_iter().max_by_key(|(_, count)| *count)?;
                Some((name, total))
            })
            .collect())
    }

    /// Every folder that directly contains notes, with their number; the notes folder
    /// itself is the empty string. Sorted by path.
    pub fn folders(&self) -> Result<Vec<(String, usize)>> {
        Ok(self.term_counts(self.fields.folder)?.into_iter().collect())
    }

    /// The live notes per term of an untokenised field.
    fn term_counts(&self, field: Field) -> Result<BTreeMap<String, usize>> {
        let searcher = self.inner.searcher();
        let mut counts = BTreeMap::new();
        for segment in searcher.segment_readers() {
            let inverted = segment.inverted_index(field)?;
            let mut terms = inverted.terms().stream()?;
            while terms.advance() {
                let mut count = 0;
                for_each_live_doc(segment, &inverted, terms.value(), |_| count += 1)?;
                if count > 0 {
                    let term = String::from_utf8_lossy(terms.key()).into_owned();
                    *counts.entry(term).or_default() += count;
                }
            }
        }
        Ok(counts)
    }

    /// Notes whose title starts with `prefix`, ignoring case, accents and punctuation,
    /// ordered by title. This is what completes `[[`.
    pub fn titles_with_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<NoteMeta>> {
        let searcher = self.inner.searcher();
        let prefix = analysis::key(prefix);
        let mut found: Vec<(Vec<u8>, DocAddress)> = Vec::new();
        for (ordinal, segment) in searcher.segment_readers().iter().enumerate() {
            let inverted = segment.inverted_index(self.fields.title_key)?;
            for form in prefix.forms() {
                let mut taken = 0;
                for_each_term_with_prefix(inverted.terms(), form.as_bytes(), |key, term| {
                    for_each_live_doc(segment, &inverted, term, |doc| {
                        found.push((key.to_vec(), DocAddress::new(ordinal as u32, doc)));
                        taken += 1;
                    })?;
                    Ok(taken < limit)
                })?;
            }
        }
        found.sort();
        let mut addresses: Vec<DocAddress> = Vec::new();
        for (_, address) in found {
            // A title with umlauts is listed under both of its forms.
            if !addresses.contains(&address) {
                addresses.push(address);
            }
            if addresses.len() == limit {
                break;
            }
        }
        self.metas(&searcher, &addresses)
    }

    /// Notes with a `[[wiki link]]` to `title`, compared case-insensitively. Most
    /// recently modified first.
    pub fn backlinks(&self, title: &str) -> Result<Vec<NoteMeta>> {
        let searcher = self.inner.searcher();
        let query = term_query(Term::from_field_text(self.fields.links, &link_key(title)));
        let (addresses, _) = self.sorted(&searcher, query.as_ref(), Sort::Modified, None)?;
        self.metas(&searcher, &addresses)
    }

    /// Path, id and modification time of every indexed note, in no particular order.
    /// The service compares this with the folder to find out what changed while it was
    /// not running. Read from columns, so it does not touch the stored note texts.
    pub fn manifest(&self) -> Result<Vec<(String, NoteId, Timestamp)>> {
        let searcher = self.inner.searcher();
        let mut manifest = Vec::with_capacity(self.count());
        let mut path = String::new();
        let mut id = String::new();
        for segment in searcher.segment_readers() {
            let columns = segment.fast_fields();
            let (Some(paths), Some(ids)) = (columns.str(PATH)?, columns.str(ID)?) else {
                continue;
            };
            let modified = columns.i64(MODIFIED)?;
            for doc in segment.doc_ids_alive() {
                let (Some(path_ord), Some(id_ord), Some(modified)) = (
                    paths.ords().first(doc),
                    ids.ords().first(doc),
                    modified.first(doc),
                ) else {
                    continue;
                };
                path.clear();
                id.clear();
                paths.ord_to_str(path_ord, &mut path)?;
                ids.ord_to_str(id_ord, &mut id)?;
                manifest.push((path.clone(), parse_id(&id)?, from_nanos(modified)));
            }
        }
        Ok(manifest)
    }

    /// The conjunction of `filters`, or `None` if there are none.
    pub(crate) fn filter_query<'a>(
        &self,
        filters: impl Iterator<Item = &'a Filter>,
        now: &ZonedDateTime,
    ) -> Option<Box<dyn Query>> {
        let mut clauses: Vec<Box<dyn Query>> = filters
            .map(|filter| self.filter_clause(filter, now))
            .collect();
        match clauses.len() {
            0 => None,
            1 => clauses.pop(),
            _ => Some(Box::new(BooleanQuery::intersection(clauses))),
        }
    }

    fn filter_clause(&self, filter: &Filter, now: &ZonedDateTime) -> Box<dyn Query> {
        let fields = &self.fields;
        let flag = |field| term_query(Term::from_field_bool(field, true));
        match filter {
            Filter::Tag(tag) => term_query(Term::from_field_text(fields.tag_key, &tag_key(tag))),
            Filter::Folder(folder) => term_query(Term::from_field_text(
                fields.folder_key,
                &folder_key(folder),
            )),
            Filter::Has(Feature::Checklist) => flag(fields.has_checklist),
            Filter::Has(Feature::Image) => flag(fields.has_image),
            Filter::Has(Feature::Table) => flag(fields.has_table),
            Filter::Has(Feature::Attachment) => flag(fields.has_attachment),
            Filter::Pinned => flag(fields.pinned),
            Filter::Open => Box::new(RangeQuery::new(
                Bound::Included(Term::from_field_u64(fields.tasks_open, 1)),
                Bound::Unbounded,
            )),
            Filter::Modified(period) => Box::new(RangeQuery::new(
                Bound::Included(Term::from_field_i64(
                    fields.modified,
                    to_nanos(period_start(*period, now)),
                )),
                Bound::Unbounded,
            )),
        }
    }

    /// The live notes matching `query` in the order of `sort`, cut to `limit`, and
    /// their number before the cut.
    pub(crate) fn sorted(
        &self,
        searcher: &Searcher,
        query: &dyn Query,
        sort: Sort,
        limit: Option<usize>,
    ) -> Result<(Vec<DocAddress>, usize)> {
        let weight = query.weight(EnableScoring::disabled_from_searcher(searcher))?;
        let limit = limit.unwrap_or(usize::MAX);
        let segments = searcher.segment_readers().iter().enumerate();
        let column = match sort {
            Sort::Modified => MODIFIED,
            Sort::Created => CREATED,
            Sort::Title => {
                let mut total = 0;
                let mut rows: Vec<(String, DocAddress)> = Vec::new();
                for (ordinal, segment) in segments {
                    let titles = segment.fast_fields().str(TITLE_SORT)?;
                    let ordinal_of =
                        |doc| titles.as_ref().and_then(|titles| titles.ords().first(doc));
                    let mut local: Vec<(Option<u64>, DocId)> = Vec::new();
                    for_each_live_match(weight.as_ref(), segment, |doc| {
                        local.push((ordinal_of(doc), doc));
                    })?;
                    total += local.len();
                    // Term ordinals sort like the titles, so only the notes that can
                    // make the cut need their title looked up.
                    local.sort_unstable();
                    local.truncate(limit);
                    for (title_ordinal, doc) in local {
                        let mut title = String::new();
                        if let (Some(titles), Some(title_ordinal)) = (&titles, title_ordinal) {
                            titles.ord_to_str(title_ordinal, &mut title)?;
                        }
                        rows.push((title, DocAddress::new(ordinal as u32, doc)));
                    }
                }
                rows.sort();
                rows.truncate(limit);
                return Ok((
                    rows.into_iter().map(|(_, address)| address).collect(),
                    total,
                ));
            }
        };
        let mut rows: Vec<(i64, DocAddress)> = Vec::new();
        for (ordinal, segment) in segments {
            let times = segment.fast_fields().i64(column)?;
            for_each_live_match(weight.as_ref(), segment, |doc| {
                let time = times.first(doc).unwrap_or(i64::MIN);
                rows.push((time, DocAddress::new(ordinal as u32, doc)));
            })?;
        }
        let total = rows.len();
        rows.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        rows.truncate(limit);
        Ok((
            rows.into_iter().map(|(_, address)| address).collect(),
            total,
        ))
    }

    pub(crate) fn metas(
        &self,
        searcher: &Searcher,
        addresses: &[DocAddress],
    ) -> Result<Vec<NoteMeta>> {
        addresses
            .iter()
            .map(|address| self.meta(&searcher.doc(*address)?))
            .collect()
    }

    /// Rebuilds a [`NoteMeta`] from the stored fields of a document.
    pub(crate) fn meta(&self, doc: &TantivyDocument) -> Result<NoteMeta> {
        let fields = &self.fields;
        let flag = |field| doc.get_first(field).and_then(|value| value.as_bool()) == Some(true);
        let number = |field| {
            let value = doc.get_first(field).and_then(|value| value.as_u64());
            u32::try_from(value.unwrap_or(0)).unwrap_or(u32::MAX)
        };
        let time = |field| {
            from_nanos(
                doc.get_first(field)
                    .and_then(|value| value.as_i64())
                    .unwrap_or(0),
            )
        };
        Ok(NoteMeta {
            id: parse_id(text(doc, fields.id))?,
            rel_path: text(doc, fields.path).to_owned(),
            folder: text(doc, fields.folder).to_owned(),
            title: text(doc, fields.title).to_owned(),
            created: time(fields.created),
            modified: time(fields.modified),
            pinned: flag(fields.pinned),
            tags: doc
                .get_all(fields.tags)
                .filter_map(|value| value.as_str())
                .map(str::to_owned)
                .collect(),
            tasks_open: number(fields.tasks_open),
            tasks_total: number(fields.tasks_total),
            has_image: flag(fields.has_image),
            has_table: flag(fields.has_table),
            has_attachment: flag(fields.has_attachment),
            conflict: flag(fields.conflict),
        })
    }
}

pub(crate) fn text(doc: &TantivyDocument, field: Field) -> &str {
    doc.get_first(field)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
}

fn parse_id(id: &str) -> Result<NoteId> {
    id.parse()
        .map_err(|_| Error::Index(format!("the index holds an invalid note id: {id}")))
}

fn period_start(period: Period, now: &ZonedDateTime) -> Timestamp {
    let days_ago = |days: i64| {
        now.timestamp()
            .checked_sub(SignedDuration::from_hours(24 * days))
            .unwrap_or(Timestamp::MIN)
    };
    match period {
        Period::Today => now
            .start_of_day()
            .map_or_else(|_| days_ago(1), |midnight| midnight.timestamp()),
        Period::Week => days_ago(7),
        Period::Month => days_ago(30),
    }
}

pub(crate) fn term_query(term: Term) -> Box<dyn Query> {
    Box::new(TermQuery::new(term, IndexRecordOption::Basic))
}

/// Calls `visit` for the terms that start with `prefix`, in order, until it returns
/// `false`.
pub(crate) fn for_each_term_with_prefix(
    terms: &TermDictionary,
    prefix: &[u8],
    mut visit: impl FnMut(&[u8], &TermInfo) -> io::Result<bool>,
) -> io::Result<()> {
    let mut range = terms.range().ge(prefix);
    // The smallest byte string greater than everything that starts with `prefix`.
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last < u8::MAX {
            end.push(last + 1);
            range = range.lt(end);
            break;
        }
    }
    let mut range = range.into_stream()?;
    while range.advance() {
        if !visit(range.key(), range.value())? {
            break;
        }
    }
    Ok(())
}

/// Calls `visit` for every document of a term, deleted ones included.
pub(crate) fn for_each_doc(
    inverted: &InvertedIndexReader,
    term: &TermInfo,
    mut visit: impl FnMut(DocId),
) -> io::Result<()> {
    let mut postings =
        inverted.read_block_postings_from_terminfo(term, IndexRecordOption::Basic)?;
    loop {
        let docs = postings.docs();
        if docs.is_empty() {
            return Ok(());
        }
        docs.iter().copied().for_each(&mut visit);
        postings.advance();
    }
}

/// Calls `visit` for every document of a term that has not been deleted.
fn for_each_live_doc(
    segment: &SegmentReader,
    inverted: &InvertedIndexReader,
    term: &TermInfo,
    mut visit: impl FnMut(DocId),
) -> io::Result<()> {
    for_each_doc(inverted, term, |doc| {
        if !segment.is_deleted(doc) {
            visit(doc);
        }
    })
}

/// Calls `visit` for every document matching `weight` that has not been deleted.
fn for_each_live_match(
    weight: &dyn Weight,
    segment: &SegmentReader,
    mut visit: impl FnMut(DocId),
) -> Result<()> {
    let live = segment.alive_bitset();
    weight.for_each_no_score(segment, &mut |docs| {
        for &doc in docs {
            if live.is_none_or(|live| live.is_alive(doc)) {
                visit(doc);
            }
        }
    })?;
    Ok(())
}

fn first_match(searcher: &Searcher, term: &Term) -> Result<Option<DocAddress>> {
    for (ordinal, segment) in searcher.segment_readers().iter().enumerate() {
        let inverted = segment.inverted_index(term.field())?;
        let Some(mut postings) = inverted.read_postings(term, IndexRecordOption::Basic)? else {
            continue;
        };
        let mut doc = postings.doc();
        while doc != TERMINATED {
            if !segment.is_deleted(doc) {
                return Ok(Some(DocAddress::new(ordinal as u32, doc)));
            }
            doc = postings.advance();
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_runs_list_every_run_of_segments() {
        assert_eq!(folder_runs(""), Vec::<String>::new());
        assert_eq!(folder_runs("Arbeit"), ["/arbeit", "arbeit"]);
        assert_eq!(
            folder_runs("Firma/Büro/Projekte"),
            [
                "/firma",
                "firma",
                "/firma/buro",
                "firma/buro",
                "/firma/buro/projekte",
                "firma/buro/projekte",
                "buro",
                "buro/projekte",
                "projekte",
            ]
        );
    }

    #[test]
    fn folder_keys_fold_and_keep_the_anchor() {
        assert_eq!(folder_key("Arbeit"), "arbeit");
        assert_eq!(folder_key("Büro/Projekte/"), "buro/projekte");
        assert_eq!(folder_key("/Meine Notizen"), "/meine notizen");
    }

    #[test]
    fn tag_paths_include_ancestors_once() {
        let tags = ["Projekt/Sheaf/UI", "projekt/visor", "Büro"].map(str::to_owned);
        assert_eq!(
            tag_paths(&tags),
            [
                ("projekt".to_owned(), "Projekt"),
                ("projekt/sheaf".to_owned(), "Projekt/Sheaf"),
                ("projekt/sheaf/ui".to_owned(), "Projekt/Sheaf/UI"),
                ("projekt/visor".to_owned(), "projekt/visor"),
                ("buro".to_owned(), "Büro"),
            ]
        );
    }

    #[test]
    fn timestamps_survive_the_index_exactly() {
        let timestamp: Timestamp = "2026-11-16T10:12:13.123456789Z".parse().unwrap();
        assert_eq!(from_nanos(to_nanos(timestamp)), timestamp);
        assert_eq!(to_nanos(Timestamp::MAX), i64::MAX);
        assert_eq!(to_nanos(Timestamp::MIN), i64::MIN);
    }
}
