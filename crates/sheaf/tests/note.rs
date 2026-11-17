//! The note view and the window's overlays in a test window: saving and renaming on
//! the executor's fake clock, changes made by other programs, the header's actions,
//! links and tags, the command palette and the list of shortcuts.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{Entity, TestAppContext, VisualTestContext};
use gpui_component::ActiveTheme as _;
use gpui_markdown_editor::{DocMeta, EditorEvent};
use sheaf::app::actions;
use sheaf::app::instance::Request;
use sheaf::app::library::{Library, LibraryEvent};
use sheaf::app::note::{Banner, NoteView, Popover, RENAME_DELAY, SAVE_DELAY};
use sheaf::app::overlays::shortcuts;
use sheaf::app::state::Recovery;
use sheaf::app::theme::ActiveTokens as _;
use sheaf::app::window::{self, MainWindow, Route};
use sheaf_core::{NoteId, NoteMeta, Sort};

struct TestApp {
    library: Entity<Library>,
    main: Entity<MainWindow>,
    cx: &'static mut VisualTestContext,
}

fn open_app(notes: &Notes, cx: &mut TestAppContext) -> TestApp {
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(notes, cx)
    });
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    TestApp { library, main, cx }
}

impl TestApp {
    fn route(&mut self) -> Route {
        self.main.read_with(self.cx, |main, cx| main.route(cx))
    }

    fn listed(&mut self) -> Vec<NoteMeta> {
        self.main
            .read_with(self.cx, |main, cx| main.gallery().read(cx).notes().to_vec())
    }

    fn id_of(&mut self, title: &str) -> NoteId {
        let note = self.listed().into_iter().find(|note| note.title == title);
        note.unwrap_or_else(|| panic!("no note titled {title}")).id
    }

    fn open(&mut self, title: &str) -> Entity<NoteView> {
        let id = self.id_of(title);
        self.request(Request::OpenNote {
            id: id.to_string(),
            query: String::new(),
        });
        self.note()
    }

    fn note(&mut self) -> Entity<NoteView> {
        self.main
            .read_with(self.cx, |main, _| main.note().cloned())
            .expect("a note is open")
    }

    fn request(&mut self, request: Request) {
        let main = self.main.clone();
        self.cx.update(|window, cx| {
            main.update(cx, |main, cx| main.handle(request, window, cx));
        });
        self.cx.run_until_parked();
    }

    fn editor_text(&mut self) -> String {
        let note = self.note();
        note.read_with(self.cx, |note, cx| note.editor().read(cx).markdown())
    }

    fn banner(&mut self) -> Option<Banner> {
        let note = self.note();
        note.read_with(self.cx, |note, _| note.banner().cloned())
    }

    fn wait(&mut self, time: Duration) {
        self.cx.executor().advance_clock(time);
        self.cx.run_until_parked();
    }

    fn note_do(&mut self, update: impl FnOnce(&mut NoteView, &mut gpui::Context<NoteView>)) {
        let note = self.note();
        note.update(self.cx, update);
        self.cx.run_until_parked();
    }

    /// What the editor would report after a click on a link, tag and so on.
    fn editor_emits(&mut self, event: EditorEvent) {
        let note = self.note();
        let editor = note.read_with(self.cx, |note, _| note.editor().clone());
        editor.update(self.cx, |_, cx| cx.emit(event));
        self.cx.run_until_parked();
    }

    fn toast(&mut self) -> Option<String> {
        self.main.read_with(self.cx, |main, cx| {
            main.toasts()
                .read(cx)
                .shown()
                .map(|toast| toast.message.to_string())
        })
    }

    /// Another program changed notes; this is what the service reports then.
    fn changed_elsewhere(&mut self, ids: Vec<NoteId>) {
        self.library
            .update(self.cx, |_, cx| cx.emit(LibraryEvent::Changed { ids }));
        self.cx.run_until_parked();
    }
}

const NOTCH: &str = "Arbeit/Notch-Ideen.md";

// ---- Saving -------------------------------------------------------------------------

#[gpui::test]
async fn typing_is_saved_600_ms_after_the_last_change(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let before = notes.read(NOTCH);

    app.cx.simulate_input("Vorab ");
    app.wait(SAVE_DELAY - Duration::from_millis(1));
    assert_eq!(notes.read(NOTCH), before, "not yet");
    app.cx.simulate_input("mehr ");
    app.wait(SAVE_DELAY - Duration::from_millis(1));
    assert_eq!(notes.read(NOTCH), before, "each change restarts the delay");
    assert!(!note.read_with(app.cx, |note, _| note.saved_shown()));

    app.wait(Duration::from_millis(2));
    assert!(notes.read(NOTCH).contains("Vorab mehr Notch-Ideen"));
    assert!(note.read_with(app.cx, |note, _| note.saved_shown()));
    assert_eq!(notes.files().len(), 14, "saved in place, not renamed yet");
}

