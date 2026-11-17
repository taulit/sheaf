//! The worker of one open note: it loads the note, then does the view's [`Job`]s one
//! after the other, so that every write sees the file as the previous one left it.
//!
//! What the worker knows about the file is [`Known`]: the note's id and folder, and the
//! [`Fingerprint`] of the file as it was last read or written here. A change by another
//! program is a different fingerprint on disk; the view's own writes never look like
//! one, however they interleave with the change events.
//!
//! `Known` is shared with the view's last save ([`NoteView::save_now`]), which runs on
//! the UI thread when the window closes or the app quits, possibly while a write of the
//! worker is under way on another thread. So each step holds the lock from reading the
//! file to recording what it wrote, and nothing is written once the note is `finished`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_channel::{Receiver, Sender};
use gpui::{AsyncApp, Entity, WeakEntity};
use jiff::Timestamp;
use sheaf_core::i18n::tr;
use sheaf_core::{
    CreateOptions, Error, Fingerprint, Note, NoteId, NoteMeta, NoteStore, WriteOptions,
};

use super::NoteView;
use crate::app::library::{Library, Touched};

/// [`Known`], as the worker and the view's last save share it.
pub type Shared = Arc<Mutex<Known>>;

/// The note's file as the worker last read or wrote it.
#[derive(Debug)]
pub struct Known {
    id: NoteId,
    /// The folder of the note, where a note deleted elsewhere is created again.
    folder: String,
    fingerprint: Option<Fingerprint>,
    /// Left, trashed or saved for the last time: nothing more is written.
    pub finished: bool,
}

pub fn shared(id: NoteId) -> Shared {
    Arc::new(Mutex::new(Known {
        id,
        folder: String::new(),
        fingerprint: None,
        finished: false,
    }))
}

pub fn lock(shared: &Shared) -> MutexGuard<'_, Known> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A write or check of the open note.
#[derive(Debug)]
pub enum Job {
    /// Store this text. `rename` also lets the file name follow the title.
    Save {
        body: String,
        rename: bool,
    },
    Rename,
    Pin(bool),
    Move(String),
    /// Move the note to the trash at `now`, after storing `body` if there is one. If
    /// that finds the file changed or gone, nothing goes to the trash.
    Trash {
        body: Option<String>,
        now: Timestamp,
    },
    /// See whether another program changed or removed the file.
    Check,
    /// Replace the text with the file as it is on disk.
    Reload,
    /// Keep this text as a copy beside the note that changed on disk, and go on
    /// editing the copy.
    KeepMine(String),
    /// The note was deleted elsewhere: store this text as a new note in its folder.
    SaveAsNew(String),
    /// Copy these files into the note's attachment folder; `done` hears the links.
    Attach {
        files: Vec<Upload>,
        done: Sender<Attached>,
    },
}

/// A file to attach: one on disk, or bytes such as a pasted image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upload {
    File(std::path::PathBuf),
    Bytes { name: String, bytes: Vec<u8> },
}

/// What came of [`Job::Attach`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attached {
    /// The name and link target of each file stored, in order.
    pub links: Vec<(String, String)>,
    /// Why files could not be stored.
    pub failed: Vec<String>,
}

/// What leaving a note stores. Whatever it is, it does not end without the text on
/// disk: it fails instead, and the view stays.
#[derive(Debug)]
pub enum Leave {
    /// Only let the file name follow the title.
    Rename,
    /// Store the text; if the file changed on disk, as a copy beside it, and if the
    /// file is gone, as a new note.
    Save(String),
    /// "Keep Mine" for a note that changed on disk.
    KeepMine(String),
    /// The note was deleted elsewhere: store the text as a new note, unless the note
    /// is back.
    SaveAsNew(String),
    /// A new note left empty, at `0`: deleted if the file is as the view last wrote it,
    /// whatever text that had, as the user deleted it since; if another program
    /// changed it, kept if it has text and trashed otherwise.
    Discard(Timestamp),
}

/// What the view hands the worker.
pub enum Queued {
    Job(Job),
    /// The view is going away; `done` hears whether everything is stored.
    Leave {
        leave: Leave,
        done: Sender<bool>,
    },
}

