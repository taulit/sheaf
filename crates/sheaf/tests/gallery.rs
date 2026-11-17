//! The gallery in a test window: groups and sort, scope chips, the search field with
//! its chips and highlights, keys and selection, the actions on notes, the view
//! toggle, the scroll position and the virtual list. No display, no bus, no files
//! outside a temporary folder.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{Entity, ListOffset, Modifiers, TestAppContext, VisualTestContext, WindowHandle, px};
use gpui_component::Root;
use sheaf::app::gallery::dates::Group;
use sheaf::app::gallery::{GalleryView, Popover, Scope};
use sheaf::app::library::Library;
use sheaf::app::window::{self, MainWindow, Route};
use sheaf_core::config::{Sort as SortOrder, View};
use sheaf_core::{CreateOptions, Feature, Filter, FolderConfig};

struct TestApp {
    library: Entity<Library>,
    main: Entity<MainWindow>,
    _window: WindowHandle<Root>,
    cx: &'static mut VisualTestContext,
}

fn open_app(notes: &Notes, cx: &mut TestAppContext) -> TestApp {
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(notes, cx)
    });
    open_window(library, cx)
}

fn open_window(library: Entity<Library>, cx: &mut TestAppContext) -> TestApp {
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    let mut app = TestApp {
        library,
        main,
        _window: window,
        cx,
    };
    app.draw();
    app
}

impl TestApp {
    fn gallery(&mut self) -> Entity<GalleryView> {
        self.main
            .read_with(self.cx, |main, _| main.gallery().clone())
    }

    fn read<R>(&mut self, read: impl FnOnce(&GalleryView) -> R) -> R {
        let gallery = self.gallery();
        gallery.read_with(self.cx, |gallery, _| read(gallery))
    }

    fn update(&mut self, update: impl FnOnce(&mut GalleryView, &mut gpui::Context<GalleryView>)) {
        let gallery = self.gallery();
        gallery.update(self.cx, update);
        self.settle();
    }

    /// Lets tasks and the search's pause run out, then draws a frame.
    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx.executor().advance_clock(Duration::from_millis(50));
        self.cx.run_until_parked();
        self.draw();
    }

    fn draw(&mut self) {
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        self.cx.run_until_parked();
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.settle();
    }

    fn titles(&mut self) -> Vec<String> {
        self.read(|gallery| {
            gallery
                .notes()
                .iter()
                .map(|note| note.title.clone())
                .collect()
        })
    }

    fn index_of(&mut self, title: &str) -> usize {
        let titles = self.titles();
        titles
            .iter()
            .position(|shown| shown == title)
            .unwrap_or_else(|| panic!("{title} is not shown"))
    }

    fn selected(&mut self) -> Option<String> {
        self.read(|gallery| gallery.selected().map(|note| note.title.clone()))
    }

    fn selection(&mut self) -> Vec<String> {
        self.read(|gallery| {
            gallery
                .selection()
                .iter()
                .map(|note| note.title.clone())
                .collect()
        })
    }

    fn groups(&mut self) -> Vec<(Option<Group>, usize)> {
        self.read(|gallery| {
            gallery
                .sections()
                .iter()
                .map(|section| (section.group, section.len))
                .collect()
        })
    }

    fn click(&mut self, title: &str, modifiers: Modifiers) {
        let index = self.index_of(title);
        self.update(|gallery, cx| gallery.click(index, modifiers, cx));
    }

    fn route(&mut self) -> Route {
        self.main.read_with(self.cx, |main, cx| main.route(cx))
    }

    fn toast(&mut self) -> Option<String> {
        self.main.read_with(self.cx, |main, cx| {
            main.toasts()
                .read(cx)
                .shown()
                .map(|toast| toast.message.to_string())
        })
    }
}

// ---- Groups, sort, scope ------------------------------------------------------------

