//! The trash: `<notes>/.trash/`, where a deleted note waits with its attachments.
//!
//! A trashed file keeps its name and records in its frontmatter when it was deleted
//! (`deleted:`) and where it came from (`from:`).
//!
//! Attachments (`<folder>/_attachments/<id>/`) go to `.trash/_attachments/<id>/` with
//! their note, except files another note that is not in the trash links to: those
//! stay where that link points, until the last such note goes to the trash as well
//! and they follow. Restoring the note brings its files back together;
//! deleting it for good removes only files no such note links to, and puts the
//! others back beside the folder the note came from.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use jiff::{SignedDuration, Timestamp};

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::model::{NoteId, NoteMeta};
use crate::scan;
use crate::store::fsops;
use crate::store::write::{checked_folder, stamped};
use crate::store::{
    ATTACHMENTS_DIR, NOTE_EXT, Note, NoteStore, TRASH_DIR, format_local, join_rel, split_rel,
};

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
        self.move_attachment_files(id, TRASH_DIR, &folder, |_| true)?;
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

        let id = &note.meta.id;
        let source = self.attachments_path(&note.meta.folder, id);
        let linked = self.linked_elsewhere(id, &source, Some(&note.meta.rel_path))?;
        let dir = self.abs(TRASH_DIR);
        fs::create_dir_all(&dir)?;
        let (_, name) = split_rel(&note.meta.rel_path);
        let stem = name.strip_suffix(NOTE_EXT).unwrap_or(name);
        // The copy in the trash exists before the original goes, so a crash in between
        // leaves the note in both places rather than in neither.
        let name = fsops::write_new(&dir, stem, " ", NOTE_EXT, text.as_bytes())?;
        fs::remove_file(self.abs(&note.meta.rel_path))?;
        self.move_attachment_files(id, &note.meta.folder, TRASH_DIR, |name| {
            !linked.contains(name)
        })?;
        self.with_ids(|ids| ids.remove(id))?;
        for owner in linked_owners(frontmatter::split(&note.text).body, id) {
            self.follow_owner(&owner)?;
        }
        Ok(self.load(&join_rel(TRASH_DIR, &name))?.meta)
    }

    /// Moves the files of `owner`, if it is in the trash, that stayed behind for links
    /// and that no note outside the trash links to any more into the trash to it.
    fn follow_owner(&self, owner: &NoteId) -> Result<()> {
        let trashed = match self.trashed(owner) {
            Ok(trashed) => trashed,
            Err(Error::NotFound(_)) => return Ok(()),
            Err(err) => return Err(err),
        };
        let folder = checked_folder(&trashed.from).unwrap_or_default();
        let linked = self.linked_elsewhere(owner, &self.attachments_path(&folder, owner), None)?;
        self.move_attachment_files(owner, &folder, TRASH_DIR, |name| !linked.contains(name))
    }

    fn trashed(&self, id: &NoteId) -> Result<TrashedNote> {
        self.list_trash()?
            .into_iter()
            .find(|trashed| trashed.meta.id == *id)
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn remove_trashed(&self, trashed: &TrashedNote) -> Result<()> {
        let id = &trashed.meta.id;
        let dir = self.attachments_path(TRASH_DIR, id);
        let linked = self.linked_elsewhere(id, &dir, None)?;
        if !linked.is_empty() {
            let folder = checked_folder(&trashed.from).unwrap_or_default();
            self.move_attachment_files(id, TRASH_DIR, &folder, |name| linked.contains(name))?;
        }
        fs::remove_file(self.abs(&trashed.meta.rel_path))?;
        match fs::remove_dir_all(&dir) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
        remove_if_empty(dir.parent());
        Ok(())
    }

    /// The names of the files in `dir`, the attachments of `id`, that a note outside
    /// the trash links to, other than the one at `except`. The folder name holds the
    /// note's id, so any link whose target ends in `_attachments/<id>/<file>` means this
    /// file.
    fn linked_elsewhere(
        &self,
        id: &NoteId,
        dir: &Path,
        except: Option<&str>,
    ) -> Result<HashSet<String>> {
        let names = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect::<Vec<_>>(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
            Err(err) => return Err(err.into()),
        };
        let mut linked = HashSet::new();
        if names.is_empty() {
            return Ok(linked);
        }
        let prefix = format!("{ATTACHMENTS_DIR}/{id}/");
        for entry in self.walk()? {
            if Some(entry.rel_path.as_str()) == except {
                continue;
            }
            let text = match fs::read_to_string(self.abs(&entry.rel_path)) {
                Ok(text) => text,
                Err(err) => {
                    log::warn!("cannot look for links in {}: {err}", entry.rel_path);
                    continue;
                }
            };
            // The id is plain ASCII, so it shows in the text however the link is written.
            if !text.contains(id.as_str()) {
                continue;
            }
            let targets = scan::file_targets(frontmatter::split(&text).body);
            for name in &names {
                if links_to(&targets, &prefix, name) {
                    linked.insert(name.clone());
                }
            }
        }
        Ok(linked)
    }

    /// Moves the attachments of `id` from the folder `from` to the folder `to`, those
    /// whose name `moves` accepts. A file already at the target stays where it is.
    fn move_attachment_files(
        &self,
        id: &NoteId,
        from: &str,
        to: &str,
        moves: impl Fn(&str) -> bool,
    ) -> Result<()> {
        let source = self.attachments_path(from, id);
        let entries = match fs::read_dir(&source) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let target = self.attachments_path(to, id);
        for entry in entries {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !moves(&name) {
                continue;
            }
            let destination = target.join(&name);
            if destination.exists() {
                log::warn!("not moving {name}: {} exists", destination.display());
                continue;
            }
            fs::create_dir_all(&target)?;
            fs::rename(entry.path(), destination)?;
        }
        remove_if_empty(Some(&source));
        remove_if_empty(source.parent());
        Ok(())
    }
}

/// The other notes whose attachments `body` links to, by the id in
/// `_attachments/<id>/`.
fn linked_owners(body: &str, own: &NoteId) -> Vec<NoteId> {
    let mut owners = Vec::new();
    for target in scan::file_targets(body) {
        let mut parts = target.split('/');
        while let Some(part) = parts.next() {
            if part != ATTACHMENTS_DIR {
                continue;
            }
            if let Some(Ok(owner)) = parts.next().map(str::parse::<NoteId>)
                && owner != *own
                && !owners.contains(&owner)
            {
                owners.push(owner);
            }
            break;
        }
    }
    owners
}

/// Removes `dir` if it is an empty folder; fails quietly otherwise, as it should.
fn remove_if_empty(dir: Option<&Path>) {
    if let Some(dir) = dir {
        let _ = fs::remove_dir(dir);
    }
}

/// Whether one of the link `targets` is the file `name` under `prefix`, relative to
/// the note or to a folder above it.
fn links_to(targets: &[String], prefix: &str, name: &str) -> bool {
    let file = format!("{prefix}{name}");
    targets.iter().any(|target| {
        target
            .strip_suffix(&file)
            .is_some_and(|before| before.is_empty() || before.ends_with('/'))
    })
}
