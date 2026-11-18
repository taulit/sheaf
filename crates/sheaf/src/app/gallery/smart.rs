//! Smart folders (plan 03 §9): saved searches with a name and a symbol, kept in the
//! folder settings (`smart_folders` in `.sheaf/config.toml`), listed in the folder
//! popover and the chip row. Choosing one shows what its search finds today; what is
//! typed narrows it further.
//!
//! "New Smart Folder" at the bottom of the folder popover (and the palette's "Save
//! Search as Smart Folder") saves the search as it is: scope, chips and text. A right
//! click on a smart folder renames or deletes it. Under the name field the symbol is
//! chosen from [`SYMBOLS`], stored as `icon` by its GNOME icon name.

use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use sheaf_core::Filter;
use sheaf_core::config::SmartFolder;
use sheaf_core::i18n::tr;

use super::{GalleryView, Popover, Scope, chips};
use crate::app::assets::{IconName, icon};
use crate::app::theme::Tokens;

/// The symbol a smart folder gets unless another is chosen: GNOME's saved-search folder.
pub const ICON: &str = "folder-saved-search-symbolic";

/// The symbols to choose from, by the GNOME icon name they are stored under.
pub const SYMBOLS: [(&str, IconName); 10] = [
    (ICON, IconName::SmartFolder),
    ("starred-symbolic", IconName::Star),
    ("object-select-symbolic", IconName::Check),
    ("x-office-calendar-symbolic", IconName::Calendar),
    ("alarm-symbolic", IconName::Alarm),
    ("user-home-symbolic", IconName::Home),
    ("emoji-food-symbolic", IconName::Food),
    ("emoji-travel-symbolic", IconName::Travel),
    ("applications-science-symbolic", IconName::Science),
    ("camera-photo-symbolic", IconName::Camera),
];

/// The symbol stored as `icon`; one this version does not know is the default.
pub fn symbol(icon: &str) -> IconName {
    SYMBOLS
        .iter()
        .find(|(name, _)| *name == icon)
        .map_or(IconName::SmartFolder, |(_, symbol)| *symbol)
}

/// What the name field in the folder popover is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Edit {
    New,
    Rename(String),
}

#[derive(Default)]
pub(super) struct SmartState {
    editing: Option<(Entity<InputState>, Edit)>,
    /// The symbol chosen for a new smart folder.
    icon: Option<&'static str>,
}

