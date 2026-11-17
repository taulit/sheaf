//! The notes as the views see them. [`Library`] is the only way a view reads or
//! changes notes: it owns the notes folder, a reader of the search index and the link
//! to `sheaf-searchd`.
//!
//! # For views
//!
//! - **Read** with [`notes`](Library::notes), [`search`](Library::search),
//!   [`folders`](Library::folders), [`tags`](Library::tags), [`meta`](Library::meta),
//!   [`body`](Library::body) (text as indexed, for previews),
//!   [`backlinks`](Library::backlinks) (notes that link to a title),
//!   [`load`](Library::load) (the file, with the fingerprint a later save needs) and
//!   [`folder_config`](Library::folder_config) (what is remembered per folder).
//! - **Change** with [`create`](Library::create), [`save_body`](Library::save_body),
//!   [`sync_filename`](Library::sync_filename), [`set_pinned`](Library::set_pinned),
//!   [`trash`](Library::trash), [`restore`](Library::restore),
//!   [`move_to`](Library::move_to) and [`duplicate`](Library::duplicate). Each
//!   resolves to the note as it is afterwards. Its id can differ from the one passed
//!   in: a file Sheaf never wrote gets its real id on the first write, so keep using
//!   the returned one.
//!   [`create_folder`](Library::create_folder) and
//!   [`update_folder_config`](Library::update_folder_config) change no note.
//! - **Several steps on the store** in one go, such as a save with its fallbacks:
//!   [`run`](Library::run), which says what it touched in a [`Touched`]. The open
//!   note's last save when the window closes or the app quits uses
//!   [`run_now`](Library::run_now), the one call that blocks.
//! - **Per-note view state** (column widths, folded headings) with
//!   [`note_state`](Library::note_state) and
//!   [`update_folder_config`](Library::update_folder_config); it lives in the folder's
//!   `.sheaf/config.toml`, not in the note.
//! - **Trash and templates**: [`trashed`](Library::trashed),
//!   [`trashed_note`](Library::trashed_note), [`purge`](Library::purge),
//!   [`empty_trash`](Library::empty_trash); [`templates`](Library::templates),
//!   [`create_from_template`](Library::create_from_template) and
//!   [`templates_dir`](Library::templates_dir).
//! - **Follow** changes by subscribing to [`LibraryEvent`]. When a change task
//!   resolves, the lists already show it: with `sheaf-searchd`, the app's own writes
//!   are read from an overlay until the service indexed them (`overlay`).
//!
//! Every method but [`run_now`](Library::run_now) returns a [`Task`]: file and index
//! work runs on the background executor, never on the UI thread. Dropping the task of
//! a change does not cancel it.
//!
//! # Modes
//!
//! Normally the index on disk is read, which `sheaf-searchd` writes
//! ([`LibraryMode::Service`]). If the service cannot be reached (no session bus, tests,
//! screenshots), the Library indexes the folder in memory and keeps that index up to
//! date after its own writes ([`LibraryMode::Local`]); changes made by other programs
//! are then not seen until the next start.

mod overlay;
mod service;
mod templates;
mod trash;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use async_channel::Receiver;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Task};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use sheaf_core::{
    CreateOptions, Filter, Fingerprint, FolderConfig, Note, NoteId, NoteMeta, NoteStore, Reader,
    Result, SearchIndex, SearchResults, Sort, WriteOptions, query,
};
use zbus::Connection;

use overlay::{Lists, Overlay};
use service::{ServiceLink, Syncer};

use super::serial::Serial;

/// Where the index comes from, see the module documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryMode {
    /// Still starting: lists show the index found on disk, or nothing yet.
    Opening,
    Service,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryEvent {
    /// Notes were created, changed, moved or removed, by this app or another program.
    /// An empty list means "anything may have changed": reload what is shown.
    Changed { ids: Vec<NoteId> },
}

/// A folder of the notes folder; the root is the empty path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Relative to the notes folder, `/`-separated.
    pub path: String,
    /// Notes directly in this folder.
    pub notes: usize,
}

/// A note after [`Library::save_body`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub meta: NoteMeta,
    /// The file as it is now; pass it to the next save.
    pub fingerprint: Fingerprint,
}

/// The time the Library takes for "now": the date filters of a search, the deletion
/// time of a trashed note, and what views should format dates against.
pub type Clock = Arc<dyn Fn() -> ZonedDateTime + Send + Sync>;

