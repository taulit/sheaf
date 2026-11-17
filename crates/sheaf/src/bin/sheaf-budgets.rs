//! The measurements behind `scripts/budgets.sh`, which sets up the private bus and the
//! temporary folders they run in (`docs/testing.md`, "Budgets"):
//!
//! ```text
//! sheaf-budgets generate <dir> <count> <cover-percent>
//! sheaf-budgets start <sheaf> <runs>
//! sheaf-budgets scroll <dir>
//! sheaf-budgets searchd <sheaf-searchd> <notes>
//! ```
//!
//! Each measurement prints what it saw to stderr and one tab-separated row per budget
//! to stdout: name, target, measured, `pass` or `FAIL`.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyWindowHandle, App, AsyncApp, Modifiers, PlatformInput, ScrollDelta, ScrollEvent, TouchPhase,
    WindowingRequest, point, px,
};
use sheaf::app::assets::Assets;
use sheaf::app::library::Library;
use sheaf::app::{actions, theme, window};
use sheaf_core::testgen;
use sheaf_searchd::api::Sheaf1Proxy;

/// A 60 Hz frame.
const FRAME: Duration = Duration::from_micros(16_667);
const START_BUDGET: Duration = Duration::from_millis(250);
/// In bytes; a megabyte is 10⁶ bytes, the stricter reading of the budgets.
const APP_MEMORY_BUDGET: u64 = 120_000_000;
const SEARCHD_MEMORY_BUDGET: u64 = 15_000_000;
/// How long the app waits, once its gallery is complete, before its memory is read.
const SETTLE: Duration = Duration::from_secs(2);
/// How long the service waits after the last request before its memory is read.
const SEARCHD_IDLE: Duration = Duration::from_secs(10);

type Outcome = Result<bool, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match measure(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("sheaf-budgets: {err}");
            ExitCode::from(2)
        }
    }
}

fn measure(args: &[&str]) -> Outcome {
    match args {
        ["generate", dir, count, covers] => {
            generate(Path::new(dir), count.parse()?, covers.parse()?)?;
            Ok(true)
        }
        ["start", sheaf, runs] => start(Path::new(sheaf), runs.parse()?),
        ["scroll", dir] => scroll(Path::new(dir)),
        ["searchd", binary, notes] => searchd(Path::new(binary), notes),
        _ => Err("usage: see the documentation of crates/sheaf/src/bin/sheaf-budgets.rs".into()),
    }
}

/// Prints the row of one budget and returns whether it was met.
fn row(name: &str, target: &str, measured: String, met: bool) -> bool {
    println!(
        "{name}\t{target}\t{measured}\t{}",
        if met { "pass" } else { "FAIL" }
    );
    met
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1e6
}

// ---- Notes ----------------------------------------------------------------------

/// Writes `count` generated notes below `dir`; `covers` percent of them start with an
/// image, a photo-sized JPEG, as notes with a cover do.
fn generate(dir: &Path, count: usize, covers: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut notes = testgen::notes(count);
    // Eight different photos; every cover is a hard link to one of them, so each note
    // has its own file to decode and the folder stays small.
    let pool = dir.join(".covers");
    fs::create_dir_all(&pool)?;
    let photos: Vec<PathBuf> = (0..8)
        .map(|seed| {
            let path = pool.join(format!("photo-{seed}.jpg"));
            photo(seed).save(&path).map(|()| path)
        })
        .collect::<Result<_, _>>()?;
    let every = (100 / covers.max(1)).max(1);
    for (index, note) in notes.iter_mut().enumerate() {
        if covers == 0 || index % every != 0 {
            continue;
        }
        let source = format!("_attachments/cover-{index}/cover.jpg");
        let file = dir.join(&note.meta.folder).join(&source);
        fs::create_dir_all(file.parent().ok_or("a cover has no folder")?)?;
        fs::hard_link(&photos[(index / every) % photos.len()], &file)?;
        let (title, rest) = note.body.split_once('\n').unwrap_or((&note.body, ""));
        note.body = format!("{title}\n\n![Cover]({source})\n{rest}");
        note.meta.has_image = true;
    }
    testgen::write_notes(dir, &notes)?;
    Ok(())
}

