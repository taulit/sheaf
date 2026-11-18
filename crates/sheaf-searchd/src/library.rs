//! The notes folder and its index, kept in step. Everything here blocks; the D-Bus side
//! runs it on a worker thread, one call at a time. Writes to the index go through the
//! [`indexer`](crate::indexer) child process.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use jiff::{Timestamp, ZonedDateTime};
use sheaf_core::i18n::tr;
use sheaf_core::query::parse;
use sheaf_core::snippet::{SNIPPET_CHARS, preview};
use sheaf_core::{
    Change, CreateOptions, Error, FolderConfig, NoteId, NoteStore, Reader, Result, SearchIndex,
    TRASH_MAX_AGE,
};
use sheaf_searchd::api::NoteRow;

use crate::indexer::{self, Op};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 200;
/// Snippets remembered for `GetResultMetas`; the shell asks right after a search.
const MAX_SNIPPETS: usize = 1000;

/// What the shell shows for one result.
#[derive(Debug)]
pub struct ResultMeta {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug)]
pub struct Library {
    store: NoteStore,
    index_dir: PathBuf,
    /// `None` while idle: see [`release`](Library::release).
    reader: Option<Reader>,
    /// The snippet of the most recent search that returned a note, by id.
    snippets: HashMap<String, String>,
}

impl Library {
    pub fn open(notes_dir: &Path, index_dir: &Path) -> Result<Self> {
        // Creates or repairs the index; this process only reads it afterwards.
        let index = SearchIndex::open_or_create(index_dir)?;
        Ok(Library {
            store: NoteStore::new(notes_dir),
            index_dir: index_dir.to_owned(),
            reader: Some(index.reader()),
            snippets: HashMap::new(),
        })
    }

    /// Catches up with what changed while the service was not running and empties the
    /// trash of old notes. Returns the ids of the notes whose index entry changed.
    pub fn start(&mut self) -> Result<Vec<String>> {
        let ids = self.reconcile()?;
        match self.store.purge_expired(Timestamp::now(), TRASH_MAX_AGE) {
            Ok(purged) if !purged.is_empty() => {
                log::info!("removed {} notes from the trash", purged.len());
            }
            Ok(_) => {}
            Err(err) => log::warn!("cannot empty the trash: {err}"),
        }
        Ok(ids)
    }

    /// Indexes the files that are new or whose modification time differs from the
    /// indexed one, and drops the notes whose file is gone. An empty index makes this a
    /// full build. Returns the ids of the notes whose index entry changed.
    pub fn reconcile(&mut self) -> Result<Vec<String>> {
        let mut indexed: HashMap<String, (NoteId, Timestamp)> = self
            .reader()?
            .manifest()?
            .into_iter()
            .map(|(path, id, modified)| (path, (id, modified)))
            .collect();
        let mut stale = Vec::new();
        for entry in self.store.walk()? {
            match indexed.remove(&entry.rel_path) {
                Some((_, modified)) if modified == entry.modified => {}
                _ => stale.push(entry.rel_path),
            }
        }
        if stale.is_empty() && indexed.is_empty() {
            return Ok(Vec::new());
        }
        log::info!(
            "indexing {} changed notes, removing {}",
            stale.len(),
            indexed.len()
        );
        let mut ids = Vec::new();
        let mut ops = Vec::with_capacity(indexed.len() + stale.len());
        for (path, (id, _)) in indexed {
            ops.push(Op::Remove(path));
            ids.push(id.to_string());
        }
        ops.extend(stale.into_iter().map(Op::Upsert));
        ids.extend(self.write(&ops)?);
        Ok(dedup(ids))
    }