#[gpui::test]
async fn closing_the_window_saves_at_once(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    app.cx.simulate_input("Zu: ");
    app.cx.dispatch_action(actions::CloseWindow);
    app.cx.run_until_parked();
    assert!(
        notes
            .read("Arbeit/Zu- Notch-Ideen.md")
            .contains("Zu: Notch-Ideen")
    );
}

#[gpui::test]
async fn the_file_follows_the_title_2_s_after_it_changed(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");

    app.cx.simulate_input("Neue ");
    app.wait(SAVE_DELAY);
    assert!(notes.read(NOTCH).contains("Neue Notch-Ideen"));
    app.wait(RENAME_DELAY - SAVE_DELAY - Duration::from_millis(1));
    assert!(notes.files().contains(&NOTCH.to_owned()));
    app.wait(Duration::from_millis(1));
    assert!(
        notes
            .files()
            .contains(&"Arbeit/Neue Notch-Ideen.md".to_owned())
    );
    assert!(!notes.files().contains(&NOTCH.to_owned()));
    assert_eq!(notes.files().len(), 14);

    // Leaving renames at once.
    app.cx.simulate_input("X");
    app.wait(SAVE_DELAY);
    let renamed = "Arbeit/Neue XNotch-Ideen.md".to_owned();
    assert!(!notes.files().contains(&renamed));
    app.cx.simulate_keystrokes("escape");
    assert!(notes.files().contains(&renamed), "{:?}", notes.files());
}

#[gpui::test]
async fn a_file_without_id_keeps_working_after_its_first_save(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Reading list");
    let provisional = note.read_with(app.cx, |note, _| note.id().clone());
    assert!(provisional.is_provisional());

    app.cx.simulate_input("My ");
    app.wait(SAVE_DELAY);
    let Route::Note { id, .. } = app.route() else {
        panic!("the note is open");
    };
    assert!(!id.is_provisional(), "the first write gave it a real id");
    assert!(notes.read("Reading list.md").contains(&format!("id: {id}")));

    app.cx.simulate_input("own ");
    app.wait(SAVE_DELAY);
    app.cx.simulate_keystrokes("escape");
    assert!(
        notes
            .read("My own Reading list.md")
            .contains("My own Reading list")
    );
    assert_eq!(notes.files().len(), 14, "no conflict copy");
}

#[gpui::test]
async fn a_new_note_left_empty_is_not_kept(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    let app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("ctrl-n");
    assert_eq!(notes.files(), ["Untitled.md"]);
    app.cx.simulate_keystrokes("escape");
    assert!(notes.files().is_empty());
    assert!(
        !notes.dir.join(".trash/Untitled.md").exists(),
        "not in the trash either"
    );

    app.cx.simulate_keystrokes("ctrl-n");
    app.cx.simulate_input("Idee");
    app.cx.simulate_keystrokes("escape");
    assert_eq!(notes.files(), ["Idee.md"]);
}

#[gpui::test]
async fn a_new_note_written_elsewhere_is_not_discarded(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    let app = open_app(&notes, cx);

    // Text from elsewhere: kept.
    app.cx.simulate_keystrokes("ctrl-n");
    let created = notes.read("Untitled.md");
    notes.write("Untitled.md", &format!("{created}Vom Handy\n"));
    app.cx.simulate_keystrokes("escape");
    assert!(notes.read("Untitled.md").ends_with("Vom Handy\n"));
    std::fs::remove_file(notes.dir.join("Untitled.md")).unwrap();

    // Still empty but written elsewhere: to the trash, not gone.
    app.cx.simulate_keystrokes("ctrl-n");
    let created = notes.read("Untitled.md");
    notes.write("Untitled.md", &format!("{created}\n"));
    app.cx.simulate_keystrokes("escape");
    assert!(notes.files().is_empty());
    assert!(notes.dir.join(".trash/Untitled.md").exists());
}

