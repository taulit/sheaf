//! The parts every view of the main window is built from: the pane that fills the
//! window, the header bar, its flat buttons and the close button. Sizes are those of
//! the prototype (`.hb`, `.fb`, `.close`).

use gpui::{
    AnyElement, App, ClickEvent, Corners, Decorations, Div, ElementId, Global, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseMoveEvent, ParentElement, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::window_corner_radii;
use sheaf_core::i18n::tr;

use super::actions::CloseWindow;
use super::assets::{IconName, icon};
use super::theme::{ActiveTokens as _, size};

/// Says that windows are client-decorated where the platform cannot: an offscreen
/// window has no compositor and reports server-side decorations, so the screenshot
/// harness sets this global to get the close button and the rounded corners a window
/// on a desktop has. The shadow and hairline around the frame are drawn by
/// gpui-component only for a real client-decorated window.
#[derive(Clone, Copy, Debug)]
pub struct AssumeClientDecorations;

impl Global for AssumeClientDecorations {}

fn client_decorated(window: &Window, cx: &App) -> bool {
    cx.has_global::<AssumeClientDecorations>()
        || matches!(window.window_decorations(), Decorations::Client { .. })
}

fn corner_radii(window: &Window, cx: &App) -> Corners<Pixels> {
    if cx.has_global::<AssumeClientDecorations>() {
        Corners::all(size::WINDOW_RADIUS)
    } else {
        window_corner_radii(window, cx)
    }
}

/// A column that fills the main window with `background` and follows its rounded
/// corners. Every route renders into one.
pub fn pane(background: Hsla, window: &Window, cx: &App) -> Div {
    let radii = corner_radii(window, cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(background)
        .text_color(cx.tokens().fg)
        .rounded_tl(radii.top_left)
        .rounded_tr(radii.top_right)
        .rounded_bl(radii.bottom_left)
        .rounded_br(radii.bottom_right)
        .overflow_hidden()
}

/// The 47 px bar at the top of a pane: `start` items, a centred title, `end` items and
/// the close button. It has no background of its own. Dragging it moves the window, a
/// double click maximises it, a right click opens the window menu.
#[derive(IntoElement, Default)]
pub struct HeaderBar {
    start: Vec<AnyElement>,
    title: Option<SharedString>,
    end: Vec<AnyElement>,
}

impl HeaderBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an item before the title.
    pub fn start(mut self, item: impl IntoElement) -> Self {
        self.start.push(item.into_any_element());
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds an item after the title, before the close button.
    pub fn end(mut self, item: impl IntoElement) -> Self {
        self.end.push(item.into_any_element());
        self
    }
}

impl std::fmt::Debug for HeaderBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderBar")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// Whether a press on the bar is still waiting to become a drag.
struct Drag {
    armed: bool,
}

impl RenderOnce for HeaderBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = cx.tokens();
        let title_size = tokens.text(14.);
        let client_decorated = client_decorated(window, cx);
        let drag = window.use_keyed_state("header-drag", cx, |_, _| Drag { armed: false });

        div()
            .id("header-bar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(size::HEADER_HEIGHT)
            .px(px(8.))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.zoom_window();
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&drag, |drag, _, _, _| drag.armed = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&drag, |drag, _, _, _| drag.armed = false),
            )
            .on_mouse_down_out(window.listener_for(&drag, |drag, _, _, _| drag.armed = false))
            // The window moves once the pointer does, so that a plain click stays one.
            .on_mouse_move(
                window.listener_for(&drag, |drag, event: &MouseMoveEvent, window, _| {
                    let pressed = event.pressed_button == Some(MouseButton::Left);
                    if drag.armed && pressed {
                        window.start_window_move();
                    }
                    drag.armed = false;
                }),
            )
            .when(client_decorated, |bar| {
                bar.on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position);
                })
            })
            .children(self.start)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_center()
                    .text_size(title_size)
                    .font_weight(gpui::FontWeight::BOLD)
                    .truncate()
                    .children(self.title),
            )
            .children(self.end)
            .when(client_decorated, |bar| bar.child(close_button(cx)))
    }
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A flat 34×34 button with a symbolic icon, as in the header bar.
#[derive(IntoElement)]
pub struct FlatButton {
    id: ElementId,
    icon: IconName,
    tooltip: Option<SharedString>,
    active: bool,
    on_click: Option<ClickHandler>,
}

impl FlatButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        FlatButton {
            id: id.into(),
            icon,
            tooltip: None,
            active: false,
            on_click: None,
        }
    }

    /// What the button does, shown on hover.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Draws the button as switched on, like the current view mode.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
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

impl std::fmt::Debug for FlatButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlatButton")
            .field("id", &self.id)
            .field("icon", &self.icon)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl RenderOnce for FlatButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = cx.tokens();
        let (hover, selected, fg) = (tokens.hover, tokens.selected, tokens.fg);
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::FLAT_BUTTON)
            .rounded(size::FLAT_BUTTON_RADIUS)
            .cursor_pointer()
            .when(self.active, |button| button.bg(selected))
            .when(!self.active, |button| button.hover(|style| style.bg(hover)))
            .active(|style| style.bg(selected))
            // A press on a button must not start dragging the window.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(self.on_click, |button, handler| {
                button.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.tooltip, |button, tooltip| {
                button.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .child(icon(self.icon, fg))
    }
}

/// The round 24 px button that closes the window.
fn close_button(cx: &App) -> impl IntoElement {
    let tokens = cx.tokens();
    let (selected, fg) = (tokens.selected, tokens.fg);
    let pressed = selected.opacity(1.6);
    div()
        .id("close-window")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::CLOSE_BUTTON)
        .mx(px(4.))
        .rounded_full()
        .bg(selected)
        .cursor_pointer()
        .hover(move |style| style.bg(pressed))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, window, cx| window.dispatch_action(Box::new(CloseWindow), cx))
        .tooltip(|window, cx| Tooltip::new(tr("Close")).build(window, cx))
        .child(icon(IconName::Close, fg).size(px(12.)))
}
