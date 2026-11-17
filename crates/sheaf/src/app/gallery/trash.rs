//! "Recently Deleted" (plan 03 §9): the notes in the trash as cards or rows with the
//! date they were deleted and the days they have left, Restore and Delete
//! Permanently for the selection, Empty Trash, and a trashed note shown read-only
//! with a banner to restore it.
//!
//! The gallery shows it for [`Scope::Trash`]. Deleting for good asks first
//! ([`Confirm`]). Notes older than [`TRASH_MAX_AGE`] go for good whenever the trash
//! is listed ([`Library::trashed`](crate::app::library::Library::trashed)).

use std::collections::HashMap;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, WeakEntity, Window, div, hsla, px, rgb,
};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::text::TextView;
use jiff::{Timestamp, ZonedDateTime};
use sheaf_core::i18n::{ntr, tr};
use sheaf_core::{NoteId, NoteMeta, TRASH_MAX_AGE};

use super::dates::DateLabel;
use super::layout::Section;
use super::{GalleryEvent, GalleryView, Listing, Scope};
use crate::app::assets::IconName;
use crate::app::chrome::{FlatButton, HeaderBar, pane};
use crate::app::overlays;
use crate::app::theme::{ActiveTokens as _, Tokens, size};

/// GNOME's colour of destructive buttons.
const DESTRUCTIVE: u32 = 0xe01b24;

/// What deleting for good waits for the user to confirm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// These notes in the trash.
    Purge(Vec<NoteId>),
    /// Everything in the trash.
    Empty,
}

/// A note of the trash shown read-only.
struct TrashPage {
    meta: NoteMeta,
    deleted: Timestamp,
    body: SharedString,
}

/// What the gallery keeps about the trash.
#[derive(Default)]
pub(super) struct TrashState {
    /// When each listed note was deleted.
    deleted: HashMap<NoteId, Timestamp>,
    /// How many notes the trash holds, for the folder popover; `None` until counted.
    count: Option<usize>,
    page: Option<TrashPage>,
    confirm: Option<Confirm>,
    counting: Option<Task<()>>,
    opening: Option<Task<()>>,
}

/// The whole days a note deleted at `deleted` has left in the trash at `now`, rounded
/// up: a note deleted just now has 30.
pub fn days_left(deleted: Timestamp, now: Timestamp) -> i64 {
    const DAY: i64 = 24 * 60 * 60;
    let left = TRASH_MAX_AGE - now.duration_since(deleted);
    let seconds = left.as_secs() + i64::from(left.subsec_nanos() > 0);
    (seconds + DAY - 1).div_euclid(DAY).max(0)
}

fn days_left_label(days: i64) -> String {
    let days = u32::try_from(days).unwrap_or(0);
    ntr("{} day left", "{} days left", days).replace("{}", &days.to_string())
}

impl GalleryView {
    /// When `id` was deleted, while the trash is shown.
    pub fn deleted_at(&self, id: &NoteId) -> Option<Timestamp> {
        self.trash.deleted.get(id).copied()
    }

    /// How many notes are in the trash, once counted.
    pub fn trash_count(&self) -> Option<usize> {
        self.trash.count
    }

    /// The trashed note shown read-only.
    pub fn shown_trashed(&self) -> Option<&NoteMeta> {
        self.trash.page.as_ref().map(|page| &page.meta)
    }

    /// What deleting for good waits to have confirmed.
    pub fn confirming(&self) -> Option<&Confirm> {
        self.trash.confirm.as_ref()
    }

