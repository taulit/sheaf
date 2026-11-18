//! Export, printing and attachments of the note view, backlinks and the settings, in a
//! test window. Dialogs are scripted: no portal is asked, no dialog opens.

#![allow(clippy::unwrap_used)]

mod fixture;

use std::io::Read as _;
use std::path::PathBuf;

use fixture::{Notes, TestApp, open_app};
use gpui::{Entity, TestAppContext};
use sheaf::app::note::Upload;
use sheaf::app::note::export::Format;
use sheaf_core::NoteId;

impl TestApp {
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

    app.note_do(|note, _, cx| note.export(Format::Pdf, cx));

    let pdf = std::fs::read(&target).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(
        app.toast_with_action(),
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

    app.note_do(|note, _, cx| note.export(Format::Markdown, cx));

    let copy = std::fs::read_to_string(&target).unwrap();
    assert!(copy.starts_with("Einkauf\n\n- [ ] Milch"), "{copy}");
    assert!(!copy.contains("id:"));
}

#[gpui::test]
async fn a_cancelled_export_writes_and_says_nothing(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.note_do(|note, _, cx| note.export(Format::Pdf, cx));
    app.note_do(|note, _, cx| note.print(cx));
    assert_eq!(app.toast_with_action(), None);
    assert!(app.dialogs.printed.lock().unwrap().is_empty());
}

#[gpui::test]
async fn an_export_that_fails_says_why(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Einkauf");
    app.save_to(notes.scratch("missing/folder/Einkauf.pdf"));
    app.note_do(|note, _, cx| note.export(Format::Pdf, cx));
    let (message, button) = app.toast_with_action().unwrap();
    assert!(message.starts_with("Could not export: "), "{message}");
    assert_eq!(button, None);
}

#[gpui::test]
async fn the_gallery_exports_one_note_to_a_file_and_several_to_a_folder(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let gallery = app.main.read_with(app.cx, |main, _| main.gallery().clone());
    let index_of = |app: &mut TestApp, title: &str| {
        gallery.read_with(app.cx, |gallery, _| {
            gallery
                .notes()
                .iter()
                .position(|note| note.title == title)
                .unwrap()
        })
    };
    let einkauf = index_of(&mut app, "Einkauf");
    gallery.update(app.cx, |gallery, cx| {
        gallery.click(einkauf, gpui::Modifiers::default(), cx);
    });
    let target = notes.scratch("Einkauf.pdf");
    app.save_to(target.clone());
    gallery.update(app.cx, |gallery, cx| {
        gallery.export_selection(Format::Pdf, cx)
    });
    app.cx.run_until_parked();
    assert!(std::fs::read(&target).unwrap().starts_with(b"%PDF-"));
    assert_eq!(
        app.toast_with_action(),
        Some(("Exported".to_owned(), Some("Open".to_owned())))
    );

    let shakshuka = index_of(&mut app, "Shakshuka");
    gallery.update(app.cx, |gallery, cx| {
        gallery.click(shakshuka, gpui::Modifiers::control(), cx);
    });
    let folder = notes.scratch("export");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Einkauf.md"), "already there").unwrap();
    app.dialogs
        .folders
        .lock()
        .unwrap()
        .push_back(folder.clone());
    gallery.update(app.cx, |gallery, cx| {
        gallery.export_selection(Format::Markdown, cx);
    });
    app.cx.run_until_parked();
    let read = |name: &str| std::fs::read_to_string(folder.join(name)).unwrap();
    assert_eq!(read("Einkauf.md"), "already there");
    assert!(read("Einkauf 2.md").starts_with("Einkauf\n\n- [ ] Milch"));
    assert!(read("Shakshuka.md").starts_with("Shakshuka\n"));
    assert_eq!(
        *app.dialogs.asked.lock().unwrap(),
        ["Export as PDF".to_owned(), "Export as Markdown".to_owned()]
    );
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

#[gpui::test]
async fn a_failed_print_offers_the_pdf_in_a_viewer(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Reise nach Lissabon");
    *app.dialogs.print_error.lock().unwrap() = Some("no print portal".to_owned());
    app.note_do(|note, _, cx| note.print(cx));
    assert_eq!(
        app.toast_with_action(),
        Some((
            "Could not print: no print portal".to_owned(),
            Some("Open as PDF".to_owned())
        ))
    );
    assert!(app.dialogs.printed.lock().unwrap().is_empty());
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

/// Files dropped onto the text, as the platform reports them: the editor's own drop
/// handling would link the dropped path instead of a copy.
#[gpui::test]
async fn files_dropped_onto_the_text_are_copied_into_the_attachments(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let photo = notes.scratch("Foto.png");
    std::fs::write(&photo, b"png").unwrap();
    let report = notes.scratch("Bericht.pdf");
    std::fs::write(&report, b"pdf").unwrap();

    let position = gpui::point(gpui::px(500.), gpui::px(400.));
    let paths = gpui::ExternalPaths(vec![photo, report].into());
    app.cx
        .simulate_event(gpui::FileDropEvent::Entered { position, paths });
    app.cx
        .simulate_event(gpui::FileDropEvent::Pending { position });
    app.cx
        .simulate_event(gpui::FileDropEvent::Submit { position });
    app.cx.run_until_parked();

    let id = "01JD3K8Z6Q4X7V9B2M5N8P0R03";
    let folder = notes.dir.join("Arbeit/_attachments").join(id);
    assert_eq!(std::fs::read(folder.join("Foto.png")).unwrap(), b"png");
    assert_eq!(std::fs::read(folder.join("Bericht.pdf")).unwrap(), b"pdf");
    let text = app.editor_text();
    assert!(
        text.contains(&format!("![Foto.png](_attachments/{id}/Foto.png)")),
        "{text}"
    );
    assert!(
        text.contains(&format!("[Bericht.pdf](_attachments/{id}/Bericht.pdf)")),
        "{text}"
    );
    assert!(
        !text.contains(notes.scratch("").to_str().unwrap()),
        "{text}"
    );
}

/// An image on the clipboard, pasted with Ctrl+V in the text.
#[gpui::test]
async fn a_pasted_image_lands_in_the_attachments_and_is_linked(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, b"png".to_vec());
    app.cx
        .update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image)));

    app.cx.simulate_keystrokes("ctrl-v");
    app.cx.run_until_parked();

    let id = "01JD3K8Z6Q4X7V9B2M5N8P0R03";
    let folder = notes.dir.join("Arbeit/_attachments").join(id);
    let stored: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(stored, ["Pasted-Image.png"]);
    assert_eq!(std::fs::read(folder.join(&stored[0])).unwrap(), b"png");
    let text = app.editor_text();
    assert!(
        text.contains(&format!(
            "![Pasted Image.png](_attachments/{id}/Pasted-Image.png)"
        )),
        "{text}"
    );
}

