//! The Quick Note window in a test window: where its note goes, that an empty one
//! leaves nothing, that every way out stores the text, continuing the last one, and
//! moving it to another folder.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use gpui_component::Root;
use sheaf::app::instance::Request;
use sheaf::app::library::Library;
use sheaf::app::note::SAVE_DELAY;
use sheaf::app::quick::{self, Ending, QuickNoteView};
use sheaf_core::NoteId;

struct Quick {
    library: Entity<Library>,
    requests: Rc<RefCell<Vec<Request>>>,
}

/// An open Quick Note window.
struct Window {
    handle: WindowHandle<Root>,
    view: Entity<QuickNoteView>,
    cx: &'static mut VisualTestContext,
}

fn start(notes: &Notes, cx: &mut TestAppContext) -> Quick {
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(notes, cx)
    });
    cx.run_until_parked();
    Quick {
        library,
        requests: Rc::default(),
    }
}

impl Quick {
    fn open(&self, cx: &mut TestAppContext) -> Window {
        let requests = self.requests.clone();
        let on_request: quick::RequestHandler =
            Rc::new(move |request, _| requests.borrow_mut().push(request));
        let library = self.library.clone();
        let (handle, view) = cx
            .update(|cx| quick::open(library, on_request, cx))
            .unwrap();
        let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
        cx.run_until_parked();
        Window { handle, view, cx }
    }
}

impl Window {
    fn text(&mut self) -> String {
        self.view.read_with(self.cx, |view, cx| {
            let note = view.note().expect("the note is there").read(cx);
            note.editor().read(cx).markdown()
        })
    }

    fn id(&mut self) -> NoteId {
        self.view.read_with(self.cx, |view, cx| {
            view.note()
                .expect("the note is there")
                .read(cx)
                .id()
                .clone()
        })
    }

    fn folder(&mut self) -> String {
        self.view.read_with(self.cx, |view, cx| view.folder(cx))
    }

    fn end(&mut self, ending: Ending) {
        let view = self.view.clone();
        self.cx
            .update(|window, cx| view.update(cx, |view, cx| view.end(ending, window, cx)));
        self.cx.run_until_parked();
    }

    fn is_open(&mut self) -> bool {
        self.cx.cx.update(|cx| quick::current(cx).is_some())
    }
}

fn quick_files(notes: &Notes) -> Vec<String> {
    notes
        .files()
        .into_iter()
        .filter(|path| path.starts_with("Quick Notes/"))
        .collect()
}

#[gpui::test]
async fn a_quick_note_is_saved_in_the_quick_notes_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    assert_eq!(window.folder(), "Quick Notes");
    window.cx.simulate_input("Idee für morgen");
    window.cx.executor().advance_clock(SAVE_DELAY * 2);
    window.cx.run_until_parked();
    assert_eq!(quick_files(&notes), ["Quick Notes/Untitled.md"]);
    assert!(
        notes
            .read("Quick Notes/Untitled.md")
            .ends_with("Idee für morgen\n")
    );

    // Done: stored, named after the title, the window gone.
    window.cx.simulate_keystrokes("escape");
    window.cx.run_until_parked();
    assert_eq!(quick_files(&notes), ["Quick Notes/Idee für morgen.md"]);
    assert!(!window.is_open());
    assert!(quick.requests.borrow().is_empty());
}

#[gpui::test]
async fn the_folder_settings_choose_the_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(".sheaf/config.toml", "[quick_note]\nfolder = \"Eingang\"\n");
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    assert_eq!(window.folder(), "Eingang");
    window.cx.simulate_input("Anrufen");
    window.end(Ending::Done);
    assert!(notes.files().contains(&"Eingang/Anrufen.md".to_owned()));
}

#[gpui::test]
async fn an_empty_quick_note_leaves_nothing(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let before = notes.files();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.end(Ending::Done);
    assert!(!window.is_open());
    assert_eq!(notes.files(), before);
    let trash = notes.dir.join(".trash");
    assert!(!trash.exists() || std::fs::read_dir(trash).unwrap().next().is_none());
}

#[gpui::test]
async fn closing_the_window_saves_at_once(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let window = quick.open(cx);
    window.cx.simulate_input("Gleich weg");
    let view = window.view.clone();
    let closes = window
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.should_close(cx)));
    assert!(closes);
    // No task ran: the save happened in the call.
    assert_eq!(quick_files(&notes), ["Quick Notes/Gleich weg.md"]);
    assert!(
        notes
            .read("Quick Notes/Gleich weg.md")
            .ends_with("Gleich weg\n")
    );
}

#[gpui::test]
async fn open_shows_the_note_in_the_main_window(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Für später");
    let id = window.id();
    window.cx.simulate_keystrokes("ctrl-enter");
    window.cx.run_until_parked();
    assert!(!window.is_open());
    assert_eq!(
        *quick.requests.borrow(),
        [Request::OpenNote {
            id: id.to_string(),
            query: String::new()
        }]
    );
    assert_eq!(quick_files(&notes), ["Quick Notes/Für später.md"]);
}

