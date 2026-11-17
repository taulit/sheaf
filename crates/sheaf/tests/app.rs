//! The app in a test window: the Library, the router, actions and keys, the theme and
//! the remembered window size. No display, no bus, no files outside a temporary folder.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{
    Background, Entity, Hsla, ListOffset, TestAppContext, VisualTestContext, WindowHandle, px, size,
};
use gpui_component::{Root, Theme, WindowExt as _};
use gpui_markdown_editor::EditorEvent;
use sheaf::app::actions::{self, CloseWindow};
use sheaf::app::instance::Request;
use sheaf::app::library::{Library, LibraryEvent, LibraryMode};
use sheaf::app::state::{StateFile, WindowState};
use sheaf::app::theme::{self, ActiveTokens as _, Appearance, Overrides};
use sheaf::app::window::{self, MainWindow, Route};
use sheaf_core::config::View;
use sheaf_core::{CreateOptions, Error, Filter, NoteId, NoteMeta, Sort};

/// The app with its main window on `notes`.
struct TestApp {
    library: Entity<Library>,
    window: WindowHandle<Root>,
    main: Entity<MainWindow>,
    cx: &'static mut VisualTestContext,
}

fn open_app(notes: &Notes, state_file: Option<StateFile>, cx: &mut TestAppContext) -> TestApp {
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(notes, cx)
    });
    open_window(library, state_file, cx)
}

fn open_window(
    library: Entity<Library>,
    state_file: Option<StateFile>,
    cx: &mut TestAppContext,
) -> TestApp {
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), state_file, cx))
        .unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    TestApp {
        library,
        window,
        main,
        cx,
    }
}

impl TestApp {
    fn route(&mut self) -> Route {
        self.main.read_with(self.cx, |main, cx| main.route(cx))
    }

    fn listed(&mut self) -> Vec<NoteMeta> {
        self.main
            .read_with(self.cx, |main, cx| main.gallery().read(cx).notes().to_vec())
    }

    fn titles(&mut self) -> Vec<String> {
        self.listed().into_iter().map(|note| note.title).collect()
    }

    fn selected(&mut self) -> Option<String> {
        self.main.read_with(self.cx, |main, cx| {
            let gallery = main.gallery().read(cx);
            gallery.selected().map(|note| note.title.clone())
        })
    }

    /// The Markdown in the editor of the open note.
    fn editor_text(&mut self) -> String {
        self.main.read_with(self.cx, |main, cx| {
            let note = main.note().expect("a note is open").read(cx);
            note.editor().read(cx).markdown()
        })
    }

    fn request(&mut self, request: Request) {
        let main = self.main.clone();
        self.cx.update(|window, cx| {
            main.update(cx, |main, cx| main.handle(request, window, cx));
        });
        self.cx.run_until_parked();
    }

    fn id_of(&mut self, title: &str) -> NoteId {
        let note = self.listed().into_iter().find(|note| note.title == title);
        note.unwrap_or_else(|| panic!("no note titled {title}")).id
    }
}

fn record_events(
    library: &Entity<Library>,
    cx: &mut TestAppContext,
) -> Rc<RefCell<Vec<LibraryEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(library, move |_, event: &LibraryEvent, _| {
            events.borrow_mut().push(event.clone());
        })
        .detach();
    });
    events
}

fn changed(ids: &[&NoteId]) -> LibraryEvent {
    LibraryEvent::Changed {
        ids: ids.iter().map(|id| (*id).clone()).collect(),
    }
}

// ---- Library ------------------------------------------------------------------------

