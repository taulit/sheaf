//! The start view: the notes of the library. For now a plain list that proves the
//! seams; the cards, chips, groups and the folder popover of plan 03 §6 replace the
//! body of this module.
//!
//! # Interface
//!
//! - The window creates one [`GalleryView`] and keeps it alive for the whole session,
//!   also while a note covers it, so it keeps its scroll position and selection.
//! - It reads and changes notes only through the [`Library`] and reloads on every
//!   [`LibraryEvent`].
//! - It asks the window to open a note by emitting [`GalleryEvent::Open`]; creating a
//!   note is the window's `NewNote` action.
//! - The window calls [`set_query`](GalleryView::set_query) for a search that came
//!   from outside (GNOME search, `sheaf` on the command line) and
//!   [`toggle_pin`](GalleryView::toggle_pin) for Ctrl+P.
//! - Keys reach it through the [`actions::GALLERY`] key context.

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use sheaf_core::i18n::tr;
use sheaf_core::{NoteId, NoteMeta, Sort};

use super::actions::{self, NewNote, OpenSelected, Search, SelectNext, SelectPrevious, Trash};
use super::assets::{IconName, icon};
use super::chrome::{FlatButton, HeaderBar, pane};
use super::library::{Library, LibraryEvent};
use super::theme::{ActiveTokens as _, size};

/// More hits than a list can usefully show; the real gallery pages.
const SEARCH_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GalleryEvent {
    /// Open this note; `query` is the search it was found with, or empty.
    Open { id: NoteId, query: String },
}

pub struct GalleryView {
    library: Entity<Library>,
    notes: Vec<NoteMeta>,
    selected: Option<usize>,
    query: String,
    search_focused: bool,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    loading: Task<()>,
    _library_changed: Subscription,
}

impl std::fmt::Debug for GalleryView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GalleryView")
            .field("notes", &self.notes.len())
            .field("selected", &self.selected)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<GalleryEvent> for GalleryView {}

impl GalleryView {
    pub fn new(library: Entity<Library>, cx: &mut Context<Self>) -> Self {
        let library_changed = cx.subscribe(&library, |this, _, _: &LibraryEvent, cx| {
            this.reload(cx);
        });
        let mut view = GalleryView {
            library,
            notes: Vec::new(),
            selected: None,
            query: String::new(),
            search_focused: false,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            loading: Task::ready(()),
            _library_changed: library_changed,
        };
        view.reload(cx);
        view
    }

    /// The notes shown, in the order shown.
    pub fn notes(&self) -> &[NoteMeta] {
        &self.notes
    }

    pub fn selected(&self) -> Option<&NoteMeta> {
        self.notes.get(self.selected?)
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether Ctrl+F has put the focus on the search field.
    pub fn search_focused(&self) -> bool {
        self.search_focused
    }

    /// The scroll position of the list, for the window to prove it survives a note.
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll_handle
    }

