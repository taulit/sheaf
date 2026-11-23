//! The desktop app. [`run`] is what `sheaf` without a subcommand does.
//!
//! | Module | Owns |
//! |---|---|
//! | [`instance`] | one app per session bus, requests from other launches |
//! | [`theme`] | colours, sizes, motion; follows the desktop through `portal` |
//! | [`library`] | the notes: the only data access of the views |
//! | [`actions`] | actions and key bindings |
//! | [`assets`] | embedded icons |
//! | [`chrome`] | pane, header bar and buttons the views are built from |
//! | [`window`] | the main window and its router |
//! | [`gallery`], [`note`] | the two views |
//! | [`document`] | the window of a Markdown file from outside the notes folder |
//! | [`overlays`] | toast, command palette, list of shortcuts, popover parts |
//! | [`dialogs`] | save, open and print dialogs of the portals |
//! | [`settings`] | the user's settings and their window |
//! | [`state`] | what is remembered between runs |
//! | `serial` | background jobs in order, for the writes of settings files |
//! | [`quick`] | the Quick Note window |
//! | [`shortcut`] | the global Quick Note shortcut through the portal |
//! | [`trace`] | the startup trace of `SHEAF_STARTUP_TRACE` |

pub mod actions;
pub mod assets;
pub mod chrome;
pub mod dialogs;
pub mod document;
pub mod gallery;
pub mod instance;
pub mod library;
pub mod note;
pub mod overlays;
pub mod portal;
pub mod quick;
mod serial;
pub mod settings;
pub mod shortcut;
pub mod state;
pub mod theme;
pub mod trace;
pub mod window;

use std::path::PathBuf;
use std::process::ExitCode;

use async_signal::{Signal, Signals};
use futures_util::StreamExt as _;
use gpui::{App, AsyncApp, Entity, Global, GraphicalEnvironment, WindowHandle, WindowingRequest};
use gpui_component::Root;
use sheaf_core::i18n::tr;
use sheaf_core::paths::Env;

use instance::{Activation, Bus, Instance, Request};
use library::Library;
use state::{Recovery, StateFile};
use window::MainWindow;

/// How the app was started.
#[derive(Clone, Debug)]
pub struct Launch {
    /// What to show first.
    pub request: Request,
    /// Started by D-Bus activation: show nothing until asked to.
    pub service: bool,
}

/// Starts the app, or hands `launch` to the instance that already runs. Returns when
/// the app quits.
pub fn run(launch: Launch, dir: Option<PathBuf>) -> ExitCode {
    trace::mark(trace::Event::Run);
    sheaf_core::logger::init("sheaf");
    let first = Activation::from_launch(launch.request);
    let (connection, requests) = match instance::start(Bus::Session, &first) {
        Ok(Instance::Forwarded) => return ExitCode::SUCCESS,
        Ok(Instance::Primary {
            connection,
            requests,
        }) => (connection, requests),
        Err(err) => return fail(&format!("{}: {err}", tr("the running app does not answer"))),
    };
    let traced_headless = trace::headless();
    if !launch.service && !has_display() && !traced_headless {
        return fail(&tr(
            "cannot open a window: neither WAYLAND_DISPLAY nor DISPLAY is set",
        ));
    }
    let env = Env::from_process();
    let notes_dir = match env.notes_dir(dir.as_deref()) {
        Ok(dir) => std::path::absolute(&dir).unwrap_or(dir),
        Err(err) => return fail(&err.to_string()),
    };
    let index_dir = env.index_dir(&notes_dir).ok();
    let settings = env.app_config_path().ok().map(settings::Settings::load);
    // PDFs a viewer was given last time are not needed any more.
    let viewer_files = note::ViewerFiles::from_process();
    if let Some(files) = &viewer_files {
        files.clear();
    }
    let for_the_app = viewer_files.clone();

    let mut application = gpui_platform::application().with_assets(assets::Assets);
    if launch.service || traced_headless {
        // No connection to a display until someone asks for a window; the token that
        // comes with the request then lets that window take the focus.
        application = application.with_windowing(WindowingRequest::Headless);
    }
    application.run(move |cx| {
        quit_on_signals(cx);
        if traced_headless {
            trace::draw_headless(cx);
        }
        gpui_component::init(cx);
        theme::init(theme::Overrides::default(), cx);
        if let Some(settings) = settings {
            cx.set_global(settings);
        }
        actions::bind_keys(cx);
        if let Some(connection) = &connection {
            portal::follow(connection.clone(), cx);
        }
        let library = match Library::open(&notes_dir, index_dir, connection, cx) {
            Ok(library) => library,
            Err(err) => {
                eprintln!("sheaf: {err}");
                cx.quit();
                return;
            }
        };
        if let Some(recovery) = Recovery::from_process() {
            cx.set_global(recovery);
        }
        if let Some(files) = for_the_app {
            cx.set_global(files);
        }
        cx.set_global(Session {
            library,
            state_file: StateFile::from_process(),
            window: None,
        });

        let first = (!launch.service).then_some(first);
        cx.spawn(async move |cx| {
            if let Some(activation) = first {
                // GPUI has taken the token of this launch from the environment itself.
                show(activation.request, None, cx).await;
            }
            while let Ok(activation) = requests.recv().await {
                show(activation.request, activation.token, cx).await;
            }
        })
        .detach();
    });
    if let Some(files) = &viewer_files {
        files.clear();
    }
    ExitCode::SUCCESS
}

