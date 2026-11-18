//! The Quick Note window (plan 03 §8): a small window of its own with only the editor
//! and a footer, `Folder ▾` · `Open` · `Done`.
//!
//! # Interface
//!
//! - [`open`] shows it, or brings the one that is open to the front; there is at most
//!   one. `sheaf --quick`, the `quick-note` action on D-Bus, Ctrl+Shift+N in the main
//!   window and the global shortcut ([`shortcut`](super::shortcut)) all end up there.
//! - What it asks of the main window (show the note there, follow a wiki link, search
//!   a tag) goes to the [`RequestHandler`] given to [`open`].
//! - Esc, Ctrl+W, the close button and "Done" store the note and close the window;
//!   Ctrl+Enter and "Open" also show the note in the main window.
//!
//! # Saving
//!
//! The note is a [`NoteView`] that is never drawn: its editor is, here. Typing is saved
//! exactly as in the note view: 600 ms after the last change, at once on Done and
//! Open, when the window is closed, and when the app quits or is told to by SIGTERM.
//! A new quick note that stays empty is deleted again.
//!
//! The note is created when the window opens, in the Quick Notes folder of the folder
//! settings (`quick_note.folder`, the localised "Quick Notes" by default), or at the
//! top of the notes folder if that folder cannot be used. If no note can be created,
//! the window says so and closes when asked; nothing was typed yet, as the editor
//! comes with the note. The last
//! quick note is continued instead if its window closed less than
//! `quick_note.resume_minutes` (5) ago.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, Edges, Entity, FocusHandle,
    Focusable, FontWeight, Global, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowDecorations, WindowHandle,
    WindowOptions, div, prelude::FluentBuilder as _, px, size as size_of,
};
use gpui_component::{Root, Theme};
use gpui_markdown_editor::EditorStyle;
use jiff::{SignedDuration, Timestamp};
use sheaf_core::i18n::tr;
use sheaf_core::{CreateOptions, NoteId};
use sheaf_searchd::api::APP_NAME;

use super::actions::{self, PasteAttachment, QuickNoteDone, QuickNoteOpen};
use super::assets::{IconName, icon};
use super::chrome::pane;
use super::instance::Request;
use super::library::{Folder, Library};
use super::note::{Banner, NoteEvent, NoteView, attach, setup};
use super::overlays::{self, MenuRow, Toast, ToastHost};
use super::settings::Settings;
use super::theme::ActiveTokens as _;

/// The window of 04 "Schnellnotiz", without the shadow around it.
pub const WIDTH: f32 = 380.;
pub const HEIGHT: f32 = 300.;
const HEADER_HEIGHT: f32 = 42.;
/// The shadow gpui-component draws around a client-decorated window, on every side.
const FRAME_SHADOW: f32 = 20.;

/// Does what the Quick Note window asks of the main window.
pub type RequestHandler = Rc<dyn Fn(Request, &mut App)>;

/// How the window ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Store the note and close.
    Done,
    /// Store the note, close, and show it in the main window.
    Open,
}

/// The open window and the last quick note, for the app as a whole.
#[derive(Default)]
struct QuickNotes {
    open: Option<(WindowHandle<Root>, Entity<QuickNoteView>)>,
    /// The last quick note and when its window closed.
    last: Option<(NoteId, Timestamp)>,
}

impl Global for QuickNotes {}

/// The Quick Note window that is open, if any.
pub fn current(cx: &App) -> Option<(WindowHandle<Root>, Entity<QuickNoteView>)> {
    cx.try_global::<QuickNotes>()?.open.clone()
}

/// Shows the Quick Note window, or brings the open one to the front. `on_request` is
/// kept only when a new window opens.
pub fn open(
    library: Entity<Library>,
    on_request: RequestHandler,
    cx: &mut App,
) -> anyhow::Result<(WindowHandle<Root>, Entity<QuickNoteView>)> {
    if let Some((handle, view)) = current(cx)
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
            title: Some(tr("Quick Note").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size_of(px(300.), px(200.))),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(APP_NAME.to_owned()),
        ..Default::default()
    };
    let mut built = None;
    let handle = cx.open_window(options, |window, cx| {
        let (root, view) = build(library, on_request, window, cx);
        built = Some(view);
        root
    })?;
    let view = built.ok_or_else(|| anyhow::anyhow!("the window was not built"))?;
    cx.default_global::<QuickNotes>().open = Some((handle, view.clone()));
    Ok((handle, view))
}

