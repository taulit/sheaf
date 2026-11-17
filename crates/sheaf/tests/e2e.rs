//! End-to-end scenario 6 of plan 05 §3, "a change made elsewhere", through the whole
//! chain: a private D-Bus session, the real `sheaf-searchd` watching the folder, the
//! app's Library following it, and the open note on the real headless platform. `sed`
//! changes the file of an open checklist: without unsaved edits the note reloads
//! silently, with them the banner asks.
//!
//! Run by `scripts/e2e.sh`, which builds `sheaf-searchd` and names it in
//! `SHEAF_E2E_SEARCHD`; ignored otherwise. As in `shutdown.rs`, the app runs in a
//! child process: an app runs once per process.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fixture::{Look, Notes};
use gpui::{AnyWindowHandle, App, AsyncApp, WindowingRequest};
use gpui_markdown_editor::Command as EditorCommand;
use sheaf::app::assets::Assets;
use sheaf::app::instance::Request;
use sheaf::app::library::{Library, LibraryMode};
use sheaf::app::note::Banner;
use sheaf::app::window;
use sheaf_core::paths::Env;

const SCENARIO: &str = "SHEAF_TEST_SCENARIO";
const NOTE: &str = "Einkauf.md";
const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// Ends the processes when the test is done, also when it fails, the last started
/// first: SIGTERM lets the service finish a write before the folder goes.
struct Reaper(Vec<Child>);

impl Drop for Reaper {
    fn drop(&mut self) {
        for child in self.0.iter_mut().rev() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

#[test]
#[ignore = "run by scripts/e2e.sh"]
fn an_open_checklist_follows_a_change_made_elsewhere() {
    if std::env::var(SCENARIO).is_ok() {
        return follow_changes(PathBuf::from(std::env::var_os("SHEAF_NOTES_DIR").unwrap()));
    }
    let searchd = std::env::var_os("SHEAF_E2E_SEARCHD")
        .expect("SHEAF_E2E_SEARCHD names the sheaf-searchd binary; run scripts/e2e.sh");
    let notes = Notes::empty();
    notes.write(NOTE, "Einkauf\n\n- [ ] Milch\n- [ ] Brot\n");
    std::fs::write(notes.scratch("bus.conf"), BUS_CONFIG).unwrap();
    let mut processes = Reaper(Vec::new());
    let mut bus = Command::new("dbus-daemon")
        .arg(format!(
            "--config-file={}",
            notes.scratch("bus.conf").display()
        ))
        .args(["--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut address = String::new();
    BufReader::new(bus.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    processes.0.push(bus);
    let environment = |command: &mut Command| {
        command
            .env_clear()
            .env("HOME", notes.scratch("home"))
            .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
            .env("SHEAF_NOTES_DIR", &notes.dir)
            .env("XDG_CACHE_HOME", notes.scratch("cache"))
            .env("XDG_CONFIG_HOME", notes.scratch("config"))
            .env("XDG_STATE_HOME", notes.scratch("state"))
            .stdin(Stdio::null());
    };
    let mut service = Command::new(searchd);
    environment(&mut service);
    processes.0.push(service.spawn().unwrap());

    let mut app = Command::new(std::env::current_exe().unwrap());
    environment(&mut app);
    let status = app
        .args([
            "an_open_checklist_follows_a_change_made_elsewhere",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env(SCENARIO, "1")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "the app failed: {status}");
}

/// The child: opens the checklist and lets `sed` change it twice.
fn follow_changes(notes: PathBuf) {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        eprintln!("the scenario did not end");
        std::process::exit(2);
    });
    gpui_platform::application()
        .with_assets(Assets)
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            fixture::init_app(Look::default(), cx);
            let connection = zbus::block_on(zbus::Connection::session()).unwrap();
            let index = Env::from_process().index_dir(&notes).unwrap();
            let library = Library::open(&notes, Some(index), Some(connection), cx).unwrap();
            cx.spawn(async move |cx| {
                let passed = run_scenario(library, &notes, cx).await;
                cx.update(|cx| cx.quit());
                if let Err(failure) = passed {
                    eprintln!("FAIL: {failure}");
                    std::process::exit(1);
                }
            })
            .detach();
        });
}

async fn run_scenario(
    library: gpui::Entity<Library>,
    notes: &Path,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    wait_for(cx, "the service", |cx| {
        (library.read(cx).mode() == LibraryMode::Service).then_some(())
    })
    .await?;
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .map_err(|err| err.to_string())?;
    let id = wait_for(cx, "the note in the gallery", |cx| {
        let gallery = main.read(cx).gallery().read(cx);
        let note = gallery.notes().iter().find(|note| note.title == "Einkauf");
        note.map(|note| note.id.to_string())
    })
    .await?;
    window
        .update(cx, |_, window, cx| {
            let request = Request::OpenNote {
                id,
                query: String::new(),
            };
            main.update(cx, |main, cx| main.handle(request, window, cx));
        })
        .map_err(|err| err.to_string())?;
    let note = wait_for(cx, "the note to load", |cx| {
        let note = main.read(cx).note()?;
        note.read(cx).is_loaded().then(|| note.clone())
    })
    .await?;
    let text = |cx: &App| note.read(cx).editor().read(cx).markdown();
    let banner = |cx: &App| note.read(cx).banner().cloned();

    sed(&notes.join(NOTE), "s/- \\[ \\] Milch/- [x] Milch/")?;
    wait_for(cx, "the silent reload", |cx| {
        text(cx).contains("- [x] Milch").then_some(())
    })
    .await?;
    if let Some(banner) = cx.update(|cx| banner(cx)) {
        return Err(format!("a banner after the silent reload: {banner:?}"));
    }

    // A headless window draws only when asked; typing goes by the last frame.
    let any: AnyWindowHandle = window.into();
    cx.update(|cx| any.update(cx, |_, window, cx| window.draw(cx).clear(cx)))
        .map_err(|err| err.to_string())?;
    window
        .update(cx, |_, window, cx| {
            let editor = note.read(cx).editor().clone();
            editor.update(cx, |editor, cx| {
                editor.command(EditorCommand::InsertText("Heute: ".into()), window, cx);
            });
        })
        .map_err(|err| err.to_string())?;
    sed(&notes.join(NOTE), "s/Brot/Brötchen/")?;
    wait_for(cx, "the banner", |cx| {
        (banner(cx) == Some(Banner::ChangedOnDisk)).then_some(())
    })
    .await?;
    let kept = cx.update(|cx| text(cx));
    if !kept.contains("Heute: ") {
        return Err(format!("the edits are gone: {kept:?}"));
    }
    Ok(())
}

fn sed(file: &Path, script: &str) -> Result<(), String> {
    let status = Command::new("sed")
        .arg("-i")
        .arg(script)
        .arg(file)
        .status()
        .map_err(|err| err.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("sed failed: {status}"))
}

/// Polls `found` until it has an answer, for at most 10 s; the app's tasks run in
/// between.
async fn wait_for<T>(
    cx: &mut AsyncApp,
    what: &str,
    found: impl Fn(&App) -> Option<T>,
) -> Result<T, String> {
    let started = Instant::now();
    loop {
        if let Some(found) = cx.update(|cx| found(cx)) {
            return Ok(found);
        }
        if started.elapsed() > Duration::from_secs(10) {
            return Err(format!("no {what} after 10 s"));
        }
        let timer = cx.update(|cx| cx.background_executor().timer(Duration::from_millis(20)));
        timer.await;
    }
}
