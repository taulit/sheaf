//! What the window lays over its views: the [`toast`], the command [`palette`] and the
//! list of keyboard [`shortcuts`], plus the popover surface and menu rows views build
//! their own popovers from.
//!
//! # For views
//!
//! - **Toast:** emit an event the window turns into
//!   [`MainWindow::toast`](crate::app::window::MainWindow::toast) with a [`Toast`];
//!   the note view does it with `NoteEvent::Toast`. Deletions only report what went
//!   to the trash (`NoteEvent::Trashed`, `GalleryEvent::Trashed`); the window builds
//!   the one Undo toast for both.
//! - **Palette and shortcuts** take their entries from
//!   [`actions::commands`](crate::app::actions::commands) and the keymap; a new action
//!   shows up in both once it is named there.

pub mod palette;
pub mod shortcuts;
pub mod toast;

use gpui::{
    App, BoxShadow, ClickEvent, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, hsla, point, prelude::FluentBuilder as _, px,
};
use sheaf_core::i18n::tr;

use super::assets::{IconName, icon};
use super::theme::{ActiveTokens as _, size};

pub use toast::{Toast, ToastHost};

/// The shadow of popovers, the palette and toasts: `0 14px 40px -8px rgba(0,0,0,.55)`.
pub fn popover_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(0., 0., 0., 0.55),
        offset: point(px(0.), px(14.)),
        blur_radius: px(40.),
        spread_radius: px(-8.),
        inset: false,
    }]
}

/// The surface of a popover (04 "Stil-Popover"): popover colour, radius 14, a hairline
/// and a shadow. It takes the clicks inside it.
pub fn popover(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let tokens = cx.tokens();
    div()
        .id(id)
        .occlude()
        .flex()
        .flex_col()
        .p(px(6.))
        .rounded(size::POPOVER_RADIUS)
        .bg(tokens.popover)
        .border_1()
        .border_color(tokens.hair)
        .shadow(popover_shadow())
        .text_color(tokens.fg)
        .text_size(tokens.text(14.))
}

type RowHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A row of a popover menu: rows 8 12, radius 9, hover surface.
#[derive(IntoElement)]
pub struct MenuRow {
    id: ElementId,
    label: String,
    icon: Option<IconName>,
    trailing: Option<String>,
    disabled: bool,
    on_click: Option<RowHandler>,
}

impl MenuRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<String>) -> Self {
        MenuRow {
            id: id.into(),
            label: label.into(),
            icon: None,
            trailing: None,
            disabled: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Quiet text at the end of the row, such as a shortcut or a count.
    pub fn trailing(mut self, text: impl Into<String>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    /// Shown, but not yet available.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl std::fmt::Debug for MenuRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuRow")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl gpui::RenderOnce for MenuRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = cx.tokens();
        let (hover, muted, faint, fg) = (tokens.hover, tokens.muted, tokens.faint, tokens.fg);
        let color = if self.disabled { faint } else { fg };
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(9.))
            .text_color(color)
            .when(!self.disabled, |row| {
                row.cursor_pointer().hover(move |style| style.bg(hover))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(self.icon, |row, name| {
                row.child(icon(name, color).size(px(16.)))
            })
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .when_some(self.trailing, |row, text| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(tokens.text(12.5))
                        .text_color(muted)
                        .child(text),
                )
            })
            .when_some(self.on_click.filter(|_| !self.disabled), |row, handler| {
                row.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// A thin line between groups of menu rows.
pub fn separator(cx: &App) -> Div {
    div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(cx.tokens().hair)
}

/// A quiet heading over a group of rows or table lines.
pub fn heading(text: impl Into<String>, cx: &App) -> Div {
    let tokens = cx.tokens();
    div()
        .px(px(12.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(tokens.text(12.))
        .font_weight(FontWeight::BOLD)
        .text_color(tokens.faint)
        .child(text.into())
}

/// A key binding as GNOME writes it in the user's language: "Ctrl+Shift+N", "Esc".
pub fn format_keys(binding: &gpui::KeyBinding) -> String {
    binding
        .keystrokes()
        .iter()
        .map(|stroke| {
            let modifiers = stroke.modifiers();
            let mut parts = Vec::new();
            if modifiers.control {
                parts.push(tr("Ctrl"));
            }
            if modifiers.alt {
                parts.push(tr("Alt"));
            }
            if modifiers.shift {
                parts.push(tr("Shift"));
            }
            if modifiers.platform {
                parts.push(tr("Super"));
            }
            parts.push(key_name(stroke.key()));
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn key_name(key: &str) -> String {
    match key {
        "escape" => tr("Esc"),
        "delete" => tr("Del"),
        "backspace" => tr("Backspace"),
        "enter" => tr("Enter"),
        "space" => tr("Space"),
        "tab" => tr("Tab"),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        key if key.chars().count() == 1 => key.to_uppercase(),
        key => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

/// The bindings of `action` with their key context, in the order they were added.
pub fn bindings_of(action: &dyn gpui::Action, cx: &App) -> Vec<(String, String)> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings_for_action(action)
        .map(|binding| {
            let context = binding
                .predicate()
                .map(|predicate| predicate.to_string())
                .unwrap_or_default();
            (context, format_keys(binding))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_like_gnome_writes_them() {
        let binding = gpui::KeyBinding::new("ctrl-shift-n", crate::app::actions::QuickNote, None);
        assert_eq!(format_keys(&binding), "Ctrl+Shift+N");
        let binding = gpui::KeyBinding::new("escape", crate::app::actions::Back, None);
        assert_eq!(format_keys(&binding), "Esc");
        let binding = gpui::KeyBinding::new("ctrl-,", crate::app::actions::OpenSettings, None);
        assert_eq!(format_keys(&binding), "Ctrl+,");
    }
}
