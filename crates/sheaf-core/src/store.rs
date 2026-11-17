//! The notes folder: enumerate, read, resolve and change notes.
//!
//! Reading lives here, the mutating calls are in `write` and in [`crate::trash`].

mod edit;
pub(crate) mod fsops;
pub(crate) mod write;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::error::{Error, Result};
use crate::frontmatter::{self, Frontmatter};
use crate::model::{NoteDoc, NoteId, NoteMeta, NoteRef};
use crate::scan::{self, Scan};

pub use write::{CreateOptions, TaskRef, WriteOptions, checked_folder};

pub(crate) const TRASH_DIR: &str = ".trash";
pub(crate) const ATTACHMENTS_DIR: &str = "_attachments";
pub(crate) const NOTE_EXT: &str = ".md";

/// How much of a file is read to find its id. Frontmatter is rarely longer.
const HEAD_BYTES: usize = 4096;

/// A notes folder on disk.
///
/// Notes are addressed by id. The store keeps an id → path table that it builds on
/// first use from the head of every file and updates with its own writes. Changes made
/// by other programs are picked up when a lookup fails, or at once with [`refresh`].
///
/// [`refresh`]: NoteStore::refresh
#[derive(Debug)]
pub struct NoteStore {
    root: PathBuf,
    ids: Mutex<Option<HashMap<NoteId, String>>>,
}

/// A note file as a directory listing shows it; no file was read for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the notes folder, `/`-separated.
    pub rel_path: String,
    pub modified: Timestamp,
    pub size: u64,
}

/// The state of a file as a reader last saw it, to detect changes by others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub modified: Timestamp,
    pub size: u64,
}

/// A note read from disk.
#[derive(Clone, Debug)]
pub struct Note {
    pub meta: NoteMeta,
    /// The file exactly as stored.
    pub text: String,
    pub frontmatter: Frontmatter,
    /// What the body contains, including the tasks with their positions.
    pub scan: Scan,
    pub fingerprint: Fingerprint,
    body_start: usize,
}

impl Entry {
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            modified: self.modified,
            size: self.size,
        }
    }
}

impl Note {
    /// The Markdown text without frontmatter.
    pub fn body(&self) -> &str {
        &self.text[self.body_start..]
    }

    /// The byte order mark and frontmatter block in front of the body.
    pub(crate) fn head(&self) -> &str {
        &self.text[..self.body_start]
    }

    pub fn into_doc(self) -> NoteDoc {
        let body = self.text[self.body_start..].to_owned();
        NoteDoc {
            meta: self.meta,
            headings: self.scan.headings,
            body,
            links: self.scan.links,
        }
    }
}

impl NoteStore {
    /// A store over `root`. Nothing is read or created until the first call.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        NoteStore {
            root: root.into(),
            ids: Mutex::new(None),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All note files, sorted by path. A missing notes folder is an empty one.
    pub fn walk(&self) -> Result<Vec<Entry>> {
        let mut notes = Vec::new();
        self.visit("", Some(&mut notes), &mut Vec::new())?;
        notes.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Ok(notes)
    }

    /// All folders below the root, empty ones included, `/`-separated and sorted.
    pub fn folders(&self) -> Result<Vec<String>> {
        let mut folders = Vec::new();
        self.visit("", None, &mut folders)?;
        folders.sort();
        Ok(folders)
    }

