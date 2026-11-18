//! "Export as Markdown…", "Export as PDF…" and "Print…" of the share popover (plan 03
//! §9), and "Export" of the gallery's context menu. The file, or for several notes the
//! folder, is chosen in the portal's dialog, the PDF rendered by `sheaf-export` in the
//! background; a toast tells how it went.
//!
//! A PDF opened in a viewer after printing failed is written to [`ViewerFiles`]: a
//! folder only the user can read, emptied when the app starts and quits.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gpui::{App, AppContext as _, Context, Global};
use jiff::tz::TimeZone;
use sheaf_core::Note;
use sheaf_core::i18n::tr;
use sheaf_export::{FontFile, Fonts, Options, PageSetup};

use super::{NoteEvent, NoteView, date};
use crate::app::assets::FONTS;
use crate::app::dialogs::{Dialogs, Filter, dialogs};
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

/// What the export of a note is made from.
#[derive(Clone, Debug)]
pub struct Source {
    title: String,
    body: String,
    /// The note's folder, which its images are relative to.
    folder: PathBuf,
    date: Option<String>,
}

impl Source {
    /// The note as stored in its file.
    pub fn of(note: &Note, notes_dir: &Path, zone: &TimeZone) -> Self {
        Source {
            title: note.meta.title.clone(),
            body: note.body().to_owned(),
            folder: notes_dir.join(&note.meta.folder),
            date: Some(date::long_date_time(
                &note.meta.modified.to_zoned_datetime(zone.clone()),
            )),
        }
    }

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

/// What an export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// The text without its frontmatter, so that the copy does not carry the note's
    /// id. Attachments stay where they are.
    Markdown,
    /// A4 with 20 mm margins (`docs/verified.md` §2).
    Pdf,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Pdf => "pdf",
        }
    }

    fn title(self) -> String {
        match self {
            Format::Markdown => tr("Export as Markdown"),
            Format::Pdf => tr("Export as PDF"),
        }
    }

    fn bytes(self, source: &Source) -> Result<Vec<u8>, String> {
        match self {
            Format::Markdown => Ok(source.body.clone().into_bytes()),
            Format::Pdf => source.render(PageSetup::A4),
        }
    }
}

/// Exports `sources`: one into the file the save dialog names, several into the
/// folder the folder dialog names, each under its title. Blocks until the user has
/// answered. Returns where the export went, or `None` if the dialog was cancelled.
pub fn run(
    sources: &[Source],
    format: Format,
    dialogs: &Dialogs,
) -> Result<Option<PathBuf>, String> {
    if let [source] = sources {
        let filter = match format {
            Format::Markdown => Filter {
                label: tr("Markdown"),
                mime_type: "text/markdown".to_owned(),
            },
            Format::Pdf => Filter {
                label: tr("PDF"),
                mime_type: "application/pdf".to_owned(),
            },
        };
        let name = file_name(&source.title, format.extension());
        let Some(path) = dialogs.save_file(&format.title(), &name, &filter)? else {
            return Ok(None);
        };
        write(&path, &format.bytes(source)?)?;
        return Ok(Some(path));
    }
    let Some(folder) = dialogs.choose_folder(&format.title())? else {
        return Ok(None);
    };
    for source in sources {
        let path = free_path(&folder, &source.title, format.extension());
        write(&path, &format.bytes(source)?)?;
    }
    Ok(Some(folder))
}

/// A path in `folder` for `title` that no file has yet: ` 2`, ` 3`, … on a collision.
fn free_path(folder: &Path, title: &str, extension: &str) -> PathBuf {
    let first = folder.join(file_name(title, extension));
    let stem = first
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::iter::once(first)
        .chain((2..).map(|n| folder.join(format!("{stem} {n}.{extension}"))))
        .find(|path| !path.exists())
        .unwrap_or_default()
}

/// How an export went, for a toast: "Exported" with "Open", or why it failed. A
/// cancelled dialog says nothing.
pub fn toast(exported: Result<Option<PathBuf>, String>) -> Option<Toast> {
    match exported {
        Ok(None) => None,
        Ok(Some(path)) => Some(Toast::new(tr("Exported")).action(tr("Open"), move |_, cx| {
            cx.open_with_system(&path);
        })),
        Err(reason) => {
            log::error!("cannot export: {reason}");
            Some(Toast::new(
                tr("Could not export: {}").replace("{}", &reason),
            ))
        }
    }
}

/// Where PDFs wait for the viewer they were opened in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewerFiles(PathBuf);

impl Global for ViewerFiles {}