/// A 1600×1067 picture with detail everywhere, so that it compresses like a photo.
fn photo(seed: u32) -> image::RgbImage {
    image::RgbImage::from_fn(1600, 1067, |x, y| {
        let noise = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ seed)
            .wrapping_mul(2_654_435_761)
            >> 27;
        let channel = |base: u32| ((base + noise) % 256) as u8;
        image::Rgb([
            channel(x / 7 + seed * 40),
            channel(y / 5 + seed * 20),
            channel((x + y) / 11),
        ])
    })
}

// ---- Start and memory of the app --------------------------------------------------

/// What one start of the app took, from the moment it was spawned.
struct Start {
    run: Duration,
    window: Duration,
    cards: Duration,
    miniatures: Duration,
    rss: u64,
    pss: u64,
}

/// Starts the app `runs` times without a display, with the startup trace on, and
/// measures each start and the memory once its gallery is complete.
fn start(sheaf: &Path, runs: usize) -> Outcome {
    let trace = std::env::temp_dir().join(format!("sheaf-startup-{}.trace", std::process::id()));
    let mut starts = Vec::new();
    for run in 1..=runs {
        let _ = fs::remove_file(&trace);
        let spawned = SystemTime::now();
        let mut child = Command::new(sheaf)
            .env("SHEAF_STARTUP_TRACE", &trace)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .spawn()?;
        let measured = measure_start(&mut child, &trace, spawned);
        stop(&mut child)?;
        let start = measured?;
        eprintln!(
            "start {run}: run {:.1} ms, window {:.1} ms, cards {:.1} ms, miniatures {:.1} ms; \
             RSS {:.1} MB, PSS {:.1} MB",
            millis(start.run),
            millis(start.window),
            millis(start.cards),
            millis(start.miniatures),
            megabytes(start.rss),
            megabytes(start.pss),
        );
        starts.push(start);
    }
    let _ = fs::remove_file(&trace);
    let (first, warm) = starts.split_first().ok_or("no runs")?;
    if warm.is_empty() {
        return Err("at least two runs: the first one is not counted".into());
    }
    let median = |value: fn(&Start) -> Duration| {
        let mut values: Vec<Duration> = warm.iter().map(value).collect();
        values.sort_unstable();
        values[values.len() / 2]
    };
    let cards = median(|start| start.cards);
    let miniatures = median(|start| start.miniatures);
    let mut rss: Vec<u64> = warm.iter().map(|start| start.rss).collect();
    rss.sort_unstable();
    let rss = rss[rss.len() / 2];
    let mut pss: Vec<u64> = warm.iter().map(|start| start.pss).collect();
    pss.sort_unstable();
    let pss = pss[pss.len() / 2];
    let started = row(
        "cold start to gallery, 1,000 notes",
        "< 250 ms",
        format!(
            "{:.0} ms with miniatures, cards {:.0} ms (median of {}; first run {:.0} ms)",
            millis(miniatures),
            millis(cards),
            warm.len(),
            millis(first.miniatures),
        ),
        miniatures < START_BUDGET,
    );
    let small = row(
        "memory, 1,000 notes, gallery open",
        "< 120 MB RSS",
        format!("{:.1} MB RSS, {:.1} MB PSS", megabytes(rss), megabytes(pss)),
        rss < APP_MEMORY_BUDGET,
    );
    Ok(started && small)
}

