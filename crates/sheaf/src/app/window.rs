//! The main window: a router over the gallery and one open note.
//!
//! The [`GalleryView`] lives as long as the window and is always drawn. A
//! [`NoteView`] is created when a note is opened, slides in over the gallery from the
//! right and is dropped after it has slid back out.
//!
//! The window frame (rounded corners, shadow, resize edges) comes from
//! gpui-component's `Root`, which [`open`] wraps the view in; the header bars are the
//! views' own, built from [`chrome`](super::chrome).
//!
//! Over the pages lie the [`overlays`](super::overlays): the toast
//! ([`MainWindow::toast`]), the command palette and the list of shortcuts.

use gpui::{
    AnimationExt as _, App, AppContext as _, Bounds, Context, DismissEvent, Entity, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowDecorations, WindowHandle,
    WindowOptions, div, prelude::FluentBuilder as _, px, relative, size as size_of,
};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt as _};
use sheaf_core::CreateOptions;
use sheaf_core::NoteId;
use sheaf_core::i18n::{ntr, tr};
use sheaf_searchd::api::APP_NAME;

use super::actions::{
    self, Back, CloseWindow, NewNote, OpenCommandPalette, OpenSettings, QuickNote, Search,
    ShowShortcuts, TogglePin,
};
use super::gallery::{GalleryEvent, GalleryView};
use super::instance::Request;
use super::library::Library;
use super::note::{NoteEvent, NoteView};
use super::overlays::palette::{CommandPalette, PaletteEvent};
use super::overlays::shortcuts::ShortcutsSheet;
use super::overlays::{Toast, ToastHost};
use super::state::{Recovery, StateFile, WindowState};
use super::theme::{ActiveTokens as _, motion, size};

/// The shadow gpui-component draws around a client-decorated window, on every side.
const FRAME_SHADOW: f32 = 20.0;

/// What the window shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Gallery,
    /// A note; `query` is the search it was opened from, or empty.
    Note {
        id: NoteId,
        query: String,
    },
}

pub struct MainWindow {
    library: Entity<Library>,
    gallery: Entity<GalleryView>,
    /// The open note and the search it was opened from.
    note: Option<(Entity<NoteView>, String)>,
    /// The note that is sliding out.
    leaving: Option<Entity<NoteView>>,
    /// Whether the open note is still sliding in.
    entering: bool,
    /// Counts the slides, so that each one animates afresh.
    slide: usize,
    state_file: Option<StateFile>,
    toasts: Entity<ToastHost>,
    palette: Option<Entity<CommandPalette>>,
    shortcuts: Option<Entity<ShortcutsSheet>>,
    /// Where the keyboard was before the palette or the shortcuts opened.
    restore_focus: Option<FocusHandle>,
    _sliding: Task<()>,
    _opening: Task<()>,
    _note_events: Option<Subscription>,
    _overlay_events: Option<Subscription>,
    _subscriptions: [Subscription; 1],
}

impl std::fmt::Debug for MainWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainWindow")
            .field("note", &self.note.as_ref().map(|(_, query)| query))
            .field("leaving", &self.leaving.is_some())
            .finish_non_exhaustive()
    }
}

impl MainWindow {
    pub fn new(
        library: Entity<Library>,
        state_file: Option<StateFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let gallery = cx.new(|cx| GalleryView::new(library.clone(), window, cx));
        let gallery_event =
            cx.subscribe_in(&gallery, window, |this, _, event, window, cx| match event {
                GalleryEvent::Open { id, query } => {
                    this.open_note(id.clone(), query.clone(), window, cx);
                }
                GalleryEvent::Trashed(ids) => this.offer_undo(ids.clone(), cx),
            });
        gallery.focus_handle(cx).focus(window, cx);
        let mut main = MainWindow {
            library,
            gallery,
            note: None,
            leaving: None,
            entering: false,
            slide: 0,
            state_file,
            toasts: cx.new(|_| ToastHost::default()),
            palette: None,
            shortcuts: None,
            restore_focus: None,
            _sliding: Task::ready(()),
            _opening: Task::ready(()),
            _note_events: None,
            _overlay_events: None,
            _subscriptions: [gallery_event],
        };
        if let Some(recovery) = cx.try_global::<Recovery>().cloned()
            && recovery.take_unseen()
        {
            let folder = recovery.dir().to_owned();
            let toast = Toast::new(tr("Recovered unsaved notes"))
                .action(tr("Open Folder"), move |_, cx| cx.open_with_system(&folder));
            main.toast(toast, cx);
        }
        main
    }

