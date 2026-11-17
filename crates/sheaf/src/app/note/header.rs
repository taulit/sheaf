//! The parts of the note view around the editor: the header bar with its popovers
//! (share, menu, info, "Move to…") and the banner for changes made elsewhere and
//! failed saves.

use gpui::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, deferred, div,
    prelude::FluentBuilder as _, px,
};
use sheaf_core::i18n::tr;

use super::{Banner, NoteView, Popover, date};
use crate::app::actions::Back;
use crate::app::assets::{IconName, icon};
use crate::app::chrome::{FlatButton, HeaderBar};
use crate::app::library::Folder;
use crate::app::overlays::{self, MenuRow};
use crate::app::theme::{ActiveTokens as _, size};

/// Popovers are 250 wide (04 "Stil-Popover").
const POPOVER_WIDTH: f32 = 250.;

pub fn header(view: &NoteView, cx: &mut Context<NoteView>) -> impl IntoElement {
    let tokens = cx.tokens().clone();
    let pinned = view.meta.as_ref().is_some_and(|meta| meta.pinned);
    let folder = view
        .meta
        .as_ref()
        .and_then(|meta| meta.folder.rsplit('/').next())
        .unwrap_or_default()
        .to_owned();
    let share = (view.popover == Some(Popover::Share)).then(|| share_popover(cx));
    let menu = view.popover.as_ref().and_then(|popover| match popover {
        Popover::Menu => Some(menu_popover(cx)),
        Popover::Info => Some(info_popover(view, cx)),
        Popover::Move(folders) => Some(move_popover(view, folders, cx)),
        Popover::Share => None,
    });
    let toggle = |popover: Popover| {
        cx.listener(move |this: &mut NoteView, _: &gpui::ClickEvent, _, cx| {
            let next = (this.popover.as_ref() != Some(&popover)).then(|| popover.clone());
            this.set_popover(next, cx);
        })
    };

    HeaderBar::new()
        .start(back_button(cx))
        .title(folder)
        .end(
            FlatButton::new("pin", IconName::Pin)
                .tooltip(if pinned { tr("Unpin") } else { tr("Pin") })
                .active(pinned)
                .icon_color(if pinned { tokens.accent } else { tokens.fg })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_pin(cx))),
        )
        .end(anchor(
            FlatButton::new("share", IconName::Share)
                .tooltip(tr("Share"))
                .active(share.is_some())
                .on_click(toggle(Popover::Share)),
            share,
        ))
        .end(anchor(
            FlatButton::new("more", IconName::More)
                .tooltip(tr("More"))
                .active(menu.is_some())
                .on_click(toggle(Popover::Menu)),
            menu,
        ))
}

/// `‹ Notes`: back to the gallery.
fn back_button(cx: &App) -> impl IntoElement {
    let tokens = cx.tokens();
    let hover = tokens.hover;
    div()
        .id("back")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .h(size::FLAT_BUTTON)
        .pl(px(6.))
        .pr(px(10.))
        .rounded(size::FLAT_BUTTON_RADIUS)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, window, cx| window.dispatch_action(Box::new(Back), cx))
        .child(icon(IconName::Back, tokens.fg))
        .child(
            div()
                .text_size(tokens.text(14.))
                .font_weight(FontWeight::MEDIUM)
                .child(tr("Notes")),
        )
}

/// A header button with its popover hanging from its right edge, drawn over
/// everything else.
fn anchor(button: FlatButton, popover: Option<AnyElement>) -> impl IntoElement {
    div()
        .relative()
        .child(button)
        .when_some(popover, |anchor, popover| {
            anchor.child(
                deferred(
                    div()
                        .absolute()
                        .top(size::FLAT_BUTTON + px(6.))
                        .right_0()
                        .w(px(POPOVER_WIDTH))
                        .child(popover),
                )
                .with_priority(2),
            )
        })
}

/// Catches a click beside an open popover, which closes it without doing anything
/// else.
pub fn dismiss_layer(cx: &mut Context<NoteView>) -> impl IntoElement {
    deferred(
        div()
            .id("popover-dismiss")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_popover(None, cx)),
            ),
    )
    .with_priority(1)
}

fn share_popover(cx: &mut Context<NoteView>) -> AnyElement {
    overlays::popover("share-popover", cx)
        .child(
            MenuRow::new("copy-markdown", tr("Copy as Markdown"))
                .icon(IconName::Copy)
                .on_click(cx.listener(|this, _, _, cx| this.copy_markdown(cx))),
        )
        .child(
            MenuRow::new("export-pdf", tr("Export as PDF…"))
                .icon(IconName::SaveAs)
                .disabled(true),
        )
        .child(
            MenuRow::new("print", tr("Print…"))
                .icon(IconName::Print)
                .disabled(true),
        )
        .into_any_element()
}