/// `name`, or `name 2`, `name 3`, … if a smart folder of that name exists.
pub fn unique_name(existing: &[SmartFolder], name: &str) -> String {
    let taken = |candidate: &str| existing.iter().any(|folder| folder.name == candidate);
    if !taken(name) {
        return name.to_owned();
    }
    (2..)
        .map(|number| format!("{name} {number}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

impl GalleryView {
    pub fn smart_folders(&self) -> &[SmartFolder] {
        self.config
            .as_ref()
            .map(|config| config.smart_folders.as_slice())
            .unwrap_or_default()
    }

    /// The filters and the words of the smart folder shown, if it is one.
    pub(super) fn smart_query(&self) -> (Vec<Filter>, String) {
        let Scope::Smart(name) = &self.scope else {
            return (Vec::new(), String::new());
        };
        self.smart_folders()
            .iter()
            .find(|folder| folder.name == *name)
            .map(|folder| chips::take(&folder.query, true))
            .unwrap_or_default()
    }

    /// The search as shown, with the folder or tag it is in and the smart folder's
    /// own search: what a new smart folder saves.
    pub fn full_query(&self) -> String {
        let (mut filters, words) = self.smart_query();
        filters.extend(self.scope.filter());
        filters.extend(self.chips.iter().cloned());
        let text = [words.as_str(), self.text.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        chips::compose(&filters, &text)
    }

    /// Saves the search shown as a smart folder called `name` (or `name 2`, …) and
    /// shows it. Nothing happens without a search or a name.
    pub fn save_smart_folder(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        let query = self.full_query();
        if name.is_empty() || query.is_empty() {
            return;
        }
        if self.config.is_none() {
            return;
        }
        let name = unique_name(self.smart_folders(), name);
        let folder = SmartFolder {
            name: name.clone(),
            icon: self.smart.icon.take().unwrap_or(ICON).to_owned(),
            query,
            other: toml::Table::new(),
        };
        self.change_config(move |config| config.smart_folders.push(folder.clone()), cx);
        self.smart.editing = None;
        self.popover = None;
        self.chips.clear();
        self.text.clear();
        self.pending_text = Some(String::new());
        self.scope = Scope::All;
        self.set_scope(Scope::Smart(name), cx);
    }

    /// Renames the smart folder `from`; the gallery keeps showing it if it did.
    pub fn rename_smart_folder(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let to = to.trim();
        self.smart.editing = None;
        if to.is_empty() || to == from {
            cx.notify();
            return;
        }
        if !self
            .smart_folders()
            .iter()
            .any(|folder| folder.name == from)
        {
            return;
        }
        let to = unique_name(self.smart_folders(), to);
        let (old, new) = (from.to_owned(), to.clone());
        self.change_config(
            move |config| {
                if let Some(folder) = config
                    .smart_folders
                    .iter_mut()
                    .find(|folder| folder.name == old)
                {
                    folder.name.clone_from(&new);
                }
            },
            cx,
        );
        if self.scope == Scope::Smart(from.to_owned()) {
            self.scope = Scope::Smart(to);
        }
        cx.notify();
    }

    /// Gives the smart folder `name` the symbol stored as `icon`.
    pub fn set_smart_icon(&mut self, name: &str, icon: &str, cx: &mut Context<Self>) {
        let unchanged = self
            .smart_folders()
            .iter()
            .find(|folder| folder.name == name)
            .is_none_or(|folder| folder.icon == icon);
        if unchanged {
            return;
        }
        let (name, icon) = (name.to_owned(), icon.to_owned());
        self.change_config(
            move |config| {
                if let Some(folder) = config
                    .smart_folders
                    .iter_mut()
                    .find(|folder| folder.name == name)
                {
                    folder.icon.clone_from(&icon);
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Chooses the symbol in the name field: of the smart folder being renamed at once,
    /// of a new one for when it is saved.
    pub fn choose_smart_icon(&mut self, icon: &'static str, cx: &mut Context<Self>) {
        match self.smart.editing.as_ref().map(|(_, edit)| edit.clone()) {
            Some(Edit::Rename(name)) => self.set_smart_icon(&name, icon, cx),
            Some(Edit::New) => {
                self.smart.icon = Some(icon);
                cx.notify();
            }
            None => {}
        }
    }

    /// Deletes the smart folder `name`; its notes stay where they are.
    pub fn delete_smart_folder(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self
            .smart_folders()
            .iter()
            .any(|folder| folder.name == name)
        {
            return;
        }
        let gone = name.to_owned();
        self.change_config(
            move |config| config.smart_folders.retain(|folder| folder.name != gone),
            cx,
        );
        if self.scope == Scope::Smart(name.to_owned()) {
            self.set_scope(Scope::All, cx);
        }
        cx.notify();
    }

    /// Opens the folder popover with the name field of a new smart folder, as the
    /// palette's "Save Search as Smart Folder" does.
    pub fn ask_smart_folder_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.full_query().is_empty() {
            return;
        }
        self.set_popover(Some(Popover::Folders), cx);
        self.start_smart_edit(Edit::New, window, cx);
    }

    fn start_smart_edit(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match &edit {
            Edit::New => String::new(),
            Edit::Rename(name) => name.clone(),
        };
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("Smart Folder Name"))
                .default_value(initial)
        });
        cx.subscribe_in(&field, window, |this, state, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let name = state.read(cx).value().to_string();
                match this.smart.editing.as_ref().map(|(_, edit)| edit.clone()) {
                    Some(Edit::New) => this.save_smart_folder(&name, cx),
                    Some(Edit::Rename(from)) => this.rename_smart_folder(&from, &name, cx),
                    None => {}
                }
            }
        })
        .detach();
        field.update(cx, |state, cx| state.focus(window, cx));
        self.new_folder = None;
        self.smart.editing = Some((field, edit));
        self.smart.icon = None;
        cx.notify();
    }

    /// Forgets the name field, as closing the popover does.
    pub(super) fn stop_smart_edit(&mut self) {
        self.smart.editing = None;
        self.smart.icon = None;
    }

    /// The symbol the name field shows as chosen.
    fn editing_icon(&self) -> &str {
        match self.smart.editing.as_ref().map(|(_, edit)| edit) {
            Some(Edit::Rename(name)) => self
                .smart_folders()
                .iter()
                .find(|folder| folder.name == *name)
                .map_or(ICON, |folder| folder.icon.as_str()),
            _ => self.smart.icon.unwrap_or(ICON),
        }
    }

    /// The name field with the symbols to choose from under it.
    fn editor(
        &self,
        field: &Entity<InputState>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chosen = symbol(self.editing_icon());
        let symbols = SYMBOLS.iter().enumerate().map(|(index, (name, symbol))| {
            let on = *symbol == chosen;
            let name: &'static str = name;
            div()
                .id(("smart-icon", index))
                .debug_selector(move || format!("smart-icon-{name}"))
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(on, |button| button.bg(tokens.selected))
                .when(!on, |button| button.hover(|style| style.bg(tokens.hover)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_smart_icon(name, cx);
                    // Back to the name, so that Enter saves.
                    if let Some((field, _)) = &this.smart.editing {
                        field.update(cx, |state, cx| state.focus(window, cx));
                    }
                }))
                .child(icon(*symbol, if on { tokens.accent } else { tokens.muted }).size(px(16.)))
        });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(4.))
            .child(name_field(field, chosen, tokens))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(2.))
                    .px(px(6.))
                    .pb(px(4.))
                    .children(symbols),
            )
            .into_any_element()
    }

    /// The smart folders of the folder popover, each with a menu to rename or delete
    /// it, and the name field while one is renamed.
    pub(super) fn render_smart_rows(
        &self,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let gallery = cx.entity().downgrade();
        self.smart_folders()
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                if let Some((field, Edit::Rename(from))) = &self.smart.editing
                    && *from == folder.name
                {
                    return self.editor(field, tokens, cx);
                }
                let scope = Scope::Smart(folder.name.clone());
                let on = self.scope == scope;
                let name = folder.name.clone();
                let gallery = gallery.clone();
                super::render::menu_row(
                    ("smart", index),
                    symbol(&folder.icon),
                    name.clone(),
                    tokens,
                )
                .when(on, |row| row.bg(tokens.selected))
                .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope.clone(), cx)))
                .context_menu(move |menu, _, _| {
                    let (rename, delete) = (gallery.clone(), gallery.clone());
                    let (from, name) = (name.clone(), name.clone());
                    menu.item(PopupMenuItem::new(tr("Rename…")).on_click(
                        move |_: &ClickEvent, window, cx| {
                            let from = from.clone();
                            let _ = rename.update(cx, |gallery, cx| {
                                gallery.start_smart_edit(Edit::Rename(from), window, cx);
                            });
                        },
                    ))
                    .item(PopupMenuItem::new(tr("Delete")).on_click(
                        move |_: &ClickEvent, _, cx| {
                            let _ = delete.update(cx, |gallery, cx| {
                                gallery.delete_smart_folder(&name, cx);
                            });
                        },
                    ))
                })
                .into_any_element()
            })
            .collect()
    }

    /// "New Smart Folder" at the bottom of the folder popover, or its name field.
    pub(super) fn render_new_smart_folder(
        &self,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some((field, Edit::New)) = &self.smart.editing {
            return self.editor(field, tokens, cx);
        }
        if self.full_query().is_empty() {
            // From the search only: without one there is nothing to save.
            return div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(11.))
                .h(px(34.))
                .px(px(12.))
                .text_size(tokens.text(13.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(tokens.faint)
                .child(icon(IconName::SmartFolder, tokens.faint).size(px(16.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(tr("New Smart Folder")),
                )
                .into_any_element();
        }
        super::render::menu_row(
            "new-smart-folder",
            IconName::SmartFolder,
            tr("New Smart Folder"),
            tokens,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.start_smart_edit(Edit::New, window, cx);
        }))
        .into_any_element()
    }

    /// The smart folders as chips, after the folders and tags.
    pub(super) fn smart_scopes(&self) -> Vec<(Scope, String)> {
        self.smart_folders()
            .iter()
            .map(|folder| (Scope::Smart(folder.name.clone()), folder.name.clone()))
            .collect()
    }
}

