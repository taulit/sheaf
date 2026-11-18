//! `/vorlage` (plan 03 §9): the slash menu's "Template" entry lists the templates of
//! `.sheaf/templates/` at the caret. The chosen one is filled in as for a new note
//! (`{{title}}` is this note's title) and inserted at the caret without its
//! frontmatter, so in an empty note it gives the note "+ ▾" would have made.
//!
//! The editor's slash entries are fixed when it is created and the templates are not,
//! so there is one entry, and the choice is a menu of Sheaf's own.

use gpui::{
    AnyElement, App, Bounds, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, anchored, deferred, div,
    point, prelude::FluentBuilder as _, px,
};
use gpui_markdown_editor::{Command, MarkdownEditor, SlashItem};
use sheaf_core::i18n::tr;

use super::{NoteEvent, NoteView, files, title};
use crate::app::actions::{Back, OpenSelected, SelectNext, SelectPrevious, TEMPLATE_MENU};
use crate::app::assets::IconName;
use crate::app::overlays::{self, MenuRow, Toast};

/// The menu of templates, open at the caret.
#[derive(Debug)]
pub struct TemplateMenu {
    at: Point<Pixels>,
    names: Vec<String>,
    /// The highlighted row; one past the templates is "Edit Templates…".
    selected: usize,
    focus: FocusHandle,
}

impl TemplateMenu {
    pub fn names(&self) -> &[String] {
        &self.names
    }
}

/// The slash menu's entry, which opens the menu for `note`.
pub fn slash_item(note: WeakEntity<NoteView>) -> SlashItem {
    SlashItem::new("template", tr("Template"), move |editor, window, cx| {
        let at = caret(editor, window, cx);
        let _ = note.update(cx, |note, cx| note.list_templates(at, window, cx));
    })
    .description(tr("Insert a note template"))
    .keywords(["template", "vorlage"])
}

/// Under the caret, in window coordinates.
fn caret(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) -> Point<Pixels> {
    let range = editor
        .selected_text_range(false, window, cx)
        .map(|selection| selection.range);
    range
        .and_then(|range| {
            let end = range.end..range.end;
            editor.bounds_for_range(end, Bounds::default(), window, cx)
        })
        .map_or_else(|| point(px(48.), px(120.)), |bounds| bounds.bottom_left())
}

impl NoteView {
    /// Opens the menu of templates at `at`, once they are listed.
    pub fn list_templates(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = self.library.read(cx).templates(cx);
        self._templates = cx.spawn_in(window, async move |this, cx| {
            let names = names.await.unwrap_or_else(|err| {
                log::warn!("cannot list the templates: {err}");
                Vec::new()
            });
            let _ = this.update_in(cx, |this, window, cx| {
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                this.template_menu = Some(TemplateMenu {
                    at,
                    names,
                    selected: 0,
                    focus,
                });
                cx.notify();
            });
        });
    }

    pub fn template_menu(&self) -> Option<&TemplateMenu> {
        self.template_menu.as_ref()
    }

    /// Inserts the template `name` at the caret.
    pub fn insert_template(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_template_menu(window, cx);
        let title = title(self.editor.read(cx));
        let text = self.library.read(cx).template_text(name, &title, cx);
        self._templates = cx.spawn_in(window, async move |this, cx| {
            let text = text.await;
            let _ = this.update_in(cx, |this, window, cx| match text {
                Ok(text) if !this.closed => this.editor.update(cx, |editor, cx| {
                    editor.command(Command::InsertMarkdown(text), window, cx);
                }),
                Ok(_) => {}
                Err(err) => {
                    let message =
                        tr("Could not insert the template: {}").replace("{}", &err.to_string());
                    cx.emit(NoteEvent::Toast(Toast::new(message)));
                }
            });
        });
    }

    /// Closes the menu and gives the keyboard back to the text.
    pub fn close_template_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.template_menu.take().is_some() {
            self.editor.focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn move_template_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.template_menu {
            let rows = menu.names.len() + 1;
            menu.selected = if down {
                (menu.selected + 1) % rows
            } else {
                (menu.selected + rows - 1) % rows
            };
            cx.notify();
        }
    }

    fn choose_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.template_menu else {
            return;
        };
        match menu.names.get(menu.selected).cloned() {
            Some(name) => self.insert_template(&name, window, cx),
            None => self.edit_templates(window, cx),
        }
    }

    /// "Edit Templates…": the templates folder in the file manager.
    fn edit_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_template_menu(window, cx);
        let dir = self.library.read(cx).templates_dir(cx);
        cx.spawn(async move |this, cx| match dir.await {
            Ok(dir) => cx.update(|cx| cx.open_url(&files::file_uri(&dir))),
            Err(err) => {
                log::error!("cannot create the templates folder: {err}");
                let message =
                    tr("Could not open the templates: {}").replace("{}", &err.to_string());
                let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Toast(Toast::new(message))));
            }
        })
        .detach();
    }
}

/// The open `menu` of `note`, drawn above everything, with a layer around it that
/// closes it on a click elsewhere.
pub fn menu(note: &Entity<NoteView>, menu: Option<&TemplateMenu>, cx: &App) -> Option<AnyElement> {
    let menu = menu?;
    let row = |index: usize, label: String, icon: IconName| {
        let note = note.clone();
        MenuRow::new(("template", index), label)
            .icon(icon)
            .selected(index == menu.selected)
            .on_click(move |_, window, cx| {
                note.update(cx, |note, cx| {
                    if let Some(menu) = &mut note.template_menu {
                        menu.selected = index;
                    }
                    note.choose_template(window, cx);
                });
            })
    };
    let rows = menu
        .names
        .iter()
        .enumerate()
        .map(|(index, name)| row(index, name.clone(), IconName::Text));
    let edit = row(menu.names.len(), tr("Edit Templates…"), IconName::Folder);
    let surface = overlays::popover("template-menu", cx)
        .w(px(240.))
        .max_h(px(320.))
        .overflow_y_scroll()
        .track_focus(&menu.focus)
        .key_context(TEMPLATE_MENU)
        .on_action::<SelectNext>(on(note, |note, _, cx| {
            note.move_template_selection(true, cx)
        }))
        .on_action::<SelectPrevious>(on(note, |note, _, cx| {
            note.move_template_selection(false, cx)
        }))
        .on_action::<OpenSelected>(on(note, NoteView::choose_template))
        .on_action::<Back>(on(note, NoteView::close_template_menu))
        .children(rows)
        .when(!menu.names.is_empty(), |surface| {
            surface.child(overlays::separator(cx))
        })
        .child(edit);
    let dismiss = {
        let note = note.clone();
        div()
            .id("template-menu-dismiss")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                note.update(cx, |note, cx| note.close_template_menu(window, cx));
            })
    };
    Some(
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(dismiss)
                .child(anchored().position(menu.at).snap_to_window().child(surface)),
        )
        .with_priority(2)
        .into_any_element(),
    )
}

/// An action handler of the menu that runs `run` on `note`.
fn on<A: 'static>(
    note: &Entity<NoteView>,
    run: fn(&mut NoteView, &mut Window, &mut Context<NoteView>),
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let note = note.clone();
    move |_, window, cx| note.update(cx, |note, cx| run(note, window, cx))
}
