//! The worker of one open note: it loads the note, then does the view's [`Job`]s one
//! after the other, so that every write sees the file as the previous one left it.
//!
//! It also remembers the [`Fingerprint`] of the file as it last read or wrote it. A
//! change by another program is a different fingerprint on disk; the view's own
//! writes never look like one, however they interleave with the change events.

use async_channel::{Receiver, Sender};
use gpui::{AsyncApp, Entity, WeakEntity};
use sheaf_core::{CreateOptions, Error, Fingerprint, Note, NoteId, NoteMeta};

use super::NoteView;
use crate::app::library::Library;

/// A write or check of the open note.
#[derive(Debug)]
pub enum Job {
    /// Store this text. `rename` also lets the file name follow the title. `leaving`:
    /// the view is going away, so a conflict cannot be asked about and the text goes
    /// into a copy instead.
    Save {
        body: String,
        rename: bool,
        leaving: bool,
    },
    Rename,
    Pin(bool),
    Move(String),
    Trash,
    /// Delete a new note that was left empty.
    Discard,
    /// See whether another program changed or removed the file.
    Check,
    /// Replace the text with the file as it is on disk.
    Reload,
    /// Keep this text as a copy beside the note that changed on disk, and go on
    /// editing the copy.
    KeepMine(String),
    /// The note was deleted elsewhere: store this text as a new note in `folder`.
    SaveAsNew {
        body: String,
        folder: String,
    },
}

/// A job and who waits for it to be done.
pub struct Queued {
    pub job: Job,
    pub done: Option<Sender<()>>,
}

/// What the worker tells the view.
pub enum Report {
    Loaded(Note),
    /// The note was written; `saved` if this text is now on disk.
    Written {
        meta: NoteMeta,
        saved: bool,
    },
    /// A [`Job::Save`] is done, whatever came of it.
    SaveDone,
    /// The text went into another note, after "Keep Mine" or "Save as New Note"; the
    /// view goes on with that one.
    Resolved(NoteMeta),
    /// A save found the file changed on disk; nothing was written.
    Conflict,
    /// The file is gone.
    Deleted,
    Trashed(NoteMeta),
    /// Took the text from disk after [`Job::Reload`].
    Reloaded(Note),
}

struct Worker {
    view: WeakEntity<NoteView>,
    library: Entity<Library>,
    id: NoteId,
    /// The folder of the note, where a note deleted elsewhere is created again.
    folder: String,
    /// The file as this worker last read or wrote it.
    known: Option<Fingerprint>,
}

pub async fn work(
    view: WeakEntity<NoteView>,
    library: Entity<Library>,
    id: NoteId,
    queue: Receiver<Queued>,
    cx: &mut AsyncApp,
) {
    let mut worker = Worker {
        view,
        library,
        id,
        folder: String::new(),
        known: None,
    };
    match worker.load(cx).await {
        Ok(note) => {
            worker.adopt(&note);
            worker.report(Report::Loaded(note), cx);
        }
        Err(err) => {
            log::error!("cannot open the note {}: {err}", worker.id);
            worker.report(Report::Deleted, cx);
        }
    }
    while let Ok(Queued { job, done }) = queue.recv().await {
        let save = matches!(job, Job::Save { .. });
        worker.run(job, cx).await;
        if save {
            worker.report(Report::SaveDone, cx);
        }
        if let Some(done) = done {
            let _ = done.try_send(());
        }
    }
}

impl Worker {
    fn report(&self, report: Report, cx: &mut AsyncApp) {
        let _ = self
            .view
            .update(cx, |view, cx| view.worker_reported(report, cx));
    }

    async fn load(&self, cx: &mut AsyncApp) -> sheaf_core::Result<Note> {
        self.library
            .read_with(cx, |library, cx| library.load(&self.id, cx))
            .await
    }

    async fn run(&mut self, job: Job, cx: &mut AsyncApp) {
        let result = match job {
            Job::Save {
                body,
                rename,
                leaving,
            } => self.save(body, rename, leaving, cx).await,
            Job::Rename => {
                if !self.still_ours(cx).await {
                    return;
                }
                let id = self.id.clone();
                let renamed = self
                    .library
                    .update(cx, |library, cx| library.sync_filename(&id, cx))
                    .await;
                self.wrote(renamed, cx).await
            }
            Job::Pin(pinned) => {
                if !self.still_ours(cx).await {
                    return;
                }
                let id = self.id.clone();
                let written = self
                    .library
                    .update(cx, |library, cx| library.set_pinned(&id, pinned, cx))
                    .await;
                self.wrote(written, cx).await
            }
            Job::Move(folder) => {
                if !self.still_ours(cx).await {
                    return;
                }
                let id = self.id.clone();
                let moved = self
                    .library
                    .update(cx, |library, cx| library.move_to(&id, &folder, cx))
                    .await;
                self.wrote(moved, cx).await
            }
            Job::Trash => {
                let id = self.id.clone();
                let trashed = self
                    .library
                    .update(cx, |library, cx| library.trash(&id, cx))
                    .await;
                trashed.map(|meta| self.report(Report::Trashed(meta), cx))
            }
            Job::Discard => {
                let id = self.id.clone();
                self.library
                    .update(cx, |library, cx| library.discard(&id, cx))
                    .await
                    .map(drop)
            }
            Job::Check => {
                self.still_ours(cx).await;
                Ok(())
            }
            Job::Reload => match self.load(cx).await {
                Ok(note) => {
                    self.adopt(&note);
                    self.report(Report::Reloaded(note), cx);
                    Ok(())
                }
                Err(err) => Err(err),
            },
            Job::KeepMine(body) => self.keep_mine(body, cx).await,
            Job::SaveAsNew { body, folder } => self.save_as_new(body, folder, cx).await,
        };
        if let Err(err) = result {
            log::error!("cannot write the note {}: {err}", self.id);
        }
    }

