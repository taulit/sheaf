//! Finding and reaching `sheaf-searchd`, shared by the command line and the app.

use std::path::{Path, PathBuf};

use sheaf_searchd::api::Sheaf1Proxy;
use zbus::Connection;
use zbus::proxy::CacheProperties;

const BINARY: &str = "sheaf-searchd";

/// The proxy of `sheaf-searchd` if it answers and serves `notes_dir`. Asking starts the
/// service through D-Bus activation where its service file is installed.
pub(crate) async fn service_for<'c>(
    conn: &'c Connection,
    notes_dir: &Path,
) -> zbus::Result<Option<Sheaf1Proxy<'c>>> {
    let proxy = Sheaf1Proxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let served = proxy.notes_dir().await?;
    Ok(same_dir(Path::new(&served), notes_dir).then_some(proxy))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    canonical(a) == canonical(b)
}

/// `sheaf-searchd` next to this executable, else in `PATH`.
pub(crate) fn binary() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(BINARY)));
    let in_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join(BINARY));
    beside
        .into_iter()
        .chain(in_path)
        .find(|path| path.is_file())
}
