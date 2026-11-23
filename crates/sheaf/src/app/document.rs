//! A Markdown file from outside the notes folder, edited where it is, in a window of
//! its own: opening a `.md` file from the file manager or with `sheaf FILE` comes here
//! unless the file is a note.
//!
//! # Interface
//!
//! - [`open`] shows the window of a file, or brings the one already open for it to the
//!   front. Every file gets one window.
//! - The file stays a plain file: it gets no id, no frontmatter and is neither indexed
//!   nor renamed nor moved; see [`sheaf_core::document`].
//! - Ctrl+S saves at once, Ctrl+W and the close button save and close.
//!
//! # Saving
//!
//! As in the note view: 600 ms after the last change, at once on Ctrl+S and when the
//! window closes or the app quits. The writes and the checks for changes made by other
//! programs run one after another on a [`Serial`]; the file is checked every
//! [`CHECK_INTERVAL`]. A change on disk is taken silently while nothing is unsaved,
//! otherwise the banner asks: "Reload" or "Keep Mine", which overwrites the file. A
//! file deleted elsewhere is written again only on "Save Again". What cannot be written
//! when the window closes goes to a recovery file, like a note.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use gpui::{
    AnimationExt as _, AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Entity,
    FocusHandle, Focusable, FontWeight, Global, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowDecorations, WindowHandle,
    WindowOptions, deferred, div, prelude::FluentBuilder as _, px, size as size_of,
};
use gpui_component::{Root, Theme};
use gpui_markdown_editor::extensions::Highlight;
use gpui_markdown_editor::{EditorEvent, EditorOptions, EditorStrings, MarkdownEditor, UiParts};
use sheaf_core::document::{self, Checked, Document, Saved};
use sheaf_core::i18n::tr;
use sheaf_searchd::api::APP_NAME;

use super::actions::{self, CloseDocument, CloseWindow, PasteAttachment, Print, SaveDocument};
use super::assets::{IconName, icon};
use super::chrome::{FlatButton, HeaderBar, pane};
use super::note::export::{self, Format, Source};
use super::note::{date, files, setup};
use super::overlays::{self, MenuRow, Toast, ToastHost};
use super::serial::Serial;
use super::settings::{self, Settings};
use super::state::Recovery;
use super::theme::{ActiveTokens as _, size};

/// How long typing may pause before the file is written.
pub const SAVE_DELAY: Duration = Duration::from_millis(600);
/// How often the file is checked for changes by other programs.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// "Saved" fading in.
const FADE: Duration = Duration::from_millis(300);
/// The frame of a new window, without the shadow around it.
const WIDTH: f32 = 860.;
const HEIGHT: f32 = 760.;
/// The shadow gpui-component draws around a client-decorated window, on every side.
const FRAME_SHADOW: f32 = 20.;
const POPOVER_WIDTH: f32 = 250.;

/// The document windows that are open, by file.
#[derive(Default)]
struct Documents {
    open: Vec<(PathBuf, WindowHandle<Root>, Entity<DocumentView>)>,
}

impl Global for Documents {}

/// The window that shows the file at `path`, if one is open.
pub fn current(path: &Path, cx: &App) -> Option<(WindowHandle<Root>, Entity<DocumentView>)> {
    let path = canonical(path);
    let documents = cx.try_global::<Documents>()?;
    documents
        .open
        .iter()
        .find(|(open, _, _)| *open == path)
        .map(|(_, handle, view)| (*handle, view.clone()))
}

