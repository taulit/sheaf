//! The calls that change notes and folders.
//!
//! Every write goes through a temporary file and a rename. A note with a provisional id
//! gets a real one with the first change Sheaf makes to it, so callers must continue
//! with the id of the returned [`NoteMeta`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use super::fsops::{self, attachment_name, file_stem_for};
use super::{
    ATTACHMENTS_DIR, Fingerprint, NOTE_EXT, Note, NoteStore, edit, format_local, join_rel,
    split_rel,
};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::i18n::tr;
use crate::model::{NoteId, NoteMeta};
use crate::scan;

/// Where and how [`NoteStore::create`] stores a new note.
#[derive(Clone, Debug, Default)]
pub struct CreateOptions {
    /// Folder relative to the notes folder; empty for the root. Created if missing.
    pub folder: String,
    /// Tags to add. Those the text does not already carry go to the frontmatter.
    pub tags: Vec<String>,
    pub pinned: bool,
    /// The creation time to record; now if `None`.
    pub created: Option<Timestamp>,
}

/// How [`NoteStore::write_body`] stores a changed body.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteOptions {
    /// Rename the file if the title changed.
    pub rename: bool,
    /// The file as the caller last saw it. If it differs now, nothing is written and
    /// the call fails with [`Error::Conflict`].
    pub expected: Option<Fingerprint>,
}

/// Names a checklist item of a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskRef {
    /// Position among the tasks of the note, starting at 1.
    Number(u32),
    /// Text that exactly one task contains, compared without case.
    Text(String),
}

impl NoteStore {
    /// Creates a note from `text`, whose first line is the title.
    pub fn create(&self, text: &str, options: &CreateOptions) -> Result<NoteMeta> {
        let folder = checked_folder(&options.folder)?;
        let eol = frontmatter::line_ending(text);
        let id = NoteId::new();
        let created = options.created.unwrap_or_else(Timestamp::now);

        let mut text = text.to_owned();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(eol);
        }
        text = frontmatter::set(&text, "id", id.as_str());
        text = frontmatter::set(&text, "created", &format_local(created));
        if options.pinned {
            text = frontmatter::set(&text, "pinned", "true");
        }

        let parts = frontmatter::split(&text);
        let scan = scan::scan(parts.body);
        let stored = parts.frontmatter().tags;
        let mut tags = scan::merge_tags(&stored, &scan.tags);
        let known = tags.len();
        for tag in &options.tags {
            let tag = tag.trim().trim_start_matches('#');
            if !scan::is_tag(tag) {
                return Err(Error::Invalid(format!("not a tag: {tag}")));
            }
            tags = scan::merge_tags(&tags, &[tag.to_owned()]);
        }
        if tags.len() > known {
            let listed = scan::merge_tags(&stored, &tags[known..]);
            text = frontmatter::set(&text, "tags", &format!("[{}]", listed.join(", ")));
        }