/// Sets the permission bits of `path`, as `chmod` does.
fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[gpui::test]
async fn a_failed_save_says_so_keeps_the_view_and_saves_once_it_can(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let folder = notes.dir.join("Arbeit");
    chmod(&folder.join("Notch-Ideen.md"), 0o444);
    chmod(&folder, 0o555);

    app.cx.simulate_input("Eins ");
    app.wait(SAVE_DELAY);
    let failed = Some(Banner::SaveFailed("permission denied".to_owned()));
    assert_eq!(app.banner(), failed);
    assert!(!app.note().read_with(app.cx, |note, _| note.saved_shown()));
    assert!(!notes.read(NOTCH).contains("Eins"));

    // The next change tries again; leaving does too, and stays while it fails.
    app.cx.simulate_input("Zwei ");
    app.wait(SAVE_DELAY);
    assert_eq!(app.banner(), failed);
    app.cx.simulate_keystrokes("escape");
    assert!(
        matches!(app.route(), Route::Note { .. }),
        "the text is only here"
    );
    assert_eq!(app.banner(), failed);

    chmod(&folder, 0o755);
    app.note_do(|note, cx| note.retry_save(cx));
    assert_eq!(app.banner(), None);
    let renamed = "Arbeit/Eins Zwei Notch-Ideen.md";
    assert!(
        notes
            .read(renamed)
            .contains("Eins Zwei Notch-Ideen\n\nWas die")
    );
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(notes.files().len(), 14);
}

#[gpui::test]
async fn closing_the_window_keeps_a_text_that_cannot_be_saved(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let recovery = Recovery::at(notes.scratch("recovered"));
    let mut app = open_app(&notes, cx);
    app.cx.update(|_, cx| cx.set_global(recovery.clone()));
    app.open("Notch-Ideen");
    chmod(&notes.dir.join("Arbeit"), 0o555);

    app.cx.simulate_input("Rettung ");
    app.cx.dispatch_action(actions::CloseWindow);
    app.cx.run_until_parked();
    chmod(&notes.dir.join("Arbeit"), 0o755);
    assert!(!notes.read(NOTCH).contains("Rettung"));
    let kept: Vec<_> = std::fs::read_dir(recovery.dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("Rettung Notch-Ideen-"), "{name}");
    assert!(
        std::fs::read_to_string(&kept[0])
            .unwrap()
            .starts_with("Rettung Notch-Ideen\n\nWas die Notch")
    );

    // The next window says so, once.
    let mut app = open_app(&notes, cx);
    assert_eq!(app.toast().as_deref(), Some("Recovered unsaved notes"));
    assert!(!recovery.take_unseen());
}

// ---- Changes made elsewhere ---------------------------------------------------------

/// Writes `text` into the Notch note, as another editor would.
fn edit_elsewhere(notes: &Notes, text: &str) {
    let old = notes.read(NOTCH);
    let head_end = old.match_indices("---\n").nth(1).unwrap().0 + 4;
    notes.write(NOTCH, &format!("{}{text}", &old[..head_end]));
}

#[gpui::test]
async fn a_change_elsewhere_is_taken_silently_when_nothing_is_unsaved(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let id = note.read_with(app.cx, |note, _| note.id().clone());

    edit_elsewhere(&notes, "Notch-Ideen\n\nVom Handy ergänzt.\n");
    app.changed_elsewhere(vec![id]);
    assert_eq!(app.editor_text(), "Notch-Ideen\n\nVom Handy ergänzt.\n");
    assert_eq!(app.banner(), None);

    // The next save expects the file as it was reloaded: no conflict.
    app.cx.simulate_input("A");
    app.wait(SAVE_DELAY);
    assert!(
        notes
            .read(NOTCH)
            .contains("ANotch-Ideen\n\nVom Handy ergänzt.")
    );
    assert_eq!(app.banner(), None);
}

#[gpui::test]
async fn own_saves_never_look_like_changes_elsewhere(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    for word in ["Eins ", "Zwei ", "Drei "] {
        app.cx.simulate_input(word);
        app.wait(SAVE_DELAY);
        // As the service would echo the write.
        let id = note.read_with(app.cx, |note, _| note.id().clone());
        app.changed_elsewhere(vec![id]);
        app.changed_elsewhere(Vec::new());
    }
    app.note_do(|note, cx| note.toggle_pin(cx));
    app.wait(RENAME_DELAY);
    assert_eq!(app.banner(), None);
    assert!(app.editor_text().starts_with("Eins Zwei Drei Notch-Ideen"));
    assert_eq!(notes.files().len(), 14);
}