fn menu_popover(cx: &mut Context<NoteView>) -> AnyElement {
    let page = |popover: Popover| {
        cx.listener(move |this, _, _, cx| this.set_popover(Some(popover.clone()), cx))
    };
    overlays::popover("menu-popover", cx)
        .child(
            MenuRow::new("info", tr("Info"))
                .icon(IconName::Edit)
                .on_click(page(Popover::Info)),
        )
        .child(
            MenuRow::new("show-in-files", tr("Show in Files"))
                .icon(IconName::Folder)
                .on_click(cx.listener(|this, _, _, cx| this.show_in_files(cx))),
        )
        .child(
            MenuRow::new("move", tr("Move to…"))
                .icon(IconName::Forward)
                .on_click(page(Popover::Move(Vec::new()))),
        )
        .child(overlays::separator(cx))
        .child(
            MenuRow::new("delete", tr("Delete"))
                .icon(IconName::Trash)
                .on_click(cx.listener(|this, _, _, cx| this.trash(cx))),
        )
        .into_any_element()
}

/// The first row of a page of the menu: back to the menu.
fn page_title(title: String, cx: &mut Context<NoteView>) -> MenuRow {
    MenuRow::new("menu-back", title)
        .icon(IconName::Back)
        .on_click(cx.listener(|this, _, _, cx| this.set_popover(Some(Popover::Menu), cx)))
}

fn info_popover(view: &NoteView, cx: &mut Context<NoteView>) -> AnyElement {
    let tokens = cx.tokens().clone();
    let words = view.editor.read(cx).word_count();
    let zone = view.library.read(cx).now().time_zone().clone();
    let (created, modified, path) = match &view.meta {
        Some(meta) => (
            date::long_date_time(&meta.created.to_zoned_datetime(zone.clone())),
            date::long_date_time(&meta.modified.to_zoned_datetime(zone)),
            view.notes_dir.join(&meta.rel_path).display().to_string(),
        ),
        None => Default::default(),
    };
    let line = |label: String, value: String| {
        div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(px(12.))
            .py(px(6.))
            .child(
                div()
                    .text_size(tokens.text(12.))
                    .text_color(tokens.muted)
                    .child(label),
            )
            .child(div().text_size(tokens.text(13.5)).child(value))
    };
    let words = sheaf_core::i18n::ntr("{} word", "{} words", words.try_into().unwrap_or(u32::MAX))
        .replace("{}", &words.to_string());
    overlays::popover("info-popover", cx)
        .child(page_title(tr("Info"), cx))
        .child(overlays::separator(cx))
        .child(line(tr("Words"), words))
        .child(line(tr("Created"), created))
        .child(line(tr("Modified"), modified))
        .child(line(tr("Path"), path))
        .into_any_element()
}

fn move_popover(view: &NoteView, folders: &[Folder], cx: &mut Context<NoteView>) -> AnyElement {
    let current = view.folder();
    let rows: Vec<MenuRow> = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| {
            let label = if folder.path.is_empty() {
                tr("Notes")
            } else {
                folder.path.clone()
            };
            let here = folder.path == current;
            let path = folder.path.clone();
            let row = MenuRow::new(("folder", index), label)
                .icon(if here {
                    IconName::Check
                } else {
                    IconName::Folder
                })
                .disabled(here);
            row.on_click(cx.listener(move |this, _, _, cx| this.move_to(&path, cx)))
        })
        .collect();
    overlays::popover("move-popover", cx)
        .child(page_title(tr("Move to…"), cx))
        .child(overlays::separator(cx))
        .child(
            div()
                .id("move-folders")
                .flex()
                .flex_col()
                .max_h(px(320.))
                .overflow_y_scroll()
                .children(rows),
        )
        .into_any_element()
}

/// The question about a change made elsewhere, or a failed save, under the header bar.
pub fn banner(banner: Banner, cx: &mut Context<NoteView>) -> impl IntoElement {
    let tokens = cx.tokens().clone();
    let (message, buttons): (String, Vec<AnyElement>) = match banner {
        Banner::ChangedOnDisk => (
            tr("File changed on disk"),
            vec![
                pill("reload", tr("Reload"), false, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_from_disk(cx)))
                    .into_any_element(),
                pill("keep-mine", tr("Keep Mine"), true, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.keep_mine(cx)))
                    .into_any_element(),
            ],
        ),
        Banner::DeletedOnDisk => (
            tr("The note was deleted elsewhere"),
            vec![
                pill("save-as-new", tr("Save as New Note"), true, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.save_as_new(cx)))
                    .into_any_element(),
            ],
        ),
        Banner::SaveFailed(reason) => (
            tr("Could not save: {}").replace("{}", &reason),
            vec![
                pill("try-again", tr("Try Again"), true, cx)
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
