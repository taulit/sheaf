//! What the app tests and the screenshots share: a seeded notes folder in a temporary
//! directory, a fixed "now", the app set up the way `app::run` does it, without a
//! bus, a display or the user's files, and a [`TestApp`] to drive it.

// Each test binary uses its own part of these helpers.
#![allow(dead_code, clippy::unwrap_used)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui::{App, Entity, TestAppContext, VisualTestContext, WindowHandle};
use gpui_component::Root;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use sheaf::app::actions;
use sheaf::app::dialogs::{Dialogs, Scripted};
use sheaf::app::gallery::GalleryView;
use sheaf::app::instance::Request;
use sheaf::app::library::Library;
use sheaf::app::note::NoteView;
use sheaf::app::state::StateFile;
use sheaf::app::theme::{self, Appearance, Overrides};
use sheaf::app::window::{self, MainWindow, Route};
use sheaf_core::{NoteId, NoteMeta};
use tempfile::TempDir;

/// The moment the fixture pretends it is: Monday, 16 November 2026, 13:24 in Berlin.
pub const NOW: &str = "2026-11-16T13:24:00+01:00";

pub fn now() -> ZonedDateTime {
    let time: Timestamp = NOW.parse().unwrap();
    time.to_zoned_datetime(TimeZone::fixed(jiff::tz::offset(1)))
}

/// One seeded note: path, last modification relative to [`NOW`] in minutes, text.
struct Seed {
    path: &'static str,
    minutes_ago: u64,
    text: &'static str,
}

const HOUR: u64 = 60;
const DAY: u64 = 24 * HOUR;

