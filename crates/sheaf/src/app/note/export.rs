//! "Export as Markdown…", "Export as PDF…" and "Print…" of the share popover (plan 03
//! §9). The file is chosen in the portal's save dialog, the PDF rendered by
//! `sheaf-export` in the background; a toast tells how it went.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gpui::{App, AppContext as _, Context, Task};
use sheaf_core::i18n::tr;
use sheaf_export::{FontFile, Fonts, Options, PageSetup};

use super::{NoteEvent, NoteView, date};
use crate::app::assets::FONTS;
use crate::app::dialogs::{Filter, dialogs};
use crate::app::overlays::Toast;

/// Inter, plus a monospaced face and fallbacks found on the system. The system is
/// scanned once, on first use, off the UI thread.
fn fonts() -> &'static Fonts {
    static FONTS_OF_PDF: OnceLock<Fonts> = OnceLock::new();
    FONTS_OF_PDF.get_or_init(|| {
        let face = |index: usize| FontFile::new(FONTS[index], 0);
        Fonts::new(face(0), face(2), face(3), face(4)).with_system_fonts()
    })
}

/// What a PDF of the open note is made from.
#[derive(Clone)]
struct Source {
    body: String,
    folder: PathBuf,
    date: Option<String>,
}

impl Source {
    fn render(&self, page: PageSetup) -> Result<Vec<u8>, String> {
        let options = Options {
            page,
            base_dir: Some(&self.folder),
            date: self.date.clone(),
        };
        sheaf_export::render(&self.body, fonts(), &options)
            .map(|pdf| pdf.bytes)
            .map_err(|err| err.to_string())
    }
}

/// A file name for `title` with `extension`, without the characters file systems
/// refuse.
fn file_name(title: &str, extension: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() || matches!(c, '?' | '*' | '"' | '<' | '>' | '|') => ' ',
            c => c,
        })
        .collect();
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = stem.trim_start_matches('.');
    let stem = if stem.is_empty() {
        tr("Untitled")
    } else {
        stem.to_owned()
    };
    format!("{stem}.{extension}")
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

/// Renders `source` on A4 into a file named `name` in the temporary folder and opens
/// it with the system's PDF viewer. The viewer reads the file after this returns, so
/// it stays.
fn open_in_viewer(source: Source, name: String, cx: &mut App) {
    let path = std::env::temp_dir().join(name);
    let written = cx.background_spawn({
        let path = path.clone();
        async move { write(&path, &source.render(PageSetup::A4)?) }
    });
    cx.spawn(async move |cx| match written.await {
        Ok(()) => cx.update(|cx| cx.open_with_system(&path)),
        Err(reason) => log::error!("cannot open the PDF: {reason}"),
    })
    .detach();
}

impl NoteView {
    fn source(&self, cx: &Context<Self>) -> Source {
        let zone = self.library.read(cx).now().time_zone().clone();
        Source {
            body: self.editor.read(cx).markdown(),
            folder: self.notes_dir.join(self.folder()),
            date: self
                .meta
                .as_ref()
                .map(|meta| date::long_date_time(&meta.modified.to_zoned_datetime(zone))),
        }
    }

    /// "Export as Markdown…": the text of the note, without its frontmatter, so that
    /// the copy does not carry the note's id. Attachments stay where they are.
    pub fn export_markdown(&mut self, cx: &mut Context<Self>) {
        self.popover = None;
        let body = self.editor.read(cx).markdown();
        let name = file_name(&self.title, "md");
        let dialogs = dialogs(cx);
        let exported = cx.background_spawn(async move {
            let filter = Filter {
                label: tr("Markdown"),
                mime_type: "text/markdown".to_owned(),
            };
            let Some(path) = dialogs.save_file(&tr("Export as Markdown"), &name, &filter)? else {
                return Ok(None);
            };
            write(&path, body.as_bytes())?;
            Ok(Some(path))
        });
        self.exported(exported, cx);
        cx.notify();
    }

    /// "Export as PDF…": A4 with 20 mm margins (`docs/verified.md` §2).
    pub fn export_pdf(&mut self, cx: &mut Context<Self>) {
        self.popover = None;
        let source = self.source(cx);
        let name = file_name(&self.title, "pdf");
        let dialogs = dialogs(cx);
        let exported = cx.background_spawn(async move {
            let filter = Filter {
                label: tr("PDF"),
                mime_type: "application/pdf".to_owned(),
            };
            let Some(path) = dialogs.save_file(&tr("Export as PDF"), &name, &filter)? else {
                return Ok(None);
            };
            write(&path, &source.render(PageSetup::A4)?)?;
            Ok(Some(path))
        });
        self.exported(exported, cx);
        cx.notify();
    }

    /// "Print…": the print dialog, then the PDF for the paper chosen there. If that
    /// fails, as without a print portal, the toast offers the PDF in a viewer to print
    /// from there (`docs/verified.md` §2).
    pub fn print(&mut self, cx: &mut Context<Self>) {
        self.popover = None;
        let source = self.source(cx);
        let fallback = (source.clone(), file_name(&self.title, "pdf"));
        let title = self.title.clone();
        let dialogs = dialogs(cx);
        let printed =
            cx.background_spawn(async move { dialogs.print(&title, &|page| source.render(page)) });
        self._export = cx.spawn(async move |this, cx| {
            if let Err(reason) = printed.await {
                log::error!("cannot print: {reason}");
                let message = tr("Could not print: {}").replace("{}", &reason);
                let toast = Toast::new(message).action(tr("Open as PDF"), move |_, cx| {
                    let (source, name) = fallback.clone();
                    open_in_viewer(source, name, cx);
                });
                let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Toast(toast)));
            }
        });
        cx.notify();
    }

    /// Tells how an export went: "Exported" with "Open", or why it failed. A cancelled
    /// dialog says nothing.
    fn exported(
        &mut self,
        exported: Task<Result<Option<PathBuf>, String>>,
        cx: &mut Context<Self>,
    ) {
        self._export = cx.spawn(async move |this, cx| {
            let toast = match exported.await {
                Ok(None) => return,
                Ok(Some(path)) => Toast::new(tr("Exported")).action(tr("Open"), move |_, cx| {
                    cx.open_with_system(&path);
                }),
                Err(reason) => {
                    log::error!("cannot export: {reason}");
                    Toast::new(tr("Could not export: {}").replace("{}", &reason))
                }
            };
            let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Toast(toast)));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_leave_out_what_file_systems_refuse() {
        assert_eq!(file_name("Einkauf", "pdf"), "Einkauf.pdf");
        assert_eq!(file_name("a/b: c?", "md"), "a-b- c.md");
        assert_eq!(file_name("..  ", "md"), "Untitled.md");
    }
}
