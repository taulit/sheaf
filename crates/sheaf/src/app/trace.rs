//! The startup trace, for measuring the start (`scripts/budgets.sh`).
//!
//! With `SHEAF_STARTUP_TRACE=<file>` the app appends a line `<event> <µs since the Unix
//! epoch>` to the file at each step of [`Event`], the first frame with cards followed
//! by a line `card <path>` for each note it shows. Without a display it then opens its
//! windows on the headless platform and draws them at 60 Hz itself, as a compositor
//! would ask it to, so that the start can be measured where no desktop runs. Unset, a
//! step costs one look at a static.

use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::App;

const VARIABLE: &str = "SHEAF_STARTUP_TRACE";
/// The frame interval of a 60 Hz display.
const FRAME: Duration = Duration::from_micros(16_667);

/// A step of the start, each written once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// `app::run` begins: the binary is loaded and the command line parsed.
    Run,
    /// The main window is open.
    Window,
    /// The first frame that shows cards (or rows) of notes is painted.
    Cards,
    /// The first frame in which every card drawn shows its miniature is painted.
    Miniatures,
}

impl Event {
    const ALL: [Event; 4] = [Event::Run, Event::Window, Event::Cards, Event::Miniatures];

    fn name(self) -> &'static str {
        match self {
            Event::Run => "run",
            Event::Window => "window",
            Event::Cards => "cards",
            Event::Miniatures => "miniatures",
        }
    }
}

struct Trace {
    file: Mutex<(File, [bool; Event::ALL.len()])>,
}

static TRACE: OnceLock<Option<Trace>> = OnceLock::new();

fn trace() -> Option<&'static Trace> {
    TRACE
        .get_or_init(|| {
            let path = PathBuf::from(std::env::var_os(VARIABLE)?);
            match File::options().create(true).append(true).open(&path) {
                Ok(file) => Some(Trace {
                    file: Mutex::new((file, [false; Event::ALL.len()])),
                }),
                Err(err) => {
                    log::warn!("cannot write the startup trace {}: {err}", path.display());
                    None
                }
            }
        })
        .as_ref()
}

/// Whether the trace is on and there is no display: the app is measured on the
/// headless platform and draws its windows with [`draw_headless`].
pub fn headless() -> bool {
    trace().is_some() && !super::has_display()
}

/// Whether `event` is still to be written; `false` when the trace is off.
pub fn pending(event: Event) -> bool {
    trace().is_some_and(|trace| {
        let file = trace.file.lock().unwrap_or_else(PoisonError::into_inner);
        !file.1[event as usize]
    })
}

/// Writes `event` with the time now, the first time only.
pub fn mark(event: Event) {
    mark_with(event, &[]);
}

/// [`mark`], followed by a line `card <note>` for each of `notes`.
pub fn mark_with(event: Event, notes: &[String]) {
    let Some(trace) = trace() else { return };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut file = trace.file.lock().unwrap_or_else(PoisonError::into_inner);
    let (file, written) = &mut *file;
    if std::mem::replace(&mut written[event as usize], true) {
        return;
    }
    let mut lines = format!("{} {}\n", event.name(), now.as_micros());
    for note in notes {
        lines.push_str(&format!("card {note}\n"));
    }
    if let Err(err) = file.write_all(lines.as_bytes()) {
        log::warn!("cannot write the startup trace: {err}");
    }
}

/// Draws every window at 60 Hz, for as long as the app runs: the frames a compositor
/// would ask for, on the headless platform, where nobody asks.
pub fn draw_headless(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            let timer = cx.update(|cx| cx.background_executor().timer(FRAME));
            timer.await;
            cx.update(|cx| {
                for window in cx.windows() {
                    let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                }
            });
        }
    })
    .detach();
}