/// A notes folder as someone might have it after a few weeks: German and English,
/// pinned notes, checklists, a table, tags, wiki links, an image, nested folders, and
/// one file another editor wrote (no frontmatter).
const SEEDS: &[Seed] = &[
    Seed {
        path: "Privat/Einkauf.md",
        minutes_ago: 2 * HOUR + 22,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R01\ncreated: 2026-11-10T08:00:00+01:00\npinned: true\n---\n\
Einkauf\n\n- [ ] Milch\n- [ ] Geschenk für Jukek\n- [x] Kaffeebohnen\n- [ ] Backpapier\n",
    },
    Seed {
        path: "Arbeit/Ausschreibung – offene Fragen.md",
        minutes_ago: 3 * HOUR + 44,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R02\ncreated: 2026-11-09T09:40:00+01:00\npinned: true\n---\n\
Ausschreibung – offene Fragen\n\nAbgabe am Freitag. Stand nach dem Call mit Frau K.:\n\n## Offen\n\n\
- [ ] Kostenschätzung finalisieren\n- [ ] Kapitel 4 gegenlesen lassen\n- [x] Referenzen anfragen\n\n## Kosten\n\n\
| Posten | Betrag |\n|---|---:|\n| Konzept | 4.800 € |\n| Umsetzung | 18.200 € |\n| Betrieb (12 Mon.) | 3.600 € |\n\n\
#ausschreibung\n",
    },
    Seed {
        path: "Arbeit/Notch-Ideen.md",
        minutes_ago: DAY + 3 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R03\ncreated: 2026-11-08T17:05:00+01:00\n---\n\
Notch-Ideen\n\nWas die Notch können soll, ohne zu überladen:\n\n- Hover deutet die Notch an\n\
- Nur zeigen, was gerade läuft\n- Module: Jetzt, Ausblick oder Dock\n\n\
Notizen als eigene App, siehe [[Notch-Konzept]].\n",
    },
    Seed {
        path: "Privat/Geschenkideen Jukek.md",
        minutes_ago: 3 * DAY + 2 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R04\ncreated: 2026-11-05T19:30:00+01:00\n---\n\
Geschenkideen Jukek\n\n![Skizze](_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R04/skizze.png)\n\n\
- Konzertkarten\n- Gutes Buch über Architektur\n- Siebträger-Kurs\n",
    },
    Seed {
        path: "Privat/Rezepte/Shakshuka.md",
        minutes_ago: 4 * DAY + HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R05\ncreated: 2026-10-28T18:10:00+01:00\n---\n\
Shakshuka\n\n![Shakshuka](_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R05/pfanne.png)\n\nFür 2 Personen, 25 Minuten.\n\n- [ ] 2 Paprika, 1 Zwiebel\n- [ ] 1 Dose Tomaten\n\
- [ ] 4 Eier, Feta, Kreuzkümmel\n\n#rezepte\n",
    },
    Seed {
        path: "Arbeit/Sprint Review 12.11..md",
        minutes_ago: 4 * DAY + 5 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R06\ncreated: 2026-11-12T15:00:00+01:00\n---\n\
Sprint Review 12.11.\n\nDemo lief gut. Feedback:\n\n- Ladezeiten im Dashboard\n- Export als CSV gewünscht\n\n\
Nächster Termin Do 15:00.\n",
    },
    Seed {
        path: "Privat/Lesestoff.md",
        minutes_ago: 8 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R07\ncreated: 2026-11-01T21:15:00+01:00\n---\n\
Lesestoff\n\n![](_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R07/cover.png)\n\n[GNOME Human Interface Guidelines](https://developer.gnome.org/hig/)\n\n\
[libadwaita: Adaptive Layouts](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/adaptive-layouts.html)\n\n\
„The Design of Everyday Things“, Kapitel 3\n",
    },
    Seed {
        path: "Work/Project plan.md",
        minutes_ago: 9 * DAY + 4 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R08\ncreated: 2026-10-30T10:00:00+01:00\n---\n\
# Project plan\n\nKickoff next week. Owner: ==Maya==. #project\n\n## Milestones\n\n\
1. Write the brief\n2. Book the venue\n3. Send invitations\n\n- [x] Budget approved\n- [ ] Agenda drafted\n",
    },
    Seed {
        path: "Work/Meeting notes 2026-11-03.md",
        minutes_ago: 13 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R09\ncreated: 2026-11-03T14:00:00+01:00\n---\n\
Meeting notes 2026-11-03\n\n> Ship the smallest thing that is useful.\n\n\
Decisions: weekly demo on Thursdays, see [[Project plan]].\n\n```sh\ncargo test --workspace\n```\n",
    },
    Seed {
        path: "Reading list.md",
        minutes_ago: 15 * DAY,
        text: "Reading list\n\nThe Overstory and Braiding Sweetgrass. #books\n",
    },
    Seed {
        path: "Ideen für den Garten.md",
        minutes_ago: 20 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R11\ncreated: 2026-10-20T11:00:00+02:00\ntags: [garten]\n---\n\
Ideen für den Garten\n\nTomaten und Kräuter pflanzen. Hochbeet an die Südseite.\n",
    },
    Seed {
        path: "Privat/Reise nach Lissabon.md",
        minutes_ago: 27 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R12\ncreated: 2026-10-12T12:00:00+02:00\n---\n\
Reise nach Lissabon\n\n| Tag | Plan |\n|---|---|\n| Fr | Ankunft, Alfama |\n| Sa | Belém, Pastéis |\n| So | Sintra |\n\n\
- [ ] Flüge buchen\n- [ ] Unterkunft\n\n#urlaub\n",
    },
    Seed {
        path: "Schnellnotizen/Idee Hover-Vorschau.md",
        minutes_ago: 40 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R13\ncreated: 2026-10-07T08:45:00+02:00\n---\n\
Idee: Hover-Vorschau zeigt auch die letzte Notiz?\n",
    },
    Seed {
        path: "Archive/Tax return 2025.md",
        minutes_ago: 120 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R14\ncreated: 2026-05-02T09:00:00+02:00\n---\n\
Tax return 2025\n\nSubmitted on 2 May. Reference number in the letter from the tax office.\n\n\
- [x] Collect receipts\n- [x] Submit\n",
    },
];

/// Covers like the prototype's gradients, at the paths the seeded notes link to.
const COVERS: &[(&str, &[u8])] = &[
    (
        "Privat/_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R04/skizze.png",
        include_bytes!("covers/purple.png"),
    ),
    (
        "Privat/Rezepte/_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R05/pfanne.png",
        include_bytes!("covers/orange.png"),
    ),
    (
        "Privat/_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R07/cover.png",
        include_bytes!("covers/green.png"),
    ),
];

/// What the folder remembers: the tags shown as chips, as in the prototype.
const FOLDER_CONFIG: &str = "pinned_tags = [\"ausschreibung\", \"rezepte\"]\n";

/// A temporary notes folder, removed when dropped.
pub struct Notes {
    root: TempDir,
    pub dir: PathBuf,
}

impl Notes {
    pub fn empty() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("notes");
        fs::create_dir(&dir).unwrap();
        Notes { root, dir }
    }

    /// The folder with the notes of [`SEEDS`], last modified at fixed times.
    pub fn seeded() -> Self {
        let notes = Self::empty();
        for seed in SEEDS {
            notes.write_at(seed.path, seed.text, seed.minutes_ago);
        }
        for (path, bytes) in COVERS {
            fs::create_dir_all(notes.dir.join(path).parent().unwrap()).unwrap();
            fs::write(notes.dir.join(path), bytes).unwrap();
        }
        notes.write(".sheaf/config.toml", FOLDER_CONFIG);
        notes
    }

    pub fn write(&self, rel_path: &str, text: &str) {
        let path = self.dir.join(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Writes a file last modified `minutes_ago` before [`NOW`].
    pub fn write_at(&self, rel_path: &str, text: &str, minutes_ago: u64) {
        self.write(rel_path, text);
        let now: Timestamp = NOW.parse().unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(now.as_second().unsigned_abs());
        let file = File::options()
            .write(true)
            .open(self.dir.join(rel_path))
            .unwrap();
        file.set_modified(now - Duration::from_secs(minutes_ago * 60))
            .unwrap();
    }

    pub fn read(&self, rel_path: &str) -> String {
        fs::read_to_string(self.dir.join(rel_path)).unwrap()
    }

    /// Paths of all note files, relative to the folder, sorted.
    pub fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect(&self.dir, &self.dir, &mut found);
        found.sort();
        found
    }

    /// A place for files that are not notes, such as the state file.
    pub fn scratch(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn collect(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with(['.', '_']) {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, found);
        } else if name.ends_with(".md") {
            found.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

/// How a test wants the app to look.
#[derive(Clone, Copy)]
pub struct Look {
    pub dark: bool,
    pub accent: u32,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            dark: true,
            accent: 0x3584e4,
        }
    }
}

/// Sets the app up as `app::run` does, with a fixed look and nothing moving, so that
/// tests need no clock and screenshots show the end of every animation.
pub fn init_app(look: Look, cx: &mut App) {
    gpui_component::init(cx);
    let overrides = Overrides {
        dark: Some(look.dark),
        accent: Some(look.accent),
    };
    theme::init(overrides, cx);
    theme::set_appearance(
        Appearance {
            enable_animations: false,
            ..Appearance::default()
        },
        cx,
    );
    actions::bind_keys(cx);
}

/// A Library over `notes` that never looks for the service and lives at [`NOW`].
pub fn open_library(notes: &Notes, cx: &mut App) -> Entity<Library> {
    let library = Library::local(&notes.dir, cx).unwrap();
    library.update(cx, |library, _| library.set_clock(Arc::new(now)));
    library
}

/// The app with its main window on a notes folder, in a test window, for the tests
/// to drive. The dialogs answer as [`TestApp::dialogs`] is told; none ever opens.
pub struct TestApp {
    pub library: Entity<Library>,
    pub main: Entity<MainWindow>,
    pub window: WindowHandle<Root>,
    pub dialogs: Arc<Scripted>,
    pub cx: &'static mut VisualTestContext,
}

pub fn open_app(notes: &Notes, cx: &mut TestAppContext) -> TestApp {
    open_app_with_state(notes, None, cx)
}

/// [`open_app`] with the window size remembered in `state_file`.
pub fn open_app_with_state(
    notes: &Notes,
    state_file: Option<StateFile>,
    cx: &mut TestAppContext,
) -> TestApp {
    let library = cx.update(|cx| {
        init_app(Look::default(), cx);
        open_library(notes, cx)
    });
    open_window(library, state_file, cx)
}

/// A main window on `library`, set up already.
pub fn open_window(
    library: Entity<Library>,
    state_file: Option<StateFile>,
    cx: &mut TestAppContext,
) -> TestApp {
    let dialogs = Arc::new(Scripted::default());
    cx.update(|cx| cx.set_global(Dialogs::new(dialogs.clone())));
    let (window, main) = cx
        .update(|cx| window::open(library.clone(), state_file, cx))
        .unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    let mut app = TestApp {
        library,
        main,
        window,
        dialogs,
        cx,
    };
    app.draw();
    app
}

impl TestApp {
    pub fn gallery(&mut self) -> Entity<GalleryView> {
        self.main
            .read_with(self.cx, |main, _| main.gallery().clone())
    }

    pub fn read<R>(&mut self, read: impl FnOnce(&GalleryView) -> R) -> R {
        let gallery = self.gallery();
        gallery.read_with(self.cx, |gallery, _| read(gallery))
    }

    /// Changes the gallery, then lets it settle and draws.
    pub fn update(
        &mut self,
        update: impl FnOnce(&mut GalleryView, &mut gpui::Context<GalleryView>),
    ) {
        let gallery = self.gallery();
        gallery.update(self.cx, update);
        self.settle();
    }

    /// Lets tasks and the search's pause run out, then draws a frame.
    pub fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx.executor().advance_clock(Duration::from_millis(50));
        self.cx.run_until_parked();
        self.draw();
    }

    pub fn draw(&mut self) {
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        self.cx.run_until_parked();
    }

    pub fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.settle();
    }

    pub fn route(&mut self) -> Route {
        self.main.read_with(self.cx, |main, cx| main.route(cx))
    }

    /// The notes the gallery lists, in order.
    pub fn listed(&mut self) -> Vec<NoteMeta> {
        self.read(|gallery| gallery.notes().to_vec())
    }

    pub fn titles(&mut self) -> Vec<String> {
        self.listed().into_iter().map(|note| note.title).collect()
    }

    pub fn id_of(&mut self, title: &str) -> NoteId {
        let note = self.listed().into_iter().find(|note| note.title == title);
        note.unwrap_or_else(|| panic!("no note titled {title}")).id
    }

    pub fn request(&mut self, request: Request) {
        let main = self.main.clone();
        self.cx.update(|window, cx| {
            main.update(cx, |main, cx| main.handle(request, window, cx));
        });
        self.cx.run_until_parked();
    }

    /// Opens the note titled `title` as the service would ask for it.
    pub fn open(&mut self, title: &str) -> Entity<NoteView> {
        let id = self.id_of(title);
        self.request(Request::OpenNote {
            id: id.to_string(),
            query: String::new(),
        });
        self.note()
    }

    pub fn note(&mut self) -> Entity<NoteView> {
        self.main
            .read_with(self.cx, |main, _| main.note().cloned())
            .expect("a note is open")
    }

    pub fn note_do(
        &mut self,
        run: impl FnOnce(&mut NoteView, &mut gpui::Window, &mut gpui::Context<NoteView>),
    ) {
        let note = self.note();
        self.cx
            .update(|window, cx| note.update(cx, |note, cx| run(note, window, cx)));
        self.cx.run_until_parked();
    }

    /// The Markdown in the editor of the open note.
    pub fn editor_text(&mut self) -> String {
        let note = self.note();
        note.read_with(self.cx, |note, cx| note.editor().read(cx).markdown())
    }

    pub fn toast(&mut self) -> Option<String> {
        self.toast_with_action().map(|(message, _)| message)
    }

    /// The toast shown, with the label of its button.
    pub fn toast_with_action(&mut self) -> Option<(String, Option<String>)> {
        self.main.read_with(self.cx, |main, cx| {
            main.toasts().read(cx).shown().map(|toast| {
                (
                    toast.message.to_string(),
                    toast.action_label().map(ToString::to_string),
                )
            })
        })
    }
}
