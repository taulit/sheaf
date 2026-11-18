//! One note, open for editing (plan 03 §7): header bar, date line, the editor, the
//! banner for changes made elsewhere and the quiet "Saved".
//!
//! # Interface
//!
//! - The window creates a [`NoteView`] per opened note and drops it after it has slid
//!   out. Before that it calls [`leave`](NoteView::leave), which saves what was typed
//!   (or deletes a new note that stayed empty, see [`mark_new`](NoteView::mark_new)),
//!   and lets the view go only once that is on disk. When the window closes or the app
//!   quits, [`save_now`](NoteView::save_now) does the same at once on the UI thread:
//!   GPUI runs no task after that.
//! - Everything that writes the open note goes through one worker task in file order
//!   (`worker`): saving, renaming, pinning, moving. Add new writes as a `Job`, not as
//!   a direct call to the [`Library`]: a write behind the worker's back changes the
//!   file under it and the next save would look like a conflict.
//! - The id of a note can change with its first save (see `NoteId`), and after
//!   "Keep Mine" the view edits the copy; [`id`](NoteView::id) is always the current
//!   one.
//! - What the window has to do is a [`NoteEvent`]: open another note, search, show a
//!   toast, return after the note went to the trash.
//! - Leaving is the window's `Back` action; keys reach the view through the
//!   [`actions::NOTE`] key context.
//!
//! # Saving
//!
//! 600 ms after the last change, and at once on leaving. The file name follows the
//! title 2 s after the title last changed, and on leaving. A change on disk that the
//! view did not make is told apart from its own writes by the file's fingerprint: with
//! nothing unsaved the view takes the new text silently, otherwise it asks.
//!
//! The editor counts as saved only once the worker reports that its text is on disk.
//! A save that fails shows a banner and is tried again with the next change, with "Try
//! Again" and on leaving; leaving that fails keeps the view open.

pub(crate) mod attach;
mod backlinks;
mod date;
pub mod export;
pub use export::ViewerFiles;
pub(crate) mod files;
mod header;
mod jump;
pub(crate) mod setup;
mod swipe;
pub(crate) mod template;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_channel::Sender;
use gpui::{
    AnimationExt as _, App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::Theme;
use gpui_markdown_editor::{DocMeta, EditorEvent, MarkdownEditor, UiParts};
use sheaf_core::i18n::tr;
use sheaf_core::{CreateOptions, Note, NoteId, NoteMeta, Sort};

use super::actions::{
    self, AttachFile, Back, ExportMarkdown, ExportPdf, PasteAttachment, Print, Trash,
};
use super::assets::{IconName, icon};
use super::chrome::pane;
use super::library::{Folder, Library, LibraryEvent};
use super::overlays::Toast;
use super::settings::{self, Settings};
use super::state::Recovery;
use super::theme::ActiveTokens as _;
use export::Format;
use setup::{Lookup, SharedLookup};
pub use worker::Upload;
use worker::{Change, Job, Leave, Queued, Report, Shared};

/// How long typing may pause before the text is saved.
pub const SAVE_DELAY: Duration = Duration::from_millis(600);
/// How long the title must stay unchanged before the file is renamed after it.
pub const RENAME_DELAY: Duration = Duration::from_secs(2);
/// "Saved" fading in, as content does in 04.
const FADE: Duration = Duration::from_millis(300);

/// What the note view asks the window to do.
#[derive(Clone, Debug)]
pub enum NoteEvent {
    /// Open this note, such as the target of a wiki link.
    Open(NoteId),
    /// Go back to the gallery and search for this, such as a clicked tag.
    Search(String),
    /// The note went to the trash; `0` is it there, for "Undo".
    Trashed(NoteMeta),
    Toast(Toast),
}

/// What the view has to tell about the file, under the header bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Banner {
    /// Another program changed the file while there were unsaved edits.
    ChangedOnDisk,
    /// Another program deleted the file.
    DeletedOnDisk,
    /// The text could not be stored; `0` says why.
    SaveFailed(String),
}