#[gpui::test]
async fn a_second_attach_while_the_first_runs_links_both(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let report = notes.scratch("Bericht.pdf");
    std::fs::write(&report, b"pdf").unwrap();

    app.note_do(|note, window, cx| {
        note.attach(vec![Upload::File(report)], window, cx);
        note.attach(
            vec![Upload::Bytes {
                name: "Bild.png".to_owned(),
                bytes: b"png".to_vec(),
            }],
            window,
            cx,
        );
    });

    let text = app.editor_text();
    let id = "01JD3K8Z6Q4X7V9B2M5N8P0R03";
    assert!(
        text.contains(&format!("[Bericht.pdf](_attachments/{id}/Bericht.pdf)")),
        "{text}"
    );
    assert!(
        text.contains(&format!("![Bild.png](_attachments/{id}/Bild.png)")),
        "{text}"
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
async fn leaving_the_note_gives_up_a_copy_that_runs(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    app.open("Notch-Ideen");
    let big = notes.scratch("gross.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();

    let note = app.note();
    let left = app.cx.update(|window, cx| {
        note.update(cx, |note, cx| {
            note.attach(vec![Upload::File(big)], window, cx);
            note.leave(cx)
        })
    });
    app.cx.run_until_parked();
    if let Some(left) = left {
        assert!(
            app.cx.foreground_executor().block_test(left),
            "left at once"
        );
    }
    let folder = notes
        .dir
        .join("Arbeit/_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R03");
    let left_behind: Vec<_> = std::fs::read_dir(&folder)
        .map(|entries| entries.map(|entry| entry.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(left_behind.is_empty(), "{left_behind:?}");
    assert!(!notes.read("Arbeit/Notch-Ideen.md").contains("gross"));
}

#[test]
fn a_copy_that_is_given_up_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let id: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R03".parse().unwrap();
    let pieces = std::sync::atomic::AtomicUsize::new(0);
    let go_on = || pieces.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2;
    let mut source = std::io::repeat(7).take(10 * 1024 * 1024);
    let given_up =
        sheaf_core::store::store_attachment(dir.path(), &id, "a.bin", &mut source, &go_on);
    assert!(given_up.is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    let link = sheaf_core::store::store_attachment(
        dir.path(),
        &id,
        "a.bin",
        &mut std::io::repeat(7).take(1024 * 1024),
        &|| true,
    )
    .unwrap();
    assert_eq!(link, format!("_attachments/{id}/a.bin"));
    assert_eq!(
        std::fs::metadata(dir.path().join("a.bin")).unwrap().len(),
        1024 * 1024
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
    let (message, _) = app.toast_with_action().unwrap();
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
async fn settings_this_version_cannot_read_survive_a_change(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let file = notes.scratch("config/sheaf/config.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "sort = \"manual\"\nwindow = \"big\"\n").unwrap();
    cx.update(|cx| cx.set_global(Settings::load(file.clone())));

    let saved =
        cx.update(|cx| settings::update(cx, |config| config.editor.font = EditorFont::Mono));
    cx.run_until_parked();
    cx.foreground_executor().block_test(saved).unwrap();

    let stored: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
    assert_eq!(stored["sort"].as_str(), Some("manual"), "{stored}");
    assert_eq!(stored["window"].as_str(), Some("big"), "{stored}");
    assert_eq!(stored["editor"]["font"].as_str(), Some("mono"), "{stored}");
}

#[gpui::test]
async fn settings_that_are_not_toml_are_not_overwritten(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let file = notes.scratch("config/sheaf/config.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "sort = [\n").unwrap();
    cx.update(|cx| cx.set_global(Settings::load(file.clone())));

    let saved =
        cx.update(|cx| settings::update(cx, |config| config.editor.font = EditorFont::Mono));
    cx.run_until_parked();
    let reason = cx.foreground_executor().block_test(saved).unwrap_err();
    assert!(reason.contains("config.toml"), "{reason}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "sort = [\n");
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
async fn a_quick_notes_folder_outside_the_notes_is_refused(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let (view, window) = app.open_settings();
    for folder in ["../x", ".privat", "Arbeit/../..", "/etc", "_attachments"] {
        app.settings_do(&view, window, |view, window, cx| {
            view.set_quick_folder(folder, window, cx);
        });
        let error = view.read_with(app.cx, |view, _| view.quick_folder_error().is_some());
        assert!(error, "{folder}");
        let config = sheaf_core::FolderConfig::load(&notes.dir).unwrap();
        assert_eq!(config.quick_note.folder, None, "{folder}");
    }
    app.settings_do(&view, window, |view, window, cx| {
        view.set_quick_folder("Eingang/Heute", window, cx);
    });
    assert!(view.read_with(app.cx, |view, _| view.quick_folder_error().is_none()));
    let config = sheaf_core::FolderConfig::load(&notes.dir).unwrap();
    assert_eq!(config.quick_note.folder.as_deref(), Some("Eingang/Heute"));
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

// ---- With quick note, trash and templates ---------------------------------------------

use sheaf::app::gallery::Scope;
use sheaf::app::quick;

#[gpui::test]
async fn backlinks_leave_out_notes_in_the_trash(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    let mut app = open_app(&notes, cx);
    let linking = app.id_of("Meeting notes 2026-11-03");
    app.open("Project plan");
    assert_eq!(backlink_titles(&mut app), ["Meeting notes 2026-11-03"]);

    let trashed = app
        .library
        .update(app.cx, |library, cx| library.trash(&linking, cx));
    app.cx.run_until_parked();
    trashed.await.unwrap();
    app.cx.run_until_parked();
    assert!(backlink_titles(&mut app).is_empty());
}

#[gpui::test]
async fn a_note_from_a_template_links_like_any_other(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    notes.write(
        ".sheaf/templates/Verweis.md",
        "Verweis {{date}}\n\nSiehe [[Project plan]].\n",
    );
    let mut app = open_app(&notes, cx);
    let created = app.library.update(app.cx, |library, cx| {
        library.create_from_template("Verweis", sheaf_core::CreateOptions::default(), cx)
    });
    app.cx.run_until_parked();
    created.await.unwrap();
    app.cx.run_until_parked();
    app.open("Project plan");
    assert_eq!(
        backlink_titles(&mut app),
        ["Verweis 2026-11-16", "Meeting notes 2026-11-03"]
    );
}

#[gpui::test]
async fn the_quick_note_follows_the_settings(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    with_settings(&notes, cx);
    let mut app = open_app(&notes, cx);
    let (settings_view, settings_window) = app.open_settings();
    app.settings_do(&settings_view, settings_window, |view, window, cx| {
        view.set_quick_folder("Eingang", window, cx);
        view.set_font(EditorFont::Mono, window, cx);
    });

    let library = app.library.clone();
    let (_, view) = app
        .cx
        .update(|_, cx| quick::open(library, std::rc::Rc::new(|_, _| {}), cx))
        .unwrap();
    app.cx.run_until_parked();
    let font = |app: &mut TestApp| {
        app.cx.update(|_, cx| {
            let note = view.read(cx).note().expect("the note is there").read(cx);
            note.editor().read(cx).style().font_family.clone()
        })
    };
    let folder = view.read_with(app.cx, |view, cx| view.folder(cx));
    assert_eq!(folder, "Eingang");
    let mono = app
        .cx
        .update(|_, cx| settings::font_family(EditorFont::Mono, cx));
    assert_eq!(font(&mut app), mono);

    app.settings_do(&settings_view, settings_window, |view, window, cx| {
        view.set_font(EditorFont::System, window, cx);
    });
    let system = app
        .cx
        .update(|_, cx| settings::font_family(EditorFont::System, cx));
    assert_eq!(font(&mut app), system, "an open quick note follows");
    let size = app.cx.update(|_, cx| {
        let note = view.read(cx).note().unwrap().read(cx);
        note.editor().read(cx).style().title_size
    });
    let text = app.cx.update(|_, cx| {
        let note = view.read(cx).note().unwrap().read(cx);
        note.editor().read(cx).style().text_size
    });
    assert_eq!(size, text, "still the quick note's own style");
}

#[gpui::test]
async fn a_new_default_order_keeps_the_trash_open(cx: &mut TestAppContext) {
    let notes = Notes::seeded();
    with_settings(&notes, cx);
    let mut app = open_app(&notes, cx);
    let gallery = app.main.read_with(app.cx, |main, _| main.gallery().clone());
    gallery.update(app.cx, |gallery, cx| gallery.set_scope(Scope::Trash, cx));
    app.cx.run_until_parked();
    let (view, window) = app.open_settings();
    app.settings_do(&view, window, |view, window, cx| {
        view.set_sort(Order::Title, window, cx)
    });
    let (same, scope) = app.main.read_with(app.cx, |main, cx| {
        (
            *main.gallery() == gallery,
            main.gallery().read(cx).scope().clone(),
        )
    });
    assert!(same, "the gallery is kept");
    assert_eq!(scope, Scope::Trash);
}
