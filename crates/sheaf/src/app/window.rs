//! The main window: a router over the gallery and one open note.
//!
//! The [`GalleryView`] lives as long as the window and is always drawn. A
//! [`NoteView`] is created when a note is opened, slides in over the gallery from the
//! right and is dropped after it has slid back out.
//!
//! The window frame (rounded corners, shadow, resize edges) comes from
//! gpui-component's `Root`, which [`open`] wraps the view in; the header bars are the
//! views' own, built from [`chrome`](super::chrome).

use gpui::{
    AnimationExt as _, App, AppContext as _, Bounds, Context, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, TitlebarOptions, Window, WindowBounds, WindowDecorations, WindowHandle, WindowOptions,
    div, prelude::FluentBuilder as _, px, relative, size as size_of,
};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt as _};
use sheaf_core::CreateOptions;
use sheaf_core::NoteId;
use sheaf_core::i18n::tr;
use sheaf_searchd::api::APP_NAME;

use super::actions::{
    self, Back, CloseWindow, NewNote, OpenCommandPalette, OpenSettings, QuickNote, ShowShortcuts,
    TogglePin,
};
use super::gallery::{GalleryEvent, GalleryView};
use super::instance::Request;
use super::library::Library;
use super::note::NoteView;
use super::state::{StateFile, WindowState};
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
    _sliding: Task<()>,
    _opening: Task<()>,
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
            });
        gallery.focus_handle(cx).focus(window, cx);
        MainWindow {
            library,
            gallery,
            note: None,
            leaving: None,
            entering: false,
            slide: 0,
            state_file,
            _sliding: Task::ready(()),
            _opening: Task::ready(()),
            _subscriptions: [gallery_event],
        }
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
    /// already open is saved and replaced.
    pub fn open_note(
        &mut self,
        id: NoteId,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((open, _)) = self.note.take() {
            open.update(cx, |note, cx| note.flush(cx)).detach();
        }
        let library = self.library.clone();
        let note = cx.new(|cx| NoteView::new(library, id, query.clone(), window, cx));
        note.focus_handle(cx).focus(window, cx);
        self.note = Some((note, query));
        self.entering = true;
        self.slide(cx);
    }

    /// Returns to the gallery; what was typed is saved first.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((note, _)) = self.note.take() else {
            return;
        };
        note.update(cx, |note, cx| note.flush(cx)).detach();
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

    /// Creates an empty note in the folder the gallery shows and opens it.
    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options = CreateOptions {
            folder: self.gallery.read(cx).current_folder(),
            ..CreateOptions::default()
        };
        let created = self
            .library
            .update(cx, |library, cx| library.create("", options, cx));
        self._opening = cx.spawn_in(window, async move |this, cx| match created.await {
            Ok(meta) => {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.open_note(meta.id, String::new(), window, cx);
                });
            }
            Err(err) => log::error!("cannot create a note: {err}"),
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
            Request::Search { query } => {
                self.back(window, cx);
                self.gallery
                    .update(cx, |gallery, cx| gallery.set_query(&query, cx));
            }
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

    /// Hook for the command palette (Ctrl+K), which does not exist yet.
    fn open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        log::debug!("the command palette is not built yet");
    }

    /// Hook for the settings window (Ctrl+,), which does not exist yet.
    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, _: &mut Context<Self>) {
        log::debug!("the settings are not built yet");
    }

    /// Hook for the list of shortcuts (Ctrl+?), which does not exist yet.
    fn show_shortcuts(&mut self, _: &ShowShortcuts, _: &mut Window, _: &mut Context<Self>) {
        log::debug!("the list of shortcuts is not built yet");
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember(window);
        if let Some((note, _)) = &self.note {
            // The app waits for the save when it quits with its last window.
            note.update(cx, |note, cx| note.flush(cx)).detach();
        }
        window.remove_window();
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
            .child(self.gallery.clone())
            .when_some(leaving, |root, page| root.child(page))
            .when_some(open, |root, page| root.child(page))
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
                if let Some((note, _)) = &view.note {
                    note.update(cx, |note, cx| note.flush(cx)).detach();
                }
            });
            true
        }
    });
    let root = cx.new(|cx| Root::new(view.clone(), window, cx));
    (root, view)
}