/// The content of the Quick Note window inside gpui-component's `Root`. [`open`] uses
/// it; the screenshot harness calls it for its offscreen window.
pub fn build(
    library: Entity<Library>,
    on_request: RequestHandler,
    window: &mut Window,
    cx: &mut App,
) -> (Entity<Root>, Entity<QuickNoteView>) {
    let view = cx.new(|cx| QuickNoteView::new(library, on_request, window, cx));
    window.on_window_should_close(cx, {
        let view = view.downgrade();
        move |_, cx| {
            view.update(cx, |view, cx| view.should_close(cx))
                .unwrap_or(true)
        }
    });
    let root = cx.new(|cx| Root::new(view.clone(), window, cx));
    (root, view)
}

pub struct QuickNoteView {
    library: Entity<Library>,
    on_request: RequestHandler,
    /// The note, once it is created or found.
    note: Option<Entity<NoteView>>,
    /// The folder new quick notes go to, shown until the note is there.
    folder: String,
    /// The folders of the "Folder ▾" menu while it is open.
    folders: Option<Vec<Folder>>,
    /// An ending asked for before the note was there, done when it is.
    pending: Option<Ending>,
    /// No note could be created: the window closes without one.
    failed: bool,
    leaving: bool,
    focus_handle: FocusHandle,
    toasts: Entity<ToastHost>,
    _opening: Task<()>,
    _leaving: Task<()>,
    _folders: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for QuickNoteView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickNoteView")
            .field("folder", &self.folder)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl QuickNoteView {
    fn new(
        library: Entity<Library>,
        on_request: RequestHandler,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let opening = Self::find_or_create(&library, window, cx);
        QuickNoteView {
            library,
            on_request,
            note: None,
            folder: String::new(),
            folders: None,
            pending: None,
            failed: false,
            leaving: false,
            focus_handle,
            toasts: cx.new(|_| ToastHost::default()),
            _opening: opening,
            _leaving: Task::ready(()),
            _folders: Task::ready(()),
            _subscriptions: Vec::new(),
        }
    }

    /// Continues the last quick note if it is recent enough, otherwise creates one.
    fn find_or_create(
        library: &Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let config = library.read(cx).folder_config(cx);
        let now = library.read(cx).now().timestamp();
        let last = cx
            .try_global::<QuickNotes>()
            .and_then(|quick| quick.last.clone());
        let library = library.clone();
        cx.spawn_in(window, async move |this, cx| {
            let settings = match config.await {
                Ok(config) => config.quick_note,
                Err(err) => {
                    log::warn!("cannot read the folder settings: {err}");
                    sheaf_core::config::QuickNote::default()
                }
            };
            let folder = settings.folder_name();
            let _ = this.update(cx, |this, cx| {
                this.folder.clone_from(&folder);
                cx.notify();
            });
            let resume = SignedDuration::from_mins(i64::from(settings.resume_minutes));
            let recent = last.filter(|(_, closed)| now.duration_since(*closed) < resume);
            if let Some((id, _)) = recent {
                let found = library
                    .update(cx, |library, cx| library.meta(&id, cx))
                    .await;
                if let Ok(Some(meta)) = found {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.attach(meta.id, false, window, cx);
                    });
                    return;
                }
            }
            let mut create = |folder: String| {
                let options = CreateOptions {
                    folder,
                    ..CreateOptions::default()
                };
                library.update(cx, |library, cx| library.create("", options, cx))
            };
            let mut created = create(folder.clone()).await;
            let mut moved = None;
            if let Err(err) = &created
                && !folder.is_empty()
            {
                // The Quick Notes folder cannot be used: the top of the notes folder can.
                log::warn!("cannot create a quick note in {folder:?}: {err}");
                moved = Some(err.to_string());
                created = create(String::new()).await;
            }
            let _ = this.update_in(cx, |this, window, cx| match created {
                Ok(meta) => {
                    if let Some(reason) = moved {
                        let message = tr(
                            "Could not use the folder “{folder}”, the note is at the top: {reason}",
                        )
                        .replace("{folder}", &folder)
                        .replace("{reason}", &reason);
                        this.toast(Toast::new(message), cx);
                    }
                    this.attach(meta.id, true, window, cx);
                }
                Err(err) => {
                    log::error!("cannot create a quick note: {err}");
                    let message = tr("Could not create a note: {}").replace("{}", &err.to_string());
                    this.toast(Toast::new(message), cx);
                    this.failed = true;
                    if let Some(ending) = this.pending.take() {
                        this.ended(ending, window, cx);
                    }
                }
            });
        })
    }

    /// Takes the note `id` into the window; `fresh` if it was just created for it.
    fn attach(&mut self, id: NoteId, fresh: bool, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let note = cx.new(|cx| NoteView::new(library, id, String::new(), window, cx));
        if fresh {
            note.update(cx, |note, _| note.mark_new());
        }
        let editor = note.read(cx).editor().clone();
        editor.update(cx, |editor, cx| editor.set_style(style(cx), cx));
        self._subscriptions = vec![
            cx.observe(&note, |_, _, cx| cx.notify()),
            cx.subscribe(&note, |this, _, event: &NoteEvent, cx| match event {
                NoteEvent::Toast(toast) => this.toast(toast.clone(), cx),
                NoteEvent::Open(id) => (this.on_request)(
                    Request::OpenNote {
                        id: id.to_string(),
                        query: String::new(),
                    },
                    cx,
                ),
                NoteEvent::Search(query) => (this.on_request)(
                    Request::Search {
                        query: query.clone(),
                    },
                    cx,
                ),
                _ => {}
            }),
            // After the note view's own observers, which set the style of the note.
            cx.observe_global::<Theme>({
                let editor = editor.clone();
                move |_, cx| editor.update(cx, |editor, cx| editor.set_style(style(cx), cx))
            }),
            cx.observe_global::<Settings>(move |_, cx| {
                editor.update(cx, |editor, cx| editor.set_style(style(cx), cx));
            }),
        ];
        self.note = Some(note);
        if let Some(ending) = self.pending.take() {
            self.end(ending, window, cx);
        }
        cx.notify();
    }

    /// The note, once it is there.
    pub fn note(&self) -> Option<&Entity<NoteView>> {
        self.note.as_ref()
    }

    /// The folder the note is in, relative to the notes folder.
    pub fn folder(&self, cx: &App) -> String {
        self.note
            .as_ref()
            .and_then(|note| note.read(cx).meta().map(|meta| meta.folder.clone()))
            .unwrap_or_else(|| self.folder.clone())
    }

    /// Whether the folder menu is open.
    pub fn folder_menu_open(&self) -> bool {
        self.folders.is_some()
    }

    /// Stores the note and closes the window; with [`Ending::Open`] the main window
    /// shows the note afterwards. If the note cannot be stored, the window stays and
    /// says so.
    pub fn end(&mut self, ending: Ending, window: &mut Window, cx: &mut Context<Self>) {
        if self.leaving {
            return;
        }
        let Some(note) = self.note.clone() else {
            if self.failed {
                self.ended(ending, window, cx);
            } else {
                // Still being created: done as soon as it is there.
                self.pending = Some(ending);
            }
            return;
        };
        let Some(leaving) = note.update(cx, |note, cx| note.leave(cx)) else {
            self.ended(ending, window, cx);
            return;
        };
        self.leaving = true;
        self._leaving = cx.spawn_in(window, async move |this, cx| {
            let left = leaving.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.leaving = false;
                if left {
                    this.ended(ending, window, cx);
                } else {
                    this.toast(Toast::new(tr("Could not save the note")), cx);
                }
            });
        });
    }

    /// The note is stored: remembers it, closes the window and, for
    /// [`Ending::Open`], hands it to the main window.
    fn ended(&mut self, ending: Ending, window: &mut Window, cx: &mut Context<Self>) {
        let kept = self.remember(cx);
        if ending == Ending::Open {
            let request = match kept {
                Some(id) => Request::OpenNote {
                    id: id.to_string(),
                    query: String::new(),
                },
                None => Request::Activate,
            };
            (self.on_request)(request, cx);
        }
        window.remove_window();
    }

    /// The window is closed from outside, such as by the window manager: the last save
    /// happens now, as the app may quit with it. Returns whether the window may close;
    /// before the note is there it closes once it is, or once creating it failed.
    pub fn should_close(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(note) = self.note.clone() else {
            if self.failed {
                self.remember(cx);
                return true;
            }
            self.pending = Some(Ending::Done);
            return false;
        };
        note.update(cx, |note, cx| note.save_now(cx));
        self.remember(cx);
        true
    }

    /// Forgets the window and remembers the note to continue, unless it was empty and
    /// went away. Returns the note that stays.
    fn remember(&mut self, cx: &mut Context<Self>) -> Option<NoteId> {
        let now = self.library.read(cx).now().timestamp();
        let kept = self.note.as_ref().and_then(|note| {
            let note = note.read(cx);
            let text = note.editor().read(cx).markdown();
            (!text.trim().is_empty()).then(|| note.id().clone())
        });
        let quick = cx.default_global::<QuickNotes>();
        quick.open = None;
        quick.last = kept.clone().map(|id| (id, now));
        kept
    }

    /// Opens the folder menu, or closes it.
    pub fn toggle_folder_menu(&mut self, cx: &mut Context<Self>) {
        if self.folders.take().is_some() {
            cx.notify();
            return;
        }
        let folders = self.library.read(cx).folders(cx);
        self._folders = cx.spawn(async move |this, cx| {
            let folders = folders.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.folders = Some(folders);
                cx.notify();
            });
        });
    }

    /// Moves the note to `folder`, as the folder menu does.
    pub fn move_to(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.folders = None;
        if let Some(note) = &self.note {
            note.update(cx, |note, cx| note.move_to(folder, cx));
        }
        cx.notify();
    }

    fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toasts.update(cx, |toasts, cx| toasts.show(toast, cx));
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens();
        let pressed = tokens.selected.opacity(1.6);
        div()
            .id("quick-header")
            .flex()
            .flex_none()
            .items_center()
            .h(px(HEADER_HEIGHT))
            .px(px(8.))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(
                div()
                    .flex_1()
                    .pl(px(10.))
                    .text_size(tokens.text(13.))
                    .font_weight(FontWeight::BOLD)
                    .child(tr("Quick Note")),
            )
            .child(
                div()
                    .id("quick-close")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(24.))
                    .mx(px(4.))
                    .rounded_full()
                    .bg(tokens.selected)
                    .cursor_pointer()
                    .hover(move |style| style.bg(pressed))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| this.end(Ending::Done, window, cx)))
                    .child(icon(IconName::Close, tokens.fg).size(px(12.))),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let folder = self.folder(cx);
        let name = match folder.rsplit('/').next() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => tr("Notes"),
        };
        let status = match self.note.as_ref().and_then(|note| note.read(cx).banner()) {
            Some(Banner::SaveFailed(reason)) => tr("Not saved: {}").replace("{}", reason),
            Some(Banner::ChangedOnDisk) => tr("Changed elsewhere"),
            Some(Banner::DeletedOnDisk) => tr("Deleted elsewhere"),
            None => tr("Saved"),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .border_t_1()
            .border_color(tokens.hair)
            .text_size(tokens.text(12.))
            .text_color(tokens.faint)
            .child(
                div()
                    .id("quick-folder")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .h(px(26.))
                    .px(px(6.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|style| style.bg(tokens.hover))
                    .when(self.folders.is_some(), |button| button.bg(tokens.selected))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_folder_menu(cx)))
                    .child(icon(IconName::Folder, tokens.faint).size(px(12.)))
                    .child(div().max_w(px(110.)).truncate().child(name))
                    .child(icon(IconName::PanDown, tokens.faint).size(px(10.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(format!("· {status}")),
            )
            .child(
                button("quick-open", tr("Open"), false, &tokens).on_click(
                    cx.listener(|this, _, window, cx| this.end(Ending::Open, window, cx)),
                ),
            )
            .child(
                button("quick-done", tr("Done"), true, &tokens).on_click(
                    cx.listener(|this, _, window, cx| this.end(Ending::Done, window, cx)),
                ),
            )
    }

    /// The folders to move the note to, above the footer.
    fn render_folder_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let folders = self.folders.as_ref()?;
        let current = self.folder(cx);
        let rows = folders.iter().enumerate().map(|(index, folder)| {
            let label = if folder.path.is_empty() {
                tr("Top Level")
            } else {
                folder.path.clone()
            };
            let path = folder.path.clone();
            let mut row = MenuRow::new(("quick-folder", index), label).icon(IconName::Folder);
            if folder.path == current {
                row = row.trailing("✓");
            }
            row.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.move_to(&path, cx)))
        });
        let menu = overlays::popover("quick-folders", cx)
            .w(px(240.))
            .max_h(px(200.))
            .overflow_y_scroll()
            .children(rows);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(
                    div()
                        .id("quick-folders-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.toggle_folder_menu(cx)),
                        ),
                )
                .child(div().absolute().left(px(8.)).bottom(px(46.)).child(menu))
                .into_any_element(),
        )
    }
}