        self.add(&folder, &file_stem_for(&scan.title), id, &text)
    }

    /// Replaces the body of a note and keeps its frontmatter.
    pub fn write_body(&self, id: &NoteId, body: &str, options: &WriteOptions) -> Result<NoteMeta> {
        let note = self.get(id)?;
        if options
            .expected
            .is_some_and(|seen| seen != note.fingerprint)
        {
            return Err(Error::Conflict(note.meta.rel_path));
        }
        let old_title = note.meta.title.clone();
        let text = frontmatter::join(note.head(), body);
        let note = self.edit(note, text)?;
        if options.rename && note.meta.title != old_title {
            self.follow_title(note.meta)
        } else {
            Ok(note.meta)
        }
    }

    /// Renames the file so that it follows the title.
    pub fn sync_filename(&self, id: &NoteId) -> Result<NoteMeta> {
        let note = self.stamp(self.get(id)?)?;
        self.follow_title(note.meta)
    }

    /// Adds `text` as new lines at the end of the note.
    pub fn append(&self, id: &NoteId, text: &str) -> Result<NoteMeta> {
        self.edit_body(id, |body, eol| edit::append(body, text, eol))
    }

    /// Adds `text` as new lines below the title line.
    pub fn prepend(&self, id: &NoteId, text: &str) -> Result<NoteMeta> {
        self.edit_body(id, |body, eol| edit::prepend(body, text, eol))
    }

    /// Pins or unpins a note. Only a pinned note carries the `pinned` key.
    pub fn set_pinned(&self, id: &NoteId, pinned: bool) -> Result<NoteMeta> {
        let note = self.get(id)?;
        let text = if pinned == note.frontmatter.pinned {
            note.text.clone()
        } else if pinned {
            frontmatter::set(&note.text, "pinned", "true")
        } else {
            frontmatter::remove(&note.text, "pinned")
        };
        Ok(self.edit(note, text)?.meta)
    }

    /// Checks or unchecks a checklist item. Only the marker changes.
    pub fn set_task(&self, id: &NoteId, task: &TaskRef, checked: bool) -> Result<NoteMeta> {
        let note = self.get(id)?;
        let task = edit::pick_task(&note.scan.tasks, task)?;
        match edit::set_task(note.body(), task, checked)? {
            Some(body) => {
                let text = frontmatter::join(note.head(), &body);
                Ok(self.edit(note, text)?.meta)
            }
            None => Ok(note.meta),
        }
    }

    /// Moves a note and its attachments to another folder, which is created if missing.
    pub fn move_to(&self, id: &NoteId, folder: &str) -> Result<NoteMeta> {
        let folder = checked_folder(folder)?;
        let note = self.stamp(self.get(id)?)?;
        let meta = note.meta;
        if meta.folder == folder {
            return Ok(meta);
        }
        let dir = self.abs(&folder);
        fs::create_dir_all(&dir)?;
        let (_, name) = split_rel(&meta.rel_path);
        let stem = name.strip_suffix(NOTE_EXT).unwrap_or(name);

        self.move_attachments(&meta.id, &meta.folder, &folder)?;
        match fsops::move_new(&self.abs(&meta.rel_path), &dir, stem, " ", NOTE_EXT) {
            Ok(name) => self.moved(&meta.id, &join_rel(&folder, &name)),
            Err(err) => {
                let _ = self.move_attachments(&meta.id, &folder, &meta.folder);
                Err(err.into())
            }
        }
    }

    /// Copies a note, with its attachments, into a new note in the same folder.
    pub fn duplicate(&self, id: &NoteId) -> Result<NoteMeta> {
        let note = self.get(id)?;
        let copy = NoteId::new();
        let mut text = frontmatter::set(&note.text, "id", copy.as_str());
        text = frontmatter::set(&text, "created", &format_local(Timestamp::now()));
        text = frontmatter::remove(&text, "pinned");

        let dir = self.abs(&note.meta.folder);
        let attachments = dir.join(ATTACHMENTS_DIR);
        let source = attachments.join(note.meta.id.as_str());
        if source.is_dir() {
            copy_files(&source, &attachments.join(copy.as_str()))?;
            text = text.replace(
                &format!("{ATTACHMENTS_DIR}/{}/", note.meta.id),
                &format!("{ATTACHMENTS_DIR}/{copy}/"),
            );
        }
        self.add(
            &note.meta.folder,
            &file_stem_for(&note.meta.title),
            copy,
            &text,
        )
    }

    /// Stores `body` as a separate note next to the original, named
    /// `<Title> (conflict).md`. For edits that lost against a change on disk.
    pub fn save_conflict_copy(&self, id: &NoteId, body: &str) -> Result<NoteMeta> {
        let note = self.get(id)?;
        let copy = NoteId::new();
        let mut text = frontmatter::join(note.head(), body);
        text = frontmatter::set(&text, "id", copy.as_str());
        if note.frontmatter.created.is_none() {
            text = frontmatter::set(&text, "created", &format_local(note.meta.created));
        }
        text = frontmatter::remove(&text, "pinned");

        let title = scan::scan(body).title;
        let stem = format!("{} ({})", file_stem_for(&title), tr("conflict"));
        self.add(&note.meta.folder, &stem, copy, &text)
    }

    /// Creates a folder, and its parents if missing.
    pub fn create_folder(&self, folder: &str) -> Result<()> {
        let folder = named_folder(folder)?;
        let dir = self.abs(&folder);
        if dir.exists() {
            return Err(Error::Exists(folder));
        }
        fs::create_dir_all(dir)?;
        Ok(())
    }

    /// Renames or moves a folder with everything in it.
    ///
    /// Notes with a provisional id get a new one, because it derives from the path.
    pub fn rename_folder(&self, from: &str, to: &str) -> Result<()> {
        let from = named_folder(from)?;
        let to = named_folder(to)?;
        if to.starts_with(&format!("{from}/")) {
            return Err(Error::Invalid(format!("cannot move {from} into itself")));
        }
        let source = self.abs(&from);
        let target = self.abs(&to);
        if !source.is_dir() {
            return Err(Error::NotFound(from));
        }
        if target.exists() {
            return Err(Error::Exists(to));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(source, target)?;
        self.refresh();
        Ok(())
    }

    /// Moves every note in the folder to the trash and removes the folder.
    ///
    /// Returns the trashed notes. Files that are not notes are never deleted, nor are
    /// notes that cannot be read or trashed; if any remain, the folder stays.
    pub fn delete_folder(&self, folder: &str, now: Timestamp) -> Result<Vec<NoteMeta>> {
        let folder = named_folder(folder)?;
        let dir = self.abs(&folder);
        if !dir.is_dir() {
            return Err(Error::NotFound(folder));
        }
        let inside = format!("{folder}/");
        let mut trashed = Vec::new();
        for entry in self.walk()? {
            if entry.rel_path.starts_with(&inside) {
                match self
                    .load(&entry.rel_path)
                    .and_then(|note| self.trash_note(note, now))
                {
                    Ok(meta) => trashed.push(meta),
                    Err(err) => log::warn!("{} stays in its folder: {err}", entry.rel_path),
                }
            }
        }
        remove_empty_dirs(&dir)?;
        Ok(trashed)
    }

    /// The folder that holds the attachments of a note: `<note folder>/_attachments/<id>/`.
    pub fn attachments_dir(&self, id: &NoteId) -> Result<PathBuf> {
        let rel_path = self.path_of(id)?;
        Ok(self.attachments_path(split_rel(&rel_path).0, id))
    }

    /// Stores a file as an attachment of the note.
    ///
    /// Returns the note, which has a real id from now on, and the target to use in a
    /// Markdown link or image. The file name is reduced to characters that need no
    /// escaping and numbered if it is taken.
    pub fn add_attachment(
        &self,
        id: &NoteId,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<(NoteMeta, String)> {
        let (meta, dir) = self.prepare_attachments(id)?;
        let link = store_attachment(&dir, &meta.id, file_name, &mut &bytes[..], &|| true)?;
        Ok((meta, link))
    }

    /// The first step of attaching files: gives the note its real id if it has none
    /// yet, and returns the note with the folder its attachments go to. The files are
    /// then copied with [`store_attachment`], which does not touch the note.
    pub fn prepare_attachments(&self, id: &NoteId) -> Result<(NoteMeta, PathBuf)> {
        let meta = self.stamp(self.get(id)?)?.meta;
        let dir = self.attachments_path(&meta.folder, &meta.id);
        Ok((meta, dir))
    }

    pub(crate) fn attachments_path(&self, folder: &str, id: &NoteId) -> PathBuf {
        self.abs(folder).join(ATTACHMENTS_DIR).join(id.as_str())
    }

    /// Moves the attachments of a note along with it, if it has any.
    pub(crate) fn move_attachments(&self, id: &NoteId, from: &str, to: &str) -> Result<()> {
        let source = self.attachments_path(from, id);
        if !source.is_dir() {
            return Ok(());
        }
        let target = self.attachments_path(to, id);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&source, &target)?;
        if let Some(parent) = source.parent() {
            // Fails while other notes still have attachments there, as it should.
            let _ = fs::remove_dir(parent);
        }
        Ok(())
    }

    /// Writes a new note file and returns its metadata.
    pub(crate) fn add(&self, folder: &str, stem: &str, id: NoteId, text: &str) -> Result<NoteMeta> {
        let dir = self.abs(folder);
        fs::create_dir_all(&dir)?;
        let name = fsops::write_new(&dir, stem, " ", NOTE_EXT, text.as_bytes())?;
        self.moved(&id, &join_rel(folder, &name))
    }

    /// Records the new path of a note and reads it there.
    fn moved(&self, id: &NoteId, rel_path: &str) -> Result<NoteMeta> {
        self.with_ids(|ids| ids.insert(id.clone(), rel_path.to_owned()))?;
        Ok(self.load(rel_path)?.meta)
    }

    fn edit_body(
        &self,
        id: &NoteId,
        change: impl FnOnce(&str, &str) -> String,
    ) -> Result<NoteMeta> {
        let note = self.get(id)?;
        let body = change(note.body(), frontmatter::line_ending(&note.text));
        let text = frontmatter::join(note.head(), &body);
        Ok(self.edit(note, text)?.meta)
    }

    /// Stores a changed text; an unchanged one is not written.
    fn edit(&self, note: Note, text: String) -> Result<Note> {
        if text == note.text {
            Ok(note)
        } else {
            self.rewrite(note, text)
        }
    }

    /// Gives a note with a provisional id a real one.
    pub(crate) fn stamp(&self, note: Note) -> Result<Note> {
        let text = note.text.clone();
        self.rewrite(note, text)
    }

    fn rewrite(&self, note: Note, text: String) -> Result<Note> {
        let (id, text) = stamped(&note, text);
        if text == note.text {
            return Ok(note);
        }
        let rel_path = &note.meta.rel_path;
        fsops::write_atomic(&self.abs(rel_path), text.as_bytes())?;
        self.with_ids(|ids| {
            ids.remove(&note.meta.id);
            ids.insert(id, rel_path.clone());
        })?;
        self.load(rel_path)
    }

    fn follow_title(&self, meta: NoteMeta) -> Result<NoteMeta> {
        let (folder, _) = split_rel(&meta.rel_path);
        let stem = file_stem_for(&meta.title);
        let name = fsops::move_new(
            &self.abs(&meta.rel_path),
            &self.abs(folder),
            &stem,
            " ",
            NOTE_EXT,
        )?;
        let rel_path = join_rel(folder, &name);
        if rel_path == meta.rel_path {
            Ok(meta)
        } else {
            self.moved(&meta.id, &rel_path)
        }
    }
}