#[derive(Clone)]
enum Backend {
    /// Writes go to the files only; the index catches up when it is ready.
    Opening,
    /// The service is asked to index each write; until it confirms, the write is in
    /// the overlay.
    Service(Arc<Syncer>, Arc<Mutex<Overlay>>),
    /// The lock keeps writers apart: an index takes one at a time.
    Local(Arc<Mutex<SearchIndex>>),
}

impl Backend {
    /// Brings the index in line after a write: each of `paths` is indexed if it is a
    /// note file now and dropped from the index otherwise. Blocks on the in-memory
    /// indexes only: the service is asked without waiting for it.
    fn reflect(&self, store: &NoteStore, reader: &Reader, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        match self {
            Backend::Opening => Ok(()),
            Backend::Service(syncer, overlay) => {
                let mut overlay = overlay.lock().unwrap_or_else(PoisonError::into_inner);
                let indexed = overlay.reflect(store, paths);
                syncer.request(overlay.writes());
                indexed
            }
            Backend::Local(index) => {
                let index = index.lock().unwrap_or_else(PoisonError::into_inner);
                index_paths(&index, store, paths)?;
                reader.reload()
            }
        }
    }

    fn overlay(&self) -> Option<Arc<Mutex<Overlay>>> {
        match self {
            Backend::Service(_, overlay) => Some(overlay.clone()),
            _ => None,
        }
    }
}

/// Indexes each of `paths` if it is a note file now and drops it from `index`
/// otherwise.
fn index_paths(index: &SearchIndex, store: &NoteStore, paths: &[String]) -> Result<()> {
    let mut writer = index.writer()?;
    for rel_path in paths {
        if store.root().join(rel_path).is_file() {
            writer.upsert(&store.load_doc(rel_path)?)?;
        } else {
            writer.remove_path(rel_path);
        }
    }
    writer.finish()
}

/// What a write does to the note it was given, for the index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Effect {
    /// The note exists afterwards, possibly under another path.
    Keep,
    /// The note is no longer among the notes.
    Remove,
}

/// What a [`Library::run`] did to the notes: the paths the index has to look at again
/// and the notes to tell the listeners about.
#[derive(Debug, Default)]
pub struct Touched {
    ids: Vec<NoteId>,
    paths: Vec<String>,
}

impl Touched {
    /// `meta` was written. `before` is the note it started from with its path then,
    /// looked at again; `None` for a new note.
    pub fn wrote(&mut self, before: Option<(NoteId, String)>, meta: &NoteMeta) {
        if let Some((id, rel_path)) = before {
            self.removed(id, rel_path);
        }
        self.note(meta.id.clone(), meta.rel_path.clone());
    }

    /// The note `id`, which was at `rel_path`, is no longer among the notes.
    pub fn removed(&mut self, id: NoteId, rel_path: String) {
        self.note(id, rel_path);
    }

    fn note(&mut self, id: NoteId, rel_path: String) {
        self.tell(id);
        if !self.paths.contains(&rel_path) {
            self.paths.push(rel_path);
        }
    }

    /// Tells the listeners about `id` without indexing anything for it, such as the
    /// id a note has in the trash.
    pub fn tell(&mut self, id: NoteId) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }
}

pub struct Library {
    store: Arc<NoteStore>,
    reader: Reader,
    backend: Backend,
    clock: Clock,
    /// Keeps the writers of `.sheaf/config.toml` apart and in order: each reads the
    /// file, changes its keys and writes it.
    config: Serial,
    /// The session bus it was opened with, for [`reopen`](Self::reopen).
    connection: Option<Connection>,
    _starting: Task<()>,
    _following: Option<Task<()>>,
    _confirming: Option<Task<()>>,
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Library")
            .field("notes_dir", &self.store.root())
            .field("mode", &self.mode())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<LibraryEvent> for Library {}

impl Library {
    /// Opens the notes folder `notes_dir`, creating it if needed.
    ///
    /// With a `connection` and an `index_dir`, the index found there is shown at once
    /// and the service is brought up in the background. Without either, or if that
    /// fails, the folder is indexed in memory. Either way a [`LibraryEvent::Changed`]
    /// without ids follows when the lists are complete.
    pub fn open(
        notes_dir: &Path,
        index_dir: Option<PathBuf>,
        connection: Option<Connection>,
        cx: &mut App,
    ) -> Result<Entity<Self>> {
        std::fs::create_dir_all(notes_dir)?;
        let store = Arc::new(NoteStore::new(notes_dir));
        let kept = connection.clone();
        let service = connection.zip(index_dir);
        let on_disk = match &service {
            Some((_, index_dir)) => SearchIndex::open_read_only(index_dir).ok().flatten(),
            None => None,
        };
        let reader = match on_disk {
            Some(index) => index.reader(),
            None => SearchIndex::in_memory()?.reader(),
        };
        Ok(cx.new(|cx| Library {
            _starting: Self::start(store.clone(), service, cx),
            store,
            reader,
            backend: Backend::Opening,
            clock: Arc::new(|| Timestamp::now().to_zoned_datetime(TimeZone::system())),
            config: Serial::new(cx),
            connection: kept,
            _following: None,
            _confirming: None,
        }))
    }