/// Shows the file at `path` in a window of its own, or brings the window that already
/// shows it to the front.
pub fn open(
    path: &Path,
    cx: &mut App,
) -> anyhow::Result<(WindowHandle<Root>, Entity<DocumentView>)> {
    let path = canonical(path);
    if let Some((handle, view)) = current(&path, cx)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Ok((handle, view));
    }
    let frame = size_of(
        px(WIDTH + 2. * FRAME_SHADOW),
        px(HEIGHT + 2. * FRAME_SHADOW),
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, frame, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(file_name(&path).into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size_of(size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT)),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(APP_NAME.to_owned()),
        ..Default::default()
    };
    let mut built = None;
    let handle = cx.open_window(options, |window, cx| {
        let (root, view) = build(path.clone(), window, cx);
        built = Some(view);
        root
    })?;
    let view = built.ok_or_else(|| anyhow::anyhow!("the window was not built"))?;
    let documents = cx.default_global::<Documents>();
    documents.open.retain(|(open, _, _)| *open != path);
    documents.open.push((path, handle, view.clone()));
    Ok((handle, view))
}

/// The content of a document window inside gpui-component's `Root`. [`open`] uses it;
/// tests call it for their own window.
pub fn build(
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) -> (Entity<Root>, Entity<DocumentView>) {
    let view = cx.new(|cx| DocumentView::new(path, window, cx));
    window.on_window_should_close(cx, {
        let view = view.downgrade();
        move |_, cx| {
            let _ = view.update(cx, |view, cx| view.close_now(cx));
            true
        }
    });
    let root = cx.new(|cx| Root::new(view.clone(), window, cx));
    (root, view)
}

/// `path` with symbolic links resolved, so that one file gets one window.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// What the view has to tell about the file, under the header bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Banner {
    /// Another program changed the file while there were unsaved edits.
    ChangedOnDisk,
    /// Another program deleted the file.
    DeletedOnDisk,
    /// The text could not be written; `0` says why.
    SaveFailed(String),
}

/// The file as the background jobs know it.
#[derive(Debug, Default)]
struct Known {
    /// The file as last read or written; `None` until it is loaded.
    document: Option<Document>,
    /// The window closed: nothing more is written.
    finished: bool,
}

type Shared = Arc<Mutex<Known>>;

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Known> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a background job found.
enum Outcome {
    Saved { body: String },
    Changed(Document),
    Deleted,
    Same,
    Failed(String),
}