/// `.btn` of the prototype; `primary` is `.btn.pri`.
fn button(
    id: &'static str,
    label: String,
    primary: bool,
    tokens: &super::theme::Tokens,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(30.))
        .px(px(13.))
        .rounded(px(8.))
        .text_size(tokens.text(12.5))
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .when(primary, |button| {
            button.bg(tokens.accent).text_color(tokens.on_accent)
        })
        .when(!primary, |button| {
            button.bg(tokens.selected).text_color(tokens.fg)
        })
        .child(SharedString::from(label))
}

/// The note view's editor style at the size of 04 "Schnellnotiz": text 15/1.55 and a
/// title no larger than the text, in the window's padding. The font follows the
/// settings; their line widths are all wider than this window.
fn style(cx: &App) -> EditorStyle {
    let tokens = cx.tokens();
    let mut style = setup::style(cx);
    style.padding = Edges {
        top: px(4.),
        right: px(18.),
        bottom: px(14.),
        left: px(18.),
    };
    style.text_size = tokens.text(15.);
    style.line_height = 1.55;
    style.title_size = tokens.text(15.);
    style.title_weight = FontWeight::BOLD;
    style
}

impl Focusable for QuickNoteView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for QuickNoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self
            .note
            .as_ref()
            .filter(|note| note.read(cx).is_loaded())
            .map(|note| note.read(cx).editor().clone());
        if let Some(editor) = &editor
            && self.focus_handle.is_focused(window)
        {
            editor.focus_handle(cx).focus(window, cx);
        }
        let tokens = cx.tokens().clone();
        let folder_menu = self.render_folder_menu(cx);
        pane(tokens.view, window, cx)
            .relative()
            .track_focus(&self.focus_handle)
            .key_context(actions::QUICK_NOTE)
            .on_action(cx.listener(|this, _: &QuickNoteDone, window, cx| {
                if this.folders.is_some() {
                    this.toggle_folder_menu(cx);
                } else {
                    this.end(Ending::Done, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &QuickNoteOpen, window, cx| {
                this.end(Ending::Open, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteAttachment, window, cx| {
                if let Some(note) = &this.note {
                    note.update(cx, |note, cx| note.paste(window, cx));
                }
            }))
            .child(self.render_header(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when_some(editor, |body, editor| body.child(editor))
                    .when_some(
                        self.note.clone().filter(|_| cx.has_active_drag()),
                        |body, note| body.child(attach::drop_layer(note)),
                    ),
            )
            .child(self.render_footer(cx))
            .children(folder_menu)
            .child(self.toasts.clone())
    }
}
