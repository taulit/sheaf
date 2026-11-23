//! Renders the main window offscreen and writes PNGs to `docs/screenshots/`.
//!
//! No window is opened: GPUI draws into a texture with its headless renderer. The
//! tests are ignored by default because they need a GPU (or a software Vulkan driver)
//! and overwrite files in the repository. Run them with `scripts/screenshot.sh`.
//!
//! # Adding a scene
//!
//! Write a function that brings the window into the state to show and add it to
//! [`SCENES`] with the notes folder it starts from. A scene gets the main window's view
//! and the offscreen app; it runs for the dark and the light theme and ends up as
//! `docs/screenshots/app-<name>-<theme>.png`.
//!
//! An offscreen window reports server-side decorations (GPUI 0.3.12 has no way to ask
//! for others), so gpui-component draws no frame. The harness puts the window on a
//! wallpaper and draws the frame `WindowBorder` gives a client-decorated window:
//! rounded corners, hairline and shadow.

mod fixture;

use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use fixture::{Look, Notes};
use gpui::{
    AnyWindowHandle, AppContext as _, BoxShadow, Context, Entity, EntityInputHandler as _,
    Focusable as _, HeadlessAppContext, IntoElement, ParentElement as _, PlatformTextSystem,
    Render, Styled as _, Window, div, hsla, linear_color_stop, linear_gradient, point, px, rgb,
    size,
};
use gpui_component::{ActiveTheme as _, Root, Theme};
use gpui_markdown_editor::{Command, MarkdownEditor, TextPos};
use sheaf::app::actions::{OpenCommandPalette, ShowShortcuts};
use sheaf::app::assets::Assets;
use sheaf::app::chrome::AssumeClientDecorations;
use sheaf::app::gallery::{Popover as GalleryPopover, Scope};
use sheaf::app::library::{Library, LibraryEvent};
use sheaf::app::note::Popover as NotePopover;
use sheaf::app::quick;
use sheaf::app::theme::size::WINDOW_RADIUS;
use sheaf::app::window::{self, MainWindow};
use sheaf_core::Sort;
use sheaf_core::config::View;

/// The default size of the main window.
const WIDTH: f32 = 1100.;
const HEIGHT: f32 = 720.;
/// Wallpaper around the window, enough for its shadow.
const MARGIN: f32 = 40.;

/// The window on the wallpaper of the prototype, in the frame of a client-decorated
/// window.
struct Desktop {
    root: Entity<Root>,
    dark: bool,
}

impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (from, to) = if self.dark {
            (0x0b1020, 0x1b1433)
        } else {
            (0xd9e2f1, 0xefe3ee)
        };
        div()
            .size_full()
            .p(px(MARGIN))
            .bg(linear_gradient(
                160.,
                linear_color_stop(rgb(from), 0.),
                linear_color_stop(rgb(to), 1.),
            ))
            .child(
                div()
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .rounded(WINDOW_RADIUS)
                    .border_1()
                    .border_color(cx.theme().window_border)
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.35),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(30.),
                        spread_radius: px(-6.),
                        inset: false,
                    }])
                    .child(self.root.clone()),
            )
    }
}

/// What a scene works with.
struct Stage {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    main: Entity<MainWindow>,
    library: Entity<Library>,
}

impl Stage {
    /// Runs `update` on the main window's view, then everything it set in motion.
    fn update(
        &mut self,
        update: impl FnOnce(&mut MainWindow, &mut gpui::Window, &mut gpui::Context<MainWindow>),
    ) {
        let main = self.main.clone();
        self.cx
            .update_window(self.window, |_, window, cx| {
                main.update(cx, |main, cx| update(main, window, cx));
            })
            .expect("the window is open");
        self.cx.run_until_parked();
    }
}

type Scene = fn(&mut Stage);

/// The notes folder a scene starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Folder {
    Seeded,
    Empty,
    /// Seeded, and a sync tool's conflict copy of "Einkauf".
    Conflict,
}

/// The conflict copy of [`Folder::Conflict`].
const CONFLICT_COPY: &str = "Privat/Einkauf.sync-conflict-20261116-121500-K4XQ2ZP.md";