#[gpui::test]
async fn library_indexes_the_folder_in_memory_without_a_service(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(&notes, cx)
    });
    let events = record_events(&library, cx);
    assert_eq!(
        library.read_with(cx, |library, _| library.mode()),
        LibraryMode::Opening
    );

    cx.run_until_parked();
    assert_eq!(
        library.read_with(cx, |library, _| library.mode()),
        LibraryMode::Local
    );
    assert_eq!(*events.borrow(), [changed(&[])]);

    let listed = library
        .read_with(cx, |library, cx| {
            library.notes(Vec::new(), Sort::Modified, cx)
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 14);
    assert_eq!(listed[0].title, "Einkauf");
    assert_eq!(listed[13].title, "Tax return 2025");

    let pinned = library
        .read_with(cx, |library, cx| {
            library.notes(vec![Filter::Pinned], Sort::Title, cx)
        })
        .await
        .unwrap();
    let pinned: Vec<&str> = pinned.iter().map(|note| note.title.as_str()).collect();
    assert_eq!(pinned, ["Ausschreibung – offene Fragen", "Einkauf"]);

    let hits = library
        .read_with(cx, |library, cx| library.search("kreuzkümmel", 10, cx))
        .await
        .unwrap();
    assert_eq!(hits.total, 1);
    assert_eq!(hits.hits[0].meta.title, "Shakshuka");

    let folders = library
        .read_with(cx, |library, cx| library.folders(cx))
        .await
        .unwrap();
    let folders: Vec<(&str, usize)> = folders
        .iter()
        .map(|folder| (folder.path.as_str(), folder.notes))
        .collect();
    assert_eq!(
        folders,
        [
            ("", 2),
            ("Arbeit", 3),
            ("Archive", 1),
            ("Privat", 4),
            ("Privat/Rezepte", 1),
            ("Schnellnotizen", 1),
            ("Work", 2),
        ]
    );

    let tags = library
        .read_with(cx, |library, cx| library.tags(cx))
        .await
        .unwrap();
    let tags: Vec<&str> = tags.iter().map(|(tag, _)| tag.as_str()).collect();
    assert_eq!(
        tags,
        [
            "ausschreibung",
            "books",
            "garten",
            "project",
            "rezepte",
            "urlaub"
        ]
    );

    let shakshuka = &hits.hits[0].meta.id;
    let body = library
        .read_with(cx, |library, cx| library.body(shakshuka, cx))
        .await
        .unwrap();
    let body = body.unwrap();
    assert!(body.starts_with("Shakshuka\n\n![Shakshuka]"));
    assert!(body.contains("\n\nFür 2 Personen"));
}

