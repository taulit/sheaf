//! A Markdown file from outside the notes folder in its own window: it is edited and
//! saved where it is, keeps its frontmatter, never becomes a note, and changes made by
//! other programs are taken or asked about.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::fs;
use std::path::{Path, PathBuf};

use fixture::{Look, Notes, open_app};
use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use gpui_component::Root;
use sheaf::app::document::{self, Banner, CHECK_INTERVAL, DocumentView, SAVE_DELAY};
use sheaf::app::instance::Request;

struct Window {
    handle: WindowHandle<Root>,
    view: Entity<DocumentView>,
    cx: &'static mut VisualTestContext,
}

fn open(path: &Path, cx: &mut TestAppContext) -> Window {
    cx.update(|cx| fixture::init_app(Look::default(), cx));
    let (handle, view) = cx.update(|cx| document::open(path, cx)).unwrap();
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    Window { handle, view, cx }
}

impl Window {
    fn text(&mut self) -> String {
        self.view
            .read_with(self.cx, |view, cx| view.editor().read(cx).markdown())
    }

    fn banner(&mut self) -> Option<Banner> {
        self.view
            .read_with(self.cx, |view, _| view.banner().cloned())
    }

    fn wait(&mut self, time: std::time::Duration) {
        self.cx.executor().advance_clock(time);
        self.cx.run_until_parked();
    }

    fn update(&mut self, change: impl FnOnce(&mut DocumentView, &mut gpui::Context<DocumentView>)) {
        let view = self.view.clone();
        self.cx.update(|_, cx| view.update(cx, change));
        self.cx.run_until_parked();
    }
}

/// A folder outside any notes folder with `name` in it.
fn file(name: &str, text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    (dir, path)
}

#[gpui::test]
async fn a_file_is_edited_where_it_is_and_keeps_its_frontmatter(cx: &mut TestAppContext) {
    let (dir, path) = file(
        "README.md",
        "---\ntitle: Projekt\n---\nEinleitung ohne Überschrift\n\n## Teil\n",
    );
    let mut window = open(&path, cx);
    assert_eq!(window.text(), "Einleitung ohne Überschrift\n\n## Teil\n");

    window.cx.simulate_keystrokes("ctrl-end");
    window.cx.simulate_input("\nMehr Text");
    window.wait(SAVE_DELAY * 2);
    let saved = fs::read_to_string(&path).unwrap();
    assert!(
        saved.starts_with("---\ntitle: Projekt\n---\nEinleitung"),
        "{saved}"
    );
    assert!(saved.contains("Mehr Text"), "{saved}");
    assert!(
        window
            .view
            .read_with(window.cx, |view, _| view.saved_shown())
    );
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["README.md"], "no copy, no temporary file, no note");
}

#[gpui::test]
async fn ctrl_s_saves_at_once_and_ctrl_w_closes(cx: &mut TestAppContext) {
    let (_dir, path) = file("plan.md", "# Plan\n");
    let window = open(&path, cx);
    window.cx.simulate_keystrokes("ctrl-end");
    window.cx.simulate_input("\nSchritt eins");
    window.cx.simulate_keystrokes("ctrl-s");
    window.cx.run_until_parked();
    assert!(fs::read_to_string(&path).unwrap().contains("Schritt eins"));

    window.cx.simulate_input(" und zwei");
    window.cx.simulate_keystrokes("ctrl-w");
    window.cx.run_until_parked();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("Schritt eins und zwei")
    );
    assert!(
        window
            .cx
            .cx
            .update(|cx| document::current(&path, cx).is_none())
    );
    let _ = window.handle;
}

#[gpui::test]
async fn closing_the_window_writes_at_once(cx: &mut TestAppContext) {
    let (_dir, path) = file("notiz.md", "Text\n");
    let window = open(&path, cx);
    window.cx.simulate_keystrokes("ctrl-end");
    window.cx.simulate_input(" dazu");
    let view = window.view.clone();
    window
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.close_now(cx)));
    // No task ran: the write happened in the call.
    assert_eq!(fs::read_to_string(&path).unwrap(), "Text dazu\n");
}

#[gpui::test]
async fn a_change_elsewhere_is_taken_or_asked_about(cx: &mut TestAppContext) {
    let (_dir, path) = file("liste.md", "eins\n");
    let mut window = open(&path, cx);

    // Nothing unsaved: the new text is shown silently.
    fs::write(&path, "eins, zwei von woanders\n").unwrap();
    window.wait(CHECK_INTERVAL * 2);
    assert_eq!(window.text(), "eins, zwei von woanders\n");
    assert_eq!(window.banner(), None);

    // Unsaved edits: the banner asks, nothing is overwritten.
    window.cx.simulate_keystrokes("ctrl-end");
    window.cx.simulate_input(" mein");
    fs::write(&path, "ganz anders, viel länger als vorher\n").unwrap();
    window.wait(SAVE_DELAY * 2);
    assert_eq!(window.banner(), Some(Banner::ChangedOnDisk));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "ganz anders, viel länger als vorher\n"
    );

    // Keep Mine writes over it.
    window.update(|view, cx| view.keep_mine(cx));
    assert_eq!(window.banner(), None);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "eins, zwei von woanders mein\n"
    );

    // Reload drops the edits.
    window.cx.simulate_input(" weg");
    fs::write(&path, "neu von woanders, auch länger\n").unwrap();
    window.wait(SAVE_DELAY * 2);
    assert_eq!(window.banner(), Some(Banner::ChangedOnDisk));
    window.update(|view, cx| view.reload_from_disk(cx));
    assert_eq!(window.banner(), None);
    assert_eq!(window.text(), "neu von woanders, auch länger\n");
}

#[gpui::test]
async fn a_deleted_file_is_written_again_on_request(cx: &mut TestAppContext) {
    let (_dir, path) = file("weg.md", "bleibt\n");
    let mut window = open(&path, cx);
    fs::remove_file(&path).unwrap();
    window.wait(CHECK_INTERVAL * 2);
    assert_eq!(window.banner(), Some(Banner::DeletedOnDisk));
    assert!(!path.exists());
    window.update(|view, cx| view.keep_mine(cx));
    assert_eq!(fs::read_to_string(&path).unwrap(), "bleibt\n");
}

#[gpui::test]
async fn a_file_gets_one_window_and_a_missing_one_says_why(cx: &mut TestAppContext) {
    let (dir, path) = file("eins.md", "eins\n");
    let window = open(&path, cx);
    let again = window.cx.cx.update(|cx| document::open(&path, cx)).unwrap();
    assert_eq!(again.1, window.view);

    let missing = open(&dir.path().join("fehlt.md"), cx);
    let error = missing
        .view
        .read_with(missing.cx, |view, _| view.error().map(str::to_owned));
    assert!(error.is_some_and(|error| error.contains("fehlt.md")));
}

#[gpui::test]
async fn a_file_outside_the_notes_folder_opens_in_its_own_window(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let before = notes.files();
    let mut app = open_app(&notes, cx);
    let (_dir, path) = file("Außerhalb.md", "# Fremd\n");
    let uri = format!("file://{}", path.display()).replace(' ', "%20");
    app.request(Request::OpenUris(vec![
        uri.replace("Außerhalb", "Au%C3%9Ferhalb"),
    ]));
    let shown = app.cx.cx.update(|cx| document::current(&path, cx));
    let (_, view) = shown.expect("a window for the file");
    let text = view.read_with(app.cx, |view, cx| view.editor().read(cx).markdown());
    assert_eq!(text, "# Fremd\n");
    assert_eq!(notes.files(), before, "no note was made of it");
}
