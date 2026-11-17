//! The toast: one short message at the bottom centre of the window, with an optional
//! action such as "Undo". A new toast replaces the one shown; each goes away on its
//! own after [`TIMEOUT`].

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnimationExt as _, App, Context, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, div, prelude::FluentBuilder as _, px,
};

use crate::app::theme::{ActiveTokens as _, motion};

/// How long a toast stays.
pub const TIMEOUT: Duration = Duration::from_secs(5);

type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A message for [`MainWindow::toast`](crate::app::window::MainWindow::toast).
#[derive(Clone)]
pub struct Toast {
    pub message: SharedString,
    action: Option<(SharedString, ActionHandler)>,
}

impl Toast {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Toast {
            message: message.into(),
            action: None,
        }
    }

    /// Adds a button, such as "Undo"; pressing it runs `run` and closes the toast.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(run)));
        self
    }

    pub fn action_label(&self) -> Option<&SharedString> {
        self.action.as_ref().map(|(label, _)| label)
    }
}

impl std::fmt::Debug for Toast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toast")
            .field("message", &self.message)
            .field("action", &self.action_label())
            .finish()
    }
}

/// Where the window's toast is drawn; the window owns one and lays it over everything.
#[derive(Default)]
pub struct ToastHost {
    shown: Option<Toast>,
    /// Counts the toasts, so that each one animates in afresh.
    generation: usize,
    _expiring: Option<Task<()>>,
}

impl std::fmt::Debug for ToastHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastHost")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl ToastHost {
    pub fn show(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.shown = Some(toast);
        self.generation += 1;
        self._expiring = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TIMEOUT).await;
            let _ = this.update(cx, |this, cx| this.dismiss(cx));
        }));
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.shown = None;
        self._expiring = None;
        cx.notify();
    }

    pub fn shown(&self) -> Option<&Toast> {
        self.shown.as_ref()
    }

    /// Presses the toast's button, as a click would.
    pub fn run_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, run)) = self.shown.as_ref().and_then(|toast| toast.action.clone()) {
            run(window, cx);
        }
        self.dismiss(cx);
    }
}

impl Render for ToastHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(toast) = self.shown.clone() else {
            return div().into_any_element();
        };
        let tokens = cx.tokens().clone();
        let surface = div()
            .id("toast")
            .occlude()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(42.))
            .pl(px(18.))
            .pr(px(if toast.action.is_some() { 6. } else { 18. }))
            .rounded_full()
            .bg(tokens.popover)
            .border_1()
            .border_color(tokens.hair)
            .shadow(super::popover_shadow())
            .text_size(tokens.text(14.))
            .text_color(tokens.fg)
            .child(toast.message.clone())
            .when_some(toast.action.clone(), |surface, (label, _)| {
                surface.child(
                    div()
                        .id("toast-action")
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .px(px(12.))
                        .rounded_full()
                        .cursor_pointer()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.accent)
                        .hover(|style| style.bg(tokens.hover))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| this.run_action(window, cx)))
                        .child(label),
                )
            });
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .pb(px(28.))
            .flex()
            .justify_center()
            .child(surface.with_animation(
                ("toast-in", self.generation),
                tokens.ease_out(motion::POPOVER),
                |surface, delta| surface.opacity(delta).mb(px(8. * (1. - delta))),
            ))
            .into_any_element()
    }
}