    pub fn route(&self, cx: &App) -> Route {
        match &self.note {
            Some((note, query)) => Route::Note {
                id: note.read(cx).id().clone(),
                query: query.clone(),
            },
            None => Route::Gallery,
        }
    }

    pub fn gallery(&self) -> &Entity<GalleryView> {
        &self.gallery
    }

    /// The open note, if the route is a note.
    pub fn note(&self) -> Option<&Entity<NoteView>> {
        self.note.as_ref().map(|(note, _)| note)
    }

    /// Opens a note over the gallery; `query` is what to jump to in it. A note that is
    /// already open is left first, see [`back`](Self::back).
    pub fn open_note(
        &mut self,
        id: NoteId,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(id, query, false, window, cx);
    }

    /// [`open_note`](Self::open_note); `fresh` for a note just created for this visit.
    fn open(
        &mut self,
        id: NoteId,
        query: String,
        fresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_note(window, cx, move |this, _, window, cx| {
            let library = this.library.clone();
            let note = cx.new(|cx| NoteView::new(library, id, query.clone(), window, cx));
            if fresh {
                note.update(cx, |note, _| note.mark_new());
            }
            note.focus_handle(cx).focus(window, cx);
            this._note_events = Some(cx.subscribe_in(&note, window, Self::note_event));
            this.note = Some((note, query));
            this.entering = true;
            this.slide(cx);
        });
    }

