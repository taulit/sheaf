//! The notes as the views see them. [`Library`] is the only way a view reads or
//! changes notes: it owns the notes folder, a reader of the search index and the link
//! to `sheaf-searchd`.
//!
//! # For views
//!
//! - **Read** with [`notes`](Library::notes), [`search`](Library::search),
//!   [`folders`](Library::folders), [`tags`](Library::tags), [`meta`](Library::meta),
//!   [`body`](Library::body) (text as indexed, for previews),
//!   [`load`](Library::load) (the file, with the fingerprint a later save needs) and
//!   [`folder_config`](Library::folder_config) (what is remembered per folder).
//! - **Change** with [`create`](Library::create), [`save_body`](Library::save_body),
//!   [`save_conflict_copy`](Library::save_conflict_copy),
//!   [`sync_filename`](Library::sync_filename), [`set_pinned`](Library::set_pinned),
//!   [`trash`](Library::trash), [`restore`](Library::restore),
//!   [`move_to`](Library::move_to) and [`duplicate`](Library::duplicate). Each
//!   resolves to the note as it is afterwards. Its id can differ from the one passed
//!   in: a file Sheaf never wrote gets its real id on the first write, so keep using
//!   the returned one.
//!   [`create_folder`](Library::create_folder) and
//!   [`save_folder_config`](Library::save_folder_config) change no note.
//! - **Several steps on the store** in one go, such as a save with its fallbacks:
//!   [`run`](Library::run), which says what it touched in a [`Touched`]. The open
//!   note's last save when the window closes or the app quits uses
//!   [`run_now`](Library::run_now), the one call that blocks.
//! - **Per-note view state** (column widths, folded headings) with
//!   [`note_state`](Library::note_state) and [`set_note_state`](Library::set_note_state);
//!   it lives in the folder's `.sheaf/config.toml`, not in the note.
//! - **Follow** changes by subscribing to [`LibraryEvent`]; when a change task
//!   resolves, the lists already contain it.
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

mod service;

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

use service::ServiceLink;

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
    Service(Arc<ServiceLink>),
    /// The lock keeps writers apart: an index takes one at a time.
    Local(Arc<Mutex<SearchIndex>>),
}

