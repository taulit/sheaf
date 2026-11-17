//! Settings: the app-wide [`AppConfig`] and the per-folder [`FolderConfig`] that
//! travels with the notes in `<notes>/.sheaf/config.toml`.
//!
//! Both keep keys they do not know, so a file written by a newer version or edited by
//! hand survives a load and save.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::i18n::tr;
use crate::model::NoteId;
use crate::store::fsops;

const FOLDER_CONFIG: &str = ".sheaf/config.toml";

/// The app's own settings, in `$XDG_CONFIG_HOME/sheaf/config.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// The notes folder, if the user chose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_dir: Option<PathBuf>,
    /// The order of folders that have none of their own; see [`FolderConfig::view`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// How the note view sets the text.
    #[serde(default, skip_serializing_if = "EditorSettings::is_default")]
    pub editor: EditorSettings,
    #[serde(flatten)]
    pub other: toml::Table,
}

/// How the note view sets the text: `[editor]` in the app configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub font: EditorFont,
    pub line_width: LineWidth,
    #[serde(flatten)]
    pub other: toml::Table,
}

impl EditorSettings {
    fn is_default(&self) -> bool {
        *self == EditorSettings::default()
    }
}

/// The typeface of the note text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorFont {
    /// The font of the desktop.
    #[default]
    System,
    Serif,
    Mono,
}

/// The longest line of note text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineWidth {
    Narrow,
    /// The 700 px column of plan 04.
    #[default]
    Medium,
    Wide,
}

/// What Sheaf remembers about one notes folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderConfig {
    /// Tags shown as filter chips.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pinned_tags: Vec<String>,
    #[serde(skip_serializing_if = "QuickNote::is_default")]
    pub quick_note: QuickNote,
    /// Sort order and view per folder, keyed by folder path; the empty key is "all notes".
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<String, FolderView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub smart_folders: Vec<SmartFolder>,
    /// View state per note, such as column widths and folded headings. The editor
    /// defines what is in it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<NoteId, toml::Table>,
    /// The order of folders without one of their own: [`AppConfig::sort`], put here
    /// by whoever loads this for a view. Not stored in this file.
    #[serde(skip)]
    pub default_sort: Sort,
    #[serde(flatten)]
    pub other: toml::Table,
}

/// A saved search that shows up like a folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SmartFolder {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    pub query: String,
    #[serde(flatten)]
    pub other: toml::Table,
}

/// How one folder is shown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderView {
    pub sort: Sort,
    pub view: View,
    #[serde(flatten)]
    pub other: toml::Table,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Modified,
    Created,
    Title,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    #[default]
    Gallery,
    List,
}

/// Settings of the Quick Note window, changed in the settings window. The Quick Note
/// window and `sheaf-searchd` read them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickNote {
    /// The folder for quick notes; see [`QuickNote::folder_name`] for the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// A quick note younger than this is continued instead of starting a new one;
    /// 0 always starts a new one.
    pub resume_minutes: u32,
    #[serde(flatten)]
    pub other: toml::Table,
}

impl Default for QuickNote {
    fn default() -> Self {
        QuickNote {
            folder: None,
            resume_minutes: 5,
            other: toml::Table::new(),
        }
    }
}

impl QuickNote {
    /// The configured folder, or the localised "Quick Notes".
    pub fn folder_name(&self) -> String {
        self.folder.clone().unwrap_or_else(|| tr("Quick Notes"))
    }

    fn is_default(&self) -> bool {
        *self == QuickNote::default()
    }
}

impl AppConfig {
    /// Reads the file at `path`; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        load(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save(self, path)
    }
}

impl FolderConfig {
    /// Where the configuration of the notes folder `root` lives.
    pub fn path(root: &Path) -> PathBuf {
        root.join(FOLDER_CONFIG)
    }

    /// Reads the configuration of the notes folder `root`; a missing file gives the defaults.
    pub fn load(root: &Path) -> Result<Self> {
        load(&Self::path(root))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        save(self, &Self::path(root))
    }

    /// How `folder` is shown; the defaults, with [`default_sort`](Self::default_sort),
    /// if nothing was stored for it.
    pub fn view(&self, folder: &str) -> FolderView {
        self.folders
            .get(folder)
            .cloned()
            .unwrap_or_else(|| FolderView {
                sort: self.default_sort,
                ..FolderView::default()
            })
    }
}

fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).map_err(|err| Error::Config(format!("{}: {err}", path.display())))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(err.into()),
    }
}