#[gpui::test]
async fn notes_are_grouped_by_the_date_of_the_sort_order(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    assert_eq!(
        app.groups(),
        [
            (Some(Group::Pinned), 2),
            (Some(Group::Yesterday), 1),
            (Some(Group::Previous7Days), 3),
            (Some(Group::Previous30Days), 6),
            (
                Some(Group::Month {
                    year: 2026,
                    month: 10
                }),
                1
            ),
            (
                Some(Group::Month {
                    year: 2026,
                    month: 7
                }),
                1
            ),
        ]
    );
    let titles = app.titles();
    assert_eq!(
        &titles[..3],
        ["Einkauf", "Ausschreibung – offene Fragen", "Notch-Ideen"]
    );

    app.update(|gallery, cx| gallery.set_sort(SortOrder::Title, cx));
    assert_eq!(
        app.groups(),
        [(Some(Group::Pinned), 2), (Some(Group::Notes), 12)]
    );
    let titles = app.titles();
    assert_eq!(titles[2], "Geschenkideen Jukek");
    assert_eq!(titles[13], "Tax return 2025");

    // Created order groups by the creation date.
    app.update(|gallery, cx| gallery.set_sort(SortOrder::Created, cx));
    let groups = app.groups();
    assert_eq!(groups[0], (Some(Group::Pinned), 2));
    assert!(groups.iter().any(|(group, _)| *group
        == Some(Group::Month {
            year: 2026,
            month: 5
        })));
    assert_eq!(app.read(|gallery| gallery.sort()), SortOrder::Created);
}

#[gpui::test]
async fn scope_chips_restrict_to_a_folder_with_subfolders_or_a_tag(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);

    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Privat".to_owned()), cx));
    let mut titles = app.titles();
    titles.sort();
    assert_eq!(
        titles,
        [
            "Einkauf",
            "Geschenkideen Jukek",
            "Lesestoff",
            "Reise nach Lissabon",
            "Shakshuka"
        ]
    );
    assert_eq!(app.read(|gallery| gallery.current_folder()), "Privat");

    app.update(|gallery, cx| gallery.set_scope(Scope::Tag("rezepte".to_owned()), cx));
    assert_eq!(app.titles(), ["Shakshuka"]);
    assert_eq!(app.read(|gallery| gallery.current_folder()), "");

    app.update(|gallery, cx| gallery.set_scope(Scope::All, cx));
    assert_eq!(app.titles().len(), 14);

    // The folder popover lists every folder.
    app.update(|gallery, cx| gallery.set_popover(Some(Popover::Folders), cx));
    let folders: Vec<String> = app.read(|gallery| {
        gallery
            .folders()
            .iter()
            .map(|folder| folder.path.clone())
            .collect()
    });
    assert!(folders.contains(&"Privat/Rezepte".to_owned()));
    app.keys("escape");
    assert_eq!(app.read(|gallery| gallery.popover()), None);
}

#[gpui::test]
async fn ctrl_n_creates_the_note_in_the_folder_shown(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Work".to_owned()), cx));
    app.keys("ctrl-n");
    assert!(matches!(app.route(), Route::Note { .. }));
    assert!(notes.files().contains(&"Work/Untitled.md".to_owned()));
}

// ---- Search -------------------------------------------------------------------------

#[gpui::test]
async fn typing_in_the_gallery_searches_and_filters_become_chips(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);

    app.keys("k a f f e e");
    assert!(app.read(|gallery| gallery.search_focused()));
    assert_eq!(
        app.read(|gallery| gallery.search_text().to_owned()),
        "kaffee"
    );
    assert_eq!(app.titles(), ["Einkauf"]);
    let id = app.read(|gallery| gallery.notes()[0].id.clone());
    let snippet = app.read(|gallery| gallery.marks(&id).unwrap().snippet.clone());
    assert!(!snippet.highlights.is_empty());
    let marked = &snippet.text[snippet.highlights[0].clone()];
    assert!(marked.to_lowercase().starts_with("kaffee"), "{marked}");
    // Search results are ranked, without date groups.
    assert_eq!(app.groups(), [(None, 1)]);

    // Esc empties the field, then leaves it.
    app.keys("escape");
    assert_eq!(app.read(|gallery| gallery.query()), "");
    assert_eq!(app.titles().len(), 14);
    assert!(app.read(|gallery| gallery.search_focused()));
    app.keys("escape");
    assert!(!app.read(|gallery| gallery.search_focused()));

    // A filter becomes a chip once a blank follows it; German keywords too.
    app.keys("ctrl-f");
    app.cx.simulate_input("hat:checkliste");
    app.settle();
    assert!(app.read(|gallery| gallery.chips().is_empty()));
    app.cx.simulate_input(" für");
    app.settle();
    assert_eq!(
        app.read(|gallery| gallery.chips().to_vec()),
        [Filter::Has(Feature::Checklist)]
    );
    assert_eq!(app.read(|gallery| gallery.search_text().to_owned()), "für");
    let mut titles = app.titles();
    titles.sort();
    assert_eq!(titles, ["Einkauf", "Shakshuka"]);
    let shakshuka = app.read(|gallery| {
        let note = gallery
            .notes()
            .iter()
            .find(|note| note.title == "Shakshuka")
            .unwrap();
        gallery.marks(&note.id).unwrap().clone()
    });
    assert!(!shakshuka.snippet.highlights.is_empty());

    // Enter opens the first hit with the whole query.
    let first = app.read(|gallery| gallery.notes()[0].id.clone());
    app.keys("enter");
    assert_eq!(
        app.route(),
        Route::Note {
            id: first,
            query: "has:checklist für".to_owned()
        }
    );
    app.keys("escape");

    // Backspace at the start of the field takes the last chip.
    app.update(|gallery, cx| gallery.set_query("#rezepte", cx));
    assert_eq!(app.read(|gallery| gallery.chips().len()), 1);
    assert_eq!(app.titles(), ["Shakshuka"]);
    app.keys("ctrl-f backspace");
    assert!(app.read(|gallery| gallery.chips().is_empty()));
    assert_eq!(app.titles().len(), 14);

    // A chip's × removes it.
    app.update(|gallery, cx| gallery.set_query("pinned: open:", cx));
    assert_eq!(app.read(|gallery| gallery.chips().len()), 2);
    app.update(|gallery, cx| gallery.remove_chip(0, cx));
    assert_eq!(app.read(|gallery| gallery.chips().to_vec()), [Filter::Open]);
}