/// `text` with a real id, and the creation time known so far, if the note has only a
/// provisional id.
pub(crate) fn stamped(note: &Note, text: String) -> (NoteId, String) {
    if !note.meta.id.is_provisional() {
        return (note.meta.id.clone(), text);
    }
    let id = NoteId::new();
    let mut text = frontmatter::set(&text, "id", id.as_str());
    if note.frontmatter.created.is_none() {
        text = frontmatter::set(&text, "created", &format_local(note.meta.created));
    }
    (id, text)
}

/// Copies what `source` reads into `dir`, the attachment folder of the note `id` from
/// [`NoteStore::prepare_attachments`], as a file named after `file_name`, and returns
/// the target to use in a Markdown link or image. The copy goes to a hidden file
/// first and gets its name only when it is complete; one that fails, or that is given
/// up because `go_on` says no, leaves nothing.
pub fn store_attachment(
    dir: &Path,
    id: &NoteId,
    file_name: &str,
    source: &mut dyn io::Read,
    go_on: &dyn Fn() -> bool,
) -> Result<String> {
    fs::create_dir_all(dir)?;
    let (stem, ext) = attachment_name(file_name);
    let name = fsops::copy_new(dir, &stem, "-", &ext, source, go_on)?;
    Ok(format!("{ATTACHMENTS_DIR}/{id}/{name}"))
}

