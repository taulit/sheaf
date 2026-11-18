//! The last save when the window closes, the app quits or is told to by SIGTERM, and
//! the signals while quitting, on the real platform without a display. GPUI runs no task while it quits, which `TestAppContext` hides: it drives
//! every task until none is left.
//!
//! An app runs once per process, so each test starts this test binary again with
//! only itself selected and the scenario in the environment; the child runs the app,
//! the parent looks at the files afterwards.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{AnyWindowHandle, App, AsyncApp, Entity, WindowHandle, WindowingRequest};
use gpui_component::Root;
use gpui_markdown_editor::Command as EditorCommand;
use sheaf::app::actions::CloseWindow;
use sheaf::app::assets::Assets;
use sheaf::app::instance::Request;
use sheaf::app::library::Library;
use sheaf::app::quick;
use sheaf::app::window::{self, MainWindow};

const SCENARIO: &str = "SHEAF_TEST_SCENARIO";
const NOTES: &str = "SHEAF_TEST_NOTES";

#[derive(Clone, Copy)]
enum Ending {
    /// "Close Window", the last window: the app quits after it.
    Close,
    Quit,
    /// SIGTERM, as at logout or from `kill`.
    Terminate,
}

#[test]
fn closing_the_window_right_after_typing_saves() {
    scenario("closing_the_window_right_after_typing_saves", Ending::Close);
}

#[test]
fn quitting_right_after_typing_saves() {
    scenario("quitting_right_after_typing_saves", Ending::Quit);
}

#[test]
fn sigterm_right_after_typing_saves() {
    scenario("sigterm_right_after_typing_saves", Ending::Terminate);
}

#[test]
fn a_second_sigterm_while_quitting_ends_at_once() {
    const NAME: &str = "a_second_sigterm_while_quitting_ends_at_once";
    if std::env::var(SCENARIO).as_deref() == Ok(NAME) {
        return hang_while_quitting();
    }
    let notes = Notes::empty();
    let status = child(NAME, &notes, Command::new(std::env::current_exe().unwrap()));
    assert_eq!(status.code(), Some(128 + 15), "ended by the second SIGTERM");
}

#[test]
fn an_ignored_sighup_stays_ignored() {
    const NAME: &str = "an_ignored_sighup_stays_ignored";
    if std::env::var(SCENARIO).as_deref() == Ok(NAME) {
        let notes = PathBuf::from(std::env::var_os(NOTES).expect("the notes folder"));
        return outlive_sighup(&notes);
    }
    let notes = Notes::empty();
    let mut nohup = Command::new("nohup");
    nohup.arg(std::env::current_exe().unwrap());
    let status = child(NAME, &notes, nohup);
    assert!(status.success(), "the app failed: {status}");
    assert!(
        notes.dir.join("outlived").exists(),
        "the app went on after SIGHUP"
    );
}

/// Runs this test binary with only `name` selected, for the scenario of that name.
fn child(name: &str, notes: &Notes, mut command: Command) -> std::process::ExitStatus {
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(SCENARIO, name)
        .env(NOTES, &notes.dir)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env("XDG_STATE_HOME", notes.scratch("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .unwrap()
}

/// Ends the child if the scenario hangs.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        eprintln!("the scenario did not end");
        std::process::exit(2);
    });
}

fn kill(signal: &str) {
    let pid = std::process::id().to_string();
    let sent = Command::new("kill").args([signal, &pid]).status();
    assert!(sent.unwrap().success());
}

/// The child: quitting hangs, as a last save on a stuck file system would; a second
/// SIGTERM comes meanwhile.
fn hang_while_quitting() {
    watchdog();
    let quitting = Arc::new(AtomicBool::new(false));
    gpui_platform::application()
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            sheaf::app::quit_on_signals(cx);
            cx.on_app_quit({
                let quitting = quitting.clone();
                move |_| {
                    quitting.store(true, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_secs(60));
                    std::future::ready(())
                }
            })
            .detach();
            std::thread::spawn(move || {
                kill("-TERM");
                while !quitting.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                kill("-TERM");
            });
        });
}