    /// Applies a batch of the folder watcher. Returns the ids of the touched notes, or
    /// none after a rescan.
    pub fn apply(&mut self, changes: Vec<Change>) -> Result<Vec<String>> {
        if changes.contains(&Change::Rescan) {
            self.store.refresh();
            self.reconcile()?;
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        let mut ops = Vec::with_capacity(changes.len());
        for change in changes {
            match change {
                Change::Upsert(path) => ops.push(Op::Upsert(path)),
                Change::Remove(path) => {
                    if let Some(meta) = self.reader()?.get_by_path(&path)? {
                        ids.push(meta.id.to_string());
                    }
                    ops.push(Op::Remove(path));
                }
                Change::Rescan => {}
            }
        }
        ids.extend(self.write(&ops)?);
        Ok(dedup(ids))
    }

    /// Throws the index away and builds it again from the folder.
    pub fn reindex(&mut self) -> Result<()> {
        self.store.refresh();
        self.snippets.clear();
        let entries = self.store.walk()?;
        let ops: Vec<Op> = std::iter::once(Op::Clear)
            .chain(entries.into_iter().map(|entry| Op::Upsert(entry.rel_path)))
            .collect();
        self.write(&ops)?;
        Ok(())
    }

    pub fn search(&mut self, query: &str, limit: u32) -> Result<Vec<NoteRow>> {
        let limit = match limit {
            0 => DEFAULT_LIMIT,
            limit => usize::try_from(limit).map_or(MAX_LIMIT, |limit| limit.min(MAX_LIMIT)),
        };
        let results = self
            .reader()?
            .search(&parse(query), limit, &ZonedDateTime::now())?;
        if self.snippets.len() + results.hits.len() > MAX_SNIPPETS {
            self.snippets.clear();
        }
        let rows = results
            .hits
            .into_iter()
            .map(|hit| {
                let id = hit.meta.id.to_string();
                self.snippets.insert(id.clone(), hit.snippet.text.clone());
                let modified = hit.meta.modified.as_second();
                (
                    id,
                    hit.meta.title,
                    hit.snippet.text,
                    hit.meta.folder,
                    modified,
                )
            })
            .collect();
        Ok(rows)
    }

    /// The notes `ids` names that still exist, as the shell shows them.
    pub fn result_metas(&mut self, ids: &[String]) -> Result<Vec<ResultMeta>> {
        let reader = self.reader()?;
        let mut metas = Vec::with_capacity(ids.len());
        for id in ids {
            let Ok(note_id) = id.parse::<NoteId>() else {
                continue;
            };
            let Some(meta) = reader.get(&note_id)? else {
                continue;
            };
            let description = match self.snippets.get(id) {
                Some(snippet) => snippet.clone(),
                None => preview(&reader.body(&note_id)?.unwrap_or_default(), SNIPPET_CHARS),
            };
            let name = if meta.title.is_empty() {
                tr("Untitled")
            } else {
                meta.title
            };
            metas.push(ResultMeta {
                id: id.clone(),
                name,
                description,
            });
        }
        Ok(metas)
    }

    /// Writes a new note and indexes it. An empty `folder` is the Quick Notes folder,
    /// `/` the notes folder itself.
    pub fn create(&mut self, text: &str, folder: &str) -> Result<String> {
        let folder = match folder {
            "" => FolderConfig::load(self.store.root())?
                .quick_note
                .folder_name(),
            "/" => String::new(),
            folder => folder.to_owned(),
        };
        let options = CreateOptions {
            folder,
            ..CreateOptions::default()
        };
        let meta = self.store.create(text, &options)?;
        self.write(&[Op::Upsert(meta.rel_path)])?;
        Ok(meta.id.to_string())
    }

    /// Appends to a note and indexes it. Returns its id and, if appending gave it a real
    /// id, the provisional one it had.
    pub fn append(&mut self, id: &str, text: &str) -> Result<Vec<String>> {
        let note_id: NoteId = id.parse().map_err(|_| Error::NotFound(id.to_owned()))?;
        let meta = self.store.append(&note_id, text)?;
        self.write(&[Op::Upsert(meta.rel_path)])?;
        Ok(dedup(vec![note_id.to_string(), meta.id.to_string()]))
    }

    /// Applies `ops` and shows their result to the following reads. Returns the ids of
    /// the notes that were indexed. If the index is gone, the indexer starts a new one
    /// with just these notes, so the rest of the folder is indexed right after.
    fn write(&mut self, ops: &[Op]) -> Result<Vec<String>> {
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        let lost = !SearchIndex::exists(&self.index_dir);
        let ids = indexer::apply(self.store.root(), &self.index_dir, ops)?;
        if lost {
            self.rebuild()?;
        } else if let Some(reader) = &self.reader {
            reader.reload()?;
        }
        Ok(ids)
    }

    /// The reader of the index, opened again if it was released. An index that was
    /// cleared or broken meanwhile is built again before anything is read from it, so
    /// a search never answers from an empty index.
    fn reader(&mut self) -> Result<Reader> {
        if let Some(reader) = &self.reader {
            return Ok(reader.clone());
        }
        if !SearchIndex::exists(&self.index_dir) {
            return self.rebuild();
        }
        let reader = SearchIndex::open_or_create(&self.index_dir)?.reader();
        self.reader = Some(reader.clone());
        Ok(reader)
    }

    /// Opens the index again, creating it if it is gone, and catches up with the folder.
    fn rebuild(&mut self) -> Result<Reader> {
        log::warn!("the index is gone or broken, indexing the notes folder again");
        let reader = SearchIndex::open_or_create(&self.index_dir)?.reader();
        self.reader = Some(reader.clone());
        self.snippets.clear();
        self.reconcile()?;
        Ok(reader)
    }

    /// Lets go of the index and what was read from it, while nobody asks: the pages of
    /// the index that searches touched count towards the resident size for as long as
    /// it is open (`SearchIndex`). The next call opens it again, which takes a few
    /// milliseconds.
    pub fn release(&mut self) {
        self.reader = None;
        self.snippets = HashMap::new();
    }
}

fn dedup(mut ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    ids
}