/// A folder path in its canonical form: no leading or trailing `/`, empty for the root.
/// Paths that leave the notes folder or name something the store hides are refused.
pub fn checked_folder(folder: &str) -> Result<String> {
    let invalid = || Error::Invalid(format!("not a folder inside the notes folder: {folder}"));
    if folder.starts_with('/') {
        return Err(invalid());
    }
    let folder = folder.trim_end_matches('/');
    let usable = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part != ATTACHMENTS_DIR
            && !part.contains(|c: char| c.is_control() || c == '\\')
    };
    if folder.is_empty() || folder.split('/').all(usable) {
        Ok(folder.to_owned())
    } else {
        Err(invalid())
    }
}

/// Like [`checked_folder`], for calls that cannot act on the root.
fn named_folder(folder: &str) -> Result<String> {
    let folder = checked_folder(folder)?;
    if folder.is_empty() {
        Err(Error::Invalid(
            "the notes folder itself is not a folder to change".to_owned(),
        ))
    } else {
        Ok(folder)
    }
}

fn copy_files(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Removes `dir` and the folders below it as far as they are empty.
fn remove_empty_dirs(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_empty_dirs(&entry.path())?;
        }
    }
    match fs::remove_dir(dir) {
        Err(err) if err.kind() != io::ErrorKind::DirectoryNotEmpty => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_must_stay_inside_the_root() {
        for folder in ["", "Arbeit", "Arbeit/Q4", "Arbeit/", "with space/ä"] {
            assert_eq!(
                checked_folder(folder).unwrap(),
                folder.trim_end_matches('/')
            );
        }
        for folder in [
            "/abs",
            "..",
            "a/../b",
            "a//b",
            ".",
            ".trash",
            "a/.hidden",
            "_attachments",
            "a/_attachments/x",
            "a\\b",
            "a\nb",
        ] {
            assert!(
                matches!(checked_folder(folder), Err(Error::Invalid(_))),
                "{folder:?}"
            );
        }
        assert!(matches!(named_folder(""), Err(Error::Invalid(_))));
        assert!(matches!(named_folder("/"), Err(Error::Invalid(_))));
    }
}
