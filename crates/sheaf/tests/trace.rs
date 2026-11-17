//! The startup trace of `SHEAF_STARTUP_TRACE`: the binary without a display draws its
//! gallery on the headless platform and writes each step of the start once, with the
//! notes of the first frame that shows cards.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fixture::Notes;

#[test]
fn the_start_is_traced_without_a_display() {
    let notes = Notes::seeded();
    let trace = notes.scratch("startup.trace");
    let mut app = Command::new(env!("CARGO_BIN_EXE_sheaf"))
        .env_clear()
        .env("HOME", notes.scratch("home"))
        .env("XDG_CONFIG_HOME", notes.scratch("config"))
        .env("XDG_STATE_HOME", notes.scratch("state"))
        .env("XDG_CACHE_HOME", notes.scratch("cache"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env("SHEAF_NOTES_DIR", &notes.dir)
        .env("SHEAF_STARTUP_TRACE", &trace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let text = loop {
        let text = std::fs::read_to_string(&trace).unwrap_or_default();
        if text.contains("\nminiatures ") || started.elapsed() > Duration::from_secs(30) {
            break text;
        }
        assert!(app.try_wait().unwrap().is_none(), "the app ended: {text}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = Command::new("kill")
        .args(["-TERM", &app.id().to_string()])
        .status();
    assert!(app.wait().unwrap().success());

    let events: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(name, _)| *name != "card")
        .map(|(name, _)| name)
        .collect();
    assert_eq!(events, ["run", "window", "cards", "miniatures"], "{text}");
    let times: Vec<u64> = text
        .lines()
        .filter(|line| !line.starts_with("card "))
        .map(|line| line.split_once(' ').unwrap().1.parse().unwrap())
        .collect();
    assert!(times.is_sorted(), "{text}");
    let cards: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("card "))
        .collect();
    // Pinned first, then the newest.
    assert_eq!(
        cards[..2],
        [
            "Privat/Einkauf.md",
            "Arbeit/Ausschreibung – offene Fragen.md"
        ]
    );
    assert!(cards.contains(&"Arbeit/Notch-Ideen.md"), "{text}");
}
