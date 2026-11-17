//! Export, printing and attachments of the note view, backlinks and the settings, in a
//! test window. Dialogs are scripted: no portal is asked, no dialog opens.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::path::PathBuf;
use std::sync::Arc;

use fixture::{Look, Notes};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheaf::app::dialogs::{Dialogs, Scripted};
use sheaf::app::instance::Request;
use sheaf::app::library::Library;
use sheaf::app::note::{NoteView, Upload};
use sheaf::app::window::{self, MainWindow};
use sheaf_core::NoteId;

struct TestApp {
    library: Entity<Library>,
    main: Entity<MainWindow>,
    dialogs: Arc<Scripted>,
    cx: &'static mut VisualTestContext,
}

fn open_app(notes: &Notes, cx: &mut TestAppContext) -> TestApp {
    let dialogs = Arc::new(Scripted::default());
    let library = cx.update(|cx| {
        fixture::init_app(Look::default(), cx);
        cx.set_global(Dialogs::new(dialogs.clone()));
        fixture::open_library(notes, cx)
    });
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), None, cx))
        .unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    TestApp {
        library,
        main,
        dialogs,
        cx,
    }
}

impl TestApp {
    fn id_of(&mut self, title: &str) -> NoteId {
        let listed = self
            .main
            .read_with(self.cx, |main, cx| main.gallery().read(cx).notes().to_vec());
        let note = listed.into_iter().find(|note| note.title == title);
        note.unwrap_or_else(|| panic!("no note titled {title}")).id
    }

    fn open(&mut self, title: &str) -> Entity<NoteView> {
        let id = self.id_of(title);
        let main = self.main.clone();
        self.cx.update(|window, cx| {
            main.update(cx, |main, cx| {
                main.handle(
                    Request::OpenNote {
                        id: id.to_string(),
                        query: String::new(),
                    },
                    window,
                    cx,
                );
            });
        });
        self.cx.run_until_parked();
        self.note()
    }

    fn note(&mut self) -> Entity<NoteView> {
        self.main
            .read_with(self.cx, |main, _| main.note().cloned())
            .expect("a note is open")
    }

    fn note_do(
        &mut self,
        run: impl FnOnce(&mut NoteView, &mut gpui::Window, &mut gpui::Context<NoteView>),
    ) {
        let note = self.note();
        self.cx
            .update(|window, cx| note.update(cx, |note, cx| run(note, window, cx)));
        self.cx.run_until_parked();
    }

    fn editor_text(&mut self) -> String {
        let note = self.note();
        note.read_with(self.cx, |note, cx| note.editor().read(cx).markdown())
    }

    /// The toast shown, with the label of its button.
    fn toast(&mut self) -> Option<(String, Option<String>)> {
        self.main.read_with(self.cx, |main, cx| {
            main.toasts().read(cx).shown().map(|toast| {
                (
                    toast.message.to_string(),
                    toast.action_label().map(ToString::to_string),
                )
            })
        })
    }

    fn save_to(&self, path: PathBuf) {
        self.dialogs.saves.lock().unwrap().push_back(path);
    }
}

// ---- Export --------------------------------------------------------------------------

#[gpui::test]
async fn export_as_pdf_writes_the_chosen_file(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Ausschreibung – offene Fragen");
    let target = notes.scratch("Ausschreibung.pdf");
    app.save_to(target.clone());

    app.note_do(|note, _, cx| note.export_pdf(cx));

    let pdf = std::fs::read(&target).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(
        app.toast(),
        Some(("Exported".to_owned(), Some("Open".to_owned())))
    );
    assert_eq!(
        *app.dialogs.asked.lock().unwrap(),
        ["Export as PDF".to_owned()]
    );
}

#[gpui::test]
async fn export_as_markdown_leaves_out_the_frontmatter(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    let target = notes.scratch("Einkauf.md");
    app.save_to(target.clone());

    app.note_do(|note, _, cx| note.export_markdown(cx));

    let copy = std::fs::read_to_string(&target).unwrap();
    assert!(copy.starts_with("Einkauf\n\n- [ ] Milch"), "{copy}");
    assert!(!copy.contains("id:"));
}