pub struct DocumentView {
    path: PathBuf,
    editor: Entity<MarkdownEditor>,
    /// The folder of the file, which relative image paths start from.
    lookup: setup::SharedLookup,
    shared: Shared,
    serial: Serial,
    /// Whether the text is there. Until then the editor is not drawn.
    loaded: bool,
    /// Why the file could not be opened.
    error: Option<String>,
    banner: Option<Banner>,
    /// When the file was last changed, for the line under the header.
    modified: Option<jiff::Timestamp>,
    menu_open: bool,
    /// A write is under way; another one waits for it.
    saving: bool,
    save_again: bool,
    closed: bool,
    /// Counts the saves; "Saved" fades in afresh after each one.
    saves: usize,
    saved_shown: bool,
    focus_handle: FocusHandle,
    toasts: Entity<ToastHost>,
    _save_timer: Task<()>,
    _checks: Task<()>,
    _check: Task<()>,
    _job: Task<()>,
    _export: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for DocumentView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentView")
            .field("path", &self.path)
            .field("banner", &self.banner)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl DocumentView {
    fn new(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = path.parent().map(Path::to_owned).unwrap_or_default();
        let lookup: setup::SharedLookup = Arc::new(Mutex::new(setup::Lookup {
            folder,
            ..setup::Lookup::default()
        }));
        let editor = cx.new(|cx| {
            let mut editor = MarkdownEditor::new(window, cx, editor_options(&lookup));
            editor.set_style(setup::style(cx), cx);
            editor
        });
        let subscriptions = vec![
            cx.observe_global::<Theme>(|this, cx| this.restyle(cx)),
            cx.observe_global::<Settings>(|this, cx| this.restyle(cx)),
            // GPUI runs no task while it quits, so the last write happens right here.
            cx.on_app_quit(|this, cx| {
                this.close_now(cx);
                std::future::ready(())
            }),
            cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
                this.editor_event(event, cx);
            }),
        ];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut view = DocumentView {
            path,
            editor,
            lookup,
            shared: Shared::default(),
            serial: Serial::new(cx),
            loaded: false,
            error: None,
            banner: None,
            modified: None,
            menu_open: false,
            saving: false,
            save_again: false,
            closed: false,
            saves: 0,
            saved_shown: false,
            focus_handle,
            toasts: cx.new(|_| ToastHost::default()),
            _save_timer: Task::ready(()),
            _checks: Task::ready(()),
            _check: Task::ready(()),
            _job: Task::ready(()),
            _export: Task::ready(()),
            _subscriptions: subscriptions,
        };
        view.load(cx);
        view
    }

    /// The file this window shows.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn editor(&self) -> &Entity<MarkdownEditor> {
        &self.editor
    }

    pub fn banner(&self) -> Option<&Banner> {
        self.banner.as_ref()
    }

    /// Why the file could not be opened, if it could not.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Whether "Saved" is shown at the bottom.
    pub fn saved_shown(&self) -> bool {
        self.saved_shown
    }

    fn restyle(&mut self, cx: &mut Context<Self>) {
        let style = setup::style(cx);
        self.editor
            .update(cx, |editor, cx| editor.set_style(style, cx));
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let shared = self.shared.clone();
        let loaded = self.serial.run(
            move || {
                let document = Document::load(&path)?;
                lock(&shared).document = Some(document.clone());
                Ok::<_, sheaf_core::Error>(document)
            },
            cx,
        );
        self._job = cx.spawn(async move |this, cx| {
            let Some(loaded) = loaded.await else { return };
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => {
                        this.show(&document, cx);
                        this.loaded = true;
                        this.watch(cx);
                    }
                    Err(err) => {
                        log::error!("cannot open {}: {err}", this.path.display());
                        this.error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        });
    }

    /// Shows the text of `document` in the editor, as saved.
    fn show(&mut self, document: &Document, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_markdown(document.body(), cx));
        self.modified = Some(document.modified());
    }

    /// Checks the file for changes by other programs every [`CHECK_INTERVAL`].
    fn watch(&mut self, cx: &mut Context<Self>) {
        self._checks = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let Ok(()) = this.update(cx, |this, cx| this.check(cx)) else {
                    return;
                };
            }
        });
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        if self.closed || self.saving || self.banner.is_some() {
            return;
        }
        let shared = self.shared.clone();
        let checked = self.serial.run(
            move || {
                let known = lock(&shared);
                let Some(document) = &known.document else {
                    return Outcome::Same;
                };
                match document::check(document) {
                    Ok(Checked::Same) => Outcome::Same,
                    Ok(Checked::Changed(now)) => Outcome::Changed(now),
                    Ok(Checked::Deleted) => Outcome::Deleted,
                    Err(err) => {
                        log::warn!("cannot check {}: {err}", document.path.display());
                        Outcome::Same
                    }
                }
            },
            cx,
        );
        self._check = cx.spawn(async move |this, cx| {
            let Some(outcome) = checked.await else { return };
            let _ = this.update(cx, |this, cx| this.found(outcome, cx));
        });
    }

    fn editor_event(&mut self, event: &EditorEvent, cx: &mut Context<Self>) {
        match event {
            EditorEvent::Changed => self.edited(cx),
            EditorEvent::LinkClicked(url) => self.open_link(url, cx),
            EditorEvent::AttachmentClicked(src) => self.open_link(src, cx),
            _ => {}
        }
    }

    /// A web address opens in the browser, a file relative to this one with its app.
    fn open_link(&self, target: &str, cx: &mut Context<Self>) {
        if target.contains("://") || target.starts_with("mailto:") {
            cx.open_url(target);
            return;
        }
        let relative = sheaf_core::scan::percent_decode(target.split('#').next().unwrap_or(""));
        if relative.is_empty() {
            return;
        }
        let folder = self
            .lookup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .folder
            .clone();
        cx.open_with_system(&folder.join(relative));
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.saved_shown = false;
        self._save_timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, cx| this.save(false, cx));
        });
        cx.notify();
    }

    /// Writes the text if it changed; with `force` over a change made elsewhere, or
    /// anew after the file was deleted.
    pub fn save(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.closed || !self.loaded {
            return;
        }
        if !force
            && matches!(
                self.banner,
                Some(Banner::ChangedOnDisk | Banner::DeletedOnDisk)
            )
        {
            return;
        }
        if !force && !self.editor.read(cx).is_dirty() {
            return;
        }
        if self.saving {
            self.save_again = true;
            return;
        }
        self.saving = true;
        self._save_timer = Task::ready(());
        let body = self.editor.read(cx).markdown();
        let shared = self.shared.clone();
        let written = self.serial.run(move || write(&shared, &body, force), cx);
        cx.spawn(async move |this, cx| {
            let outcome = written
                .await
                .unwrap_or_else(|| Outcome::Failed(tr("the app is closing")));
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.found(outcome, cx);
                if std::mem::take(&mut this.save_again) {
                    this.save(false, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Takes in what a write or a check found.
    fn found(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        match outcome {
            Outcome::Same => return,
            Outcome::Saved { body } => {
                self.banner = None;
                self.modified = lock(&self.shared).document.as_ref().map(Document::modified);
                if self.editor.read(cx).markdown() == body {
                    self.editor.update(cx, |editor, _| editor.mark_saved());
                    self.saves += 1;
                    self.saved_shown = true;
                }
            }
            Outcome::Changed(now) => {
                if self.editor.read(cx).is_dirty() {
                    self.banner = Some(Banner::ChangedOnDisk);
                } else {
                    lock(&self.shared).document = Some(now.clone());
                    self.show(&now, cx);
                }
            }
            Outcome::Deleted => self.banner = Some(Banner::DeletedOnDisk),
            Outcome::Failed(reason) => self.banner = Some(Banner::SaveFailed(reason)),
        }
        cx.notify();
    }

    /// "Reload": drops the edits and shows the file as it is on disk.
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.banner != Some(Banner::ChangedOnDisk) {
            return;
        }
        let path = self.path.clone();
        let shared = self.shared.clone();
        let loaded = self.serial.run(
            move || {
                let document = Document::load(&path)?;
                lock(&shared).document = Some(document.clone());
                Ok::<_, sheaf_core::Error>(document)
            },
            cx,
        );
        self._job = cx.spawn(async move |this, cx| {
            let Some(loaded) = loaded.await else { return };
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => {
                        this.banner = None;
                        this.show(&document, cx);
                    }
                    Err(err) => this.banner = Some(Banner::SaveFailed(err.to_string())),
                }
                cx.notify();
            });
        });
    }

    /// "Keep Mine" and "Save Again": writes the text over what is on disk.
    pub fn keep_mine(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.banner,
            Some(Banner::ChangedOnDisk | Banner::DeletedOnDisk)
        ) {
            self.save(true, cx);
        }
    }

    /// "Try Again" after a write failed.
    pub fn retry_save(&mut self, cx: &mut Context<Self>) {
        if matches!(self.banner, Some(Banner::SaveFailed(_))) {
            self.save(false, cx);
        }
    }

    /// The last write, at once on this thread, for the window closing and the app
    /// quitting: GPUI runs no task after that. What cannot be written to the file goes
    /// to a recovery file. Nothing is written after this.
    pub fn close_now(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self._save_timer = Task::ready(());
        self._checks = Task::ready(());
        if let Some(documents) = cx.try_global::<Documents>()
            && documents.open.iter().any(|(path, _, _)| *path == self.path)
        {
            cx.global_mut::<Documents>()
                .open
                .retain(|(path, _, _)| *path != self.path);
        }
        let editor = self.editor.read(cx);
        let dirty = self.loaded && editor.is_dirty();
        let body = editor.markdown();
        // Waits for a write that is under way; nothing queued after it writes.
        let mut known = lock(&self.shared);
        known.finished = true;
        if !dirty {
            return;
        }
        let outcome = match (&self.banner, &known.document) {
            (Some(Banner::ChangedOnDisk | Banner::DeletedOnDisk), _) | (_, None) => None,
            (_, Some(document)) => match document::save(document, &body, false) {
                Ok(Saved::Written(_)) => return,
                Ok(Saved::Changed(_) | Saved::Deleted) => None,
                Err(err) => Some(err.to_string()),
            },
        };
        drop(known);
        self.recover(&body, outcome.as_deref(), cx);
    }

    /// Keeps the text in a recovery file after the last write could not happen.
    fn recover(&self, text: &str, reason: Option<&str>, cx: &App) {
        let reason = reason.unwrap_or("the file changed elsewhere");
        let name = file_name(&self.path);
        match cx
            .try_global::<Recovery>()
            .map(|recovery| recovery.keep(&name, text))
        {
            Some(Ok(kept)) => log::error!(
                "cannot write {} ({reason}); the text is in {}",
                self.path.display(),
                kept.display()
            ),
            Some(Err(err)) => log::error!(
                "cannot write {} ({reason}) nor keep it ({err}), the text:\n{text}",
                self.path.display()
            ),
            None => log::error!(
                "cannot write {} ({reason}), the text:\n{text}",
                self.path.display()
            ),
        }
    }

    /// Ctrl+W and the close button: writes the text, then closes the window.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_now(cx);
        window.remove_window();
    }

    fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_open = !self.menu_open;
        cx.notify();
    }

    fn copy_markdown(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let markdown = self.editor.read(cx).markdown();
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        self.toast(Toast::new(tr("Copied as Markdown")), cx);
    }

    fn show_in_files(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        files::show(self.path.clone(), cx);
        cx.notify();
    }

    fn source(&self, cx: &App) -> Source {
        let zone = jiff::tz::TimeZone::system();
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = self
            .lookup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .folder
            .clone();
        Source::new(
            stem,
            self.editor.read(cx).markdown(),
            folder,
            self.modified
                .map(|modified| date::long_date_time(&modified.to_zoned_datetime(zone))),
        )
    }

    fn export_pdf(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let exported = export::export_one(self.source(cx), Format::Pdf, cx);
        self._export = cx.spawn(async move |this, cx| {
            if let Some(toast) = exported.await {
                let _ = this.update(cx, |this, cx| this.toast(toast, cx));
            }
        });
        cx.notify();
    }

    fn print(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let printed = export::print_one(self.source(cx), cx);
        self._export = cx.spawn(async move |this, cx| {
            if let Some(toast) = printed.await {
                let _ = this.update(cx, |this, cx| this.toast(toast, cx));
            }
        });
        cx.notify();
    }

    fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toasts.update(cx, |toasts, cx| toasts.show(toast, cx));
    }

    /// Keeps the column of text at the line width of the settings, as the note view.
    fn fit_column(&mut self, window: &Window, cx: &mut Context<Self>) {
        let frame = match window.window_decorations() {
            gpui::Decorations::Client { .. } if !window.is_maximized() => px(40.),
            _ => px(0.),
        };
        let width = window.viewport_size().width - frame;
        let side = setup::side_padding(width, settings::config(cx).editor.line_width);
        let padding = self.editor.read(cx).style().padding;
        if padding.left != side || padding.right != side || padding.top != px(8.) {
            let mut style = self.editor.read(cx).style().clone();
            style.padding.left = side;
            style.padding.right = side;
            style.padding.top = px(8.);
            self.editor
                .update(cx, |editor, cx| editor.set_style(style, cx));
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu_open.then(|| self.render_menu(cx));
        HeaderBar::new()
            .start(
                div()
                    .flex()
                    .items_center()
                    .pl(px(8.))
                    .child(icon(IconName::Text, cx.tokens().faint)),
            )
            .title(SharedString::from(file_name(&self.path)))
            .end(
                div()
                    .relative()
                    .child(
                        FlatButton::new("document-menu", IconName::More)
                            .tooltip(tr("More"))
                            .active(self.menu_open)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx))),
                    )
                    .when_some(menu, |anchor, menu| {
                        anchor.child(
                            deferred(
                                div()
                                    .absolute()
                                    .top(size::FLAT_BUTTON + px(6.))
                                    .right_0()
                                    .w(px(POPOVER_WIDTH))
                                    .child(menu),
                            )
                            .with_priority(2),
                        )
                    }),
            )
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        overlays::popover("document-popover", cx)
            .child(
                MenuRow::new("document-save", tr("Save"))
                    .icon(IconName::SaveAs)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_open = false;
                        this.save(false, cx);
                    })),
            )
            .child(
                MenuRow::new("document-show-in-files", tr("Show in Files"))
                    .icon(IconName::Folder)
                    .on_click(cx.listener(|this, _, _, cx| this.show_in_files(cx))),
            )
            .child(overlays::separator(cx))
            .child(
                MenuRow::new("document-copy", tr("Copy as Markdown"))
                    .icon(IconName::Copy)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_markdown(cx))),
            )
            .child(
                MenuRow::new("document-pdf", tr("Export as PDF…"))
                    .icon(IconName::SaveAs)
                    .on_click(cx.listener(|this, _, _, cx| this.export_pdf(cx))),
            )
            .child(
                MenuRow::new("document-print", tr("Print…"))
                    .icon(IconName::Print)
                    .on_click(cx.listener(|this, _, _, cx| this.print(cx))),
            )
            .into_any_element()
    }

    /// When the file was last changed, under the header, as the date line of a note.
    fn render_subtitle(&self, cx: &App) -> impl IntoElement {
        let tokens = cx.tokens();
        let line = self
            .modified
            .map(|modified| {
                date::long_date_time(&modified.to_zoned_datetime(jiff::tz::TimeZone::system()))
            })
            .unwrap_or_default();
        div()
            .id("document-subtitle")
            .flex_none()
            .px(px(24.))
            .pt(px(6.))
            .pb(px(10.))
            .text_center()
            .text_size(tokens.text(12.))
            .text_color(tokens.faint)
            .truncate()
            .child(line)
    }

    fn render_banner(&self, banner: &Banner, cx: &mut Context<Self>) -> AnyElement {
        let tokens = cx.tokens().clone();
        let (message, buttons): (String, Vec<AnyElement>) = match banner {
            Banner::ChangedOnDisk => (
                tr("File changed on disk"),
                vec![
                    pill("document-reload", tr("Reload"), false, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.reload_from_disk(cx)))
                        .into_any_element(),
                    pill("document-keep-mine", tr("Keep Mine"), true, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.keep_mine(cx)))
                        .into_any_element(),
                ],
            ),
            Banner::DeletedOnDisk => (
                tr("The file was deleted elsewhere"),
                vec![
                    pill("document-save-again", tr("Save Again"), true, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.keep_mine(cx)))
                        .into_any_element(),
                ],
            ),
            Banner::SaveFailed(reason) => (
                tr("Could not save: {}").replace("{}", reason),
                vec![
                    pill("document-try-again", tr("Try Again"), true, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.retry_save(cx)))
                        .into_any_element(),
                ],
            ),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .mx(px(12.))
            .mb(px(4.))
            .pl(px(14.))
            .pr(px(8.))
            .py(px(7.))
            .rounded(px(12.))
            .bg(tokens.accent.opacity(0.14))
            .text_size(tokens.text(13.5))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(message),
            )
            .children(buttons)
            .into_any_element()
    }

    fn render_error(&self, reason: &str, cx: &App) -> AnyElement {
        let tokens = cx.tokens();
        div()
            .flex()
            .flex_1()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .px(px(32.))
            .child(icon(IconName::Warning, tokens.faint).size(px(32.)))
            .child(
                div()
                    .text_size(tokens.text(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr("Could not open the file")),
            )
            .child(
                div()
                    .text_size(tokens.text(13.))
                    .text_color(tokens.muted)
                    .text_center()
                    .child(reason.to_owned()),
            )
            .into_any_element()
    }
}

