//! Templates in a test window: the `+ ▾` menu lists them, a note made from one has
//! its variables filled in from the Library's clock, and "Edit Templates…" creates the
//! folder.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::time::Duration;

use fixture::{Look, Notes};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheaf::app::actions::ShowTemplates;
use sheaf::app::gallery::{GalleryView, Popover, Scope};
use sheaf::app::window::{self, MainWindow, Route};
use sheaf_core::templates;

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
    }
}

fn with_templates() -> Notes {
    let notes = Notes::seeded();
    notes.write(
        ".sheaf/templates/Protokoll.md",
        "# {{title}}\n\nDatum: {{date}}, {{time}}\n\n- [ ] Teilnehmer\n",
    );
    notes.write(
        ".sheaf/templates/Anruf.md",
        "Anruf {{date}}\n\n{{unbekannt}}\n",
    );
    notes
}

#[gpui::test]
async fn the_menu_lists_the_templates(cx: &mut TestAppContext) {
    let notes = with_templates();
    let mut app = open_app(&notes, cx);
    app.cx
        .update(|window, cx| window.dispatch_action(Box::new(ShowTemplates), cx));
    app.cx.run_until_parked();
    assert_eq!(
        app.read(|gallery| gallery.popover()),
        Some(Popover::Templates)
    );
    let names = app.read(|gallery| gallery.templates().to_vec());
    assert_eq!(names, ["Anruf", "Protokoll"]);
}

#[gpui::test]
async fn a_note_from_a_template_has_its_variables_filled_in(cx: &mut TestAppContext) {
    let notes = with_templates();
    let mut app = open_app(&notes, cx);
    app.update(|gallery, cx| gallery.set_scope(Scope::Folder("Arbeit".into()), cx));
    app.update(|gallery, cx| gallery.set_popover(Some(Popover::Templates), cx));
    app.update(|gallery, cx| gallery.new_from_template("Protokoll", cx));
    assert!(matches!(
        app.main.read_with(app.cx, |main, cx| main.route(cx)),
        Route::Note { .. }
    ));
    assert_eq!(app.read(|gallery| gallery.popover()), None);
    let text = notes.read("Arbeit/Untitled.md");
    assert!(
        text.ends_with("# \n\nDatum: 2026-11-16, 13:24\n\n- [ ] Teilnehmer\n"),
        "{text}"
    );
    let editor = app.main.read_with(app.cx, |main, cx| {
        let note = main.note().unwrap().read(cx);
        note.editor().read(cx).markdown()
    });
    assert!(editor.contains("Datum: 2026-11-16, 13:24"), "{editor}");

    // Unknown placeholders stay as they are.
    app.cx.simulate_keystrokes("escape");
    app.cx.run_until_parked();
    app.update(|gallery, cx| gallery.new_from_template("Anruf", cx));
    let files = notes.files();
    assert!(
        files.contains(&"Arbeit/Anruf 2026-11-16.md".to_owned()),
        "{files:?}"
    );
    assert_eq!(
        notes
            .read("Arbeit/Anruf 2026-11-16.md")
            .rsplit("---\n")
            .next(),
        Some("Anruf 2026-11-16\n\n{{unbekannt}}\n")
    );
}

#[gpui::test]
async fn edit_templates_creates_the_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    assert!(!templates::dir(&notes.dir).exists());
    app.update(|gallery, cx| gallery.set_popover(Some(Popover::Templates), cx));
    assert!(app.read(|gallery| gallery.templates().is_empty()));
    app.update(|gallery, cx| gallery.edit_templates(cx));
    assert!(templates::dir(&notes.dir).is_dir());
    let opened = app.cx.opened_url();
    assert!(
        opened
            .as_deref()
            .is_some_and(|url| url.ends_with("/.sheaf/templates")),
        "{opened:?}"
    );
    assert_eq!(app.read(|gallery| gallery.popover()), None);
}