    /// A Library that never looks for the service, for tests and screenshots.
    pub fn local(notes_dir: &Path, cx: &mut App) -> Result<Entity<Self>> {
        Self::open(notes_dir, None, None, cx)
    }

    /// Finds out in the background where the index comes from, then switches to it.
    fn start(
        store: Arc<NoteStore>,
        service: Option<(Connection, PathBuf)>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let (sender, changes) = async_channel::unbounded();
        let (confirm, confirmed) = async_channel::unbounded();
        cx.spawn(async move |this, cx| {
            let established = cx
                .background_spawn(async move { establish(&store, service, sender, confirm) })
                .await;
            let _ = this.update(cx, |this, cx| match established {
                Ok((backend, reader)) => {
                    if matches!(backend, Backend::Local(_)) {
                        log::info!("indexing the notes in memory, without sheaf-searchd");
                    }
                    this.backend = backend;
                    this.reader = reader;
                    this._following = Some(Self::follow(changes, cx));
                    this._confirming = Some(Self::confirm(confirmed, cx));
                    cx.emit(LibraryEvent::Changed { ids: Vec::new() });
                }
                Err(err) => log::error!("cannot index the notes folder: {err}"),
            });
        })
    }

    /// Reloads the reader and tells the views whenever the service reports a change.
    fn follow(changes: Receiver<Vec<String>>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Ok(ids) = changes.recv().await {
                let Ok(reader) = this.read_with(cx, |this, _| this.reader.clone()) else {
                    break;
                };
                let reloaded = cx.background_spawn(async move { reader.reload() }).await;
                if let Err(err) = reloaded {
                    log::warn!("cannot reload the index: {err}");
                }
                let ids = ids.iter().filter_map(|id| id.parse().ok()).collect();
                if this
                    .update(cx, |_, cx| cx.emit(LibraryEvent::Changed { ids }))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// Drops the app's writes from the overlay once the service confirms it indexed
    /// them. The lists stay the same, so nobody is told.
    fn confirm(confirmed: Receiver<u64>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Ok(upto) = confirmed.recv().await {
                let Ok((reader, overlay)) =
                    this.read_with(cx, |this, _| (this.reader.clone(), this.backend.overlay()))
                else {
                    break;
                };
                let Some(overlay) = overlay else { continue };
                let confirmed = cx
                    .background_spawn(async move {
                        // The service's index first, so that no write shows as missing.
                        reader.reload()?;
                        overlay
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .confirm(upto)
                    })
                    .await;
                if let Err(err) = confirmed {
                    log::warn!("cannot take the confirmed writes from the overlay: {err}");
                }
            }
        })
    }

    pub fn mode(&self) -> LibraryMode {
        match self.backend {
            Backend::Opening => LibraryMode::Opening,
            Backend::Service(..) => LibraryMode::Service,
            Backend::Local(_) => LibraryMode::Local,
        }
    }

    pub fn notes_dir(&self) -> &Path {
        self.store.root()
    }

    /// The current time by the Library's clock.
    pub fn now(&self) -> ZonedDateTime {
        (self.clock)()
    }

    /// Replaces the clock, so that screenshots and tests show the same dates every run.
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    /// The notes that pass all `filters`, in the order of `sort`.
    pub fn notes(&self, filters: Vec<Filter>, sort: Sort, cx: &App) -> Task<Result<Vec<NoteMeta>>> {
        let lists = self.lists();
        let now = self.now();
        cx.background_spawn(async move { lists.list(&filters, sort, &now) })
    }

    fn lists(&self) -> Lists {
        Lists::new(self.reader.clone(), self.backend.overlay())
    }

    /// The `limit` best hits for `query`, in the search language of
    /// [`sheaf_core::query`], with snippets and highlights.
    pub fn search(&self, query: &str, limit: usize, cx: &App) -> Task<Result<SearchResults>> {
        let lists = self.lists();
        let now = self.now();
        let parsed = query::parse(query);
        cx.background_spawn(async move { lists.search(&parsed, limit, &now) })
    }