/// Writes `body` as the job of a [`Serial`].
fn write(shared: &Shared, body: &str, force: bool) -> Outcome {
    let mut known = lock(shared);
    if known.finished {
        return Outcome::Same;
    }
    let Some(document) = &known.document else {
        return Outcome::Failed(tr("the file is not loaded"));
    };
    match document::save(document, body, force) {
        Ok(Saved::Written(now)) => {
            known.document = Some(now);
            Outcome::Saved {
                body: body.to_owned(),
            }
        }
        Ok(Saved::Changed(now)) => Outcome::Changed(now),
        Ok(Saved::Deleted) => Outcome::Deleted,
        Err(err) => {
            log::error!("cannot write {}: {err}", document.path.display());
            Outcome::Failed(err.to_string())
        }
    }
}

/// The editor for a plain Markdown file: headings are headings, the first line is no
/// title, and wiki links and tags, which only notes know, are plain text.
fn editor_options(lookup: &setup::SharedLookup) -> EditorOptions {
    let mut options = setup::options(lookup, Vec::new(), UiParts::ALL);
    options.first_line_is_title = false;
    options.extensions = vec![Box::new(Highlight)];
    options.completions = None;
    options.strings = EditorStrings::from_env();
    options
}

/// A button of 04 "Buttons": 28 high, 12 px/600, primary in the accent colour.
fn pill(
    id: &'static str,
    label: impl Into<SharedString>,
    primary: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let tokens = cx.tokens();
    let (background, color, hover) = if primary {
        (tokens.accent, tokens.on_accent, tokens.accent.opacity(0.85))
    } else {
        (tokens.selected, tokens.fg, tokens.hover)
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(8.))
        .bg(background)
        .text_color(color)
        .text_size(tokens.text(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(label.into())
}

impl Focusable for DocumentView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.loaded {
            self.editor.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl Render for DocumentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.loaded && self.focus_handle.is_focused(window) {
            self.editor.focus_handle(cx).focus(window, cx);
        }
        self.fit_column(window, cx);
        let tokens = cx.tokens().clone();
        let saved = self.saved_shown.then(|| {
            div()
                .absolute()
                .bottom(px(12.))
                .right(px(18.))
                .text_size(tokens.text(12.))
                .text_color(tokens.faint)
                .child(tr("Saved"))
                .with_animation(
                    ("document-saved", self.saves),
                    tokens.ease_out(FADE),
                    |label, delta| label.opacity(delta),
                )
        });
        let banner = self
            .banner
            .clone()
            .map(|banner| self.render_banner(&banner, cx));
        let error = self
            .error
            .clone()
            .map(|reason| self.render_error(&reason, cx));

        pane(tokens.view, window, cx)
            .relative()
            .track_focus(&self.focus_handle)
            .key_context(actions::DOCUMENT)
            .on_action(cx.listener(|this, _: &SaveDocument, _, cx| this.save(false, cx)))
            .on_action(cx.listener(|this, _: &CloseDocument, window, cx| this.close(window, cx)))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| this.close(window, cx)))
            .on_action(cx.listener(|this, _: &Print, _, cx| this.print(cx)))
            .on_action(cx.listener(|_, _: &PasteAttachment, window, cx| {
                // Files and images become attachments only in notes; here it is the
                // editor's own paste.
                match cx.build_action("markdown_editor::PasteClipboard", None) {
                    Ok(paste) => window.dispatch_action(paste, cx),
                    Err(err) => log::error!("the editor has no paste action: {err}"),
                }
            }))
            .child(self.render_header(cx))
            .child(self.render_subtitle(cx))
            .children(banner)
            .children(error)
            .child(
                div()
                    .id("document-text")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when(self.loaded, |area| area.child(self.editor.clone())),
            )
            .children(saved)
            .when(self.menu_open, |pane| {
                pane.child(
                    deferred(
                        div()
                            .id("document-menu-dismiss")
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.toggle_menu(cx)),
                            ),
                    )
                    .with_priority(1),
                )
            })
            .child(self.toasts.clone())
    }
}
