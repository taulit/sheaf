//! "Show in Files": the file manager opens the note's folder with the file selected.

use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _};

/// Asks the file manager over D-Bus (`org.freedesktop.FileManager1.ShowItems`) to show
/// `path`; without one, opens its folder.
pub fn show(path: PathBuf, cx: &mut App) {
    let shown = cx.background_spawn({
        let path = path.clone();
        async move { show_items(&path) }
    });
    cx.spawn(async move |cx| {
        if let Err(err) = shown.await {
            log::info!("no file manager over D-Bus ({err}), opening the folder");
            let folder = path.parent().map(Path::to_owned).unwrap_or(path);
            cx.update(|cx| cx.open_url(&file_uri(&folder)));
        }
    })
    .detach();
}

/// Blocks on the session bus.
fn show_items(path: &Path) -> zbus::Result<()> {
    zbus::block_on(async {
        let connection = zbus::Connection::session().await?;
        connection
            .call_method(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                Some("org.freedesktop.FileManager1"),
                "ShowItems",
                &(vec![file_uri(path)], ""),
            )
            .await?;
        Ok(())
    })
}

/// `path` as a `file://` URI, with the characters a URI cannot hold escaped.
pub(crate) fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(byte as char);
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_become_escaped_uris() {
        assert_eq!(
            file_uri(Path::new("/n/Ideen für den Garten.md")),
            "file:///n/Ideen%20f%C3%BCr%20den%20Garten.md"
        );
    }
}
