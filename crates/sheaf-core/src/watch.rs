//! Watches the notes folder and reports what other programs (and Sheaf itself) change,
//! in debounced batches.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::store::{ATTACHMENTS_DIR, NOTE_EXT};

/// A batch is delivered once nothing has happened for this long.
pub const QUIET_PERIOD: Duration = Duration::from_millis(150);

/// A batch is delivered at the latest this long after its first event.
pub const MAX_DELAY: Duration = Duration::from_millis(300);

/// One thing that changed in the notes folder. Paths are relative and `/`-separated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    /// The note file is new or has changed.
    Upsert(String),
    /// The note file is gone. A rename is a `Remove` and an `Upsert`.
    Remove(String),
    /// Too much changed to say what: events were lost, or a folder was moved, renamed
    /// or removed. Compare the whole folder with what is known.
    Rescan,
}

/// Keeps a [`watch`] running; dropping it stops the watcher and its thread.
#[derive(Debug)]
pub struct WatchHandle {
    stop: Sender<Message>,
    thread: Option<JoinHandle<()>>,
    _watcher: RecommendedWatcher,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Message {
    Event(notify::Result<notify::Event>),
    Stop,
}

/// Watches `root` with everything below it and calls `on_batch` from a background
/// thread with the changes to note files.
///
/// Hidden entries, `_attachments` and files that are not notes are ignored, and with
/// them the temporary files of atomic writes. A change is reported as what the file is
/// at delivery time: `Upsert` if it exists, `Remove` if not; removals come first in a
/// batch. The thread sleeps while nothing changes.
pub fn watch(
    root: &Path,
    on_batch: impl FnMut(Vec<Change>) + Send + 'static,
) -> Result<WatchHandle> {
    let (sender, receiver) = mpsc::channel();
    let events = sender.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = events.send(Message::Event(event));
    })
    .map_err(|err| watch_error(err, root))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|err| watch_error(err, root))?;

    let tracker = Tracker::new(root);
    let thread = thread::Builder::new()
        .name("sheaf-watch".to_owned())
        .spawn(move || run(tracker, &receiver, on_batch))?;
    Ok(WatchHandle {
        stop: sender,
        thread: Some(thread),
        _watcher: watcher,
    })
}

fn watch_error(err: notify::Error, root: &Path) -> Error {
    match err.kind {
        notify::ErrorKind::Io(err) => Error::Io(err),
        notify::ErrorKind::PathNotFound => Error::NotFound(root.display().to_string()),
        _ => Error::Io(io::Error::other(err)),
    }
}

fn run(mut tracker: Tracker, messages: &Receiver<Message>, mut on_batch: impl FnMut(Vec<Change>)) {
    loop {
        // Blocks without a timeout, so a quiet folder costs no wakeups.
        let Ok(Message::Event(event)) = messages.recv() else {
            return;
        };
        let mut batch = Batch::default();
        tracker.apply(event, &mut batch);
        if batch.is_empty() {
            continue;
        }
        let deadline = Instant::now() + MAX_DELAY;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match messages.recv_timeout(QUIET_PERIOD.min(left)) {
                Ok(Message::Event(event)) => tracker.apply(event, &mut batch),
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        let changes = tracker.finish(batch);
        if !changes.is_empty() {
            on_batch(changes);
        }
    }
}

/// What happened since the last delivery.
#[derive(Default)]
struct Batch {
    /// Note paths that were touched, with the position of their last event.
    notes: HashMap<String, usize>,
    events: usize,
    /// Folders that appeared; their content is looked at on delivery.
    new_folders: Vec<PathBuf>,
    rescan: bool,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.new_folders.is_empty() && !self.rescan
    }

    fn touch(&mut self, rel_path: String) {
        self.events += 1;
        self.notes.insert(rel_path, self.events);
    }
}

/// Turns file system events into batches. Knows the folders below the root, because
/// the event for a folder that was moved away does not say that it was a folder.
struct Tracker {
    root: PathBuf,
    folders: HashSet<PathBuf>,
}