#[gpui::test]
async fn a_cancelled_export_writes_and_says_nothing(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.note_do(|note, _, cx| note.export_pdf(cx));
    app.note_do(|note, _, cx| note.print(cx));
    assert_eq!(app.toast(), None);
    assert!(app.dialogs.printed.lock().unwrap().is_empty());
}

#[gpui::test]
async fn an_export_that_fails_says_why(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.save_to(notes.scratch("missing/folder/Einkauf.pdf"));
    app.note_do(|note, _, cx| note.export_pdf(cx));
    let (message, button) = app.toast().unwrap();
    assert!(message.starts_with("Could not export: "), "{message}");
    assert_eq!(button, None);
}

#[gpui::test]
async fn print_renders_for_the_chosen_paper(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Reise nach Lissabon");
    *app.dialogs.paper.lock().unwrap() = Some(sheaf_export::PageSetup {
        width: 148.,
        height: 210.,
        ..sheaf_export::PageSetup::A4
    });
    app.note_do(|note, _, cx| note.print(cx));
    let printed = app.dialogs.printed.lock().unwrap();
    assert_eq!(printed.len(), 1);
    let text = String::from_utf8_lossy(&printed[0]);
    // A5 in points: 419.53 × 595.28.
    assert!(text.contains("/MediaBox[0 0 419.52"), "an A5 page");
}

// ---- Attachments ---------------------------------------------------------------------

#[gpui::test]
async fn a_dropped_file_is_copied_and_linked(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let photo = notes.scratch("Mein Foto.png");
    std::fs::write(&photo, b"png").unwrap();
    let report = notes.scratch("Bericht.pdf");
    std::fs::write(&report, b"pdf").unwrap();

    app.note_do(|note, window, cx| {
        note.attach(vec![Upload::File(photo), Upload::File(report)], window, cx);
    });

    let id = "01JD3K8Z6Q4X7V9B2M5N8P0R03";
    let folder = notes.dir.join("Arbeit/_attachments").join(id);
    assert_eq!(std::fs::read(folder.join("Mein-Foto.png")).unwrap(), b"png");
    assert_eq!(std::fs::read(folder.join("Bericht.pdf")).unwrap(), b"pdf");
    let text = app.editor_text();
    assert!(
        text.contains(&format!(
            "![Mein Foto.png](_attachments/{id}/Mein-Foto.png)"
        )),
        "{text}"
    );
    assert!(text.contains(&format!("[Bericht.pdf](_attachments/{id}/Bericht.pdf)")));
    app.cx
        .executor()
        .advance_clock(sheaf::app::note::SAVE_DELAY);
    app.cx.run_until_parked();
    assert!(
        notes
            .read("Arbeit/Notch-Ideen.md")
            .contains("Mein-Foto.png")
    );
}

#[gpui::test]
async fn attach_file_takes_the_chosen_files_and_gives_a_new_note_its_id(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Reading list");
    let file = notes.scratch("liste.csv");
    std::fs::write(&file, b"a;b").unwrap();
    app.dialogs.opens.lock().unwrap().push_back(vec![file]);

    app.note_do(|note, window, cx| note.attach_file(window, cx));

    let id = app.note().read_with(app.cx, |note, _| note.id().clone());
    assert!(!id.is_provisional(), "the note has its real id now");
    assert!(
        notes
            .dir
            .join("_attachments")
            .join(id.as_str())
            .join("liste.csv")
            .exists()
    );
    assert!(
        app.editor_text()
            .contains(&format!("[liste.csv](_attachments/{id}/liste.csv)"))
    );
    app.cx
        .executor()
        .advance_clock(sheaf::app::note::SAVE_DELAY);
    app.cx.run_until_parked();
    let file = notes.read("Reading list.md");
    assert!(
        file.contains("liste.csv"),
        "saved without a conflict: {file}"
    );
}