/// Quits on SIGTERM (logout, `kill`), SIGINT (Ctrl+C) and SIGHUP (the terminal
/// closing) the way the app quits itself, with the last save of the open note.
/// Otherwise the signal would end the process before it. A second signal ends the
/// app at once, also while the last save hangs. SIGHUP is left alone if it was
/// ignored at startup, as under `nohup`.
pub fn quit_on_signals(cx: &mut App) {
    let mut wanted = vec![Signal::Term, Signal::Int];
    if !hangup_ignored() {
        wanted.push(Signal::Hup);
    }
    let mut signals = match Signals::new(wanted) {
        Ok(signals) => signals,
        Err(err) => {
            log::warn!("cannot follow the signals to quit: {err}");
            return;
        }
    };
    let (quit, quitting) = async_channel::bounded(1);
    // Its own thread: the UI thread does not take a second signal while it saves.
    let spawned = std::thread::Builder::new()
        .name("sheaf-signals".to_owned())
        .spawn(move || {
            async_io::block_on(async move {
                let Some(Ok(first)) = signals.next().await else {
                    return;
                };
                log::info!("quitting on {first:?}");
                let _ = quit.send(()).await;
                if let Some(Ok(second)) = signals.next().await {
                    log::warn!("{second:?} while quitting, ending at once");
                    std::process::exit(128 + number(second));
                }
            });
        });
    if let Err(err) = spawned {
        log::warn!("cannot follow the signals to quit: {err}");
        return;
    }
    cx.spawn(async move |cx| {
        if quitting.recv().await.is_ok() {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}

/// Whether SIGHUP was ignored when the process started. Read from
/// `/proc/self/status`, since asking the kernel directly takes `unsafe`; where that
/// cannot be read, it counts as ignored, and the app leaves SIGHUP alone.
fn hangup_ignored() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").ok();
    let ignored = status.as_deref().and_then(ignored_signals);
    ignored.is_none_or(|mask| mask & (1 << (number(Signal::Hup) - 1)) != 0)
}

/// The mask of ignored signals in the text of `/proc/<pid>/status`: bit `n - 1`
/// stands for signal `n`.
fn ignored_signals(status: &str) -> Option<u64> {
    let mask = status
        .lines()
        .find_map(|line| line.strip_prefix("SigIgn:"))?;
    u64::from_str_radix(mask.trim(), 16).ok()
}

/// The number of one of the signals the app quits on, as on Linux.
fn number(signal: Signal) -> i32 {
    match signal {
        Signal::Hup => 1,
        Signal::Int => 2,
        _ => 15,
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("sheaf: {message}");
    ExitCode::FAILURE
}

fn has_display() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// What the running app consists of.
struct Session {
    library: Entity<Library>,
    state_file: Option<StateFile>,
    window: Option<(WindowHandle<Root>, Entity<MainWindow>)>,
}

impl Global for Session {}

/// Makes `library` the one a main window opened from now on shows, after the notes
/// folder changed.
pub(crate) fn use_library(library: Entity<Library>, cx: &mut App) {
    if cx.has_global::<Session>() {
        cx.global_mut::<Session>().library = library;
    }
}

/// Brings the main window to the front, opening it if needed, and passes it `request`.
async fn show(request: Request, token: Option<String>, cx: &mut AsyncApp) {
    let headless = cx.update(|cx| cx.graphical_environment().is_none());
    if headless && !trace::headless() {
        let environment = GraphicalEnvironment {
            activation_token: token,
            ..GraphicalEnvironment::detect()
        };
        let connecting =
            cx.update(|cx| cx.request_windowing(WindowingRequest::Windowed(environment)));
        if let Err(err) = connecting.await {
            log::warn!("no display to show the window on: {err:#}");
        }
    }
    cx.update(|cx| {
        let shown = match request {
            // Only its own small window, not the main one (plan 03 §8).
            Request::QuickNote => open_quick_note(cx),
            Request::OpenUris(uris) => open_uris(uris, cx),
            request => present(request, cx),
        };
        if let Err(err) = shown {
            log::error!("cannot show the window: {err:#}");
        }
    });
}

/// Opens the Quick Note window; what it asks of the main window opens that.
fn open_quick_note(cx: &mut App) -> anyhow::Result<()> {
    let library = cx.global::<Session>().library.clone();
    let on_request: quick::RequestHandler = std::rc::Rc::new(|request, cx| {
        if let Err(err) = present(request, cx) {
            log::error!("cannot show the window: {err:#}");
        }
    });
    quick::open(library, on_request, cx).map(|_| ())
}

/// Opens files: notes in the main window, any other Markdown file in a window of its
/// own, edited where it is.
fn open_uris(uris: Vec<String>, cx: &mut App) -> anyhow::Result<()> {
    let notes_dir = cx
        .global::<Session>()
        .library
        .read(cx)
        .notes_dir()
        .to_owned();
    let notes_dir = notes_dir.canonicalize().unwrap_or(notes_dir);
    let mut notes = Vec::new();
    for uri in uris {
        if uri.contains("://") && !uri.starts_with("file://") {
            log::warn!("cannot open {uri}: not a local file");
            continue;
        }
        let path = PathBuf::from(library::decode_file_uri(&uri));
        let path = path.canonicalize().unwrap_or(path);
        if path.starts_with(&notes_dir) {
            notes.push(uri);
        } else {
            document::open(&path, cx)?;
        }
    }
    if notes.is_empty() {
        return Ok(());
    }
    present(Request::OpenUris(notes), cx)
}

fn present(request: Request, cx: &mut App) -> anyhow::Result<()> {
    let open = cx.global::<Session>().window.clone();
    let alive = open.filter(|(handle, _)| handle.update(cx, |_, _, _| ()).is_ok());
    let (handle, main) = match alive {
        Some(window) => window,
        None => {
            let session = cx.global::<Session>();
            let (library, state_file) = (session.library.clone(), session.state_file.clone());
            let window = window::open(library, state_file, cx)?;
            trace::mark(trace::Event::Window);
            cx.global_mut::<Session>().window = Some(window.clone());
            window
        }
    };
    cx.activate(true);
    handle.update(cx, |_, window, cx| {
        window.activate_window();
        main.update(cx, |main, cx| main.handle(request, window, cx));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_signals_are_read_from_the_status() {
        let status = "Name:\tsheaf\nSigBlk:\t0000000000000000\nSigIgn:\t0000000000001001\nSigCgt:\t0000000180004002\n";
        let mask = ignored_signals(status);
        assert_eq!(mask, Some(0x1001));
        assert_eq!(ignored_signals("Name:\tsheaf\n"), None);
        assert_eq!(ignored_signals("SigIgn:\tzz\n"), None);
    }
}