#[gpui::test]
async fn library_writes_update_the_lists_and_tell_listeners(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(&notes, cx)
    });
    cx.run_until_parked();
    let events = record_events(&library, cx);
    let list = |cx: &mut TestAppContext| {
        library.read_with(cx, |library, cx| {
            library.notes(Vec::new(), Sort::Modified, cx)
        })
    };

    let created = library
        .update(cx, |library, cx| {
            library.create("Neue Idee\n\nNoch roh.", CreateOptions::default(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();
    assert_eq!(events.borrow().last(), Some(&changed(&[&created.id])));
    let listed = list(cx).await.unwrap();
    assert_eq!(listed.len(), 15);
    assert_eq!(listed[0].id, created.id);
    assert_eq!(notes.read("Neue Idee.md").lines().last(), Some("Noch roh."));

    // Saving with the fingerprint of the load.
    let loaded = library
        .read_with(cx, |library, cx| library.load(&created.id, cx))
        .await
        .unwrap();
    assert_eq!(loaded.body(), "Neue Idee\n\nNoch roh.\n");
    let saved = library
        .update(cx, |library, cx| {
            let body = "Bessere Idee\n\nAusgearbeitet. #einfall".to_owned();
            library.save_body(&created.id, body, Some(loaded.fingerprint), cx)
        })
        .await
        .unwrap();
    assert_eq!(saved.meta.title, "Bessere Idee");
    assert_eq!(
        saved.meta.rel_path, "Neue Idee.md",
        "saving does not rename"
    );
    assert_eq!(saved.meta.tags, ["einfall"]);
    let hits = library
        .read_with(cx, |library, cx| library.search("#einfall", 10, cx))
        .await
        .unwrap();
    assert_eq!(hits.total, 1);

    // The file has moved on since `loaded`: nothing is written.
    let stale = library
        .update(cx, |library, cx| {
            library.save_body(
                &created.id,
                "Überschrieben".to_owned(),
                Some(loaded.fingerprint),
                cx,
            )
        })
        .await;
    assert!(matches!(stale, Err(Error::Conflict(_))), "{stale:?}");
    assert!(notes.read("Neue Idee.md").contains("Ausgearbeitet."));
    let again = library
        .update(cx, |library, cx| {
            let body = "Bessere Idee\n\nFertig.".to_owned();
            library.save_body(&created.id, body, Some(saved.fingerprint), cx)
        })
        .await
        .unwrap();
    assert_eq!(again.meta.id, created.id);

    let renamed = library
        .update(cx, |library, cx| library.sync_filename(&created.id, cx))
        .await
        .unwrap();
    assert_eq!(renamed.rel_path, "Bessere Idee.md");
    assert!(notes.files().contains(&"Bessere Idee.md".to_owned()));
    assert!(!notes.files().contains(&"Neue Idee.md".to_owned()));
    assert_eq!(
        list(cx).await.unwrap().len(),
        15,
        "the old path left the index"
    );

    let pinned = library
        .update(cx, |library, cx| library.set_pinned(&created.id, true, cx))
        .await
        .unwrap();
    assert!(pinned.pinned);
    let moved = library
        .update(cx, |library, cx| {
            library.move_to(&created.id, "Arbeit/Ideen", cx)
        })
        .await
        .unwrap();
    assert_eq!(moved.rel_path, "Arbeit/Ideen/Bessere Idee.md");

    let copy = library
        .update(cx, |library, cx| library.duplicate(&created.id, cx))
        .await
        .unwrap();
    assert_ne!(copy.id, created.id);
    assert_eq!(list(cx).await.unwrap().len(), 16);

    let trashed = library
        .update(cx, |library, cx| library.trash(&copy.id, cx))
        .await
        .unwrap();
    cx.run_until_parked();
    assert_eq!(events.borrow().last(), Some(&changed(&[&copy.id])));
    let listed = list(cx).await.unwrap();
    assert_eq!(listed.len(), 15);
    assert!(listed.iter().all(|note| note.id != copy.id));

    let restored = library
        .update(cx, |library, cx| library.restore(&trashed.id, cx))
        .await
        .unwrap();
    assert_eq!(restored.folder, "Arbeit/Ideen");
    assert_eq!(list(cx).await.unwrap().len(), 16);
}

#[gpui::test]
async fn a_file_from_another_editor_gets_its_id_with_the_first_write(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(&notes, cx)
    });
    cx.run_until_parked();
    let events = record_events(&library, cx);
    let provisional = NoteId::provisional("Reading list.md");

    let pinned = library
        .update(cx, |library, cx| library.set_pinned(&provisional, true, cx))
        .await
        .unwrap();
    cx.run_until_parked();
    assert!(!pinned.id.is_provisional());
    assert_eq!(*events.borrow(), [changed(&[&provisional, &pinned.id])]);
    let found = library
        .read_with(cx, |library, cx| library.meta(&pinned.id, cx))
        .await
        .unwrap();
    assert!(found.unwrap().pinned);
    let old = library
        .read_with(cx, |library, cx| library.meta(&provisional, cx))
        .await
        .unwrap();
    assert_eq!(old, None);
}

// ---- Router -------------------------------------------------------------------------

#[gpui::test]
async fn opening_a_note_and_going_back_keeps_the_gallery_as_it_was(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);
    app.cx.simulate_resize(size(px(1100.), px(420.)));
    app.cx.run_until_parked();
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(app.titles().len(), 14);

    // In the grid, down goes from the pinned row to the row of yesterday.
    app.cx.simulate_keystrokes("down down");
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));
    let scrolled = ListOffset {
        item_ix: 3,
        offset_in_item: px(20.),
    };
    app.main.read_with(app.cx, |main, cx| {
        main.gallery().read(cx).list_state().scroll_to(scrolled);
    });
    let gallery = app
        .main
        .read_with(app.cx, |main, _| main.gallery().entity_id());

    app.cx.simulate_keystrokes("enter");
    let id = app.id_of("Notch-Ideen");
    assert_eq!(
        app.route(),
        Route::Note {
            id,
            query: String::new()
        }
    );
    assert!(
        app.editor_text()
            .starts_with("Notch-Ideen\n\nWas die Notch können soll")
    );

    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    app.main.read_with(app.cx, |main, cx| {
        assert!(main.note().is_none());
        assert_eq!(
            main.gallery().entity_id(),
            gallery,
            "the same view, not a new one"
        );
        let gallery = main.gallery().read(cx);
        assert_eq!(gallery.list_state().logical_scroll_top(), scrolled);
    });
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));

    // The gallery has the keyboard again.
    app.cx.simulate_keystrokes("down enter");
    assert!(app.editor_text().starts_with("Geschenkideen Jukek"));
    app.cx.simulate_keystrokes("ctrl-w");
    assert_eq!(app.route(), Route::Gallery);
}