#[gpui::test]
async fn a_file_that_cannot_be_read_is_reported(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.note_do(|note, window, cx| {
        note.attach(
            vec![Upload::File(notes.scratch("gibt es nicht.png"))],
            window,
            cx,
        );
    });
    let (message, _) = app.toast().unwrap();
    assert!(message.starts_with("Could not attach: "), "{message}");
    assert!(!app.editor_text().contains("_attachments"));
}

// ---- Backlinks -----------------------------------------------------------------------

fn backlink_titles(app: &mut TestApp) -> Vec<String> {
    let note = app.note();
    note.read_with(app.cx, |note, _| {
        note.backlinks()
            .iter()
            .map(|note| note.title.clone())
            .collect()
    })
}

#[gpui::test]
async fn backlinks_list_the_notes_that_link_here_and_open_them(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Project plan");
    assert_eq!(backlink_titles(&mut app), ["Meeting notes 2026-11-03"]);
    let open = app
        .note()
        .read_with(app.cx, |note, _| note.backlinks_open());
    assert!(!open, "folded at first");

    // Another note starts linking here: the list follows.
    let created = app.library.update(app.cx, |library, cx| {
        library.create(
            "Kickoff\n\nSiehe [[project plan]].\n",
            sheaf_core::CreateOptions::default(),
            cx,
        )
    });
    app.cx.run_until_parked();
    created.await.unwrap();
    app.cx.run_until_parked();
    assert_eq!(
        backlink_titles(&mut app),
        ["Kickoff", "Meeting notes 2026-11-03"]
    );

    app.note_do(|note, _, cx| note.toggle_backlinks(cx));
    app.note_do(|note, _, cx| note.open_backlink(1, cx));
    let title = app
        .note()
        .read_with(app.cx, |note, _| note.meta().map(|meta| meta.title.clone()));
    assert_eq!(title.as_deref(), Some("Meeting notes 2026-11-03"));
}

#[gpui::test]
async fn a_note_nobody_links_to_has_no_backlinks(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    assert!(backlink_titles(&mut app).is_empty());
}

// ---- Settings ------------------------------------------------------------------------

use sheaf::app::settings::view::SettingsView;
use sheaf::app::settings::{self, Settings};
use sheaf_core::config::{EditorFont, LineWidth, Sort as Order};

impl TestApp {
    /// Opens the settings window with Ctrl+, and returns its view and window.
    fn open_settings(&mut self) -> (Entity<SettingsView>, gpui::AnyWindowHandle) {
        self.cx.dispatch_action(sheaf::app::actions::OpenSettings);
        self.cx.run_until_parked();
        let view = self
            .main
            .read_with(self.cx, |main, _| main.settings_window().cloned())
            .expect("the settings window is open");
        let window = self
            .cx
            .update(|_, cx| {
                cx.windows().into_iter().find(|window| {
                    window
                        .downcast::<gpui_component::Root>()
                        .and_then(|root| root.read(cx).ok())
                        .is_some_and(|root| root.view().entity_id() == view.entity_id())
                })
            })
            .expect("its window");
        (view, window)
    }

    fn settings_do(
        &mut self,
        view: &Entity<SettingsView>,
        window: gpui::AnyWindowHandle,
        run: impl FnOnce(&mut SettingsView, &mut gpui::Window, &mut gpui::Context<SettingsView>),
    ) {
        let view = view.clone();
        self.cx
            .update(|_, cx| {
                window.update(cx, |_, window, cx| {
                    view.update(cx, |view, cx| run(view, window, cx))
                })
            })
            .unwrap();
        self.cx.run_until_parked();
    }
}

fn with_settings(notes: &Notes, cx: &mut TestAppContext) -> std::path::PathBuf {
    let file = notes.scratch("config/sheaf/config.toml");
    let settings = Settings::new(Some(file.clone()), sheaf_core::AppConfig::default());
    cx.update(|cx| cx.set_global(settings));
    file
}