fn name_field(field: &Entity<InputState>, symbol: IconName, tokens: &Tokens) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(11.))
        .h(px(34.))
        .px(px(12.))
        .rounded(px(9.))
        .bg(tokens.hover)
        .child(icon(symbol, tokens.muted).size(px(16.)))
        .child(
            Input::new(field)
                .appearance(false)
                .flex_1()
                .h_full()
                .px_0()
                .text_size(tokens.text(13.5)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> SmartFolder {
        SmartFolder {
            name: name.to_owned(),
            ..SmartFolder::default()
        }
    }

    #[test]
    fn symbols_are_found_by_their_icon_name() {
        assert_eq!(symbol("starred-symbolic"), IconName::Star);
        assert_eq!(symbol(ICON), IconName::SmartFolder);
        assert_eq!(symbol("something-else"), IconName::SmartFolder);
        assert_eq!(symbol(""), IconName::SmartFolder);
    }

    #[test]
    fn names_are_made_unique() {
        let existing = [folder("Offen"), folder("Offen 2"), folder("Woche")];
        assert_eq!(unique_name(&existing, "Neu"), "Neu");
        assert_eq!(unique_name(&existing, "Offen"), "Offen 3");
        assert_eq!(unique_name(&existing, "Woche"), "Woche 2");
        assert_eq!(unique_name(&[], "Offen"), "Offen");
    }
}