    /// Every folder, empty ones included, sorted by path. The root comes first.
    pub fn folders(&self, cx: &App) -> Task<Result<Vec<Folder>>> {
        let reader = self.reader.clone();
        let store = self.store.clone();
        cx.background_spawn(async move {
            let counts: std::collections::HashMap<String, usize> =
                reader.folders()?.into_iter().collect();
            let mut paths = vec![String::new()];
            paths.extend(store.folders()?);
            Ok(paths
                .into_iter()
                .map(|path| Folder {
                    notes: counts.get(&path).copied().unwrap_or(0),
                    path,
                })
                .collect())
        })
    }

    /// Every tag with the number of notes it finds, sorted by name.
    pub fn tags(&self, cx: &App) -> Task<Result<Vec<(String, usize)>>> {
        let reader = self.reader.clone();
        cx.background_spawn(async move { reader.tags() })
    }

    /// What the index knows about a note; `None` if it has no such note.
    pub fn meta(&self, id: &NoteId, cx: &App) -> Task<Result<Option<NoteMeta>>> {
        let lists = self.lists();
        let id = id.clone();
        cx.background_spawn(async move { lists.get(&id) })
    }

    /// The Markdown text of a note as indexed, without reading its file. For previews.
    pub fn body(&self, id: &NoteId, cx: &App) -> Task<Result<Option<String>>> {
        let lists = self.lists();
        let id = id.clone();
        cx.background_spawn(async move { lists.body(&id) })
    }

    /// The notes with a `[[wiki link]]` to `title`, ignoring case, most recently
    /// modified first.
    pub fn backlinks(&self, title: &str, cx: &App) -> Task<Result<Vec<NoteMeta>>> {
        let lists = self.lists();
        let title = title.to_owned();
        cx.background_spawn(async move { lists.backlinks(&title) })
    }

    /// Reads the note's file. This is what an editor starts from.
    pub fn load(&self, id: &NoteId, cx: &App) -> Task<Result<Note>> {
        let store = self.store.clone();
        let id = id.clone();
        cx.background_spawn(async move { store.get(&id) })
    }

    /// The state of the note's file now, to tell a change by another program from one's
    /// own write.
    pub fn fingerprint(&self, id: &NoteId, cx: &App) -> Task<Result<Fingerprint>> {
        let store = self.store.clone();
        let id = id.clone();
        cx.background_spawn(async move { store.fingerprint(&id) })
    }

    /// What a view remembered about a note, such as column widths; empty if nothing.
    pub fn note_state(&self, id: &NoteId, cx: &App) -> Task<Result<toml::Table>> {
        let root = self.store.root().to_owned();
        let id = id.clone();
        cx.background_spawn(async move {
            let mut config = FolderConfig::load(&root)?;
            Ok(config.notes.remove(&id).unwrap_or_default())
        })
    }

    /// The note a `file://` URI or a path points to, if it is in the notes folder.
    pub fn note_at(&self, uri: &str, cx: &App) -> Task<Option<NoteMeta>> {
        let store = self.store.clone();
        let path = PathBuf::from(decode_file_uri(uri));
        cx.background_spawn(async move {
            let canonical = |path: &Path| path.canonicalize().ok();
            let rel_path = canonical(&path)?
                .strip_prefix(canonical(store.root())?)
                .ok()?
                .to_str()?
                .to_owned();
            Some(store.load(&rel_path).ok()?.meta)
        })
    }

    /// The settings of the notes folder: view and sort per folder, pinned tags. Folders
    /// without an order of their own take the default order of the settings. Read
    /// after the changes asked for before, so it shows them.
    pub fn folder_config(&self, cx: &App) -> Task<Result<FolderConfig>> {
        let root = self.store.root().to_owned();
        let default_sort = super::settings::config(cx).sort.unwrap_or_default();
        let loaded = self.config.run(
            move || {
                let mut config = FolderConfig::load(&root)?;
                config.default_sort = default_sort;
                Ok(config)
            },
            cx,
        );
        cx.background_spawn(async move { loaded.await.unwrap_or_else(closing) })
    }

    /// Changes the settings of the notes folder: the file is read again under the
    /// queue that keeps its writers apart and in order, `change` changes what it was read as, and
    /// only the keys `change` touched are written. Every writer of
    /// `.sheaf/config.toml` goes through here, so none overwrites another's keys with
    /// an old copy. Resolves to the settings as they are now, with the default order
    /// of [`folder_config`](Self::folder_config); fails without writing if the file
    /// cannot be read. Dropping the task does not cancel the change.
    pub fn update_folder_config(
        &self,
        change: impl FnOnce(&mut FolderConfig) + Send + 'static,
        cx: &App,
    ) -> Task<Result<FolderConfig>> {
        let root = self.store.root().to_owned();
        let default_sort = super::settings::config(cx).sort.unwrap_or_default();
        let updated = self.config.run(
            move || {
                FolderConfig::update(&root, |config| {
                    config.default_sort = default_sort;
                    change(config);
                })
            },
            cx,
        );
        cx.background_spawn(async move { updated.await.unwrap_or_else(closing) })
    }

