//! Templates in a test window: the `+ ▾` menu lists them, a note made from one has
//! its variables filled in from the Library's clock, and "Edit Templates…" creates the
//! folder.

#![allow(clippy::unwrap_used)]

mod fixture;

use fixture::{Notes, TestApp, open_app};
use gpui::TestAppContext;
use sheaf::app::actions::ShowTemplates;
use sheaf::app::gallery::{Popover, Scope};
use sheaf::app::window::Route;
use sheaf_core::templates;

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

impl TestApp {
    fn note_menu(&mut self) -> Option<Vec<String>> {
        self.main.read_with(self.cx, |main, cx| {
            let note = main.note()?.read(cx);
            note.template_menu().map(|menu| menu.names().to_vec())
        })
    }

    fn note_text(&mut self) -> String {
        self.main.read_with(self.cx, |main, cx| {
            let note = main.note().unwrap().read(cx);
            note.editor().read(cx).markdown()
        })
    }
}

/// `/vorlage` (and `/template`) in the text: the slash menu's entry lists the templates,
/// the keys choose one, and it is filled in at the caret.
#[gpui::test]
async fn the_slash_menu_inserts_a_template(cx: &mut TestAppContext) {
    let notes = with_templates();
    let mut app = open_app(&notes, cx);
    app.cx.simulate_keystrokes("ctrl-n");
    app.cx.run_until_parked();

    app.cx.simulate_input("/vorlage");
    app.cx.simulate_keystrokes("enter");
    app.cx.run_until_parked();
    assert_eq!(
        app.note_menu(),
        Some(vec!["Anruf".to_owned(), "Protokoll".to_owned()])
    );
    assert_eq!(app.note_text().trim(), "", "the typed /vorlage is gone");

    // Esc closes the menu and stays in the note.
    app.cx.simulate_keystrokes("escape");
    assert_eq!(app.note_menu(), None);
    assert!(matches!(
        app.main.read_with(app.cx, |main, cx| main.route(cx)),
        Route::Note { .. }
    ));

    app.cx.simulate_input("/template");
    app.cx.simulate_keystrokes("enter");
    app.cx.run_until_parked();
    app.cx.simulate_keystrokes("down down up enter");
    app.cx.run_until_parked();
    assert_eq!(app.note_menu(), None);
    let text = app.note_text();
    assert_eq!(text, "#\n\nDatum: 2026-11-16, 13:24\n\n- [ ] Teilnehmer\n");

    // The keyboard is back in the text.
    app.cx.simulate_input("Wochenrunde");
    assert!(
        app.note_text().contains("Wochenrunde"),
        "{}",
        app.note_text()
    );
}