#[gpui::test]
async fn with_animations_the_note_stays_until_it_has_slid_out(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);
    app.cx
        .update(|_, cx| theme::set_appearance(Appearance::default(), cx));
    assert!(app.cx.update(|_, cx| cx.tokens().animations));

    app.cx.simulate_keystrokes("down enter");
    let note = app
        .main
        .read_with(app.cx, |main, _| main.note().unwrap().downgrade());
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery, "the route changes at once");
    assert!(note.upgrade().is_some(), "the page is still on its way out");

    app.cx.executor().advance_clock(Duration::from_millis(349));
    app.cx.run_until_parked();
    assert!(note.upgrade().is_some());
    app.cx.executor().advance_clock(Duration::from_millis(2));
    app.cx.run_until_parked();
    assert!(note.upgrade().is_none(), "dropped after 350 ms");
}

// ---- Actions and keys ---------------------------------------------------------------

#[gpui::test]
async fn ctrl_n_creates_and_opens_a_note_and_escape_saves_it(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    let mut app = open_app(&notes, None, cx);
    assert!(app.titles().is_empty());

    app.cx.simulate_keystrokes("ctrl-n");
    assert!(matches!(app.route(), Route::Note { .. }));
    assert_eq!(notes.files(), ["Untitled.md"]);

    app.cx.simulate_input("Hallo Welt");
    assert_eq!(app.editor_text(), "Hallo Welt");
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(
        notes.files(),
        ["Hallo Welt.md"],
        "the file follows the title"
    );
    assert!(notes.read("Hallo Welt.md").ends_with("Hallo Welt"));
    assert_eq!(app.titles(), ["Hallo Welt"]);

    // Nothing typed, nothing written.
    let before = notes.read("Hallo Welt.md");
    app.cx.simulate_keystrokes("down enter");
    app.cx.simulate_keystrokes("ctrl-w");
    assert_eq!(notes.read("Hallo Welt.md"), before);
}

#[gpui::test]
async fn keys_of_the_gallery_pin_trash_and_focus_the_search(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);

    app.cx.simulate_keystrokes("down down ctrl-p");
    let pinned: Vec<String> = app
        .listed()
        .into_iter()
        .filter(|note| note.pinned)
        .map(|note| note.title)
        .collect();
    assert_eq!(pinned.len(), 3);
    assert!(pinned.contains(&"Notch-Ideen".to_owned()));
    assert!(notes.read("Arbeit/Notch-Ideen.md").contains("pinned: true"));

    app.cx.simulate_keystrokes("delete");
    assert_eq!(app.titles().len(), 13);
    assert!(!notes.files().contains(&"Arbeit/Notch-Ideen.md".to_owned()));
    assert!(notes.dir.join(".trash/Notch-Ideen.md").exists());

    let focused = |app: &mut TestApp| {
        app.main
            .read_with(app.cx, |main, cx| main.gallery().read(cx).search_focused())
    };
    assert!(!focused(&mut app));
    app.cx.simulate_keystrokes("ctrl-f");
    assert!(focused(&mut app));

    // The actions that have no target yet are accepted and change nothing.
    app.cx.simulate_keystrokes("ctrl-k ctrl-, ctrl-?");
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(app.titles().len(), 13);
}

#[gpui::test]
async fn the_pin_button_of_a_note_goes_through_its_worker(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let app = open_app(&notes, None, cx);
    app.cx.simulate_keystrokes("down down enter");
    app.cx.simulate_input("Vorab: ");
    app.cx.simulate_keystrokes("ctrl-p");
    assert!(notes.read("Arbeit/Notch-Ideen.md").contains("pinned: true"));

    // The save after the pin sees the file as the pin left it: no conflict copy.
    app.cx.simulate_keystrokes("escape");
    let text = notes.read("Arbeit/Vorab- Notch-Ideen.md");
    assert!(text.contains("pinned: true") && text.contains("Vorab: Notch-Ideen"));
    assert_eq!(notes.files().len(), 14);
}

// ---- Gallery, note and overlays together --------------------------------------------

impl TestApp {
    /// Runs the palette's entry for `typed`, as Ctrl+K, typing and Enter do.
    fn palette(&mut self, typed: &str) {
        self.cx.simulate_keystrokes("ctrl-k");
        self.cx.simulate_input(typed);
        self.cx.simulate_keystrokes("enter");
        self.cx.run_until_parked();
    }

    fn view(&mut self) -> View {
        self.main
            .read_with(self.cx, |main, cx| main.gallery().read(cx).view())
    }
}