    /// Lists the trash in place of the notes, filtered by the words typed.
    pub(super) fn reload_trash(&mut self, generation: u64, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let (trashed, folders) = (library.trashed(cx), library.folders(cx));
        let words = self.text.trim().to_lowercase();
        self.loading = cx.spawn(async move |this, cx| {
            let (trashed, folders) = (trashed.await, folders.await);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match folders {
                    Ok(folders) => this.folders = folders,
                    Err(err) => log::warn!("cannot list the folders: {err}"),
                }
                let trashed = match trashed {
                    Ok(trashed) => trashed,
                    Err(err) => {
                        log::error!("cannot list the trash: {err}");
                        return;
                    }
                };
                this.trash.count = Some(trashed.len());
                this.trash.deleted = trashed
                    .iter()
                    .map(|trashed| (trashed.meta.id.clone(), trashed.deleted))
                    .collect();
                let notes: Vec<NoteMeta> = trashed
                    .into_iter()
                    .map(|trashed| trashed.meta)
                    .filter(|meta| words.is_empty() || meta.title.to_lowercase().contains(&words))
                    .collect();
                let sections = vec![Section {
                    group: None,
                    len: notes.len(),
                }];
                this.apply(
                    Listing {
                        notes,
                        sections,
                        marks: HashMap::new(),
                    },
                    cx,
                );
            });
        });
    }

    /// Counts the notes in the trash for the folder popover.
    pub(super) fn count_trash(&mut self, cx: &mut Context<Self>) {
        let trashed = self.library.read(cx).trashed(cx);
        self.trash.counting = Some(cx.spawn(async move |this, cx| {
            let Ok(trashed) = trashed.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.trash.count = Some(trashed.len());
                cx.notify();
            });
        }));
    }

    /// The date of deletion and the days left, under a card of the trash.
    pub(super) fn trash_label(&self, note: &NoteMeta, now: &ZonedDateTime) -> Option<String> {
        let deleted = self.trashed_date(note)?;
        let date = DateLabel::of(deleted, now).format();
        let left = days_left_label(days_left(deleted, now.timestamp()));
        Some(format!("{date} · {left}"))
    }

    /// The days left of a note in a row of the trash.
    pub(super) fn trash_days_left(&self, note: &NoteMeta, now: Timestamp) -> Option<String> {
        let deleted = self.trashed_date(note)?;
        Some(days_left_label(days_left(deleted, now)))
    }

    /// When a note shown in the trash was deleted; `None` outside the trash.
    pub(super) fn trashed_date(&self, note: &NoteMeta) -> Option<Timestamp> {
        if self.scope != Scope::Trash {
            return None;
        }
        self.trash.deleted.get(&note.id).copied()
    }

    /// Shows the trashed note at `index` read-only.
    pub(super) fn open_trashed(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(note) = self.notes.get(index).cloned() else {
            return;
        };
        self.select_only(index, cx);
        let deleted = self
            .trash
            .deleted
            .get(&note.id)
            .copied()
            .unwrap_or(note.modified);
        let loaded = self.library.read(cx).trashed_note(&note.id, cx);
        self.trash.opening = Some(cx.spawn(async move |this, cx| {
            let loaded = loaded.await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok(loaded) => {
                    this.trash.page = Some(TrashPage {
                        meta: loaded.meta.clone(),
                        deleted,
                        body: loaded.body().to_owned().into(),
                    });
                    cx.notify();
                }
                Err(err) => {
                    log::error!("cannot read the note in the trash: {err}");
                    let message = tr("Could not open the note: {}").replace("{}", &err.to_string());
                    cx.emit(GalleryEvent::Failed(message));
                }
            });
        }));
    }

    pub fn close_trashed(&mut self, cx: &mut Context<Self>) {
        self.trash.page = None;
        cx.notify();
    }

    /// Puts the selected notes of the trash back where they came from.
    pub fn restore_selection(&mut self, cx: &mut Context<Self>) {
        let ids = self.selection_ids();
        self.restore(ids, cx);
    }

    /// Puts the note shown read-only back and closes it.
    pub fn restore_shown(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.trash.page.take() {
            self.restore(vec![page.meta.id], cx);
        }
        cx.notify();
    }

    fn restore(&mut self, ids: Vec<NoteId>, cx: &mut Context<Self>) {
        if self.scope != Scope::Trash || ids.is_empty() {
            return;
        }
        let restoring: Vec<_> = ids
            .iter()
            .map(|id| {
                self.library
                    .update(cx, |library, cx| library.restore(id, cx))
            })
            .collect();
        cx.spawn(async move |this, cx| {
            let mut restored = 0;
            let mut failed = None;
            for task in restoring {
                match task.await {
                    Ok(_) => restored += 1,
                    Err(err) => {
                        log::error!("cannot restore a note: {err}");
                        failed.get_or_insert(err.to_string());
                    }
                }
            }
            let event = match failed {
                Some(reason) => GalleryEvent::Failed(
                    tr("Could not restore the note: {}").replace("{}", &reason),
                ),
                None => {
                    let count = u32::try_from(restored).unwrap_or(u32::MAX);
                    let message = ntr("Note restored", "{} notes restored", count)
                        .replace("{}", &count.to_string());
                    GalleryEvent::Notice(message)
                }
            };
            let _ = this.update(cx, |_, cx| cx.emit(event));
        })
        .detach();
    }

    /// Asks before deleting the selected notes of the trash for good.
    pub fn ask_purge_selection(&mut self, cx: &mut Context<Self>) {
        let ids = self.selection_ids();
        if self.scope == Scope::Trash && !ids.is_empty() {
            self.trash.confirm = Some(Confirm::Purge(ids));
            cx.notify();
        }
    }

    /// Asks before deleting the note shown read-only for good.
    pub fn ask_purge_shown(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = &self.trash.page {
            self.trash.confirm = Some(Confirm::Purge(vec![page.meta.id.clone()]));
            cx.notify();
        }
    }

    /// Asks before emptying the trash.
    pub fn ask_empty_trash(&mut self, cx: &mut Context<Self>) {
        self.trash.confirm = Some(Confirm::Empty);
        cx.notify();
    }

    pub fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
        self.trash.confirm = None;
        cx.notify();
    }

    /// Does what was asked: deletes for good. A failure is a toast.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.trash.confirm.take() else {
            return;
        };
        let purging: Vec<Task<sheaf_core::Result<()>>> = match confirm {
            Confirm::Purge(ids) => {
                if self
                    .trash
                    .page
                    .as_ref()
                    .is_some_and(|page| ids.contains(&page.meta.id))
                {
                    self.trash.page = None;
                }
                ids.iter()
                    .map(|id| self.library.update(cx, |library, cx| library.purge(id, cx)))
                    .collect()
            }
            Confirm::Empty => {
                self.trash.page = None;
                let emptying = self
                    .library
                    .update(cx, |library, cx| library.empty_trash(cx));
                vec![cx.background_spawn(async move { emptying.await.map(|_| ()) })]
            }
        };
        cx.spawn(async move |this, cx| {
            for task in purging {
                if let Err(err) = task.await {
                    log::error!("cannot delete a note for good: {err}");
                    let message =
                        tr("Could not delete the note: {}").replace("{}", &err.to_string());
                    let _ = this.update(cx, |_, cx| cx.emit(GalleryEvent::Failed(message)));
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// The read-only note over the gallery, with the banner to restore it.
    pub(super) fn render_trash_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let page = self.trash.page.as_ref()?;
        let tokens = cx.tokens().clone();
        let left = days_left(page.deleted, self.library.read(cx).now().timestamp());
        let message = ntr(
            "Deleted for good in {} day",
            "Deleted for good in {} days",
            u32::try_from(left).unwrap_or(0),
        )
        .replace("{}", &left.to_string());
        let title = if page.meta.title.is_empty() {
            tr("Untitled")
        } else {
            page.meta.title.clone()
        };
        let banner = div()
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
            .child(icon_for_banner(&tokens))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(message),
            )
            .child(
                pill(
                    "trashed-purge",
                    tr("Delete Permanently"),
                    Kind::Plain,
                    &tokens,
                )
                .on_click(cx.listener(|this, _, _, cx| this.ask_purge_shown(cx))),
            )
            .child(
                pill("trashed-restore", tr("Restore"), Kind::Primary, &tokens)
                    .on_click(cx.listener(|this, _, _, cx| this.restore_shown(cx))),
            );
        let text = div()
            .id("trashed-text")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(size::EDITOR_MAX_WIDTH)
                    .mx_auto()
                    .px(px(48.))
                    .pt(px(18.))
                    .pb(px(120.))
                    .text_size(tokens.text(15.5))
                    .line_height(gpui::relative(1.6))
                    .child(TextView::markdown("trashed-note", page.body.clone()).selectable(true)),
            );
        let surface = pane(tokens.view, window, cx)
            .child(
                HeaderBar::new()
                    .start(
                        FlatButton::new("trashed-back", IconName::Back)
                            .tooltip(tr("Back"))
                            .on_click(cx.listener(|this, _, _, cx| this.close_trashed(cx))),
                    )
                    .title(title),
            )
            .child(banner)
            .child(text);
        Some(
            div()
                .id("trashed-page")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .child(surface)
                .into_any_element(),
        )
    }

    /// The question before deleting for good, over everything.
    pub(super) fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.trash.confirm.as_ref()?;
        let tokens = cx.tokens().clone();
        let (question, action) = match confirm {
            Confirm::Purge(ids) => {
                let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                (
                    ntr(
                        "Delete this note permanently?",
                        "Delete {} notes permanently?",
                        count,
                    )
                    .replace("{}", &count.to_string()),
                    tr("Delete Permanently"),
                )
            }
            Confirm::Empty => (
                tr("Delete all notes in the trash permanently?"),
                tr("Empty Trash"),
            ),
        };
        let sheet = overlays::popover("confirm-purge", cx)
            .w(px(340.))
            .p(px(18.))
            .gap(px(6.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(tokens.text(15.))
                    .font_weight(FontWeight::BOLD)
                    .child(question),
            )
            .child(
                div()
                    .text_size(tokens.text(13.5))
                    .text_color(tokens.muted)
                    .child(tr("This cannot be undone.")),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .mt(px(12.))
                    .child(
                        pill("confirm-cancel", tr("Cancel"), Kind::Plain, &tokens)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_confirm(cx))),
                    )
                    .child(
                        pill("confirm-delete", action, Kind::Destructive, &tokens)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                    ),
            );
        Some(
            div()
                .id("confirm-backdrop")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.3))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.cancel_confirm(cx)),
                )
                .child(sheet)
                .into_any_element(),
        )
    }
}

