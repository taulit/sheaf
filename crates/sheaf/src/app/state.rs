//! What the app remembers between runs: `$XDG_STATE_HOME/sheaf/state.toml`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use gpui::Pixels;

use super::theme::size;

/// Size and state of the main window, as GPUI reports and takes them: the size
/// includes the shadow around a client-decorated window, so that what is saved opens
/// at the same size whoever draws the decorations. The position is not kept: Wayland
/// does not let an app place its windows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowState {
    /// Whether a window can be opened at this size.
    fn usable(self) -> bool {
        let at_least = |value: f32, min: Pixels| value.is_finite() && value >= f32::from(min);
        at_least(self.width, size::WINDOW_MIN_WIDTH)
            && at_least(self.height, size::WINDOW_MIN_HEIGHT)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    window: Option<WindowState>,
}

/// The state file. Reading never fails: a missing or broken file is no state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateFile(PathBuf);

impl StateFile {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        StateFile(path.into())
    }

    /// The file of this user: below `$XDG_STATE_HOME`, or `~/.local/state`. `None`
    /// without either, in which case nothing is remembered.
    pub fn from_process() -> Option<Self> {
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let state_home =
            var("XDG_STATE_HOME").or_else(|| Some(var("HOME")?.join(".local/state")))?;
        Some(StateFile(state_home.join("sheaf/state.toml")))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// The window as it was last time; `None` on the first run and for a file that
    /// cannot be used.
    pub fn load(&self) -> Option<WindowState> {
        let text = std::fs::read_to_string(&self.0).ok()?;
        match toml::from_str::<Document>(&text) {
            Ok(document) => document.window.filter(|window| window.usable()),
            Err(err) => {
                log::warn!("ignoring {}: {err}", self.0.display());
                None
            }
        }
    }

    pub fn save(&self, window: WindowState) -> io::Result<()> {
        let text = toml::to_string(&Document {
            window: Some(window),
        })
        .map_err(io::Error::other)?;
        if let Some(dir) = self.0.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Written beside the file and renamed, so a crash never leaves half a file.
        let partial = self.0.with_extension("toml.part");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_broken_file_is_no_state() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let file = StateFile::at(dir.path().join("sheaf/state.toml"));
        assert_eq!(file.load(), None);

        std::fs::create_dir_all(dir.path().join("sheaf")).expect("a directory");
        for broken in [
            "window = 3\n",
            "[window]\nwidth = 20.0\nheight = 700.0\n",
            "[window]\nwidth = 900.0\nheight = nan\n",
            "[window]\nwidth = 900.0\n",
            "[later]\nkey = 1\n",
        ] {
            std::fs::write(file.path(), broken).expect("a file");
            assert_eq!(file.load(), None, "{broken}");
        }
    }

    #[test]
    fn the_window_state_round_trips() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let file = StateFile::at(dir.path().join("state/sheaf/state.toml"));
        let state = WindowState {
            width: 1280.0,
            height: 800.5,
            maximized: true,
        };
        file.save(state).expect("saved");
        assert_eq!(file.load(), Some(state));
        assert!(!file.path().with_extension("toml.part").exists());

        // Keys this version does not know are ignored, `maximized` may be left out.
        std::fs::write(
            file.path(),
            "[window]\nwidth = 900.0\nheight = 600.0\n\n[later]\nkey = 1\n",
        )
        .expect("a file");
        assert_eq!(
            file.load(),
            Some(WindowState {
                width: 900.0,
                height: 600.0,
                maximized: false,
            })
        );
    }
}
