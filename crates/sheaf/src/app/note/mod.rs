//! One note, open for editing. For now the header bar and the editor, which proves the
//! seams; the date line, format bar, backlinks and export of plan 03 §7 are added here.
//!
//! # Interface
//!
//! - The window creates a [`NoteView`] per opened note and drops it after it has slid
//!   out. Before that it calls [`flush`](NoteView::flush), which saves what was typed.
//! - Everything that writes the open note goes through one worker task in file order:
//!   saving, renaming, pinning. Add new writes as a [`Job`], not as a direct call to
//!   the [`Library`]: a write behind the worker's back changes the file under it and
//!   the next save would look like a conflict.
//! - The id of a note can change with its first save (see `NoteId`);
//!   [`id`](NoteView::id) is always the current one.
//! - Leaving is the window's `Back` action; keys reach the view through the
//!   [`actions::NOTE`] key context.

use async_channel::{Receiver, Sender};
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, WeakEntity, Window, div,
};
use gpui_component::Theme;
use gpui_markdown_editor::extensions::{Highlight, Tag, WikiLink};
use gpui_markdown_editor::{EditorOptions, EditorStyle, MarkdownEditor};
use sheaf_core::i18n::tr;
use sheaf_core::{Error, Fingerprint, NoteId, NoteMeta};

use super::actions::{self, Back, TogglePin};
use super::assets::IconName;
use super::chrome::{FlatButton, HeaderBar, pane};
use super::library::Library;
use super::theme::{ActiveTokens as _, size};

/// A write to the open note, done by the worker in the order given.
enum Job {
    /// Store this text; then tell `done`, if anyone waits.
    Save {
        body: String,
        /// Also let the file name follow the title. Done when leaving, not on every
        /// save, so the file does not jump around while the title is being typed.
        rename: bool,
        done: Option<Sender<()>>,
    },
    Pin(bool),
}

pub struct NoteView {
    id: NoteId,
    /// What is known about the note; `None` until it is loaded.
    meta: Option<NoteMeta>,
    /// The search the note was opened from, to jump to once the text is there.
    query: String,
    editor: Entity<MarkdownEditor>,
    jobs: Sender<Job>,
    _theme_changed: Subscription,
    _save_on_quit: Subscription,
}

impl std::fmt::Debug for NoteView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteView")
            .field("id", &self.id)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

impl NoteView {
    pub fn new(
        library: Entity<Library>,
        id: NoteId,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = EditorOptions {
            first_line_is_title: true,
            max_width: Some(size::EDITOR_MAX_WIDTH),
            extensions: vec![Box::new(WikiLink), Box::new(Tag), Box::new(Highlight)],
            ..Default::default()
        };
        let editor = cx.new(|cx| {
            let mut editor = MarkdownEditor::new(window, cx, options);
            editor.set_style(editor_style(cx), cx);
            editor
        });
        let theme_changed = cx.observe_global::<Theme>(|this, cx| {
            let style = editor_style(cx);
            this.editor
                .update(cx, |editor, cx| editor.set_style(style, cx));
        });
        // The app waits for this before it exits, so nothing typed is lost.
        let save_on_quit = cx.on_app_quit(|this, cx| this.flush(cx));

        let (jobs, queue) = async_channel::unbounded();
        cx.spawn({
            let id = id.clone();
            async move |this, cx| work(this, library, id, queue, cx).await
        })
        .detach();

        NoteView {
            id,
            meta: None,
            query,
            editor,
            jobs,
            _theme_changed: theme_changed,
            _save_on_quit: save_on_quit,
        }
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

    /// Saves what was typed and lets the file name follow the title. Resolves when the
    /// file is written; the write also happens if the view is dropped before that.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let dirty = self.editor.read(cx).is_dirty();
        if !dirty {
            return Task::ready(());
        }
        let body = self.editor.read(cx).markdown();
        self.editor.update(cx, |editor, _| editor.mark_saved());
        let (done, saved) = async_channel::bounded(1);
        let job = Job::Save {
            body,
            rename: true,
            done: Some(done),
        };
        if self.jobs.try_send(job).is_err() {
            return Task::ready(());
        }
        cx.background_spawn(async move {
            let _ = saved.recv().await;
        })
    }

