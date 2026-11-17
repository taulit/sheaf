//! The settings (plan 03 §9): what the user chose, in `$XDG_CONFIG_HOME/sheaf/config.toml`
//! ([`AppConfig`], kept lossless), and the window to change it in ([`view`]).
//!
//! # Interface
//!
//! - [`config`] reads the settings; [`update`] changes them, writes the file in the
//!   background and tells every observer of the [`Settings`] global, so open views
//!   follow at once: the note view's font and line width, the gallery's default
//!   order, the notes folder of the main window.
//! - The Quick Note settings belong to the notes folder (`FolderConfig::quick_note`)
//!   and are changed through the [`Library`](super::library::Library).
//! - Without a [`Settings`] global, as in tests that do not set one, the defaults
//!   apply and nothing is written.

pub mod view;

use std::path::PathBuf;

use gpui::{App, AppContext as _, Global, Pixels, SharedString, Task, px};
use sheaf_core::AppConfig;
use sheaf_core::config::{EditorFont, LineWidth};

use super::theme::ActiveTokens as _;

/// The settings and the file they live in.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    file: Option<PathBuf>,
    config: AppConfig,
}

impl Global for Settings {}

impl Settings {
    /// The settings in `file`; the defaults if it is missing or cannot be read, which
    /// is logged. Blocks: read once at startup.
    pub fn load(file: PathBuf) -> Self {
        let config = AppConfig::load(&file).unwrap_or_else(|err| {
            log::warn!("cannot read the settings, using the defaults: {err}");
            AppConfig::default()
        });
        Settings {
            file: Some(file),
            config,
        }
    }

    /// `config`, stored in `file` when changed; nowhere without one.
    pub fn new(file: Option<PathBuf>, config: AppConfig) -> Self {
        Settings { file, config }
    }
}

/// The settings now.
pub fn config(cx: &App) -> AppConfig {
    cx.try_global::<Settings>()
        .map(|settings| settings.config.clone())
        .unwrap_or_default()
}

/// Changes the settings, tells the observers and writes the file in the background.
/// The task fails with the reason if the file cannot be written.
pub fn update(cx: &mut App, change: impl FnOnce(&mut AppConfig)) -> Task<Result<(), String>> {
    let mut settings = cx.try_global::<Settings>().cloned().unwrap_or_default();
    change(&mut settings.config);
    let file = settings.file.clone();
    let config = settings.config.clone();
    cx.set_global(settings);
    cx.background_spawn(async move {
        match file {
            Some(file) => config
                .save(&file)
                .map_err(|err| format!("{}: {err}", file.display())),
            None => Ok(()),
        }
    })
}

/// The column of note text for `width`: plan 04's 700 px in the middle.
pub fn column(width: LineWidth) -> Pixels {
    match width {
        LineWidth::Narrow => px(600.),
        LineWidth::Medium => px(700.),
        LineWidth::Wide => px(860.),
    }
}

/// The widest column, which the editor is created with; narrower ones are padding.
pub const WIDEST_COLUMN: Pixels = px(860.);

/// Families tried in order for the serif and monospaced choices.
const SERIF: &[&str] = &[
    "Noto Serif",
    "Source Serif 4",
    "DejaVu Serif",
    "Liberation Serif",
    "Georgia",
];
const MONO: &[&str] = &[
    "Adwaita Mono",
    "Source Code Pro",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
];

/// The family the note text is set in: the desktop's for [`EditorFont::System`], else
/// the first installed of the usual serif or monospaced families.
pub fn font_family(font: EditorFont, cx: &App) -> SharedString {
    let candidates = match font {
        EditorFont::System => return cx.tokens().font_family.clone(),
        EditorFont::Serif => SERIF,
        EditorFont::Mono => MONO,
    };
    let installed = cx.text_system().all_font_names();
    candidates
        .iter()
        .find(|family| installed.iter().any(|name| name == *family))
        .map(|family| SharedString::from(*family))
        .unwrap_or_else(|| cx.tokens().font_family.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_grow_with_the_line_width() {
        assert!(column(LineWidth::Narrow) < column(LineWidth::Medium));
        assert_eq!(
            column(LineWidth::Medium),
            super::super::theme::size::EDITOR_MAX_WIDTH
        );
        assert_eq!(column(LineWidth::Wide), WIDEST_COLUMN);
    }
}