/// The popover open under a header button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Popover {
    Share,
    Menu,
    Info,
    /// "Move to…" with the folders to choose from.
    Move(Vec<Folder>),
}

pub struct NoteView {
    library: Entity<Library>,
    notes_dir: PathBuf,
    id: NoteId,
    /// What is known about the note; `None` until it is loaded.
    meta: Option<NoteMeta>,
    /// The search the note was opened from, to jump to once the text is there.
    query: String,
    editor: Entity<MarkdownEditor>,
    /// Whether the text is there. Until then the editor is not drawn and this view's
    /// own focus handle takes the keys, so that nothing typed is overwritten by the
    /// text arriving.
    loaded: bool,
    focus_handle: FocusHandle,
    jobs: Sender<Queued>,
    /// The file as the worker knows it, for the last save.
    shared: Shared,
    lookup: SharedLookup,
    /// Created for this visit: if it is left empty, it is deleted on leaving.
    fresh: bool,
    /// Left, trashed or saved for the last time: nothing more is written.
    closed: bool,
    /// Leaving is under way.
    leaving: bool,
    /// Set when the note is left or closed: copies of attachments still running are
    /// given up, as leaving does not wait for them.
    closing: Arc<AtomicBool>,
    /// Moving to the trash is under way.
    trashing: bool,
    /// The first line as last seen, to notice when the title changes.
    title: String,
    /// The title changed since the file was last renamed after it.
    rename_pending: bool,
    banner: Option<Banner>,
    popover: Option<Popover>,
    /// The touchpad gesture over the text, for swiping back.
    swipe: swipe::Swipe,
    template_menu: Option<template::TemplateMenu>,
    _templates: Task<()>,
    /// Counts the saves; "Saved" fades in afresh after each one.
    saves: usize,
    saved_shown: bool,
    _save_timer: Task<()>,
    _rename_timer: Task<()>,
    _lookup: Task<()>,
    _state: Task<()>,
    /// The notes that link here, and the title they were looked up for.
    backlinks: Vec<NoteMeta>,
    backlinks_title: String,
    backlinks_open: bool,
    _backlinks: Task<()>,
    _export: Task<()>,
    _attach: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for NoteView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteView")
            .field("id", &self.id)
            .field("query", &self.query)
            .field("banner", &self.banner)
            .field("popover", &self.popover)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<NoteEvent> for NoteView {}

impl NoteView {
    /// The view of note `id`, opened from the search `query`. `ui` is what the editor
    /// shows of its own: the Quick Note window is too small for the format bar.
    pub fn new(
        library: Entity<Library>,
        id: NoteId,
        query: String,
        ui: UiParts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let notes_dir = library.read(cx).notes_dir().to_owned();
        let lookup: SharedLookup = Arc::new(Mutex::new(Lookup {
            folder: notes_dir.clone(),
            ..Lookup::default()
        }));
        let note = cx.weak_entity();
        let editor = cx.new(|cx| {
            let slash = vec![template::slash_item(note)];
            let mut editor = MarkdownEditor::new(window, cx, setup::options(&lookup, slash, ui));
            editor.set_style(setup::style(cx), cx);
            editor
        });
        let subscriptions = vec![
            cx.observe_global::<Theme>(|this, cx| {
                let style = setup::style(cx);
                this.editor
                    .update(cx, |editor, cx| editor.set_style(style, cx));
            }),
            cx.observe_global::<Settings>(|this, cx| {
                let style = setup::style(cx);
                this.editor
                    .update(cx, |editor, cx| editor.set_style(style, cx));
            }),
            // GPUI runs no task while it quits, so the save happens right here.
            cx.on_app_quit(|this, cx| {
                this.save_now(cx);
                std::future::ready(())
            }),
            cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
                this.editor_event(event, cx);
            }),
            cx.subscribe(&library, |this, _, event: &LibraryEvent, cx| {
                let LibraryEvent::Changed { ids } = event;
                if ids.is_empty() || ids.contains(&this.id) {
                    this.send(Job::Check);
                }
                this.refresh_lookup(cx);
                this.refresh_backlinks(true, cx);
            }),
        ];

