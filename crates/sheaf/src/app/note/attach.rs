//! Attachments (plan 03 §2, §7): a file dropped or pasted into the note, or chosen
//! with "Attach File…", is copied to `<folder>/_attachments/<id>/` and linked at the
//! caret, an image as an image. Files no note links to any more stay where they are.

use gpui::{AppContext as _, ClipboardEntry, Context, Window};
use gpui_markdown_editor::Command;
use sheaf_core::i18n::tr;

use super::worker::{Attached, Job, Upload};
use super::{NoteEvent, NoteView};
use crate::app::dialogs::dialogs;
use crate::app::overlays::Toast;

/// Extensions the editor shows as images.
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

/// The Markdown that links `attached`: an image for images, a link for other files.
pub fn markdown(attached: &[(String, String)]) -> String {
    attached
        .iter()
        .map(|(name, link)| {
            let extension = link.rsplit_once('.').map(|(_, ext)| ext.to_lowercase());
            let image = extension.is_some_and(|ext| IMAGES.contains(&ext.as_str()));
            let label = name.replace(['[', ']'], "");
            if image {
                format!("![{label}]({link})")
            } else {
                format!("[{label}]({link})")
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What the clipboard holds that becomes an attachment: files copied in a file
/// manager, or an image. `None` for text, which the editor pastes itself.
pub fn from_clipboard(entries: &[ClipboardEntry]) -> Option<Vec<Upload>> {
    let mut uploads = Vec::new();
    for entry in entries {
        match entry {
            ClipboardEntry::ExternalPaths(paths) => uploads.extend(
                paths
                    .paths()
                    .iter()
                    .filter(|path| path.is_file())
                    .cloned()
                    .map(Upload::File),
            ),
            ClipboardEntry::Image(image) => uploads.push(Upload::Bytes {
                name: format!("{}.{}", tr("Pasted Image"), image.format.extension()),
                bytes: image.bytes.clone(),
            }),
            ClipboardEntry::String(_) => {}
        }
    }
    (!uploads.is_empty()).then_some(uploads)
}

impl NoteView {
    /// Copies `files` into the note's attachment folder and links them at the caret.
    pub fn attach(&mut self, files: Vec<Upload>, window: &mut Window, cx: &mut Context<Self>) {
        self.popover = None;
        if self.closed || files.is_empty() {
            return;
        }
        let (done, attached) = async_channel::bounded(1);
        self.send(Job::Attach { files, done });
        self._attach = cx.spawn_in(window, async move |this, cx| {
            let Ok(attached) = attached.recv().await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| this.link(attached, window, cx));
        });
        cx.notify();
    }

    /// "Attach File…": files chosen in the portal's file chooser.
    pub fn attach_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popover = None;
        let dialogs = dialogs(cx);
        let chosen = cx.background_spawn(async move { dialogs.open_files(&tr("Attach File")) });
        self._attach = cx.spawn_in(window, async move |this, cx| {
            let files = match chosen.await {
                Ok(files) => files,
                Err(reason) => {
                    log::error!("cannot choose a file: {reason}");
                    let message = tr("Could not attach: {}").replace("{}", &reason);
                    let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Toast(Toast::new(message))));
                    return;
                }
            };
            let files = files.into_iter().map(Upload::File).collect();
            let _ = this.update_in(cx, |this, window, cx| this.attach(files, window, cx));
        });
        cx.notify();
    }

    /// Ctrl+V: attachments from the clipboard if it holds files or an image, else the
    /// editor's own paste.
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let uploads = cx
            .read_from_clipboard()
            .and_then(|item| from_clipboard(item.entries()));
        match uploads {
            Some(uploads) => self.attach(uploads, window, cx),
            None => match cx.build_action("markdown_editor::PasteClipboard", None) {
                Ok(paste) => window.dispatch_action(paste, cx),
                Err(err) => log::error!("the editor has no paste action: {err}"),
            },
        }
    }

    fn link(&mut self, attached: Attached, window: &mut Window, cx: &mut Context<Self>) {
        if !attached.links.is_empty() {
            let markdown = markdown(&attached.links);
            self.editor.update(cx, |editor, cx| {
                editor.command(Command::InsertMarkdown(markdown), window, cx);
            });
            // Some files went in and some did not: say which did not. If none did,
            // the worker has said so.
            for reason in attached.failed {
                let message = tr("Could not attach: {}").replace("{}", &reason);
                cx.emit(NoteEvent::Toast(Toast::new(message)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_link_as_images_and_files_as_links() {
        let links = [
            ("Foto.JPG".to_owned(), "_attachments/x/Foto.JPG".to_owned()),
            (
                "Bericht [v2].pdf".to_owned(),
                "_attachments/x/Bericht-v2-.pdf".to_owned(),
            ),
        ];
        assert_eq!(
            markdown(&links),
            "![Foto.JPG](_attachments/x/Foto.JPG)\n\n[Bericht v2.pdf](_attachments/x/Bericht-v2-.pdf)"
        );
    }
}
