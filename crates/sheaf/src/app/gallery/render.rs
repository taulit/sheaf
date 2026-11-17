//! Drawing the gallery around its notes: the header bar, the search field with its
//! chips, the chip row, the popovers of the header bar and the empty states. Sizes
//! are those of the prototype (`.search`, `.chips`, `.chip`, `.popo`) and of 04.

use gpui::{
    AnyElement, AppContext as _, BoxShadow, Context, Div, ElementId, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, canvas, deferred, div, hsla,
    list, point, prelude::FluentBuilder as _, px, relative,
};
use gpui_component::input::{self as input, Input, InputEvent, InputState};
use sheaf_core::config::{Sort as SortOrder, View};
use sheaf_core::i18n::tr;

use super::{GalleryView, Popover, Scope, chips};
use crate::app::actions::{
    self, NewNote, SaveSearchAsSmartFolder, ShowFolders, ShowSortOptions, ShowTemplates, ToggleView,
};
use crate::app::assets::{IconName, icon};
use crate::app::chrome::{FlatButton, HeaderBar, pane};
use crate::app::theme::{ActiveTokens as _, Tokens, size};
use crate::app::trace::{self, Event};

/// The side padding of the grid (`.gal`).
pub(super) const GRID_SIDE: f32 = 24.;

impl Render for GalleryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.covers.next_frame();
        self.drawn = Default::default();
        if let Some(text) = self.pending_text.take() {
            self.search
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        let width = self
            .list_width
            .unwrap_or_else(|| window.viewport_size().width);
        let columns = super::layout::columns(width - px(2. * GRID_SIDE));
        if columns != self.columns {
            self.columns = columns;
            self.relayout();
        }
        let tokens = cx.tokens().clone();

        pane(tokens.window, window, cx)
            .relative()
            // CSS `line-height: normal` of Inter, which the prototype's sizes assume.
            .line_height(relative(1.21))
            .key_context(actions::GALLERY)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::trash))
            .on_action(cx.listener(Self::search_action))
            .on_action(cx.listener(|this, _: &ToggleView, _, cx| this.toggle_view(cx)))
            .on_action(cx.listener(|this, _: &ShowSortOptions, _, cx| {
                this.set_popover(Some(Popover::Sort), cx)
            }))
            .on_action(cx.listener(|this, _: &ShowFolders, _, cx| {
                this.set_popover(Some(Popover::Folders), cx)
            }))
            .on_action(
                cx.listener(|this, _: &SaveSearchAsSmartFolder, window, cx| {
                    this.ask_smart_folder_name(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShowTemplates, _, cx| this.show_templates(cx)))
            .on_action(cx.listener(|this, action, window, cx| {
                if this.confirming().is_some() {
                    this.cancel_confirm(cx);
                } else if this.shown_trashed().is_some() {
                    this.close_trashed(cx);
                } else if this.popover.is_some() {
                    this.set_popover(None, cx);
                } else {
                    this.clear(action, window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event, window, cx| {
                this.type_to_search(event, window, cx);
            }))
            .child(self.render_header(cx))
            .child(self.render_search(&tokens, cx))
            .children(self.render_scope_chips(&tokens, cx))
            .child(self.render_body(&tokens, cx))
            .children(self.render_popover(&tokens, cx))
            .children(self.render_trash_page(window, cx))
            .children(self.render_confirm(cx))
    }
}

impl GalleryView {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view();
        let toggle = |popover: Popover| {
            cx.listener(move |this: &mut Self, _: &gpui::ClickEvent, _, cx| {
                let next = (this.popover != Some(popover)).then_some(popover);
                this.set_popover(next, cx);
            })
        };
        HeaderBar::new()
            .start(
                FlatButton::new("folders", IconName::Sidebar)
                    .tooltip(tr("Folders"))
                    .active(self.popover == Some(Popover::Folders))
                    .on_click(toggle(Popover::Folders)),
            )
            .title(self.scope.title())
            .when(
                self.scope == Scope::Trash && !self.notes.is_empty(),
                |bar| {
                    let tokens = cx.tokens().clone();
                    bar.end(
                        super::trash::pill(
                            "empty-trash",
                            tr("Empty Trash"),
                            super::trash::Kind::Plain,
                            &tokens,
                        )
                        .mr(px(4.))
                        .on_click(cx.listener(|this, _, _, cx| this.ask_empty_trash(cx))),
                    )
                },
            )
            .end(
                FlatButton::new("sort", IconName::Sort)
                    .tooltip(tr("Sort"))
                    .active(self.popover == Some(Popover::Sort))
                    .on_click(toggle(Popover::Sort)),
            )
            .end(
                FlatButton::new("as-gallery", IconName::Grid)
                    .tooltip(tr("Gallery"))
                    .active(view == View::Gallery)
                    .on_click(cx.listener(|this, _, _, cx| this.set_view(View::Gallery, cx))),
            )
            .end(
                FlatButton::new("as-list", IconName::List)
                    .tooltip(tr("List"))
                    .active(view == View::List)
                    .on_click(cx.listener(|this, _, _, cx| this.set_view(View::List, cx))),
            )
            // New notes never land in the trash.
            .when(self.scope != Scope::Trash, |bar| {
                bar.end(
                    FlatButton::new("new-note", IconName::Edit)
                        .tooltip(tr("New Note"))
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewNote), cx)),
                )
                .end(
                    FlatButton::new("new-note-menu", IconName::PanDown)
                        .tooltip(tr("New Note from Template"))
                        .active(self.popover == Some(Popover::Templates))
                        .on_click(toggle(Popover::Templates)),
                )
            })
    }

    /// The field: the search icon, the chips, then the text.
    fn render_search(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let chips = self.chips.iter().enumerate().map(|(index, filter)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(3.))
                .h(px(24.))
                .pl(px(9.))
                .pr(px(4.))
                .rounded_full()
                .bg(tokens.selected)
                .text_size(tokens.text(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.fg)
                .child(chips::label(filter))
                .child(
                    div()
                        .id(("remove-chip", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|style| style.bg(tokens.hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_chip(index, cx)))
                        .child(icon(IconName::Close, tokens.muted).size(px(9.))),
                )
        });
        div().flex_none().px(px(12.)).mt(px(2.)).mb(px(6.)).child(
            div()
                .id("search-field")
                .flex()
                .items_center()
                .gap(px(8.))
                .w_full()
                .max_w(px(420.))
                .mx_auto()
                .h(size::SEARCH_HEIGHT)
                .pl(px(10.))
                .pr(px(4.))
                .rounded(size::SEARCH_RADIUS)
                .bg(tokens.hover)
                .overflow_hidden()
                .text_size(tokens.text(14.))
                .when(self.search_focused, |field| {
                    field.shadow(vec![ring(tokens.accent.opacity(0.7), 2.)])
                })
                // The field passes these on when it has nothing to do with them.
                .on_action(
                    cx.listener(|this, _: &input::Backspace, _, cx| this.remove_last_chip(cx)),
                )
                .on_action(cx.listener(|this, _: &input::Escape, window, cx| {
                    this.escape_search(window, cx);
                }))
                .child(icon(IconName::Search, tokens.faint).size(px(15.)))
                .children(chips)
                .child(
                    Input::new(&self.search)
                        .appearance(false)
                        .flex_1()
                        .min_w(px(40.))
                        .h_full()
                        .px_0()
                        .text_size(tokens.text(14.)),
                ),
        )
    }

    /// "All", the folders at the top and the pinned tags; nothing if there are no
    /// folders or tags to choose, or in the trash, which they do not filter.
    fn render_scope_chips(
        &self,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if self.scope == Scope::Trash {
            return None;
        }
        let mut scopes = vec![(Scope::All, tr("All"))];
        scopes.extend(
            self.folders
                .iter()
                .filter(|folder| !folder.path.is_empty() && !folder.path.contains('/'))
                .map(|folder| (Scope::Folder(folder.path.clone()), folder.path.clone())),
        );
        scopes.extend(
            self.config
                .iter()
                .flat_map(|config| &config.pinned_tags)
                .map(|tag| (Scope::Tag(tag.clone()), format!("#{tag}"))),
        );
        scopes.extend(self.smart_scopes());
        if scopes.len() == 1 {
            return None;
        }
        let chips = scopes
            .into_iter()
            .enumerate()
            .map(|(index, (scope, label))| {
                let on = self.scope == scope;
                div()
                    .id(("scope", index))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(size::CHIP_HEIGHT)
                    .px(px(11.))
                    .rounded_full()
                    .text_size(tokens.text(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(on, |chip| chip.bg(tokens.fg).text_color(tokens.view))
                    .when(!on, |chip| {
                        chip.bg(tokens.hover)
                            .text_color(tokens.muted)
                            .hover(|style| style.bg(tokens.selected))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope.clone(), cx)))
                    .child(label)
            });
        let chips: Vec<_> = chips.collect();
        Some(
            div()
                .id("scope-chips")
                .flex()
                .flex_none()
                .overflow_x_scroll()
                .px(px(12.))
                .pb(px(8.))
                .child(div().flex().gap(px(6.)).mx_auto().children(chips)),
        )
    }

    fn render_body(&mut self, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        if !self.loaded {
            return div().flex_1().into_any_element();
        }
        if self.notes.is_empty() {
            return self.render_empty(tokens, cx).into_any_element();
        }
        let gallery = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(
                    self.list_state.clone(),
                    cx.processor(|this, index, window, cx| this.render_row(index, window, cx)),
                )
                .size_full(),
            )
            // Measures the list, whose width decides how many cards make a row.
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = gallery.update(cx, |this, cx| {
                            if this.list_width != Some(bounds.size.width) {
                                this.list_width = Some(bounds.size.width);
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(trace::pending(Event::Miniatures), |body| {
                body.child(self.trace_frame(cx))
            })
            .into_any_element()
    }

    /// Marks the frame in the startup trace once the list above it is painted.
    fn trace_frame(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let gallery = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, _, cx| {
                let Some(gallery) = gallery.upgrade() else {
                    return;
                };
                let drawn = &gallery.read(cx).drawn;
                if drawn.notes.is_empty() {
                    return;
                }
                trace::mark_with(Event::Cards, &drawn.notes);
                if drawn.unready == 0 {
                    trace::mark(Event::Miniatures);
                }
            },
        )
    }

    fn render_empty(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = !self.chips.is_empty() || !self.text.trim().is_empty();
        let filtered = !self.chips.is_empty() || self.scope != Scope::All;
        let message = if searching && self.text.trim().is_empty() {
            tr("No results")
        } else if searching {
            tr("No results for “{}”").replace("{}", self.text.trim())
        } else {
            match &self.scope {
                Scope::All => tr("No notes yet"),
                Scope::Folder(_) => tr("No notes in this folder"),
                Scope::Tag(_) => tr("No notes with this tag"),
                Scope::Trash => tr("No recently deleted notes"),
                Scope::Smart(_) => tr("No notes in this smart folder"),
            }
        };
        let button = if searching {
            filtered.then(|| {
                pill(
                    "without-filters",
                    tr("Search Without Filters"),
                    false,
                    tokens,
                )
                .on_click(cx.listener(|this, _, _, cx| this.search_without_filters(cx)))
            })
        } else if self.scope == Scope::Trash {
            None
        } else {
            Some(
                pill("empty-new-note", tr("New Note"), true, tokens)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewNote), cx)),
            )
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .pb(size::HEADER_HEIGHT)
            .child(
                div()
                    .text_size(tokens.text(14.))
                    .text_color(tokens.faint)
                    .child(message),
            )
            .children(button)
    }

    /// The open popover over a backdrop that closes it.
    fn render_popover(&mut self, tokens: &Tokens, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popover = self.popover?;
        let surface = match popover {
            Popover::Folders => self.render_folders(tokens, cx).into_any_element(),
            Popover::Sort => self.render_sort_menu(tokens, cx).into_any_element(),
            Popover::Templates => self.render_templates_menu(tokens, cx).into_any_element(),
        };
        let overlay = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                div()
                    .id("popover-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.set_popover(None, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| this.set_popover(None, cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(size::HEADER_HEIGHT - px(4.))
                    .when(popover == Popover::Folders, |slot| slot.left(px(8.)))
                    // Under the sort button: the bar's padding, the close button and
                    // three 34 px buttons with their gaps are right of it.
                    .when(popover == Popover::Sort, |slot| slot.right(px(196.)))
                    // Under `+ ▾`, the last button before the close button.
                    .when(popover == Popover::Templates, |slot| slot.right(px(44.)))
                    .child(surface),
            );
        Some(deferred(overlay).with_priority(1).into_any_element())
    }

    fn render_folders(&mut self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let total: usize = self.folders.iter().map(|folder| folder.notes).sum();
        let mut rows = vec![
            menu_row("folder-all", IconName::Text, tr("All Notes"), tokens)
                .when(self.scope == Scope::All, |row| row.bg(tokens.selected))
                .child(count(total, tokens))
                .on_click(cx.listener(|this, _, _, cx| this.set_scope(Scope::All, cx)))
                .into_any_element(),
        ];
        for (index, folder) in self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.path.is_empty())
        {
            let depth = folder.path.matches('/').count();
            let name = folder
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            let prefix = format!("{}/", folder.path);
            let notes: usize = self
                .folders
                .iter()
                .filter(|other| other.path == folder.path || other.path.starts_with(&prefix))
                .map(|other| other.notes)
                .sum();
            let scope = Scope::Folder(folder.path.clone());
            let on = self.scope == scope;
            rows.push(
                menu_row(("folder", index), IconName::Folder, name, tokens)
                    .pl(px(12. + 18. * depth as f32))
                    .when(on, |row| row.bg(tokens.selected))
                    .child(count(notes, tokens))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope.clone(), cx)))
                    .into_any_element(),
            );
        }
        let trash = menu_row(
            "folder-trash",
            IconName::Trash,
            tr("Recently Deleted"),
            tokens,
        )
        .when(self.scope == Scope::Trash, |row| row.bg(tokens.selected))
        .when_some(self.trash_count(), |row, notes| {
            row.child(count(notes, tokens))
        })
        .on_click(cx.listener(|this, _, _, cx| this.set_scope(Scope::Trash, cx)));
        let new_folder = match &self.new_folder {
            Some(name) => div()
                .flex()
                .items_center()
                .gap(px(11.))
                .h(px(34.))
                .px(px(12.))
                .rounded(px(9.))
                .bg(tokens.hover)
                .child(icon(IconName::FolderNew, tokens.muted).size(px(16.)))
                .child(
                    Input::new(name)
                        .appearance(false)
                        .flex_1()
                        .h_full()
                        .px_0()
                        .text_size(tokens.text(13.5)),
                )
                .into_any_element(),
            None => menu_row("new-folder", IconName::FolderNew, tr("New Folder"), tokens)
                .on_click(cx.listener(|this, _, window, cx| this.start_new_folder(window, cx)))
                .into_any_element(),
        };
        let smart = self.render_smart_rows(tokens, cx);
        let new_smart_folder = self.render_new_smart_folder(tokens, cx);
        surface(tokens)
            .id("folders-popover")
            .on_action(cx.listener(|this, _: &input::Escape, _, cx| this.set_popover(None, cx)))
            .w(px(250.))
            .max_h(px(520.))
            .overflow_y_scroll()
            .children(rows)
            .when(!smart.is_empty(), |popover| {
                popover.child(separator(tokens)).children(smart)
            })
            .child(separator(tokens))
            .child(trash)
            .child(separator(tokens))
            .child(new_folder)
            .child(new_smart_folder)
    }

    fn start_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Folder Name")));
        cx.subscribe_in(
            &name,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let name = state.read(cx).value().to_string();
                    this.create_folder(&name, window, cx);
                }
            },
        )
        .detach();
        name.update(cx, |state, cx| state.focus(window, cx));
        self.new_folder = Some(name);
        cx.notify();
    }

    fn render_sort_menu(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.sort();
        let options = [
            (SortOrder::Modified, tr("Date Edited")),
            (SortOrder::Created, tr("Date Created")),
            (SortOrder::Title, tr("Title")),
        ];
        surface(tokens)
            .id("sort-popover")
            .w(px(220.))
            .child(
                div()
                    .px(px(12.))
                    .pt(px(6.))
                    .pb(px(4.))
                    .text_size(tokens.text(12.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens.faint)
                    .child(tr("Sort By")),
            )
            .children(options.into_iter().map(|(sort, label)| {
                div()
                    .id(SharedString::from(format!("sort-{sort:?}")))
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .px(px(12.))
                    .rounded(px(9.))
                    .text_size(tokens.text(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .hover(|style| style.bg(tokens.hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_sort(sort, cx)))
                    .child(div().flex_1().child(label))
                    .when(sort == current, |row| {
                        row.child(icon(IconName::Check, tokens.accent).size(px(14.)))
                    })
            }))
    }
}

/// `color` around an element, as CSS `box-shadow: 0 0 0 <width>px`.
pub(super) fn ring(color: Hsla, width: f32) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: false,
    }
}