        let (jobs, queue) = async_channel::unbounded();
        let shared = worker::shared(id.clone());
        cx.spawn({
            let library = library.clone();
            let shared = shared.clone();
            async move |this, cx| worker::work(this, library, shared, queue, cx).await
        })
        .detach();

        let mut view = NoteView {
            library,
            notes_dir,
            id,
            meta: None,
            query,
            editor,
            loaded: false,
            focus_handle: cx.focus_handle(),
            jobs,
            shared,
            lookup,
            fresh: false,
            closed: false,
            closing: Arc::default(),
            leaving: false,
            trashing: false,
            title: String::new(),
            rename_pending: false,
            banner: None,
            popover: None,
            swipe: swipe::Swipe::default(),
            template_menu: None,
            _templates: Task::ready(()),
            saves: 0,
            saved_shown: false,
            _save_timer: Task::ready(()),
            _rename_timer: Task::ready(()),
            _lookup: Task::ready(()),
            _state: Task::ready(()),
            backlinks: Vec::new(),
            backlinks_title: String::new(),
            backlinks_open: false,
            _backlinks: Task::ready(()),
            _export: Task::ready(()),
            _attach: Task::ready(()),
            _subscriptions: subscriptions,
        };
        view.load_state(cx);
        view.refresh_lookup(cx);
        view
    }

    /// The id the note has now.
    pub fn id(&self) -> &NoteId {
        &self.id
    }

    pub fn meta(&self) -> Option<&NoteMeta> {
        self.meta.as_ref()
    }

    pub fn editor(&self) -> &Entity<MarkdownEditor> {
        &self.editor
    }

    pub fn banner(&self) -> Option<&Banner> {
        self.banner.as_ref()
    }

    pub fn popover(&self) -> Option<&Popover> {
        self.popover.as_ref()
    }

    /// Whether the text is there and the editor takes the keys.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Whether "Saved" is shown at the bottom.
    pub fn saved_shown(&self) -> bool {
        self.saved_shown
    }

    /// Says that the note was just created for this visit: left empty, it is deleted
    /// on leaving instead of staying behind as "Untitled".
    pub fn mark_new(&mut self) {
        self.fresh = true;
    }

    /// Leaving the note: stores what was typed and lets the file name follow the
    /// title; a new note that stayed empty is deleted. `None` if there is nothing to
    /// store and the view may go at once. Otherwise the task resolves to whether it may
    /// go: `false` if the text could not be stored, and the banner says why.
    pub fn leave(&mut self, cx: &mut Context<Self>) -> Option<Task<bool>> {
        if self.closed {
            return None;
        }
        if self.leaving || self.trashing {
            return Some(Task::ready(false));
        }
        let Some(leave) = self.leave_plan(cx) else {
            self.finish();
            return None;
        };
        let text = self.editor.read(cx).markdown();
        let (done, left) = async_channel::bounded(1);
        if self.jobs.try_send(Queued::Leave { leave, done }).is_err() {
            return Some(Task::ready(false));
        }
        self.leaving = true;
        self.closing.store(true, Ordering::SeqCst);
        self._save_timer = Task::ready(());
        self._rename_timer = Task::ready(());
        Some(cx.spawn(async move |this, cx| {
            let left = left.recv().await.unwrap_or(false);
            this.update(cx, |this, cx| this.left(left, &text, cx))
                .unwrap_or(left)
        }))
    }

    /// The last save, at once on this thread, for the window closing and the app
    /// quitting: GPUI runs no task after that, not even the worker. What cannot be
    /// stored in the notes folder goes to a recovery file. Nothing is written after
    /// this.
    pub fn save_now(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        if let Some(leave) = self.leave_plan(cx) {
            let text = self.editor.read(cx).markdown();
            let shared = self.shared.clone();
            let stored = self.library.update(cx, |library, cx| {
                library.run_now(cx, move |store, touched| {
                    let mut known = worker::lock(&shared);
                    if known.finished {
                        return Ok(());
                    }
                    known.leave(store, touched, leave)
                })
            });
            if let Err(err) = stored {
                self.recover(&text, &err, cx);
            }
        }
        self.finish();
    }

    /// What leaving has to store, if anything.
    fn leave_plan(&self, cx: &App) -> Option<Leave> {
        let editor = self.editor.read(cx);
        let body = editor.markdown();
        if self.fresh && body.trim().is_empty() {
            return Some(Leave::Discard(self.library.read(cx).now().timestamp()));
        }
        match self.banner {
            Some(Banner::ChangedOnDisk) => Some(Leave::KeepMine(body)),
            // Unsaved edits exist only here; without them the deletion was meant.
            Some(Banner::DeletedOnDisk) => {
                (editor.is_dirty() && !body.trim().is_empty()).then_some(Leave::SaveAsNew(body))
            }
            _ if editor.is_dirty() => Some(Leave::Save(body)),
            _ if self.rename_pending => Some(Leave::Rename),
            _ => None,
        }
    }

    /// The worker is done with leaving: the view may go if that stored everything and
    /// nothing was typed since.
    fn left(&mut self, left: bool, text: &str, cx: &mut Context<Self>) -> bool {
        self.leaving = false;
        if !left {
            self.closing = Arc::default();
            return false;
        }
        if self.editor.read(cx).markdown() != text {
            // Typed while leaving: stay, with what leaving stored as the note.
            self.closing = Arc::default();
            worker::lock(&self.shared).finished = false;
            self.banner = None;
            self._save_timer = self.after(SAVE_DELAY, false, cx);
            return false;
        }
        self.finish();
        true
    }

    /// Keeps the text in a recovery file after the last save failed.
    fn recover(&self, text: &str, err: &sheaf_core::Error, cx: &App) {
        let kept = cx
            .try_global::<Recovery>()
            .map(|recovery| recovery.keep(first_line(text), text));
        match kept {
            Some(Ok(path)) => log::error!(
                "cannot save the note {}: {err}; its text is in {}",
                self.id,
                path.display()
            ),
            Some(Err(kept)) => log::error!(
                "cannot save the note {} ({err}) nor keep it ({kept}), its text:\n{text}",
                self.id
            ),
            None => log::error!("cannot save the note {}: {err}, its text:\n{text}", self.id),
        }
    }

    /// Pins the note, or unpins it.
    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let Some(meta) = &mut self.meta else { return };
        meta.pinned = !meta.pinned;
        let pinned = meta.pinned;
        self.send(Job::Pin(pinned));
        cx.notify();
    }

    /// Moves the note to the trash, after saving what was typed. The window hears
    /// [`NoteEvent::Trashed`] when it is there; if it fails, the view stays as it was
    /// and a toast says why. A note that changed or went elsewhere stays until the
    /// banner is answered, so that neither text is lost.
    pub fn trash(&mut self, cx: &mut Context<Self>) {
        if self.closed || self.leaving || self.trashing {
            return;
        }
        self.popover = None;
        if !matches!(
            self.banner,
            Some(Banner::ChangedOnDisk | Banner::DeletedOnDisk)
        ) {
            let editor = self.editor.read(cx);
            let body = editor.is_dirty().then(|| editor.markdown());
            let now = self.library.read(cx).now().timestamp();
            self.trashing = true;
            self.send(Job::Trash { body, now });
        }
        cx.notify();
    }

    /// Moves the note to `folder`, relative to the notes folder.
    pub fn move_to(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.popover = None;
        if !self.closed {
            self.send(Job::Move(folder.to_owned()));
        }
        cx.notify();
    }

    /// "Copy as Markdown": the text as it is in the editor.
    pub fn copy_markdown(&mut self, cx: &mut Context<Self>) {
        self.popover = None;
        let markdown = self.editor.read(cx).markdown();
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        cx.emit(NoteEvent::Toast(Toast::new(tr("Copied as Markdown"))));
        cx.notify();
    }

    /// Opens the file manager at the note's file.
    pub fn show_in_files(&mut self, cx: &mut Context<Self>) {
        self.popover = None;
        if let Some(meta) = &self.meta {
            let path = self.notes_dir.join(&meta.rel_path);
            files::show(path, cx);
        }
        cx.notify();
    }

    /// Opens a popover under the header, or closes it with `None`. The folders of
    /// "Move to…" are looked up first.
    pub fn set_popover(&mut self, popover: Option<Popover>, cx: &mut Context<Self>) {
        if matches!(popover, Some(Popover::Move(_))) {
            let folders = self.library.read(cx).folders(cx);
            self._lookup = cx.spawn(async move |this, cx| {
                let folders = folders.await.unwrap_or_default();
                let _ = this.update(cx, |this, cx| {
                    this.popover = Some(Popover::Move(folders));
                    cx.notify();
                });
            });
            return;
        }
        self.popover = popover;
        cx.notify();
    }

    /// "Reload": drops the edits and shows the file as it is on disk.
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.banner != Some(Banner::ChangedOnDisk) {
            return;
        }
        self.send(Job::Reload);
        cx.notify();
    }

    /// "Keep Mine": keeps the edits as `Title (conflict).md` beside the note that
    /// changed on disk, and goes on editing that copy.
    pub fn keep_mine(&mut self, cx: &mut Context<Self>) {
        if self.banner != Some(Banner::ChangedOnDisk) {
            return;
        }
        let body = self.editor.read(cx).markdown();
        self.send(Job::KeepMine(body));
    }

    /// "Save as New Note" for a note deleted elsewhere.
    pub fn save_as_new(&mut self, cx: &mut Context<Self>) {
        if self.banner != Some(Banner::DeletedOnDisk) {
            return;
        }
        let body = self.editor.read(cx).markdown();
        self.send(Job::SaveAsNew(body));
    }

    /// "Try Again" after a save failed.
    pub fn retry_save(&mut self, cx: &mut Context<Self>) {
        if matches!(self.banner, Some(Banner::SaveFailed(_))) {
            self.save(true, cx);
        }
    }

    /// Nothing more is written.
    fn finish(&mut self) {
        self.closed = true;
        self.closing.store(true, Ordering::SeqCst);
        worker::lock(&self.shared).finished = true;
        self._save_timer = Task::ready(());
        self._rename_timer = Task::ready(());
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.try_send(Queued::Job(job));
    }

    fn folder(&self) -> String {
        self.meta
            .as_ref()
            .map(|meta| meta.folder.clone())
            .unwrap_or_default()
    }

    fn editor_event(&mut self, event: &EditorEvent, cx: &mut Context<Self>) {
        match event {
            EditorEvent::Changed => self.edited(cx),
            EditorEvent::MetaChanged => self.store_state(cx),
            EditorEvent::LinkClicked(url) => cx.open_url(url),
            EditorEvent::WikiLinkClicked(title) => self.open_title(title.clone(), cx),
            EditorEvent::TagClicked(tag) => cx.emit(NoteEvent::Search(format!("#{tag}"))),
            EditorEvent::AttachmentClicked(src) => {
                let lookup = self.lookup.lock().unwrap_or_else(PoisonError::into_inner);
                let path = lookup.folder.join(src);
                drop(lookup);
                cx.open_with_system(&path);
            }
            _ => {}
        }
    }

    /// Restarts the save delay, and the rename delay if the title changed.
    fn edited(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.saved_shown = false;
        let title = title(self.editor.read(cx));
        if title != self.title {
            self.title = title;
            self.rename_pending = true;
            self._rename_timer = self.after(RENAME_DELAY, true, cx);
        }
        self._save_timer = self.after(SAVE_DELAY, false, cx);
        cx.notify();
    }

    fn after(&self, delay: Duration, rename: bool, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| this.save(rename, cx));
        })
    }

    /// Hands the text to the worker if it changed; with `rename`, the file name also
    /// follows the title if that changed.
    fn save(&mut self, rename: bool, cx: &mut Context<Self>) {
        let asks = matches!(
            self.banner,
            Some(Banner::ChangedOnDisk | Banner::DeletedOnDisk)
        );
        if self.closed || self.leaving || asks {
            return;
        }
        let rename = rename && self.rename_pending;
        if rename {
            self.rename_pending = false;
        }
        if self.editor.read(cx).is_dirty() {
            let body = self.editor.read(cx).markdown();
            self.send(Job::Save { body, rename });
        } else if rename {
            self.send(Job::Rename);
        }
    }

    fn worker_reported(&mut self, report: Report, cx: &mut Context<Self>) {
        match report {
            Report::Loaded(note) => self.loaded(note, cx),
            Report::Saved { meta, body } => {
                self.took(meta);
                if matches!(self.banner, Some(Banner::SaveFailed(_))) {
                    self.banner = None;
                }
                self.saved(&body, cx);
            }
            Report::Written(meta) => self.took(meta),
            Report::Resolved { meta, body } => {
                self.took(meta);
                self.banner = None;
                self.saved(&body, cx);
            }
            Report::Conflict => self.ask(Banner::ChangedOnDisk),
            Report::Deleted => {
                // A note that cannot be read opens empty, to be written as a new one.
                self.loaded = true;
                self.ask(Banner::DeletedOnDisk);
            }
            Report::Trashed(meta) => {
                self.finish();
                cx.emit(NoteEvent::Trashed(meta));
            }
            Report::TrashFailed(reason) => {
                self.trashing = false;
                let message = tr("Could not move to trash: {}").replace("{}", &reason);
                cx.emit(NoteEvent::Toast(Toast::new(message)));
            }
            Report::Reloaded(note) => {
                self.banner = None;
                self.show(&note, cx);
            }
            Report::SaveFailed(reason) => {
                if !self.closed {
                    self.banner = Some(Banner::SaveFailed(reason));
                }
            }
            Report::ChangeFailed { change, reason } => self.change_failed(change, &reason, cx),
        }
        self.refresh_backlinks(false, cx);
        cx.notify();
    }

    /// A pin or move did not happen: the header shows the note as it is, a toast says
    /// why. A file name that did not follow the title loses nothing and goes unmentioned.
    fn change_failed(&mut self, change: Change, reason: &str, cx: &mut Context<Self>) {
        let message = match change {
            Change::Rename => return,
            Change::Pin(pinned) => {
                if let Some(meta) = &mut self.meta {
                    meta.pinned = !pinned;
                }
                if pinned {
                    tr("Could not pin the note: {}")
                } else {
                    tr("Could not unpin the note: {}")
                }
            }
            Change::Move => tr("Could not move the note: {}"),
            Change::Attach => tr("Could not attach: {}"),
        };
        cx.emit(NoteEvent::Toast(Toast::new(message.replace("{}", reason))));
    }

    /// Shows `banner`, unless the view is done with the note; a trash that was under
    /// way did not happen.
    fn ask(&mut self, banner: Banner) {
        self.trashing = false;
        if !self.closed {
            self.banner = Some(banner);
        }
    }

    /// `body` is on disk: if the editor still shows it, nothing is unsaved.
    fn saved(&mut self, body: &str, cx: &mut Context<Self>) {
        if self.editor.read(cx).markdown() == body {
            self.editor.update(cx, |editor, _| editor.mark_saved());
            self.saves += 1;
            self.saved_shown = true;
        }
    }

    /// The worker found the file changed by another program. Takes the new text and
    /// returns `true` if nothing typed would be lost; otherwise asks with the banner.
    fn changed_on_disk(&mut self, note: &Note, cx: &mut Context<Self>) -> bool {
        if self.closed || self.banner.is_some() {
            return false;
        }
        if self.editor.read(cx).is_dirty() {
            self.banner = Some(Banner::ChangedOnDisk);
            cx.notify();
            return false;
        }
        self.show(note, cx);
        true
    }

    /// Shows the text of the loaded note and, if it was opened from a search, marks
    /// the matches and scrolls to the first.
    fn loaded(&mut self, note: Note, cx: &mut Context<Self>) {
        self.show(&note, cx);
        self.loaded = true;
        let query = self.query.clone();
        self.editor.update(cx, |editor, cx| {
            let matches = jump::matches(editor, &query);
            if let Some(first) = matches.first().copied() {
                editor.scroll_to(first, cx);
            }
            editor.highlight(matches, cx);
        });
    }

    fn show(&mut self, note: &Note, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_markdown(note.body(), cx));
        self.title = title(self.editor.read(cx));
        self.took(note.meta.clone());
    }

    fn took(&mut self, meta: NoteMeta) {
        let folder = self.notes_dir.join(&meta.folder);
        self.lookup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .folder = folder;
        self.id = meta.id.clone();
        self.meta = Some(meta);
    }

    /// Opens the note with this title, creating it beside this one if there is none.
    fn open_title(&mut self, title: String, cx: &mut Context<Self>) {
        let listed = self.library.read(cx).notes(Vec::new(), Sort::Title, cx);
        let library = self.library.clone();
        let folder = self.folder();
        cx.spawn(async move |this, cx| {
            let found = listed
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|note| note.title.to_lowercase() == title.to_lowercase());
            let id = match found {
                Some(note) => note.id,
                None => {
                    let options = CreateOptions {
                        folder,
                        ..CreateOptions::default()
                    };
                    let created = library
                        .update(cx, |library, cx| library.create(&title, options, cx))
                        .await;
                    match created {
                        Ok(meta) => meta.id,
                        Err(err) => {
                            log::error!("cannot create the note {title}: {err}");
                            return;
                        }
                    }
                }
            };
            let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Open(id)));
        })
        .detach();
    }

    /// Keeps the titles and tags of the completions up to date.
    fn refresh_lookup(&mut self, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let notes = library.notes(Vec::new(), Sort::Title, cx);
        let tags = library.tags(cx);
        let lookup = self.lookup.clone();
        self._lookup = cx.background_spawn(async move {
            let titles: Vec<(String, String)> = notes
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|note| (note.title, note.folder))
                .collect();
            let tags = tags.await.unwrap_or_default();
            let mut lookup = lookup.lock().unwrap_or_else(PoisonError::into_inner);
            lookup.titles = titles;
            lookup.tags = tags;
        });
    }

    /// Keeps the column of text at the line width of the settings for the width of
    /// the window. The editor's column is fixed when it is created, so narrower lines
    /// are space beside the text.
    fn fit_column(&mut self, window: &Window, cx: &mut Context<Self>) {
        let frame = match window.window_decorations() {
            gpui::Decorations::Client { .. } if !window.is_maximized() => px(40.),
            _ => px(0.),
        };
        let width = window.viewport_size().width - frame;
        let side = setup::side_padding(width, settings::config(cx).editor.line_width);
        let padding = self.editor.read(cx).style().padding;
        if padding.left != side || padding.right != side {
            let mut style = self.editor.read(cx).style().clone();
            style.padding.left = side;
            style.padding.right = side;
            self.editor
                .update(cx, |editor, cx| editor.set_style(style, cx));
        }
    }

    /// Hands the editor what it remembered about this note, such as column widths.
    fn load_state(&mut self, cx: &mut Context<Self>) {
        let state = self.library.read(cx).note_state(&self.id, cx);
        self._state = cx.spawn(async move |this, cx| {
            let Ok(state) = state.await else { return };
            let Some(meta) = table_to_meta(state) else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.editor
                    .update(cx, |editor, cx| editor.set_meta(meta, cx));
            });
        });
    }

    fn store_state(&mut self, cx: &mut Context<Self>) {
        let Some(state) = meta_to_table(self.editor.read(cx).meta()) else {
            return;
        };
        let id = self.id.clone();
        let stored = self.library.read(cx).update_folder_config(
            move |config| {
                if state.is_empty() {
                    config.notes.remove(&id);
                } else {
                    config.notes.insert(id, state);
                }
            },
            cx,
        );
        cx.background_spawn(async move {
            if let Err(err) = stored.await {
                log::warn!("cannot remember the view of a note: {err}");
            }
        })
        .detach();
    }
}

