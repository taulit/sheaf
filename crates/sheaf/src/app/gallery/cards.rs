//! The notes themselves: rows of cards (`.gc` of the prototype), compact list rows
//! (`.nr` of variant 1), group headings, and the context menu of a note.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnimationExt as _, AnyElement, ClickEvent, Context, Div, FontWeight, HighlightStyle,
    InteractiveElement as _, IntoElement, MouseButton, ObjectFit, ParentElement as _, RenderImage,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, StyledText,
    WeakEntity, Window, div, img, linear_color_stop, linear_gradient, prelude::FluentBuilder as _,
    px, relative,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use jiff::ZonedDateTime;
use sheaf_core::config::{Sort as SortOrder, View};
use sheaf_core::i18n::tr;
use sheaf_core::{NoteId, NoteMeta};

use super::dates::{DateLabel, Group};
use super::layout::Row;
use super::preview::{Line, Preview};
use super::render::{GRID_SIDE, ring};
use super::{GalleryView, Scope};
use crate::app::assets::{IconName, icon};
use crate::app::theme::{ActiveTokens as _, Tokens, motion, size};

/// The list view is a column of this width at most, as rows across a wide window
/// would be hard to read.
const LIST_MAX_WIDTH: f32 = 760.;
const COVER_HEIGHT: f32 = 70.;

impl GalleryView {
    /// Item `index` of the virtual list.
    pub(super) fn render_row(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = self.layout.rows();
        let Some(row) = rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let (first, last) = (index == 0, index + 1 == rows.len());
        let tokens = cx.tokens().clone();
        let content = match (&row, self.view()) {
            (Row::Header(group), View::Gallery) => grid_heading(*group, &tokens).into_any_element(),
            (Row::Header(group), View::List) => list_heading(*group, &tokens).into_any_element(),
            (Row::Notes(range), View::Gallery) => self.render_cards(range.clone(), &tokens, cx),
            (Row::Notes(range), View::List) => self.render_list_row(range.start, &tokens, cx),
        };
        let list = self.view() == View::List;
        div()
            .w_full()
            .when(!list, |row| {
                row.px(px(GRID_SIDE))
                    .pt(px(if first { 8. } else { 16. }))
                    .when(last, |row| row.pb(px(30.)))
            })
            .when(list, |row| {
                row.flex()
                    .justify_center()
                    .when(first, |row| row.pt(px(4.)))
                    .when(last, |row| row.pb(px(30.)))
            })
            .child(content)
            .into_any_element()
    }

    fn render_cards(
        &mut self,
        range: Range<usize>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = self.library.read(cx).now();
        let mut cards: Vec<AnyElement> = range
            .clone()
            .map(|index| self.render_card(index, &now, tokens, cx))
            .collect();
        // A short last row keeps the width of the columns above it.
        cards.extend((range.len()..self.columns).map(|_| div().flex_1().into_any_element()));
        div()
            .flex()
            .gap(size::GALLERY_GAP)
            .children(cards)
            .into_any_element()
    }