/// What the worker tells the view.
pub enum Report {
    Loaded(Note),
    /// `body` is on disk, in the note `meta`.
    Saved {
        meta: NoteMeta,
        body: String,
    },
    /// The file changed but not its text: renamed, pinned or moved.
    Written(NoteMeta),
    /// The text went into another note, after "Keep Mine" or "Save as New Note"; the
    /// view goes on with that one.
    Resolved {
        meta: NoteMeta,
        body: String,
    },
    /// A save found the file changed on disk; nothing was written.
    Conflict,
    /// The file is gone.
    Deleted,
    Trashed(NoteMeta),
    /// Took the text from disk after [`Job::Reload`].
    Reloaded(Note),
    /// The text could not be stored, for this reason.
    SaveFailed(String),
    /// The note could not be moved to the trash, for this reason.
    TrashFailed(String),
    /// A pin or move did not happen, for this reason.
    ChangeFailed {
        change: Change,
        reason: String,
    },
}

/// A write that changes the file but not its text.
#[derive(Clone, Debug)]
pub enum Change {
    Rename,
    /// Pin, or unpin with `false`.
    Pin(bool),
    Move,
    Attach,
}

/// Whether the file is as the worker last saw it.
enum Ours {
    Yes,
    /// No, for this reason; the view knows.
    No(String),
    /// The note is finished; nothing more is written.
    Finished,
}

pub async fn work(
    view: WeakEntity<NoteView>,
    library: Entity<Library>,
    shared: Shared,
    queue: Receiver<Queued>,
    cx: &mut AsyncApp,
) {
    let worker = Worker {
        view,
        library,
        shared,
    };
    match worker.step(cx, |known, store, _| known.load(store)).await {
        Ok(Some(note)) => worker.report(Report::Loaded(note), cx),
        Ok(None) => {}
        Err(err) => {
            log::error!("cannot open the note: {err}");
            worker.report(Report::Deleted, cx);
        }
    }
    while let Ok(queued) = queue.recv().await {
        match queued {
            Queued::Job(job) => {
                if let Some(report) = worker.run(job, cx).await {
                    worker.report(report, cx);
                }
            }
            Queued::Leave { leave, done } => {
                let left = worker
                    .step(cx, move |known, store, touched| {
                        known.leave(store, touched, leave)
                    })
                    .await;
                if let Err(err) = &left {
                    worker.report(Report::SaveFailed(reason(err)), cx);
                }
                let _ = done.try_send(left.is_ok());
            }
        }
    }
}

struct Worker {
    view: WeakEntity<NoteView>,
    library: Entity<Library>,
    shared: Shared,
}

impl Worker {
    fn report(&self, report: Report, cx: &mut AsyncApp) {
        let _ = self
            .view
            .update(cx, |view, cx| view.worker_reported(report, cx));
    }

