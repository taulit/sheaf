//! The desktop's dialogs, through the portals (`docs/verified.md` §2): where to save a
//! file, which files to open, which folder to use, and printing.
//!
//! Views ask [`dialogs`] and run the call on the background executor: every call
//! blocks until the user has answered. Tests and screenshots set [`Dialogs`] as a
//! global with a [`Scripted`] backend, so that no dialog ever opens there.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use ashpd::desktop::print::{
    Orientation, OutputFileFormat, PageSetup as PortalPageSetup, PreparePrintOptions, PrintOptions,
    PrintProxy, Settings,
};
use gpui::{App, Global};
use sheaf_export::PageSetup;

/// A file type a save or open dialog offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub label: String,
    pub mime_type: String,
}

/// The page setup the user chose and renders a PDF for it; an error says why not.
pub type Renderer<'a> = &'a (dyn Fn(PageSetup) -> Result<Vec<u8>, String> + Sync);

/// What answers the dialogs. Every method blocks until the user has answered; a
/// cancelled dialog is `Ok(None)`, `Ok(Vec::new())` or `Ok(false)`.
pub trait Backend: Send + Sync {
    /// Where to save a file named `name` at first.
    fn save_file(
        &self,
        title: &str,
        name: &str,
        filter: &Filter,
    ) -> Result<Option<PathBuf>, String>;
    /// Files to open.
    fn open_files(&self, title: &str) -> Result<Vec<PathBuf>, String>;
    /// A folder.
    fn choose_folder(&self, title: &str) -> Result<Option<PathBuf>, String>;
    /// Shows the print dialog, renders for the paper the user chose and prints.
    fn print(&self, title: &str, render: Renderer<'_>) -> Result<bool, String>;
}

/// The dialogs of the app.
#[derive(Clone)]
pub struct Dialogs(Arc<dyn Backend>);

impl std::fmt::Debug for Dialogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dialogs")
    }
}

impl Global for Dialogs {}

impl Dialogs {
    pub fn new(backend: impl Backend + 'static) -> Self {
        Dialogs(Arc::new(backend))
    }

    pub fn save_file(
        &self,
        title: &str,
        name: &str,
        filter: &Filter,
    ) -> Result<Option<PathBuf>, String> {
        self.0.save_file(title, name, filter)
    }

    pub fn open_files(&self, title: &str) -> Result<Vec<PathBuf>, String> {
        self.0.open_files(title)
    }

    pub fn choose_folder(&self, title: &str) -> Result<Option<PathBuf>, String> {
        self.0.choose_folder(title)
    }

    pub fn print(&self, title: &str, render: Renderer<'_>) -> Result<bool, String> {
        self.0.print(title, render)
    }
}

/// The dialogs set for this app, or those of the portals.
pub fn dialogs(cx: &App) -> Dialogs {
    cx.try_global::<Dialogs>()
        .cloned()
        .unwrap_or_else(|| Dialogs::new(Portal))
}

/// The XDG desktop portals.
#[derive(Clone, Copy, Debug)]
pub struct Portal;

fn failed(err: ashpd::Error) -> String {
    err.to_string()
}

fn cancelled(err: &ashpd::Error) -> bool {
    matches!(err, ashpd::Error::Response(ResponseError::Cancelled))
}

/// The local paths of the `file://` URIs the file chooser answered with.
fn paths(files: &SelectedFiles) -> Vec<PathBuf> {
    files
        .uris()
        .iter()
        .filter_map(|uri| uri.as_str().strip_prefix("file://"))
        .map(|path| PathBuf::from(super::library::decode_file_uri(&format!("file://{path}"))))
        .collect()
}

impl Backend for Portal {
    fn save_file(
        &self,
        title: &str,
        name: &str,
        filter: &Filter,
    ) -> Result<Option<PathBuf>, String> {
        let filter = FileFilter::new(&filter.label).mimetype(&filter.mime_type);
        let answer = zbus::block_on(async {
            SelectedFiles::save_file()
                .title(title)
                .modal(true)
                .current_name(name)
                .filter(filter)
                .send()
                .await?
                .response()
        });
        match answer {
            Ok(files) => Ok(paths(&files).into_iter().next()),
            Err(err) if cancelled(&err) => Ok(None),
            Err(err) => Err(failed(err)),
        }
    }

    fn open_files(&self, title: &str) -> Result<Vec<PathBuf>, String> {
        let answer = zbus::block_on(async {
            SelectedFiles::open_file()
                .title(title)
                .modal(true)
                .multiple(true)
                .send()
                .await?
                .response()
        });
        match answer {
            Ok(files) => Ok(paths(&files)),
            Err(err) if cancelled(&err) => Ok(Vec::new()),
            Err(err) => Err(failed(err)),
        }
    }

    fn choose_folder(&self, title: &str) -> Result<Option<PathBuf>, String> {
        let answer = zbus::block_on(async {
            SelectedFiles::open_file()
                .title(title)
                .modal(true)
                .directory(true)
                .send()
                .await?
                .response()
        });
        match answer {
            Ok(files) => Ok(paths(&files).into_iter().next()),
            Err(err) if cancelled(&err) => Ok(None),
            Err(err) => Err(failed(err)),
        }
    }