    /// Returns to the gallery once what was typed is saved. If it cannot be saved, the
    /// note stays open with a banner that says why.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_note(window, cx, |this, note, window, cx| {
            if let Some(note) = note {
                this.slide_out(note, window, cx);
            }
        });
    }

    /// Leaves the open note and runs `then` with it once what was typed is on disk;
    /// with no note open, at once. If it cannot be stored, the note stays open and
    /// `then` does not run.
    fn leave_note(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<Entity<NoteView>>, &mut Window, &mut Context<Self>)
        + 'static,
    ) {
        let Some((note, _)) = &self.note else {
            then(self, None, window, cx);
            return;
        };
        let note = note.clone();
        let Some(leaving) = note.update(cx, |note, cx| note.leave(cx)) else {
            self.note = None;
            then(self, Some(note), window, cx);
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if !leaving.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if this.note().is_some_and(|open| *open == note) {
                    this.note = None;
                    then(this, Some(note), window, cx);
                }
            });
        })
        .detach();
    }

    /// Lets `note` slide out over the gallery, which takes the keyboard.
    fn slide_out(&mut self, note: Entity<NoteView>, window: &mut Window, cx: &mut Context<Self>) {
        self.gallery.focus_handle(cx).focus(window, cx);
        self.entering = false;
        self.leaving = Some(note);
        self.slide(cx);
    }

    /// Starts the slide of what was just set to enter or leave, and ends it when its
    /// time is up: the page that left is dropped, the one that entered stays put.
    fn slide(&mut self, cx: &mut Context<Self>) {
        self.slide += 1;
        let duration = cx.tokens().duration(motion::SLIDE);
        if duration.is_zero() {
            self.settle();
        } else {
            self._sliding = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(duration).await;
                let _ = this.update(cx, |this, cx| {
                    this.settle();
                    cx.notify();
                });
            });
        }
        cx.notify();
    }

    fn settle(&mut self) {
        self.entering = false;
        self.leaving = None;
    }

    /// Creates an empty note in the folder the gallery shows and opens it, once the
    /// open note is left.
    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_note(window, cx, |this, _, window, cx| {
            let options = CreateOptions {
                folder: this.gallery.read(cx).current_folder(),
                ..CreateOptions::default()
            };
            let created = this
                .library
                .update(cx, |library, cx| library.create("", options, cx));
            this._opening = cx.spawn_in(window, async move |this, cx| match created.await {
                Ok(meta) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.open(meta.id, String::new(), true, window, cx);
                    });
                }
                Err(err) => log::error!("cannot create a note: {err}"),
            });
        });
    }

    /// Does what another launch, the desktop or `sheaf-searchd` asked for.
    pub fn handle(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::Activate => {}
            Request::NewNote => self.new_note(window, cx),
            Request::QuickNote => self.quick_note(window, cx),
            Request::OpenNote { id, query } => match id.parse::<NoteId>() {
                Ok(id) => self.open_note(id, query, window, cx),
                Err(err) => log::warn!("cannot open a note: {err}"),
            },
            Request::Search { query } => self.search_in_gallery(Some(&query), window, cx),
            Request::OpenUris(uris) => {
                let Some(uri) = uris.first() else { return };
                let found = self.library.read(cx).note_at(uri, cx);
                self._opening = cx.spawn_in(window, async move |this, cx| {
                    let Some(meta) = found.await else { return };
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.open_note(meta.id, String::new(), window, cx);
                    });
                });
            }
        }
    }

    /// The Quick Note window (plan 03 §8) is not built yet.
    fn quick_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.push_notification(
            Notification::info(tr("Quick Note is not available yet")),
            cx,
        );
    }

    fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        match &self.note {
            Some((note, _)) => note.update(cx, |note, cx| note.toggle_pin(cx)),
            None => self
                .gallery
                .update(cx, |gallery, cx| gallery.toggle_pin(cx)),
        }
    }

    /// Shows `toast` at the bottom of the window, in place of the one shown.
    pub fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toasts.update(cx, |toasts, cx| toasts.show(toast, cx));
    }

    /// Tells that `ids` went to the trash, with an Undo that puts them back.
    fn offer_undo(&mut self, ids: Vec<NoteId>, cx: &mut Context<Self>) {
        let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
        let message = ntr("Note moved to trash", "{} notes moved to trash", count)
            .replace("{}", &count.to_string());
        let library = self.library.clone();
        let toast = Toast::new(message).action(tr("Undo"), move |_, cx| {
            for id in &ids {
                let restored = library.update(cx, |library, cx| library.restore(id, cx));
                cx.background_spawn(async move {
                    if let Err(err) = restored.await {
                        log::error!("cannot restore the note: {err}");
                    }
                })
                .detach();
            }
        });
        self.toast(toast, cx);
    }

    pub fn toasts(&self) -> &Entity<ToastHost> {
        &self.toasts
    }

    pub fn palette(&self) -> Option<&Entity<CommandPalette>> {
        self.palette.as_ref()
    }

    pub fn shortcuts(&self) -> Option<&Entity<ShortcutsSheet>> {
        self.shortcuts.as_ref()
    }

    fn note_event(
        &mut self,
        _: &Entity<NoteView>,
        event: &NoteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            NoteEvent::Open(id) => self.open_note(id.clone(), String::new(), window, cx),
            NoteEvent::Search(query) => self.search_in_gallery(Some(query), window, cx),
            NoteEvent::Trashed(trashed) => {
                self.back(window, cx);
                self.offer_undo(vec![trashed.id.clone()], cx);
            }
            NoteEvent::Toast(toast) => self.toast(toast.clone(), cx),
        }
    }

    /// Opens the command palette (Ctrl+K), or closes it.
    fn open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_overlay(window, cx);
            return;
        }
        self.close_overlay(window, cx);
        self.restore_focus = window.focused(cx);
        let library = self.library.clone();
        let palette = cx.new(|cx| CommandPalette::new(library, window, cx));
        self._overlay_events = Some(cx.subscribe_in(
            &palette,
            window,
            |this, _, event: &PaletteEvent, window, cx| match event {
                PaletteEvent::Run(action) => {
                    this.close_overlay(window, cx);
                    window.dispatch_action(action.boxed_clone(), cx);
                }
                PaletteEvent::Open { note, query } => {
                    this.close_overlay(window, cx);
                    this.open_note(note.id.clone(), query.clone(), window, cx);
                }
                PaletteEvent::Dismiss => this.close_overlay(window, cx),
            },
        ));
        self.palette = Some(palette);
        cx.notify();
    }

    /// Closes the palette or the list of shortcuts and gives the keyboard back.
    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_none() && self.shortcuts.take().is_none() {
            return;
        }
        self._overlay_events = None;
        match self.restore_focus.take() {
            Some(focus) => focus.focus(window, cx),
            None => match &self.note {
                Some((note, _)) => note.focus_handle(cx).focus(window, cx),
                None => self.gallery.focus_handle(cx).focus(window, cx),
            },
        }
        cx.notify();
    }

    /// Search from a note (the palette's "Search"): back to the gallery's search field.
    fn search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.is_some() {
            self.search_in_gallery(None, window, cx);
        }
    }

    /// Shows the gallery with the cursor in its search field; with `query`, that
    /// search replaces the one shown.
    fn search_in_gallery(
        &mut self,
        query: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = query.map(str::to_owned);
        self.leave_note(window, cx, move |this, note, window, cx| {
            if let Some(note) = note {
                this.slide_out(note, window, cx);
            }
            this.gallery.update(cx, |gallery, cx| {
                if let Some(query) = &query {
                    gallery.set_query(query, cx);
                }
                gallery.focus_search(window, cx);
            });
        });
    }

    /// Hook for the settings window (Ctrl+,), which does not exist yet.
    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, _: &mut Context<Self>) {
        log::debug!("the settings are not built yet");
    }

    /// Shows the list of keyboard shortcuts (Ctrl+?), or closes it.
    fn show_shortcuts(&mut self, _: &ShowShortcuts, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts.is_some() {
            self.close_overlay(window, cx);
            return;
        }
        self.close_overlay(window, cx);
        self.restore_focus = window.focused(cx);
        let sheet = cx.new(|cx| ShortcutsSheet::new(window, cx));
        self._overlay_events = Some(cx.subscribe_in(
            &sheet,
            window,
            |this, _, _: &DismissEvent, window, cx| this.close_overlay(window, cx),
        ));
        self.shortcuts = Some(sheet);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember(window);
        self.save_now(cx);
        window.remove_window();
    }

    /// The last save of the notes in the window, at once: it is closing, and with the
    /// last window the app quits without running another task.
    fn save_now(&mut self, cx: &mut Context<Self>) {
        let open = self.note.as_ref().map(|(note, _)| note);
        let notes: Vec<Entity<NoteView>> = open.into_iter().chain(&self.leaving).cloned().collect();
        for note in notes {
            note.update(cx, |note, cx| note.save_now(cx));
        }
    }

    /// Writes size and state of the window to the state file.
    fn remember(&self, window: &Window) {
        let Some(file) = &self.state_file else { return };
        let (bounds, maximized) = match window.window_bounds() {
            WindowBounds::Windowed(bounds) => (bounds, false),
            // The bounds to return to when the window is restored.
            WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
        };
        let state = WindowState {
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            maximized,
        };
        if let Err(err) = file.save(state) {
            log::warn!("cannot write {}: {err}", file.path().display());
        }
    }
}