impl ViewerFiles {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        ViewerFiles(dir.into())
    }

    /// `$XDG_RUNTIME_DIR/sheaf/pdf`, else `$XDG_CACHE_HOME/sheaf/pdf` of this user.
    pub fn from_process() -> Option<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute());
        let base = match runtime {
            Some(dir) => dir,
            None => sheaf_core::paths::Env::from_process().cache_home().ok()?,
        };
        Some(ViewerFiles(base.join("sheaf/pdf")))
    }

    /// Writes `bytes` to a file called `name` in a folder of its own, both readable by
    /// the user only, and returns its path.
    pub fn write(&self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        private_dir(&self.0, true)?;
        let dir = self.0.join(ulid::Ulid::generate().to_string());
        private_dir(&dir, false)?;
        let path = dir.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(bytes)?;
        Ok(path)
    }

    /// Removes every file written here.
    pub fn clear(&self) {
        match fs::remove_dir_all(&self.0) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => {
                log::warn!("cannot remove {}: {err}", self.0.display());
            }
            _ => {}
        }
    }
}

/// Creates `dir` (with its parents if `parents`) for the user alone; an existing
/// one is made so.
fn private_dir(dir: &Path, parents: bool) -> io::Result<()> {
    DirBuilder::new()
        .recursive(parents)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Renders `source` on A4 into a file named `name` among the [`ViewerFiles`] and opens
/// it with the system's PDF viewer. The viewer reads the file after this returns, so
/// it stays until the app quits.
fn open_in_viewer(source: Source, name: String, cx: &mut App) {
    let Some(files) = cx.try_global::<ViewerFiles>().cloned() else {
        log::error!("cannot open the PDF: no folder for it");
        return;
    };
    let written = cx.background_spawn(async move {
        let bytes = source.render(PageSetup::A4)?;
        files
            .write(&name, &bytes)
            .map_err(|err| format!("{name}: {err}"))
    });
    cx.spawn(async move |cx| match written.await {
        Ok(path) => cx.update(|cx| cx.open_with_system(&path)),
        Err(reason) => log::error!("cannot open the PDF: {reason}"),
    })
    .detach();
}

impl NoteView {
    fn source(&self, cx: &Context<Self>) -> Source {
        let zone = self.library.read(cx).now().time_zone().clone();
        Source {
            title: self.title.clone(),
            body: self.editor.read(cx).markdown(),
            folder: self.notes_dir.join(self.folder()),
            date: self
                .meta
                .as_ref()
                .map(|meta| date::long_date_time(&meta.modified.to_zoned_datetime(zone))),
        }
    }

    /// "Export as Markdown…" and "Export as PDF…".
    pub fn export(&mut self, format: Format, cx: &mut Context<Self>) {
        self.popover = None;
        let source = self.source(cx);
        let dialogs = dialogs(cx);
        let exported = cx.background_spawn(async move { run(&[source], format, &dialogs) });
        self._export = cx.spawn(async move |this, cx| {
            if let Some(toast) = toast(exported.await) {
                let _ = this.update(cx, |_, cx| cx.emit(NoteEvent::Toast(toast)));
            }
        });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdfs_for_the_viewer_are_private_unique_and_cleared() {
        let root = tempfile::tempdir().unwrap();
        let files = ViewerFiles::at(root.path().join("runtime/sheaf/pdf"));
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let first = files.write("Reise.pdf", b"%PDF-1").unwrap();
        let second = files.write("Reise.pdf", b"%PDF-2").unwrap();
        assert_ne!(first, second, "a folder of its own each time");
        assert_eq!(fs::read(&first).unwrap(), b"%PDF-1");
        assert_eq!(first.file_name().unwrap(), "Reise.pdf");
        assert_eq!(mode(&root.path().join("runtime/sheaf/pdf")), 0o700);
        for pdf in [&first, &second] {
            assert_eq!(mode(pdf), 0o600);
            assert_eq!(mode(pdf.parent().unwrap()), 0o700);
        }
        files.clear();
        assert!(!root.path().join("runtime/sheaf/pdf").exists());
        files.clear();
    }

    #[test]
    fn several_notes_get_free_names_in_the_folder() {
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path();
        assert_eq!(free_path(folder, "Reise", "md"), folder.join("Reise.md"));
        fs::write(folder.join("Reise.md"), "").unwrap();
        assert_eq!(free_path(folder, "Reise", "md"), folder.join("Reise 2.md"));
        fs::write(folder.join("Reise 2.md"), "").unwrap();
        assert_eq!(free_path(folder, "Reise", "md"), folder.join("Reise 3.md"));
        assert_eq!(free_path(folder, "Reise", "pdf"), folder.join("Reise.pdf"));
    }

    #[test]
    fn file_names_leave_out_what_file_systems_refuse() {
        assert_eq!(file_name("Einkauf", "pdf"), "Einkauf.pdf");
        assert_eq!(file_name("a/b: c?", "md"), "a-b- c.md");
        assert_eq!(file_name("..  ", "md"), "Untitled.md");
    }
}
