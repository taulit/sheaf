//! "Recently Deleted" in a test window: the trash as a list with deletion dates and
//! days left, restoring, deleting for good after asking, emptying, several notes at
//! once, the read-only note, and the purge after 30 days.

#![allow(clippy::unwrap_used)]

mod fixture;

use fixture::{Notes, TestApp, open_app};
use gpui::{Modifiers, TestAppContext};
use jiff::Timestamp;
use sheaf::app::gallery::Scope;
use sheaf::app::gallery::trash::{Confirm, days_left};

/// The app with `titles` moved to the trash, showing it.
fn open_trash(notes: &Notes, titles: &[&str], cx: &mut gpui::TestAppContext) -> TestApp {
    let mut app = open_app(notes, cx);
    for title in titles {
        let id = app.read(|gallery| {
            let note = gallery.notes().iter().find(|note| note.title == *title);
            note.unwrap().id.clone()
        });
        let trashed = app
            .library
            .update(app.cx, |library, cx| library.trash(&id, cx));
        app.cx.run_until_parked();
        app.cx.foreground_executor().block_test(trashed).unwrap();
    }
    app.update(|gallery, cx| gallery.set_scope(Scope::Trash, cx));
    app
}

impl TestApp {
    fn click_button(&mut self, id: &'static str) {
        let bounds = self.cx.debug_bounds(id).expect("the button is drawn");
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.update(|_, _| {});
    }

    fn click(&mut self, index: usize, modifiers: Modifiers) {
        self.update(|gallery, cx| gallery.click(index, modifiers, cx));
    }
}

fn in_trash(notes: &Notes) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(notes.dir.join(".trash")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md"))
        .collect();
    names.sort();
    names
}

#[gpui::test]
async fn the_trash_lists_deleted_notes_with_their_days_left(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_trash(&notes, &["Einkauf", "Lesestoff"], cx);
    assert_eq!(app.titles(), ["Einkauf", "Lesestoff"]);
    assert_eq!(app.read(|gallery| gallery.trash_count()), Some(2));
    let now: Timestamp = fixture::NOW.parse().unwrap();
    let ids: Vec<_> = app.read(|gallery| gallery.notes().iter().map(|n| n.id.clone()).collect());
    for id in &ids {
        let deleted = app.read(|gallery| gallery.deleted_at(id)).unwrap();
        assert_eq!(deleted, now);
        assert_eq!(days_left(deleted, now), 30);
    }
    let title = app.read(|gallery| gallery.scope().title());
    assert_eq!(title, "Recently Deleted");
    // Searching the trash goes by title.
    app.update(|gallery, cx| gallery.set_query("lese", cx));
    assert_eq!(app.titles(), ["Lesestoff"]);
}

#[gpui::test]
async fn restore_puts_notes_back_where_they_were(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_trash(&notes, &["Einkauf", "Lesestoff", "Notch-Ideen"], cx);
    assert!(!notes.files().contains(&"Privat/Einkauf.md".to_owned()));
    // Two of three, picked with Ctrl.
    app.click(0, Modifiers::control());
    app.click(2, Modifiers::control());
    app.update(|gallery, cx| gallery.restore_selection(cx));
    assert_eq!(app.titles(), ["Lesestoff"]);
    let files = notes.files();
    assert!(files.contains(&"Privat/Einkauf.md".to_owned()), "{files:?}");
    assert!(
        files.contains(&"Arbeit/Notch-Ideen.md".to_owned()),
        "{files:?}"
    );
    assert_eq!(in_trash(&notes), ["Lesestoff.md"]);
    let toast = app.main.read_with(app.cx, |main, cx| {
        main.toasts()
            .read(cx)
            .shown()
            .map(|toast| toast.message.to_string())
    });
    assert_eq!(toast.as_deref(), Some("2 notes restored"));
}