    /// Reads the note at `rel_path`.
    pub fn load(&self, rel_path: &str) -> Result<Note> {
        let path = self.abs_checked(rel_path)?;
        let not_found = |err: io::Error| match err.kind() {
            io::ErrorKind::NotFound => Error::NotFound(rel_path.to_owned()),
            _ => Error::Io(err),
        };
        // Stat before read: if the file changes in between, the fingerprint is the older
        // one and the next guarded write reports the conflict.
        let mut file = File::open(&path).map_err(not_found)?;
        let metadata = file.metadata()?;
        let mut text = String::with_capacity(metadata.len() as usize);
        file.read_to_string(&mut text)?;

        let parts = frontmatter::split(&text);
        let frontmatter = parts.frontmatter();
        let scan = scan::scan(parts.body);
        let body_start = text.len() - parts.body.len();
        let modified = timestamp(metadata.modified()).unwrap_or_default();
        let created = frontmatter
            .created
            .or_else(|| timestamp(metadata.created()))
            .unwrap_or(modified);
        let (folder, name) = split_rel(rel_path);

        let meta = NoteMeta {
            id: self.effective_id(rel_path, frontmatter.id.as_ref())?,
            rel_path: rel_path.to_owned(),
            folder: folder.to_owned(),
            title: scan.title.clone(),
            created,
            modified,
            pinned: frontmatter.pinned,
            tags: scan::merge_tags(&frontmatter.tags, &scan.tags),
            tasks_open: scan.tasks.iter().filter(|task| !task.checked).count() as u32,
            tasks_total: scan.tasks.len() as u32,
            has_image: scan.has_image,
            has_table: scan.has_table,
            has_attachment: scan.has_attachment,
            conflict: is_conflict_name(name),
        };
        Ok(Note {
            meta,
            text,
            frontmatter,
            scan,
            fingerprint: Fingerprint {
                modified,
                size: metadata.len(),
            },
            body_start,
        })
    }

    /// Reads the note at `rel_path` in the form the index consumes.
    pub fn load_doc(&self, rel_path: &str) -> Result<NoteDoc> {
        self.load(rel_path).map(Note::into_doc)
    }

    /// Reads the note with this id.
    pub fn get(&self, id: &NoteId) -> Result<Note> {
        let note = self.load(&self.path_of(id)?)?;
        if note.meta.id == *id {
            return Ok(note);
        }
        // Another program put a different note at the path the table had for this id.
        self.refresh();
        let note = self.load(&self.path_of(id)?)?;
        if note.meta.id == *id {
            Ok(note)
        } else {
            Err(Error::NotFound(id.to_string()))
        }
    }

    /// The path of the note with this id, relative to the notes folder.
    pub fn path_of(&self, id: &NoteId) -> Result<String> {
        let lookup = || self.with_ids(|ids| ids.get(id).cloned());
        if let Some(rel_path) = lookup()?
            && self.abs(&rel_path).is_file()
        {
            return Ok(rel_path);
        }
        self.refresh();
        lookup()?.ok_or_else(|| Error::NotFound(id.to_string()))
    }

    /// The current state of the note's file, for [`WriteOptions::expected`].
    pub fn fingerprint(&self, id: &NoteId) -> Result<Fingerprint> {
        let metadata = fs::metadata(self.abs(&self.path_of(id)?))?;
        Ok(Fingerprint {
            modified: timestamp(metadata.modified()).unwrap_or_default(),
            size: metadata.len(),
        })
    }

    /// The state of the file at `rel_path` now; `None` if there is none.
    pub fn fingerprint_at(&self, rel_path: &str) -> Option<Fingerprint> {
        let metadata = fs::metadata(self.abs(rel_path)).ok()?;
        Some(Fingerprint {
            modified: timestamp(metadata.modified()).unwrap_or_default(),
            size: metadata.len(),
        })
    }

    /// Every note without its text. A file that cannot be read is logged and skipped.
    pub fn all_meta(&self) -> Result<Vec<NoteMeta>> {
        Ok(self.load_all()?.map(|note| note.meta).collect())
    }

    /// Every note as the index consumes it. A file that cannot be read is logged and skipped.
    pub fn all_docs(&self) -> Result<Vec<NoteDoc>> {
        Ok(self.load_all()?.map(Note::into_doc).collect())
    }

    /// Finds the note meant by an id, an exact title or the start of a title.
    ///
    /// Titles compare without case. An id wins over an exact title, an exact title over
    /// a title prefix. Several matches of the winning kind are [`Error::Ambiguous`].
    pub fn resolve(&self, reference: &str) -> Result<NoteMeta> {
        let reference = reference.trim();
        if let Ok(id) = reference.parse::<NoteId>() {
            match self.get(&id) {
                Ok(note) => return Ok(note.meta),
                Err(Error::NotFound(_)) => {}
                Err(err) => return Err(err),
            }
        }
        let needle = reference.to_lowercase();
        let all = self.all_meta()?;
        let titles: Vec<String> = all.iter().map(|meta| meta.title.to_lowercase()).collect();
        if !needle.is_empty() {
            for exact in [true, false] {
                let matches: Vec<&NoteMeta> = all
                    .iter()
                    .zip(&titles)
                    .filter(|(_, title)| {
                        if exact {
                            **title == needle
                        } else {
                            title.starts_with(&needle)
                        }
                    })
                    .map(|(meta, _)| meta)
                    .collect();
                match matches.as_slice() {
                    [] => {}
                    [meta] => return Ok((*meta).clone()),
                    _ => {
                        return Err(Error::Ambiguous {
                            query: reference.to_owned(),
                            candidates: matches.into_iter().map(NoteRef::from).collect(),
                        });
                    }
                }
            }
        }
        Err(Error::NotFound(reference.to_owned()))
    }