impl Render for MainWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let slide = self.slide;
        // A page that covers the gallery and takes its pointer events.
        let page = |note: &Entity<NoteView>| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .child(note.clone())
        };
        // The spring overshoots; a page that slid past the edge would bare the
        // gallery behind it, so it stops there.
        let open = self.note.as_ref().map(|(note, _)| {
            if self.entering {
                page(note)
                    .with_animation(
                        ("slide-in", slide),
                        tokens.spring(motion::SLIDE),
                        |page, progress| page.left(relative((1.0 - progress).max(0.0))),
                    )
                    .into_any_element()
            } else {
                page(note).into_any_element()
            }
        });
        let leaving = self.leaving.as_ref().map(|note| {
            page(note).with_animation(
                ("slide-out", slide),
                tokens.spring(motion::SLIDE),
                |page, progress| page.left(relative(progress.min(1.0))),
            )
        });

        div()
            .id("main-window")
            .key_context(actions::WINDOW)
            .relative()
            .size_full()
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &NewNote, window, cx| this.new_note(window, cx)))
            .on_action(cx.listener(|this, _: &Back, window, cx| this.back(window, cx)))
            .on_action(cx.listener(|this, _: &QuickNote, window, cx| this.quick_note(window, cx)))
            .on_action(cx.listener(|this, _: &TogglePin, _, cx| this.toggle_pin(cx)))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| this.close(window, cx)))
            .on_action(cx.listener(Self::open_command_palette))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::show_shortcuts))
            .on_action(cx.listener(Self::search))
            .child(self.gallery.clone())
            .when_some(leaving, |root, page| root.child(page))
            .when_some(open, |root, page| root.child(page))
            .when_some(self.palette.clone(), |root, palette| root.child(palette))
            .when_some(self.shortcuts.clone(), |root, sheet| root.child(sheet))
            .child(self.toasts.clone())
    }
}