const SCENES: &[(&str, Folder, Scene)] = &[
    ("gallery", Folder::Seeded, gallery),
    ("gallery-search", Folder::Seeded, gallery_search),
    ("gallery-list", Folder::Seeded, gallery_list),
    ("gallery-folders", Folder::Seeded, gallery_folders),
    ("gallery-empty", Folder::Empty, gallery),
    ("note", Folder::Seeded, note),
    ("note-menu", Folder::Seeded, note_menu),
    ("note-conflict", Folder::Seeded, note_conflict),
    ("note-save-error", Folder::Seeded, note_save_error),
    ("palette", Folder::Seeded, palette),
    ("shortcuts", Folder::Seeded, shortcuts),
    ("trash", Folder::Seeded, trash),
    ("templates-menu", Folder::Seeded, templates_menu),
    ("note-share", Folder::Seeded, note_share),
    ("note-backlinks", Folder::Seeded, note_backlinks),
    ("note-format-bar", Folder::Seeded, note_format_bar),
    ("note-slash", Folder::Seeded, note_slash),
    ("note-template", Folder::Seeded, note_template),
    ("note-image", Folder::Seeded, note_image),
    ("note-find", Folder::Seeded, note_find),
    ("gallery-conflict", Folder::Conflict, gallery),
    ("note-sync-conflict", Folder::Conflict, note_sync_conflict),
    ("gallery-smart-symbol", Folder::Seeded, gallery_smart_symbol),
    ("gallery-folder-menu", Folder::Seeded, gallery_folder_menu),
    ("gallery-tags", Folder::Seeded, gallery_tags),
];

/// The start view as it opens.
fn gallery(_: &mut Stage) {}

/// A search with a filter chip and highlighted hits.
fn gallery_search(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_query("hat:checkliste für", cx)
        });
    });
}

/// The compact list instead of the cards.
fn gallery_list(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery()
            .update(cx, |gallery, cx| gallery.set_view(View::List, cx));
    });
}

/// The folder popover of the header bar, with two smart folders.
fn gallery_folders(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_query("offen:", cx);
            gallery.save_smart_folder("Offene Aufgaben", cx);
            gallery.set_query("geändert:woche", cx);
            gallery.save_smart_folder("Diese Woche", cx);
            gallery.set_scope(Scope::All, cx);
        });
    });
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_popover(Some(GalleryPopover::Folders), cx)
        });
    });
}

/// The conflict copy open, with the hint under its date.
fn note_sync_conflict(stage: &mut Stage) {
    let id = stage.cx.update(|cx| {
        let gallery = stage.main.read(cx).gallery().read(cx);
        let copy = gallery.notes().iter().find(|note| note.conflict);
        copy.expect("the conflict copy is listed").id.clone()
    });
    stage.update(|main, window, cx| main.open_note(id, String::new(), window, cx));
}

/// The name field of a new smart folder with its symbols, one of them chosen.
fn gallery_smart_symbol(stage: &mut Stage) {
    stage.update(|main, window, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_query("#urlaub", cx);
            gallery.ask_smart_folder_name(window, cx);
            gallery.choose_smart_icon("emoji-travel-symbolic", cx);
        });
    });
}

/// The tags at the end of the folder popover, scrolled to, one pinned by its pin.
fn gallery_tags(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_popover(Some(GalleryPopover::Folders), cx);
            gallery.set_tag_pinned("garten", true, cx);
        });
    });
    stage
        .cx
        .update_window(stage.window, |_, window, cx| window.draw(cx).clear(cx))
        .expect("failed to draw");
    let over_the_lists = point(px(MARGIN + 120.), px(MARGIN + 300.));
    let scroll = gpui::PlatformInput::Scroll(gpui::ScrollEvent {
        position: over_the_lists,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-400.))),
        ..Default::default()
    });
    stage
        .cx
        .update_window(stage.window, |_, window, cx| {
            window.dispatch_event(scroll, cx);
        })
        .expect("the window is open");
    stage.cx.run_until_parked();
}