    /// Stores `body` if the file is as this worker last saw it.
    async fn save(
        &mut self,
        body: String,
        rename: bool,
        leaving: bool,
        cx: &mut AsyncApp,
    ) -> sheaf_core::Result<()> {
        let id = self.id.clone();
        let expected = self.known;
        let saved = self
            .library
            .update(cx, |library, cx| {
                library.save_body(&id, body.clone(), expected, cx)
            })
            .await;
        match saved {
            Ok(saved) => {
                self.took(&saved.meta);
                self.known = Some(saved.fingerprint);
                let meta = if rename {
                    let id = self.id.clone();
                    let renamed = self
                        .library
                        .update(cx, |library, cx| library.sync_filename(&id, cx))
                        .await?;
                    self.refresh(&renamed, cx).await;
                    renamed
                } else {
                    saved.meta
                };
                self.report(Report::Written { meta, saved: true }, cx);
                Ok(())
            }
            Err(Error::Conflict(_)) if leaving => {
                log::warn!("the note {id} changed on disk, keeping the edits as a copy");
                self.keep_mine(body, cx).await
            }
            Err(Error::Conflict(_)) => {
                self.report(Report::Conflict, cx);
                Ok(())
            }
            Err(Error::NotFound(_)) if leaving => {
                let folder = self.folder.clone();
                self.save_as_new(body, folder, cx).await
            }
            Err(Error::NotFound(_)) => {
                self.report(Report::Deleted, cx);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Whether the file is as this worker last saw it. If not, the view hears that it
    /// changed or is gone, and takes the text from disk if it has nothing unsaved.
    async fn still_ours(&mut self, cx: &mut AsyncApp) -> bool {
        let id = self.id.clone();
        let now = self
            .library
            .read_with(cx, |library, cx| library.fingerprint(&id, cx))
            .await;
        match now {
            Ok(now) if Some(now) == self.known => true,
            Ok(_) => {
                let Ok(note) = self.load(cx).await else {
                    return false;
                };
                let reloaded = self
                    .view
                    .update(cx, |view, cx| view.changed_on_disk(&note, cx))
                    .unwrap_or(false);
                if reloaded {
                    self.adopt(&note);
                }
                reloaded
            }
            Err(Error::NotFound(_)) => {
                self.report(Report::Deleted, cx);
                false
            }
            Err(err) => {
                log::warn!("cannot look at the note {id}: {err}");
                false
            }
        }
    }

    async fn keep_mine(&mut self, body: String, cx: &mut AsyncApp) -> sheaf_core::Result<()> {
        let id = self.id.clone();
        let copy = self
            .library
            .update(cx, |library, cx| library.save_conflict_copy(&id, body, cx))
            .await?;
        self.switch_to(copy, cx).await;
        Ok(())
    }

    async fn save_as_new(
        &mut self,
        body: String,
        folder: String,
        cx: &mut AsyncApp,
    ) -> sheaf_core::Result<()> {
        let options = CreateOptions {
            folder,
            ..CreateOptions::default()
        };
        let created = self
            .library
            .update(cx, |library, cx| library.create(&body, options, cx))
            .await?;
        self.switch_to(created, cx).await;
        Ok(())
    }

    /// Goes on with another note that holds the view's text.
    async fn switch_to(&mut self, meta: NoteMeta, cx: &mut AsyncApp) {
        self.took(&meta);
        self.refresh(&meta, cx).await;
        self.report(Report::Resolved(meta), cx);
    }

    /// Takes in the result of a write that changed the file but not its text.
    async fn wrote(
        &mut self,
        written: sheaf_core::Result<NoteMeta>,
        cx: &mut AsyncApp,
    ) -> sheaf_core::Result<()> {
        let meta = written?;
        self.took(&meta);
        self.refresh(&meta, cx).await;
        self.report(Report::Written { meta, saved: false }, cx);
        Ok(())
    }

    /// Every write changes the file; the next one must expect it as it is now.
    async fn refresh(&mut self, meta: &NoteMeta, cx: &mut AsyncApp) {
        let fingerprint = self
            .library
            .read_with(cx, |library, cx| library.fingerprint(&meta.id, cx))
            .await;
        self.known = fingerprint.ok();
    }

    /// Takes the text of `note` as the one the view shows.
    fn adopt(&mut self, note: &Note) {
        self.took(&note.meta);
        self.known = Some(note.fingerprint);
    }

    fn took(&mut self, meta: &NoteMeta) {
        self.id = meta.id.clone();
        self.folder = meta.folder.clone();
    }
}
