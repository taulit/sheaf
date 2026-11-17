//! The `+ ▾` menu of the header bar (plan 03 §9): New Note, the templates of
//! `.sheaf/templates/`, and "Edit Templates…", which opens that folder in the file
//! manager. The window creates the note, on [`GalleryEvent::NewFromTemplate`].

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Task, px,
};
use sheaf_core::i18n::tr;

use super::render::{menu_row, separator, surface};
use super::{GalleryEvent, GalleryView, Popover};
use crate::app::actions::NewNote;
use crate::app::assets::IconName;
use crate::app::note::files;
use crate::app::theme::Tokens;

#[derive(Default)]
pub(super) struct TemplatesState {
    names: Vec<String>,
    loading: Option<Task<()>>,
}

impl GalleryView {
    /// The templates of the menu, as last read.
    pub fn templates(&self) -> &[String] {
        &self.templates.names
    }

    /// Reads the templates again, for the menu that opens.
    pub(super) fn load_templates(&mut self, cx: &mut Context<Self>) {
        let names = self.library.read(cx).templates(cx);
        self.templates.loading = Some(cx.spawn(async move |this, cx| {
            let names = match names.await {
                Ok(names) => names,
                Err(err) => {
                    log::warn!("cannot list the templates: {err}");
                    Vec::new()
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.templates.names = names;
                cx.notify();
            });
        }));
    }

    /// Asks the window for a new note from the template `name`.
    pub fn new_from_template(&mut self, name: &str, cx: &mut Context<Self>) {
        self.set_popover(None, cx);
        cx.emit(GalleryEvent::NewFromTemplate(name.to_owned()));
    }

    /// Opens the templates folder in the file manager, creating it first if needed.
    pub fn edit_templates(&mut self, cx: &mut Context<Self>) {
        self.set_popover(None, cx);
        let dir = self.library.read(cx).templates_dir(cx);
        cx.spawn(async move |this, cx| match dir.await {
            Ok(dir) => cx.update(|cx| cx.open_url(&files::file_uri(&dir))),
            Err(err) => {
                log::error!("cannot create the templates folder: {err}");
                let message =
                    tr("Could not open the templates: {}").replace("{}", &err.to_string());
                let _ = this.update(cx, |_, cx| cx.emit(GalleryEvent::Failed(message)));
            }
        })
        .detach();
    }

    pub(super) fn render_templates_menu(
        &self,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let templates = self
            .templates
            .names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let template = name.clone();
                menu_row(("template", index), IconName::Text, name.clone(), tokens).on_click(
                    cx.listener(move |this, _, _, cx| this.new_from_template(&template, cx)),
                )
            });
        surface(tokens)
            .id("templates-popover")
            .w(px(240.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .child(
                menu_row("template-none", IconName::Edit, tr("New Note"), tokens).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.set_popover(None, cx);
                        window.dispatch_action(Box::new(NewNote), cx);
                    }),
                ),
            )
            .children(templates)
            .child(separator(tokens))
            .child(
                menu_row(
                    "edit-templates",
                    IconName::Folder,
                    tr("Edit Templates…"),
                    tokens,
                )
                .on_click(cx.listener(|this, _, _, cx| this.edit_templates(cx))),
            )
    }

    /// Opens the menu, as the palette's "New Note from Template…" does.
    pub(super) fn show_templates(&mut self, cx: &mut Context<Self>) {
        self.set_popover(Some(Popover::Templates), cx);
    }
}