impl Tracker {
    fn new(root: &Path) -> Self {
        let mut tracker = Tracker {
            root: root.to_owned(),
            folders: HashSet::new(),
        };
        tracker.scan(root, None);
        tracker
    }

    fn apply(&mut self, event: notify::Result<notify::Event>, batch: &mut Batch) {
        let event = match event {
            Ok(event) if !event.need_rescan() => event,
            Ok(_) => {
                batch.rescan = true;
                return;
            }
            Err(err) => {
                log::warn!("watching {}: {err}", self.root.display());
                batch.rescan = true;
                return;
            }
        };
        for path in &event.paths {
            match event.kind {
                EventKind::Create(CreateKind::Folder) => {
                    if self.visible(path).is_some() {
                        batch.new_folders.push(path.clone());
                    }
                }
                EventKind::Create(_)
                | EventKind::Modify(
                    ModifyKind::Data(_)
                    | ModifyKind::Metadata(_)
                    | ModifyKind::Any
                    | ModifyKind::Other,
                )
                | EventKind::Access(AccessKind::Close(AccessMode::Write)) => {
                    self.touch(path, batch);
                }
                // Each end of a rename is judged by what is there now.
                EventKind::Modify(ModifyKind::Name(_)) if path.is_dir() => {
                    batch.rescan |= self.visible(path).is_some();
                }
                EventKind::Modify(ModifyKind::Name(_)) | EventKind::Remove(_) => {
                    if self.folders.contains(path) {
                        batch.rescan = true;
                    } else {
                        self.touch(path, batch);
                    }
                }
                _ => {}
            }
        }
    }

    fn touch(&self, path: &Path, batch: &mut Batch) {
        if let Some(rel_path) = self.visible(path).filter(|rel| rel.ends_with(NOTE_EXT)) {
            batch.touch(rel_path);
        }
    }

    fn finish(&mut self, mut batch: Batch) -> Vec<Change> {
        if batch.rescan {
            self.folders.clear();
            let root = self.root.clone();
            self.scan(&root, None);
            return vec![Change::Rescan];
        }
        // Files written into a new folder before it was watched raised no events.
        for folder in std::mem::take(&mut batch.new_folders) {
            self.scan(&folder, Some(&mut batch));
        }
        let mut notes: Vec<(String, usize)> = batch.notes.into_iter().collect();
        notes.sort_by_key(|(_, position)| *position);
        let (present, gone): (Vec<_>, Vec<_>) = notes
            .into_iter()
            .map(|(rel_path, _)| rel_path)
            .partition(|rel_path| self.root.join(rel_path).is_file());
        // Removals first, so that a rename reads as "gone there, new here".
        let gone = gone.into_iter().map(Change::Remove);
        gone.chain(present.into_iter().map(Change::Upsert))
            .collect()
    }

    /// Records `dir` and the folders below it, and the notes in them if a batch is given.
    fn scan(&mut self, dir: &Path, mut batch: Option<&mut Batch>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        self.folders.insert(dir.to_owned());
        for entry in entries.flatten() {
            let path = entry.path();
            if self.visible(&path).is_none() {
                continue;
            }
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => self.scan(&path, batch.as_deref_mut()),
                Ok(file_type) if file_type.is_file() => {
                    if let Some(batch) = batch.as_deref_mut() {
                        self.touch(&path, batch);
                    }
                }
                _ => {}
            }
        }
    }

    /// The path relative to the root, unless a part of it is hidden or `_attachments`.
    fn visible(&self, path: &Path) -> Option<String> {
        let mut parts = Vec::new();
        for part in path.strip_prefix(&self.root).ok()?.components() {
            let name = part.as_os_str().to_str()?;
            if name.starts_with('.') || name == ATTACHMENTS_DIR {
                return None;
            }
            parts.push(name);
        }
        Some(parts.join("/"))
    }
}