/// The context menu of a folder in the folder popover.
fn gallery_folder_menu(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_popover(Some(GalleryPopover::Folders), cx)
        });
    });
    stage
        .cx
        .update_window(stage.window, |_, window, cx| window.draw(cx).clear(cx))
        .expect("failed to draw");
    // "Arbeit", the row under "All Notes": the window is inset by the wallpaper, the
    // popover hangs under the header bar, and its rows are 34 px high.
    let at = point(px(MARGIN + 60.), px(MARGIN + 47. - 4. + 7. + 34. + 17.));
    for event in [
        gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
            button: gpui::MouseButton::Right,
            position: at,
            click_count: 1,
            ..Default::default()
        }),
        gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
            button: gpui::MouseButton::Right,
            position: at,
            click_count: 1,
            ..Default::default()
        }),
    ] {
        stage
            .cx
            .update_window(stage.window, |_, window, cx| {
                window.dispatch_event(event, cx);
            })
            .expect("the window is open");
        stage.cx.run_until_parked();
    }
}

/// "Recently Deleted" with three notes deleted on different days.
fn trash(stage: &mut Stage) {
    let library = stage.library.clone();
    let titles = [
        ("Sprint Review 12.11.", 2),
        ("Lesestoff", 9),
        ("Reading list", 23),
    ];
    for (title, days_ago) in titles {
        let id = stage.cx.update(|cx| {
            let gallery = stage.main.read(cx).gallery().read(cx);
            let note = gallery.notes().iter().find(|note| note.title == title);
            note.expect("the fixture has the note").id.clone()
        });
        let trashed = stage.cx.update(|cx| {
            library.update(cx, |library, cx| {
                library.set_clock(Arc::new(move || {
                    let ago = std::time::Duration::from_secs(days_ago * 24 * 60 * 60);
                    fixture::now().checked_sub(ago).expect("a date")
                }));
                library.trash(&id, cx)
            })
        });
        stage.cx.run_until_parked();
        pollster(&stage.cx, trashed);
    }
    stage.cx.update(|cx| {
        library.update(cx, |library, _| library.set_clock(Arc::new(fixture::now)));
    });
    stage.update(|main, _, cx| {
        main.gallery()
            .update(cx, |gallery, cx| gallery.set_scope(Scope::Trash, cx));
    });
}

/// The `+ ▾` menu with two templates.
fn templates_menu(stage: &mut Stage) {
    let dir = stage
        .cx
        .update(|cx| sheaf_core::templates::dir(stage.library.read(cx).notes_dir()));
    std::fs::create_dir_all(&dir).expect("a folder");
    for (name, text) in [
        ("Besprechung", "# {{title}}\n\n{{date}}, {{time}}\n"),
        ("Tagebuch", "{{date}}\n\n"),
    ] {
        std::fs::write(dir.join(format!("{name}.md")), text).expect("a template");
    }
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_popover(Some(GalleryPopover::Templates), cx)
        });
    });
}

/// A note with a checklist and a table, opened from the gallery.
fn note(stage: &mut Stage) {
    let listing = stage.cx.update(|cx| {
        let gallery = stage.main.read(cx).gallery().read(cx);
        gallery
            .notes()
            .iter()
            .find(|note| note.title.starts_with("Ausschreibung"))
            .map(|note| note.id.clone())
    });
    let id = listing.expect("the fixture has the note");
    stage.update(|main, window, cx| main.open_note(id, String::new(), window, cx));
}

/// Opens the note whose title starts with `title`.
fn open_titled(stage: &mut Stage, title: &str) {
    let id = stage.cx.update(|cx| {
        let gallery = stage.main.read(cx).gallery().read(cx);
        gallery
            .notes()
            .iter()
            .find(|note| note.title.starts_with(title))
            .map(|note| note.id.clone())
    });
    let id = id.expect("the fixture has the note");
    stage.update(|main, window, cx| main.open_note(id, String::new(), window, cx));
}