    fn render_card(
        &mut self,
        index: usize,
        now: &ZonedDateTime,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let note = self.notes[index].clone();
        let selected = self.is_selected(index);
        let marks = self.marks.get(&note.id).cloned();
        let preview = self.preview(&note, cx);
        let cover = self.cover_of(&note, preview.as_deref(), cx);

        let mut card = div()
            .relative()
            .h(size::CARD_HEIGHT)
            .p(size::CARD_PADDING)
            .rounded(size::CARD_RADIUS)
            .bg(tokens.card)
            // A ring outside, as `box-shadow` in the prototype, so that the fade at the
            // bottom does not cover it.
            .shadow(vec![if selected {
                ring(tokens.accent, 2.)
            } else {
                ring(tokens.hair, 1.)
            }])
            .overflow_hidden()
            .text_size(tokens.text(11.5))
            .line_height(relative(1.5))
            .text_color(tokens.muted)
            .when_some(cover, |card, image| {
                let place = div()
                    .flex()
                    .w_full()
                    .h(px(COVER_HEIGHT))
                    .mb(px(6.))
                    .rounded(px(8.));
                card.child(match image {
                    Some(image) => place
                        .child(
                            // With a width in percent an image takes its height from
                            // its own aspect ratio and grows into the text below; as
                            // a flex item it keeps the height given here.
                            img(image)
                                .flex_1()
                                .min_w_0()
                                .h(px(COVER_HEIGHT))
                                .rounded(px(8.))
                                .object_fit(ObjectFit::Cover),
                        )
                        .into_any_element(),
                    None => place.bg(tokens.hover).into_any_element(),
                })
            });
        card = match (&marks, &preview) {
            (Some(marks), _) => card.child(highlighted(
                &marks.snippet.text,
                &marks.snippet.highlights,
                tokens,
            )),
            (None, Some(preview)) => card.children(miniature(preview, tokens)),
            (None, None) => card,
        };
        let card = card.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(50.))
                // GPUI clips children to the rectangle, not the rounded corners.
                .rounded_b(size::CARD_RADIUS)
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(tokens.card.opacity(0.), 0.),
                    linear_color_stop(tokens.card, 1.),
                )),
        );
        let card = self.lift(card, &note, tokens);

        let title = match &marks {
            Some(marks) => highlighted(&title_of(&note), &marks.title, tokens).into_any_element(),
            None => title_of(&note).into_any_element(),
        };
        let date = DateLabel::of(self.date_of(&note), now).format();
        let meta = match self.trash_label(&note, now) {
            Some(label) => label,
            None => match note
                .folder
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
            {
                Some(folder) => format!("{date} · {folder}"),
                None => date,
            },
        };
        let element = div()
            .id(SharedString::from(format!("card-{}", note.id)))
            .flex_1()
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener({
                let id = note.id.clone();
                move |this, event: &ClickEvent, _, cx| this.click_note(&id, event.modifiers(), cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = note.id.clone();
                    move |this, _, _, cx| this.select_for_menu(&id, cx)
                }),
            )
            .on_hover(cx.listener({
                let id = note.id.clone();
                move |this, hovered: &bool, _, cx| this.set_hovered(&id, *hovered, cx)
            }))
            .child(card)
            .child(
                div()
                    .mt(px(10.))
                    .mx(px(2.))
                    .text_center()
                    .text_size(tokens.text(13.5))
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .text_center()
                    .text_size(tokens.text(12.))
                    .text_color(tokens.faint)
                    .truncate()
                    .child(meta),
            );
        self.with_menu(element, cx)
    }

    /// Raises the card under the pointer by 2 px on the spring, and lets the one it
    /// left sink back.
    fn lift(&self, card: Div, note: &NoteMeta, tokens: &Tokens) -> AnyElement {
        let on = self.hover.on.as_ref() == Some(&note.id);
        let left = self.hover.left.as_ref() == Some(&note.id);
        if !tokens.animations {
            return card.when(on, |card| card.top(px(-2.))).into_any_element();
        }
        let id = SharedString::from(format!("lift-{}-{}", note.id, self.hover.epoch));
        let spring = tokens.spring(motion::CARD_HOVER);
        if on {
            card.with_animation(id, spring, |card, t| card.top(px(-2. * t)))
                .into_any_element()
        } else if left {
            card.with_animation(id, spring, |card, t| card.top(px(-2. * (1. - t))))
                .into_any_element()
        } else {
            card.into_any_element()
        }
    }

    fn set_hovered(&mut self, id: &sheaf_core::NoteId, hovered: bool, cx: &mut Context<Self>) {
        let hover = &mut self.hover;
        if hovered && hover.on.as_ref() != Some(id) {
            hover.left = hover.on.replace(id.clone());
        } else if !hovered && hover.on.as_ref() == Some(id) {
            hover.left = hover.on.take();
        } else {
            return;
        }
        hover.epoch += 1;
        cx.notify();
    }

    fn render_list_row(
        &mut self,
        index: usize,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let note = self.notes[index].clone();
        let selected = self.is_selected(index);
        // The hairline between rows gives way around the selected one.
        let after_selected = index > 0 && self.is_selected(index - 1);
        let first_in_group = self
            .layout
            .row_of(index)
            .and_then(|row| row.checked_sub(1))
            .is_none_or(|above| matches!(self.layout.rows()[above], Row::Header(_)));
        let marks = self.marks.get(&note.id).cloned();
        let preview = self.preview(&note, cx);
        let now = self.library.read(cx).now();
        let date = DateLabel::of(self.date_of(&note), &now).format();
        let summary = match (&marks, &preview) {
            (Some(marks), _) => highlighted(&marks.snippet.text, &marks.snippet.highlights, tokens)
                .into_any_element(),
            (None, Some(preview)) => preview.summary().into_any_element(),
            (None, None) => "".into_any_element(),
        };
        let title = match &marks {
            Some(marks) => highlighted(&title_of(&note), &marks.title, tokens).into_any_element(),
            None => title_of(&note).into_any_element(),
        };
        let cover = self.cover_of(&note, preview.as_deref(), cx);
        let days_left = self.trash_days_left(&note, now.timestamp());
        let folder = (self.scope == Scope::All && !note.folder.is_empty()).then(|| {
            note.folder
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned()
        });
        let (folder, folder_icon) = match days_left {
            Some(left) => (Some(left), IconName::Trash),
            None => (folder, IconName::Folder),
        };

        let row = div()
            .id(SharedString::from(format!("row-{}", note.id)))
            .relative()
            .flex()
            .gap(px(10.))
            .w_full()
            .max_w(px(LIST_MAX_WIDTH))
            .mx(px(8.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .when(selected, |row| row.bg(tokens.accent.opacity(0.26)))
            .when(!selected, |row| row.hover(|style| style.bg(tokens.hover)))
            .when(!first_in_group && !selected && !after_selected, |row| {
                row.child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(12.))
                        .right(px(12.))
                        .h(px(1.))
                        .bg(tokens.hair),
                )
            })
            .on_click(cx.listener({
                let id = note.id.clone();
                move |this, event: &ClickEvent, _, cx| this.click_note(&id, event.modifiers(), cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = note.id.clone();
                    move |this, _, _, cx| this.select_for_menu(&id, cx)
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(tokens.text(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(title),
                            )
                            .when(note.pinned, |line| {
                                line.child(icon(IconName::Pin, tokens.faint).size(px(12.)))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .mt(px(2.))
                            .text_size(tokens.text(12.5))
                            .text_color(tokens.muted)
                            .child(
                                div()
                                    .flex_none()
                                    .mr(px(6.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(tokens.fg.opacity(0.85))
                                    .child(date),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(summary)),
                    )
                    .when_some(folder, |column, folder| {
                        column.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .mt(px(3.))
                                .text_size(tokens.text(11.5))
                                .text_color(tokens.faint)
                                .child(icon(folder_icon, tokens.faint).size(px(11.)))
                                .child(folder),
                        )
                    }),
            )
            .when_some(cover, |row, image| {
                let place = div()
                    .flex_none()
                    .size(px(44.))
                    .self_center()
                    .rounded(px(8.));
                row.child(match image {
                    Some(image) => place
                        .child(
                            img(image)
                                .size(px(44.))
                                .rounded(px(8.))
                                .object_fit(ObjectFit::Cover),
                        )
                        .into_any_element(),
                    None => place.bg(tokens.hover).into_any_element(),
                })
            });
        self.with_menu(row, cx)
    }

    /// A right click on a note outside the selection makes it the selection.
    fn select_for_menu(&mut self, id: &NoteId, cx: &mut Context<Self>) {
        if let Some(index) = self.position(id)
            && !self.is_selected(index)
        {
            self.select_only(index, cx);
        }
    }

    fn with_menu(&self, element: gpui::Stateful<Div>, cx: &mut Context<Self>) -> AnyElement {
        let gallery = cx.entity().downgrade();
        if self.scope == Scope::Trash {
            return element
                .context_menu(move |menu, _, _| super::trash::menu(menu, &gallery))
                .into_any_element();
        }
        let folders: Rc<Vec<String>> = Rc::new(
            self.folders
                .iter()
                .map(|folder| folder.path.clone())
                .collect(),
        );
        element
            .context_menu(move |menu, window, cx| note_menu(menu, &gallery, &folders, window, cx))
            .into_any_element()
    }

    /// The date the groups go by, also shown on the card.
    fn date_of(&self, note: &NoteMeta) -> jiff::Timestamp {
        if let Some(deleted) = self.trashed_date(note) {
            return deleted;
        }
        match self.sort() {
            SortOrder::Created => note.created,
            SortOrder::Modified | SortOrder::Title => note.modified,
        }
    }

    /// The cover of a note: `None` without one, `Some(None)` while it is decoded or if
    /// it cannot be.
    fn cover_of(
        &mut self,
        note: &NoteMeta,
        preview: Option<&Preview>,
        cx: &mut Context<Self>,
    ) -> Option<Option<Arc<RenderImage>>> {
        let source = preview?.cover.as_deref()?;
        let path = self.cover_path(note, source, cx)?;
        Some(self.cover(&path, cx))
    }

    /// Where the image a note links to is on disk; `None` for a remote image.
    fn cover_path(&self, note: &NoteMeta, source: &str, cx: &Context<Self>) -> Option<PathBuf> {
        if source.contains("://") {
            return None;
        }
        let source = Path::new(source);
        Some(if source.is_absolute() {
            source.to_owned()
        } else {
            self.library
                .read(cx)
                .notes_dir()
                .join(&note.folder)
                .join(source)
        })
    }
}

/// Pin, move, duplicate and delete what is selected.
fn note_menu(
    menu: PopupMenu,
    gallery: &WeakEntity<GalleryView>,
    folders: &Rc<Vec<String>>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let pinned = gallery.upgrade().is_some_and(|gallery| {
        let selection = gallery.read(cx).selection();
        !selection.is_empty() && selection.iter().all(|note| note.pinned)
    });
    let act = |run: fn(&mut GalleryView, &mut Window, &mut Context<GalleryView>)| {
        let gallery = gallery.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut gpui::App| {
            let _ = gallery.update(cx, |gallery, cx| run(gallery, window, cx));
        }
    };
    let targets = folders.clone();
    let mover = gallery.clone();
    menu.item(
        PopupMenuItem::new(if pinned { tr("Unpin") } else { tr("Pin") })
            .on_click(act(|gallery, _, cx| gallery.toggle_pin(cx))),
    )
    .submenu(tr("Move To"), window, cx, move |menu, _, _| {
        targets.iter().fold(menu, |menu, folder| {
            let label = if folder.is_empty() {
                tr("Top Level")
            } else {
                folder.clone()
            };
            let (folder, mover) = (folder.clone(), mover.clone());
            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                let _ = mover.update(cx, |gallery, cx| gallery.move_selection_to(&folder, cx));
            }))
        })
    })
    .item(
        PopupMenuItem::new(tr("Duplicate"))
            .on_click(act(|gallery, _, cx| gallery.duplicate_selection(cx))),
    )
    .separator()
    .item(
        PopupMenuItem::new(tr("Delete"))
            .on_click(act(|gallery, _, cx| gallery.trash_selection(cx))),
    )
}

/// The miniature: tasks with small circles, headings bolder, tags faint.
fn miniature(preview: &Arc<Preview>, tokens: &Tokens) -> Vec<AnyElement> {
    preview
        .lines
        .iter()
        .map(|line| match line {
            Line::Heading(text) => div()
                .text_size(tokens.text(13.))
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_color(tokens.fg)
                .child(text.clone())
                .into_any_element(),
            Line::Task { text, done } => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_none()
                        .size(px(9.))
                        .rounded_full()
                        .when(*done, |dot| dot.bg(tokens.accent))
                        .when(!*done, |dot| dot.border(px(1.3)).border_color(tokens.faint)),
                )
                .child(div().min_w_0().child(text.clone()))
                .into_any_element(),
            Line::Tags(text) => div()
                .text_color(tokens.faint)
                .child(text.clone())
                .into_any_element(),
            Line::Text(text) => div().child(text.clone()).into_any_element(),
        })
        .collect()
}

/// `text` with the matched ranges in the text colour on a tint of the accent.
fn highlighted(text: &str, ranges: &[Range<usize>], tokens: &Tokens) -> StyledText {
    let style = HighlightStyle {
        color: Some(tokens.fg),
        background_color: Some(tokens.accent.opacity(0.3)),
        ..HighlightStyle::default()
    };
    let highlights = ranges
        .iter()
        .filter(|range| {
            range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .map(|range| (range.clone(), style))
        .collect::<Vec<_>>();
    StyledText::new(SharedString::from(text.to_owned())).with_highlights(highlights)
}

fn title_of(note: &NoteMeta) -> String {
    if note.title.is_empty() {
        tr("Untitled")
    } else {
        note.title.clone()
    }
}

/// `.gsec`: 13 px/700 faint, inset 26 px into the grid.
fn grid_heading(group: Group, tokens: &Tokens) -> impl IntoElement {
    div()
        .pl(px(26.))
        .pt(px(14.))
        .pb(px(2.))
        .text_size(tokens.text(13.))
        .font_weight(FontWeight::BOLD)
        .text_color(tokens.faint)
        .child(group.label())
}

/// `.lhead` of the list: 12 px/700 faint, with a pin for the pinned notes.
fn list_heading(group: Group, tokens: &Tokens) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .w_full()
        .max_w(px(LIST_MAX_WIDTH))
        .px(px(20.))
        .pt(px(12.))
        .pb(px(4.))
        .text_size(tokens.text(12.))
        .font_weight(FontWeight::BOLD)
        .text_color(tokens.faint)
        .when(group == Group::Pinned, |heading| {
            heading.child(icon(IconName::Pin, tokens.faint).size(px(12.)))
        })
        .child(group.label())
}