#[gpui::test]
async fn a_change_elsewhere_with_unsaved_edits_asks_and_reload_takes_the_disk(
    cx: &mut TestAppContext,
) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let id = note.read_with(app.cx, |note, _| note.id().clone());

    app.cx.simulate_input("Meins ");
    edit_elsewhere(&notes, "Notch-Ideen\n\nVon Syncthing.\n");
    app.changed_elsewhere(vec![id]);
    assert_eq!(app.banner(), Some(Banner::ChangedOnDisk));
    assert!(
        app.editor_text().starts_with("Meins Notch-Ideen"),
        "the edits stay"
    );

    // While it asks, nothing is written.
    app.wait(RENAME_DELAY);
    assert!(notes.read(NOTCH).contains("Von Syncthing."));

    app.note_do(|note, cx| note.reload_from_disk(cx));
    assert_eq!(app.banner(), None);
    assert_eq!(app.editor_text(), "Notch-Ideen\n\nVon Syncthing.\n");
    app.cx.simulate_keystrokes("escape");
    assert_eq!(notes.files().len(), 14);
}

#[gpui::test]
async fn keep_mine_stores_the_edits_as_a_conflict_copy(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");

    // The save finds the file changed: the same question, without a change event.
    app.cx.simulate_input("Meins ");
    edit_elsewhere(&notes, "Notch-Ideen\n\nVon Syncthing.\n");
    app.wait(SAVE_DELAY);
    assert_eq!(app.banner(), Some(Banner::ChangedOnDisk));

    app.note_do(|note, cx| note.keep_mine(cx));
    assert_eq!(app.banner(), None);
    assert!(notes.read(NOTCH).contains("Von Syncthing."), "theirs stays");
    let copy = "Arbeit/Meins Notch-Ideen (conflict).md";
    assert!(
        notes
            .read(copy)
            .contains("Meins Notch-Ideen\n\nWas die Notch")
    );
    let Route::Note { id, .. } = app.route() else {
        panic!("the note is open");
    };
    assert!(
        notes.read(copy).contains(&format!("id: {id}")),
        "the view edits the copy"
    );

    app.cx.simulate_input("!");
    app.cx.simulate_keystrokes("escape");
    assert!(
        notes
            .files()
            .contains(&"Arbeit/Meins !Notch-Ideen.md".to_owned()),
        "{:?}",
        notes.files()
    );
    assert!(notes.read(NOTCH).contains("Von Syncthing."));
}

/// Leaving a note deleted elsewhere keeps unsaved edits as a new note, whether the
/// deletion was seen through a change event or by the save; without them it stays
/// deleted.
#[gpui::test]
async fn leaving_a_note_deleted_elsewhere_keeps_its_text(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let restored = "Arbeit/Rettung Notch-Ideen.md";

    // The change event first.
    let note = app.open("Notch-Ideen");
    let id = note.read_with(app.cx, |note, _| note.id().clone());
    app.cx.simulate_input("Rettung ");
    std::fs::remove_file(notes.dir.join(NOTCH)).unwrap();
    app.changed_elsewhere(vec![id]);
    assert_eq!(app.banner(), Some(Banner::DeletedOnDisk));
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    assert!(
        notes
            .read(restored)
            .contains("Rettung Notch-Ideen\n\nWas die Notch")
    );

    // The save first.
    std::fs::rename(notes.dir.join(restored), notes.dir.join(NOTCH)).unwrap();
    app.changed_elsewhere(Vec::new());
    app.open("Rettung Notch-Ideen");
    app.cx.simulate_input("Noch ");
    std::fs::remove_file(notes.dir.join(NOTCH)).unwrap();
    app.wait(SAVE_DELAY);
    assert_eq!(app.banner(), Some(Banner::DeletedOnDisk));
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    let again = "Arbeit/Noch Rettung Notch-Ideen.md";
    assert!(notes.read(again).contains("Noch Rettung Notch-Ideen"));

    // Without edits since the last save: deleted elsewhere on purpose, it stays so.
    let note = app.open("Noch Rettung Notch-Ideen");
    let id = note.read_with(app.cx, |note, _| note.id().clone());
    std::fs::remove_file(notes.dir.join(again)).unwrap();
    app.changed_elsewhere(vec![id]);
    assert_eq!(app.banner(), Some(Banner::DeletedOnDisk));
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    assert!(
        !notes.files().iter().any(|file| file.contains("Rettung")),
        "{:?}",
        notes.files()
    );
}