/// Runs `edit` on the open note's editor, with the keyboard in it.
fn in_editor(
    stage: &mut Stage,
    edit: impl FnOnce(&mut MarkdownEditor, &mut gpui::Window, &mut gpui::Context<MarkdownEditor>),
) {
    stage.update(|main, window, cx| {
        let note = main.note().expect("the note is open").clone();
        let editor = note.read(cx).editor().clone();
        editor.focus_handle(cx).focus(window, cx);
        editor.update(cx, |editor, cx| edit(editor, window, cx));
    });
}

/// Selects `from..to` in block `index` of the editor.
fn select(
    editor: &mut MarkdownEditor,
    index: usize,
    from: usize,
    to: usize,
    cx: &mut gpui::Context<MarkdownEditor>,
) {
    let block = editor.document().blocks()[index].id();
    editor.__set_selection(
        TextPos {
            block,
            offset: from,
        },
        TextPos { block, offset: to },
        cx,
    );
}

/// The format bar over a word selected in the text.
fn note_format_bar(stage: &mut Stage) {
    note(stage);
    in_editor(stage, |editor, _, cx| select(editor, 1, 19, 38, cx));
}

/// The slash menu, opened by typing `/` on an empty line.
fn note_slash(stage: &mut Stage) {
    open_titled(stage, "Notch-Ideen");
    in_editor(stage, |editor, _, cx| {
        let last = editor.document().blocks().len() - 1;
        let end = editor.document().blocks()[last].text().len();
        select(editor, last, end, end, cx);
    });
    stage.update(|_, window, cx| {
        let newline = cx
            .build_action("markdown_editor::Newline", None)
            .expect("the editor has the action");
        window.dispatch_action(newline, cx);
    });
    in_editor(stage, |editor, window, cx| {
        editor.replace_text_in_range(None, "/", window, cx);
    });
}

/// The menu of templates that the slash menu's "Template" opens.
fn note_template(stage: &mut Stage) {
    templates_menu(stage);
    stage.update(|main, _, cx| {
        main.gallery()
            .update(cx, |gallery, cx| gallery.set_popover(None, cx));
    });
    open_titled(stage, "Notch-Ideen");
    stage.update(|main, window, cx| {
        let note = main.note().expect("the note is open").clone();
        note.update(cx, |note, cx| {
            note.list_templates(point(px(480.), px(330.)), window, cx);
        });
    });
}

/// A note with an image from its attachments.
fn note_image(stage: &mut Stage) {
    open_titled(stage, "Shakshuka");
}

/// The editor's find bar, with the word that was selected as the query.
fn note_find(stage: &mut Stage) {
    note(stage);
    in_editor(stage, |editor, window, cx| {
        let kosten = editor
            .document()
            .blocks()
            .iter()
            .position(|block| block.text() == "Kosten")
            .expect("the note has the heading");
        select(editor, kosten, 0, 6, cx);
        editor.command(Command::OpenFind, window, cx);
    });
}

/// The note with the menu of its header open.
fn note_menu(stage: &mut Stage) {
    note(stage);
    stage.update(|main, _, cx| {
        let note = main.note().expect("the note is open").clone();
        note.update(cx, |note, cx| note.set_popover(Some(NotePopover::Menu), cx));
    });
}

/// The note after another program changed it while it had unsaved edits.
fn note_conflict(stage: &mut Stage) {
    note(stage);
    let library = stage.library.clone();
    let notes_dir = stage
        .cx
        .update(|cx| library.read(cx).notes_dir().to_owned());
    stage.update(|main, window, cx| {
        let editor = main
            .note()
            .expect("the note is open")
            .read(cx)
            .editor()
            .clone();
        editor.update(cx, |editor, cx| {
            editor.command(Command::InsertText("Neu: ".into()), window, cx);
        });
    });
    let path = notes_dir.join("Arbeit/Ausschreibung – offene Fragen.md");
    let text = std::fs::read_to_string(&path).expect("the fixture has the note");
    std::fs::write(
        &path,
        text.replace("Abgabe am Freitag", "Abgabe am Donnerstag"),
    )
    .expect("the note is writable");
    stage.update(|main, _, cx| {
        let id = main.note().expect("the note is open").read(cx).id().clone();
        library.update(cx, |_, cx| cx.emit(LibraryEvent::Changed { ids: vec![id] }));
    });
}