#[gpui::test]
async fn an_empty_result_offers_to_search_without_filters(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Work".to_owned()), cx));
    app.update(|gallery, cx| gallery.set_query("#rezepte feta", cx));
    assert!(app.titles().is_empty());

    app.update(|gallery, cx| gallery.search_without_filters(cx));
    assert_eq!(app.read(|gallery| gallery.scope().clone()), Scope::All);
    assert_eq!(app.read(|gallery| gallery.query()), "feta");
    assert_eq!(app.titles(), ["Shakshuka"]);
}

// ---- Keys and selection -------------------------------------------------------------

#[gpui::test]
async fn arrow_keys_move_through_the_grid_and_clicks_select_several(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);

    app.keys("right");
    assert_eq!(app.selected().as_deref(), Some("Einkauf"));
    app.keys("right");
    assert_eq!(
        app.selected().as_deref(),
        Some("Ausschreibung – offene Fragen")
    );
    // Down from the second column of the pinned row: the row of yesterday has one card.
    app.keys("down");
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));
    app.keys("down right");
    assert_eq!(app.selected().as_deref(), Some("Shakshuka"));
    app.keys("up");
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));
    app.keys("left left");
    assert_eq!(app.selected().as_deref(), Some("Einkauf"));

    // In the list, down is the next note.
    app.update(|gallery, cx| gallery.set_view(View::List, cx));
    app.keys("down down");
    assert_eq!(app.selected().as_deref(), Some("Notch-Ideen"));

    // Ctrl toggles, Shift extends from the last clicked note.
    let ctrl = Modifiers::control();
    app.click("Shakshuka", ctrl);
    assert_eq!(app.selection(), ["Notch-Ideen", "Shakshuka"]);
    app.click("Lesestoff", Modifiers::shift());
    assert_eq!(
        app.selection(),
        ["Shakshuka", "Sprint Review 12.11.", "Lesestoff"]
    );
    app.click("Sprint Review 12.11.", ctrl);
    assert_eq!(app.selection(), ["Shakshuka", "Lesestoff"]);

    // Ctrl+P pins the whole selection, a second time unpins it.
    app.keys("ctrl-p");
    assert!(
        notes
            .read("Privat/Rezepte/Shakshuka.md")
            .contains("pinned: true")
    );
    assert!(notes.read("Privat/Lesestoff.md").contains("pinned: true"));
    assert_eq!(app.groups()[0], (Some(Group::Pinned), 4));
    let mut selection = app.selection();
    selection.sort();
    assert_eq!(
        selection,
        ["Lesestoff", "Shakshuka"],
        "the selection follows its notes"
    );
    app.keys("ctrl-p");
    assert!(!notes.read("Privat/Lesestoff.md").contains("pinned: true"));

    // A plain click opens.
    app.click("Lesestoff", Modifiers::none());
    assert!(matches!(app.route(), Route::Note { .. }));
}

