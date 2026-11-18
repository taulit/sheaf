//! Where Sheaf keeps things: configuration, notes folder and index.
//!
//! The environment is passed in as an [`Env`] instead of being read here, so that
//! callers decide where it comes from and tests never touch the process environment.

use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::i18n::tr;

/// The environment variables that decide where Sheaf's files are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    /// `HOME`
    pub home: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`
    pub config_home: Option<PathBuf>,
    /// `XDG_CACHE_HOME`
    pub cache_home: Option<PathBuf>,
    /// `XDG_DOCUMENTS_DIR`, which usually comes from `user-dirs.dirs` instead.
    pub documents_dir: Option<PathBuf>,
    /// `SHEAF_NOTES_DIR`
    pub notes_dir: Option<PathBuf>,
}

impl Env {
    /// The environment of this process.
    pub fn from_process() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Env {
            home: var("HOME"),
            config_home: var("XDG_CONFIG_HOME"),
            cache_home: var("XDG_CACHE_HOME"),
            documents_dir: var("XDG_DOCUMENTS_DIR"),
            notes_dir: var("SHEAF_NOTES_DIR"),
        }
    }

    /// `$XDG_CONFIG_HOME`, or `~/.config`.
    pub fn config_home(&self) -> Result<PathBuf> {
        self.base_dir(&self.config_home, ".config")
    }

    /// `$XDG_CACHE_HOME`, or `~/.cache`.
    pub fn cache_home(&self) -> Result<PathBuf> {
        self.base_dir(&self.cache_home, ".cache")
    }

    /// The user's documents folder: `XDG_DOCUMENTS_DIR` from the environment or from
    /// `user-dirs.dirs`, or `~/Documents`.
    pub fn documents_dir(&self) -> Result<PathBuf> {
        if let Some(dir) = self.documents_dir.as_ref().filter(|dir| dir.is_absolute()) {
            return Ok(dir.clone());
        }
        let home = self.home()?;
        let user_dirs = self.config_home()?.join("user-dirs.dirs");
        let configured = std::fs::read_to_string(user_dirs)
            .ok()
            .and_then(|text| user_dir(&text, "XDG_DOCUMENTS_DIR", home));
        Ok(configured.unwrap_or_else(|| home.join("Documents")))
    }

    /// Where the [`AppConfig`] is stored.
    pub fn app_config_path(&self) -> Result<PathBuf> {
        Ok(self.config_home()?.join("sheaf/config.toml"))
    }

    /// The notes folder taken on first run: "Notes", localised, in the documents folder.
    /// A notes folder that is already there under another language's name is taken
    /// instead of making a second one.
    pub fn default_notes_dir(&self) -> Result<PathBuf> {
        Ok(default_in(&self.documents_dir()?, &tr("Notes")))
    }

    /// The notes folder to use: `explicit` (the `--dir` option), else `SHEAF_NOTES_DIR`,
    /// else `notes_dir` of the app configuration, else the default. Whoever comes to
    /// the default first (app, service or CLI) records it in the app configuration, so
    /// the folder stays where it is when the language changes.
    pub fn notes_dir(&self, explicit: Option<&Path>) -> Result<PathBuf> {
        if let Some(dir) = explicit.or(self.notes_dir.as_deref()) {
            return Ok(dir.to_owned());
        }
        let path = self.app_config_path()?;
        let config = AppConfig::load(&path)?;
        if let Some(dir) = config.notes_dir {
            return match dir.strip_prefix("~") {
                Ok(below_home) => Ok(self.home()?.join(below_home)),
                Err(_) => Ok(dir),
            };
        }
        let dir = self.default_notes_dir()?;
        let stored = match dir.strip_prefix(self.home()?) {
            Ok(below_home) => Path::new("~").join(below_home),
            Err(_) => dir.clone(),
        };
        let chosen = AppConfig {
            notes_dir: Some(stored),
            ..config.clone()
        };
        if let Err(err) = AppConfig::save_changes(&path, &config, &chosen) {
            log::warn!("cannot record the notes folder: {err}");
        }
        Ok(dir)
    }

    /// Where the search index of a notes folder lives:
    /// `$XDG_CACHE_HOME/sheaf/index/<hash of the folder's canonical path>/`.
    pub fn index_dir(&self, notes_dir: &Path) -> Result<PathBuf> {
        let canonical = notes_dir
            .canonicalize()
            .or_else(|_| std::path::absolute(notes_dir))
            .unwrap_or_else(|_| notes_dir.to_owned());
        let hash = fnv1a_64(canonical.as_os_str().as_encoded_bytes());
        Ok(self.cache_home()?.join(format!("sheaf/index/{hash:016x}")))
    }

    fn home(&self) -> Result<&Path> {
        self.home
            .as_deref()
            .ok_or_else(|| Error::Config("HOME is not set".to_owned()))
    }

    /// An XDG base directory; the specification says to ignore relative values.
    fn base_dir(&self, configured: &Option<PathBuf>, fallback: &str) -> Result<PathBuf> {
        match configured.as_ref().filter(|dir| dir.is_absolute()) {
            Some(dir) => Ok(dir.clone()),
            None => Ok(self.home()?.join(fallback)),
        }
    }
}