impl Backend {
    /// Brings the index in line after a write: each of `paths` is indexed if it is a
    /// note file now and dropped from the index otherwise. Blocks.
    fn reflect(&self, store: &NoteStore, reader: &Reader, paths: &[String]) -> Result<()> {
        match self {
            Backend::Opening => Ok(()),
            Backend::Service(service) => {
                if !service.sync() {
                    log::warn!("sheaf-searchd did not confirm the change, lists may lag");
                }
                reader.reload()
            }
            Backend::Local(index) => {
                let index = index.lock().unwrap_or_else(PoisonError::into_inner);
                let mut writer = index.writer()?;
                for rel_path in paths {
                    if store.root().join(rel_path).is_file() {
                        writer.upsert(&store.load_doc(rel_path)?)?;
                    } else {
                        writer.remove_path(rel_path);
                    }
                }
                writer.finish()?;
                reader.reload()
            }
        }
    }
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
    /// `meta` was written. `before` is the note it replaced with its path then, `None`
    /// for a new note.
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
    /// Keeps the writers of `.sheaf/config.toml` apart: each reads, changes and writes
    /// the whole file.
    config: Arc<Mutex<()>>,
    _starting: Task<()>,
    _following: Option<Task<()>>,
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
            config: Arc::new(Mutex::new(())),
            _following: None,
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
        cx.spawn(async move |this, cx| {
            let established = cx
                .background_spawn(async move { establish(&store, service, sender) })
                .await;
            let _ = this.update(cx, |this, cx| match established {
                Ok((backend, reader)) => {
                    if matches!(backend, Backend::Local(_)) {
                        log::info!("indexing the notes in memory, without sheaf-searchd");
                    }
                    this.backend = backend;
                    this.reader = reader;
                    this._following = Some(Self::follow(changes, cx));
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

    pub fn mode(&self) -> LibraryMode {
        match self.backend {
            Backend::Opening => LibraryMode::Opening,
            Backend::Service(_) => LibraryMode::Service,
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
        let reader = self.reader.clone();
        let now = self.now();
        cx.background_spawn(async move { reader.list(&filters, sort, None, &now) })
    }

    /// The `limit` best hits for `query`, in the search language of
    /// [`sheaf_core::query`], with snippets and highlights.
    pub fn search(&self, query: &str, limit: usize, cx: &App) -> Task<Result<SearchResults>> {
        let reader = self.reader.clone();
        let now = self.now();
        let parsed = query::parse(query);
        cx.background_spawn(async move { reader.search(&parsed, limit, &now) })
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
        let reader = self.reader.clone();
        let id = id.clone();
        cx.background_spawn(async move { reader.get(&id) })
    }

    /// The Markdown text of a note as indexed, without reading its file. For previews.
    pub fn body(&self, id: &NoteId, cx: &App) -> Task<Result<Option<String>>> {
        let reader = self.reader.clone();
        let id = id.clone();
        cx.background_spawn(async move { reader.body(&id) })
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

    /// Remembers `state` for a note; an empty table forgets it.
    pub fn set_note_state(&self, id: &NoteId, state: toml::Table, cx: &App) -> Task<Result<()>> {
        let root = self.store.root().to_owned();
        let lock = self.config.clone();
        let id = id.clone();
        cx.background_spawn(async move {
            let _writing = lock.lock().unwrap_or_else(PoisonError::into_inner);
            let mut config = FolderConfig::load(&root)?;
            if state.is_empty() {
                config.notes.remove(&id);
            } else {
                config.notes.insert(id, state);
            }
            config.save(&root)
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

    /// The settings of the notes folder: view and sort per folder, pinned tags.
    pub fn folder_config(&self, cx: &App) -> Task<Result<FolderConfig>> {
        let root = self.store.root().to_owned();
        cx.background_spawn(async move { FolderConfig::load(&root) })
    }

    /// Writes the folder settings. The per-note view state is taken from the file, not
    /// from `config`: [`set_note_state`](Self::set_note_state) may have changed it since
    /// `config` was read.
    pub fn save_folder_config(&self, mut config: FolderConfig, cx: &App) -> Task<Result<()>> {
        let root = self.store.root().to_owned();
        let lock = self.config.clone();
        cx.background_spawn(async move {
            let _writing = lock.lock().unwrap_or_else(PoisonError::into_inner);
            config.notes = FolderConfig::load(&root)?.notes;
            config.save(&root)
        })
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

    /// Stores `body` as a note of its own beside the original, for edits that lost
    /// against a change on disk.
    pub fn save_conflict_copy(
        &mut self,
        id: &NoteId,
        body: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        self.write(None, Effect::Keep, cx, move |store| {
            store.save_conflict_copy(&target, &body)
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

    /// Deletes a note for good, without a stop in the trash: for a new note that was
    /// left empty.
    pub fn discard(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<NoteMeta>> {
        let target = id.clone();
        let now = self.now().timestamp();
        self.write(Some(id.clone()), Effect::Remove, cx, move |store| {
            let trashed = store.trash(&target, now)?;
            store.purge(&trashed.id)?;
            Ok(trashed)
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
        self.write(None, Effect::Keep, cx, move |store| {
            store.duplicate(&target)
        })
    }

    /// Runs `change` on the background executor, brings the index in line and tells
    /// the listeners. `id` is the note the change replaces or removes, if any.
    fn write(
        &mut self,
        id: Option<NoteId>,
        effect: Effect,
        cx: &mut Context<Self>,
        change: impl FnOnce(&NoteStore) -> Result<NoteMeta> + Send + 'static,
    ) -> Task<Result<NoteMeta>> {
        self.run(cx, move |store, touched| {
            let before = id.and_then(|id| {
                let rel_path = store.path_of(&id).ok()?;
                Some((id, rel_path))
            });
            let meta = change(store)?;
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
        // The service sees the files change through its watcher; waiting for it to
        // confirm could hold up the closing window by the length of a D-Bus timeout.
        if !matches!(self.backend, Backend::Service(_))
            && let Err(err) = self
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

/// Decides where the index comes from and returns a reader of it. Blocks.
fn establish(
    store: &NoteStore,
    service: Option<(Connection, PathBuf)>,
    changes: async_channel::Sender<Vec<String>>,
) -> Result<(Backend, Reader)> {
    if let Some((connection, index_dir)) = service
        && let Some(link) = ServiceLink::connect(connection, store.root(), changes)
        && let Some(index) = SearchIndex::open_read_only(&index_dir)?
    {
        let reader = index.reader();
        reader.reload()?;
        return Ok((Backend::Service(Arc::new(link)), reader));
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
fn decode_file_uri(uri: &str) -> String {
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
    use super::decode_file_uri;

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