/// The note after leaving it could not save it: its folder was read-only.
fn note_save_error(stage: &mut Stage) {
    use std::os::unix::fs::PermissionsExt as _;
    note(stage);
    let library = stage.library.clone();
    let folder = stage
        .cx
        .update(|cx| library.read(cx).notes_dir().join("Arbeit"));
    let chmod = |mode| {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode))
            .expect("the folder is ours")
    };
    chmod(0o555);
    stage.update(|main, window, cx| {
        let editor = main
            .note()
            .expect("the note is open")
            .read(cx)
            .editor()
            .clone();
        editor.update(cx, |editor, cx| {
            editor.command(Command::InsertText("Neu: ".into()), window, cx);
        });
    });
    stage.update(|main, window, cx| main.back(window, cx));
    chmod(0o755);
}

/// The note with its share popover open.
fn note_share(stage: &mut Stage) {
    note(stage);
    stage.update(|main, _, cx| {
        let note = main.note().expect("the note is open").clone();
        note.update(cx, |note, cx| {
            note.set_popover(Some(NotePopover::Share), cx)
        });
    });
}

/// A note other notes link to, with "Linked from" unfolded.
fn note_backlinks(stage: &mut Stage) {
    let created = stage.cx.update(|cx| {
        stage.library.update(cx, |library, cx| {
            library.create(
                "Kickoff\n\nAgenda aus dem [[Project plan]].\n",
                sheaf_core::CreateOptions {
                    folder: "Work".into(),
                    ..Default::default()
                },
                cx,
            )
        })
    });
    stage.cx.run_until_parked();
    pollster(&stage.cx, created);
    let id = stage.cx.update(|cx| {
        let gallery = stage.main.read(cx).gallery().read(cx);
        gallery
            .notes()
            .iter()
            .find(|note| note.title == "Project plan")
            .map(|note| note.id.clone())
    });
    let id = id.expect("the fixture has the note");
    stage.update(|main, window, cx| main.open_note(id, String::new(), window, cx));
    stage.update(|main, _, cx| {
        let note = main.note().expect("the note is open").clone();
        note.update(cx, |note, cx| note.toggle_backlinks(cx));
    });
}

/// The command palette over the gallery.
fn palette(stage: &mut Stage) {
    stage.update(|_, window, cx| window.dispatch_action(Box::new(OpenCommandPalette), cx));
}

/// The list of keyboard shortcuts over the gallery.
fn shortcuts(stage: &mut Stage) {
    stage.update(|_, window, cx| window.dispatch_action(Box::new(ShowShortcuts), cx));
}

#[test]
#[ignore = "needs a GPU and writes docs/screenshots, run scripts/screenshot.sh"]
fn app_scenes() {
    for (name, folder, scene) in SCENES {
        for (theme, dark) in [("dark", true), ("light", false)] {
            capture(&format!("app-{name}-{theme}"), dark, *folder, *scene);
        }
    }
}

/// The settings window, at its default size.
#[test]
#[ignore = "needs a GPU and writes docs/screenshots, run scripts/screenshot.sh"]
fn settings_scenes() {
    for (theme, dark) in [("dark", true), ("light", false)] {
        let notes = Notes::seeded();
        let (mut cx, library) = headless(dark, &notes);
        let window = cx
            .open_window(
                size(
                    px(SETTINGS_WIDTH + 2. * MARGIN),
                    px(SETTINGS_HEIGHT + 2. * MARGIN),
                ),
                |window, cx| {
                    let (root, _) = sheaf::app::settings::view::build(library.clone(), window, cx);
                    cx.new(|_| Desktop { root, dark })
                },
            )
            .expect("failed to open the offscreen window");
        cx.run_until_parked();
        draw_and_save(&mut cx, window.into(), &format!("app-settings-{theme}"));
    }
}