#[gpui::test]
async fn delete_permanently_asks_first(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_trash(&notes, &["Einkauf", "Lesestoff"], cx);
    app.click(0, Modifiers::control());
    app.cx.simulate_keystrokes("delete");
    app.cx.run_until_parked();
    let asked = app.read(|gallery| gallery.confirming().cloned());
    assert!(matches!(asked, Some(Confirm::Purge(ids)) if ids.len() == 1));
    // Esc says no.
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.read(|gallery| gallery.confirming().cloned()), None);
    assert_eq!(in_trash(&notes).len(), 2);

    app.update(|gallery, cx| gallery.ask_purge_selection(cx));
    app.update(|gallery, cx| gallery.confirm(cx));
    assert_eq!(app.titles(), ["Lesestoff"]);
    assert_eq!(in_trash(&notes), ["Lesestoff.md"]);
}

#[gpui::test]
async fn empty_trash_deletes_everything_after_asking(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_trash(&notes, &["Einkauf", "Lesestoff", "Reading list"], cx);
    let before = notes.files().len();
    app.update(|gallery, cx| gallery.ask_empty_trash(cx));
    assert_eq!(
        app.read(|gallery| gallery.confirming().cloned()),
        Some(Confirm::Empty)
    );
    assert_eq!(in_trash(&notes).len(), 3);
    // Enter says no as well: deleting for good takes a click.
    app.cx.simulate_keystrokes("enter");
    app.update(|_, _| {});
    assert_eq!(app.read(|gallery| gallery.confirming().cloned()), None);
    assert_eq!(in_trash(&notes).len(), 3);

    app.update(|gallery, cx| gallery.ask_empty_trash(cx));
    app.click_button("confirm-delete");
    assert!(in_trash(&notes).is_empty());
    assert!(app.titles().is_empty());
    assert_eq!(notes.files().len(), before, "the other notes stay");
}

#[gpui::test]
async fn a_trashed_note_opens_read_only_with_restore(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_trash(&notes, &["Einkauf"], cx);
    app.click(0, Modifiers::none());
    let shown = app.read(|gallery| gallery.shown_trashed().map(|note| note.title.clone()));
    assert_eq!(shown.as_deref(), Some("Einkauf"));
    // No note view: the trashed note is not edited.
    assert!(app.main.read_with(app.cx, |main, _| main.note().is_none()));

    app.cx.simulate_keystrokes("escape");
    assert!(app.read(|gallery| gallery.shown_trashed().is_none()));
    app.click(0, Modifiers::none());
    app.update(|gallery, cx| gallery.restore_shown(cx));
    assert!(app.read(|gallery| gallery.shown_trashed().is_none()));
    assert!(notes.files().contains(&"Privat/Einkauf.md".to_owned()));
    assert!(app.titles().is_empty());
}

#[gpui::test]
async fn notes_older_than_thirty_days_are_deleted(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(
        ".trash/Alt.md",
        "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R99\ndeleted: 2026-10-16T13:00:00+01:00\nfrom: \"\"\n---\nAlt\n",
    );
    notes.write(
        ".trash/Neu.md",
        "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R98\ndeleted: 2026-10-20T13:00:00+01:00\nfrom: \"\"\n---\nNeu\n",
    );
    let mut app = open_trash(&notes, &[], cx);
    assert_eq!(app.titles(), ["Neu"]);
    assert_eq!(in_trash(&notes), ["Neu.md"]);
}

#[gpui::test]
async fn an_image_another_note_shows_survives_the_trash(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    let id = "01JD3K8Z6Q4X7V9B2M5N8P0R3T";
    let foto = format!("_attachments/{id}/foto.png");
    notes.write(
        "Urlaub.md",
        &format!("---\nid: {id}\n---\nUrlaub\n\n![Foto]({foto})\n"),
    );
    notes.write(&foto, "png");
    notes.write("Album.md", &format!("Album\n\n![Foto]({foto})\n"));

    let mut app = open_trash(&notes, &["Urlaub"], cx);
    assert_eq!(app.titles(), ["Urlaub"]);
    assert_eq!(notes.read(&foto), "png", "the album still shows it");

    let id: sheaf_core::NoteId = id.parse().unwrap();
    let purged = app
        .library
        .update(app.cx, |library, cx| library.purge(&id, cx));
    app.cx.run_until_parked();
    app.cx.foreground_executor().block_test(purged).unwrap();
    assert_eq!(notes.read(&foto), "png", "not deleted for good either");
}