/// The first line of a text, where the title is.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// The text of the editor's first block, the title, without Markdown syntax.
fn title(editor: &MarkdownEditor) -> String {
    let first = editor.document().blocks().first();
    first
        .map(|block| block.text().to_owned())
        .unwrap_or_default()
}

/// The editor's view state as it is kept in the folder configuration: TOML, whose
/// keys are strings, by way of JSON, which turns the numbered keys into strings and
/// back.
fn meta_to_table(meta: &DocMeta) -> Option<toml::Table> {
    if meta.is_empty() {
        return Some(toml::Table::new());
    }
    let json = serde_json::to_value(meta).ok()?;
    toml::Table::try_from(json).ok()
}

fn table_to_meta(table: toml::Table) -> Option<DocMeta> {
    if table.is_empty() {
        return None;
    }
    serde_json::from_value(serde_json::to_value(table).ok()?).ok()
}

impl Focusable for NoteView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.loaded {
            self.editor.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl Render for NoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.loaded && self.focus_handle.is_focused(window) {
            self.editor.focus_handle(cx).focus(window, cx);
        }
        self.fit_column(window, cx);
        let tokens = cx.tokens().clone();
        let date = self.meta.as_ref().map(|meta| {
            let zone = self.library.read(cx).now().time_zone().clone();
            date::long_date_time(&meta.modified.to_zoned_datetime(zone))
        });
        let conflict = self.meta.as_ref().is_some_and(|meta| meta.conflict);
        let saved = self.saved_shown.then(|| {
            div()
                .absolute()
                .bottom(px(12.))
                .right(px(18.))
                .text_size(tokens.text(12.))
                .text_color(tokens.faint)
                .child(tr("Saved"))
                .with_animation(
                    ("saved", self.saves),
                    tokens.ease_out(FADE),
                    |label, delta| label.opacity(delta),
                )
        });

