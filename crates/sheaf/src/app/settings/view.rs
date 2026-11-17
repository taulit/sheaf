//! The settings window (Ctrl+,): notes folder, default order, Quick Note, the font and
//! line width of the note text, and the keyboard shortcuts to read, among them the
//! global Quick Note shortcut with the fallback of plan 03 §8. Every choice takes
//! effect at once; there is nothing to confirm.

use std::path::PathBuf;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, MouseButton,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    TitlebarOptions, Window, WindowBounds, WindowDecorations, WindowHandle, WindowOptions, div,
    prelude::FluentBuilder as _, px, size as size_of,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt as _};
use sheaf_core::config::{EditorFont, LineWidth, QuickNote, Sort};
use sheaf_core::i18n::tr;
use sheaf_searchd::api::APP_NAME;

use super::{config, update};
use crate::app::actions::{CloseWindow, QuickNote as QuickNoteAction};
use crate::app::chrome::{HeaderBar, pane};
use crate::app::dialogs::dialogs;
use crate::app::library::Library;
use crate::app::overlays::{self, shortcuts};
use crate::app::theme::ActiveTokens as _;

const WIDTH: f32 = 640.;
const HEIGHT: f32 = 720.;
/// The rows stop growing here and stay in the middle.
const CONTENT_WIDTH: f32 = 560.;
/// Choices for continuing the last quick note, in minutes; 0 is never.
const RESUME_MINUTES: [u32; 4] = [0, 5, 15, 60];

pub struct SettingsView {
    library: Entity<Library>,
    /// The Quick Note settings of the notes folder; `None` until they are read.
    quick: Option<QuickNote>,
    quick_folder: Entity<InputState>,
    /// The notes folder changed: its Quick Note settings are read on the next frame.
    reload_quick_note: bool,
    focus_handle: FocusHandle,
    _loading: Task<()>,
    // Apart, so that one change does not cancel the write of another.
    _choosing: Task<()>,
    _saving: Task<()>,
    _saving_quick_note: Task<()>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl std::fmt::Debug for SettingsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsView")
            .field("quick", &self.quick)
            .finish_non_exhaustive()
    }
}

