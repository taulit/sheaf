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
//! | [`state`] | what is remembered between runs |

pub mod actions;
pub mod assets;
pub mod chrome;
pub mod gallery;
pub mod instance;
pub mod library;
mod logger;
pub mod note;
pub mod portal;
pub mod state;
pub mod theme;
pub mod window;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{App, AsyncApp, Entity, Global, GraphicalEnvironment, WindowHandle, WindowingRequest};
use gpui_component::Root;
use sheaf_core::i18n::tr;
use sheaf_core::paths::Env;

use instance::{Activation, Bus, Instance, Request};
use library::Library;
use state::StateFile;
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
    logger::init();
    let first = Activation::from_launch(launch.request);
    let (connection, requests) = match instance::start(Bus::Session, &first) {
        Ok(Instance::Forwarded) => return ExitCode::SUCCESS,
        Ok(Instance::Primary {
            connection,
            requests,
        }) => (connection, requests),
        Err(err) => return fail(&format!("{}: {err}", tr("the running app does not answer"))),
    };
    if !launch.service && !has_display() {
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

    let mut application = gpui_platform::application().with_assets(assets::Assets);
    if launch.service {
        // No connection to a display until someone asks for a window; the token that
        // comes with the request then lets that window take the focus.
        application = application.with_windowing(WindowingRequest::Headless);
    }
    application.run(move |cx| {
        gpui_component::init(cx);
        theme::init(theme::Overrides::from_process(), cx);
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
    ExitCode::SUCCESS
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

/// Brings the main window to the front, opening it if needed, and passes it `request`.
async fn show(request: Request, token: Option<String>, cx: &mut AsyncApp) {
    let headless = cx.update(|cx| cx.graphical_environment().is_none());
    if headless {
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
        if let Err(err) = present(request, cx) {
            log::error!("cannot show the window: {err:#}");
        }
    });
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