/// What the translations call the notes folder; see [`Env::default_notes_dir`].
const NOTES_NAMES: [&str; 2] = ["Notes", "Notizen"];

/// The notes folder in `documents`: the one named `localized`, else one there under
/// another language's name, else `localized`, which does not exist yet.
fn default_in(documents: &Path, localized: &str) -> PathBuf {
    std::iter::once(localized)
        .chain(NOTES_NAMES)
        .map(|name| documents.join(name))
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| documents.join(localized))
}

/// The value of `key` in a `user-dirs.dirs` file: `KEY="$HOME/relative"` or `KEY="/absolute"`.
fn user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    text.lines().rev().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?;
        let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        match value.strip_prefix("$HOME") {
            Some(below_home) => Some(home.join(below_home.trim_start_matches('/'))),
            None if value.starts_with('/') => Some(PathBuf::from(value)),
            None => None,
        }
    })
}

/// FNV-1a: the same on every machine and Rust version, unlike `DefaultHasher`.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn env(home: &Path) -> Env {
        Env {
            home: Some(home.to_owned()),
            ..Env::default()
        }
    }

    #[test]
    fn falls_back_to_the_xdg_defaults() {
        let env = env(Path::new("/home/me"));
        assert_eq!(env.config_home().unwrap(), Path::new("/home/me/.config"));
        assert_eq!(env.cache_home().unwrap(), Path::new("/home/me/.cache"));
        assert_eq!(
            env.documents_dir().unwrap(),
            Path::new("/home/me/Documents")
        );
        assert_eq!(
            env.app_config_path().unwrap(),
            Path::new("/home/me/.config/sheaf/config.toml")
        );
        assert_eq!(
            env.default_notes_dir().unwrap(),
            Path::new("/home/me/Documents/Notes")
        );

        let nowhere = Env::default();
        assert!(matches!(nowhere.config_home(), Err(Error::Config(_))));
        assert!(matches!(nowhere.notes_dir(None), Err(Error::Config(_))));
    }

    #[test]
    fn honours_the_environment() {
        let env = Env {
            home: Some("/home/me".into()),
            config_home: Some("/cfg".into()),
            cache_home: Some("relative/ignored".into()),
            documents_dir: Some("/docs".into()),
            notes_dir: None,
        };
        assert_eq!(env.config_home().unwrap(), Path::new("/cfg"));
        assert_eq!(env.cache_home().unwrap(), Path::new("/home/me/.cache"));
        assert_eq!(env.default_notes_dir().unwrap(), Path::new("/docs/Notes"));
    }

    #[test]
    fn reads_the_documents_folder_from_user_dirs() {
        let home = tempfile::tempdir().unwrap();
        let env = env(home.path());
        let write = |text: &str| {
            fs::create_dir_all(home.path().join(".config")).unwrap();
            fs::write(home.path().join(".config/user-dirs.dirs"), text).unwrap();
        };

        write(
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\n",
        );
        assert_eq!(env.documents_dir().unwrap(), home.path().join("Dokumente"));
        write("XDG_DOCUMENTS_DIR=\"/mnt/daten/Dokumente\"\n");
        assert_eq!(
            env.documents_dir().unwrap(),
            Path::new("/mnt/daten/Dokumente")
        );
        write("XDG_DOCUMENTS_DIR=\"$HOME/\"\n");
        assert_eq!(env.documents_dir().unwrap(), home.path());
        write("XDG_DOCUMENTS_DIR=relative\nXDG_MUSIC_DIR=\"$HOME/Musik\"\n");
        assert_eq!(env.documents_dir().unwrap(), home.path().join("Documents"));
    }

    #[test]
    fn notes_folder_resolution_order() {
        let home = tempfile::tempdir().unwrap();
        let mut env = env(home.path());
        let config = AppConfig {
            notes_dir: Some("/from/config".into()),
            ..AppConfig::default()
        };
        AppConfig::save_changes(
            &env.app_config_path().unwrap(),
            &AppConfig::default(),
            &config,
        )
        .unwrap();

        assert_eq!(env.notes_dir(None).unwrap(), Path::new("/from/config"));
        env.notes_dir = Some("/from/env".into());
        assert_eq!(env.notes_dir(None).unwrap(), Path::new("/from/env"));
        assert_eq!(
            env.notes_dir(Some(Path::new("/from/cli"))).unwrap(),
            Path::new("/from/cli")
        );

        env.notes_dir = None;
        fs::write(
            env.app_config_path().unwrap(),
            "notes_dir = \"~/Notizen\"\n",
        )
        .unwrap();
        assert_eq!(env.notes_dir(None).unwrap(), home.path().join("Notizen"));
        fs::write(env.app_config_path().unwrap(), "notes_dir = [\n").unwrap();
        assert!(matches!(env.notes_dir(None), Err(Error::Config(_))));
        // An explicit folder works even with a broken configuration.
        assert_eq!(env.notes_dir(Some(Path::new("x"))).unwrap(), Path::new("x"));
    }

    #[test]
    fn the_first_run_records_the_default_folder() {
        let home = tempfile::tempdir().unwrap();
        let env = env(home.path());
        let notes = home.path().join("Documents/Notes");
        assert_eq!(env.notes_dir(None).unwrap(), notes);
        let recorded = AppConfig::load(&env.app_config_path().unwrap()).unwrap();
        assert_eq!(
            recorded.notes_dir.as_deref(),
            Some(Path::new("~/Documents/Notes"))
        );

        // A later run in another language finds the recorded folder, not its own name,
        // even where a folder of that name turned up.
        fs::create_dir_all(home.path().join("Documents/Notizen")).unwrap();
        assert_eq!(env.notes_dir(None).unwrap(), notes);
    }

    #[test]
    fn the_first_run_takes_a_notes_folder_of_another_language() {
        let documents = tempfile::tempdir().unwrap();
        let documents = documents.path();
        assert_eq!(default_in(documents, "Notizen"), documents.join("Notizen"));

        fs::create_dir(documents.join("Notes")).unwrap();
        assert_eq!(default_in(documents, "Notizen"), documents.join("Notes"));
        assert_eq!(default_in(documents, "Notas"), documents.join("Notes"));

        fs::create_dir(documents.join("Notizen")).unwrap();
        assert_eq!(default_in(documents, "Notizen"), documents.join("Notizen"));
        assert_eq!(default_in(documents, "Notes"), documents.join("Notes"));
    }

    #[test]
    fn the_first_run_with_an_existing_folder_records_it() {
        let home = tempfile::tempdir().unwrap();
        let env = env(home.path());
        fs::create_dir_all(home.path().join("Documents/Notizen")).unwrap();
        assert_eq!(
            env.notes_dir(None).unwrap(),
            home.path().join("Documents/Notizen")
        );
        let recorded = AppConfig::load(&env.app_config_path().unwrap()).unwrap();
        assert_eq!(
            recorded.notes_dir.as_deref(),
            Some(Path::new("~/Documents/Notizen"))
        );
    }

    #[test]
    fn index_folder_is_stable_per_notes_folder() {
        let home = tempfile::tempdir().unwrap();
        let env = Env {
            cache_home: Some("/cache".into()),
            ..env(home.path())
        };
        let notes = home.path().join("Notes");
        fs::create_dir(&notes).unwrap();
        std::os::unix::fs::symlink(&notes, home.path().join("link")).unwrap();

        let index = env.index_dir(&notes).unwrap();
        assert!(index.starts_with("/cache/sheaf/index"));
        assert_eq!(index.file_name().unwrap().len(), 16);
        assert_eq!(env.index_dir(&notes.join(".")).unwrap(), index);
        assert_eq!(env.index_dir(&home.path().join("link")).unwrap(), index);
        assert_ne!(env.index_dir(home.path()).unwrap(), index);
        assert_ne!(env.index_dir(&home.path().join("missing")).unwrap(), index);

        // Pinned so that an existing index is found again after an update.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"/home/me/Notes"), fnv1a_64(b"/home/me/Notes"));
        assert_eq!(format!("{:016x}", fnv1a_64(b"a")), "af63dc4c8601ec8c");
    }
}