impl SettingsView {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let quick_folder = cx
            .new(|cx| InputState::new(window, cx).placeholder(QuickNote::default().folder_name()));
        let subscriptions = vec![
            cx.subscribe_in(
                &quick_folder,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.quick_folder_typed(window, cx);
                    }
                },
            ),
            cx.observe_global::<super::Settings>(|_, cx| cx.notify()),
        ];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut view = SettingsView {
            library,
            quick: None,
            quick_folder,
            reload_quick_note: false,
            focus_handle,
            _loading: Task::ready(()),
            _choosing: Task::ready(()),
            _saving: Task::ready(()),
            _saving_quick_note: Task::ready(()),
            _subscriptions: subscriptions,
        };
        view.load_quick_note(window, cx);
        view
    }

    /// Shows the settings of the notes folder of `library`, after the folder changed.
    pub fn set_library(&mut self, library: Entity<Library>, cx: &mut Context<Self>) {
        self.library = library;
        self.reload_quick_note = true;
        cx.notify();
    }

    /// The Quick Note settings as read from the notes folder.
    pub fn quick_note(&self) -> Option<&QuickNote> {
        self.quick.as_ref()
    }

    fn load_quick_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let loaded = self.library.read(cx).folder_config(cx);
        self._loading = cx.spawn_in(window, async move |this, cx| {
            let quick = match loaded.await {
                Ok(config) => config.quick_note,
                Err(err) => {
                    log::warn!("cannot read the folder settings: {err}");
                    QuickNote::default()
                }
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let folder = quick.folder.clone().unwrap_or_default();
                this.quick_folder
                    .update(cx, |input, cx| input.set_value(folder, window, cx));
                this.quick = Some(quick);
                cx.notify();
            });
        });
    }

    /// "Change…": the notes folder chosen in the portal's folder dialog. The main
    /// window follows the setting and shows the new folder.
    pub fn choose_notes_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialogs = dialogs(cx);
        let chosen =
            cx.background_spawn(async move { dialogs.choose_folder(&tr("Choose Notes Folder")) });
        self._choosing = cx.spawn_in(window, async move |this, cx| {
            let folder = match chosen.await {
                Ok(Some(folder)) => folder,
                Ok(None) => return,
                Err(reason) => {
                    let _ = this.update_in(cx, |_, window, cx| failed(&reason, window, cx));
                    return;
                }
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.set_notes_dir(folder, window, cx);
            });
        });
    }

    pub fn set_notes_dir(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let saved = update(cx, |config| config.notes_dir = Some(folder));
        self.saved(saved, window, cx);
    }

    pub fn set_sort(&mut self, sort: Sort, window: &mut Window, cx: &mut Context<Self>) {
        let saved = update(cx, |config| config.sort = Some(sort));
        self.saved(saved, window, cx);
    }

    pub fn set_font(&mut self, font: EditorFont, window: &mut Window, cx: &mut Context<Self>) {
        let saved = update(cx, |config| config.editor.font = font);
        self.saved(saved, window, cx);
    }

    pub fn set_line_width(
        &mut self,
        width: LineWidth,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = update(cx, |config| config.editor.line_width = width);
        self.saved(saved, window, cx);
    }

    pub fn set_resume_minutes(
        &mut self,
        minutes: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut quick) = self.quick.clone() else {
            return;
        };
        quick.resume_minutes = minutes;
        self.store_quick_note(quick, window, cx);
    }

    /// The folder for quick notes as typed; empty for the default.
    fn quick_folder_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut quick) = self.quick.clone() else {
            return;
        };
        let typed = self
            .quick_folder
            .read(cx)
            .value()
            .trim()
            .trim_matches('/')
            .to_owned();
        let folder = (!typed.is_empty()).then_some(typed);
        if folder == quick.folder {
            return;
        }
        quick.folder = folder;
        self.store_quick_note(quick, window, cx);
    }

    pub fn set_quick_folder(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_folder.update(cx, |input, cx| {
            input.set_value(folder.to_owned(), window, cx)
        });
        self.quick_folder_typed(window, cx);
    }

    fn store_quick_note(&mut self, quick: QuickNote, window: &mut Window, cx: &mut Context<Self>) {
        self.quick = Some(quick.clone());
        let stored = self.library.read(cx).set_quick_note(quick, cx);
        self._saving_quick_note = cx.spawn_in(window, async move |this, cx| {
            if let Err(err) = stored.await {
                let reason = err.to_string();
                let _ = this.update_in(cx, |_, window, cx| failed(&reason, window, cx));
            }
        });
        cx.notify();
    }

    fn saved(
        &mut self,
        saved: Task<Result<(), String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._saving = cx.spawn_in(window, async move |this, cx| {
            if let Err(reason) = saved.await {
                let _ = this.update_in(cx, |_, window, cx| failed(&reason, window, cx));
            }
        });
        cx.notify();
    }
}

