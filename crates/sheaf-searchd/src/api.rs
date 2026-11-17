//! Bus names, object paths and typed proxies. See `docs/ARCHITECTURE.md`, section D-Bus.

use std::collections::HashMap;

use zbus::Variant;

/// Well-known name owned by `sheaf-searchd`.
pub const SERVICE_NAME: &str = "io.github.taulit.Sheaf.SearchProvider";
pub const SERVICE_PATH: &str = "/io/github/taulit/Sheaf/SearchProvider";

/// Well-known name owned by the app.
pub const APP_NAME: &str = "io.github.taulit.Sheaf";
pub const APP_PATH: &str = "/io/github/taulit/Sheaf";

/// Actions of the app, called through `org.freedesktop.Application.ActivateAction`.
pub const ACTION_OPEN_NOTE: &str = "open-note";
pub const ACTION_SEARCH: &str = "search";
pub const ACTION_QUICK_NOTE: &str = "quick-note";
pub const ACTION_NEW_NOTE: &str = "new-note";

/// The `parameter` argument of `ActivateAction` for an action taking `params`.
///
/// The Desktop Entry specification allows no or one item there, so several strings
/// travel as one array of strings (`as`).
pub fn action_parameter(params: &[&str]) -> Vec<Variant<'static>> {
    if params.is_empty() {
        return Vec::new();
    }
    let strings: Vec<String> = params.iter().map(|param| (*param).to_owned()).collect();
    vec![Variant::from(strings)]
}

/// Prefix of the D-Bus error names; the suffix is `sheaf_core::Error::code` in
/// CamelCase, e.g. `io.github.taulit.Sheaf1.Error.NotFound`.
pub const ERROR_PREFIX: &str = "io.github.taulit.Sheaf1.Error";

/// One note in a result list: id, title, snippet, folder, modification time in Unix
/// seconds. D-Bus signature `(ssssx)`.
pub type NoteRow = (String, String, String, String, i64);

/// `io.github.taulit.Sheaf1`: notes for the Visor module, scripts and the CLI.
#[zbus::proxy(
    interface = "io.github.taulit.Sheaf1",
    default_service = "io.github.taulit.Sheaf.SearchProvider",
    default_path = "/io/github/taulit/Sheaf/SearchProvider"
)]
pub trait Sheaf1 {
    /// Searches with the query language of the app; best matches first.
    fn search(&self, query: &str, limit: u32) -> zbus::Result<Vec<NoteRow>>;

    /// The most recently modified notes.
    fn recent(&self, limit: u32) -> zbus::Result<Vec<NoteRow>>;

    /// Creates a note whose first line is the title and returns its id. An empty
    /// `folder` means the Quick Notes folder, `/` the notes folder itself.
    fn create(&self, text: &str, folder: &str) -> zbus::Result<String>;

    fn append(&self, id: &str, text: &str) -> zbus::Result<()>;

    /// Shows the note in the app, starting the app if needed.
    fn open(&self, id: &str) -> zbus::Result<()>;

    /// Opens the Quick Note window of the app.
    fn quick_note(&self) -> zbus::Result<()>;

    /// Returns once the index reflects every change made to the notes folder so far.
    fn sync(&self) -> zbus::Result<()>;

    /// Rebuilds the index from the notes folder.
    fn reindex(&self) -> zbus::Result<()>;

    /// The notes folder this service indexes, as an absolute path.
    #[zbus(property)]
    fn notes_dir(&self) -> zbus::Result<String>;

    /// Notes were created, changed or removed; `ids` may be empty after a full rescan.
    #[zbus(signal)]
    fn changed(&self, ids: Vec<String>) -> zbus::Result<()>;
}

/// `org.freedesktop.Application` as implemented by the app.
#[zbus::proxy(
    interface = "org.freedesktop.Application",
    default_service = "io.github.taulit.Sheaf",
    default_path = "/io/github/taulit/Sheaf"
)]
pub trait Application {
    fn activate(&self, platform_data: HashMap<&str, Variant<'_>>) -> zbus::Result<()>;

    fn open(&self, uris: &[&str], platform_data: HashMap<&str, Variant<'_>>) -> zbus::Result<()>;

    fn activate_action(
        &self,
        action_name: &str,
        parameter: &[Variant<'_>],
        platform_data: HashMap<&str, Variant<'_>>,
    ) -> zbus::Result<()>;
}