#[gpui::test]
async fn delete_moves_the_selection_to_the_trash_and_undo_restores_it(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.click("Notch-Ideen", Modifiers::control());
    app.click("Lesestoff", Modifiers::control());
    app.keys("delete");
    assert_eq!(app.titles().len(), 12);
    assert!(notes.dir.join(".trash/Lesestoff.md").exists());
    let toasts = app.main.read_with(app.cx, |main, _| main.toasts().clone());
    let shown = toasts.read_with(app.cx, |toasts, _| {
        toasts.shown().map(|toast| toast.message.to_string())
    });
    assert_eq!(shown.as_deref(), Some("2 notes moved to trash"));

    app.cx
        .update(|window, cx| toasts.update(cx, |toasts, cx| toasts.run_action(window, cx)));
    app.settle();
    assert_eq!(app.titles().len(), 14);
    assert!(notes.files().contains(&"Privat/Lesestoff.md".to_owned()));
    assert!(notes.files().contains(&"Arbeit/Notch-Ideen.md".to_owned()));
}

type Change = fn(&mut GalleryView, &mut gpui::Context<GalleryView>);

#[gpui::test]
async fn a_change_that_fails_says_why_and_shows_the_note_as_it_is(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let changes: [(&str, &str, Change, &str); 4] = [
        (
            "Privat/Lesestoff.md",
            "Lesestoff",
            |gallery, cx| gallery.toggle_pin(cx),
            "Could not pin the note: ",
        ),
        (
            "Arbeit/Notch-Ideen.md",
            "Notch-Ideen",
            |gallery, cx| gallery.move_selection_to("Work", cx),
            "Could not move the note: ",
        ),
        (
            "Privat/Einkauf.md",
            "Einkauf",
            |gallery, cx| gallery.duplicate_selection(cx),
            "Could not duplicate the note: ",
        ),
        (
            "Reading list.md",
            "Reading list",
            |gallery, cx| gallery.trash_selection(cx),
            "Could not move to trash: ",
        ),
    ];
    for (path, title, change, message) in changes {
        // Deleted by another program, still listed.
        std::fs::remove_file(notes.dir.join(path)).unwrap();
        app.click(title, Modifiers::control());
        app.update(change);
        let toast = app.toast().unwrap_or_default();
        assert!(toast.starts_with(message), "{title}: {toast:?}");
        assert!(!app.titles().contains(&title.to_owned()), "{title} is gone");
    }
    assert_eq!(app.titles().len(), 10);
}

#[gpui::test]
async fn the_context_menu_moves_and_duplicates(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.click("Sprint Review 12.11.", Modifiers::control());
    app.update(|gallery, cx| gallery.move_selection_to("Work", cx));
    assert!(
        notes
            .files()
            .contains(&"Work/Sprint Review 12.11..md".to_owned())
    );
    assert_eq!(app.selected().as_deref(), Some("Sprint Review 12.11."));

    app.update(|gallery, cx| gallery.duplicate_selection(cx));
    let copies = app
        .titles()
        .iter()
        .filter(|title| title.starts_with("Sprint Review"))
        .count();
    assert_eq!(copies, 2);
    assert_eq!(notes.files().len(), 15);
}

// ---- View, scroll, reload, virtual list ---------------------------------------------

#[gpui::test]
async fn the_view_is_remembered_per_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    assert_eq!(app.read(|gallery| gallery.view()), View::Gallery);
    app.update(|gallery, cx| gallery.set_view(View::List, cx));
    let rows = app.read(|gallery| gallery.rows().len());
    assert_eq!(rows, 14 + 6, "one row per note and per heading");

    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Arbeit".to_owned()), cx));
    assert_eq!(app.read(|gallery| gallery.view()), View::Gallery);

    let config = FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.view("").view, View::List);
    assert_eq!(
        config.pinned_tags,
        ["ausschreibung", "rezepte"],
        "other settings stay"
    );

    // A new window on the same folder starts with what was chosen.
    let library = app.library.clone();
    let mut again = open_window(library, app.cx);
    assert_eq!(again.read(|gallery| gallery.view()), View::List);
}

const FULL_CONFIG: &str = r#"pinned_tags = ["ausschreibung", "rezepte"]
future_setting = "kept"

[folders.Arbeit]
sort = "title"
density = 2

[[smart_folders]]
name = "Offen"
query = "offen:"
color = "blue"

[notes.01JD3K8Z6Q4X7V9B2M5N8P0R3T]
folded = ["Anhang"]
"#;

