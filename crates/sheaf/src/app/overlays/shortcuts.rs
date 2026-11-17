//! The list of keyboard shortcuts (Ctrl+?): every binding of the keymap whose action
//! is named in [`actions::commands`], grouped by where it works. Esc or a click beside
//! it closes it.

use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use sheaf_core::i18n::tr;

use crate::app::actions;
use crate::app::assets::{IconName, icon};
use crate::app::theme::{ActiveTokens as _, size};

const WIDTH: f32 = 520.;

/// One group of the list: a heading and its lines of label and keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub title: String,
    pub lines: Vec<(String, String)>,
}

/// The groups of the list, from the commands and the keymap of `cx`.
pub fn groups(cx: &App) -> Vec<ShortcutGroup> {
    let contexts = [
        (actions::WINDOW, tr("General")),
        (actions::GALLERY, tr("Gallery")),
        (actions::NOTE, tr("Note")),
        (actions::QUICK_NOTE, tr("Quick Note")),
    ];
    let commands = actions::commands();
    let bound: Vec<(String, String, String)> = commands
        .iter()
        .flat_map(|command| {
            super::bindings_of(command.action.as_ref(), cx)
                .into_iter()
                .map(|(context, keys)| (context, command.label.clone(), keys))
        })
        .collect();
    contexts
        .into_iter()
        .map(|(context, title)| {
            let mut lines: Vec<(String, String)> = Vec::new();
            for (_, label, keys) in bound.iter().filter(|(of, _, _)| of == context) {
                // Two keys for one command share a line: "Ctrl+W, Esc".
                match lines.iter_mut().find(|(known, _)| known == label) {
                    Some((_, all)) => {
                        all.push_str(", ");
                        all.push_str(keys);
                    }
                    None => lines.push((label.clone(), keys.clone())),
                }
            }
            ShortcutGroup { title, lines }
        })
        .filter(|group| !group.lines.is_empty())
        .collect()
}

pub struct ShortcutsSheet {
    focus_handle: FocusHandle,
}

impl ShortcutsSheet {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        ShortcutsSheet { focus_handle }
    }
}

impl std::fmt::Debug for ShortcutsSheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutsSheet").finish_non_exhaustive()
    }
}

impl EventEmitter<DismissEvent> for ShortcutsSheet {}

impl Focusable for ShortcutsSheet {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ShortcutsSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let body = groups(cx).into_iter().map(|group| {
            let lines = group.lines.into_iter().map(|(label, keys)| {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .px(px(12.))
                    .py(px(5.))
                    .text_size(tokens.text(13.5))
                    .border_t_1()
                    .border_color(tokens.hair)
                    .child(div().min_w_0().truncate().child(label))
                    .child(
                        div()
                            .flex_none()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .bg(tokens.hover)
                            .text_size(tokens.text(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens.muted)
                            .child(keys),
                    )
            });
            div()
                .flex()
                .flex_col()
                .child(super::heading(group.title, cx))
                .children(lines)
        });
        let header = div()
            .flex()
            .items_center()
            .px(px(12.))
            .pt(px(6.))
            .pb(px(4.))
            .child(
                div()
                    .flex_1()
                    .text_size(tokens.text(15.))
                    .font_weight(FontWeight::BOLD)
                    .child(tr("Keyboard Shortcuts")),
            )
            .child(
                div()
                    .id("shortcuts-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(size::CLOSE_BUTTON)
                    .rounded_full()
                    .bg(tokens.selected)
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
                    .child(icon(IconName::Close, tokens.fg).size(px(12.))),
            );

        div()
            .id("shortcuts-scrim")
            .track_focus(&self.focus_handle)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(gpui::black().opacity(0.18))
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(DismissEvent);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(DismissEvent)),
            )
            .child(
                super::popover("shortcuts", cx)
                    .w(px(WIDTH))
                    .max_w(gpui::relative(0.9))
                    .max_h(gpui::relative(0.85))
                    .overflow_y_scroll()
                    .p(px(10.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(header)
                    .children(body),
            )
    }
}