    /// Runs `step` on the store in the background, holding the lock; `None` if the
    /// note is finished and nothing ran.
    async fn step<T: Send + 'static>(
        &self,
        cx: &mut AsyncApp,
        step: impl FnOnce(&mut Known, &NoteStore, &mut Touched) -> sheaf_core::Result<T>
        + Send
        + 'static,
    ) -> sheaf_core::Result<Option<T>> {
        let shared = self.shared.clone();
        self.library
            .update(cx, |library, cx| {
                library.run(cx, move |store, touched| {
                    let mut known = lock(&shared);
                    if known.finished {
                        return Ok(None);
                    }
                    step(&mut known, store, touched).map(Some)
                })
            })
            .await
    }

    async fn run(&self, job: Job, cx: &mut AsyncApp) -> Option<Report> {
        match job {
            Job::Save { body, rename } => {
                let saved = self
                    .step(cx, move |known, store, touched| {
                        known
                            .save(store, touched, &body, rename)
                            .map(|saved| (saved, body))
                    })
                    .await;
                match saved {
                    Ok(Some((Saved::Written(meta), body))) => Some(Report::Saved { meta, body }),
                    Ok(Some((Saved::Conflict, _))) => Some(Report::Conflict),
                    Ok(Some((Saved::Deleted, _))) => Some(Report::Deleted),
                    Ok(None) => None,
                    Err(err) => Some(Report::SaveFailed(reason(&err))),
                }
            }
            Job::Rename => {
                self.change(cx, Change::Rename, |store, id| store.sync_filename(id))
                    .await
            }
            Job::Pin(pinned) => {
                self.change(cx, Change::Pin(pinned), move |store, id| {
                    store.set_pinned(id, pinned)
                })
                .await
            }
            Job::Move(folder) => {
                self.change(cx, Change::Move, move |store, id| {
                    store.move_to(id, &folder)
                })
                .await
            }
            Job::Trash { body, now } => {
                let trashed = self
                    .step(cx, move |known, store, touched| {
                        known.trash(store, touched, body.as_deref(), now)
                    })
                    .await;
                match trashed {
                    Ok(Some(Trashing::Done(meta))) => Some(Report::Trashed(meta)),
                    Ok(Some(Trashing::Conflict)) => Some(Report::Conflict),
                    Ok(Some(Trashing::Deleted)) => Some(Report::Deleted),
                    Ok(None) => None,
                    Err(err) => Some(Report::TrashFailed(reason(&err))),
                }
            }
            Job::Check => {
                self.still_ours(cx).await;
                None
            }
            Job::Reload => match self.step(cx, |known, store, _| known.load(store)).await {
                Ok(note) => note.map(Report::Reloaded),
                Err(err) => {
                    log::error!("cannot reload the note: {err}");
                    None
                }
            },
            Job::KeepMine(body) => {
                self.resolve(cx, body, |known, store, touched, body| {
                    known.keep_mine(store, touched, body)
                })
                .await
            }
            Job::Attach { files, done } => {
                let attached = Arc::new(Mutex::new(Attached::default()));
                let sink = attached.clone();
                let report = self
                    .change(cx, Change::Attach, move |store, id| {
                        attach(store, id, &files, &sink)
                    })
                    .await;
                let attached =
                    std::mem::take(&mut *attached.lock().unwrap_or_else(PoisonError::into_inner));
                let _ = done.try_send(attached);
                report
            }
            Job::SaveAsNew(body) => {
                self.resolve(cx, body, |known, store, touched, body| {
                    known.create(store, touched, body)
                })
                .await
            }
        }
    }

    /// A write that changes the file but not its text, if the file is as the worker
    /// last saw it.
    async fn change(
        &self,
        cx: &mut AsyncApp,
        kind: Change,
        change: impl FnOnce(&NoteStore, &NoteId) -> sheaf_core::Result<NoteMeta> + Send + 'static,
    ) -> Option<Report> {
        let reason = match self.still_ours(cx).await {
            Ours::Yes => {
                let changed = self
                    .step(cx, move |known, store, touched| {
                        known.change(store, touched, change)
                    })
                    .await;
                match changed {
                    Ok(meta) => return meta.map(Report::Written),
                    Err(err) => {
                        log::error!("cannot write the note: {err}");
                        reason(&err)
                    }
                }
            }
            Ours::No(reason) => reason,
            Ours::Finished => return None,
        };
        Some(Report::ChangeFailed {
            change: kind,
            reason,
        })
    }

    /// "Keep Mine" or "Save as New Note": the text goes into another note.
    async fn resolve(
        &self,
        cx: &mut AsyncApp,
        body: String,
        resolve: impl FnOnce(&mut Known, &NoteStore, &mut Touched, &str) -> sheaf_core::Result<NoteMeta>
        + Send
        + 'static,
    ) -> Option<Report> {
        let resolved = self
            .step(cx, move |known, store, touched| {
                resolve(known, store, touched, &body).map(|meta| (meta, body))
            })
            .await;
        match resolved {
            Ok(Some((meta, body))) => Some(Report::Resolved { meta, body }),
            Ok(None) => None,
            Err(err) => Some(Report::SaveFailed(reason(&err))),
        }
    }

    /// Whether the file is as the worker last saw it. If not, the view hears that it
    /// changed or is gone, and takes the text from disk if it has nothing unsaved.
    async fn still_ours(&self, cx: &mut AsyncApp) -> Ours {
        match self.step(cx, |known, store, _| known.check(store)).await {
            Ok(Some(Checked::Same)) => Ours::Yes,
            Ok(Some(Checked::Changed(note))) => {
                let taken = self
                    .view
                    .update(cx, |view, cx| view.changed_on_disk(&note, cx))
                    .unwrap_or(false);
                let mut known = lock(&self.shared);
                if known.finished {
                    Ours::Finished
                } else if taken {
                    known.adopt(&note);
                    Ours::Yes
                } else {
                    Ours::No(tr("the file changed elsewhere"))
                }
            }
            Ok(Some(Checked::Deleted)) => {
                self.report(Report::Deleted, cx);
                Ours::No(tr("the file was deleted elsewhere"))
            }
            Ok(None) => Ours::Finished,
            Err(err) => {
                log::warn!("cannot look at the note: {err}");
                Ours::No(reason(&err))
            }
        }
    }
}

