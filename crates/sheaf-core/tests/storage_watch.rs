//! The watcher against real file operations in a temporary folder.

// Helpers outside `#[test]` functions are not covered by `allow-unwrap-in-tests`.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use sheaf_core::watch::{MAX_DELAY, QUIET_PERIOD};
use sheaf_core::{Change, CreateOptions, Error, NoteStore, WatchHandle, WriteOptions, watch};
use tempfile::TempDir;

/// Far longer than a batch ever takes; only a failing test waits this long.
const PATIENCE: Duration = Duration::from_secs(10);

struct Watched {
    dir: TempDir,
    batches: Receiver<Vec<Change>>,
    handle: WatchHandle,
}

impl Watched {
    /// A watched folder, prepared by `setup` before the watcher starts.
    fn new(setup: impl FnOnce(&Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path());
        let (sender, batches) = mpsc::channel();
        let handle = watch(dir.path(), move |batch| {
            let _ = sender.send(batch);
        })
        .unwrap();
        Watched {
            dir,
            batches,
            handle,
        }
    }

    fn path(&self, rel_path: &str) -> std::path::PathBuf {
        self.dir.path().join(rel_path)
    }

    fn next(&self) -> Vec<Change> {
        self.batches
            .recv_timeout(PATIENCE)
            .expect("a batch of changes")
    }

    /// Batches until every expected change has arrived; fails on any other change.
    fn collect(&self, expected: &[Change]) {
        let expected: BTreeSet<String> = expected
            .iter()
            .map(|change| format!("{change:?}"))
            .collect();
        let mut seen = BTreeSet::new();
        while seen != expected {
            for change in self.next() {
                let change = format!("{change:?}");
                assert!(
                    expected.contains(&change),
                    "unexpected {change}, wanted {expected:?}"
                );
                seen.insert(change);
            }
        }
    }

    fn assert_quiet(&self) {
        let pause = MAX_DELAY + QUIET_PERIOD;
        assert_eq!(
            self.batches.recv_timeout(pause),
            Err(RecvTimeoutError::Timeout)
        );
    }
}

fn upsert(rel_path: &str) -> Change {
    Change::Upsert(rel_path.to_owned())
}

fn remove(rel_path: &str) -> Change {
    Change::Remove(rel_path.to_owned())
}

#[test]
fn reports_create_modify_rename_and_delete() {
    let watched = Watched::new(|root| fs::write(root.join("alt.md"), "Alt\n").unwrap());

    fs::write(watched.path("neu.md"), "Neu\n").unwrap();
    assert_eq!(watched.next(), [upsert("neu.md")]);

    fs::write(watched.path("alt.md"), "Alt\ngeändert\n").unwrap();
    assert_eq!(watched.next(), [upsert("alt.md")]);

    fs::rename(watched.path("alt.md"), watched.path("umbenannt.md")).unwrap();
    assert_eq!(watched.next(), [remove("alt.md"), upsert("umbenannt.md")]);

    fs::remove_file(watched.path("neu.md")).unwrap();
    assert_eq!(watched.next(), [remove("neu.md")]);
    watched.assert_quiet();
}