fn measure_start(child: &mut Child, trace: &Path, spawned: SystemTime) -> Result<Start, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let events = loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Err(format!("the app ended early: {status}"));
        }
        let text = fs::read_to_string(trace).unwrap_or_default();
        if text.lines().any(|line| line.starts_with("miniatures ")) {
            break text;
        }
        if Instant::now() > deadline {
            return Err(format!("no complete gallery after 30 s; trace: {text:?}"));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let since_spawn = |name: &str| -> Result<Duration, String> {
        let micros: u64 = events
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(' ')?.parse().ok())
            .ok_or_else(|| format!("no {name} in the trace"))?;
        let at = UNIX_EPOCH + Duration::from_micros(micros);
        at.duration_since(spawned)
            .map_err(|_| format!("{name} before the spawn"))
    };
    std::thread::sleep(SETTLE);
    let (rss, pss) = memory(child.id()).map_err(|err| err.to_string())?;
    Ok(Start {
        run: since_spawn("run")?,
        window: since_spawn("window")?,
        cards: since_spawn("cards")?,
        miniatures: since_spawn("miniatures")?,
        rss,
        pss,
    })
}

/// VmRSS and the proportional set size of process `pid`, in bytes.
fn memory(pid: u32) -> std::io::Result<(u64, u64)> {
    let kilobytes = |text: &str, key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map_or(0, |kb| kb * 1024)
    };
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let rollup = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))?;
    Ok((kilobytes(&status, "VmRSS:"), kilobytes(&rollup, "Pss:")))
}