fn save<T: Serialize>(config: &T, path: &Path) -> Result<()> {
    let text = toml::to_string_pretty(config)
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fsops::write_atomic(path, text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r##"
pinned_tags = ["rezepte", "büro"]
future_setting = "kept"

[quick_note]
folder = "Eingang"
resume_minutes = 10
ask_first = true

[folders.""]
sort = "created"

[folders."Arbeit/Q4"]
sort = "title"
view = "list"
density = 2

[[smart_folders]]
name = "Offene Aufgaben"
icon = "checkbox-checked-symbolic"
query = "offen: #arbeit"
color = "blue"

[[smart_folders]]
name = "Diese Woche"
query = "geändert:woche"

[notes.01JD3K8Z6Q4X7V9B2M5N8P0R3T]
folded = ["Anhang", "Notizen"]
column_widths = { "0" = [120, 80.5] }

[plugin.settings]
enabled = true
"##;

    #[test]
    fn missing_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = FolderConfig::load(dir.path()).unwrap();
        assert_eq!(config, FolderConfig::default());
        assert_eq!(config.quick_note.resume_minutes, 5);
        assert_eq!(config.quick_note.folder_name(), "Quick Notes");
        assert_eq!(config.view("Arbeit"), FolderView::default());
        assert_eq!(
            (Sort::default(), View::default()),
            (Sort::Modified, View::Gallery)
        );
        assert_eq!(
            AppConfig::load(&dir.path().join("config.toml")).unwrap(),
            AppConfig::default()
        );
        assert!(!dir.path().join(".sheaf").exists());
    }

    #[test]
    fn folder_config_reads_every_section() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        fs::write(FolderConfig::path(dir.path()), FULL).unwrap();

        let config = FolderConfig::load(dir.path()).unwrap();
        assert_eq!(config.pinned_tags, ["rezepte", "büro"]);
        assert_eq!(config.quick_note.folder_name(), "Eingang");
        assert_eq!(config.quick_note.resume_minutes, 10);
        assert_eq!(config.view("").sort, Sort::Created);
        assert_eq!(config.view("").view, View::Gallery);
        assert_eq!(config.view("Arbeit/Q4").sort, Sort::Title);
        assert_eq!(config.view("Arbeit/Q4").view, View::List);
        assert_eq!(config.smart_folders.len(), 2);
        assert_eq!(config.smart_folders[0].name, "Offene Aufgaben");
        assert_eq!(config.smart_folders[0].icon, "checkbox-checked-symbolic");
        assert_eq!(config.smart_folders[0].query, "offen: #arbeit");
        assert_eq!(config.smart_folders[1].icon, "");
        let id: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();
        assert_eq!(config.notes[&id]["folded"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unknown_keys_survive_a_load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        fs::write(FolderConfig::path(dir.path()), FULL).unwrap();
        let loaded = FolderConfig::load(dir.path()).unwrap();

        let mut changed = loaded.clone();
        changed.pinned_tags.push("neu".to_owned());
        changed.folders.entry("Privat".to_owned()).or_default().view = View::List;
        changed.save(dir.path()).unwrap();

        let reloaded = FolderConfig::load(dir.path()).unwrap();
        assert_eq!(reloaded, changed);
        assert_eq!(reloaded.other["future_setting"].as_str(), Some("kept"));
        assert_eq!(
            reloaded.other["plugin"]["settings"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(reloaded.quick_note.other["ask_first"].as_bool(), Some(true));
        assert_eq!(
            reloaded.folders["Arbeit/Q4"].other["density"].as_integer(),
            Some(2)
        );
        assert_eq!(
            reloaded.smart_folders[0].other["color"].as_str(),
            Some("blue")
        );
        assert_eq!(reloaded.view("Privat").view, View::List);
        let entries: Vec<_> = fs::read_dir(dir.path().join(".sheaf")).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn defaults_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        FolderConfig::default().save(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(FolderConfig::path(dir.path())).unwrap(),
            ""
        );

        let config = FolderConfig {
            pinned_tags: vec!["a".to_owned()],
            ..FolderConfig::default()
        };
        config.save(dir.path()).unwrap();
        let text = fs::read_to_string(FolderConfig::path(dir.path())).unwrap();
        assert_eq!(text.trim(), "pinned_tags = [\"a\"]");
    }

    #[test]
    fn a_broken_file_is_an_error_and_stays() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        for broken in [
            "pinned_tags = [",
            "[folders.x]\nsort = \"size\"\n",
            "quick_note = 3\n",
        ] {
            fs::write(FolderConfig::path(dir.path()), broken).unwrap();
            let err = FolderConfig::load(dir.path()).unwrap_err();
            assert!(matches!(err, Error::Config(_)), "{broken:?}: {err:?}");
            assert_eq!(err.code(), "config");
            assert_eq!(
                fs::read_to_string(FolderConfig::path(dir.path())).unwrap(),
                broken
            );
        }
    }

    #[test]
    fn app_config_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheaf/config.toml");
        let mut config = AppConfig {
            notes_dir: Some(PathBuf::from("/home/me/Notizen")),
            ..AppConfig::default()
        };
        config.other.insert(
            "window".to_owned(),
            toml::Value::String("1100x720".to_owned()),
        );
        config.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), config);

        fs::write(&path, "notes_dir = 5\n").unwrap();
        assert!(matches!(AppConfig::load(&path), Err(Error::Config(_))));
    }

    #[test]
    fn app_settings_round_trip_and_keep_what_they_do_not_know() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "notes_dir = \"/n\"\nsort = \"title\"\nwindow = \"big\"\n\n[editor]\nfont = \"serif\"\nline_width = \"wide\"\nligatures = false\n",
        )
        .unwrap();
        let mut config = AppConfig::load(&path).unwrap();
        assert_eq!(config.sort, Some(Sort::Title));
        assert_eq!(config.editor.font, EditorFont::Serif);
        assert_eq!(config.editor.line_width, LineWidth::Wide);
        config.editor.font = EditorFont::Mono;
        config.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("window = \"big\""), "{text}");
        assert!(text.contains("ligatures = false"), "{text}");
        assert!(text.contains("font = \"mono\""), "{text}");
        assert_eq!(AppConfig::load(&path).unwrap(), config);

        AppConfig::default().save(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn folders_without_an_order_take_the_default_one() {
        let mut config = FolderConfig::default();
        config.folders.entry("Arbeit".to_owned()).or_default().sort = Sort::Created;
        config.default_sort = Sort::Title;
        assert_eq!(config.view("Privat").sort, Sort::Title);
        assert_eq!(config.view("Arbeit").sort, Sort::Created);
        let dir = tempfile::tempdir().unwrap();
        config.save(dir.path()).unwrap();
        let text = fs::read_to_string(FolderConfig::path(dir.path())).unwrap();
        assert!(!text.contains("default_sort"), "{text}");
    }
}