#[gpui::test]
async fn a_note_deleted_elsewhere_can_be_saved_as_new(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let id = note.read_with(app.cx, |note, _| note.id().clone());

    app.cx.simulate_input("Rettung ");
    std::fs::remove_file(notes.dir.join(NOTCH)).unwrap();
    app.changed_elsewhere(vec![id]);
    assert_eq!(app.banner(), Some(Banner::DeletedOnDisk));
    assert!(app.editor_text().starts_with("Rettung "));

    app.note_do(|note, cx| note.save_as_new(cx));
    assert_eq!(app.banner(), None);
    assert!(
        notes
            .read("Arbeit/Rettung Notch-Ideen.md")
            .contains("Was die Notch")
    );
}

// ---- Header -------------------------------------------------------------------------

#[gpui::test]
async fn pin_move_copy_and_delete_with_undo(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");

    app.note_do(|note, cx| note.toggle_pin(cx));
    assert!(notes.read(NOTCH).contains("pinned: true"));
    app.note_do(|note, cx| note.toggle_pin(cx));
    assert!(!notes.read(NOTCH).contains("pinned"));

    app.note_do(|note, cx| note.copy_markdown(cx));
    let copied = app.cx.read_from_clipboard().and_then(|item| item.text());
    assert!(copied.unwrap().starts_with("Notch-Ideen\n\n"));
    let toast = app.main.read_with(app.cx, |main, cx| {
        main.toasts()
            .read(cx)
            .shown()
            .map(|toast| toast.message.to_string())
    });
    assert_eq!(toast.as_deref(), Some("Copied as Markdown"));

    app.note_do(|note, cx| note.set_popover(Some(Popover::Move(Vec::new())), cx));
    let folders = app
        .note()
        .read_with(app.cx, |note, _| match note.popover() {
            Some(Popover::Move(folders)) => folders.len(),
            _ => 0,
        });
    assert_eq!(folders, 7, "the root and the six folders");
    app.note_do(|note, cx| note.move_to("Privat", cx));
    assert!(notes.files().contains(&"Privat/Notch-Ideen.md".to_owned()));
    assert_eq!(
        app.note()
            .read_with(app.cx, |note, _| note.popover().cloned()),
        None
    );

    app.cx.simulate_input("Zuletzt ");
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    assert_eq!(app.route(), Route::Gallery);
    assert!(notes.dir.join(".trash/Notch-Ideen.md").exists());
    assert!(!notes.files().iter().any(|file| file.contains("Notch")));
    let undo = app.main.read_with(app.cx, |main, cx| {
        main.toasts()
            .read(cx)
            .shown()
            .and_then(|toast| toast.action_label().map(ToString::to_string))
    });
    assert_eq!(undo.as_deref(), Some("Undo"));

    let toasts = app.main.read_with(app.cx, |main, _| main.toasts().clone());
    app.cx.update(|window, cx| {
        toasts.update(cx, |toasts, cx| toasts.run_action(window, cx));
    });
    app.cx.run_until_parked();
    let restored = notes.read("Privat/Notch-Ideen.md");
    assert!(
        restored.contains("Zuletzt Notch-Ideen"),
        "saved before it went"
    );
    assert!(
        app.main
            .read_with(app.cx, |main, cx| main.toasts().read(cx).shown().is_none())
    );
}

#[gpui::test]
async fn a_trash_that_fails_leaves_the_note_working(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let trash = notes.dir.join(".trash");
    std::fs::create_dir(&trash).unwrap();
    chmod(&trash, 0o555);
    app.open("Notch-Ideen");

    app.cx.simulate_input("Vorher ");
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    chmod(&trash, 0o755);
    assert!(matches!(app.route(), Route::Note { .. }));
    assert_eq!(
        app.toast().as_deref(),
        Some("Could not move to trash: permission denied")
    );
    assert!(notes.files().contains(&NOTCH.to_owned()));

    app.cx.simulate_input("Nachher ");
    app.wait(SAVE_DELAY);
    assert!(notes.read(NOTCH).contains("Vorher Nachher Notch-Ideen"));
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    assert_eq!(app.route(), Route::Gallery);
    assert!(notes.dir.join(".trash/Notch-Ideen.md").exists());
}