/// A Markdown file from outside the notes folder in its own window, with frontmatter
/// that stays out of sight, lines kept as they are written, a checklist and a table;
/// once as it is and once with the reading aid.
#[test]
#[ignore = "needs a GPU and writes docs/screenshots, run scripts/screenshot.sh"]
fn document_scenes() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = folder.path().join("fragen.md");
    std::fs::write(
        &path,
        "---\ntitle: Fragen\n---\n# Fragen an die Projektleitung\n\n\
         Stand: 23.11.2026 · Entwurf. Erst gehen wir die Fragen intern durch.\n\n\
         ## 1. Im Dummy vorhanden, in der Beschreibung nicht\n\n\
         **P-01 ★ Angebotsrechner.** Eigene Hauptseite: schlägt für ein neues Angebot \
         einen Preis vor, berechnet aus ähnlichen abgeschlossenen Aufträgen.\n\
         Annahme: nicht Teil des Pakets, optional am Ende.\n\
         Antwort: kommt direkt mit rein.\n\n\
         **P-02 ★ Berichte als PDF.** Eigene Hauptseite mit Berichtsvorlagen und \
         Versand per Mail nach Zeitplan.\n\
         Annahme: Berichte und Versand sind optional.\n\
         Antwort: Berichte ja, das Format ist noch offen.\n\n\
         ## Offen\n\n- [x] Dummy durchgesehen\n- [ ] Fragen an die Projektleitung\n\n\
         | Teil | Stunden |\n| --- | --- |\n| Rechner | 40 |\n| Berichte | 24 |\n",
    )
    .expect("the file");
    for (name, dark, bionic) in [
        ("app-document-dark", true, false),
        ("app-document-light", false, false),
        ("app-document-bionic-dark", true, true),
    ] {
        let notes = Notes::seeded();
        let (mut cx, _) = headless(dark, &notes);
        cx.update(|cx| {
            let mut config = sheaf_core::AppConfig::default();
            config.editor.bionic_reading = bionic;
            cx.set_global(sheaf::app::settings::Settings::new(None, config));
        });
        let window = cx
            .open_window(
                size(px(WIDTH + 2. * MARGIN), px(HEIGHT + 2. * MARGIN)),
                |window, cx| {
                    let (root, _) = sheaf::app::document::build(path.clone(), window, cx);
                    cx.new(|_| Desktop { root, dark })
                },
            )
            .expect("failed to open the offscreen window");
        cx.run_until_parked();
        draw_and_save(&mut cx, window.into(), name);
    }
}

/// The default size of the settings window.
const SETTINGS_WIDTH: f32 = 640.;
const SETTINGS_HEIGHT: f32 = 720.;

/// An offscreen app set up like the real one, with a Library over `notes`.
fn headless(dark: bool, notes: &Notes) -> (HeadlessAppContext, Entity<Library>) {
    // Every platform instance starts its own worker threads, so the text system is
    // taken from one platform and shared by all screenshots.
    static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();
    let text_system =
        TEXT_SYSTEM.get_or_init(|| gpui_platform::current_platform(true).text_system());
    let mut cx = HeadlessAppContext::with_platform(
        text_system.clone(),
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    let library = cx.update(|cx| {
        fixture::init_app(
            Look {
                dark,
                ..Look::default()
            },
            cx,
        );
        cx.set_global(AssumeClientDecorations);
        // The surface behind the views would fill the rounded corners.
        Theme::update(cx, |theme| theme.background = gpui::transparent_black());
        fixture::open_library(notes, cx)
    });
    (cx, library)
}

/// Renders `scene` on `folder` and writes `docs/screenshots/<name>.png`. The image is
/// twice the window size because the offscreen window has a scale factor of 2.
fn capture(name: &str, dark: bool, folder: Folder, scene: Scene) {
    let notes = match folder {
        Folder::Seeded => Notes::seeded(),
        Folder::Empty => Notes::empty(),
        Folder::Conflict => {
            let notes = Notes::seeded();
            let text = "Einkauf\n\n- [ ] Milch\n- [ ] Brot\n- [x] Kaffeebohnen\n";
            notes.write_at(CONFLICT_COPY, text, 70);
            notes
        }
    };
    let (mut cx, library) = headless(dark, &notes);

    let mut main = None;
    let window = cx
        .open_window(
            size(px(WIDTH + 2. * MARGIN), px(HEIGHT + 2. * MARGIN)),
            |window, cx| {
                let (root, view) = window::build(library.clone(), None, window, cx);
                main = Some(view);
                cx.new(|_| Desktop { root, dark })
            },
        )
        .expect("failed to open the offscreen window");
    cx.run_until_parked();
    let mut stage = Stage {
        cx,
        window: window.into(),
        main: main.expect("the window was built"),
        library: library.clone(),
    };
    // The lists are there before any scene looks at them.
    let listed = stage
        .cx
        .update(|cx| library.read(cx).notes(Vec::new(), Sort::Modified, cx));
    let expected = match folder {
        Folder::Seeded => 14,
        Folder::Empty => 0,
        Folder::Conflict => 15,
    };
    assert_eq!(pollster(&stage.cx, listed).len(), expected);

    scene(&mut stage);
    draw_and_save(&mut stage.cx, stage.window, name);
}

/// Draws `window` until what loads in the background is there and writes it to
/// `docs/screenshots/<name>.png`.
fn draw_and_save(cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) {
    cx.run_until_parked();
    // Miniatures and covers load in the background; the first frame asks for them.
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("failed to draw");
        std::thread::sleep(std::time::Duration::from_millis(100));
        cx.run_until_parked();
    }
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .expect("failed to draw");

    let image = cx
        .capture_screenshot(window)
        .expect("offscreen rendering failed");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/screenshots")
        .join(format!("{name}.png"));
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
    image.save(&path).expect("failed to write the PNG");
    println!("wrote {}", path.display());
}

