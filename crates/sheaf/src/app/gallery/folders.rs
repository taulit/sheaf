//! Renaming and deleting folders from the folder popover: a right click on a folder
//! offers "Rename…", which turns its row into a name field, and "Delete…", which asks
//! first and moves its notes to the trash, with Undo in the toast. What the folder
//! settings say about a renamed folder follows it ([`FolderConfig::rename_folder`]).

use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement as _,
    Styled as _, WeakEntity, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use sheaf_core::FolderConfig;
use sheaf_core::i18n::{ntr, tr};

use super::trash::Confirm;
use super::{GalleryEvent, GalleryView, Scope};
use crate::app::assets::{IconName, icon};
use crate::app::theme::Tokens;

/// `folder` moved to `from`'s new path `to`, if it is `from` or inside it.
fn moved(folder: &str, from: &str, to: &str) -> Option<String> {
    match folder.strip_prefix(from) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => Some(format!("{to}{rest}")),
        _ => None,
    }
}

/// The path of `folder` once it is called `name`, in the same parent.
fn renamed_path(folder: &str, name: &str) -> String {
    match folder.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{name}"),
        None => name.to_owned(),
    }
}

/// "3 files stayed in the folder", for a toast.
fn kept_message(kept: usize) -> String {
    let count = u32::try_from(kept).unwrap_or(u32::MAX);
    ntr(
        "{} file stayed in the folder",
        "{} files stayed in the folder",
        count,
    )
    .replace("{}", &count.to_string())
}

impl GalleryView {
    /// The folder whose row is a name field, with that field.
    pub(super) fn renaming_folder(&self) -> Option<&(Entity<InputState>, String)> {
        self.renaming.as_ref()
    }

    /// Calls `folder` `name`, in the same parent folder. The gallery keeps showing it,
    /// and what the folder settings say about it follows. A failure is a toast.
    pub fn rename_folder(&mut self, folder: &str, name: &str, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
        let name = name.trim().trim_matches('/');
        let to = renamed_path(folder, name);
        if name.is_empty() || name.contains('/') || to == folder {
            return;
        }
        let from = folder.to_owned();
        let renaming = self
            .library
            .update(cx, |library, cx| library.rename_folder(&from, &to, cx));
        cx.spawn(async move |this, cx| {
            let renamed = renaming.await;
            let _ = this.update(cx, |this, cx| match renamed {
                Ok(renamed) => {
                    if renamed.unreadable > 0 {
                        let count = u32::try_from(renamed.unreadable).unwrap_or(u32::MAX);
                        let message = ntr(
                            "Renamed; {} note could not be read and is missing from search",
                            "Renamed; {} notes could not be read and are missing from search",
                            count,
                        )
                        .replace("{}", &count.to_string());
                        cx.emit(GalleryEvent::Notice(message));
                    }
                    let (old, new) = (from.clone(), to.clone());
                    this.change_config(
                        move |config: &mut FolderConfig| {
                            config.rename_folder(&old, &new, &renamed.folders);
                        },
                        cx,
                    );
                    if let Scope::Folder(shown) = &this.scope
                        && let Some(shown) = moved(shown, &from, &to)
                    {
                        this.set_scope(Scope::Folder(shown), cx);
                    }
                }
                Err(err) => {
                    log::error!("cannot rename the folder: {err}");
                    let message =
                        tr("Could not rename the folder: {}").replace("{}", &err.to_string());
                    cx.emit(GalleryEvent::Failed(message));
                }
            });
        })
        .detach();
    }

    /// Asks before moving the notes of `folder` to the trash.
    pub fn ask_delete_folder(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.popover = None;
        self.trash.confirm = Some(Confirm::DeleteFolder(folder.to_owned()));
        cx.notify();
    }

    /// Moves the notes of `folder` to the trash and removes it; the toast offers Undo.
    pub(super) fn delete_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        let deleting = self
            .library
            .update(cx, |library, cx| library.delete_folder(&folder, cx));
        if let Scope::Folder(shown) = &self.scope
            && moved(shown, &folder, &folder).is_some()
        {
            self.set_scope(Scope::All, cx);
        }
        cx.spawn(async move |this, cx| {
            let event = match deleting.await {
                Ok(deleted) if deleted.trashed.is_empty() => return,
                Ok(deleted) => GalleryEvent::Trashed {
                    ids: deleted.trashed.into_iter().map(|meta| meta.id).collect(),
                    kept: (deleted.kept > 0).then(|| kept_message(deleted.kept)),
                },
                Err(err) => {
                    log::error!("cannot delete the folder: {err}");
                    GalleryEvent::Failed(
                        tr("Could not delete the folder: {}").replace("{}", &err.to_string()),
                    )
                }
            };
            let _ = this.update(cx, |_, cx| cx.emit(event));
        })
        .detach();
    }

    fn start_folder_rename(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = folder.rsplit('/').next().unwrap_or(folder).to_owned();
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("Folder Name"))
                .default_value(name)
        });
        let from = folder.to_owned();
        cx.subscribe_in(
            &field,
            window,
            move |this, state, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let name = state.read(cx).value().to_string();
                    this.rename_folder(&from, &name, cx);
                }
            },
        )
        .detach();
        field.update(cx, |state, cx| state.focus(window, cx));
        self.new_folder = None;
        self.stop_smart_edit();
        self.renaming = Some((field, folder.to_owned()));
        cx.notify();
    }
}

/// "Rename…" and "Delete…" for the folder at `path`.
pub(super) fn menu(menu: PopupMenu, gallery: &WeakEntity<GalleryView>, path: &str) -> PopupMenu {
    let (rename, delete) = (gallery.clone(), gallery.clone());
    let (from, folder) = (path.to_owned(), path.to_owned());
    menu.item(
        PopupMenuItem::new(tr("Rename…")).on_click(move |_: &ClickEvent, window, cx| {
            let _ = rename.update(cx, |gallery, cx| {
                gallery.start_folder_rename(&from, window, cx)
            });
        }),
    )
    .item(
        PopupMenuItem::new(tr("Delete…")).on_click(move |_: &ClickEvent, _, cx| {
            let _ = delete.update(cx, |gallery, cx| gallery.ask_delete_folder(&folder, cx));
        }),
    )
}

/// The name field in place of a folder's row, indented as the row.
pub(super) fn name_field(field: &Entity<InputState>, depth: usize, tokens: &Tokens) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(11.))
        .h(px(34.))
        .pl(px(12. + 18. * depth as f32))
        .pr(px(12.))
        .rounded(px(9.))
        .bg(tokens.hover)
        .child(icon(IconName::Folder, tokens.muted).size(px(16.)))
        .child(
            Input::new(field)
                .appearance(false)
                .flex_1()
                .h_full()
                .px_0()
                .text_size(tokens.text(13.5)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_folder_stays_in_its_parent() {
        assert_eq!(renamed_path("Arbeit", "Büro"), "Büro");
        assert_eq!(renamed_path("Privat/Rezepte", "Kochen"), "Privat/Kochen");
    }

    #[test]
    fn folders_inside_move_with_it() {
        assert_eq!(moved("Arbeit", "Arbeit", "Büro").as_deref(), Some("Büro"));
        assert_eq!(
            moved("Arbeit/A", "Arbeit", "Büro").as_deref(),
            Some("Büro/A")
        );
        assert_eq!(moved("Arbeitszeit", "Arbeit", "Büro"), None);
    }
}