fn icon_for_banner(tokens: &Tokens) -> impl IntoElement {
    crate::app::assets::icon(IconName::Trash, tokens.muted).size(px(16.))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Plain,
    Primary,
    Destructive,
}

/// A button of 04 "Buttons": 28 high, 12.5 px bold.
pub(super) fn pill(
    id: &'static str,
    label: String,
    kind: Kind,
    tokens: &Tokens,
) -> gpui::Stateful<gpui::Div> {
    let (background, color) = match kind {
        Kind::Plain => (tokens.selected, tokens.fg),
        Kind::Primary => (tokens.accent, tokens.on_accent),
        Kind::Destructive => (rgb(DESTRUCTIVE).into(), tokens.on_accent),
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
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

/// Restore and Delete Permanently for what is selected in the trash.
pub(super) fn menu(menu: PopupMenu, gallery: &WeakEntity<GalleryView>) -> PopupMenu {
    let act = |run: fn(&mut GalleryView, &mut Context<GalleryView>)| {
        let gallery = gallery.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = gallery.update(cx, run);
        }
    };
    menu.item(
        PopupMenuItem::new(tr("Restore"))
            .on_click(act(|gallery, cx| gallery.restore_selection(cx))),
    )
    .separator()
    .item(
        PopupMenuItem::new(tr("Delete Permanently"))
            .on_click(act(|gallery, cx| gallery.ask_purge_selection(cx))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_left_count_down_from_thirty() {
        let deleted: Timestamp = "2026-11-16T12:00:00Z".parse().unwrap_or_default();
        let at = |hours: i64| deleted + jiff::SignedDuration::from_hours(hours);
        assert_eq!(days_left(deleted, at(0)), 30);
        assert_eq!(days_left(deleted, at(1)), 30);
        assert_eq!(days_left(deleted, at(24)), 29);
        assert_eq!(days_left(deleted, at(29 * 24 + 23)), 1);
        assert_eq!(days_left(deleted, at(30 * 24)), 0);
        assert_eq!(days_left(deleted, at(40 * 24)), 0);
    }
}