    fn print(&self, title: &str, render: Renderer<'_>) -> Result<bool, String> {
        zbus::block_on(async {
            let proxy = PrintProxy::new().await.map_err(failed)?;
            let a4 = PageSetup::A4;
            let setup = PortalPageSetup::default()
                .set_name("iso_a4")
                .set_width(f64::from(a4.width))
                .set_height(f64::from(a4.height))
                .set_margin_top(f64::from(a4.margin_top))
                .set_margin_right(f64::from(a4.margin_right))
                .set_margin_bottom(f64::from(a4.margin_bottom))
                .set_margin_left(f64::from(a4.margin_left))
                .set_orientation(Orientation::Portrait);
            let prepared = proxy
                .prepare_print(
                    None,
                    title,
                    Settings::default().set_paper_format("iso_a4"),
                    setup,
                    PreparePrintOptions::default()
                        .set_modal(true)
                        .set_supported_output_file_formats([OutputFileFormat::Pdf]),
                )
                .await
                .map_err(failed)?
                .response();
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(err) if cancelled(&err) => return Ok(false),
                Err(err) => return Err(failed(err)),
            };
            let pdf = render(page_setup(&prepared.page_setup))?;
            let file = TemporaryFile::write(&pdf).map_err(|err| err.to_string())?;
            let opened = std::fs::File::open(file.path()).map_err(|err| err.to_string())?;
            let printed = proxy
                .print(
                    None,
                    title,
                    &opened,
                    PrintOptions::default()
                        .set_token(prepared.token)
                        .set_modal(true)
                        .set_supported_output_file_formats([OutputFileFormat::Pdf]),
                )
                .await
                .map_err(failed)?
                .response();
            match printed {
                Ok(()) => Ok(true),
                Err(err) if cancelled(&err) => Ok(false),
                Err(err) => Err(failed(err)),
            }
        })
    }
}

/// The paper and margins the print dialog answered with; A4 for what it left out.
fn page_setup(setup: &PortalPageSetup) -> PageSetup {
    let a4 = PageSetup::A4;
    let mm = |value: Option<f64>, default: f32| value.map_or(default, |value| value as f32);
    let (width, height) = (mm(setup.width, a4.width), mm(setup.height, a4.height));
    let landscape = matches!(
        setup.orientation,
        Some(Orientation::Landscape | Orientation::ReverseLandscape)
    );
    let (width, height) = if landscape == (width < height) {
        (height, width)
    } else {
        (width, height)
    };
    PageSetup {
        width,
        height,
        margin_top: mm(setup.margin_top, a4.margin_top),
        margin_right: mm(setup.margin_right, a4.margin_right),
        margin_bottom: mm(setup.margin_bottom, a4.margin_bottom),
        margin_left: mm(setup.margin_left, a4.margin_left),
    }
}

/// A file that is removed when dropped: the PDF handed to the print portal.
struct TemporaryFile(PathBuf);

impl TemporaryFile {
    fn write(bytes: &[u8]) -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let name = format!("sheaf-print-{}-{nanos}.pdf", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes)?;
        Ok(TemporaryFile(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Answers the dialogs with what a test or the screenshots set up, and records what
/// was printed. Without an answer set, a dialog counts as cancelled.
#[derive(Debug, Default)]
pub struct Scripted {
    pub saves: Mutex<VecDeque<PathBuf>>,
    pub opens: Mutex<VecDeque<Vec<PathBuf>>>,
    pub folders: Mutex<VecDeque<PathBuf>>,
    /// The page setup the print dialog answers with; `None` cancels.
    pub paper: Mutex<Option<PageSetup>>,
    /// Why printing fails, as without a print portal; checked before `paper`.
    pub print_error: Mutex<Option<String>>,
    /// Every PDF printed.
    pub printed: Mutex<Vec<Vec<u8>>>,
    /// The titles of the dialogs asked for, in order.
    pub asked: Mutex<Vec<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Backend for Arc<Scripted> {
    fn save_file(&self, title: &str, _: &str, _: &Filter) -> Result<Option<PathBuf>, String> {
        lock(&self.asked).push(title.to_owned());
        Ok(lock(&self.saves).pop_front())
    }

    fn open_files(&self, title: &str) -> Result<Vec<PathBuf>, String> {
        lock(&self.asked).push(title.to_owned());
        Ok(lock(&self.opens).pop_front().unwrap_or_default())
    }

    fn choose_folder(&self, title: &str) -> Result<Option<PathBuf>, String> {
        lock(&self.asked).push(title.to_owned());
        Ok(lock(&self.folders).pop_front())
    }

    fn print(&self, title: &str, render: Renderer<'_>) -> Result<bool, String> {
        lock(&self.asked).push(title.to_owned());
        if let Some(reason) = lock(&self.print_error).clone() {
            return Err(reason);
        }
        let Some(paper) = *lock(&self.paper) else {
            return Ok(false);
        };
        let pdf = render(paper)?;
        lock(&self.printed).push(pdf);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landscape_paper_is_turned() {
        let setup = PortalPageSetup::default()
            .set_width(210.)
            .set_height(297.)
            .set_margin_top(10.)
            .set_orientation(Orientation::Landscape);
        let page = page_setup(&setup);
        assert_eq!((page.width, page.height), (297., 210.));
        assert_eq!(page.margin_top, 10.);
        assert_eq!(page.margin_left, 20.);
        assert_eq!(page_setup(&PortalPageSetup::default()), PageSetup::A4);
    }
}
