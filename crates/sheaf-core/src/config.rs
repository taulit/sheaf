//! Settings: the app-wide [`AppConfig`] and the per-folder [`FolderConfig`] that
//! travels with the notes in `<notes>/.sheaf/config.toml`.
//!
//! Both keep keys they do not know, so a file written by a newer version or edited by
//! hand survives a load and a change. A change is written key by key
//! ([`FolderConfig::update`], [`AppConfig::save_changes`]): the file is read again and
//! only the keys the change touched are replaced, so every other key stays as it is on
//! disk, also one with a value this version does not accept.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::i18n::tr;
pub use crate::index::Sort;
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
    #[serde(default)]
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
}

impl AppConfig {
    /// Reads the file at `path`; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        load(path)
    }

    /// Writes to the file at `path` the keys in which `after` differs from `before`,
    /// and nothing else. Fails without writing if the file is not TOML.
    pub fn save_changes(path: &Path, before: &Self, after: &Self) -> Result<()> {
        let mut file = read_table(path)?;
        if patch(
            &mut file,
            &to_table(before, path)?,
            &to_table(after, path)?,
            path,
        )? {
            write_table(&file, path)?;
        }
        Ok(())
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

    /// Reads the configuration of `root` again, lets `change` change it and writes back
    /// only the keys it changed. Returns the configuration as it is now. Fails without
    /// writing if the file cannot be read, for example because of a value this version
    /// does not accept.
    pub fn update(root: &Path, change: impl FnOnce(&mut FolderConfig)) -> Result<FolderConfig> {
        let path = Self::path(root);
        let mut file = read_table(&path)?;
        let before: FolderConfig = file
            .clone()
            .try_into()
            .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
        let mut after = before.clone();
        change(&mut after);
        if patch(
            &mut file,
            &to_table(&before, &path)?,
            &to_table(&after, &path)?,
            &path,
        )? {
            write_table(&file, &path)?;
        }
        Ok(after)
    }

    /// Makes what names the folder `from`, or one inside it, name `to` instead: its
    /// view, the Quick Notes folder and the smart folders' searches.
    pub fn rename_folder(&mut self, from: &str, to: &str) {
        let moved = |folder: &str| match folder.strip_prefix(from) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => Some(format!("{to}{rest}")),
            _ => None,
        };
        let renamed: Vec<(String, String)> = self
            .folders
            .keys()
            .filter_map(|folder| Some((folder.clone(), moved(folder)?)))
            .collect();
        for (old, new) in renamed {
            if let Some(view) = self.folders.remove(&old) {
                self.folders.insert(new, view);
            }
        }
        // The default "Quick Notes" is written out once renamed, or the next Quick Note
        // would make the old folder again.
        if let Some(folder) = moved(&self.quick_note.folder_name()) {
            self.quick_note.folder = Some(folder);
        }
        for smart in &mut self.smart_folders {
            smart.query = crate::query::rename_folder(&smart.query, from, to);
        }
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

/// The file at `path` as plain TOML, whatever its values mean; empty if it is missing.
fn read_table(path: &Path) -> Result<toml::Table> {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .map_err(|err| Error::Config(format!("{}: {err}", path.display()))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(err) => Err(err.into()),
    }
}

fn to_table<T: Serialize>(config: &T, path: &Path) -> Result<toml::Table> {
    toml::Table::try_from(config).map_err(|err| Error::Config(format!("{}: {err}", path.display())))
}

fn write_table(table: &toml::Table, path: &Path) -> Result<()> {
    let text = toml::to_string_pretty(table)
        .map_err(|err| Error::Config(format!("{}: {err}", path.display())))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fsops::write_atomic(path, text.as_bytes())?;
    Ok(())
}

/// Makes the keys of `file` in which `after` differs from `before` what they are in
/// `after`; the others stay. Tables are compared key by key, other values whole.
/// Returns whether `file` changed. Fails, leaving `file` as it was, if a table of
/// `after` is something else in `file`.
fn patch(
    file: &mut toml::Table,
    before: &toml::Table,
    after: &toml::Table,
    path: &Path,
) -> Result<bool> {
    let mut patched = file.clone();
    let changed = patch_table(&mut patched, before, after, path)?;
    if changed {
        *file = patched;
    }
    Ok(changed)
}

fn patch_table(
    file: &mut toml::Table,
    before: &toml::Table,
    after: &toml::Table,
    path: &Path,
) -> Result<bool> {
    let empty = toml::Table::new();
    let mut changed = false;
    for (key, new) in after {
        let old = before.get(key);
        if old == Some(new) {
            continue;
        }
        if let toml::Value::Table(new) = new {
            let old = match old {
                Some(toml::Value::Table(old)) => old,
                _ => &empty,
            };
            if new.is_empty() && !file.contains_key(key) {
                continue;
            }
            let target = file
                .entry(key.clone())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            let toml::Value::Table(target) = target else {
                return Err(Error::Config(format!(
                    "{}: `{key}` is not a table",
                    path.display()
                )));
            };
            changed |= patch_table(target, old, new, path)?;
        } else if file.get(key) != Some(new) {
            file.insert(key.clone(), new.clone());
            changed = true;
        }
    }
    for (key, old) in before.iter().filter(|(key, _)| !after.contains_key(*key)) {
        match (old, file.get_mut(key)) {
            (toml::Value::Table(old), Some(toml::Value::Table(target))) => {
                changed |= patch_table(target, old, &empty, path)?;
                if target.is_empty() {
                    file.remove(key);
                    changed = true;
                }
            }
            (_, Some(_)) => {
                file.remove(key);
                changed = true;
            }
            (_, None) => {}
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_names_a_renamed_folder_follows_it() {
        let mut config = FolderConfig {
            quick_note: QuickNote {
                folder: Some("Arbeit/Schnell".to_owned()),
                ..QuickNote::default()
            },
            smart_folders: vec![SmartFolder {
                name: "Offen".to_owned(),
                query: "folder:/Arbeit open:".to_owned(),
                ..SmartFolder::default()
            }],
            ..FolderConfig::default()
        };
        for folder in ["Arbeit", "Arbeit/Kunden", "Arbeitszeit", ""] {
            config
                .folders
                .insert(folder.to_owned(), FolderView::default());
        }
        config.folders.get_mut("Arbeit").unwrap().view = View::List;

        config.rename_folder("Arbeit", "Büro");
        let folders: Vec<&str> = config.folders.keys().map(String::as_str).collect();
        assert_eq!(folders, ["", "Arbeitszeit", "Büro", "Büro/Kunden"]);
        assert_eq!(config.folders["Büro"].view, View::List);
        assert_eq!(config.quick_note.folder.as_deref(), Some("Büro/Schnell"));
        assert_eq!(config.smart_folders[0].query, "folder:/Büro open:");
    }

    #[test]
    fn the_default_quick_notes_folder_follows_a_rename() {
        let mut config = FolderConfig::default();
        config.rename_folder("Arbeit", "Büro");
        assert_eq!(config.quick_note.folder, None);

        config.rename_folder("Quick Notes", "Inbox");
        assert_eq!(config.quick_note.folder.as_deref(), Some("Inbox"));
    }

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
    fn an_update_changes_only_its_keys_and_keeps_unknown_ones() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        fs::write(FolderConfig::path(dir.path()), FULL).unwrap();
        let loaded = FolderConfig::load(dir.path()).unwrap();

        let changed = FolderConfig::update(dir.path(), |config| {
            config.pinned_tags.push("neu".to_owned());
            config.folders.entry("Privat".to_owned()).or_default().view = View::List;
        })
        .unwrap();

        let reloaded = FolderConfig::load(dir.path()).unwrap();
        assert_eq!(reloaded, changed);
        assert_eq!(reloaded.pinned_tags, ["rezepte", "büro", "neu"]);
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
        assert_eq!(
            FolderConfig {
                pinned_tags: loaded.pinned_tags.clone(),
                folders: loaded.folders.clone(),
                ..reloaded.clone()
            },
            loaded
        );
        let entries: Vec<_> = fs::read_dir(dir.path().join(".sheaf")).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn updates_of_different_keys_both_survive() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        fs::write(FolderConfig::path(dir.path()), FULL).unwrap();
        let id: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();

        FolderConfig::update(dir.path(), |config| {
            config.folders.entry(String::new()).or_default().view = View::List;
        })
        .unwrap();
        FolderConfig::update(dir.path(), |config| {
            config.notes.remove(&id);
            config.quick_note.resume_minutes = 0;
        })
        .unwrap();

        let config = FolderConfig::load(dir.path()).unwrap();
        assert_eq!(config.view("").view, View::List);
        assert_eq!(config.view("").sort, Sort::Created);
        assert!(config.notes.is_empty());
        assert_eq!(config.quick_note.resume_minutes, 0);
        assert_eq!(config.quick_note.folder_name(), "Eingang");
        assert_eq!(config.smart_folders.len(), 2);
        assert_eq!(config.other["future_setting"].as_str(), Some("kept"));
    }

    #[test]
    fn an_update_that_changes_nothing_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        FolderConfig::update(dir.path(), |_| {}).unwrap();
        assert!(!dir.path().join(".sheaf").exists());

        let config = FolderConfig::update(dir.path(), |config| {
            config.pinned_tags = vec!["a".to_owned()];
        })
        .unwrap();
        assert_eq!(config.pinned_tags, ["a"]);
        let text = fs::read_to_string(FolderConfig::path(dir.path())).unwrap();
        assert_eq!(text.trim(), "pinned_tags = [\"a\"]");
    }

    #[test]
    fn an_update_of_an_unreadable_file_fails_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".sheaf")).unwrap();
        for broken in ["pinned_tags = [", "[folders.x]\nsort = \"size\"\n"] {
            fs::write(FolderConfig::path(dir.path()), broken).unwrap();
            let err = FolderConfig::update(dir.path(), |config| {
                config.pinned_tags.push("a".to_owned());
            })
            .unwrap_err();
            assert!(matches!(err, Error::Config(_)), "{broken:?}: {err:?}");
            assert_eq!(
                fs::read_to_string(FolderConfig::path(dir.path())).unwrap(),
                broken
            );
        }
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
        AppConfig::save_changes(&path, &AppConfig::default(), &config).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), config);

        fs::write(&path, "notes_dir = 5\n").unwrap();
        assert!(matches!(AppConfig::load(&path), Err(Error::Config(_))));
    }

    #[test]
    fn app_settings_change_only_their_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "notes_dir = \"/n\"\nsort = \"title\"\nwindow = \"big\"\n\n[editor]\nfont = \"serif\"\nline_width = \"wide\"\nligatures = false\n",
        )
        .unwrap();
        let before = AppConfig::load(&path).unwrap();
        assert_eq!(before.sort, Some(Sort::Title));
        assert_eq!(before.editor.font, EditorFont::Serif);
        assert_eq!(before.editor.line_width, LineWidth::Wide);
        let mut after = before.clone();
        after.editor.font = EditorFont::Mono;
        AppConfig::save_changes(&path, &before, &after).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("window = \"big\""), "{text}");
        assert!(text.contains("ligatures = false"), "{text}");
        assert!(text.contains("font = \"mono\""), "{text}");
        assert_eq!(AppConfig::load(&path).unwrap(), after);

        let mut unset = after.clone();
        unset.sort = None;
        AppConfig::save_changes(&path, &after, &unset).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), unset);
    }

    #[test]
    fn values_this_version_rejects_survive_a_change_of_another_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = "sort = \"manual\"\nwindow = \"big\"\n\n[editor]\nline_width = \"huge\"\n";
        fs::write(&path, text).unwrap();
        assert!(AppConfig::load(&path).is_err());

        // What the app works with when the file could not be read.
        let before = AppConfig::default();
        let mut after = before.clone();
        after.editor.font = EditorFont::Mono;
        AppConfig::save_changes(&path, &before, &after).unwrap();

        let table: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(table["sort"].as_str(), Some("manual"));
        assert_eq!(table["window"].as_str(), Some("big"));
        assert_eq!(table["editor"]["line_width"].as_str(), Some("huge"));
        assert_eq!(table["editor"]["font"].as_str(), Some("mono"));
    }

    #[test]
    fn a_change_to_a_file_that_is_not_toml_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for broken in ["sort = [", "editor = 3\n"] {
            fs::write(&path, broken).unwrap();
            let mut after = AppConfig::default();
            after.editor.font = EditorFont::Mono;
            let err = AppConfig::save_changes(&path, &AppConfig::default(), &after).unwrap_err();
            assert!(matches!(err, Error::Config(_)), "{broken:?}: {err:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        }
    }

    #[test]
    fn folders_without_an_order_take_the_default_one() {
        let mut config = FolderConfig::default();
        config.folders.entry("Arbeit".to_owned()).or_default().sort = Sort::Created;
        config.default_sort = Sort::Title;
        assert_eq!(config.view("Privat").sort, Sort::Title);
        assert_eq!(config.view("Arbeit").sort, Sort::Created);
        let dir = tempfile::tempdir().unwrap();
        FolderConfig::update(dir.path(), |stored| {
            stored.folders.clone_from(&config.folders);
            stored.default_sort = Sort::Title;
        })
        .unwrap();
        let text = fs::read_to_string(FolderConfig::path(dir.path())).unwrap();
        assert!(!text.contains("default_sort"), "{text}");
    }
}