    /// Pins the note, or unpins it.
    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        let Some(meta) = &mut self.meta else { return };
        meta.pinned = !meta.pinned;
        let _ = self.jobs.try_send(Job::Pin(meta.pinned));
        cx.notify();
    }

    /// Shows the text of the loaded note and, if it was opened from a search, its
    /// first match.
    fn loaded(&mut self, meta: NoteMeta, body: &str, cx: &mut Context<Self>) {
        let query = self.query.clone();
        self.editor.update(cx, |editor, cx| {
            editor.set_markdown(body, cx);
            let matches = editor.find(&query);
            if let Some(first) = matches.first().copied() {
                editor.scroll_to(first, cx);
            }
            editor.highlight(matches, cx);
        });
        self.written(meta, cx);
    }

    fn written(&mut self, meta: NoteMeta, cx: &mut Context<Self>) {
        self.id = meta.id.clone();
        self.meta = Some(meta);
        cx.notify();
    }
}

/// The worker of one open note: loads it, then does its [`Job`]s one after the other
/// until the view is gone and the queue is empty.
async fn work(
    view: WeakEntity<NoteView>,
    library: Entity<Library>,
    mut id: NoteId,
    queue: Receiver<Job>,
    cx: &mut AsyncApp,
) {
    let note = library
        .read_with(cx, |library, cx| library.load(&id, cx))
        .await;
    let mut fingerprint = match note {
        Ok(note) => {
            let fingerprint = note.fingerprint;
            let body = note.body().to_owned();
            let _ = view.update(cx, |view, cx| view.loaded(note.meta, &body, cx));
            Some(fingerprint)
        }
        Err(err) => {
            log::error!("cannot open the note {id}: {err}");
            None
        }
    };

    while let Ok(job) = queue.recv().await {
        let written = match job {
            Job::Save { body, rename, done } => {
                let saved = save(&library, &id, body, fingerprint, rename, cx).await;
                if let Some(done) = done {
                    let _ = done.try_send(());
                }
                saved
            }
            Job::Pin(pinned) => {
                library
                    .update(cx, |library, cx| library.set_pinned(&id, pinned, cx))
                    .await
            }
        };
        let meta = match written {
            Ok(meta) => meta,
            Err(err) => {
                log::error!("cannot write the note {id}: {err}");
                continue;
            }
        };
        id = meta.id.clone();
        // Every write changes the file; the next save must expect it as it is now.
        fingerprint = library
            .read_with(cx, |library, cx| library.load(&id, cx))
            .await
            .ok()
            .map(|note| note.fingerprint);
        let _ = view.update(cx, |view, cx| view.written(meta, cx));
    }
}

/// Stores `body`. If the file changed on disk since it was read, the text goes into a
/// copy beside it instead, so neither version is lost.
async fn save(
    library: &Entity<Library>,
    id: &NoteId,
    body: String,
    expected: Option<Fingerprint>,
    rename: bool,
    cx: &mut AsyncApp,
) -> sheaf_core::Result<NoteMeta> {
    let saved = library
        .update(cx, |library, cx| {
            library.save_body(id, body.clone(), expected, cx)
        })
        .await;
    let meta = match saved {
        Ok(saved) => saved.meta,
        Err(Error::Conflict(_)) => {
            log::warn!("the note {id} changed on disk, keeping the edits as a copy");
            return library
                .update(cx, |library, cx| library.save_conflict_copy(id, body, cx))
                .await;
        }
        Err(err) => return Err(err),
    };
    if !rename {
        return Ok(meta);
    }
    library
        .update(cx, |library, cx| library.sync_filename(&meta.id, cx))
        .await
}

/// The editor's style in the colours and at the text scale of the theme.
fn editor_style(cx: &App) -> EditorStyle {
    let tokens = cx.tokens();
    let mut style = EditorStyle::from_theme(cx);
    style.text_color = tokens.fg;
    style.muted_color = tokens.muted;
    style.faint_color = tokens.faint;
    style.checkbox_border_color = tokens.faint;
    style.table_border_color = tokens.hair;
    style.table_header_background = tokens.hover;
    style.text_size = tokens.text(15.5);
    style.title_size = tokens.text(28.);
    style.h2_size = tokens.text(19.);
    style.h3_size = tokens.text(16.);
    style.code_size = tokens.text(14.);
    style
}

impl Focusable for NoteView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for NoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let pinned = self.meta.as_ref().is_some_and(|meta| meta.pinned);
        let header = HeaderBar::new()
            .start(
                FlatButton::new("back", IconName::Back)
                    .tooltip(tr("Notes"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Back), cx)),
            )
            .end(
                FlatButton::new("pin", IconName::Pin)
                    .tooltip(if pinned { tr("Unpin") } else { tr("Pin") })
                    .active(pinned)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(TogglePin), cx)),
            )
            .end(FlatButton::new("share", IconName::Share).tooltip(tr("Share")))
            .end(FlatButton::new("more", IconName::More).tooltip(tr("More")));

        pane(tokens.view, window, cx)
            .key_context(actions::NOTE)
            .child(header)
            .child(div().flex_1().min_h_0().child(self.editor.clone()))
    }
}
