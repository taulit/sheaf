//! Tags in the folder popover, with the number of their notes, and pinning them to the
//! chip row (plan 03 §6: "Hauptordner, angeheftete Tags und intelligente Ordner").
//! Pinned tags are `pinned_tags` in `.sheaf/config.toml`. A tag row pins or unpins
//! with its pin or its context menu; a pinned tag's chip unpins with its context menu.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Task, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use sheaf_core::i18n::tr;

use super::{GalleryView, Scope};
use crate::app::assets::{IconName, icon};
use crate::app::theme::Tokens;

/// The tags in use, read when the folder popover opens.
#[derive(Default)]
pub(super) struct TagsState {
    listed: Vec<(String, usize)>,
    loading: Option<Task<()>>,
}

/// Whether `a` and `b` are one tag: tags differ only in case for the search too.
fn same_tag(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl GalleryView {
    /// The tags in use with their number of notes, as the folder popover lists them.
    pub fn tags(&self) -> &[(String, usize)] {
        &self.tags.listed
    }

    pub fn pinned_tags(&self) -> &[String] {
        self.config
            .as_ref()
            .map(|config| config.pinned_tags.as_slice())
            .unwrap_or_default()
    }

    pub fn is_tag_pinned(&self, tag: &str) -> bool {
        self.pinned_tags()
            .iter()
            .any(|pinned| same_tag(pinned, tag))
    }

    /// Pins `tag` to the chip row, or takes it off.
    pub fn set_tag_pinned(&mut self, tag: &str, pinned: bool, cx: &mut Context<Self>) {
        if self.is_tag_pinned(tag) == pinned {
            return;
        }
        let tag = tag.to_owned();
        self.change_config(
            move |config| {
                if pinned {
                    config.pinned_tags.push(tag.clone());
                } else {
                    config.pinned_tags.retain(|other| !same_tag(other, &tag));
                }
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn load_tags(&mut self, cx: &mut Context<Self>) {
        let tags = self.library.read(cx).tags(cx);
        self.tags.loading = Some(cx.spawn(async move |this, cx| match tags.await {
            Ok(tags) => {
                let _ = this.update(cx, |this, cx| {
                    this.tags.listed = tags;
                    cx.notify();
                });
            }
            Err(err) => log::warn!("cannot list the tags: {err}"),
        }));
    }

    /// The rows of the tags in the folder popover, under a heading; none without tags.
    pub(super) fn render_tag_rows(
        &self,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.tags.listed.is_empty() {
            return Vec::new();
        }
        let gallery = cx.entity().downgrade();
        let heading = div()
            .px(px(12.))
            .pt(px(6.))
            .pb(px(4.))
            .text_size(tokens.text(12.))
            .font_weight(FontWeight::BOLD)
            .text_color(tokens.faint)
            .child(tr("Tags"))
            .into_any_element();
        let rows = self
            .tags
            .listed
            .iter()
            .enumerate()
            .map(|(index, (tag, notes))| {
                let scope = Scope::Tag(tag.clone());
                let on = self.scope == scope;
                let pinned = self.is_tag_pinned(tag);
                let toggle = {
                    let tag = tag.clone();
                    let selector = format!("tag-pin-{tag}");
                    div()
                        .id(("tag-pin", index))
                        .debug_selector(move || selector)
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|style| style.bg(tokens.selected))
                        .when(!pinned, |button| button.opacity(0.45))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_tag_pinned(&tag, !pinned, cx);
                        }))
                        .child(
                            icon(
                                IconName::Pin,
                                if pinned { tokens.accent } else { tokens.faint },
                            )
                            .size(px(14.)),
                        )
                };
                let menu_tag = tag.clone();
                let gallery = gallery.clone();
                div()
                    .id(("tag", index))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(11.))
                    .h(px(34.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .rounded(px(9.))
                    .text_size(tokens.text(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .hover(|style| style.bg(tokens.hover))
                    .when(on, |row| row.bg(tokens.selected))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope.clone(), cx)))
                    .child(
                        div()
                            .w(px(16.))
                            .text_center()
                            .text_color(tokens.muted)
                            .child("#"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(tag.clone())),
                    )
                    .child(
                        div()
                            .text_size(tokens.text(12.))
                            .text_color(tokens.faint)
                            .child(notes.to_string()),
                    )
                    .child(toggle)
                    .context_menu(move |menu, _, _| pin_menu(menu, &gallery, &menu_tag, pinned))
                    .into_any_element()
            });
        std::iter::once(heading).chain(rows).collect()
    }
}

/// "Pin to Chips" or "Unpin" for `tag`.
pub(super) fn pin_menu(
    menu: PopupMenu,
    gallery: &gpui::WeakEntity<GalleryView>,
    tag: &str,
    pinned: bool,
) -> PopupMenu {
    let (gallery, tag) = (gallery.clone(), tag.to_owned());
    let label = if pinned { tr("Unpin") } else { tr("Pin") };
    menu.item(
        PopupMenuItem::new(label).on_click(move |_: &ClickEvent, _, cx| {
            let _ = gallery.update(cx, |gallery, cx| gallery.set_tag_pinned(&tag, !pinned, cx));
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_one_whatever_their_case() {
        assert!(same_tag("Rezepte", "rezepte"));
        assert!(same_tag("Büro", "büro"));
        assert!(!same_tag("rezepte", "rezept"));
    }
}