    /// A Library of another notes folder, opened the way this one was: with the same
    /// link to the session bus and the same clock.
    pub fn reopen(this: &Entity<Self>, notes_dir: &Path, cx: &mut App) -> Result<Entity<Self>> {
        let (connection, clock) = {
            let this = this.read(cx);
            (this.connection.clone(), this.clock.clone())
        };
        let index_dir = connection.as_ref().and_then(|_| {
            sheaf_core::paths::Env::from_process()
                .index_dir(notes_dir)
                .ok()
        });
        let library = Self::open(notes_dir, index_dir, connection, cx)?;
        library.update(cx, |library, _| library.clock = clock);
        Ok(library)
    }

    /// Creates an empty folder (relative to the notes folder) and tells the listeners,
    /// with no ids, so that folder lists reload.
    pub fn create_folder(&mut self, folder: &str, cx: &mut Context<Self>) -> Task<Result<()>> {
        let store = self.store.clone();
        let folder = folder.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_spawn(async move { store.create_folder(&folder) })
                .await?;
            let _ = this.update(cx, |_, cx| {
                cx.emit(LibraryEvent::Changed { ids: Vec::new() })
            });
            Ok(())
        })
    }

    /// Creates a note from `text`, whose first line is the title.
    pub fn create(
        &mut self,
        text: &str,
        options: CreateOptions,
        cx: &mut Context<Self>,
    ) -> Task<Result<NoteMeta>> {
        let text = text.to_owned();
        self.write(None, Effect::Keep, cx, move |store| {
            store.create(&text, &options)
        })
    }

    /// Replaces the text of a note. With `expected`, the fingerprint from
    /// [`load`](Self::load) or the previous save, nothing is written if the file changed
    /// in between and the task fails with [`sheaf_core::Error::Conflict`]. The file
    /// keeps its name; see [`sync_filename`](Self::sync_filename).
    pub fn save_body(
        &mut self,
        id: &NoteId,
        body: String,
        expected: Option<Fingerprint>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Saved>> {
        let options = WriteOptions {
            rename: false,
            expected,
        };
        let store = self.store.clone();
        let target = id.clone();
        let written = self.write(Some(id.clone()), Effect::Keep, cx, move |store| {
            store.write_body(&target, &body, &options)
        });
        cx.background_spawn(async move {
            let meta = written.await?;
            let fingerprint = store.fingerprint(&meta.id)?;
            Ok(Saved { meta, fingerprint })
        })
    }

    /// Renames the file so that it follows the title.
    pub fn sync_filename(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        self.write(Some(id.clone()), Effect::Keep, cx, move |store| {
            store.sync_filename(&target)
        })
    }

    pub fn set_pinned(
        &mut self,
        id: &NoteId,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        self.write(Some(id.clone()), Effect::Keep, cx, move |store| {
            store.set_pinned(&target, pinned)
        })
    }

    /// Moves a note to the trash. [`restore`](Self::restore) takes the returned id.
    pub fn trash(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        let now = self.now().timestamp();
        self.write(Some(id.clone()), Effect::Remove, cx, move |store| {
            store.trash(&target, now)
        })
    }

    /// Puts a trashed note back into the folder it came from.
    pub fn restore(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        self.write(None, Effect::Keep, cx, move |store| store.restore(&target))
    }

    /// Moves a note to `folder` (relative to the notes folder; empty for the root).
    pub fn move_to(
        &mut self,
        id: &NoteId,
        folder: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        let folder = folder.to_owned();
        self.write(Some(id.clone()), Effect::Keep, cx, move |store| {
            store.move_to(&target, &folder)
        })
    }

    /// Copies a note into a new one beside it and resolves to the copy.
    pub fn duplicate(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        self.write(Some(id.clone()), Effect::Keep, cx, move |store| {
            store.duplicate(&target)
        })
    }

    /// Runs `change` on the background executor, brings the index in line and tells
    /// the listeners. `id` is the note the change starts from, if any: if the change
    /// fails, the index looks at its file again.
    fn write(
        &mut self,
        id: Option<NoteId>,
        effect: Effect,
        cx: &mut Context<Self>,
        change: impl FnOnce(&NoteStore) -> Result<NoteMeta> + Send + 'static,
    ) -> Task<Result<NoteMeta>> {
        let lists = self.lists();
        self.run(cx, move |store, touched| {
            let before = id.and_then(|id| {
                // A file deleted elsewhere has its path only in the index.
                let rel_path = match store.path_of(&id) {
                    Ok(rel_path) => rel_path,
                    Err(_) => lists.get(&id).ok()??.rel_path,
                };
                Some((id, rel_path))
            });
            let meta = match change(store) {
                Ok(meta) => meta,
                Err(err) => {
                    // The file may not be what the lists show, such as deleted
                    // elsewhere: they take it as it is.
                    if let Some((id, rel_path)) = before {
                        touched.note(id, rel_path);
                    }
                    return Err(err);
                }
            };
            match effect {
                Effect::Keep => touched.wrote(before, &meta),
                Effect::Remove => {
                    if let Some((id, rel_path)) = before {
                        touched.removed(id, rel_path);
                    }
                    touched.tell(meta.id.clone());
                }
            }
            Ok(meta)
        })
    }

    /// Runs `change` on the background executor, then brings the index in line with
    /// what it reports in [`Touched`] and tells the listeners, also if it fails half
    /// way. For writes of several steps, such as a save with its fallbacks.
    pub fn run<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&NoteStore, &mut Touched) -> Result<T> + Send + 'static,
    ) -> Task<Result<T>> {
        let store = self.store.clone();
        let reader = self.reader.clone();
        let backend = self.backend.clone();
        let written = cx.background_spawn(async move {
            let mut touched = Touched::default();
            let outcome = change(&store, &mut touched);
            if let Err(err) = backend.reflect(&store, &reader, &touched.paths) {
                log::warn!("cannot bring the index in line: {err}");
            }
            (outcome, touched.ids)
        });
        let (done, result) = async_channel::bounded(1);
        // Detached, so that the change and its announcement happen even if the caller
        // drops the task it gets.
        cx.spawn(async move |this, cx| {
            let (outcome, ids) = written.await;
            if !ids.is_empty() {
                let _ = this.update(cx, |_, cx| cx.emit(LibraryEvent::Changed { ids }));
            }
            let _ = done.send(outcome).await;
        })
        .detach();
        cx.background_spawn(async move {
            result
                .recv()
                .await
                .unwrap_or_else(|_| Err(sheaf_core::Error::Invalid("the app is closing".into())))
        })
    }

    /// [`run`](Self::run) at once, on this thread. Only for the last save when the
    /// window closes or the app quits: GPUI runs no task after that.
    pub fn run_now<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&NoteStore, &mut Touched) -> Result<T>,
    ) -> Result<T> {
        let mut touched = Touched::default();
        let outcome = change(&self.store, &mut touched);
        if let Err(err) = self
            .backend
            .reflect(&self.store, &self.reader, &touched.paths)
        {
            log::warn!("cannot bring the index in line: {err}");
        }
        if !touched.ids.is_empty() {
            cx.emit(LibraryEvent::Changed { ids: touched.ids });
        }
        outcome
    }
}

