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
use std::sync::{Arc, OnceLock};

use fixture::{Look, Notes};
use gpui::{
    AnyWindowHandle, AppContext as _, BoxShadow, Context, Entity, HeadlessAppContext, IntoElement,
    ParentElement as _, PlatformTextSystem, Render, Styled as _, Window, div, hsla,
    linear_color_stop, linear_gradient, point, px, rgb, size,
};
use gpui_component::{ActiveTheme as _, Root, Theme};
use gpui_markdown_editor::Command;
use sheaf::app::actions::{OpenCommandPalette, ShowShortcuts};
use sheaf::app::assets::Assets;
use sheaf::app::chrome::AssumeClientDecorations;
use sheaf::app::gallery::Popover as GalleryPopover;
use sheaf::app::library::{Library, LibraryEvent};
use sheaf::app::note::Popover as NotePopover;
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
}

const SCENES: &[(&str, Folder, Scene)] = &[
    ("gallery", Folder::Seeded, gallery),
    ("gallery-search", Folder::Seeded, gallery_search),
    ("gallery-list", Folder::Seeded, gallery_list),
    ("gallery-folders", Folder::Seeded, gallery_folders),
    ("gallery-empty", Folder::Empty, gallery),
    ("note", Folder::Seeded, note),
    ("note-menu", Folder::Seeded, note_menu),
    ("note-conflict", Folder::Seeded, note_conflict),
    ("palette", Folder::Seeded, palette),
    ("shortcuts", Folder::Seeded, shortcuts),
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

/// The folder popover of the header bar.
fn gallery_folders(stage: &mut Stage) {
    stage.update(|main, _, cx| {
        main.gallery().update(cx, |gallery, cx| {
            gallery.set_popover(Some(GalleryPopover::Folders), cx)
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

/// Renders `scene` on `folder` and writes `docs/screenshots/<name>.png`. The image is
/// twice the window size because the offscreen window has a scale factor of 2.
fn capture(name: &str, dark: bool, folder: Folder, scene: Scene) {
    // Every platform instance starts its own worker threads, so the text system is
    // taken from one platform and shared by all screenshots.
    static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();
    let text_system =
        TEXT_SYSTEM.get_or_init(|| gpui_platform::current_platform(true).text_system());

    let notes = match folder {
        Folder::Seeded => Notes::seeded(),
        Folder::Empty => Notes::empty(),
    };
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
        fixture::open_library(&notes, cx)
    });

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
    let expected = if folder == Folder::Seeded { 14 } else { 0 };
    assert_eq!(pollster(&stage.cx, listed).len(), expected);

    scene(&mut stage);
    stage.cx.run_until_parked();
    // Miniatures and covers load in the background; the first frame asks for them.
    for _ in 0..3 {
        stage
            .cx
            .update_window(stage.window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("failed to draw");
        std::thread::sleep(std::time::Duration::from_millis(100));
        stage.cx.run_until_parked();
    }
    stage
        .cx
        .update_window(stage.window, |_, window, cx| window.draw(cx).clear(cx))
        .expect("failed to draw");

    let image = stage
        .cx
        .capture_screenshot(stage.window)
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