    /// Forgets the id → path table, so the next lookup sees changes made by other programs.
    pub fn refresh(&self) {
        *self.ids.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    pub(crate) fn abs(&self, rel_path: &str) -> PathBuf {
        if rel_path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(rel_path)
        }
    }

    /// Runs `f` on the id → path table, building it first if needed.
    pub(crate) fn with_ids<T>(
        &self,
        f: impl FnOnce(&mut HashMap<NoteId, String>) -> T,
    ) -> Result<T> {
        let mut guard = self.ids.lock().unwrap_or_else(PoisonError::into_inner);
        let ids = match &mut *guard {
            Some(ids) => ids,
            none => none.insert(self.build_ids()?),
        };
        Ok(f(ids))
    }

    fn abs_checked(&self, rel_path: &str) -> Result<PathBuf> {
        let inside = !rel_path.is_empty()
            && rel_path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if inside {
            Ok(self.root.join(rel_path))
        } else {
            Err(Error::Invalid(format!(
                "not a path inside the notes folder: {rel_path}"
            )))
        }
    }

    fn load_all(&self) -> Result<impl Iterator<Item = Note>> {
        let notes = self.walk()?.into_iter().filter_map(|entry| {
            self.load(&entry.rel_path)
                .inspect_err(|err| log::warn!("skipping {}: {err}", entry.rel_path))
                .ok()
        });
        Ok(notes)
    }

    fn visit(
        &self,
        folder: &str,
        mut notes: Option<&mut Vec<Entry>>,
        folders: &mut Vec<String>,
    ) -> Result<()> {
        let entries = match fs::read_dir(self.abs(folder)) {
            Ok(entries) => entries,
            Err(err) if folder.is_empty() && err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) if folder.is_empty() => return Err(err.into()),
            Err(err) => {
                log::warn!("skipping folder {folder}: {err}");
                return Ok(());
            }
        };
        for entry in entries {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                log::warn!("skipping {:?}: the name is not UTF-8", entry.path());
                continue;
            };
            if name.starts_with('.') || name == ATTACHMENTS_DIR {
                continue;
            }
            // `file_type` does not follow symbolic links, so links are neither notes nor
            // folders and the walk cannot leave the root or loop.
            let file_type = entry.file_type()?;
            let rel_path = join_rel(folder, &name);
            if file_type.is_dir() {
                self.visit(&rel_path, notes.as_deref_mut(), folders)?;
                folders.push(rel_path);
            } else if file_type.is_file()
                && name.ends_with(NOTE_EXT)
                && let Some(notes) = notes.as_deref_mut()
            {
                let metadata = entry.metadata()?;
                notes.push(Entry {
                    rel_path,
                    modified: timestamp(metadata.modified()).unwrap_or_default(),
                    size: metadata.len(),
                });
            }
        }
        Ok(())
    }

    fn build_ids(&self) -> Result<HashMap<NoteId, String>> {
        let mut entries = self.walk()?;
        // Among files that share an id (sync conflict copies), the original keeps it.
        entries.sort_by_key(|entry| is_conflict_name(&entry.rel_path));
        let mut ids = HashMap::with_capacity(entries.len());
        for entry in entries {
            let stored = match read_head(&self.abs(&entry.rel_path)) {
                Ok(head) => frontmatter::split(&head).frontmatter().id,
                Err(err) => {
                    log::warn!("skipping {}: {err}", entry.rel_path);
                    continue;
                }
            };
            let id = stored
                .filter(|id| !ids.contains_key(id))
                .unwrap_or_else(|| NoteId::provisional(&entry.rel_path));
            ids.insert(id, entry.rel_path);
        }
        Ok(ids)
    }

    /// The id the note at `rel_path` goes by: the stored one, unless another file holds
    /// it already, else the provisional id of the path.
    fn effective_id(&self, rel_path: &str, stored: Option<&NoteId>) -> Result<NoteId> {
        let provisional = || NoteId::provisional(rel_path);
        if in_trash(rel_path) {
            return Ok(stored.cloned().unwrap_or_else(provisional));
        }
        self.with_ids(|ids| {
            let id = match stored {
                Some(id) => match ids.get(id) {
                    Some(owner) if owner != rel_path && self.abs(owner).is_file() => provisional(),
                    _ => id.clone(),
                },
                None => provisional(),
            };
            ids.insert(id.clone(), rel_path.to_owned());
            id
        })
    }
}

