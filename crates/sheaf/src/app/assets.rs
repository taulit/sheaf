//! The files compiled into the binary: symbolic icons and the app icon. Their origin
//! and licence are recorded in `crates/sheaf/assets/README.md`.

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, SharedString, Svg, svg};

use super::theme::size;

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        /// A symbolic icon, drawn in the colour of the text around it.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum IconName {
            $($variant,)*
        }

        impl IconName {
            pub const ALL: &[IconName] = &[$(IconName::$variant,)*];

            /// The path the icon is loaded from, through the [`AssetSource`].
            pub const fn path(self) -> &'static str {
                match self {
                    $(IconName::$variant => concat!("icons/", $file, ".svg"),)*
                }
            }
        }

        const ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/", $file, ".svg"),
                include_bytes!(concat!("../../assets/icons/", $file, ".svg")),
            ),)*
        ];
    };
}

icons! {
    Attachment => "mail-attachment",
    Back => "go-previous",
    Check => "object-select",
    Clear => "edit-clear",
    Close => "window-close",
    Copy => "edit-copy",
    Delete => "edit-delete",
    Edit => "document-edit",
    Folder => "folder",
    FolderNew => "folder-new",
    Forward => "go-next",
    Grid => "view-grid",
    Image => "image-x-generic",
    List => "view-list",
    Menu => "open-menu",
    More => "view-more",
    PanDown => "pan-down",
    Pin => "view-pin",
    Plus => "list-add",
    Print => "document-print",
    SaveAs => "document-save-as",
    Search => "edit-find",
    Share => "send-to",
    Sidebar => "sidebar-show",
    SmartFolder => "folder-saved-search",
    Trash => "user-trash",
    Undo => "edit-undo",
}

/// The app icon in colour; draw it with `gpui::img(APP_ICON)`.
pub const APP_ICON: &str = "icons/app.svg";
/// The app icon as a symbolic icon; draw it with `gpui::svg().path(..)`.
pub const APP_ICON_SYMBOLIC: &str = "icons/app-symbolic.svg";

const APP_ICONS: &[(&str, &[u8])] = &[
    (
        APP_ICON,
        include_bytes!("../../../../data/icons/hicolor/scalable/apps/io.github.taulit.Sheaf.svg"),
    ),
    (
        APP_ICON_SYMBOLIC,
        include_bytes!(
            "../../../../data/icons/hicolor/symbolic/apps/io.github.taulit.Sheaf-symbolic.svg"
        ),
    ),
];

/// `name` at the size icons have in the 34 px buttons of the header bar, in `color`.
pub fn icon(name: IconName, color: Hsla) -> Svg {
    use gpui::Styled as _;
    svg()
        .path(name.path())
        .size(size::ICON)
        .flex_none()
        .text_color(color)
}

/// Serves our files first and gpui-component's own icons (its menus, inputs and
/// notifications draw them) for every other path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let ours = ICONS
            .iter()
            .chain(APP_ICONS)
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes));
        match ours {
            Some(bytes) => Ok(Some(bytes)),
            // Its only error is "no such file", which this trait spells `None`.
            None => Ok(gpui_kit_assets::Assets.load(path).ok().flatten()),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = ICONS
            .iter()
            .chain(APP_ICONS)
            .map(|(name, _)| *name)
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect();
        names.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_embedded_as_svg() {
        for icon in IconName::ALL {
            let bytes = Assets
                .load(icon.path())
                .expect("loads")
                .unwrap_or_else(|| panic!("{} is missing", icon.path()));
            assert!(bytes.windows(4).any(|window| window == b"<svg"), "{icon:?}");
        }
        for path in [APP_ICON, APP_ICON_SYMBOLIC] {
            assert!(Assets.load(path).expect("loads").is_some(), "{path}");
        }
        assert_eq!(Assets.load("icons/no-such-icon.svg").expect("loads"), None);
    }

    #[test]
    fn the_readme_lists_every_icon_file() {
        let readme = include_str!("../../assets/README.md");
        for (path, _) in ICONS {
            let file = path.trim_start_matches("icons/");
            assert!(
                readme.contains(&format!("`{file}`")),
                "{file} is not documented"
            );
        }
    }

    #[test]
    fn component_icons_still_load() {
        let listed = Assets.list("icons/").expect("lists");
        assert!(listed.len() > ICONS.len() + APP_ICONS.len());
    }
}