/// The settings file as plain TOML, to see keys this version does not know.
fn stored_config(notes: &Notes) -> toml::Table {
    notes.read(".sheaf/config.toml").parse().unwrap()
}

fn assert_kept(config: &toml::Table) {
    assert_eq!(
        config["pinned_tags"].as_array().unwrap().len(),
        2,
        "{config}"
    );
    assert_eq!(config["future_setting"].as_str(), Some("kept"), "{config}");
    assert_eq!(config["folders"]["Arbeit"]["sort"].as_str(), Some("title"));
    assert_eq!(config["folders"]["Arbeit"]["density"].as_integer(), Some(2));
    let smart = config["smart_folders"].as_array().unwrap();
    assert_eq!(smart[0]["color"].as_str(), Some("blue"), "{config}");
    assert!(config["notes"]["01JD3K8Z6Q4X7V9B2M5N8P0R3T"].is_table());
}

#[gpui::test]
async fn a_search_at_launch_keeps_the_folder_settings(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(".sheaf/config.toml", FULL_CONFIG);
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(&notes, cx)
    });
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx).into_mut();
    // A search from the command line arrives before the settings are read.
    let gallery = main.read_with(vcx, |main, _| main.gallery().clone());
    gallery.update(vcx, |gallery, cx| gallery.set_query("Rezept", cx));
    vcx.run_until_parked();
    assert_eq!(
        gallery.read_with(vcx, |gallery, _| gallery.smart_folders().len()),
        1
    );

    gallery.update(vcx, |gallery, cx| gallery.set_view(View::List, cx));
    vcx.run_until_parked();

    let config = stored_config(&notes);
    assert_kept(&config);
    assert_eq!(config["folders"][""]["view"].as_str(), Some("list"));
}

#[gpui::test]
async fn nothing_is_changed_before_the_folder_settings_are_read(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(".sheaf/config.toml", FULL_CONFIG);
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        fixture::open_library(&notes, cx)
    });
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let gallery = main.read_with(vcx, |main, _| main.gallery().clone());
    // A change while the settings are still being read: it is not written over them.
    gallery.update(vcx, |gallery, cx| gallery.set_view(View::List, cx));
    vcx.run_until_parked();
    assert_kept(&stored_config(&notes));
    assert_eq!(notes.read(".sheaf/config.toml"), FULL_CONFIG);
}

#[gpui::test]
async fn every_writer_of_the_folder_settings_keeps_the_keys_of_the_others(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(".sheaf/config.toml", FULL_CONFIG);
    let mut app = open_app(&notes, cx);

    // Another writer changes the file after the gallery read it.
    let id: sheaf_core::NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();
    let library = app.library.clone();
    let quick = app.cx.update(|_, cx| {
        library
            .read(cx)
            .update_folder_config(|config| config.quick_note.resume_minutes = 60, cx)
    });
    app.cx.run_until_parked();
    app.cx.foreground_executor().block_test(quick).unwrap();

    app.update(|gallery, cx| gallery.set_view(View::List, cx));
    app.update(|gallery, cx| gallery.set_query("#rezepte", cx));
    app.update(|gallery, cx| gallery.save_smart_folder("Rezepte", cx));
    app.update(|gallery, cx| gallery.set_sort(SortOrder::Title, cx));
    let state = app.cx.update(|_, cx| {
        let id = id.clone();
        library.read(cx).update_folder_config(
            move |config| {
                config.notes.remove(&id);
            },
            cx,
        )
    });
    app.cx.run_until_parked();
    app.cx.foreground_executor().block_test(state).unwrap();

    let config = stored_config(&notes);
    assert_eq!(config["future_setting"].as_str(), Some("kept"));
    assert_eq!(config["folders"]["Arbeit"]["density"].as_integer(), Some(2));
    assert_eq!(config["folders"][""]["view"].as_str(), Some("list"));
    assert_eq!(
        config["quick_note"]["resume_minutes"].as_integer(),
        Some(60)
    );
    let smart = config["smart_folders"].as_array().unwrap();
    assert_eq!(smart.len(), 2);
    assert_eq!(smart[0]["color"].as_str(), Some("blue"));
    assert_eq!(smart[1]["name"].as_str(), Some("Rezepte"));
    assert!(config.get("notes").is_none(), "{config}");
    let stored = FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(stored.view("").sort, SortOrder::Title);
}