/// Folder and file name of a relative path; the folder is empty for the root.
pub(crate) fn split_rel(rel_path: &str) -> (&str, &str) {
    rel_path.rsplit_once('/').unwrap_or(("", rel_path))
}

pub(crate) fn join_rel(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

pub(crate) fn in_trash(rel_path: &str) -> bool {
    split_rel(rel_path).0 == TRASH_DIR
}

/// A point in time as frontmatter stores it: RFC 3339 with the local offset, to the second.
pub(crate) fn format_local(time: Timestamp) -> String {
    time.to_zoned_datetime(TimeZone::system())
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

fn timestamp(time: io::Result<SystemTime>) -> Option<Timestamp> {
    time.ok().and_then(|time| Timestamp::try_from(time).ok())
}

/// Whether a sync tool named this file as a conflict copy: Syncthing's
/// `name.sync-conflict-…`, Nextcloud's `name (conflicted copy …)` or Dropbox's
/// `name (… conflicted copy …)`.
fn is_conflict_name(name: &str) -> bool {
    name.contains(".sync-conflict-")
        || name.match_indices(" (").any(|(at, _)| {
            let inside = &name[at + 2..];
            inside
                .find(')')
                .is_some_and(|end| inside[..end].contains("conflicted copy"))
        })
}

/// The start of a file, extended to the whole file if its frontmatter is longer.
fn read_head(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut head = Vec::with_capacity(HEAD_BYTES);
    file.by_ref()
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)?;
    if head.len() == HEAD_BYTES {
        let text = String::from_utf8_lossy(&head);
        let opened = text.trim_start_matches('\u{feff}').starts_with("---");
        if opened && frontmatter::split(&text).block.is_none() {
            file.read_to_end(&mut head)?;
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_conflict_copies() {
        let cases = [
            ("Einkauf.md", false),
            ("Einkauf.sync-conflict-20261116-101200-ABCDEFG.md", true),
            ("Einkauf (conflicted copy 2026-11-16 101200).md", true),
            ("Einkauf (Alexs conflicted copy 2026-11-16).md", true),
            ("Einkauf (conflict).md", false),
            ("How to resolve a conflicted copy.md", false),
            ("Notes (2026) on a conflicted copy.md", false),
        ];
        for (name, conflict) in cases {
            assert_eq!(is_conflict_name(name), conflict, "{name}");
        }
    }

    #[test]
    fn local_timestamps_round_trip_to_the_second() {
        let time: Timestamp = "2026-11-16T09:12:00.75Z".parse().unwrap();
        let text = format_local(time);
        assert_eq!(text.len(), "2026-11-16T10:12:00+01:00".len());
        let parsed: Timestamp = text.parse().unwrap();
        assert_eq!(parsed, "2026-11-16T09:12:00Z".parse::<Timestamp>().unwrap());
    }

    #[test]
    fn long_frontmatter_is_read_past_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.md");
        let filler = "x: y\n".repeat(HEAD_BYTES / 4);
        let id = "01JD3K8Z6Q4X7V9B2M5N8P0R3T";
        fs::write(&path, format!("---\n{filler}id: {id}\n---\nBody\n")).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!(
            frontmatter::split(&head).frontmatter().id.unwrap().as_str(),
            id
        );

        fs::write(&path, format!("Title\n{}", "text\n".repeat(HEAD_BYTES))).unwrap();
        assert_eq!(read_head(&path).unwrap().len(), HEAD_BYTES);
    }
}