/// A popover's panel: `.popo` of the prototype.
pub(super) fn surface(tokens: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .p(px(6.))
        .rounded(size::POPOVER_RADIUS)
        .bg(tokens.popover)
        .text_color(tokens.fg)
        .border_1()
        .border_color(tokens.hair)
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.55),
            offset: point(px(0.), px(14.)),
            blur_radius: px(40.),
            spread_radius: px(-8.),
            inset: false,
        }])
        // Clicks inside must not reach the backdrop.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(super) fn menu_row(
    id: impl Into<ElementId>,
    symbol: IconName,
    label: impl Into<SharedString>,
    tokens: &Tokens,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(11.))
        .h(px(34.))
        .px(px(12.))
        .rounded(px(9.))
        .text_size(tokens.text(13.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|style| style.bg(tokens.hover))
        .child(icon(symbol, tokens.muted).size(px(16.)))
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

fn count(notes: usize, tokens: &Tokens) -> impl IntoElement {
    div()
        .text_size(tokens.text(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(tokens.faint)
        .child(notes.to_string())
}

pub(super) fn separator(tokens: &Tokens) -> impl IntoElement {
    div()
        .flex_none()
        .h(px(1.))
        .mx(px(6.))
        .my(px(4.))
        .bg(tokens.hair)
}

/// `.btn` of the prototype; `primary` is `.btn.pri`.
fn pill(id: &'static str, label: String, primary: bool, tokens: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
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
        .child(label)
}
