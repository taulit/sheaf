//! The trash: `<notes>/.trash/`, where a deleted note waits with its attachments.
//!
//! A trashed file keeps its name and records in its frontmatter when it was deleted
//! (`deleted:`) and where it came from (`from:`).

use std::fs;
use std::io;

use jiff::{SignedDuration, Timestamp};

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::model::{NoteId, NoteMeta};
use crate::store::fsops;
use crate::store::write::{checked_folder, stamped};
use crate::store::{NOTE_EXT, Note, NoteStore, TRASH_DIR, format_local, join_rel, split_rel};

/// How long a note stays in the trash before [`NoteStore::purge_expired`] removes it.
pub const TRASH_MAX_AGE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// A note in the trash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashedNote {
    pub meta: NoteMeta,
    pub deleted: Timestamp,
    /// The folder the note was in, relative to the notes folder; empty for the root.
    pub from: String,
}

impl NoteStore {
    /// Moves a note and its attachments to the trash.
    pub fn trash(&self, id: &NoteId, now: Timestamp) -> Result<NoteMeta> {
        self.trash_note(self.get(id)?, now)
    }

    /// The notes in the trash, most recently deleted first.
    pub fn list_trash(&self) -> Result<Vec<TrashedNote>> {
        let entries = match fs::read_dir(self.abs(TRASH_DIR)) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut trashed = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') || !name.ends_with(NOTE_EXT) || !entry.file_type()?.is_file() {
                continue;
            }
            match self.load(&join_rel(TRASH_DIR, &name)) {
                Ok(note) => trashed.push(TrashedNote {
                    deleted: note.frontmatter.deleted.unwrap_or(note.meta.modified),
                    from: note.frontmatter.from.unwrap_or_default(),
                    meta: note.meta,
                }),
                Err(err) => log::warn!("skipping {name} in the trash: {err}"),
            }
        }
        trashed.sort_by(|a, b| {
            let newest_first = b.deleted.cmp(&a.deleted);
            newest_first.then_with(|| a.meta.rel_path.cmp(&b.meta.rel_path))
        });
        Ok(trashed)
    }

    /// Puts a trashed note back into the folder it came from, creating it if needed.
    pub fn restore(&self, id: &NoteId) -> Result<NoteMeta> {
        let trashed = self.trashed(id)?;
        let note = self.load(&trashed.meta.rel_path)?;
        // A `from:` edited into something unusable sends the note to the root.
        let folder = checked_folder(&trashed.from).unwrap_or_default();
        let text = frontmatter::remove(&frontmatter::remove(&note.text, "deleted"), "from");
        let (_, name) = split_rel(&trashed.meta.rel_path);
        let stem = name.strip_suffix(NOTE_EXT).unwrap_or(name);

        let meta = self.add(&folder, stem, id.clone(), &text)?;
        fs::remove_file(self.abs(&trashed.meta.rel_path))?;
        self.move_attachments(id, TRASH_DIR, &folder)?;
        Ok(meta)
    }

    /// Deletes a trashed note and its attachments for good.
    pub fn purge(&self, id: &NoteId) -> Result<()> {
        let trashed = self.trashed(id)?;
        self.remove_trashed(&trashed)
    }

    /// Deletes every note that has been in the trash for `max_age` or longer, and
    /// returns what was removed.
    pub fn purge_expired(
        &self,
        now: Timestamp,
        max_age: SignedDuration,
    ) -> Result<Vec<TrashedNote>> {
        let mut removed = Vec::new();
        for trashed in self.list_trash()? {
            if now.duration_since(trashed.deleted) >= max_age {
                self.remove_trashed(&trashed)?;
                removed.push(trashed);
            }
        }
        Ok(removed)
    }

    pub(crate) fn trash_note(&self, note: Note, now: Timestamp) -> Result<NoteMeta> {
        let (_, mut text) = stamped(&note, note.text.clone());
        text = frontmatter::set(&text, "deleted", &format_local(now));
        text = frontmatter::set(&text, "from", &frontmatter::quoted(&note.meta.folder));

        let dir = self.abs(TRASH_DIR);
        fs::create_dir_all(&dir)?;
        let (_, name) = split_rel(&note.meta.rel_path);
        let stem = name.strip_suffix(NOTE_EXT).unwrap_or(name);
        // The copy in the trash exists before the original goes, so a crash in between
        // leaves the note in both places rather than in neither.
        let name = fsops::write_new(&dir, stem, " ", NOTE_EXT, text.as_bytes())?;
        fs::remove_file(self.abs(&note.meta.rel_path))?;
        self.move_attachments(&note.meta.id, &note.meta.folder, TRASH_DIR)?;
        self.with_ids(|ids| ids.remove(&note.meta.id))?;
        Ok(self.load(&join_rel(TRASH_DIR, &name))?.meta)
    }

    fn trashed(&self, id: &NoteId) -> Result<TrashedNote> {
        self.list_trash()?
            .into_iter()
            .find(|trashed| trashed.meta.id == *id)
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn remove_trashed(&self, trashed: &TrashedNote) -> Result<()> {
        fs::remove_file(self.abs(&trashed.meta.rel_path))?;
        match fs::remove_dir_all(self.attachments_path(TRASH_DIR, &trashed.meta.id)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        }
    }
}