#[gpui::test]
async fn delete_in_the_gallery_shows_the_window_toast_and_undo_restores(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);
    app.cx.simulate_keystrokes("down down delete");
    app.cx.run_until_parked();
    assert_eq!(app.titles().len(), 13);

    let toasts = app.main.read_with(app.cx, |main, _| main.toasts().clone());
    let shown = toasts.read_with(app.cx, |toasts, _| {
        toasts.shown().map(|toast| {
            let action = toast.action_label().map(ToString::to_string);
            (toast.message.to_string(), action)
        })
    });
    assert_eq!(
        shown,
        Some(("Note moved to trash".to_owned(), Some("Undo".to_owned())))
    );
    let notifications = app.cx.update(|window, cx| window.notifications(cx).len());
    assert_eq!(notifications, 0, "one toast, the window's");

    app.cx.update(|window, cx| {
        toasts.update(cx, |toasts, cx| toasts.run_action(window, cx));
    });
    app.cx.run_until_parked();
    assert_eq!(app.titles().len(), 14);
    assert!(notes.files().contains(&"Arbeit/Notch-Ideen.md".to_owned()));
}

#[gpui::test]
async fn the_palette_acts_on_the_gallery(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);
    assert_eq!(app.view(), View::Gallery);
    app.palette("toggle view");
    assert_eq!(app.view(), View::List);
    assert!(
        app.main
            .read_with(app.cx, |main, _| main.palette().is_none())
    );
    app.palette("toggle view");
    assert_eq!(app.view(), View::Gallery);

    app.cx.simulate_keystrokes("down down");
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));
    app.palette("pin or unpin");
    assert!(notes.read("Arbeit/Notch-Ideen.md").contains("pinned: true"));
    app.palette("move to trash");
    assert!(notes.dir.join(".trash/Notch-Ideen.md").exists());

    app.palette("new note");
    assert_eq!(app.editor_text(), "");
    assert_eq!(notes.files().len(), 14);
}

#[gpui::test]
async fn a_tag_or_search_in_a_note_ends_in_the_gallery_search_field(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);
    let search = |app: &mut TestApp| {
        app.main.read_with(app.cx, |main, cx| {
            let gallery = main.gallery().read(cx);
            (
                gallery.chips().to_vec(),
                gallery.search_text().to_owned(),
                gallery.search_focused(),
            )
        })
    };

    app.cx.simulate_keystrokes("down down enter");
    assert!(app.editor_text().starts_with("Notch-Ideen"));
    let note = app
        .main
        .read_with(app.cx, |main, _| main.note().unwrap().clone());
    let editor = note.read_with(app.cx, |note, _| note.editor().clone());
    editor.update(app.cx, |_, cx| {
        cx.emit(EditorEvent::TagClicked("rezepte".into()))
    });
    app.cx.run_until_parked();
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(
        search(&mut app),
        (vec![Filter::Tag("rezepte".to_owned())], String::new(), true)
    );
    assert_eq!(app.titles(), ["Shakshuka"]);

    // Search from a note keeps the search and puts the cursor into the field.
    app.cx.simulate_keystrokes("enter");
    assert!(matches!(app.route(), Route::Note { .. }));
    app.palette("search");
    assert_eq!(app.route(), Route::Gallery);
    let (chips, _, focused) = search(&mut app);
    assert_eq!(chips, [Filter::Tag("rezepte".to_owned())]);
    assert!(focused);
}

// ---- Requests from other launches ---------------------------------------------------

#[gpui::test]
async fn requests_of_other_launches_reach_the_window(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, None, cx);

    app.request(Request::Activate);
    assert_eq!(app.route(), Route::Gallery);

    let shakshuka = app.id_of("Shakshuka");
    app.request(Request::OpenNote {
        id: shakshuka.to_string(),
        query: "Feta".to_owned(),
    });
    assert_eq!(
        app.route(),
        Route::Note {
            id: shakshuka,
            query: "Feta".to_owned()
        }
    );
    let highlights = app.main.read_with(app.cx, |main, cx| {
        let note = main.note().unwrap().read(cx);
        note.editor().read(cx).highlights().len()
    });
    assert_eq!(highlights, 1, "the match of the search is marked");

    app.request(Request::Search {
        query: "#rezepte".to_owned(),
    });
    assert_eq!(app.route(), Route::Gallery);
    assert_eq!(app.titles(), ["Shakshuka"]);

    let uri = format!(
        "file://{}",
        notes.dir.join("Ideen für den Garten.md").display()
    )
    .replace(' ', "%20");
    app.request(Request::OpenUris(vec![uri]));
    assert!(app.editor_text().starts_with("Ideen für den Garten"));

    app.request(Request::OpenNote {
        id: "not an id".to_owned(),
        query: String::new(),
    });
    assert!(app.editor_text().starts_with("Ideen für den Garten"));

    app.request(Request::NewNote);
    assert_eq!(app.editor_text(), "");
    assert_eq!(notes.files().len(), 15);

    let toasts = |app: &mut TestApp| app.cx.update(|window, cx| window.notifications(cx).len());
    assert_eq!(toasts(&mut app), 0);
    app.request(Request::QuickNote);
    assert_eq!(toasts(&mut app), 1);
}