/// Opens the main window at the size it had last time and returns it with its view.
pub fn open(
    library: Entity<Library>,
    state_file: Option<StateFile>,
    cx: &mut App,
) -> anyhow::Result<(WindowHandle<Root>, Entity<MainWindow>)> {
    // On the first run the frame gets its default size; it sits inside a shadow on
    // every side, which is part of the window.
    let state = state_file
        .as_ref()
        .and_then(StateFile::load)
        .unwrap_or(WindowState {
            width: f32::from(size::WINDOW_WIDTH) + 2.0 * FRAME_SHADOW,
            height: f32::from(size::WINDOW_HEIGHT) + 2.0 * FRAME_SHADOW,
            maximized: false,
        });
    let bounds = Bounds::centered(None, size_of(px(state.width), px(state.height)), cx);
    let options = WindowOptions {
        window_bounds: Some(if state.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }),
        titlebar: Some(TitlebarOptions {
            title: Some(tr("Notes").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size_of(size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT)),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(APP_NAME.to_owned()),
        ..Default::default()
    };

    let mut main = None;
    let handle = cx.open_window(options, |window, cx| {
        let (root, view) = build(library, state_file, window, cx);
        main = Some(view);
        root
    })?;
    let main = main.ok_or_else(|| anyhow::anyhow!("the window was not built"))?;
    Ok((handle, main))
}

/// The content of the main window for `window`: the view inside gpui-component's
/// `Root`, which draws the frame and hosts tooltips and notifications. [`open`] uses
/// it; the screenshot harness calls it for its offscreen window.
pub fn build(
    library: Entity<Library>,
    state_file: Option<StateFile>,
    window: &mut Window,
    cx: &mut App,
) -> (Entity<Root>, Entity<MainWindow>) {
    let view = cx.new(|cx| MainWindow::new(library, state_file, window, cx));
    window.on_window_should_close(cx, {
        let view = view.downgrade();
        move |window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.remember(window);
                view.save_now(cx);
            });
            true
        }
    });
    let root = cx.new(|cx| Root::new(view.clone(), window, cx));
    (root, view)
}