#[gpui::test]
async fn delete_on_a_note_changed_elsewhere_asks_first(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");

    app.cx.simulate_input("Meins ");
    edit_elsewhere(&notes, "Notch-Ideen\n\nVon Syncthing.\n");
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    assert_eq!(app.banner(), Some(Banner::ChangedOnDisk));
    assert!(matches!(app.route(), Route::Note { .. }));
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    assert!(
        matches!(app.route(), Route::Note { .. }),
        "not while it asks"
    );
    assert!(!notes.dir.join(".trash").exists());
    assert!(notes.read(NOTCH).contains("Von Syncthing."));
    assert!(app.editor_text().starts_with("Meins Notch-Ideen"));

    // Once it is answered, Delete takes the note the view shows.
    app.note_do(|note, cx| note.keep_mine(cx));
    app.cx.dispatch_action(actions::Trash);
    app.cx.run_until_parked();
    assert_eq!(app.route(), Route::Gallery);
    assert!(notes.read(NOTCH).contains("Von Syncthing."));
    assert!(
        notes
            .dir
            .join(".trash/Meins Notch-Ideen (conflict).md")
            .exists()
    );
}

#[gpui::test]
async fn escape_closes_a_popover_before_it_leaves(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    app.note_do(|note, cx| note.set_popover(Some(Popover::Menu), cx));
    app.cx.simulate_keystrokes("escape");
    assert!(matches!(app.route(), Route::Note { .. }));
    assert_eq!(
        app.note()
            .read_with(app.cx, |note, _| note.popover().cloned()),
        None
    );
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
}

#[gpui::test]
async fn toasts_go_away_after_5_s(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    app.note_do(|note, cx| note.copy_markdown(cx));
    let shown = |app: &mut TestApp| {
        app.main
            .read_with(app.cx, |main, cx| main.toasts().read(cx).shown().is_some())
    };
    assert!(shown(&mut app));
    app.wait(Duration::from_millis(4_999));
    assert!(shown(&mut app));
    app.wait(Duration::from_millis(2));
    assert!(!shown(&mut app));
}

// ---- Links, tags and the search a note was opened from ------------------------------

#[gpui::test]
async fn wiki_links_open_their_note_or_create_it(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Meeting notes 2026-11-03");

    app.editor_emits(EditorEvent::WikiLinkClicked("project PLAN".into()));
    let plan = app.id_of("Project plan");
    assert!(matches!(app.route(), Route::Note { id, .. } if id == plan));

    app.editor_emits(EditorEvent::WikiLinkClicked("Venue shortlist".into()));
    assert!(
        notes
            .read("Work/Venue shortlist.md")
            .contains("Venue shortlist")
    );
    assert!(app.editor_text().starts_with("Venue shortlist"));
}

#[gpui::test]
async fn tags_search_the_gallery_and_links_open_in_the_browser(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Lesestoff");
    app.editor_emits(EditorEvent::LinkClicked(
        "https://developer.gnome.org/hig/".into(),
    ));
    assert_eq!(
        app.cx.opened_url().as_deref(),
        Some("https://developer.gnome.org/hig/")
    );

    app.editor_emits(EditorEvent::TagClicked("rezepte".into()));
    assert_eq!(app.route(), Route::Gallery);
    let titles: Vec<String> = app.listed().into_iter().map(|note| note.title).collect();
    assert_eq!(titles, ["Shakshuka"]);
}

#[gpui::test]
async fn opened_from_a_search_the_matches_are_marked(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let shakshuka = app.id_of("Shakshuka");
    app.request(Request::OpenNote {
        id: shakshuka.to_string(),
        query: "kreuzkümmel Feta folder:Privat".to_owned(),
    });
    let note = app.note();
    let (highlights, debug) = note.read_with(app.cx, |note, cx| {
        let editor = note.editor().read(cx);
        (editor.highlights().to_vec(), format!("{editor:?}"))
    });
    assert_eq!(highlights.len(), 2, "both words, not the filter");
    assert!(highlights[0].start.offset < highlights[1].start.offset);
    // The editor has no getter for the scroll target yet; its debug output shows it.
    assert!(
        debug.contains(&format!("scroll_target: Some({:?})", highlights[0])),
        "{debug}"
    );
}

#[gpui::test]
async fn the_editors_view_state_is_kept_per_note(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Ausschreibung – offene Fragen");
    let mut meta = DocMeta::default();
    meta.column_widths.insert(0, vec![180.0, 96.5]);
    meta.folded_headings.insert(1);
    let editor = note.read_with(app.cx, |note, _| note.editor().clone());
    editor.update(app.cx, |editor, cx| editor.set_meta(meta.clone(), cx));
    app.editor_emits(EditorEvent::MetaChanged);
    app.cx.run_until_parked();
    let config = std::fs::read_to_string(notes.dir.join(".sheaf/config.toml")).unwrap();
    assert!(config.contains("01JD3K8Z6Q4X7V9B2M5N8P0R02"), "{config}");

    app.cx.simulate_keystrokes("escape");
    let note = app.open("Ausschreibung – offene Fragen");
    let restored = note.read_with(app.cx, |note, cx| note.editor().read(cx).meta().clone());
    assert_eq!(restored, meta);
}