#[test]
fn ignores_what_is_not_a_note() {
    let watched = Watched::new(|_| {});
    for rel_path in [
        ".versteckt.md",
        "notizen.txt",
        "bild.png",
        ".trash/geloescht.md",
        ".sheaf/config.toml",
        ".sheaf/templates/vorlage.md",
        ".git/objects/ab/cdef.md",
        "_attachments/01JD/liesmich.md",
        "Ordner/_attachments/01JD/liesmich.md",
        "Ordner/.obsidian/workspace.md",
    ] {
        let path = watched.path(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x").unwrap();
        fs::write(&path, "y").unwrap();
    }
    fs::rename(watched.path("notizen.txt"), watched.path("notizen.bak")).unwrap();
    fs::remove_file(watched.path("bild.png")).unwrap();
    fs::remove_dir_all(watched.path("_attachments")).unwrap();
    fs::remove_dir_all(watched.path(".git")).unwrap();
    watched.assert_quiet();

    fs::write(watched.path("Ordner/echt.md"), "Echt\n").unwrap();
    assert_eq!(watched.next(), [upsert("Ordner/echt.md")]);
    watched.assert_quiet();
}

#[test]
fn store_writes_show_up_without_their_temporary_files() {
    let watched = Watched::new(|_| {});
    let store = NoteStore::new(watched.dir.path());

    let meta = store
        .create("Titel\nText\n", &CreateOptions::default())
        .unwrap();
    assert_eq!(watched.next(), [upsert("Titel.md")]);

    store.append(&meta.id, "mehr").unwrap();
    assert_eq!(watched.next(), [upsert("Titel.md")]);

    let rename = WriteOptions {
        rename: true,
        expected: None,
    };
    store
        .write_body(&meta.id, "Neuer Titel\nText\n", &rename)
        .unwrap();
    assert_eq!(
        watched.next(),
        [remove("Titel.md"), upsert("Neuer Titel.md")]
    );

    store.add_attachment(&meta.id, "foto.png", b"png").unwrap();
    watched.assert_quiet();

    store.move_to(&meta.id, "Archiv").unwrap();
    watched.collect(&[remove("Neuer Titel.md"), upsert("Archiv/Neuer Titel.md")]);

    store.trash(&meta.id, Timestamp::now()).unwrap();
    assert_eq!(watched.next(), [remove("Archiv/Neuer Titel.md")]);

    store.restore(&meta.id).unwrap();
    assert_eq!(watched.next(), [upsert("Archiv/Neuer Titel.md")]);
    watched.assert_quiet();
}

#[test]
fn notes_in_a_new_folder_are_found() {
    let watched = Watched::new(|_| {});
    fs::create_dir_all(watched.path("Neu/Tief")).unwrap();
    fs::write(watched.path("Neu/Tief/a.md"), "A\n").unwrap();
    fs::write(watched.path("Neu/b.md"), "B\n").unwrap();
    watched.collect(&[upsert("Neu/Tief/a.md"), upsert("Neu/b.md")]);

    fs::write(watched.path("Neu/Tief/c.md"), "C\n").unwrap();
    assert_eq!(watched.next(), [upsert("Neu/Tief/c.md")]);
    watched.assert_quiet();
}

#[test]
fn folder_moves_ask_for_a_rescan() {
    let watched = Watched::new(|root| {
        fs::create_dir_all(root.join("Arbeit/Projekt")).unwrap();
        fs::write(root.join("Arbeit/Projekt/plan.md"), "Plan\n").unwrap();
    });

    fs::rename(watched.path("Arbeit"), watched.path("Büro")).unwrap();
    assert_eq!(watched.next(), [Change::Rescan]);

    // The renamed folder is still watched.
    fs::write(watched.path("Büro/Projekt/plan.md"), "Plan\nneu\n").unwrap();
    assert_eq!(watched.next(), [upsert("Büro/Projekt/plan.md")]);

    let outside = tempfile::tempdir().unwrap();
    fs::rename(watched.path("Büro/Projekt"), outside.path().join("Projekt")).unwrap();
    assert_eq!(watched.next(), [Change::Rescan]);

    fs::rename(outside.path().join("Projekt"), watched.path("Zurück")).unwrap();
    assert_eq!(watched.next(), [Change::Rescan]);

    fs::remove_dir_all(watched.path("Zurück")).unwrap();
    assert_eq!(watched.next(), [Change::Rescan]);
    watched.assert_quiet();
}

#[test]
fn a_burst_is_one_batch_and_a_stream_is_cut_off() {
    let watched = Watched::new(|_| {});

    let start = Instant::now();
    for n in 0..20 {
        fs::write(watched.path(&format!("n{n:02}.md")), "x").unwrap();
    }
    let batch = watched.next();
    assert!(
        start.elapsed() >= QUIET_PERIOD,
        "delivered after {:?}",
        start.elapsed()
    );
    let expected: Vec<Change> = (0..20).map(|n| upsert(&format!("n{n:02}.md"))).collect();
    assert_eq!(batch, expected);

    // Changes that never pause for the quiet period are delivered anyway.
    let path = watched.path("strom.md");
    let stream = Duration::from_millis(1500);
    let writer = thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < stream {
            fs::write(&path, "x").unwrap();
            thread::sleep(Duration::from_millis(20));
        }
    });
    let start = Instant::now();
    assert_eq!(watched.next(), [upsert("strom.md")]);
    let waited = start.elapsed();
    assert!(
        waited >= MAX_DELAY.saturating_sub(Duration::from_millis(40)),
        "{waited:?}"
    );
    assert!(waited < stream - Duration::from_millis(300), "{waited:?}");
    writer.join().unwrap();
}

#[test]
fn dropping_the_handle_stops_the_watcher() {
    let Watched {
        dir,
        batches,
        handle,
    } = Watched::new(|_| {});
    fs::write(dir.path().join("vorher.md"), "x").unwrap();
    assert_eq!(
        batches.recv_timeout(PATIENCE).unwrap(),
        [upsert("vorher.md")]
    );

    drop(handle);
    fs::write(dir.path().join("nachher.md"), "x").unwrap();
    // The thread is gone, and with it the callback that held the sender.
    assert_eq!(
        batches.recv_timeout(PATIENCE),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn a_missing_folder_cannot_be_watched() {
    let dir = tempfile::tempdir().unwrap();
    let result = watch(&dir.path().join("fehlt"), |_| {});
    assert!(
        matches!(result, Err(Error::NotFound(_) | Error::Io(_))),
        "{result:?}"
    );
}