/// The child, started by `nohup`: gets SIGHUP and leaves a mark if it is still there
/// half a second later.
fn outlive_sighup(notes: &Path) {
    watchdog();
    let mark = notes.join("outlived");
    gpui_platform::application()
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            sheaf::app::quit_on_signals(cx);
            kill("-HUP");
            cx.spawn(async move |cx| {
                let timer =
                    cx.update(|cx| cx.background_executor().timer(Duration::from_millis(500)));
                timer.await;
                std::fs::write(&mark, "").unwrap();
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

fn scenario(name: &str, ending: Ending) {
    if std::env::var(SCENARIO).as_deref() == Ok(name) {
        let notes = std::env::var_os(NOTES).expect("the notes folder");
        run_app(PathBuf::from(notes), ending);
        return;
    }
    let notes = Notes::seeded();
    let status = child(name, &notes, Command::new(std::env::current_exe().unwrap()));
    assert!(status.success(), "the app failed: {status}");
    assert!(
        notes
            .read("Arbeit/Zuletzt Notch-Ideen.md")
            .contains("Zuletzt Notch-Ideen\n\nWas die Notch"),
        "{:?}",
        notes.files()
    );
}

/// The child: opens the note, types and ends the app in the same turn, before any
/// task could save.
fn run_app(notes: PathBuf, ending: Ending) {
    watchdog();
    gpui_platform::application()
        .with_assets(Assets)
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            fixture::init_app(Look::default(), cx);
            sheaf::app::quit_on_signals(cx);
            let library = Library::local(&notes, cx).unwrap();
            let (window, main) = window::open(library, None, cx).unwrap();
            cx.spawn(async move |cx| {
                type_and_end(window, main, &notes, ending, cx).await;
            })
            .detach();
        });
}

async fn type_and_end(
    window: WindowHandle<Root>,
    main: Entity<MainWindow>,
    notes: &Path,
    ending: Ending,
    cx: &mut AsyncApp,
) {
    let id = wait_for(cx, |cx| {
        let gallery = main.read(cx).gallery().read(cx);
        let note = gallery
            .notes()
            .iter()
            .find(|note| note.title == "Notch-Ideen");
        note.map(|note| note.id.clone())
    })
    .await;
    window
        .update(cx, |_, window, cx| {
            let request = Request::OpenNote {
                id: id.to_string(),
                query: String::new(),
            };
            main.update(cx, |main, cx| main.handle(request, window, cx));
        })
        .unwrap();
    let note = wait_for(cx, |cx| {
        let note = main.read(cx).note()?;
        note.read(cx).meta().is_some().then(|| note.clone())
    })
    .await;
    assert!(notes.join("Arbeit/Notch-Ideen.md").exists());
    // A headless window draws only when asked; actions go by the last frame.
    let any: AnyWindowHandle = window.into();
    cx.update(|cx| any.update(cx, |_, window, cx| window.draw(cx).clear(cx)))
        .unwrap();
    window
        .update(cx, |_, window, cx| {
            let editor = note.read(cx).editor().clone();
            editor.update(cx, |editor, cx| {
                editor.command(EditorCommand::InsertText("Zuletzt ".into()), window, cx);
            });
            match ending {
                Ending::Close => window.dispatch_action(Box::new(CloseWindow), cx),
                Ending::Quit => cx.quit(),
                Ending::Terminate => kill("-TERM"),
            }
        })
        .unwrap();
}

/// Polls `found` until it has an answer; the app's tasks run in between.
async fn wait_for<T>(cx: &mut AsyncApp, found: impl Fn(&App) -> Option<T>) -> T {
    loop {
        if let Some(found) = cx.update(|cx| found(cx)) {
            return found;
        }
        let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(10)));
        timer.await;
    }
}

#[test]
fn sigterm_right_after_typing_a_quick_note_saves() {
    const NAME: &str = "sigterm_right_after_typing_a_quick_note_saves";
    if std::env::var(SCENARIO).as_deref() == Ok(NAME) {
        let notes = PathBuf::from(std::env::var_os(NOTES).expect("the notes folder"));
        return run_quick_note(notes);
    }
    let notes = Notes::seeded();
    let status = child(NAME, &notes, Command::new(std::env::current_exe().unwrap()));
    assert!(status.success(), "the app failed: {status}");
    assert!(
        notes
            .read("Quick Notes/Schnell notiert.md")
            .ends_with("Schnell notiert\n"),
        "{:?}",
        notes.files()
    );
}

/// The child: only the Quick Note window, whose note view is never drawn; typing and
/// SIGTERM in the same turn.
fn run_quick_note(notes: PathBuf) {
    watchdog();
    gpui_platform::application()
        .with_assets(Assets)
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            fixture::init_app(Look::default(), cx);
            sheaf::app::quit_on_signals(cx);
            let library = Library::local(&notes, cx).unwrap();
            let (window, view) = quick::open(library, Rc::new(|_, _| {}), cx).unwrap();
            cx.spawn(async move |cx| {
                let note = wait_for(cx, |cx| {
                    let note = view.read(cx).note()?;
                    note.read(cx).is_loaded().then(|| note.clone())
                })
                .await;
                let any: AnyWindowHandle = window.into();
                cx.update(|cx| any.update(cx, |_, window, cx| window.draw(cx).clear(cx)))
                    .unwrap();
                window
                    .update(cx, |_, window, cx| {
                        let editor = note.read(cx).editor().clone();
                        editor.update(cx, |editor, cx| {
                            let text = EditorCommand::InsertText("Schnell notiert".into());
                            editor.command(text, window, cx);
                        });
                        kill("-TERM");
                    })
                    .unwrap();
            })
            .detach();
        });
}