/// Stores `files` as attachments of the note `id`. Files that cannot be read are
/// noted in `attached` and skipped; it fails only if none could be stored.
fn attach(
    store: &NoteStore,
    id: &NoteId,
    files: &[Upload],
    attached: &Mutex<Attached>,
) -> sheaf_core::Result<NoteMeta> {
    let mut attached = attached.lock().unwrap_or_else(PoisonError::into_inner);
    let mut id = id.clone();
    let mut last = None;
    let mut first_error = None;
    for file in files {
        let read = match file {
            Upload::File(path) => std::fs::read(path)
                .map(|bytes| {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (name, bytes)
                })
                .map_err(Error::from),
            Upload::Bytes { name, bytes } => Ok((name.clone(), bytes.clone())),
        };
        match read.and_then(|(name, bytes)| {
            store
                .add_attachment(&id, &name, &bytes)
                .map(|(meta, link)| (name, meta, link))
        }) {
            Ok((name, meta, link)) => {
                id = meta.id.clone();
                attached.links.push((name, link));
                last = Some(meta);
            }
            Err(err) => {
                attached.failed.push(reason(&err));
                first_error.get_or_insert(err);
            }
        }
    }
    match (last, first_error) {
        (Some(meta), _) => Ok(meta),
        (None, Some(err)) => Err(err),
        (None, None) => Ok(store.get(&id)?.meta),
    }
}

/// What came of a save.
enum Saved {
    Written(NoteMeta),
    /// The file changed on disk; nothing was written.
    Conflict,
    /// The file is gone; nothing was written.
    Deleted,
}

enum Trashing {
    Done(NoteMeta),
    Conflict,
    Deleted,
}

enum Checked {
    Same,
    Changed(Box<Note>),
    Deleted,
}

impl Known {
    fn load(&mut self, store: &NoteStore) -> sheaf_core::Result<Note> {
        let note = store.get(&self.id)?;
        self.adopt(&note);
        Ok(note)
    }

    /// Takes the text of `note` as the one the view shows.
    fn adopt(&mut self, note: &Note) {
        self.id = note.meta.id.clone();
        self.folder = note.meta.folder.clone();
        self.fingerprint = Some(note.fingerprint);
    }

    /// Takes in the note as a write left it; the next write must expect it so.
    fn took(&mut self, store: &NoteStore, meta: &NoteMeta) {
        self.id = meta.id.clone();
        self.folder = meta.folder.clone();
        self.fingerprint = store.fingerprint(&meta.id).ok();
    }

    /// The note with the path it has now.
    fn place(&self, store: &NoteStore) -> sheaf_core::Result<(NoteId, String)> {
        Ok((self.id.clone(), store.path_of(&self.id)?))
    }

    /// Stores `body` if the file is as last seen here; with `rename`, the file name
    /// then follows the title. A failed rename leaves the text saved.
    fn save(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        body: &str,
        rename: bool,
    ) -> sheaf_core::Result<Saved> {
        let before = match self.place(store) {
            Ok(before) => before,
            Err(Error::NotFound(_)) => return Ok(Saved::Deleted),
            Err(err) => return Err(err),
        };
        let options = WriteOptions {
            rename: false,
            expected: self.fingerprint,
        };
        let meta = match store.write_body(&self.id, body, &options) {
            Ok(meta) => meta,
            Err(Error::Conflict(_)) => return Ok(Saved::Conflict),
            Err(Error::NotFound(_)) => return Ok(Saved::Deleted),
            Err(err) => return Err(err),
        };
        touched.wrote(Some(before), &meta);
        self.took(store, &meta);
        if !rename {
            return Ok(Saved::Written(meta));
        }
        match self.change(store, touched, |store, id| store.sync_filename(id)) {
            Ok(renamed) => Ok(Saved::Written(renamed)),
            Err(err) => {
                log::warn!("cannot rename the note {}: {err}", meta.id);
                Ok(Saved::Written(meta))
            }
        }
    }

    fn change(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        change: impl FnOnce(&NoteStore, &NoteId) -> sheaf_core::Result<NoteMeta>,
    ) -> sheaf_core::Result<NoteMeta> {
        let before = self.place(store)?;
        let meta = change(store, &self.id)?;
        touched.wrote(Some(before), &meta);
        self.took(store, &meta);
        Ok(meta)
    }

    fn keep_mine(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        body: &str,
    ) -> sheaf_core::Result<NoteMeta> {
        let copy = store.save_conflict_copy(&self.id, body)?;
        touched.wrote(None, &copy);
        self.took(store, &copy);
        Ok(copy)
    }