#[cfg(unix)]
#[gpui::test]
async fn a_view_that_cannot_be_stored_says_so_and_goes_back(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let sheaf_dir = notes.dir.join(".sheaf");
    std::fs::set_permissions(&sheaf_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    app.update(|gallery, cx| gallery.set_view(View::List, cx));
    std::fs::set_permissions(&sheaf_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(app.read(|gallery| gallery.view()), View::Gallery);
    let toast = app.toast().unwrap_or_default();
    assert!(
        toast.starts_with("Could not save the folder settings"),
        "{toast}"
    );
    assert_eq!(
        FolderConfig::load(&notes.dir).unwrap().pinned_tags,
        ["ausschreibung", "rezepte"]
    );
}

#[gpui::test]
async fn the_gallery_follows_changes_and_keeps_its_place(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_resize(gpui::size(px(1100.), px(420.)));
    app.settle();
    let top = ListOffset {
        item_ix: 5,
        offset_in_item: px(12.),
    };
    app.read(|gallery| gallery.list_state().scroll_to(top));
    let first_visible = |app: &mut TestApp| {
        app.read(|gallery| {
            let row = gallery.list_state().logical_scroll_top().item_ix;
            gallery.rows()[row].clone()
        })
    };
    app.settle();
    let before = first_visible(&mut app);

    // Another program pins a note: the library tells, the gallery reloads.
    let id = app.read(|gallery| {
        let note = gallery
            .notes()
            .iter()
            .find(|note| note.title == "Tax return 2025")
            .unwrap();
        note.id.clone()
    });
    let pinned = app
        .library
        .update(app.cx, |library, cx| library.set_pinned(&id, true, cx));
    pinned.await.unwrap();
    app.settle();
    assert_eq!(app.groups()[0], (Some(Group::Pinned), 3));
    assert!(app.titles()[..3].contains(&"Tax return 2025".to_owned()));
    // Rows moved by one note, but the same notes are at the top.
    let after = first_visible(&mut app);
    assert_ne!(before, after);
    assert_eq!(
        app.read(|gallery| gallery.list_state().logical_scroll_top().item_ix),
        5
    );

    // A note created elsewhere appears.
    let created = app.library.update(app.cx, |library, cx| {
        library.create("Neu von außen", CreateOptions::default(), cx)
    });
    created.await.unwrap();
    app.settle();
    assert!(app.titles().contains(&"Neu von außen".to_owned()));
}

#[gpui::test]
async fn only_the_visible_cards_are_built(cx: &mut TestAppContext) {
    let notes = Notes::empty();
    for index in 0..5000 {
        notes.write(
            &format!("Ordner {}/Notiz {index}.md", index % 20),
            &format!("Notiz {index}\n\n- [ ] Aufgabe {index}\n\nText {index}.\n"),
        );
    }
    let mut app = open_app(&notes, cx);
    assert_eq!(app.titles().len(), 5000);
    let built = app.read(|gallery| gallery.previews_requested());
    assert!(built > 0 && built < 100, "{built} cards were built");

    // Scrolling far builds the cards there, not those in between.
    app.read(|gallery| {
        gallery.list_state().scroll_to(ListOffset {
            item_ix: 600,
            offset_in_item: px(0.),
        });
    });
    app.draw();
    app.draw();
    let built = app.read(|gallery| gallery.previews_requested());
    assert!(built < 200, "{built} cards were built");
}

#[gpui::test]
async fn shift_click_after_the_anchor_was_deleted_does_not_crash(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let last = app.titles().last().cloned().unwrap();
    app.click(&last, Modifiers::control());
    app.keys("delete");
    assert_eq!(app.titles().len(), 13);

    let first = app.titles()[0].clone();
    app.click(&first, Modifiers::shift());
    assert_eq!(app.selected().as_deref(), Some(first.as_str()));
    assert!(app.selection().contains(&first));

    // The anchor is trashed from elsewhere: Shift+click still works.
    let titles = app.titles();
    let gone = titles.last().unwrap().clone();
    app.click(&gone, Modifiers::control());
    let id = app.read(|gallery| gallery.notes().last().unwrap().id.clone());
    let library = app.library.clone();
    library
        .update(app.cx, |library, cx| library.trash(&id, cx))
        .detach();
    app.settle();
    let third = app.titles()[2].clone();
    app.click(&third, Modifiers::shift());
    assert_eq!(app.selected().as_deref(), Some(third.as_str()));
}