#[gpui::test]
async fn the_editor_draws_with_the_font_the_theme_resolved(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("down down enter");
    let note = app.note();
    let (editor, tokens, theme) = note.read_with(app.cx, |note, cx| {
        let editor = note.editor().read(cx).style().font_family.clone();
        (
            editor,
            cx.tokens().font_family.clone(),
            cx.theme().font_family.clone(),
        )
    });
    // The family with a real bold face (the embedded Inter if the desktop font is
    // variable), not the one the desktop named.
    assert!(!tokens.is_empty());
    assert_eq!(editor, tokens);
    assert_eq!(theme, tokens, "the overlays inherit it from gpui-component");
}

// ---- Palette and shortcuts ----------------------------------------------------------

fn palette_entries(app: &mut TestApp) -> Option<Vec<String>> {
    app.main.read_with(app.cx, |main, cx| {
        main.palette().map(|palette| palette.read(cx).entries())
    })
}

#[gpui::test]
async fn the_palette_lists_commands_and_notes_and_runs_them(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);

    app.cx.simulate_keystrokes("ctrl-k");
    let entries = palette_entries(&mut app).expect("the palette is open");
    for label in [
        "New Note",
        "Search",
        "Pin or Unpin",
        "Move to Trash",
        "Keyboard Shortcuts",
    ] {
        assert!(
            entries.contains(&label.to_owned()),
            "{label} in {entries:?}"
        );
    }
    assert!(!entries.contains(&"Select Next Note".to_owned()));
    app.cx.simulate_keystrokes("escape");
    assert!(palette_entries(&mut app).is_none());

    // Typing finds notes; Enter opens the first.
    app.cx.simulate_keystrokes("ctrl-k");
    app.cx.simulate_input("shakshuka");
    let entries = palette_entries(&mut app).unwrap();
    assert!(
        entries.contains(&"Open note: Shakshuka".to_owned()),
        "{entries:?}"
    );
    app.cx.simulate_keystrokes("enter");
    assert!(palette_entries(&mut app).is_none());
    let shakshuka = app.id_of("Shakshuka");
    assert!(matches!(app.route(), Route::Note { id, .. } if id == shakshuka));

    // A command runs where the keyboard was: here, the open note.
    app.cx.simulate_keystrokes("ctrl-k");
    app.cx.simulate_input("pin or");
    app.cx.simulate_keystrokes("enter");
    assert!(
        notes
            .read("Privat/Rezepte/Shakshuka.md")
            .contains("pinned: true")
    );

    app.cx.simulate_keystrokes("ctrl-k");
    app.cx.simulate_input("back to");
    app.cx.simulate_keystrokes("enter");
    assert_eq!(app.route(), Route::Gallery);
}

#[gpui::test]
async fn the_shortcuts_list_shows_every_binding(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("ctrl-?");
    assert!(
        app.main
            .read_with(app.cx, |main, _| main.shortcuts().is_some())
    );

    let (groups, bindings) = app.cx.update(|_, cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bindings: Vec<String> = keymap
            .bindings()
            .filter(|binding| binding.action().name().starts_with("sheaf::"))
            .map(sheaf::app::overlays::format_keys)
            .collect();
        (shortcuts::groups(cx), bindings)
    });
    let shown: Vec<&str> = groups
        .iter()
        .flat_map(|group| &group.lines)
        .flat_map(|(_, keys)| keys.split(", "))
        .collect();
    assert!(!bindings.is_empty());
    for keys in &bindings {
        assert!(
            shown.contains(&keys.as_str()),
            "{keys} is missing from {groups:?}"
        );
    }
    let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();
    assert_eq!(titles, ["General", "Gallery", "Note"]);
    let note = &groups[2].lines;
    assert_eq!(
        note,
        &[("Back to Notes".to_owned(), "Ctrl+W, Esc".to_owned())]
    );

    app.cx.simulate_keystrokes("escape");
    assert!(
        app.main
            .read_with(app.cx, |main, _| main.shortcuts().is_none())
    );
    assert_eq!(app.route(), Route::Gallery);
    let listed = app.library.read_with(app.cx, |library, cx| {
        library.notes(Vec::new(), Sort::Title, cx)
    });
    assert_eq!(listed.await.unwrap().len(), 14);
}