/// What a task of the Library resolves to when the app closed before it ran.
fn closing<T>() -> Result<T> {
    Err(sheaf_core::Error::Invalid("the app is closing".into()))
}

/// Decides where the index comes from and returns a reader of it. Blocks.
fn establish(
    store: &NoteStore,
    service: Option<(Connection, PathBuf)>,
    changes: async_channel::Sender<Vec<String>>,
    confirm: async_channel::Sender<u64>,
) -> Result<(Backend, Reader)> {
    if let Some((connection, index_dir)) = service
        && let Some(link) = ServiceLink::connect(connection, store.root(), changes)
        && let Some(index) = SearchIndex::open_read_only(&index_dir)?
    {
        let reader = index.reader();
        reader.reload()?;
        let syncer = Arc::new(link.into_syncer(confirm));
        let overlay = Arc::new(Mutex::new(Overlay::new()?));
        return Ok((Backend::Service(syncer, overlay), reader));
    }
    let index = SearchIndex::in_memory()?;
    let mut writer = index.writer()?;
    for doc in store.all_docs()? {
        writer.upsert(&doc)?;
    }
    writer.finish()?;
    let reader = index.reader();
    reader.reload()?;
    Ok((Backend::Local(Arc::new(Mutex::new(index))), reader))
}

/// The path of a `file://` URI with its percent escapes resolved; anything else is
/// taken as a path as it is.
pub(crate) fn decode_file_uri(uri: &str) -> String {
    let Some(path) = uri.strip_prefix("file://") else {
        return uri.to_owned();
    };
    // `file://host/path`: the path starts at the first slash.
    let path = &path[path.find('/').unwrap_or(0)..];
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let [first, tail @ ..] = rest {
        let escaped = match (first, tail) {
            (b'%', [high, low, ..]) => std::str::from_utf8(&[*high, *low])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok()),
            _ => None,
        };
        match escaped {
            Some(byte) => {
                bytes.push(byte);
                rest = &tail[2..];
            }
            None => {
                bytes.push(*first);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gpui::TestAppContext;

    use super::*;

    /// A `sheaf-searchd` that hangs: reads and writes go on, the writes ask once.
    #[gpui::test]
    async fn notes_are_read_and_written_without_waiting_for_the_service(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let library = cx
            .update(|cx| Library::local(dir.path(), cx))
            .expect("a library");
        cx.run_until_parked();
        let asked = Arc::new(AtomicUsize::new(0));
        let (release, hang) = std::sync::mpsc::channel::<()>();
        let syncer = Syncer::new(
            {
                let asked = asked.clone();
                move || {
                    asked.fetch_add(1, Ordering::SeqCst);
                    let _ = hang.recv();
                    true
                }
            },
            async_channel::unbounded().0,
        );
        let overlay = Arc::new(Mutex::new(Overlay::new().expect("an overlay")));
        library.update(cx, |library, _| {
            library.backend = Backend::Service(Arc::new(syncer), overlay);
        });

        let created = library.update(cx, |library, cx| {
            library.create("Eins", CreateOptions::default(), cx)
        });
        cx.run_until_parked();
        let id = created.await.expect("created").id;
        let asked_once = || {
            let started = std::time::Instant::now();
            while asked.load(Ordering::SeqCst) < 1 && started.elapsed().as_secs() < 5 {
                std::thread::yield_now();
            }
            asked.load(Ordering::SeqCst)
        };
        assert_eq!(asked_once(), 1, "the write asks the service");
        let read = library.update(cx, |library, cx| {
            library.run(cx, {
                let id = id.clone();
                move |store, _| store.get(&id)
            })
        });
        cx.run_until_parked();
        assert_eq!(read.await.expect("read").body(), "Eins\n");
        for pinned in [true, false, true] {
            let pinning = library.update(cx, |library, cx| library.set_pinned(&id, pinned, cx));
            cx.run_until_parked();
            pinning.await.expect("pinned");
        }
        drop(release);
        let started = std::time::Instant::now();
        while asked.load(Ordering::SeqCst) < 2 && started.elapsed().as_secs() < 5 {
            std::thread::yield_now();
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            2,
            "one sync, and one for the writes during it"
        );
    }

    /// A Library over `dir` whose service never confirms on its own: its index shows
    /// the folder as it was when the Library opened. A test confirms writes up to a
    /// number by sending it on the returned channel, as the syncer does.
    fn with_slow_service(
        dir: &Path,
        cx: &mut TestAppContext,
    ) -> (Entity<Library>, async_channel::Sender<u64>) {
        let library = cx.update(|cx| Library::local(dir, cx)).expect("a library");
        cx.run_until_parked();
        let (confirm, confirmed) = async_channel::unbounded();
        library.update(cx, |library, cx| {
            let syncer = Syncer::new(|| false, async_channel::unbounded().0);
            let overlay = Arc::new(Mutex::new(Overlay::new().expect("an overlay")));
            library.backend = Backend::Service(Arc::new(syncer), overlay);
            library._confirming = Some(Library::confirm(confirmed, cx));
        });
        (library, confirm)
    }

    fn titles(library: &Entity<Library>, cx: &mut TestAppContext) -> Vec<(NoteId, String)> {
        let listed =
            library.read_with(cx, |library, cx| library.notes(Vec::new(), Sort::Title, cx));
        cx.run_until_parked();
        let listed = cx.foreground_executor().block_test(listed);
        let listed = listed.expect("listed");
        listed
            .into_iter()
            .map(|note| (note.id, note.title))
            .collect()
    }

    fn done<T: 'static>(task: Task<Result<T>>, cx: &mut TestAppContext) -> T {
        cx.run_until_parked();
        cx.foreground_executor().block_test(task).expect("done")
    }

    #[gpui::test]
    async fn own_writes_show_before_the_service_indexed_them(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join("Lesen.md"), "Lesen\n\nBücher\n").expect("written");
        std::fs::write(dir.path().join("Alt.md"), "Alt\n").expect("written");
        let (library, _) = with_slow_service(dir.path(), cx);
        let listed = titles(&library, cx);
        let provisional = listed[1].0.clone();
        assert!(provisional.is_provisional());

        // The first save gives the note its real id: listed once, under it.
        let saved = library.update(cx, |library, cx| {
            library.save_body(&provisional, "Lesen\n\nMehr Bücher\n".into(), None, cx)
        });
        let id = done(saved, cx).meta.id;
        assert!(!id.is_provisional());
        assert_eq!(titles(&library, cx)[1], (id.clone(), "Lesen".into()));
        assert_eq!(titles(&library, cx).len(), 2);

        // A new title, and the file following it.
        let saved = library.update(cx, |library, cx| {
            library.save_body(&id, "Gelesen\n\nAlles\n".into(), None, cx)
        });
        done(saved, cx);
        let renamed = library.update(cx, |library, cx| library.sync_filename(&id, cx));
        done(renamed, cx);
        let listed = titles(&library, cx);
        assert_eq!(listed[1], (id.clone(), "Gelesen".into()), "{listed:?}");
        assert_eq!(listed.len(), 2);
        let body = library.read_with(cx, |library, cx| library.body(&id, cx));
        assert_eq!(done(body, cx).as_deref(), Some("Gelesen\n\nAlles\n"));
        let found = library.read_with(cx, |library, cx| library.search("alles", 10, cx));
        let found = done(found, cx);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].meta.id, id);

        // Trashed: gone at once.
        let trashed = library.update(cx, |library, cx| library.trash(&id, cx));
        done(trashed, cx);
        assert_eq!(titles(&library, cx).len(), 1);
        let meta = library.read_with(cx, |library, cx| library.meta(&id, cx));
        assert_eq!(done(meta, cx), None);
        let found = library.read_with(cx, |library, cx| library.search("lesen", 10, cx));
        assert!(done(found, cx).hits.is_empty());
    }

    #[gpui::test]
    async fn confirmed_writes_leave_the_overlay(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let (library, confirm) = with_slow_service(dir.path(), cx);
        let service = SearchIndex::in_memory().expect("an index");
        library.update(cx, |library, _| library.reader = service.reader());
        let created = library.update(cx, |library, cx| {
            library.create("Eins", CreateOptions::default(), cx)
        });
        let id = done(created, cx).id;
        let overlay = library.read_with(cx, |library, _| library.backend.overlay());
        let overlay = overlay.expect("service mode");
        let pending = || {
            !overlay
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        };
        assert!(pending());

        // The service indexes the folder and confirms.
        let mut writer = service.writer().expect("a writer");
        for doc in NoteStore::new(dir.path()).all_docs().expect("docs") {
            writer.upsert(&doc).expect("indexed");
        }
        writer.finish().expect("committed");
        confirm.try_send(1).expect("sent");
        cx.run_until_parked();
        assert!(!pending(), "the confirmed write left the overlay");
        assert_eq!(titles(&library, cx), [(id, "Eins".to_owned())]);
    }

    #[test]
    fn a_confirmed_sync_reports_the_last_write_it_covers() {
        let (confirm, confirmed) = async_channel::unbounded();
        let (release, hang) = std::sync::mpsc::channel::<()>();
        let (started, running) = std::sync::mpsc::channel::<()>();
        let syncer = Syncer::new(
            move || {
                let _ = started.send(());
                let _ = hang.recv();
                true
            },
            confirm,
        );
        syncer.request(1);
        running.recv().expect("the first sync runs");
        // Asked while the first sync runs: one more sync, for both.
        syncer.request(2);
        syncer.request(3);
        release.send(()).expect("released");
        release.send(()).expect("released");
        assert_eq!(confirmed.recv_blocking(), Ok(1));
        assert_eq!(confirmed.recv_blocking(), Ok(3));
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            decode_file_uri("/home/me/Notes/a.md"),
            "/home/me/Notes/a.md"
        );
        assert_eq!(decode_file_uri("file:///home/me/a.md"), "/home/me/a.md");
        assert_eq!(
            decode_file_uri("file://localhost/home/me/a.md"),
            "/home/me/a.md"
        );
        assert_eq!(
            decode_file_uri("file:///n/Ideen%20f%C3%BCr%20den%20Garten.md"),
            "/n/Ideen für den Garten.md"
        );
        assert_eq!(decode_file_uri("file:///n/100%.md"), "/n/100%.md");
        assert_eq!(decode_file_uri("file:///n/a%2"), "/n/a%2");
    }
}