        pane(tokens.view, window, cx)
            .relative()
            .track_focus(&self.focus_handle)
            .key_context(actions::NOTE)
            .on_action(cx.listener(|this, _: &Back, _, cx| {
                // Esc closes a popover before it leaves the note.
                if this.popover.is_some() {
                    this.set_popover(None, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Trash, _, cx| this.trash(cx)))
            .on_action(cx.listener(|this, _: &ExportMarkdown, _, cx| this.export(Format::Markdown, cx)))
            .on_action(cx.listener(|this, _: &ExportPdf, _, cx| this.export(Format::Pdf, cx)))
            .on_action(cx.listener(|this, _: &Print, _, cx| this.print(cx)))
            .on_action(cx.listener(|this, _: &AttachFile, window, cx| this.attach_file(window, cx)))
            .on_action(cx.listener(|this, _: &PasteAttachment, window, cx| this.paste(window, cx)))
            .child(header::header(self, cx))
            .children(self.banner.clone().map(|banner| header::banner(banner, cx)))
            .child(
                div()
                    .flex_none()
                    .pt(px(18.))
                    .pb(px(14.))
                    .text_center()
                    .text_size(tokens.text(12.))
                    .text_color(tokens.faint)
                    .child(date.unwrap_or_default())
                    .when(conflict, |line| {
                        line.child(
                            div()
                                .debug_selector(|| "conflict-hint".into())
                                .flex()
                                .justify_center()
                                .items_center()
                                .gap(px(5.))
                                .mt(px(4.))
                                .child(icon(IconName::Warning, tokens.faint).size(px(12.)))
                                .child(tr(
                                    "Conflict copy from syncing: move what you need into the original, then delete it.",
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .id("note-text")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when(self.loaded, |area| area.child(self.editor.clone()))
                    .child(swipe::listener(cx.entity()))
                    .when(cx.has_active_drag(), |area| {
                        area.child(attach::drop_layer(cx.entity()))
                    }),
            )
            .children(self.backlinks_section(cx))
            .children(saved)
            .children(template::menu(
                &cx.entity(),
                self.template_menu.as_ref(),
                cx,
            ))
            .when(self.popover.is_some(), |pane| {
                pane.child(header::dismiss_layer(cx))
            })
    }
}