/// Waits for a task of the offscreen app.
fn pollster<T: 'static>(cx: &HeadlessAppContext, task: gpui::Task<sheaf_core::Result<T>>) -> T {
    cx.foreground_executor()
        .block_test(task)
        .expect("the library answers")
}

#[test]
#[ignore = "needs a GPU and writes docs/screenshots, run scripts/screenshot.sh"]
fn quick_scene() {
    for (theme, dark) in [("dark", true), ("light", false)] {
        capture_quick(&format!("app-quick-{theme}"), dark);
    }
}

/// The Quick Note window with a line typed, on the wallpaper, like [`capture`].
fn capture_quick(name: &str, dark: bool) {
    static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();
    let text_system =
        TEXT_SYSTEM.get_or_init(|| gpui_platform::current_platform(true).text_system());
    let notes = Notes::seeded();
    let mut cx = HeadlessAppContext::with_platform(
        text_system.clone(),
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    let library = cx.update(|cx| {
        fixture::init_app(
            Look {
                dark,
                ..Look::default()
            },
            cx,
        );
        cx.set_global(AssumeClientDecorations);
        Theme::update(cx, |theme| theme.background = gpui::transparent_black());
        fixture::open_library(&notes, cx)
    });
    cx.run_until_parked();
    let mut view = None;
    let window = cx
        .open_window(
            size(
                px(quick::WIDTH + 2. * MARGIN),
                px(quick::HEIGHT + 2. * MARGIN),
            ),
            |window, cx| {
                let (root, quick) = quick::build(library.clone(), Rc::new(|_, _| {}), window, cx);
                view = Some(quick);
                cx.new(|_| Desktop { root, dark })
            },
        )
        .expect("failed to open the offscreen window");
    let view = view.expect("the window was built");
    cx.run_until_parked();
    let window: AnyWindowHandle = window.into();
    cx.update_window(window, |_, window, cx| {
        let note = view.read(cx).note().expect("the note is there").clone();
        let editor = note.read(cx).editor().clone();
        editor.update(cx, |editor, cx| {
            let text = "Idee: Hover-Vorschau zeigt auch die letzte Notiz?";
            editor.command(Command::InsertText(text.into()), window, cx);
        });
    })
    .expect("the window is open");
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .expect("failed to draw");
    let image = cx
        .capture_screenshot(window)
        .expect("offscreen rendering failed");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/screenshots")
        .join(format!("{name}.png"));
    image.save(&path).expect("failed to write the PNG");
    println!("wrote {}", path.display());
}