fn failed(reason: &str, window: &mut Window, cx: &mut App) {
    log::error!("cannot store the settings: {reason}");
    let message = tr("Could not save the settings: {}").replace("{}", reason);
    window.push_notification(Notification::error(message), cx);
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// A group of rows under a quiet heading, on a card.
fn group(title: String, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let tokens = cx.tokens();
    let count = rows.len();
    let mut card = div()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .bg(tokens.card)
        .border_1()
        .border_color(tokens.hair);
    for (index, row) in rows.into_iter().enumerate() {
        card = card.child(row);
        if index + 1 < count {
            card = card.child(div().h(px(1.)).bg(tokens.hair));
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .px(px(4.))
                .text_size(tokens.text(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(tokens.faint)
                .child(title),
        )
        .child(card)
        .into_any_element()
}

/// A row: title and quieter subtitle on the left, the control on the right.
fn row(
    title: impl Into<SharedString>,
    subtitle: Option<String>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let tokens = cx.tokens();
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(52.))
        .px(px(14.))
        .py(px(8.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(div().text_size(tokens.text(14.)).child(title.into()))
                .children(subtitle.map(|subtitle| {
                    div()
                        .text_size(tokens.text(12.5))
                        .text_color(tokens.muted)
                        .child(subtitle)
                })),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// Keys as on the list of shortcuts.
fn key_caps(keys: String, cx: &App) -> AnyElement {
    let tokens = cx.tokens();
    div()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(6.))
        .bg(tokens.hover)
        .text_size(tokens.text(12.5))
        .text_color(tokens.muted)
        .child(keys)
        .into_any_element()
}

/// A row of buttons of which one is chosen.
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String)>,
    chosen: T,
    cx: &mut Context<SettingsView>,
    pick: fn(&mut SettingsView, T, &mut Window, &mut Context<SettingsView>),
) -> AnyElement {
    let tokens = cx.tokens().clone();
    div()
        .id(id)
        .flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(9.))
        .bg(tokens.hover)
        .children(
            options
                .into_iter()
                .enumerate()
                .map(|(index, (value, label))| {
                    let on = value == chosen;
                    let hover = tokens.hover;
                    div()
                        .id((id, index))
                        .h(px(28.))
                        .px(px(11.))
                        .flex()
                        .items_center()
                        .rounded(px(7.))
                        .text_size(tokens.text(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .when(on, |button| button.bg(tokens.view).shadow_sm())
                        .when(!on, |button| {
                            button
                                .text_color(tokens.muted)
                                .hover(move |style| style.bg(hover))
                        })
                        .on_click(
                            cx.listener(move |this, _, window, cx| pick(this, value, window, cx)),
                        )
                        .child(label)
                }),
        )
        .into_any_element()
}

/// A button of 04 "Buttons".
fn button(id: &'static str, label: String, cx: &App) -> gpui::Stateful<gpui::Div> {
    let tokens = cx.tokens();
    let hover = tokens.hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(8.))
        .bg(tokens.selected)
        .text_size(tokens.text(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

fn sort_label(sort: Sort) -> String {
    match sort {
        Sort::Modified => tr("Date Edited"),
        Sort::Created => tr("Date Created"),
        Sort::Title => tr("Title"),
    }
}

fn resume_label(minutes: u32) -> String {
    if minutes == 0 {
        tr("Never")
    } else {
        tr("{} min").replace("{}", &minutes.to_string())
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.reload_quick_note) {
            self.load_quick_note(window, cx);
        }
        let tokens = cx.tokens().clone();
        let config = config(cx);
        let notes_dir = self.library.read(cx).notes_dir().display().to_string();
        let sort = config.sort.unwrap_or_default();

        let change_folder = button("change-folder", tr("Change…"), cx)
            .on_click(cx.listener(|this, _, window, cx| this.choose_notes_folder(window, cx)));
        let sort_control = segmented(
            "sort",
            [Sort::Modified, Sort::Created, Sort::Title]
                .into_iter()
                .map(|sort| (sort, sort_label(sort)))
                .collect(),
            sort,
            cx,
            SettingsView::set_sort,
        );
        let resume = self.quick.as_ref().map_or(5, |quick| quick.resume_minutes);
        let resume_control = segmented(
            "resume",
            RESUME_MINUTES
                .into_iter()
                .map(|minutes| (minutes, resume_label(minutes)))
                .collect(),
            resume,
            cx,
            SettingsView::set_resume_minutes,
        );
        let font_control = segmented(
            "font",
            vec![
                (EditorFont::System, tr("System")),
                (EditorFont::Serif, tr("Serif")),
                (EditorFont::Mono, tr("Mono")),
            ],
            config.editor.font,
            cx,
            SettingsView::set_font,
        );
        let width_control = segmented(
            "line-width",
            vec![
                (LineWidth::Narrow, tr("Narrow")),
                (LineWidth::Medium, tr("Medium")),
                (LineWidth::Wide, tr("Wide")),
            ],
            config.editor.line_width,
            cx,
            SettingsView::set_line_width,
        );
        let folder_field = div()
            .w(px(180.))
            .h(px(32.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(tokens.hover)
            .child(
                Input::new(&self.quick_folder)
                    .appearance(false)
                    .flex_1()
                    .h_full()
                    .px_0()
                    .text_size(tokens.text(13.5)),
            );

        let folder = group(
            tr("Notes Folder"),
            vec![row(
                notes_dir,
                Some(tr("Notes are plain Markdown files in this folder")),
                change_folder,
                cx,
            )],
            cx,
        );
        let gallery = group(
            tr("Gallery"),
            vec![row(
                tr("Sort Notes By"),
                Some(tr(
                    "Folders sorted differently in the gallery keep their order",
                )),
                sort_control,
                cx,
            )],
            cx,
        );
        let quick = group(
            tr("Quick Note"),
            vec![
                row(
                    tr("Folder"),
                    Some(tr("Where new quick notes are stored")),
                    folder_field,
                    cx,
                ),
                row(
                    tr("Continue the Last Quick Note"),
                    Some(tr(
                        "A quick note this recent is continued instead of starting a new one",
                    )),
                    resume_control,
                    cx,
                ),
                row(
                    tr("Shortcut from Anywhere"),
                    Some(tr(
                        "While Notes runs. Otherwise add a custom shortcut for “sheaf --quick” in GNOME Settings, Keyboard",
                    )),
                    key_caps(
                        // The keys the global shortcut proposes to GNOME, see `shortcut`.
                        overlays::format_keys(&KeyBinding::new(
                            "ctrl-shift-n",
                            QuickNoteAction,
                            None,
                        )),
                        cx,
                    ),
                    cx,
                ),
            ],
            cx,
        );
        let editor = group(
            tr("Editor"),
            vec![
                row(tr("Font"), None, font_control, cx),
                row(tr("Line Width"), None, width_control, cx),
            ],
            cx,
        );
        // One card per context: Esc closes the Quick Note and leaves a note.
        let keys: Vec<AnyElement> = shortcuts::groups(cx)
            .into_iter()
            .map(|shortcuts| {
                let rows = shortcuts
                    .lines
                    .into_iter()
                    .map(|(label, keys)| {
                        div()
                            .flex()
                            .items_center()
                            .min_h(px(40.))
                            .px(px(14.))
                            .py(px(5.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(tokens.text(13.5))
                                    .child(label),
                            )
                            .child(key_caps(keys, cx))
                            .into_any_element()
                    })
                    .collect();
                let title = format!("{} · {}", tr("Keyboard Shortcuts"), shortcuts.title);
                group(title, rows, cx)
            })
            .collect();

        pane(tokens.window, window, cx)
            .id("settings")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            .child(HeaderBar::new().title(tr("Settings")))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .px(px(20.))
                            .pt(px(8.))
                            .pb(px(28.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(22.))
                                    .w_full()
                                    .max_w(px(CONTENT_WIDTH))
                                    .child(folder)
                                    .child(gallery)
                                    .child(quick)
                                    .child(editor)
                                    .children(keys),
                            ),
                    ),
            )
    }
}

/// Opens the settings window.
pub fn open(
    library: Entity<Library>,
    cx: &mut App,
) -> anyhow::Result<(WindowHandle<Root>, Entity<SettingsView>)> {
    let bounds = Bounds::centered(None, size_of(px(WIDTH), px(HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(tr("Settings").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size_of(px(400.), px(420.))),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(APP_NAME.to_owned()),
        ..Default::default()
    };
    let mut view = None;
    let handle = cx.open_window(options, |window, cx| {
        let (root, settings) = build(library, window, cx);
        view = Some(settings);
        root
    })?;
    let view = view.ok_or_else(|| anyhow::anyhow!("the settings window was not built"))?;
    Ok((handle, view))
}

/// The content of the settings window, inside gpui-component's `Root`.
pub fn build(
    library: Entity<Library>,
    window: &mut Window,
    cx: &mut App,
) -> (Entity<Root>, Entity<SettingsView>) {
    let view = cx.new(|cx| SettingsView::new(library, window, cx));
    let root = cx.new(|cx| Root::new(view.clone(), window, cx));
    (root, view)
}