#[gpui::test]
async fn editor_settings_apply_to_the_open_note_and_are_stored(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let file = with_settings(&notes, cx);
    let mut app = open_app(&notes, cx);
    let note = app.open("Notch-Ideen");
    let (view, window) = app.open_settings();
    let padding = |app: &mut TestApp| {
        note.read_with(app.cx, |note, cx| {
            note.editor().read(cx).style().padding.left
        })
    };
    let medium = padding(&mut app);

    app.settings_do(&view, window, |view, window, cx| {
        view.set_line_width(LineWidth::Narrow, window, cx);
        view.set_font(EditorFont::Mono, window, cx);
    });
    app.cx.update(|window, _| window.refresh());
    app.cx.run_until_parked();

    assert!(padding(&mut app) > medium, "a narrower column");
    let (family, expected) = app.cx.update(|_, cx| {
        let family = note.read(cx).editor().read(cx).style().font_family.clone();
        (family, settings::font_family(EditorFont::Mono, cx))
    });
    assert_eq!(family, expected);
    let stored = std::fs::read_to_string(&file).unwrap();
    assert!(stored.contains("font = \"mono\""), "{stored}");
    assert!(stored.contains("line_width = \"narrow\""), "{stored}");
}

#[gpui::test]
async fn the_default_order_reaches_the_gallery(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    with_settings(&notes, cx);
    let mut app = open_app(&notes, cx);
    let (view, window) = app.open_settings();
    app.settings_do(&view, window, |view, window, cx| {
        view.set_sort(Order::Title, window, cx)
    });
    let (order, titles) = app.main.read_with(app.cx, |main, cx| {
        let gallery = main.gallery().read(cx);
        let titles: Vec<String> = gallery
            .notes()
            .iter()
            .map(|note| note.title.clone())
            .collect();
        (gallery.sort(), titles)
    });
    assert_eq!(order, Order::Title);
    let unpinned = &titles[2..];
    let mut sorted = unpinned.to_vec();
    sorted.sort_by_key(|title| title.to_lowercase());
    assert_eq!(unpinned, sorted.as_slice());
}

#[gpui::test]
async fn quick_note_settings_go_to_the_notes_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let (view, window) = app.open_settings();
    app.settings_do(&view, window, |view, window, cx| {
        view.set_resume_minutes(15, window, cx);
        view.set_quick_folder("Eingang", window, cx);
    });
    let config = sheaf_core::FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.quick_note.resume_minutes, 15);
    assert_eq!(config.quick_note.folder.as_deref(), Some("Eingang"));
    assert_eq!(
        config.pinned_tags,
        ["ausschreibung", "rezepte"],
        "the rest stays"
    );
}

#[gpui::test]
async fn a_new_notes_folder_is_opened_at_once(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let file = with_settings(&notes, cx);
    let other = Notes::empty();
    other.write("Anderswo.md", "Anderswo\n\nIm neuen Ordner.\n");
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.dialogs
        .folders
        .lock()
        .unwrap()
        .push_back(other.dir.clone());
    let (view, window) = app.open_settings();

    app.settings_do(&view, window, |view, window, cx| {
        view.choose_notes_folder(window, cx)
    });
    app.cx.run_until_parked();

    let (dir, route, titles) = app.main.read_with(app.cx, |main, cx| {
        let titles: Vec<String> = main
            .gallery()
            .read(cx)
            .notes()
            .iter()
            .map(|note| note.title.clone())
            .collect();
        (
            main.library().read(cx).notes_dir().to_owned(),
            main.route(cx),
            titles,
        )
    });
    assert_eq!(dir, other.dir);
    assert_eq!(
        route,
        sheaf::app::window::Route::Gallery,
        "the note was left"
    );
    assert_eq!(titles, ["Anderswo"]);
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("notes_dir")
    );
}