    /// Shows the hits of `query` instead of all notes; an empty query shows all.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.trim().to_owned();
        self.selected = None;
        self.reload(cx);
    }

    /// Pins the selected note, or unpins it.
    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.selected() else { return };
        let (id, pinned) = (note.id.clone(), !note.pinned);
        self.library
            .update(cx, |library, cx| library.set_pinned(&id, pinned, cx))
            .detach();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let listing = if self.query.is_empty() {
            let notes = library.notes(Vec::new(), Sort::Modified, cx);
            cx.background_spawn(notes)
        } else {
            let hits = library.search(&self.query, SEARCH_LIMIT, cx);
            cx.background_spawn(async move {
                Ok(hits.await?.hits.into_iter().map(|hit| hit.meta).collect())
            })
        };
        self.loading = cx.spawn(async move |this, cx| {
            let notes: Vec<NoteMeta> = match listing.await {
                Ok(notes) => notes,
                Err(err) => {
                    log::error!("cannot list the notes: {err}");
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                // The selection follows its note, not its row.
                let selected = this.selected().map(|note| note.id.clone());
                this.selected = selected
                    .and_then(|id| notes.iter().position(|note| note.id == id))
                    .or(this.selected.filter(|_| !notes.is_empty()))
                    .map(|index| index.min(notes.len().saturating_sub(1)));
                this.notes = notes;
                cx.notify();
            });
        });
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(note) = self.notes.get(index) else {
            return;
        };
        self.selected = Some(index);
        cx.emit(GalleryEvent::Open {
            id: note.id.clone(),
            query: self.query.clone(),
        });
        cx.notify();
    }

    fn open_selected(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.open(index, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.notes.is_empty() {
            return;
        }
        let last = self.notes.len() - 1;
        let index = match self.selected {
            Some(index) => index.saturating_add_signed(by).min(last),
            None if by > 0 => 0,
            None => last,
        };
        self.selected = Some(index);
        self.scroll_handle.scroll_to_item(index);
        cx.notify();
    }

    fn trash_selected(&mut self, _: &Trash, _: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = self.selected() else { return };
        let id = note.id.clone();
        self.library
            .update(cx, |library, cx| library.trash(&id, cx))
            .detach();
    }

    /// Ctrl+F. The search field itself, with filter chips and type-to-search, is part
    /// of the real gallery; until then the field only shows that it has the focus.
    fn focus_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        self.search_focused = true;
        cx.notify();
    }

    fn render_search(&self, cx: &App) -> impl IntoElement {
        let tokens = cx.tokens();
        let text: SharedString = if self.query.is_empty() {
            tr("Search").into()
        } else {
            self.query.clone().into()
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .w_full()
            .max_w(px(420.))
            .mx_auto()
            .mt(px(2.))
            .mb(px(6.))
            .h(size::SEARCH_HEIGHT)
            .px(px(10.))
            .rounded(size::SEARCH_RADIUS)
            .bg(tokens.hover)
            .text_size(tokens.text(14.))
            .text_color(if self.query.is_empty() {
                tokens.faint
            } else {
                tokens.fg
            })
            .when(self.search_focused, |field| {
                field.border_2().border_color(tokens.accent.opacity(0.7))
            })
            .child(icon(IconName::Search, tokens.faint).size(px(15.)))
            .child(text)
    }

    fn render_row(
        &self,
        index: usize,
        note: &NoteMeta,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = cx.tokens();
        let title: SharedString = if note.title.is_empty() {
            tr("Untitled").into()
        } else {
            note.title.clone().into()
        };
        let date = note
            .modified
            .to_zoned_datetime(self.library.read(cx).now().time_zone().clone())
            .strftime("%Y-%m-%d")
            .to_string();
        let detail: SharedString = if note.folder.is_empty() {
            date.into()
        } else {
            format!("{date} · {}", note.folder).into()
        };
        let selected = self.selected == Some(index);
        div()
            .id(("note", index))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .mx(px(8.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .when(selected, |row| row.bg(tokens.selected))
            .when(!selected, |row| row.hover(|style| style.bg(tokens.hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.open(index, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(tokens.text(14.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(tokens.text(12.))
                            .text_color(tokens.faint)
                            .truncate()
                            .child(detail),
                    ),
            )
            .when(note.pinned, |row| {
                row.child(icon(IconName::Pin, tokens.faint).size(px(13.)))
            })
    }
}

impl Focusable for GalleryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GalleryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let rows: Vec<_> = self
            .notes
            .iter()
            .enumerate()
            .map(|(index, note)| self.render_row(index, note, cx).into_any_element())
            .collect();

        let header = HeaderBar::new()
            .start(FlatButton::new("folders", IconName::Sidebar).tooltip(tr("Folders")))
            .title(tr("All Notes"))
            .end(
                FlatButton::new("as-gallery", IconName::Grid)
                    .tooltip(tr("Gallery"))
                    .active(true),
            )
            .end(FlatButton::new("as-list", IconName::List).tooltip(tr("List")))
            .end(
                FlatButton::new("new-note", IconName::Edit)
                    .tooltip(tr("New Note"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewNote), cx)),
            );

        pane(tokens.window, window, cx)
            .key_context(actions::GALLERY)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::trash_selected))
            .on_action(cx.listener(Self::focus_search))
            .child(header)
            .child(self.render_search(cx))
            .child(
                div()
                    .id("notes")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .pb(px(30.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .when(rows.is_empty(), |list| {
                        list.items_center()
                            .justify_center()
                            .text_size(tokens.text(14.))
                            .text_color(tokens.faint)
                            .child(if self.query.is_empty() {
                                tr("No notes yet")
                            } else {
                                tr("No results")
                            })
                    })
                    .children(rows),
            )
    }
}
