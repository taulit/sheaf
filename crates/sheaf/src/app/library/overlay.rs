//! The app's own writes that the index of `sheaf-searchd` may not show yet.
//!
//! The service is the only writer of the index on disk and indexes a write when it
//! gets to it. Until it confirms that, [`Overlay`] keeps the notes the app wrote in a
//! small index in memory, and [`Lists`] reads both: the service's index without the
//! paths the app touched, and the overlay for those paths. So a renamed, trashed or
//! newly saved note shows as it is at once, without waiting for the service.
//!
//! Folder and tag counts come from the service's index alone and follow it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use jiff::ZonedDateTime;
use sheaf_core::{
    Filter, NoteId, NoteMeta, NoteStore, ParsedQuery, Reader, Result, SearchIndex, SearchResults,
    Sort,
};

pub(super) struct Overlay {
    /// The notes now at the paths in `paths` that are note files.
    index: SearchIndex,
    reader: Reader,
    /// Every path the app wrote or removed, with the number of the last write to it.
    paths: HashMap<String, u64>,
    /// Numbers the writes.
    writes: u64,
}

impl Overlay {
    pub fn new() -> Result<Overlay> {
        let index = SearchIndex::in_memory()?;
        Ok(Overlay {
            reader: index.reader(),
            index,
            paths: HashMap::new(),
            writes: 0,
        })
    }

    /// The number of the last write.
    pub fn writes(&self) -> u64 {
        self.writes
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Takes a write that touched `paths`.
    pub fn reflect(&mut self, store: &NoteStore, paths: &[String]) -> Result<()> {
        self.writes += 1;
        for rel_path in paths {
            self.paths.insert(rel_path.clone(), self.writes);
        }
        super::index_paths(&self.index, store, paths)?;
        self.reader.reload()
    }

    /// Forgets the writes up to number `upto`: the service's index shows them now.
    pub fn confirm(&mut self, upto: u64) -> Result<()> {
        let done: Vec<String> = self
            .paths
            .iter()
            .filter(|(_, write)| **write <= upto)
            .map(|(rel_path, _)| rel_path.clone())
            .collect();
        if done.is_empty() {
            return Ok(());
        }
        let mut writer = self.index.writer()?;
        for rel_path in &done {
            writer.remove_path(rel_path);
            self.paths.remove(rel_path);
        }
        writer.finish()?;
        self.reader.reload()
    }
}

/// What the overlay holds at one moment.
struct Own {
    reader: Reader,
    paths: HashSet<String>,
}

/// Reads the service's index with the app's own unconfirmed writes over it, see the
/// module documentation. Blocks; for the background executor.
#[derive(Clone)]
pub(super) struct Lists {
    disk: Reader,
    overlay: Option<Arc<Mutex<Overlay>>>,
}

impl Lists {
    pub fn new(disk: Reader, overlay: Option<Arc<Mutex<Overlay>>>) -> Lists {
        Lists { disk, overlay }
    }

    fn own(&self) -> Option<Own> {
        let overlay = self.overlay.as_ref()?;
        let overlay = overlay.lock().unwrap_or_else(PoisonError::into_inner);
        (!overlay.paths.is_empty()).then(|| Own {
            reader: overlay.reader.clone(),
            paths: overlay.paths.keys().cloned().collect(),
        })
    }

    pub fn list(
        &self,
        filters: &[Filter],
        sort: Sort,
        now: &ZonedDateTime,
    ) -> Result<Vec<NoteMeta>> {
        let listed = self.disk.list(filters, sort, None, now)?;
        let Some(own) = self.own() else {
            return Ok(listed);
        };
        let mut merged: Vec<NoteMeta> = listed
            .into_iter()
            .filter(|note| !own.paths.contains(&note.rel_path))
            .collect();
        merged.extend(own.reader.list(filters, sort, None, now)?);
        merged.sort_by(|a, b| sort.compare(a, b));
        Ok(merged)
    }

    pub fn search(
        &self,
        query: &ParsedQuery,
        limit: usize,
        now: &ZonedDateTime,
    ) -> Result<SearchResults> {
        let mut found = self.disk.search(query, limit, now)?;
        let Some(own) = self.own() else {
            return Ok(found);
        };
        let listed = found.hits.len();
        found
            .hits
            .retain(|hit| !own.paths.contains(&hit.meta.rel_path));
        let stale = listed - found.hits.len();
        let mine = own.reader.search(query, limit, now)?;
        found.hits.extend(mine.hits);
        // Queries without words score 0 and list the newest first.
        found.hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| Sort::Modified.compare(&a.meta, &b.meta))
        });
        found.hits.truncate(limit);
        found.total = found.total.saturating_sub(stale) + mine.total;
        Ok(found)
    }

    pub fn get(&self, id: &NoteId) -> Result<Option<NoteMeta>> {
        let own = self.own();
        if let Some(own) = &own
            && let Some(meta) = own.reader.get(id)?
        {
            return Ok(Some(meta));
        }
        let meta = self.disk.get(id)?;
        Ok(meta.filter(|meta| {
            !own.as_ref()
                .is_some_and(|own| own.paths.contains(&meta.rel_path))
        }))
    }

    /// Notes with a wiki link to `title`, most recently modified first.
    pub fn backlinks(&self, title: &str) -> Result<Vec<NoteMeta>> {
        let found = self.disk.backlinks(title)?;
        let Some(own) = self.own() else {
            return Ok(found);
        };
        let mut merged: Vec<NoteMeta> = found
            .into_iter()
            .filter(|note| !own.paths.contains(&note.rel_path))
            .collect();
        merged.extend(own.reader.backlinks(title)?);
        merged.sort_by(|a, b| Sort::Modified.compare(a, b));
        Ok(merged)
    }

    pub fn body(&self, id: &NoteId) -> Result<Option<String>> {
        let Some(own) = self.own() else {
            return self.disk.body(id);
        };
        if let Some(body) = own.reader.body(id)? {
            return Ok(Some(body));
        }
        match self.disk.get(id)? {
            Some(meta) if !own.paths.contains(&meta.rel_path) => self.disk.body(id),
            _ => Ok(None),
        }
    }
}
