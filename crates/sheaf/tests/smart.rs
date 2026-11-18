//! Smart folders in a test window: saved from a search, listed, run, renamed and
//! deleted, and stored in the folder settings without losing what Sheaf does not know.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheaf::app::actions::SaveSearchAsSmartFolder;
use sheaf::app::gallery::{GalleryView, Popover, Scope};
use sheaf::app::window::{self, MainWindow};
use sheaf_core::FolderConfig;

struct TestApp {
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
    TestApp { main, cx }
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
        self.cx.run_until_parked();
        self.cx.executor().advance_clock(Duration::from_millis(50));
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        self.cx.run_until_parked();
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

    fn names(&mut self) -> Vec<String> {
        self.read(|gallery| {
            let folders = gallery.smart_folders().iter();
            folders.map(|folder| folder.name.clone()).collect()
        })
    }
}

#[gpui::test]
async fn a_search_is_saved_as_a_smart_folder_and_shown(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    // Nothing to save without a search.
    app.update(|gallery, cx| gallery.save_smart_folder("Leer", cx));
    assert!(app.names().is_empty());

    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Arbeit".into()), cx));
    app.update(|gallery, cx| gallery.set_query("offen:", cx));
    let found = app.titles();
    assert_eq!(found, ["Ausschreibung – offene Fragen"]);
    app.update(|gallery, cx| gallery.save_smart_folder(" Offen bei der Arbeit ", cx));

    assert_eq!(app.names(), ["Offen bei der Arbeit"]);
    let scope = app.read(|gallery| gallery.scope().clone());
    assert_eq!(scope, Scope::Smart("Offen bei der Arbeit".into()));
    assert_eq!(
        app.read(|gallery| gallery.query()),
        "",
        "the search field is empty"
    );
    assert_eq!(app.titles(), found);

    let config = FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.smart_folders.len(), 1);
    let saved = &config.smart_folders[0];
    assert_eq!(saved.name, "Offen bei der Arbeit");
    assert_eq!(saved.icon, "folder-saved-search-symbolic");
    assert!(saved.query.contains("Arbeit"), "{}", saved.query);
    // What the folder settings had before is still there.
    assert_eq!(config.pinned_tags, ["ausschreibung", "rezepte"]);

    // A second one of the same name gets a number.
    app.update(|gallery, cx| gallery.set_query("#rezepte", cx));
    app.update(|gallery, cx| gallery.save_smart_folder("Offen bei der Arbeit", cx));
    assert_eq!(
        app.names(),
        ["Offen bei der Arbeit", "Offen bei der Arbeit 2"]
    );
}

#[gpui::test]
async fn smart_folders_from_the_settings_are_listed_and_run(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(
        ".sheaf/config.toml",
        "pinned_tags = [\"rezepte\"]\nfuture_setting = \"kept\"\n\n\
         [[smart_folders]]\nname = \"Rezepte\"\nicon = \"folder-saved-search-symbolic\"\n\
         query = \"#rezepte\"\ncolor = \"orange\"\n\n\
         [[smart_folders]]\nname = \"Lissabon\"\nquery = \"lissabon\"\n",
    );
    let mut app = open_app(&notes, cx);
    assert_eq!(app.names(), ["Rezepte", "Lissabon"]);
    app.update(|gallery, cx| gallery.set_scope(Scope::Smart("Rezepte".into()), cx));
    assert_eq!(app.titles(), ["Shakshuka"]);
    app.update(|gallery, cx| gallery.set_scope(Scope::Smart("Lissabon".into()), cx));
    assert_eq!(app.titles(), ["Reise nach Lissabon"]);
    // Typing narrows the smart folder further.
    app.update(|gallery, cx| gallery.set_query("gibt es nicht", cx));
    assert!(app.titles().is_empty());
}

#[gpui::test]
async fn smart_folders_are_renamed_and_deleted_losslessly(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(
        ".sheaf/config.toml",
        "future_setting = \"kept\"\n\n\
         [[smart_folders]]\nname = \"Rezepte\"\nquery = \"#rezepte\"\ncolor = \"orange\"\n\n\
         [[smart_folders]]\nname = \"Urlaub\"\nquery = \"#urlaub\"\n",
    );
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_scope(Scope::Smart("Rezepte".into()), cx));
    app.update(|gallery, cx| gallery.rename_smart_folder("Rezepte", "Kochen", cx));
    assert_eq!(app.names(), ["Kochen", "Urlaub"]);
    let scope = app.read(|gallery| gallery.scope().clone());
    assert_eq!(scope, Scope::Smart("Kochen".into()));
    assert_eq!(app.read(|gallery| gallery.scope().title()), "Kochen");
    assert_eq!(app.titles(), ["Shakshuka"]);

    let config = FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.smart_folders[0].name, "Kochen");
    assert_eq!(config.smart_folders[0].query, "#rezepte");
    assert_eq!(
        config.smart_folders[0].other["color"].as_str(),
        Some("orange")
    );
    assert_eq!(config.other["future_setting"].as_str(), Some("kept"));

    app.update(|gallery, cx| gallery.delete_smart_folder("Kochen", cx));
    assert_eq!(app.names(), ["Urlaub"]);
    assert_eq!(app.read(|gallery| gallery.scope().clone()), Scope::All);
    let config = FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.smart_folders.len(), 1);
    assert_eq!(config.other["future_setting"].as_str(), Some("kept"));
}

#[gpui::test]
async fn the_palette_action_asks_for_a_name(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_query("#urlaub", cx));
    app.cx
        .update(|window, cx| window.dispatch_action(Box::new(SaveSearchAsSmartFolder), cx));
    app.cx.run_until_parked();
    assert_eq!(
        app.read(|gallery| gallery.popover()),
        Some(Popover::Folders)
    );
    app.cx.simulate_input("Ferien");
    app.cx.simulate_keystrokes("enter");
    app.cx.run_until_parked();
    assert_eq!(app.names(), ["Ferien"]);
    assert_eq!(app.titles(), ["Reise nach Lissabon"]);
}

#[gpui::test]
async fn a_symbol_is_chosen_under_the_name(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_query("#urlaub", cx));
    app.cx
        .update(|window, cx| window.dispatch_action(Box::new(SaveSearchAsSmartFolder), cx));
    app.cx.run_until_parked();
    app.cx.update(|window, cx| window.draw(cx).clear(cx));
    let travel = app
        .cx
        .debug_bounds("smart-icon-emoji-travel-symbolic")
        .expect("the symbols are shown");
    app.cx
        .simulate_click(travel.center(), gpui::Modifiers::none());
    // The name field keeps the keys.
    app.cx.simulate_input("Ferien");
    app.cx.simulate_keystrokes("enter");
    app.cx.run_until_parked();
    let icon_of = |notes: &Notes, name: &str| {
        let config = FolderConfig::load(&notes.dir).unwrap();
        let folder = config
            .smart_folders
            .iter()
            .find(|f| f.name == name)
            .cloned();
        folder.unwrap().icon
    };
    assert_eq!(icon_of(&notes, "Ferien"), "emoji-travel-symbolic");

    app.update(|gallery, cx| gallery.set_smart_icon("Ferien", "starred-symbolic", cx));
    assert_eq!(icon_of(&notes, "Ferien"), "starred-symbolic");
}