    /// Stores `body` as a new note in the folder of this one.
    fn create(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        body: &str,
    ) -> sheaf_core::Result<NoteMeta> {
        let options = CreateOptions {
            folder: self.folder.clone(),
            ..CreateOptions::default()
        };
        let created = store.create(body, &options)?;
        touched.wrote(None, &created);
        self.took(store, &created);
        Ok(created)
    }

    fn trash(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        body: Option<&str>,
        now: Timestamp,
    ) -> sheaf_core::Result<Trashing> {
        if let Some(body) = body {
            match self.save(store, touched, body, false)? {
                Saved::Written(_) => {}
                Saved::Conflict => return Ok(Trashing::Conflict),
                Saved::Deleted => return Ok(Trashing::Deleted),
            }
        }
        let (id, rel_path) = self.place(store)?;
        let trashed = store.trash(&id, now)?;
        touched.removed(id, rel_path);
        touched.tell(trashed.id.clone());
        self.finished = true;
        Ok(Trashing::Done(trashed))
    }

    fn check(&mut self, store: &NoteStore) -> sheaf_core::Result<Checked> {
        let now = match store.fingerprint(&self.id) {
            Ok(now) => now,
            Err(Error::NotFound(_)) => return Ok(Checked::Deleted),
            Err(err) => return Err(err),
        };
        if Some(now) == self.fingerprint {
            return Ok(Checked::Same);
        }
        match store.get(&self.id) {
            Ok(note) => Ok(Checked::Changed(Box::new(note))),
            Err(Error::NotFound(_)) => Ok(Checked::Deleted),
            Err(err) => Err(err),
        }
    }

    /// Does what leaving stores, see [`Leave`]; the note is finished afterwards.
    pub fn leave(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        leave: Leave,
    ) -> sheaf_core::Result<()> {
        // Only storing the text can fail: a file name or an empty note left behind
        // loses nothing.
        match leave {
            Leave::Rename => {
                if let Err(err) = self.change(store, touched, |store, id| store.sync_filename(id)) {
                    log::warn!("cannot rename the note {}: {err}", self.id);
                }
            }
            Leave::Save(body) => self.save_or_keep(store, touched, &body)?,
            Leave::KeepMine(body) => {
                self.keep_mine(store, touched, &body)?;
            }
            Leave::SaveAsNew(body) => match store.get(&self.id) {
                Ok(note) if note.body() == body => {}
                Ok(_) => self.save_or_keep(store, touched, &body)?,
                Err(Error::NotFound(_)) => {
                    self.create(store, touched, &body)?;
                }
                Err(err) => return Err(err),
            },
            Leave::Discard(now) => {
                if let Err(err) = self.discard(store, touched, now) {
                    log::warn!("cannot delete the empty note {}: {err}", self.id);
                }
            }
        }
        self.finished = true;
        Ok(())
    }

    /// Stores `body` whatever happened to the file: as a copy beside a note that
    /// changed on disk, as a new note in place of one that is gone.
    fn save_or_keep(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        body: &str,
    ) -> sheaf_core::Result<()> {
        match self.save(store, touched, body, true)? {
            Saved::Written(_) => {}
            Saved::Conflict => {
                log::warn!(
                    "the note {} changed on disk, keeping the edits as a copy",
                    self.id
                );
                self.keep_mine(store, touched, body)?;
            }
            Saved::Deleted => {
                self.create(store, touched, body)?;
            }
        }
        Ok(())
    }

    fn discard(
        &mut self,
        store: &NoteStore,
        touched: &mut Touched,
        now: Timestamp,
    ) -> sheaf_core::Result<()> {
        let note = match store.get(&self.id) {
            Ok(note) => note,
            Err(Error::NotFound(_)) => return Ok(()),
            Err(err) => return Err(err),
        };
        let untouched = self.fingerprint == Some(note.fingerprint);
        if !untouched && !note.body().trim().is_empty() {
            log::info!(
                "keeping the new note {}: it has text from elsewhere",
                self.id
            );
            return Ok(());
        }
        let trashed = store.trash(&note.meta.id, now)?;
        touched.removed(note.meta.id, note.meta.rel_path);
        touched.tell(trashed.id.clone());
        if untouched {
            store.purge(&trashed.id)?;
        }
        Ok(())
    }
}

/// Why a write failed, for the banner: the system's words for a file error, without
/// its number.
fn reason(err: &Error) -> String {
    match err {
        Error::Io(err) => err.kind().to_string(),
        err => err.to_string(),
    }
}