/// Ends `child` with SIGTERM, as a logout would, and waits for it.
fn stop(child: &mut Child) -> std::io::Result<()> {
    if child.try_wait()?.is_none() {
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait()?.is_none() {
            if Instant::now() > deadline {
                child.kill()?;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

// ---- Scrolling --------------------------------------------------------------------

/// Pixels per frame and frames: reading, scrolling on, flinging.
const SCROLL_SCRIPT: &[(f32, usize)] = &[(30., 240), (150., 240), (600., 120)];

/// Scrolls the gallery of the notes in `dir` on the headless platform at 60 Hz and
/// times each frame on the UI thread.
fn scroll(dir: &Path) -> Outcome {
    let frames: Rc<RefCell<Result<Vec<Duration>, String>>> = Rc::new(RefCell::new(Ok(Vec::new())));
    let notes = dir.to_owned();
    let result = frames.clone();
    gpui_platform::application()
        .with_assets(Assets)
        .with_windowing(WindowingRequest::Headless)
        .run(move |cx| {
            gpui_component::init(cx);
            theme::init(theme::Overrides::default(), cx);
            actions::bind_keys(cx);
            cx.spawn(async move |cx| {
                let measured = scroll_gallery(&notes, cx).await;
                *result.borrow_mut() = measured;
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    let totals = frames.replace(Ok(Vec::new()))?;
    let mut phases = totals.as_slice();
    for (distance, count) in SCROLL_SCRIPT {
        let (phase, rest) = phases.split_at(*count);
        phases = rest;
        let [p50, p95, max] = percentiles(phase, [50, 95, 100]);
        eprintln!(
            "scroll {distance} px a frame: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            millis(p50),
            millis(p95),
            millis(max),
        );
    }
    let [p50, p95, p99, max] = percentiles(&totals, [50, 95, 99, 100]);
    let over = totals.iter().filter(|total| **total >= FRAME).count();
    eprintln!(
        "scroll: {} frames, p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms, {over} over \
         16.7 ms",
        totals.len(),
        millis(p50),
        millis(p95),
        millis(p99),
        millis(max),
    );
    Ok(row(
        "gallery scrolling, 10,000 notes",
        "60 fps (CPU frame < 16.7 ms)",
        format!(
            "CPU frame p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms; {over} of {} over",
            millis(p50),
            millis(p95),
            millis(max),
            totals.len(),
        ),
        p95 < FRAME,
    ))
}

/// The given percentiles of `samples`.
fn percentiles<const N: usize>(samples: &[Duration], percents: [usize; N]) -> [Duration; N] {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    percents.map(|percent| sorted[(sorted.len() * percent).div_ceil(100).max(1) - 1])
}

async fn scroll_gallery(notes: &Path, cx: &mut AsyncApp) -> Result<Vec<Duration>, String> {
    let (window, main) = cx.update(|cx| {
        let library = Library::local(notes, cx).map_err(|err| err.to_string())?;
        window::open(library, None, cx).map_err(|err| err.to_string())
    })?;
    let window: AnyWindowHandle = window.into();
    let gallery = cx.update(|cx| main.read(cx).gallery().clone());
    let deadline = Instant::now() + Duration::from_secs(120);
    // The notes are indexed in memory first, which takes a while for 10,000.
    while cx.update(|cx| gallery.read(cx).notes().len()) < 10_000 {
        if Instant::now() > deadline {
            return Err("the gallery did not list the 10,000 notes".to_owned());
        }
        frame(window, cx, None).await?;
    }
    // A second of frames for the first miniatures and covers.
    for _ in 0..60 {
        frame(window, cx, None).await?;
    }
    let center = cx.update(|cx| {
        window
            .update(cx, |_, window, _| window.viewport_size())
            .map_err(|err| err.to_string())
    })?;
    let at = point(center.width / 2., center.height / 2.);
    let mut times = Vec::new();
    for (distance, count) in SCROLL_SCRIPT {
        for _ in 0..*count {
            let event = PlatformInput::Scroll(ScrollEvent {
                position: at,
                delta: ScrollDelta::Pixels(point(px(0.), px(-distance))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                ..ScrollEvent::default()
            });
            times.push(frame(window, cx, Some(event)).await?);
        }
    }
    let top = cx.update(|cx| gallery.read(cx).list_state().logical_scroll_top().item_ix);
    if top < 100 {
        return Err(format!("the gallery did not scroll: row {top} at the top"));
    }
    Ok(times)
}

/// One frame: the event, if any, then the drawing, then the rest of the frame for the
/// work the app does in between. Returns the time of the event and the drawing.
async fn frame(
    window: AnyWindowHandle,
    cx: &mut AsyncApp,
    event: Option<PlatformInput>,
) -> Result<Duration, String> {
    let started = Instant::now();
    let drawn = cx.update(|cx: &mut App| {
        window.update(cx, |_, window, cx| {
            if let Some(event) = event {
                window.dispatch_event(event, cx);
            }
            window.draw(cx).clear(cx);
        })
    });
    drawn.map_err(|err| err.to_string())?;
    let time = started.elapsed();
    let rest = FRAME.saturating_sub(time);
    let timer = cx.update(|cx| cx.background_executor().timer(rest));
    timer.await;
    Ok(time)
}

// ---- The service ------------------------------------------------------------------

/// Starts `sheaf-searchd` on the session bus, lets it catch up with the notes folder,
/// searches with the benchmark's queries and measures its memory once it is idle.
/// `notes` says how many notes the folder has, for the row.
fn searchd(binary: &Path, notes: &str) -> Outcome {
    let mut child = Command::new(binary).stdin(Stdio::null()).spawn()?;
    let searched = zbus::block_on(async {
        let connection = zbus::Connection::session().await?;
        let proxy = Sheaf1Proxy::new(&connection).await?;
        let deadline = Instant::now() + Duration::from_secs(60);
        while proxy.sync().await.is_err() {
            if Instant::now() > deadline {
                return Err(zbus::Error::Failure(
                    "the service did not answer".to_owned(),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut hits = 0;
        for (_, queries) in testgen::QUERIES {
            for query in *queries {
                hits += proxy.search(query, 50).await?.len();
            }
        }
        Ok(hits)
    });
    let measured = searched.map_err(|err| err.to_string()).and_then(|hits| {
        eprintln!(
            "searchd: {hits} hits; idle for {} s",
            SEARCHD_IDLE.as_secs()
        );
        std::thread::sleep(SEARCHD_IDLE);
        memory(child.id()).map_err(|err| err.to_string())
    });
    stop(&mut child)?;
    let (rss, pss) = measured?;
    Ok(row(
        &format!("sheaf-searchd idle memory, {notes} notes"),
        "< 15 MB",
        format!("{:.1} MB RSS, {:.1} MB PSS", megabytes(rss), megabytes(pss)),
        rss < SEARCHD_MEMORY_BUDGET,
    ))
}