#[gpui::test]
async fn keys_typed_before_the_note_is_loaded_never_appear_and_are_never_lost(
    cx: &mut TestAppContext,
) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let id = app.id_of("Notch-Ideen");
    let main = app.main.clone();
    // Opened and typed into before the worker has read the file.
    app.cx.update(|window, cx| {
        main.update(cx, |main, cx| {
            main.open_note(id, String::new(), window, cx);
        });
        window.draw(cx).clear(cx);
        for key in ["a", "b"] {
            window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
        }
    });
    let typed = app.editor_text();
    assert_eq!(typed, "", "nothing appears before the text is there");

    app.cx.run_until_parked();
    let body = app.editor_text();
    assert!(body.starts_with("Notch-Ideen\n"), "{body}");
    // Once loaded, the editor has the keyboard.
    app.cx.simulate_input("Neu ");
    assert!(app.editor_text().starts_with("Neu Notch-Ideen"));
}

#[gpui::test]
async fn a_second_new_note_while_the_first_is_created_leaves_nothing_behind(
    cx: &mut TestAppContext,
) {
    let notes = Notes::empty();
    let mut app = open_app(&notes, cx);
    let main = app.main.clone();
    app.cx.update(|window, cx| {
        main.update(cx, |main, cx| {
            main.handle(Request::NewNote, window, cx);
            main.handle(Request::NewNote, window, cx);
        });
    });
    app.cx.run_until_parked();
    assert_eq!(notes.files(), ["Untitled.md"]);
    app.cx.simulate_keystrokes("escape");
    assert!(notes.files().is_empty(), "{:?}", notes.files());
    assert_eq!(app.route(), Route::Gallery);
}

#[gpui::test]
async fn a_new_note_saved_and_then_emptied_is_not_kept(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("ctrl-n");
    app.cx.simulate_input("Geheim");
    app.wait(SAVE_DELAY);
    assert!(notes.read("Untitled.md").contains("Geheim"));

    app.cx.simulate_keystrokes("ctrl-a backspace");
    assert_eq!(app.editor_text().trim(), "");
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    assert!(notes.files().is_empty(), "{:?}", notes.files());
    let trash = notes.dir.join(".trash");
    let kept = std::fs::read_dir(&trash)
        .into_iter()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.unwrap().path()).ok())
        .any(|text| text.contains("Geheim"));
    assert!(!kept, "the deleted text is nowhere");
}

#[gpui::test]
async fn a_pin_or_move_that_fails_says_so_and_shows_the_real_state(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let pinned = |app: &mut TestApp| {
        let note = app.note();
        note.read_with(app.cx, |note, _| note.meta().unwrap().pinned)
    };

    // Unsaved edits and the file changed on disk: nothing is pinned.
    app.cx.simulate_input("Meins ");
    edit_elsewhere(&notes, "Notch-Ideen\n\nVon Syncthing.\n");
    app.note_do(|note, cx| note.toggle_pin(cx));
    assert!(!notes.read(NOTCH).contains("pinned"));
    assert!(!pinned(&mut app), "the icon flips back");
    assert_eq!(
        app.toast().as_deref(),
        Some("Could not pin the note: the file changed elsewhere")
    );
    assert_eq!(app.banner(), Some(Banner::ChangedOnDisk));
    app.note_do(|note, cx| note.reload_from_disk(cx));
    let id = note.read_with(app.cx, |note, _| note.id().clone());
    app.changed_elsewhere(vec![id]);

    // A folder that cannot be written to.
    let target = notes.dir.join("Privat");
    chmod(&target, 0o555);
    app.note_do(|note, cx| note.move_to("Privat", cx));
    chmod(&target, 0o755);
    assert!(notes.files().contains(&NOTCH.to_owned()));
    assert_eq!(
        app.toast().as_deref(),
        Some("Could not move the note: permission denied")
    );
}

#[gpui::test]
async fn a_note_found_for_an_earlier_query_opens_with_that_query(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("ctrl-k");
    app.cx.simulate_input("shakshuka");
    // Typed on and confirmed before the search for the longer query is back.
    app.cx.update(|window, cx| {
        for key in ["space", "x", "y"] {
            window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
        }
    });
    app.cx.update(|window, cx| window.draw(cx).clear(cx));
    app.cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse("enter").unwrap(), cx);
    });
    app.cx.run_until_parked();
    let shakshuka = app.id_of("Shakshuka");
    assert_eq!(
        app.route(),
        Route::Note {
            id: shakshuka,
            query: "shakshuka".to_owned()
        }
    );
}