#[gpui::test]
async fn the_last_quick_note_is_continued_for_five_minutes(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Eins");
    let first = window.id();
    window.end(Ending::Done);

    let mut window = quick.open(cx);
    assert_eq!(window.id(), first);
    assert_eq!(window.text(), "Eins\n");
    // Opening it again while it is open brings the same window back.
    let again = quick.open(cx);
    assert_eq!(again.handle, window.handle);
    window.end(Ending::Done);

    quick.library.update(cx, |library, _| {
        library.set_clock(Arc::new(|| {
            fixture::now()
                .checked_add(Duration::from_secs(6 * 60))
                .unwrap()
        }));
    });
    let mut window = quick.open(cx);
    assert_ne!(window.id(), first);
    assert_eq!(window.text(), "");
    window.end(Ending::Done);
    assert_eq!(quick_files(&notes), ["Quick Notes/Eins.md"]);
}

#[gpui::test]
async fn the_folder_menu_moves_the_note(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Nach Privat");
    let view = window.view.clone();
    view.update(window.cx, |view, cx| view.toggle_folder_menu(cx));
    window.cx.run_until_parked();
    assert!(view.read_with(window.cx, |view, _| view.folder_menu_open()));
    // Esc closes the menu first, not the window.
    window.cx.simulate_keystrokes("escape");
    assert!(!view.read_with(window.cx, |view, _| view.folder_menu_open()));
    assert!(window.is_open());

    view.update(window.cx, |view, cx| view.move_to("Privat", cx));
    window.cx.run_until_parked();
    assert_eq!(window.folder(), "Privat");
    window.end(Ending::Done);
    assert!(notes.files().contains(&"Privat/Nach Privat.md".to_owned()));
}

#[gpui::test]
async fn an_unusable_quick_notes_folder_puts_the_note_at_the_top(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(".sheaf/config.toml", "[quick_note]\nfolder = \"../x\"\n");
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Gerettet");
    window.end(Ending::Done);
    assert!(!window.is_open());
    let saved: Vec<String> = notes
        .files()
        .into_iter()
        .filter(|path| !path.contains('/') && notes.read(path).contains("Gerettet"))
        .collect();
    assert_eq!(saved.len(), 1, "{:?}", notes.files());
}

#[cfg(unix)]
#[gpui::test]
async fn a_quick_note_window_without_a_note_still_closes(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let read_only = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(&notes.dir, read_only).unwrap();

    let window = quick.open(cx);
    let created = window
        .view
        .read_with(window.cx, |view, _| view.note().is_some());
    let closes = window
        .cx
        .update(|_, cx| window.view.update(cx, |view, cx| view.should_close(cx)));
    std::fs::set_permissions(&notes.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!created, "the folder is read-only");
    assert!(closes, "the window closes without a note");

    let mut again = quick.open(cx);
    again.end(Ending::Done);
    assert!(!again.is_open());
}

#[gpui::test]
async fn a_file_dropped_on_a_quick_note_is_copied_into_its_attachments(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Beleg");
    let receipt = notes.scratch("Beleg.pdf");
    std::fs::write(&receipt, b"pdf").unwrap();

    let position = gpui::point(gpui::px(190.), gpui::px(150.));
    let paths = gpui::ExternalPaths(vec![receipt].into());
    // After typing GPUI counts nothing as hovered until the mouse moves.
    window
        .cx
        .simulate_mouse_move(position, None, gpui::Modifiers::none());
    window
        .cx
        .simulate_event(gpui::FileDropEvent::Entered { position, paths });
    window
        .cx
        .simulate_event(gpui::FileDropEvent::Pending { position });
    window
        .cx
        .simulate_event(gpui::FileDropEvent::Submit { position });
    window.cx.run_until_parked();

    let id = window.id();
    let link = format!("_attachments/{id}/Beleg.pdf");
    assert!(
        window.text().contains(&format!("[Beleg.pdf]({link})")),
        "{}",
        window.text()
    );
    let folder = window.folder();
    assert_eq!(
        std::fs::read(notes.dir.join(folder).join(link)).unwrap(),
        b"pdf"
    );
}

#[gpui::test]
async fn an_image_pasted_into_a_quick_note_becomes_an_attachment(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let quick = start(&notes, cx);
    let mut window = quick.open(cx);
    window.cx.simulate_input("Skizze");
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, b"png".to_vec());
    window
        .cx
        .update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image)));

    window.cx.simulate_keystrokes("ctrl-v");
    window.cx.run_until_parked();

    let id = window.id();
    let link = format!("_attachments/{id}/Pasted-Image.png");
    assert!(
        window.text().contains(&format!("]({link})")),
        "{}",
        window.text()
    );
    let folder = window.folder();
    assert_eq!(
        std::fs::read(notes.dir.join(folder).join(link)).unwrap(),
        b"png"
    );
}
