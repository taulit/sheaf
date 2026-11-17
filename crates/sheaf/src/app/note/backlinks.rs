//! "Linked from" (plan 03 §7): under the text, folded, the notes whose `[[wiki link]]`
//! names this note's title. A click opens one. Nothing is shown while no note links
//! here.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use sheaf_core::NoteMeta;
use sheaf_core::i18n::{ntr, tr};

use super::{NoteEvent, NoteView};
use crate::app::assets::{IconName, icon};
use crate::app::theme::{ActiveTokens as _, size};

impl NoteView {
    /// The notes that link to this one, most recently changed first.
    pub fn backlinks(&self) -> &[NoteMeta] {
        &self.backlinks
    }

    /// Whether "Linked from" is unfolded.
    pub fn backlinks_open(&self) -> bool {
        self.backlinks_open
    }

    pub fn toggle_backlinks(&mut self, cx: &mut Context<Self>) {
        self.backlinks_open = !self.backlinks_open;
        cx.notify();
    }

    /// Opens the `index`th note that links here.
    pub fn open_backlink(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(note) = self.backlinks.get(index) {
            cx.emit(NoteEvent::Open(note.id.clone()));
        }
    }

    /// Looks the backlinks up again for the title the note has now; with `force` also
    /// when the title did not change, as after a change to other notes.
    pub(super) fn refresh_backlinks(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.title.trim().is_empty() {
            self.backlinks.clear();
            self.backlinks_title.clear();
            return;
        }
        if !force && self.backlinks_title == self.title {
            return;
        }
        let title = self.title.trim().to_owned();
        self.backlinks_title = self.title.clone();
        let found = self.library.read(cx).backlinks(&title, cx);
        self._backlinks = cx.spawn(async move |this, cx| {
            let found = match found.await {
                Ok(found) => found,
                Err(err) => {
                    log::warn!("cannot look up the backlinks: {err}");
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                let own = this.id.clone();
                this.backlinks = found.into_iter().filter(|note| note.id != own).collect();
                cx.notify();
            });
        });
    }

    /// The folded line, and the notes under it once unfolded.
    pub(super) fn backlinks_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.backlinks.is_empty() || !self.loaded {
            return None;
        }
        let tokens = cx.tokens().clone();
        let count = self.backlinks.len();
        let label = if self.backlinks_open {
            tr("Linked from")
        } else {
            ntr(
                "{} note links here",
                "{} notes link here",
                u32::try_from(count).unwrap_or(u32::MAX),
            )
            .replace("{}", &count.to_string())
        };
        let hover = tokens.hover;
        let toggle = div()
            .id("backlinks-toggle")
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(28.))
            .px(px(8.))
            .rounded(px(8.))
            .text_size(tokens.text(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens.faint)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_backlinks(cx)))
            .child(if self.backlinks_open {
                icon(IconName::PanDown, tokens.faint).size(px(14.))
            } else {
                icon(IconName::Forward, tokens.faint).size(px(14.))
            })
            .child(label);
        let rows = self.backlinks_open.then(|| {
            div()
                .id("backlinks")
                .flex()
                .flex_col()
                .max_h(px(180.))
                .overflow_y_scroll()
                .children(self.backlinks.iter().enumerate().map(|(index, note)| {
                    let folder = note
                        .folder
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    div()
                        .id(("backlink", index))
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_backlink(index, cx)))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(tokens.text(14.))
                                .text_color(tokens.accent)
                                .child(note.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(tokens.text(12.))
                                .text_color(tokens.faint)
                                .child(folder),
                        )
                }))
        });
        Some(
            div()
                .flex()
                .flex_none()
                .justify_center()
                .px(px(40.))
                .pb(px(30.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .max_w(size::EDITOR_MAX_WIDTH)
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(tokens.hair)
                        .child(div().flex().child(toggle))
                        .children(rows),
                )
                .into_any_element(),
        )
    }
}