// ---- Theme --------------------------------------------------------------------------

/// Whether the last frame painted something in exactly this colour.
fn paints(app: &mut TestApp, color: Hsla) -> bool {
    app.cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window
            .painted_quads()
            .iter()
            .any(|quad| quad.background == Background::from(color))
    })
}

#[gpui::test]
async fn a_change_of_the_desktop_theme_reaches_tokens_components_and_views(
    cx: &mut TestAppContext,
) {
    let notes = Notes::seeded();
    let library = cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(Overrides::default(), cx);
        actions::bind_keys(cx);
        fixture::open_library(&notes, cx)
    });
    let mut app = open_window(library, None, cx);

    let dark = app.cx.update(|_, cx| cx.tokens().clone());
    assert!(dark.dark && dark.animations);
    assert_eq!(dark.window, Hsla::from(gpui::rgb(0x222226)));
    assert!(paints(&mut app, dark.window));
    app.cx.update(|_, cx| {
        assert!(Theme::global(cx).is_dark());
        assert_eq!(Theme::global(cx).primary, dark.accent);
        assert_eq!(Theme::global(cx).window_radius, px(15.));
    });

    app.cx.update(|_, cx| {
        let appearance = Appearance {
            dark: false,
            accent: 0xe62d42,
            reduced_motion: true,
            ..Appearance::default()
        };
        theme::set_appearance(appearance, cx);
    });
    let light = app.cx.update(|_, cx| cx.tokens().clone());
    assert!(!light.dark && !light.animations);
    assert_eq!(light.window, Hsla::from(gpui::rgb(0xfafafb)));
    assert_eq!(light.accent, Hsla::from(gpui::rgb(0xe62d42)));
    app.cx.update(|_, cx| {
        assert!(!Theme::global(cx).is_dark());
        assert_eq!(Theme::global(cx).primary, light.accent);
        assert_eq!(Theme::global(cx).background, light.window);
        assert!(cx.reduce_motion());
    });
    assert!(paints(&mut app, light.window));
    assert!(!paints(&mut app, dark.window));

    // An open note is drawn on the view colour and follows as well.
    app.cx.simulate_keystrokes("down enter");
    assert!(paints(&mut app, light.view));
    app.cx
        .update(|_, cx| theme::set_appearance(Appearance::default(), cx));
    assert!(paints(&mut app, dark.view));
    assert!(!paints(&mut app, light.view));
}

#[gpui::test]
fn theme_overrides_win_over_the_desktop(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        theme::init(Overrides::parse(Some("light"), Some("#9141ac")), cx);
        assert!(!cx.tokens().dark);

        theme::set_appearance(Appearance::default(), cx);
        assert!(!cx.tokens().dark, "SHEAF_THEME stays");
        assert_eq!(cx.tokens().accent, Hsla::from(gpui::rgb(0x9141ac)));
        assert!(theme::appearance(cx).dark, "what the desktop said is kept");
    });
}

// ---- Window state -------------------------------------------------------------------

#[gpui::test]
async fn the_window_opens_at_the_size_it_was_closed_with(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let state_file = StateFile::at(notes.scratch("state/sheaf/state.toml"));
    let viewport = |app: &mut TestApp| app.cx.update(|window, _| window.viewport_size());

    // First run: the default frame of 1100×720 inside its shadow.
    let mut app = open_app(&notes, Some(state_file.clone()), cx);
    assert_eq!(viewport(&mut app), size(px(1140.), px(760.)));
    assert_eq!(state_file.load(), None);

    app.cx.simulate_resize(size(px(900.), px(640.)));
    app.cx.dispatch_action(CloseWindow);
    app.cx.run_until_parked();
    assert_eq!(
        state_file.load(),
        Some(WindowState {
            width: 900.,
            height: 640.,
            maximized: false,
        })
    );
    assert!(app.window.update(app.cx, |_, _, _| ()).is_err(), "closed");

    let mut reopened = open_window(app.library.clone(), Some(state_file), app.cx);
    assert_eq!(viewport(&mut reopened), size(px(900.), px(640.)));
}
